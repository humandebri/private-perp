//! vaultが開始した回収を口座単位で直列化する永続フェンス。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceRow {
    pub user_id: [u8; 32],
    pub request_id: Vec<u8>,
    pub epoch: u64,
    pub state: String,
    pub checked_at: Option<u64>,
}

pub fn get(connection: &Connection, account_id: &[u8; 32]) -> Result<Option<FenceRow>, Error> {
    let row = connection.query_optional(
        "SELECT user_id, request_id, epoch, state, checked_at FROM recovery_fences WHERE account_id = ?1",
        params![account_id.as_slice()],
        |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?, row.get::<i64>(2)?, row.get::<String>(3)?, row.get::<Option<i64>>(4)?)),
    ).map_err(sql)?;
    row.map(|(user_id, request_id, epoch, state, checked_at)| {
        Ok(FenceRow {
            user_id: user_id
                .try_into()
                .map_err(|_| Error::Invariant("bad fence user"))?,
            request_id,
            epoch: u64::try_from(epoch).map_err(|_| Error::Invariant("bad fence epoch"))?,
            state,
            checked_at: checked_at
                .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("bad fence time")))
                .transpose()?,
        })
    })
    .transpose()
}

pub fn active(connection: &Connection, account_id: &[u8; 32]) -> Result<bool, Error> {
    Ok(get(connection, account_id)?.is_some_and(|row| row.state != "released"))
}

pub fn any_active(connection: &Connection) -> Result<bool, Error> {
    Ok(connection
        .query_scalar::<i64>(
            "SELECT EXISTS(SELECT 1 FROM recovery_fences WHERE state != 'released')",
            &[],
        )
        .map_err(sql)?
        != 0)
}

/// 同一要求なら世代を変えず、別要求ならreleasedからのみ次世代を作る。
pub fn begin(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    user_id: &[u8; 32],
    request_id: &[u8],
    now: u64,
) -> Result<u64, Error> {
    if let Some(existing) = get(connection, account_id)? {
        if existing.state != "released" {
            if existing.user_id == *user_id && existing.request_id == request_id {
                return Ok(existing.epoch);
            }
            return Err(Error::Conflict);
        }
        let epoch = existing.epoch.checked_add(1).ok_or(Error::Overflow)?;
        connection.execute(
            "UPDATE recovery_fences SET user_id = ?2, request_id = ?3, epoch = ?4, state = 'preparing', checked_at = NULL, updated_at = ?5 WHERE account_id = ?1 AND state = 'released'",
            params![account_id.as_slice(), user_id.as_slice(), request_id, epoch as i64, now as i64],
        ).map_err(sql)?;
        return Ok(epoch);
    }
    connection.execute(
        "INSERT INTO recovery_fences (account_id, user_id, request_id, epoch, state, updated_at) VALUES (?1, ?2, ?3, 1, 'preparing', ?4)",
        params![account_id.as_slice(), user_id.as_slice(), request_id, now as i64],
    ).map_err(sql)?;
    Ok(1)
}

pub fn transition(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    request_id: &[u8],
    epoch: u64,
    from: &str,
    to: &str,
    now: u64,
) -> Result<(), Error> {
    connection.execute(
        "UPDATE recovery_fences SET state = ?5, checked_at = CASE WHEN ?5 = 'ready' THEN ?6 ELSE checked_at END, updated_at = ?6 WHERE account_id = ?1 AND request_id = ?2 AND epoch = ?3 AND state = ?4",
        params![account_id.as_slice(), request_id, epoch as i64, from, to, now as i64],
    ).map_err(sql)?;
    crate::cas::ensure_changed(crate::cas::changes(connection)?, from, "fence changed")
}

pub fn matches(
    connection: &Connection,
    account_id: &[u8; 32],
    user_id: &[u8; 32],
    request_id: &[u8],
    epoch: u64,
) -> Result<Option<FenceRow>, Error> {
    Ok(get(connection, account_id)?.filter(|row| {
        row.user_id == *user_id && row.request_id == request_id && row.epoch == epoch
    }))
}

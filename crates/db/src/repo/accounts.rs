//! 口座（取引口座）と取引所アドレスの永続化。
//!
//! `trading_core` は取引口座IDをvaultから受け取るが、取引所の照合（`/info`の
//! `userFills`・`clearinghouseState`・`orderStatus`）には**取引所のアドレス**が要る。
//! 本人の署名済み要求を処理するときにvaultから一度だけ取得してここへ保存し、
//! sweepはこの表だけを見て照合する（sweep中はセッションが無いため）。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 照合に使う口座の行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub account_id: [u8; 32],
    pub user_id: [u8; 32],
    pub master_address: [u8; 20],
}

/// 口座と取引所アドレスを登録する（同じ口座の再登録は更新）。
///
/// `users` への外部キーがあるため、利用者の行も同じトランザクションで用意する。
pub fn upsert(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    user_id: &[u8; 32],
    master_address: &[u8; 20],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO users (user_id, status, created_at) VALUES (?1, 'active', ?2)
             ON CONFLICT(user_id) DO NOTHING",
            params![user_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    connection
        .execute(
            "INSERT INTO accounts
               (account_id, user_id, hl_master_address, ownership_checked_at, state, created_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?4)
             ON CONFLICT(account_id) DO UPDATE SET
               user_id = excluded.user_id,
               hl_master_address = excluded.hl_master_address,
               ownership_checked_at = excluded.ownership_checked_at,
               state = 'active'",
            params![
                account_id.as_slice(),
                user_id.as_slice(),
                master_address.as_slice(),
                now as i64
            ],
        )
        .map_err(sql)
}

/// 口座の取引所アドレス（未取得は`None`）。
pub fn master_address(
    connection: &Connection,
    account_id: &[u8; 32],
) -> Result<Option<[u8; 20]>, Error> {
    let raw = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT hl_master_address FROM accounts WHERE account_id = ?1",
            params![account_id.as_slice()],
        )
        .map_err(sql)?;
    raw.map(|address| {
        address
            .try_into()
            .map_err(|_| Error::Invariant("expected a 20-byte address"))
    })
    .transpose()
}

/// 照合の巡回対象（有効な口座を`account_id`順に返す）。
///
/// `after` より大きい口座だけを返す（キーセットページング）。呼び出し側は空の
/// 結果を得たら先頭から巡回し直す。
pub fn reconcile_candidates(
    connection: &Connection,
    after: Option<&[u8; 32]>,
    limit: u32,
) -> Result<Vec<AccountRow>, Error> {
    let rows = connection
        .query_all(
            "SELECT account_id, user_id, hl_master_address FROM accounts
              WHERE state = 'active' AND (?1 IS NULL OR account_id > ?1)
              ORDER BY account_id LIMIT ?2",
            params![
                match after {
                    Some(account_id) => ic_sqlite_vfs::db::Value::Blob(account_id),
                    None => ic_sqlite_vfs::db::Value::Null,
                },
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                ))
            },
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|(account_id, user_id, address)| {
            Ok(AccountRow {
                account_id: account_id
                    .try_into()
                    .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
                user_id: user_id
                    .try_into()
                    .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
                master_address: address
                    .try_into()
                    .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
            })
        })
        .collect()
}

/// 照合の巡回カーソル（最後に処理した口座ID）。
pub fn reconcile_cursor(connection: &Connection) -> Result<Option<[u8; 32]>, Error> {
    let raw = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT last_account_id FROM reconcile_cursor WHERE singleton = 1",
            params![],
        )
        .map_err(sql)?;
    raw.map(|account_id| {
        account_id
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte account id"))
    })
    .transpose()
}

/// 照合の巡回カーソルを進める。
pub fn set_reconcile_cursor(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO reconcile_cursor (singleton, last_account_id, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               last_account_id = excluded.last_account_id, updated_at = excluded.updated_at",
            params![account_id.as_slice(), now as i64],
        )
        .map_err(sql)
}

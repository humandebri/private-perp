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

pub type AccountIdentity = ([u8; 32], [u8; 20]);

/// Only accounts with unresolved orders, positions, or an outstanding observation
/// need background work. A final observation after the last order catches an
/// immediate fill before the timer is allowed to stop.
const NEEDS_MONITOR: &str = "NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='monitor' AND w.work_id=accounts.account_id AND w.allowed=0) AND (NOT EXISTS (SELECT 1 FROM account_metrics m WHERE m.account_id = accounts.account_id)
    OR EXISTS (SELECT 1 FROM positions p WHERE p.account_id = accounts.account_id AND p.size != '0')
    OR EXISTS (SELECT 1 FROM orders o WHERE o.account_id = accounts.account_id
        AND NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='order' AND w.work_id=o.order_id AND w.allowed=0)
        AND (o.state IN ('pending','open','partially_filled','unknown')
          OR o.updated_at >= (SELECT m.observed_at FROM account_metrics m WHERE m.account_id = accounts.account_id)
          OR o.updated_at >= COALESCE((SELECT a.last_fills_checked_at FROM account_observations a WHERE a.account_id = accounts.account_id), 0))))";

pub fn has_monitor_work(connection: &Connection) -> Result<bool, Error> {
    Ok(connection
        .query_optional_scalar::<i64>(
            &format!("SELECT 1 WHERE EXISTS(SELECT 1 FROM accounts WHERE state='active' AND {NEEDS_MONITOR})
                OR EXISTS(SELECT 1 FROM orders o WHERE o.dispatch_state IN ('queued','signing') AND NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='order' AND w.work_id=o.order_id AND w.allowed=0))
                OR EXISTS(SELECT 1 FROM orders o WHERE o.cancel_requested=1 AND o.state IN ('open','partially_filled') AND (o.cancel_dispatch_state IS NULL OR o.cancel_dispatch_state='signing') AND NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='cancel' AND w.work_id=o.order_id AND w.allowed=0))"),
            params![],
        )
        .map_err(sql)?
        .is_some())
}

pub fn identity(
    connection: &Connection,
    account_id: &[u8; 32],
) -> Result<Option<AccountIdentity>, Error> {
    let row = connection
        .query_optional(
            "SELECT user_id, hl_master_address FROM accounts WHERE account_id = ?1",
            params![account_id.as_slice()],
            |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    row.map(|(user, address)| {
        Ok((
            user.try_into()
                .map_err(|_| Error::Invariant("bad account user"))?,
            address
                .try_into()
                .map_err(|_| Error::Invariant("bad account address"))?,
        ))
    })
    .transpose()
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
    if let Some((existing_user, existing_address)) = identity(connection, account_id)?
        && (existing_user != *user_id || existing_address != *master_address)
    {
        return Err(Error::Conflict);
    }
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
            &format!(
                "SELECT account_id, user_id, hl_master_address FROM accounts
              WHERE state = 'active' AND {NEEDS_MONITOR} AND (?1 IS NULL OR account_id > ?1)
              ORDER BY account_id LIMIT ?2"
            ),
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

/// 未終端注文を持つ口座を別カーソルで巡回する。1口座の失敗が他を飢餓させない。
pub fn urgent_reconcile_candidate(
    connection: &Connection,
    after: Option<&[u8; 32]>,
) -> Result<Option<AccountRow>, Error> {
    let row = connection
        .query_optional(
            "SELECT a.account_id, a.user_id, a.hl_master_address FROM accounts a
              WHERE a.state = 'active' AND (?1 IS NULL OR a.account_id > ?1)
                AND EXISTS (SELECT 1 FROM orders o WHERE o.account_id = a.account_id
                  AND o.state IN ('pending', 'open', 'partially_filled', 'unknown'))
              ORDER BY a.account_id LIMIT 1",
            params![match after {
                Some(id) => ic_sqlite_vfs::db::Value::Blob(id),
                None => ic_sqlite_vfs::db::Value::Null,
            }],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(account_id, user_id, address)| {
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
    .transpose()
}

pub fn priority_reconcile_cursor(connection: &Connection) -> Result<Option<[u8; 32]>, Error> {
    let raw = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT last_account_id FROM reconcile_priority_cursor WHERE singleton = 1",
            params![],
        )
        .map_err(sql)?;
    raw.map(|value| {
        value
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte account id"))
    })
    .transpose()
}

pub fn set_priority_reconcile_cursor(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO reconcile_priority_cursor (singleton, last_account_id, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               last_account_id = excluded.last_account_id, updated_at = excluded.updated_at",
            params![account_id.as_slice(), now as i64],
        )
        .map_err(sql)
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

/// 約定履歴の次回取得時刻。未終端注文があれば短めにし、空口座は低頻度にする。
pub fn fills_due(connection: &Connection, account_id: &[u8; 32], now: u64) -> Result<bool, Error> {
    let last = connection
        .query_optional(
            "SELECT last_fills_checked_at FROM account_observations WHERE account_id = ?1",
            params![account_id.as_slice()],
            |row| row.get::<Option<i64>>(0),
        )
        .map_err(sql)?
        .flatten();
    let terminal_unobserved = connection.query_optional_scalar::<i64>(
        "SELECT 1 FROM orders WHERE account_id = ?1 AND state IN ('filled','cancelled','rejected')
         AND updated_at >= ?2 LIMIT 1", params![account_id.as_slice(), last.unwrap_or(0)],
    ).map_err(sql)?.is_some();
    if terminal_unobserved {
        return Ok(true);
    }
    let active = connection
        .query_optional_scalar::<i64>(
            "SELECT 1 FROM orders WHERE account_id = ?1
               AND state IN ('pending', 'open', 'partially_filled', 'unknown') LIMIT 1",
            params![account_id.as_slice()],
        )
        .map_err(sql)?
        .is_some();
    let interval = if active { 120_000 } else { 600_000 };
    Ok(last.is_none_or(|last| now.saturating_sub(last.max(0) as u64) >= interval))
}

pub fn mark_fills_checked(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE account_observations SET last_fills_checked_at = ?2 WHERE account_id = ?1",
            params![account_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "account observation",
        "missing",
    )
}

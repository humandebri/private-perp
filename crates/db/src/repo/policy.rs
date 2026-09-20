//! 政策と停止状態の永続化（fail-closed）。`docs/phase-0/api-contract.md` 5節。
//!
//! 政策行が無い場合は「許可」ではなく**エラー**を返す。呼び出し側（`trading_core`など）は
//! 読み取り失敗時に新規受付・新規リスク増加を止める。

use crate::error::Error;
use crate::repo::sql;
use api_types::policy::{Policy, StopStatus};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 政策を設定する（版とallowlist）。
pub fn set_policy(
    connection: &mut UpdateConnection<'_>,
    version: u64,
    markets: &[String],
) -> Result<(), Error> {
    let markets = markets.join(",");
    connection
        .execute(
            "INSERT INTO policy (singleton, version, markets) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET version = excluded.version, markets = excluded.markets",
            params![version as i64, markets.as_str()],
        )
        .map_err(sql)
}

/// 政策を読む。未設定はエラー（fail-closed）。
pub fn policy(connection: &Connection) -> Result<Policy, Error> {
    let row = connection
        .query_optional(
            "SELECT version, markets FROM policy WHERE singleton = 1",
            params![],
            |row| Ok((row.get::<i64>(0)?, row.get::<String>(1)?)),
        )
        .map_err(sql)?
        .ok_or(Error::Invariant("policy is not configured"))?;
    Ok(Policy {
        version: u64::try_from(row.0).map_err(|_| Error::Invariant("negative version"))?,
        markets: row
            .1
            .split(',')
            .filter(|market| !market.is_empty())
            .map(|market| market.to_string())
            .collect(),
    })
}

/// 運営principalを設定する（controllerが初期化時に1度だけ）。
pub fn set_operator(connection: &mut UpdateConnection<'_>, operator: &[u8]) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO policy (singleton, version, markets, operator) VALUES (1, 0, '', ?1)
             ON CONFLICT(singleton) DO UPDATE SET operator = excluded.operator",
            params![operator],
        )
        .map_err(sql)
}

/// 運営principal。
pub fn operator(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT operator FROM policy WHERE singleton = 1",
            params![],
        )
        .map_err(sql)
}

/// 停止状態を設定する。`stopped = true` は停止方向のみ、解除は運営principalの判断。
pub fn set_stop(
    connection: &mut UpdateConnection<'_>,
    stopped: bool,
    reason: Option<&str>,
    now: u64,
) -> Result<(), Error> {
    let reason_value = match reason {
        Some(reason) => ic_sqlite_vfs::db::Value::Text(reason),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO stop_status (singleton, stopped, reason, since) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET stopped = excluded.stopped, reason = excluded.reason, since = excluded.since",
            params![if stopped { 1_i64 } else { 0_i64 }, reason_value, now as i64],
        )
        .map_err(sql)
}

/// 停止状態を読む。未設定は「停止していない」ではなく`stopped = true`（fail-closed）。
pub fn stop_status(connection: &Connection) -> Result<StopStatus, Error> {
    let row = connection
        .query_optional(
            "SELECT stopped, reason, since FROM stop_status WHERE singleton = 1",
            params![],
            |row| {
                Ok((
                    row.get::<i64>(0)?,
                    row.get::<Option<String>>(1)?,
                    row.get::<Option<i64>>(2)?,
                ))
            },
        )
        .map_err(sql)?;

    match row {
        Some((stopped, reason, since)) => Ok(StopStatus {
            stopped: stopped != 0,
            reason,
            since: since
                .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("negative time")))
                .transpose()?,
        }),
        // 未設定は安全側（停止）として扱う。
        None => Ok(StopStatus {
            stopped: true,
            reason: Some("policy_not_configured".to_string()),
            since: None,
        }),
    }
}

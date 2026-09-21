//! 建玉の永続化（`/info`照合の結果）。

use crate::error::Error;
use crate::repo::sql;
use api_types::order::PositionView;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 建玉を登録・更新する（口座と銘柄で一意）。
pub fn upsert(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    position: &PositionView,
    observed_at: u64,
) -> Result<(), Error> {
    let sl = optional_text(position.stop_loss.as_deref());
    let tp = optional_text(position.take_profit.as_deref());
    let liq = optional_text(position.liquidation_price.as_deref());
    connection
        .execute(
            "INSERT INTO positions
               (account_id, market, size, entry_price, liquidation_price, unrealized_pnl,
                leverage, margin_mode, stop_loss, take_profit, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(account_id, market) DO UPDATE SET
               size = excluded.size, entry_price = excluded.entry_price,
               liquidation_price = excluded.liquidation_price,
               unrealized_pnl = excluded.unrealized_pnl, leverage = excluded.leverage,
               margin_mode = excluded.margin_mode, stop_loss = excluded.stop_loss,
               take_profit = excluded.take_profit, observed_at = excluded.observed_at",
            params![
                account_id.as_slice(),
                position.market.as_str(),
                position.size.as_str(),
                position.entry_price.as_str(),
                liq,
                position.unrealized_pnl,
                position.leverage as i64,
                position.margin_mode.as_str(),
                sl,
                tp,
                observed_at as i64
            ],
        )
        .map_err(sql)
}

/// 省略可能な値をSQLのNULL/Textへ。
fn optional_text(text: Option<&str>) -> ic_sqlite_vfs::db::Value<'_> {
    match text {
        Some(text) => ic_sqlite_vfs::db::Value::Text(text),
        None => ic_sqlite_vfs::db::Value::Null,
    }
}

/// 口座の建玉一覧。
pub fn list(connection: &Connection, account_id: &[u8; 32]) -> Result<Vec<PositionView>, Error> {
    connection
        .query_all(
            "SELECT market, size, entry_price, liquidation_price, unrealized_pnl, leverage,
                    margin_mode, stop_loss, take_profit
               FROM positions WHERE account_id = ?1 ORDER BY market",
            params![account_id.as_slice()],
            |row| {
                Ok(PositionView {
                    market: row.get::<String>(0)?,
                    size: row.get::<String>(1)?,
                    entry_price: row.get::<String>(2)?,
                    liquidation_price: row.get::<Option<String>>(3)?,
                    unrealized_pnl: row.get::<i64>(4)?,
                    leverage: u32::try_from(row.get::<i64>(5)?).unwrap_or(0),
                    margin_mode: row.get::<String>(6)?,
                    stop_loss: row.get::<Option<String>>(7)?,
                    take_profit: row.get::<Option<String>>(8)?,
                })
            },
        )
        .map_err(sql)
}

/// 建玉の最新観測時刻（データ鮮度の判定に使う）。
pub fn latest_observed(
    connection: &Connection,
    account_id: &[u8; 32],
) -> Result<Option<u64>, Error> {
    let value = connection
        .query_optional_scalar::<i64>(
            "SELECT MAX(observed_at) FROM positions WHERE account_id = ?1",
            params![account_id.as_slice()],
        )
        .map_err(sql)?;
    value
        .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("negative time")))
        .transpose()
}

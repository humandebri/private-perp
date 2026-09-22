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
            position_from_row,
        )
        .map_err(sql)
}

/// 口座の銘柄の建玉（無ければ`None`）。決済・SL/TPの整合判定に使う。
pub fn find(
    connection: &Connection,
    account_id: &[u8; 32],
    market: &str,
) -> Result<Option<PositionView>, Error> {
    connection
        .query_optional(
            "SELECT market, size, entry_price, liquidation_price, unrealized_pnl, leverage,
                    margin_mode, stop_loss, take_profit
               FROM positions WHERE account_id = ?1 AND market = ?2",
            params![account_id.as_slice(), market],
            position_from_row,
        )
        .map_err(sql)
}

/// 取引所が返した建玉の全量で口座の建玉を置き換える（消えた建玉を残さない）。
///
/// `/info`の`clearinghouseState`は建玉の全量であり、決済済みの建玉を含まない。
/// 差分更新にすると、決済した建玉が建玉0として残り続ける。
///
/// 今回は現れなかった行を消すため、観測時刻を前回より必ず大きくしてから書き込み、
/// それより古い行を削除する（同一ミリ秒の再取り込みでも取り違えない）。
pub fn replace_all(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    positions: &[PositionView],
    observed_at: u64,
) -> Result<u32, Error> {
    let previous = latest_observed(connection, account_id)?.unwrap_or(0);
    let stamp = observed_at.max(previous.saturating_add(1));
    for position in positions {
        upsert(connection, account_id, position, stamp)?;
    }
    connection
        .execute(
            "DELETE FROM positions WHERE account_id = ?1 AND observed_at < ?2",
            params![account_id.as_slice(), stamp as i64],
        )
        .map_err(sql)?;
    let removed = crate::cas::changes(connection)?;
    // 空の建玉一覧も取引所から得た有効な観測である。positionsのMAXだけに頼ると、
    // 建玉を持たない口座が永遠に「未観測」となり新規注文を受け付けられない。
    connection
        .execute(
            "INSERT INTO account_observations (account_id, observed_at) VALUES (?1, ?2)
             ON CONFLICT(account_id) DO UPDATE SET observed_at = excluded.observed_at",
            params![account_id.as_slice(), stamp as i64],
        )
        .map_err(sql)?;
    u32::try_from(removed).map_err(|_| Error::Invariant("negative change count"))
}

fn position_from_row(
    row: &ic_sqlite_vfs::db::Row<'_>,
) -> Result<PositionView, ic_sqlite_vfs::DbError> {
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
}

/// 建玉の最新観測時刻（データ鮮度の判定に使う）。
pub fn latest_observed(
    connection: &Connection,
    account_id: &[u8; 32],
) -> Result<Option<u64>, Error> {
    let row = connection
        .query_optional_scalar::<i64>(
            "SELECT observed_at FROM account_observations WHERE account_id = ?1",
            params![account_id.as_slice()],
        )
        .map_err(sql)?;
    match row {
        Some(value) if value > 0 => {
            Some(u64::try_from(value).map_err(|_| Error::Invariant("negative time")))
        }
        _ => None,
    }
    .transpose()
}

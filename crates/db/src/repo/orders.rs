//! 注文とリスク予約の永続化（`Implementation.md` 4.3、`docs/phase-0/state-machines.md` 4節）。
//!
//! 未配線（`trading_core` の注文パイプライン実装時に使う）。表は coreスキーマv2で作成済み。

use crate::error::Error;
use crate::repo::sql;
use crate::states::{order_state_from_str, order_state_str};
use api_types::order::OrderState;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 新規注文の内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOrder {
    pub order_id: [u8; 32],
    pub user_id: [u8; 32],
    pub account_id: [u8; 32],
    pub client_request_id: Vec<u8>,
    pub cloid: [u8; 16],
    pub market: String,
    pub asset_index: u32,
    pub is_buy: bool,
    pub kind: String,
    pub price: Option<String>,
    pub quantity: String,
    pub reduce_only: bool,
}

/// 受付を記録して注文を`pending`で登録する（同じ受付IDの再送は検出済みとする）。
pub fn insert_pending_order(
    connection: &mut UpdateConnection<'_>,
    order: &NewOrder,
    now: u64,
) -> Result<(), Error> {
    let price_value = match order.price.as_deref() {
        Some(price) => ic_sqlite_vfs::db::Value::Text(price),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO orders
               (order_id, user_id, account_id, client_request_id, cloid, market, asset_index,
                side, kind, price, quantity, reduce_only, state, filled_quantity, cancel_requested,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, '0', 0, ?14, ?14)",
            params![
                order.order_id.as_slice(),
                order.user_id.as_slice(),
                order.account_id.as_slice(),
                order.client_request_id.as_slice(),
                order.cloid.as_slice(),
                order.market.as_str(),
                order.asset_index as i64,
                if order.is_buy { "buy" } else { "sell" },
                order.kind.as_str(),
                price_value,
                order.quantity.as_str(),
                if order.reduce_only { 1_i64 } else { 0_i64 },
                order_state_str(OrderState::Pending),
                now as i64
            ],
        )
        .map_err(sql)
}

/// 注文状態を更新する（累積約定量は置き換える）。
pub fn set_order_state(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    state: OrderState,
    filled_quantity: &str,
    hl_oid: Option<u64>,
    now: u64,
) -> Result<(), Error> {
    let oid_value = match hl_oid {
        Some(oid) => {
            ic_sqlite_vfs::db::Value::Integer(i64::try_from(oid).map_err(|_| Error::Overflow)?)
        }
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "UPDATE orders SET state = ?2, filled_quantity = ?3, hl_oid = COALESCE(?4, hl_oid), updated_at = ?5
              WHERE order_id = ?1",
            params![
                order_id.as_slice(),
                order_state_str(state),
                filled_quantity,
                oid_value,
                now as i64
            ],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "existing order", "missing order")
}

/// 取消要求を記録する（状態とは別に保持する）。
pub fn mark_cancel_requested(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET cancel_requested = 1, updated_at = ?2 WHERE order_id = ?1",
            params![order_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "existing order", "missing order")
}

/// 注文の状態を読む。
pub fn order_state(
    connection: &Connection,
    order_id: &[u8; 32],
) -> Result<Option<OrderState>, Error> {
    let value = connection
        .query_optional_scalar::<String>(
            "SELECT state FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
        )
        .map_err(sql)?;
    value
        .map(|state| order_state_from_str(&state).ok_or(Error::Invariant("unknown order state")))
        .transpose()
}

/// 口座の未終端注文（pending/open/partially_filled/unknown）の件数。
pub fn pending_order_count(connection: &Connection, account_id: &[u8; 32]) -> Result<u64, Error> {
    let count = connection
        .query_scalar::<i64>(
            "SELECT COUNT(*) FROM orders
              WHERE account_id = ?1 AND state IN ('pending', 'open', 'partially_filled', 'unknown')",
            params![account_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(count).map_err(|_| Error::Invariant("negative count"))
}

/// リスク予約を確保する（状態を持たないので同一IDの二重登録は拒否）。
pub fn reserve_risk(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    client_request_id: &[u8],
    notional: u64,
    now: u64,
) -> Result<(), Error> {
    let notional = i64::try_from(notional).map_err(|_| Error::Overflow)?;
    connection
        .execute(
            "INSERT INTO risk_reservations (account_id, client_request_id, notional, state, created_at)
             VALUES (?1, ?2, ?3, 'held', ?4)",
            params![account_id.as_slice(), client_request_id, notional, now as i64],
        )
        .map_err(sql)
}

/// 口座の保有リスク予約の合計。
pub fn held_risk(connection: &Connection, account_id: &[u8; 32]) -> Result<u64, Error> {
    let total = connection
        .query_scalar::<i64>(
            "SELECT COALESCE(SUM(notional), 0) FROM risk_reservations
              WHERE account_id = ?1 AND state = 'held'",
            params![account_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(total).map_err(|_| Error::Invariant("negative notional"))
}

/// 受付IDに対応する既存注文（再送時に同じ結果を返すため）。
pub fn order_by_request(
    connection: &Connection,
    user_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<Option<([u8; 32], [u8; 16])>, Error> {
    let raw = connection
        .query_optional(
            "SELECT order_id, cloid FROM orders WHERE user_id = ?1 AND client_request_id = ?2",
            params![user_id.as_slice(), client_request_id],
            |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    raw.map(|(order_id, cloid)| {
        Ok((
            order_id
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte order id"))?,
            cloid
                .try_into()
                .map_err(|_| Error::Invariant("expected a 16-byte cloid"))?,
        ))
    })
    .transpose()
}

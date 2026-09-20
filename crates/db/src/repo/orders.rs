//! 注文とリスク予約の永続化（`Implementation.md` 4.3、`docs/phase-0/state-machines.md` 4節）。
//!
//! 未配線（`trading_core` の注文パイプライン実装時に使う）。表は coreスキーマv2で作成済み。

use crate::error::Error;
use crate::repo::sql;
use crate::states::{order_state_from_str, order_state_str};
use api_types::order::{OrderState, OrderSummary};
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

/// 注文の同一性（`order_id` と `cloid`）。
pub type OrderIdentity = ([u8; 32], [u8; 16]);

/// 受付IDに対応する既存注文（再送時に同じ結果を返すため）。
pub fn order_by_request(
    connection: &Connection,
    user_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<Option<OrderIdentity>, Error> {
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

/// 口座ではなく**利用者**単位の注文一覧（新しい順）。`before_rowid` はページング用。
pub fn list_orders(
    connection: &Connection,
    user_id: &[u8; 32],
    before_rowid: Option<i64>,
    limit: u32,
) -> Result<Vec<(i64, OrderSummary)>, Error> {
    let rows = connection
        .query_all(
            "SELECT rowid, order_id, cloid, market, asset_index, side, kind, price, quantity,
                    filled_quantity, reduce_only, state, cancel_requested, hl_oid, created_at, updated_at
               FROM orders
              WHERE user_id = ?1 AND (?2 IS NULL OR rowid < ?2)
              ORDER BY rowid DESC
              LIMIT ?3",
            params![
                user_id.as_slice(),
                match before_rowid {
                    Some(value) => ic_sqlite_vfs::db::Value::Integer(value),
                    None => ic_sqlite_vfs::db::Value::Null,
                },
                limit as i64
            ],
            |row| {
                Ok((
                    row.get::<i64>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<String>(3)?,
                    row.get::<i64>(4)?,
                    row.get::<String>(5)?,
                    row.get::<String>(6)?,
                    row.get::<Option<String>>(7)?,
                    row.get::<String>(8)?,
                    row.get::<String>(9)?,
                    row.get::<i64>(10)?,
                    row.get::<String>(11)?,
                    row.get::<i64>(12)?,
                    row.get::<Option<i64>>(13)?,
                    row.get::<i64>(14)?,
                    row.get::<i64>(15)?,
                ))
            },
        )
        .map_err(sql)?;

    rows.into_iter()
        .map(|row| {
            let state =
                order_state_from_str(&row.11).ok_or(Error::Invariant("unknown order state"))?;
            Ok((
                row.0,
                OrderSummary {
                    order_id: row.1.into(),
                    cloid: row.2.into(),
                    market: row.3,
                    asset_index: u32::try_from(row.4).map_err(|_| Error::Invariant("bad index"))?,
                    is_buy: row.5 == "buy",
                    kind: row.6,
                    price: row.7,
                    quantity: row.8,
                    filled_quantity: row.9,
                    reduce_only: row.10 != 0,
                    state,
                    cancel_requested: row.12 != 0,
                    hl_oid: row
                        .13
                        .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("bad oid")))
                        .transpose()?,
                    created_at: u64::try_from(row.14).map_err(|_| Error::Invariant("bad time"))?,
                    updated_at: u64::try_from(row.15).map_err(|_| Error::Invariant("bad time"))?,
                },
            ))
        })
        .collect()
}

/// 注文の所有者と状態（取消の認可判定に使う）。
pub fn order_owner(
    connection: &Connection,
    order_id: &[u8; 32],
) -> Result<Option<([u8; 32], OrderState, bool)>, Error> {
    let raw = connection
        .query_optional(
            "SELECT user_id, state, cancel_requested FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<String>(1)?,
                    row.get::<i64>(2)?,
                ))
            },
        )
        .map_err(sql)?;
    raw.map(|(user_id, state, cancel_requested)| {
        Ok((
            user_id
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
            order_state_from_str(&state).ok_or(Error::Invariant("unknown order state"))?,
            cancel_requested != 0,
        ))
    })
    .transpose()
}

/// 署名に必要な注文情報（`trading_core` が Agent鍵で署名する）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignableOrder {
    pub account_id: [u8; 32],
    pub asset_index: u32,
    pub is_buy: bool,
    pub price: Option<String>,
    pub quantity: String,
    pub kind: String,
    pub reduce_only: bool,
    pub cloid: [u8; 16],
    pub created_at: u64,
}

/// 注文を署名用に取得する。
pub fn signable(
    connection: &Connection,
    order_id: &[u8; 32],
) -> Result<Option<SignableOrder>, Error> {
    let raw = connection
        .query_optional(
            "SELECT account_id, asset_index, side, price, quantity, kind, reduce_only, cloid, created_at
               FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<i64>(1)?,
                    row.get::<String>(2)?,
                    row.get::<Option<String>>(3)?,
                    row.get::<String>(4)?,
                    row.get::<String>(5)?,
                    row.get::<i64>(6)?,
                    row.get::<Vec<u8>>(7)?,
                    row.get::<i64>(8)?,
                ))
            },
        )
        .map_err(sql)?;
    raw.map(|row| {
        Ok(SignableOrder {
            account_id: row
                .0
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
            asset_index: u32::try_from(row.1).map_err(|_| Error::Invariant("bad index"))?,
            is_buy: row.2 == "buy",
            price: row.3,
            quantity: row.4,
            kind: row.5,
            reduce_only: row.6 != 0,
            cloid: row
                .7
                .try_into()
                .map_err(|_| Error::Invariant("expected a 16-byte cloid"))?,
            created_at: u64::try_from(row.8).map_err(|_| Error::Invariant("bad time"))?,
        })
    })
    .transpose()
}

/// 送信待ちの注文（古い順）。
pub fn queued_orders(connection: &Connection, limit: u32) -> Result<Vec<[u8; 32]>, Error> {
    let rows = connection
        .query_all(
            "SELECT order_id FROM orders WHERE dispatch_state = 'queued' ORDER BY rowid LIMIT ?1",
            params![limit as i64],
            |row| row.get::<Vec<u8>>(0),
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|bytes| {
            bytes
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte order id"))
        })
        .collect()
}

/// 送信権を取得する（POST発行**前**に呼ぶ。単一の実行者だけtrue）。
pub fn claim_for_dispatch(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
) -> Result<bool, Error> {
    connection
        .execute(
            "UPDATE orders SET dispatch_state = 'signing' WHERE order_id = ?1 AND dispatch_state = 'queued'",
            params![order_id.as_slice()],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    Ok(changed > 0)
}

/// 署名とpayloadを保存して`dispatching`へ（送信前に確定させる）。
pub fn mark_dispatching(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    wire_payload: &[u8],
    signature: &[u8],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET dispatch_state = 'dispatching', wire_payload = ?2, signature = ?3, updated_at = ?4
              WHERE order_id = ?1 AND dispatch_state = 'signing'",
            params![order_id.as_slice(), wire_payload, signature, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        return Err(Error::StateConflict {
            expected: "signing".to_string(),
            actual: "missing or not claimed".to_string(),
        });
    }
    Ok(())
}

/// 取引所が受理した（`open`へ）。
pub fn mark_venue_accepted(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    hl_oid: Option<u64>,
    now: u64,
) -> Result<(), Error> {
    let oid = match hl_oid {
        Some(oid) => {
            ic_sqlite_vfs::db::Value::Integer(i64::try_from(oid).map_err(|_| Error::Overflow)?)
        }
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "UPDATE orders SET state = 'open', dispatch_state = 'reconciled', hl_oid = COALESCE(?2, hl_oid), updated_at = ?3
              WHERE order_id = ?1 AND dispatch_state = 'dispatching'",
            params![order_id.as_slice(), oid, now as i64],
        )
        .map_err(sql)
        .and_then(|_| {
            let changed = crate::cas::changes(connection)?;
            crate::cas::ensure_changed(changed, "dispatching", "not dispatching")
        })
}

/// 取引所が拒否した（`rejected`へ）。
pub fn mark_venue_rejected(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET state = 'rejected', dispatch_state = 'reconciled', updated_at = ?2
              WHERE order_id = ?1 AND dispatch_state = 'dispatching'",
            params![order_id.as_slice(), now as i64],
        )
        .map_err(sql)
        .and_then(|_| {
            let changed = crate::cas::changes(connection)?;
            crate::cas::ensure_changed(changed, "dispatching", "not dispatching")
        })
}

/// 結果不明（再送しない）。
pub fn mark_unknown(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET state = 'unknown', dispatch_state = 'unknown', updated_at = ?2
              WHERE order_id = ?1 AND dispatch_state IN ('signing', 'dispatching')",
            params![order_id.as_slice(), now as i64],
        )
        .map_err(sql)
        .and_then(|_| {
            let changed = crate::cas::changes(connection)?;
            crate::cas::ensure_changed(changed, "signing or dispatching", "not in flight")
        })
}

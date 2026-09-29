//! 注文とリスク予約の永続化（`Implementation.md` 4.3、`docs/phase-0/state-machines.md` 4節）。
//!
//! 未配線（`trading_core` の注文パイプライン実装時に使う）。表は coreスキーマv2で作成済み。

use crate::error::Error;
use crate::repo::sql;
use crate::states::{
    order_state_from_str, order_state_str, trigger_kind_from_str, trigger_kind_str,
};
use api_types::order::{OrderState, OrderSummary};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 受付けるトリガ（SL/TP）注文の内容。建玉単位で、`reduce_only`が必須。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTrigger {
    /// `"stop_loss"` または `"take_profit"`。
    pub kind: String,
    pub price: String,
    pub is_market: bool,
}

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
    pub effective_leverage: u32,
    pub slippage_tolerance_bps: Option<u32>,
    pub expires_after: Option<u64>,
    /// SL/TPトリガ。`None`は通常注文。
    pub trigger: Option<NewTrigger>,
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
    let trigger_kind = match order.trigger.as_ref() {
        Some(trigger) => ic_sqlite_vfs::db::Value::Text(trigger.kind.as_str()),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let trigger_price = match order.trigger.as_ref() {
        Some(trigger) => ic_sqlite_vfs::db::Value::Text(trigger.price.as_str()),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let trigger_is_market = match order.trigger.as_ref() {
        Some(trigger) => {
            ic_sqlite_vfs::db::Value::Integer(if trigger.is_market { 1_i64 } else { 0_i64 })
        }
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let slippage = match order.slippage_tolerance_bps {
        Some(value) => ic_sqlite_vfs::db::Value::Integer(i64::from(value)),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let expires_after = match order.expires_after {
        Some(value) => {
            ic_sqlite_vfs::db::Value::Integer(i64::try_from(value).map_err(|_| Error::Overflow)?)
        }
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO orders
               (order_id, user_id, account_id, client_request_id, cloid, market, asset_index,
                side, kind, price, quantity, reduce_only, trigger_kind, trigger_price,
                trigger_is_market, effective_leverage, slippage_tolerance_bps, expires_after,
                state, filled_quantity, cancel_requested, created_at, updated_at,
                preflight_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, 'pending', '0', 0, ?19, ?19, ?20)",
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
                trigger_kind,
                trigger_price,
                trigger_is_market,
                order.effective_leverage as i64,
                slippage,
                expires_after,
                now as i64,
                if order.reduce_only {
                    "reconciled"
                } else {
                    "queued"
                },
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

/// 予約済みリスクに今回の想定元本を足しても equity を超えないことを検査する。
///
/// 取引口座のequity（vaultが導出）に対する上限で、口座ごとの無制限な発注を防ぐ。
pub fn ensure_risk_within_equity(
    connection: &Connection,
    account_id: &[u8; 32],
    notional: u64,
    equity: u64,
) -> Result<(), Error> {
    let held = held_risk(connection, account_id)?;
    if held.saturating_add(notional) > equity {
        return Err(Error::RiskLimitExceeded { limit: equity });
    }
    Ok(())
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
    select_orders(connection, user_id, before_rowid, limit, None)
}

pub fn summary_by_request(
    connection: &Connection,
    user_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<Option<OrderSummary>, Error> {
    Ok(
        select_orders(connection, user_id, None, 1, Some(client_request_id))?
            .into_iter()
            .next()
            .map(|(_, order)| order),
    )
}

fn select_orders(
    connection: &Connection,
    user_id: &[u8; 32],
    before_rowid: Option<i64>,
    limit: u32,
    client_request_id: Option<&[u8]>,
) -> Result<Vec<(i64, OrderSummary)>, Error> {
    let rows = connection
        .query_all(
            "SELECT rowid, order_id, cloid, market, asset_index, side, kind, price, quantity,
                    filled_quantity, reduce_only, state, cancel_requested, hl_oid, created_at, updated_at,
                    dispatch_state, trigger_kind, trigger_price, trigger_is_market,
                    preflight_state, effective_leverage, slippage_tolerance_bps, expires_after,
                    last_error
               FROM orders
              WHERE user_id = ?1 AND (?2 IS NULL OR rowid < ?2)
                AND (?4 IS NULL OR client_request_id = ?4)
              ORDER BY rowid DESC
              LIMIT ?3",
            params![
                user_id.as_slice(),
                match before_rowid {
                    Some(value) => ic_sqlite_vfs::db::Value::Integer(value),
                    None => ic_sqlite_vfs::db::Value::Null,
                },
                limit as i64,
                match client_request_id {
                    Some(value) => ic_sqlite_vfs::db::Value::Blob(value),
                    None => ic_sqlite_vfs::db::Value::Null,
                }
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
                    row.get::<String>(16)?,
                    row.get::<Option<String>>(17)?,
                    row.get::<Option<String>>(18)?,
                    row.get::<Option<i64>>(19)?,
                    row.get::<String>(20)?,
                    row.get::<i64>(21)?,
                    row.get::<Option<i64>>(22)?,
                    row.get::<Option<i64>>(23)?,
                    row.get::<Option<String>>(24)?,
                ))
            },
        )
        .map_err(sql)?;

    rows.into_iter()
        .map(|row| {
            let state =
                order_state_from_str(&row.11).ok_or(Error::Invariant("unknown order state"))?;
            let dispatch_state = crate::states::action_state_from_str(&row.16)
                .ok_or(Error::Invariant("unknown dispatch state"))?;
            let preflight_state = crate::states::action_state_from_str(&row.20)
                .ok_or(Error::Invariant("unknown preflight state"))?;
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
                    dispatch_state,
                    preflight_state,
                    effective_leverage: u32::try_from(row.21)
                        .map_err(|_| Error::Invariant("bad leverage"))?,
                    effective_slippage_bps: row
                        .22
                        .map(|value| {
                            u32::try_from(value).map_err(|_| Error::Invariant("bad slippage"))
                        })
                        .transpose()?,
                    expires_after: row
                        .23
                        .map(|value| {
                            u64::try_from(value).map_err(|_| Error::Invariant("bad expiry"))
                        })
                        .transpose()?,
                    last_error: row.24,
                    cancel_requested: row.12 != 0,
                    hl_oid: row
                        .13
                        .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("bad oid")))
                        .transpose()?,
                    created_at: u64::try_from(row.14).map_err(|_| Error::Invariant("bad time"))?,
                    updated_at: u64::try_from(row.15).map_err(|_| Error::Invariant("bad time"))?,
                    trigger: trigger_from_columns(row.17, row.18, row.19)?,
                },
            ))
        })
        .collect()
}

/// トリガ列（種別・価格・成行）からAPIの型を組み立てる。
///
/// スキーマのCHECKにより3列は揃っているため、欠けは不変条件違反として扱う。
fn trigger_from_columns(
    kind: Option<String>,
    price: Option<String>,
    is_market: Option<i64>,
) -> Result<Option<api_types::order::Trigger>, Error> {
    match (kind, price, is_market) {
        (None, None, None) => Ok(None),
        (Some(kind), Some(trigger_price), Some(is_market)) => {
            let kind =
                trigger_kind_from_str(&kind).ok_or(Error::Invariant("unknown trigger kind"))?;
            Ok(Some(api_types::order::Trigger {
                kind,
                trigger_price,
                is_market: is_market != 0,
            }))
        }
        _ => Err(Error::Invariant("incomplete trigger columns")),
    }
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
    pub market: String,
    pub asset_index: u32,
    pub is_buy: bool,
    pub price: Option<String>,
    pub quantity: String,
    pub kind: String,
    pub client_request_id: Vec<u8>,
    pub reduce_only: bool,
    pub cloid: [u8; 16],
    pub created_at: u64,
    pub effective_leverage: u32,
    pub expires_after: Option<u64>,
    pub preflight_state: api_types::fund::ActionState,
    pub preflight_nonce: u64,
    pub order_nonce: u64,
    /// SL/TPトリガ（`None`は通常注文）。
    pub trigger: Option<OrderTrigger>,
}

/// 署名に必要なトリガ情報。`positionTpsl`（建玉単位・reduce-only）として構築する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTrigger {
    /// `"stop_loss"` または `"take_profit"`。
    pub kind: String,
    pub price: String,
    pub is_market: bool,
}

pub fn queued_orders(
    connection: &Connection,
    now: u64,
    limit: u32,
) -> Result<Vec<[u8; 32]>, Error> {
    let rows = connection
        .query_all(
            "SELECT order_id FROM orders
              WHERE NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='order' AND w.work_id=orders.order_id AND w.allowed=0) AND ((dispatch_state = 'queued' AND (next_check_at IS NULL OR next_check_at <= ?1))
                 OR (dispatch_state = 'signing' AND lease_until < ?1))
              ORDER BY created_at LIMIT ?2",
            params![now as i64, limit as i64],
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
    now: u64,
    lease_ms: u64,
) -> Result<Option<u64>, Error> {
    let lease_until = now.saturating_add(lease_ms);
    connection
        .execute(
            "UPDATE orders
                SET dispatch_state = 'signing', worker_epoch = worker_epoch + 1,
                    lease_until = ?2, attempt = attempt + 1, last_error = NULL,
                    updated_at = ?3
              WHERE order_id = ?1
                AND ((dispatch_state = 'queued' AND (next_check_at IS NULL OR next_check_at <= ?3))
                  OR (dispatch_state = 'signing' AND lease_until < ?3))",
            params![order_id.as_slice(), lease_until as i64, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        return Ok(None);
    }
    connection
        .query_scalar::<i64>(
            "SELECT worker_epoch FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
        )
        .map_err(sql)
        .and_then(|epoch| u64::try_from(epoch).map_err(|_| Error::Invariant("bad epoch")))
        .map(Some)
}

/// POST前の失敗。送信していないことを保証できるため、再試行可能に戻す。
pub fn retry_before_dispatch(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    error: &str,
    next_check_at: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders
                SET dispatch_state = 'queued', lease_until = NULL, next_check_at = ?3,
                    last_error = ?4, updated_at = ?5
              WHERE order_id = ?1 AND dispatch_state = 'signing' AND worker_epoch = ?2",
            params![
                order_id.as_slice(),
                worker_epoch as i64,
                next_check_at as i64,
                error,
                now as i64
            ],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "claimed signing epoch", "claim lost")
}

/// 外部POST直前に、claimと受付時の安全条件がまだ有効か検証する。
pub fn dispatch_blocker(
    connection: &Connection,
    order_id: &[u8; 32],
    worker_epoch: u64,
    now: u64,
) -> Result<Option<&'static str>, Error> {
    let row = connection
        .query_optional(
            "SELECT state, dispatch_state, worker_epoch, cancel_requested, expires_after,
                    reduce_only,
                    EXISTS(
                        SELECT 1 FROM risk_reservations r
                         WHERE r.account_id = orders.account_id
                           AND r.client_request_id = orders.client_request_id
                           AND r.state = 'held'),
                    EXISTS(
                        SELECT 1 FROM recovery_fences f
                         WHERE f.account_id = orders.account_id AND f.state != 'released')
               FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<String>(0)?,
                    row.get::<String>(1)?,
                    row.get::<i64>(2)?,
                    row.get::<i64>(3)?,
                    row.get::<Option<i64>>(4)?,
                    row.get::<i64>(5)?,
                    row.get::<i64>(6)?,
                    row.get::<i64>(7)?,
                ))
            },
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    if row.0 != "pending" || row.1 != "signing" || row.2 != worker_epoch as i64 {
        return Ok(Some("dispatch claim is no longer current"));
    }
    if row.3 != 0 {
        return Ok(Some("order was cancelled before dispatch"));
    }
    if row.4.is_some_and(|expiry| expiry <= now as i64) {
        return Ok(Some("order expired before dispatch"));
    }
    if row.5 == 0 && row.6 == 0 {
        return Ok(Some("risk reservation is no longer held"));
    }
    if row.5 == 0 && row.7 != 0 {
        return Ok(Some("account recovery fence is active"));
    }
    Ok(None)
}

pub fn abort_before_dispatch(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    reason: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders
                SET state = 'rejected', dispatch_state = 'aborted', lease_until = NULL,
                    last_error = ?3, updated_at = ?4
              WHERE order_id = ?1 AND worker_epoch = ?2 AND dispatch_state = 'signing'",
            params![order_id.as_slice(), worker_epoch as i64, reason, now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "claimed signing epoch",
        "claim lost",
    )?;
    release_risk_for_order(connection, order_id, now)
}

/// Journaled acceptance whose admission expired before the local commit.
/// The request remains idempotently visible, but this order can never be sent.
pub fn reject_queued_acceptance(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET state = 'rejected', dispatch_state = 'aborted',
                    last_error = 'admission_expired', updated_at = ?2
              WHERE order_id = ?1 AND state = 'pending'
                AND dispatch_state IN ('queued', 'reconciled')",
            params![order_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "queued acceptance",
        "acceptance changed",
    )
}

/// 署名とpayloadを保存して`dispatching`へ（送信前に確定させる）。
pub fn mark_dispatching(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    wire_payload: &[u8],
    signature: &[u8],
    worker_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET dispatch_state = 'dispatching', wire_payload = ?2, signature = ?3,
                    updated_at = ?5
              WHERE order_id = ?1 AND dispatch_state = 'signing' AND worker_epoch = ?4",
            params![
                order_id.as_slice(),
                wire_payload,
                signature,
                worker_epoch as i64,
                now as i64
            ],
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
    filled: bool,
    now: u64,
) -> Result<(), Error> {
    let oid = match hl_oid {
        Some(oid) => {
            ic_sqlite_vfs::db::Value::Integer(i64::try_from(oid).map_err(|_| Error::Overflow)?)
        }
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let state = if filled { "filled" } else { "open" };
    connection
        .execute(
            "UPDATE orders SET state = ?3, dispatch_state = 'reconciled', hl_oid = COALESCE(?2, hl_oid), updated_at = ?4
              WHERE order_id = ?1 AND dispatch_state = 'dispatching'",
            params![order_id.as_slice(), oid, state, now as i64],
        )
        .map_err(sql)
        .and_then(|_| {
            let changed = crate::cas::changes(connection)?;
            crate::cas::ensure_changed(changed, "dispatching", "not dispatching")
        })?;
    if filled {
        release_risk_for_order(connection, order_id, now)?;
    }
    Ok(())
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

/// 約定一覧（新しい順）。
pub fn list_fills(
    connection: &Connection,
    user_id: &[u8; 32],
    before_rowid: Option<i64>,
    limit: u32,
) -> Result<Vec<(i64, api_types::order::FillView)>, Error> {
    let rows = connection
        .query_all(
            "SELECT rowid, order_id, market, price, quantity, fee, filled_at
               FROM fills
              WHERE user_id = ?1 AND (?2 IS NULL OR rowid < ?2)
              ORDER BY rowid DESC LIMIT ?3",
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
                    row.get::<String>(2)?,
                    row.get::<String>(3)?,
                    row.get::<String>(4)?,
                    row.get::<i64>(5)?,
                    row.get::<i64>(6)?,
                ))
            },
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.0,
                api_types::order::FillView {
                    order_id: row.1.into(),
                    cloid: None,
                    market: row.2,
                    price: row.3,
                    quantity: row.4,
                    fee: row.5,
                    at: u64::try_from(row.6).map_err(|_| Error::Invariant("bad time"))?,
                },
            ))
        })
        .collect()
}

/// 取り込む約定の内容。
#[derive(Debug, Clone, Copy)]
pub struct NewFill<'a> {
    pub tid: u64,
    pub hl_oid: u64,
    pub market: &'a str,
    pub price: &'a str,
    pub quantity: &'a str,
    pub fee: i64,
    pub filled_at: u64,
}

/// Returns the matching order only while the external fill has not already
/// been applied. HL oids are scoped to an account, including for one user who
/// owns multiple trading accounts.
pub fn pending_fill_order(
    connection: &Connection,
    user_id: &[u8; 32],
    account_id: &[u8; 32],
    tid: u64,
    hl_oid: u64,
    market: &str,
) -> Result<Option<[u8; 32]>, Error> {
    let tid = i64::try_from(tid).map_err(|_| Error::Overflow)?;
    let hl_oid = i64::try_from(hl_oid).map_err(|_| Error::Overflow)?;
    let row = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT order_id FROM orders
             WHERE hl_oid = ?1 AND user_id = ?2 AND account_id = ?3 AND market = ?4
               AND NOT EXISTS (SELECT 1 FROM fills WHERE tid = ?5)
             LIMIT 1",
            params![
                hl_oid,
                user_id.as_slice(),
                account_id.as_slice(),
                market,
                tid
            ],
        )
        .map_err(sql)?;
    row.map(|bytes| {
        bytes
            .try_into()
            .map_err(|_| Error::Invariant("bad order id"))
    })
    .transpose()
}

fn decimal_parts(value: &str) -> Result<(u128, u32), Error> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 18
    {
        return Err(Error::Invariant("invalid fill quantity"));
    }
    let scale = u32::try_from(fraction.len()).map_err(|_| Error::Overflow)?;
    let factor = 10_u128.checked_pow(scale).ok_or(Error::Overflow)?;
    let integer = if whole.is_empty() {
        0
    } else {
        whole.parse::<u128>().map_err(|_| Error::Overflow)?
    };
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u128>().map_err(|_| Error::Overflow)?
    };
    let units = integer
        .checked_mul(factor)
        .and_then(|value| value.checked_add(fraction))
        .ok_or(Error::Overflow)?;
    if units == 0 {
        return Err(Error::Invariant("zero fill quantity"));
    }
    Ok((units, scale))
}

fn at_scale(units: u128, from: u32, to: u32) -> Result<u128, Error> {
    let factor = 10_u128.checked_pow(to - from).ok_or(Error::Overflow)?;
    units.checked_mul(factor).ok_or(Error::Overflow)
}

fn format_decimal(units: u128, scale: u32) -> String {
    if scale == 0 {
        return units.to_string();
    }
    let factor = 10_u128.pow(scale);
    let mut fraction = format!("{:0width$}", units % factor, width = scale as usize);
    while fraction.ends_with('0') {
        fraction.pop();
    }
    if fraction.is_empty() {
        (units / factor).to_string()
    } else {
        format!("{}.{fraction}", units / factor)
    }
}

/// 約定を取り込む（同じ`tid`は二重計上しない）。注文は`hl_oid`で解決する。
pub fn ingest_fill(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    account_id: &[u8; 32],
    fill: &NewFill<'_>,
) -> Result<bool, Error> {
    let NewFill {
        tid,
        hl_oid,
        market,
        price,
        quantity,
        fee,
        filled_at,
    } = *fill;
    let order = connection
        .query_optional(
            // HLのoidは口座ごとに採番される。同じ本人の別口座も含めて
            // oidだけでは注文を特定できない。
            "SELECT order_id, quantity FROM orders
             WHERE hl_oid = ?1 AND user_id = ?2 AND account_id = ?3 AND market = ?4 LIMIT 1",
            params![
                hl_oid as i64,
                user_id.as_slice(),
                account_id.as_slice(),
                market
            ],
            |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<String>(1)?)),
        )
        .map_err(sql)?;
    let Some((order_id, ordered_quantity)) = order else {
        return Ok(false);
    };
    let order_id: [u8; 32] = order_id
        .try_into()
        .map_err(|_| Error::Invariant("expected a 32-byte order id"))?;
    let existing = connection
        .query_optional_scalar::<i64>("SELECT tid FROM fills WHERE tid = ?1", params![tid as i64])
        .map_err(sql)?;
    if existing.is_some() {
        return Ok(false);
    }
    connection
        .execute(
            "INSERT INTO fills (tid, user_id, order_id, market, price, quantity, fee, filled_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                tid as i64,
                user_id.as_slice(),
                order_id.as_slice(),
                market,
                price,
                quantity,
                fee,
                filled_at as i64
            ],
        )
        .map_err(sql)?;
    let quantities = connection
        .query_all(
            "SELECT quantity FROM fills WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| row.get::<String>(0),
        )
        .map_err(sql)?;
    let (order_units, order_scale) = decimal_parts(&ordered_quantity)?;
    let parsed: Vec<_> = quantities
        .iter()
        .map(|value| decimal_parts(value))
        .collect::<Result<_, _>>()?;
    let scale = parsed
        .iter()
        .map(|(_, scale)| *scale)
        .max()
        .unwrap_or(0)
        .max(order_scale);
    let total = parsed.iter().try_fold(0_u128, |sum, (units, from)| {
        sum.checked_add(at_scale(*units, *from, scale)?)
            .ok_or(Error::Overflow)
    })?;
    let ordered = at_scale(order_units, order_scale, scale)?;
    if total > ordered {
        return Err(Error::Invariant("fills exceed order quantity"));
    }
    let filled_quantity = format_decimal(total, scale);
    let state = if total == ordered {
        "filled"
    } else {
        "partially_filled"
    };
    connection
        .execute(
            "UPDATE orders SET
                 state = CASE WHEN state IN ('filled', 'cancelled', 'rejected') THEN state ELSE ?2 END,
                 filled_quantity = CASE WHEN state = 'filled' THEN quantity ELSE ?3 END,
                 updated_at = ?4 WHERE order_id = ?1",
            params![order_id.as_slice(), state, filled_quantity, filled_at as i64],
        )
        .map_err(sql)?;
    if state == "filled" {
        // 約定した注文の予約は未約定リスクではなくなったため解放する。
        release_risk_for_order(connection, &order_id, filled_at)?;
    }
    Ok(true)
}

/// `orderStatus`照合の結果を反映する（本人の口座の注文だけを `hl_oid` で引く）。
///
/// 取引所のoidは口座ごとに採番されるため、oidだけで更新すると他人の注文の状態を
/// 書き換え得る。
pub fn order_status_target(
    connection: &Connection,
    account_id: &[u8; 32],
    hl_oid: u64,
) -> Result<Option<([u8; 32], String)>, Error> {
    let oid = i64::try_from(hl_oid).map_err(|_| Error::Overflow)?;
    let matches = connection
        .query_all(
            "SELECT order_id, state FROM orders
             WHERE account_id = ?1 AND hl_oid = ?2 LIMIT 2",
            params![account_id.as_slice(), oid],
            |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<String>(1)?)),
        )
        .map_err(sql)?;
    if matches.len() > 1 {
        return Err(Error::Invariant("duplicate venue oid for account"));
    }
    matches
        .into_iter()
        .next()
        .map(|(id, state)| {
            Ok((
                id.try_into()
                    .map_err(|_| Error::Invariant("bad order id"))?,
                state,
            ))
        })
        .transpose()
}

pub fn apply_order_status(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    hl_oid: u64,
    state: &str,
    now: u64,
) -> Result<bool, Error> {
    connection
        .execute(
            "UPDATE orders SET state = ?3, updated_at = ?4,
               dispatch_state = CASE WHEN dispatch_state IN ('unknown','dispatching') THEN 'reconciled' ELSE dispatch_state END,
               filled_quantity = CASE WHEN ?3 = 'filled' THEN quantity ELSE filled_quantity END
              WHERE hl_oid = ?1 AND account_id = ?2",
            params![hl_oid as i64, account_id.as_slice(), state, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed > 0 && matches!(state, "filled" | "cancelled" | "rejected") {
        // 終端に達した注文の予約は未約定リスクではなくなったため解放する。
        release_risk_for_oid(connection, account_id, hl_oid, now)?;
    }
    Ok(changed > 0)
}

/// リスク予約を解放する（取引所が拒否・結果不明のとき）。
pub fn release_risk(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<bool, Error> {
    connection
        .execute(
            "UPDATE risk_reservations SET state = 'released'
              WHERE account_id = ?1 AND client_request_id = ?2 AND state = 'held'",
            params![account_id.as_slice(), client_request_id],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    Ok(changed > 0)
}

/// 終端に達した注文のリスク予約を解放する（注文IDで引く）。
///
/// 約定・取消・拒否で注文が終端になると、その想定元本は未約定注文のリスクでは
/// なくなる（建玉の証拠金は取引所の観測が表す）。解放しないと予約が永久に残り、
/// equityに対する新規注文の枠を食い潰す。
fn release_risk_for_order(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE risk_reservations SET state = 'released', released_at = ?2
              WHERE state = 'held'
                AND account_id = (SELECT account_id FROM orders WHERE order_id = ?1)
                AND client_request_id = (SELECT client_request_id FROM orders WHERE order_id = ?1)",
            params![order_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    Ok(())
}

/// 終端に達した注文のリスク予約を解放する（口座と取引所oidで引く）。
fn release_risk_for_oid(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    hl_oid: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE risk_reservations SET state = 'released', released_at = ?3
              WHERE state = 'held' AND account_id = ?2
                AND client_request_id IN (
                    SELECT client_request_id FROM orders
                     WHERE account_id = ?2 AND hl_oid = ?1)",
            params![hl_oid as i64, account_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    Ok(())
}

/// 注文を署名用に取得する（列順はこの関数内で完結させる）。
pub fn signable(
    connection: &Connection,
    order_id: &[u8; 32],
) -> Result<Option<SignableOrder>, Error> {
    let raw = connection
        .query_optional(
            "SELECT account_id, market, asset_index, side, price, quantity, kind, client_request_id,
                    reduce_only, cloid, created_at, trigger_kind, trigger_price, trigger_is_market,
                    effective_leverage, expires_after, preflight_state,
                    preflight_nonce, order_nonce
               FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<String>(1)?,
                    row.get::<i64>(2)?,
                    row.get::<String>(3)?,
                    row.get::<Option<String>>(4)?,
                    row.get::<String>(5)?,
                    row.get::<String>(6)?,
                    row.get::<Vec<u8>>(7)?,
                    row.get::<i64>(8)?,
                    row.get::<Vec<u8>>(9)?,
                    row.get::<i64>(10)?,
                    row.get::<Option<String>>(11)?,
                    row.get::<Option<String>>(12)?,
                    row.get::<Option<i64>>(13)?,
                    row.get::<i64>(14)?,
                    row.get::<Option<i64>>(15)?,
                    row.get::<String>(16)?,
                    row.get::<Option<i64>>(17)?,
                    row.get::<Option<i64>>(18)?,
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
            market: row.1,
            asset_index: u32::try_from(row.2).map_err(|_| Error::Invariant("bad index"))?,
            is_buy: row.3 == "buy",
            price: row.4,
            quantity: row.5,
            kind: row.6,
            client_request_id: row.7,
            reduce_only: row.8 != 0,
            cloid: row
                .9
                .try_into()
                .map_err(|_| Error::Invariant("expected a 16-byte cloid"))?,
            created_at: u64::try_from(row.10).map_err(|_| Error::Invariant("bad time"))?,
            effective_leverage: u32::try_from(row.14)
                .map_err(|_| Error::Invariant("bad leverage"))?,
            expires_after: row
                .15
                .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("bad expiry")))
                .transpose()?,
            preflight_state: crate::states::action_state_from_str(&row.16)
                .ok_or(Error::Invariant("unknown preflight state"))?,
            preflight_nonce: u64::try_from(
                row.17
                    .ok_or(Error::Invariant("preflight nonce not allocated"))?,
            )
            .map_err(|_| Error::Invariant("bad nonce"))?,
            order_nonce: u64::try_from(
                row.18
                    .ok_or(Error::Invariant("order nonce not allocated"))?,
            )
            .map_err(|_| Error::Invariant("bad nonce"))?,
            trigger: match trigger_from_columns(row.11, row.12, row.13)? {
                Some(trigger) => Some(OrderTrigger {
                    kind: trigger_kind_str(trigger.kind).to_string(),
                    price: trigger.trigger_price,
                    is_market: trigger.is_market,
                }),
                None => None,
            },
        })
    })
    .transpose()
}

/// 同一Agentのaction nonceを一度だけ確保する。lease再取得時も同じnonceを使う。
pub fn ensure_action_nonces(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    let existing = connection
        .query_optional(
            "SELECT preflight_nonce, order_nonce FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| Ok((row.get::<Option<i64>>(0)?, row.get::<Option<i64>>(1)?)),
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    if existing.0.is_some() && existing.1.is_some() {
        return Ok(());
    }
    if existing.0.is_some() || existing.1.is_some() {
        return Err(Error::Invariant("partially allocated action nonces"));
    }
    let agent = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT agent_address FROM agent_generations
              WHERE account_id = (SELECT account_id FROM orders WHERE order_id = ?1)
              ORDER BY generation DESC LIMIT 1",
            params![order_id.as_slice()],
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let last = connection
        .query_optional_scalar::<i64>(
            "SELECT last_nonce FROM nonces WHERE agent_address = ?1",
            params![agent.as_slice()],
        )
        .map_err(sql)?;
    let first = last
        .unwrap_or(0)
        .saturating_add(1)
        .max(i64::try_from(now).map_err(|_| Error::Overflow)?);
    let second = first.checked_add(1).ok_or(Error::Overflow)?;
    connection
        .execute(
            "INSERT INTO nonces (agent_address, last_nonce) VALUES (?1, ?2)
             ON CONFLICT(agent_address) DO UPDATE SET last_nonce = excluded.last_nonce",
            params![agent.as_slice(), second],
        )
        .map_err(sql)?;
    connection
        .execute(
            "UPDATE orders SET preflight_nonce = ?2, order_nonce = ?3 WHERE order_id = ?1",
            params![order_id.as_slice(), first, second],
        )
        .map_err(sql)
}

/// leverage更新を送る直前に署名済みpayloadを永続化する。
pub fn mark_preflight_dispatching(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    wire_payload: &[u8],
    signature: &[u8],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders
                SET preflight_state = 'dispatching', preflight_wire_payload = ?3,
                    preflight_signature = ?4, updated_at = ?5
              WHERE order_id = ?1 AND worker_epoch = ?2 AND dispatch_state = 'signing'
                AND preflight_state = 'queued'",
            params![
                order_id.as_slice(),
                worker_epoch as i64,
                wire_payload,
                signature,
                now as i64
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "queued preflight",
        "claim lost",
    )
}

pub fn mark_preflight_applied(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET preflight_state = 'reconciled', updated_at = ?3
              WHERE order_id = ?1 AND worker_epoch = ?2 AND dispatch_state = 'signing'
                AND preflight_state = 'dispatching'",
            params![order_id.as_slice(), worker_epoch as i64, now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "dispatching preflight",
        "claim lost",
    )?;
    let (account_id, asset_index, leverage) = leverage_target(connection, order_id)?;
    crate::repo::leverage::confirmed(
        connection,
        order_id,
        &account_id,
        asset_index,
        leverage,
        now,
    )
}

/// 確認済みの同一設定を使い、注文のpreflightだけを省く。
pub fn mark_preflight_skipped(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET preflight_state = 'reconciled', updated_at = ?3
             WHERE order_id = ?1 AND worker_epoch = ?2 AND dispatch_state = 'signing'
               AND preflight_state = 'queued'",
            params![order_id.as_slice(), worker_epoch as i64, now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "queued preflight",
        "claim lost",
    )
}

fn leverage_target(
    connection: &Connection,
    order_id: &[u8; 32],
) -> Result<([u8; 32], u32, u32), Error> {
    let row = connection
        .query_optional(
            "SELECT account_id, asset_index, effective_leverage FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<i64>(1)?,
                    row.get::<i64>(2)?,
                ))
            },
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    Ok((
        row.0
            .try_into()
            .map_err(|_| Error::Invariant("bad leverage account"))?,
        u32::try_from(row.1).map_err(|_| Error::Invariant("bad leverage asset"))?,
        u32::try_from(row.2).map_err(|_| Error::Invariant("bad leverage value"))?,
    ))
}

/// leverage更新が拒否または不明になった場合は注文本体を送らず停止する。
pub fn stop_after_preflight(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    rejected: bool,
    error: &str,
    now: u64,
) -> Result<(), Error> {
    let preflight = if rejected { "reconciled" } else { "unknown" };
    let order_state = if rejected { "rejected" } else { "unknown" };
    let dispatch = if rejected { "aborted" } else { "unknown" };
    connection
        .execute(
            "UPDATE orders
                SET preflight_state = ?3, state = ?4, dispatch_state = ?5,
                    lease_until = NULL, last_error = ?6, updated_at = ?7
              WHERE order_id = ?1 AND worker_epoch = ?2 AND dispatch_state = 'signing'
                AND preflight_state = 'dispatching'",
            params![
                order_id.as_slice(),
                worker_epoch as i64,
                preflight,
                order_state,
                dispatch,
                error,
                now as i64
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "dispatching preflight",
        "claim lost",
    )?;
    let (account_id, asset_index, _) = leverage_target(connection, order_id)?;
    if rejected {
        crate::repo::leverage::rejected(connection, order_id, &account_id, asset_index)?;
    } else {
        crate::repo::leverage::unresolved(connection, order_id, &account_id, asset_index)?;
    }
    if rejected {
        release_risk_for_order(connection, order_id, now)?;
    }
    Ok(())
}

/// controllerが取引所で確認した結果に基づき、不明preflightを解決する。
pub fn resolve_unknown_preflight(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    applied: bool,
    actor: &[u8],
    now: u64,
) -> Result<(), Error> {
    let (state, dispatch, outcome, message) = if applied {
        (
            "pending",
            "queued",
            "applied",
            "preflight manually confirmed",
        )
    } else {
        (
            "rejected",
            "aborted",
            "rejected",
            "preflight manually rejected",
        )
    };
    connection
        .execute(
            "UPDATE orders
                SET preflight_state = 'reconciled', state = ?2, dispatch_state = ?3,
                    lease_until = NULL, next_check_at = NULL, last_error = ?4,
                    updated_at = ?5
              WHERE order_id = ?1 AND preflight_state = 'unknown'
                AND dispatch_state = 'unknown'",
            params![order_id.as_slice(), state, dispatch, message, now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "unknown preflight",
        "not unresolved",
    )?;
    let (account_id, asset_index, leverage) = leverage_target(connection, order_id)?;
    crate::repo::leverage::resolve_legacy_or_pending(
        connection,
        order_id,
        &account_id,
        asset_index,
        leverage,
        applied,
        now,
    )?;
    connection
        .execute(
            "INSERT INTO order_resolution_events (order_id, actor, outcome, at)
             VALUES (?1, ?2, ?3, ?4)",
            params![order_id.as_slice(), actor, outcome, now as i64],
        )
        .map_err(sql)?;
    if !applied {
        release_risk_for_order(connection, order_id, now)?;
    }
    Ok(())
}

/// 取引所へ状態を問い合わせる注文のoid（未終端でoidが分かっているもの）。
///
/// 結果不明（`unknown`）でもoidがあれば照合で解消できるため対象に含める。
pub fn oids_awaiting_status(
    connection: &Connection,
    account_id: &[u8; 32],
    limit: u32,
) -> Result<Vec<u64>, Error> {
    let rows = connection
        .query_all(
            "SELECT hl_oid FROM orders
              WHERE NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='order' AND w.work_id=orders.order_id AND w.allowed=0) AND account_id = ?1 AND hl_oid IS NOT NULL
                AND NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='cancel' AND w.work_id=orders.order_id AND w.allowed=0 AND orders.cancel_dispatch_state IN ('dispatching','unknown'))
                AND state IN ('open', 'partially_filled', 'unknown')
              ORDER BY rowid LIMIT ?2",
            params![account_id.as_slice(), limit as i64],
            |row| row.get::<i64>(0),
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|oid| u64::try_from(oid).map_err(|_| Error::Invariant("bad oid")))
        .collect()
}

/// 送信済みか不明な取消は再送せず、一度分の照合許可を消費する。
pub fn cancel_unresolved(connection: &Connection, order_id: &[u8; 32]) -> Result<bool, Error> {
    Ok(connection.query_optional_scalar::<i64>(
        "SELECT 1 FROM orders WHERE order_id=?1 AND cancel_dispatch_state IN ('dispatching','unknown') AND state IN ('open','partially_filled','unknown')",
        params![order_id.as_slice()],
    ).map_err(sql)?.is_some())
}

/// 取消送信の対象（取消要求済みで未送信、`hl_oid`既知の未終端注文）。
pub fn cancel_candidates(
    connection: &Connection,
    now: u64,
    limit: u32,
) -> Result<Vec<[u8; 32]>, Error> {
    let rows = connection
        .query_all(
            "SELECT order_id FROM orders
              WHERE NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='cancel' AND w.work_id=orders.order_id AND w.allowed=0) AND cancel_requested = 1
                AND (cancel_dispatch_state IS NULL
                  OR (cancel_dispatch_state = 'signing' AND cancel_lease_until < ?1))
                AND hl_oid IS NOT NULL AND state IN ('open', 'partially_filled')
              ORDER BY updated_at LIMIT ?2",
            params![now as i64, limit as i64],
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

/// 取消の送信権を取得する（単一実行者）。
pub fn claim_cancel(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    now: u64,
    lease_ms: u64,
) -> Result<Option<(u64, u64)>, Error> {
    let lease_until = now.saturating_add(lease_ms);
    connection
        .execute(
            "UPDATE orders
                SET cancel_dispatch_state = 'signing',
                    cancel_worker_epoch = cancel_worker_epoch + 1,
                    cancel_lease_until = ?2, cancel_attempt = cancel_attempt + 1,
                    updated_at = ?3
              WHERE order_id = ?1 AND cancel_requested = 1
                AND (cancel_dispatch_state IS NULL
                  OR (cancel_dispatch_state = 'signing' AND cancel_lease_until < ?3))",
            params![order_id.as_slice(), lease_until as i64, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        return Ok(None);
    }
    let row = connection
        .query_optional(
            "SELECT cancel_worker_epoch, cancel_nonce,
                    (SELECT agent_address FROM agent_generations
                      WHERE account_id = orders.account_id
                      ORDER BY generation DESC LIMIT 1)
               FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<i64>(0)?,
                    row.get::<Option<i64>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                ))
            },
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let nonce = match row.1 {
        Some(value) => value,
        None => {
            let last = connection
                .query_optional_scalar::<i64>(
                    "SELECT last_nonce FROM nonces WHERE agent_address = ?1",
                    params![row.2.as_slice()],
                )
                .map_err(sql)?;
            let next = last
                .unwrap_or(0)
                .saturating_add(1)
                .max(i64::try_from(now).map_err(|_| Error::Overflow)?);
            connection
                .execute(
                    "INSERT INTO nonces (agent_address, last_nonce) VALUES (?1, ?2)
                     ON CONFLICT(agent_address) DO UPDATE SET last_nonce = excluded.last_nonce",
                    params![row.2.as_slice(), next],
                )
                .map_err(sql)?;
            connection
                .execute(
                    "UPDATE orders SET cancel_nonce = ?2 WHERE order_id = ?1",
                    params![order_id.as_slice(), next],
                )
                .map_err(sql)?;
            next
        }
    };
    Ok(Some((
        u64::try_from(row.0).map_err(|_| Error::Invariant("bad cancel epoch"))?,
        u64::try_from(nonce).map_err(|_| Error::Invariant("bad cancel nonce"))?,
    )))
}

pub fn retry_cancel_before_dispatch(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    error: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET cancel_dispatch_state = NULL, cancel_lease_until = NULL,
                    last_error = ?3, updated_at = ?4
              WHERE order_id = ?1 AND cancel_worker_epoch = ?2
                AND cancel_dispatch_state = 'signing'",
            params![order_id.as_slice(), worker_epoch as i64, error, now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "claimed cancel epoch",
        "claim lost",
    )
}

/// 取消payloadをPOST前に永続化する。以後は結果不明でも自動再送しない。
pub fn mark_cancel_dispatching(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    wire_payload: &[u8],
    signature: &[u8],
    worker_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders
                SET cancel_dispatch_state = 'dispatching', cancel_wire_payload = ?2,
                    cancel_signature = ?3, updated_at = ?5
              WHERE order_id = ?1 AND cancel_dispatch_state = 'signing'
                AND cancel_worker_epoch = ?4",
            params![
                order_id.as_slice(),
                wire_payload,
                signature,
                worker_epoch as i64,
                now as i64
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "claimed cancel epoch",
        "claim lost",
    )
}

/// 取消を送信済みにする（受理時は`cancelled`へ倒す。HLの最終状態は`orderStatus`照合で確認）。
pub fn mark_cancel_sent(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET cancel_dispatch_state = 'sent', cancel_lease_until = NULL,
                    state = 'cancelled', updated_at = ?3
              WHERE order_id = ?1 AND cancel_dispatch_state = 'dispatching'
                AND cancel_worker_epoch = ?2",
            params![order_id.as_slice(), worker_epoch as i64, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "dispatching", "not claimed")?;
    // 取消済みとして扱う注文の予約は解放する（照合の結果で状態は修正され得る）。
    release_risk_for_order(connection, order_id, now)
}

/// 取消の送信結果が不明。
pub fn mark_cancel_unknown(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    worker_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders SET cancel_dispatch_state = 'unknown', cancel_lease_until = NULL,
                    updated_at = ?3
              WHERE order_id = ?1 AND cancel_dispatch_state = 'dispatching'
                AND cancel_worker_epoch = ?2",
            params![order_id.as_slice(), worker_epoch as i64, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "dispatching", "not claimed")
}

/// POST待機中にupgrade/callback消失が起きた処理を、再送禁止の`unknown`へ倒す。
pub fn recover_expired_dispatches(
    connection: &mut UpdateConnection<'_>,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders
                SET preflight_state = 'unknown', state = 'unknown', dispatch_state = 'unknown',
                    lease_until = NULL, last_error = 'preflight callback was not observed',
                    updated_at = ?1
              WHERE dispatch_state = 'signing' AND preflight_state = 'dispatching'
                AND (lease_until IS NULL OR lease_until < ?1)",
            params![now as i64],
        )
        .map_err(sql)?;
    connection
        .execute(
            "UPDATE leverage_cache SET pending_state = 'unknown'
              WHERE pending_state = 'reserved' AND EXISTS (
                  SELECT 1 FROM orders
                   WHERE orders.order_id = leverage_cache.pending_order_id
                     AND orders.preflight_state = 'unknown'
              )",
            &[],
        )
        .map_err(sql)?;
    connection
        .execute(
            "UPDATE orders
                SET state = 'unknown', dispatch_state = 'unknown', lease_until = NULL,
                    last_error = 'order callback was not observed', updated_at = ?1
              WHERE dispatch_state = 'dispatching'
                AND (lease_until IS NULL OR lease_until < ?1)",
            params![now as i64],
        )
        .map_err(sql)?;
    connection
        .execute(
            "UPDATE orders
                SET cancel_dispatch_state = 'unknown', cancel_lease_until = NULL,
                    last_error = 'cancel callback was not observed', updated_at = ?1
              WHERE cancel_dispatch_state = 'dispatching'
                AND (cancel_lease_until IS NULL OR cancel_lease_until < ?1)",
            params![now as i64],
        )
        .map_err(sql)
}

/// 取消に必要な注文情報（口座・銘柄添字・取引所oid）。
pub fn cancel_target(
    connection: &Connection,
    order_id: &[u8; 32],
) -> Result<Option<([u8; 32], u32, u64)>, Error> {
    let row = connection
        .query_optional(
            "SELECT account_id, asset_index, hl_oid FROM orders WHERE order_id = ?1",
            params![order_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<i64>(1)?,
                    row.get::<Option<i64>>(2)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(account_id, asset_index, oid)| {
        Ok((
            account_id
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
            u32::try_from(asset_index).map_err(|_| Error::Invariant("bad index"))?,
            u64::try_from(oid.ok_or(Error::Invariant("cancel without venue oid"))?)
                .map_err(|_| Error::Invariant("bad oid"))?,
        ))
    })
    .transpose()
}

/// 利用者の未終端注文すべてに取消要求を付ける（送信はsweepが行う）。
pub fn mark_all_cancel_requested(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    now: u64,
) -> Result<u64, Error> {
    connection
        .execute(
            "UPDATE orders SET cancel_requested = 1, updated_at = ?2
              WHERE user_id = ?1 AND state IN ('open', 'partially_filled') AND cancel_requested = 0",
            params![user_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    u64::try_from(changed).map_err(|_| Error::Invariant("negative change count"))
}

/// 口座snapshotの改訂番号（利用者の注文・約定の追加で単調に増える）。
///
/// キャッシュ残高を持たないため、DBの追加行を改訂の指標にする。
pub fn account_revision(connection: &Connection, user_id: &[u8; 32]) -> Result<u64, Error> {
    let value = connection
        .query_scalar::<i64>(
            "SELECT (SELECT COALESCE(MAX(rowid), 0) FROM orders WHERE user_id = ?1)
                  + (SELECT COALESCE(MAX(rowid), 0) FROM fills WHERE user_id = ?1)",
            params![user_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(value).map_err(|_| Error::Invariant("negative revision"))
}

/// 最後に取引所の約定を取り込んだ時刻（鮮度の指標）。
pub fn latest_fill_at(connection: &Connection, user_id: &[u8; 32]) -> Result<Option<u64>, Error> {
    // `MAX` は行が無いと NULL を返すため、nullable な行として読む。
    let row = connection
        .query_optional(
            "SELECT MAX(filled_at) FROM fills WHERE user_id = ?1",
            params![user_id.as_slice()],
            |row| row.get::<Option<i64>>(0),
        )
        .map_err(sql)?;
    match row.flatten() {
        Some(value) => u64::try_from(value)
            .map(Some)
            .map_err(|_| Error::Invariant("negative timestamp")),
        None => Ok(None),
    }
}

/// 解決済み注文の機密payloadをredactし、期限を過ぎた詳細約定を少量ずつ削除する。
pub fn prune_terminal_history(
    connection: &mut UpdateConnection<'_>,
    redact_before: u64,
    delete_before: u64,
    limit: u32,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE orders
                SET wire_payload = NULL, signature = NULL,
                    preflight_wire_payload = NULL, preflight_signature = NULL,
                    cancel_wire_payload = NULL, cancel_signature = NULL
              WHERE rowid IN (
                    SELECT rowid FROM orders
                     WHERE updated_at < ?1
                       AND state IN ('filled', 'cancelled', 'rejected')
                       AND dispatch_state IN ('reconciled', 'aborted')
                       AND (cancel_dispatch_state IS NULL OR cancel_dispatch_state != 'unknown')
                       AND (wire_payload IS NOT NULL OR signature IS NOT NULL
                         OR preflight_wire_payload IS NOT NULL OR preflight_signature IS NOT NULL
                         OR cancel_wire_payload IS NOT NULL OR cancel_signature IS NOT NULL)
                     ORDER BY updated_at LIMIT ?2)",
            params![redact_before as i64, limit as i64],
        )
        .map_err(sql)?;
    connection
        .execute(
            "DELETE FROM fills WHERE fill_id IN (
                SELECT fill_id FROM fills WHERE filled_at < ?1 ORDER BY filled_at LIMIT ?2)",
            params![delete_before as i64, limit as i64],
        )
        .map_err(sql)
}

/// Unknown sends retain their preassigned cloid even when the POST response is lost.
pub fn unknown_cloids(c: &Connection, account_id: &[u8; 32]) -> Result<Vec<Vec<u8>>, Error> {
    c.query_all("SELECT cloid FROM orders WHERE NOT EXISTS(SELECT 1 FROM worker_permissions w WHERE w.kind='order' AND w.work_id=orders.order_id AND w.allowed=0) AND account_id=?1 AND hl_oid IS NULL AND state='unknown' AND preflight_state='reconciled' ORDER BY updated_at LIMIT 4", params![account_id.as_slice()], |r| r.get(0)).map_err(sql)
}
pub fn cloid_status_target(
    c: &Connection,
    account_id: &[u8; 32],
    cloid: &[u8],
) -> Result<Option<([u8; 32], String)>, Error> {
    c.query_optional("SELECT order_id,state FROM orders WHERE account_id=?1 AND cloid=?2 AND hl_oid IS NULL AND state='unknown'",params![account_id.as_slice(),cloid],|r| Ok((r.get::<Vec<u8>>(0)?,r.get::<String>(1)?))).map_err(sql)?.map(|(id,state)| Ok((id.try_into().map_err(|_| Error::Conflict)?,state))).transpose()
}
pub fn bind_observed_oid(
    c: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    order_id: &[u8; 32],
    oid: u64,
) -> Result<(), Error> {
    let oid = i64::try_from(oid).map_err(|_| Error::Overflow)?;
    if c.query_optional_scalar::<i64>(
        "SELECT 1 FROM orders WHERE account_id=?1 AND hl_oid=?2 AND order_id!=?3",
        params![account_id.as_slice(), oid, order_id.as_slice()],
    )
    .map_err(sql)?
    .is_some()
    {
        return Err(Error::Conflict);
    }
    c.execute("UPDATE orders SET hl_oid=?3 WHERE account_id=?1 AND order_id=?2 AND hl_oid IS NULL AND state='unknown'",params![account_id.as_slice(),order_id.as_slice(),oid]).map_err(sql)?;
    Ok(())
}

pub fn note_cloid_check(
    c: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    cloid: &[u8],
    now: u64,
) -> Result<(), Error> {
    c.execute("UPDATE orders SET updated_at=MAX(updated_at+1,?3) WHERE account_id=?1 AND cloid=?2 AND state='unknown'",params![account_id.as_slice(),cloid,now as i64]).map_err(sql)?;
    Ok(())
}

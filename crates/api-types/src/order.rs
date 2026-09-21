//! 注文。`docs/phase-0/api-contract.md` 3節、`state-machines.md` 4節。

use crate::auth::SessionHandle;
use crate::fund::ActionState;
use crate::{Blob, Micros, Timestamp};
use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// 注文種別。Marketはスリッページ上限付きIOC指値として構築する。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderKind {
    MarketIoc,
    LimitGtc,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    StopLoss,
    TakeProfit,
}

/// 建玉単位のSL/TP（HL `positionTpsl`、reduce-only）。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub trigger_price: String,
    pub is_market: bool,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SubmitOrderArgs {
    pub session: SessionHandle,
    pub client_request_id: Blob,
    pub account_id: Blob,
    pub market: String,
    pub side: Side,
    pub kind: OrderKind,
    /// 正規化十進文字列。
    pub quantity: String,
    /// Marketはスリッページ上限付きIOC指値のため必須。Limitでは上限価格。
    pub limit_price: Option<String>,
    pub slippage_tolerance_bps: Option<u32>,
    pub reduce_only: bool,
    pub leverage: Option<u32>,
    pub trigger: Option<Trigger>,
    pub expires_after: Option<Timestamp>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SubmitOrderResult {
    pub request_id: Blob,
    pub order_id: Blob,
    pub cloid: Blob,
    pub accepted_at: Timestamp,
}

/// `close_all` の結果（建玉ごとの受付結果と、受付できなかった銘柄）。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CloseAllOutcome {
    pub submitted: Vec<SubmitOrderResult>,
    pub failed: Vec<CloseFailure>,
}

/// 決済できなかった銘柄とその理由。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CloseFailure {
    pub market: String,
    pub error: crate::error::ErrorCode,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CancelOrderArgs {
    pub session: SessionHandle,
    pub client_request_id: Blob,
    pub order_id: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CancelAllArgs {
    pub session: SessionHandle,
    pub client_request_id: Blob,
    pub market: Option<String>,
    /// 保護用SL/TPも取り消す場合はtrue（UIで確認を必須とする）。
    pub include_protective_orders: bool,
}

/// 注文ライフサイクル（`state-machines.md` 4節）。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderState {
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Unknown,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OrderView {
    pub order_id: Blob,
    pub cloid: Option<Blob>,
    pub market: String,
    pub side: Side,
    pub kind: OrderKind,
    pub price: Option<String>,
    pub quantity: String,
    /// 発注数量を約定数量で上書きしない。
    pub filled_quantity: String,
    pub state: OrderState,
    pub venue_state: Option<String>,
    pub hl_oid: Option<u64>,
    pub cancel_requested: bool,
    /// SL/TPトリガ（`None`は通常注文）。
    pub trigger: Option<Trigger>,
    pub updated_at: Timestamp,
}

/// 受付済みでHL未受理の可能性がある注文。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PendingOrderView {
    pub request_id: Blob,
    pub cloid: Option<Blob>,
    pub order_id: Option<Blob>,
    pub action_state: ActionState,
    pub since: Timestamp,
    pub last_error: Option<String>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PositionView {
    pub market: String,
    pub size: String,
    pub entry_price: String,
    pub liquidation_price: Option<String>,
    pub unrealized_pnl: i64,
    pub leverage: u32,
    pub margin_mode: String,
    pub stop_loss: Option<String>,
    pub take_profit: Option<String>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AccountSnapshot {
    pub account_id: Blob,
    pub equity: Micros,
    pub margin_used: Micros,
    pub withdrawable: Micros,
    pub unrealized_pnl: i64,
    pub positions: Vec<PositionView>,
    pub open_orders: Vec<OrderView>,
    pub pending_orders: Vec<PendingOrderView>,
    pub observed_at: Timestamp,
    pub revision: u64,
    pub data_age_ms: u64,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FillView {
    pub order_id: Blob,
    pub cloid: Option<Blob>,
    pub market: String,
    pub price: String,
    pub quantity: String,
    pub fee: Micros,
    pub at: Timestamp,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SnapshotQuery {
    pub session: SessionHandle,
    pub account_id: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ListQuery {
    pub session: SessionHandle,
    pub account_id: Blob,
    pub cursor: Option<Blob>,
    pub limit: u32,
}

/// 注文一覧の1件（`list_orders`）。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OrderSummary {
    pub order_id: Blob,
    pub cloid: Blob,
    pub market: String,
    pub asset_index: u32,
    pub is_buy: bool,
    pub kind: String,
    pub price: Option<String>,
    pub quantity: String,
    pub filled_quantity: String,
    pub reduce_only: bool,
    pub state: OrderState,
    /// outboxの送信状態（受付から照合までの進み具合）。
    pub dispatch_state: ActionState,
    pub cancel_requested: bool,
    pub hl_oid: Option<u64>,
    /// SL/TPトリガ（`None`は通常注文）。
    pub trigger: Option<Trigger>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

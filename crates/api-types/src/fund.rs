//! 資金・Agent。`docs/phase-0/api-contract.md` 2節、`state-machines.md` 2〜3節。

use crate::auth::SessionHandle;
use crate::{AccountKind, AssetId, Blob, Micros, Network, Timestamp};
use candid::CandidType;
use serde::{Deserialize, Serialize};

/// action状態（`docs/phase-0/state-machines.md` 2節）。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionState {
    Queued,
    Signing,
    Signed,
    Dispatching,
    Reconciled,
    Unknown,
    Aborted,
}

impl ActionState {
    /// 受付前（未送信を保証できる）状態か。
    pub fn is_pre_dispatch(self) -> bool {
        matches!(self, Self::Queued | Self::Signing | Self::Signed)
    }

    /// 送信済みの可能性があり、自動再送してはならない状態か。
    pub fn is_post_dispatch(self) -> bool {
        matches!(self, Self::Dispatching | Self::Reconciled | Self::Unknown)
    }
}

/// 資金要求の状態（`state-machines.md` 3節）。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FundRequestState {
    Accepted,
    Reserved,
    Executing,
    Settled,
    Rejected,
    Unknown,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FundActionKind {
    Allocation,
    Recovery,
    Withdrawal,
    AgentApproval,
    AgentRevocation,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FundingInstructions {
    pub account_kind: AccountKind,
    pub hl_account_address: Blob,
    /// Only transfers from this authenticated HL address are automatically attributed.
    pub source_hl_account_address: Blob,
    pub asset: AssetId,
    pub network: Network,
    /// 最小額。Phase 1の実測で確定するまでは `None`。
    pub minimum_amount: Option<Micros>,
    pub memo_required: bool,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AllocationRequest {
    pub session: SessionHandle,
    pub client_request_id: Blob,
    pub amount: Micros,
    pub target: AccountKind,
    pub intent_signature: Option<Blob>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalRequest {
    pub session: SessionHandle,
    pub client_request_id: Blob,
    pub amount: Micros,
    pub asset: AssetId,
    /// 初期の出金先は認証EOAのHL口座のみ。
    pub destination: Destination,
    pub network: Network,
    pub nonce: u64,
    pub expires_at: Timestamp,
    pub intent_signature: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    AuthenticatedEoaHlAccount,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FundRequestAccepted {
    pub request_id: Blob,
    /// 送信するactionのID。wire payloadを構築する段階（署名時）に確定するため未確定は`None`。
    pub fund_action_id: Option<Blob>,
    pub state: FundRequestState,
    pub accepted_at: Timestamp,
}

/// 未解決のaction。出金可能額へ算入しない。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedAction {
    pub action_id: Blob,
    pub kind: FundActionKind,
    pub state: ActionState,
    pub since: Timestamp,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FundStatus {
    pub reserve_unallocated: Micros,
    pub in_transit: Micros,
    pub reserved_for_withdrawal: Micros,
    pub trading_equity: Micros,
    pub trading_unrealized_pnl: i64,
    pub withdrawable: Micros,
    pub observed_at: Timestamp,
    pub revision: u64,
    pub unknowns: Vec<UnresolvedAction>,
    pub recovery_fence: Option<RecoveryFenceStatus>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryFenceStatus {
    Preparing,
    Reconciling,
}

/// 資金履歴の1件。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FundEvent {
    pub event_id: Blob,
    pub kind: FundActionKind,
    pub amount: Micros,
    pub state: FundRequestState,
    pub at: Timestamp,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FundEventsQuery {
    pub session: SessionHandle,
    pub cursor: Option<Blob>,
    pub limit: u32,
}

/// Agent世代の状態（`Implementation.md` 7章）。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Requested,
    Approving,
    Active,
    Expiring,
    Revoked,
    Failed,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentGeneration {
    pub account_id: Blob,
    pub generation: u64,
    pub agent_address: Blob,
    pub approved_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub state: AgentState,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentStatus {
    pub current: Option<AgentGeneration>,
    pub next: Option<AgentGeneration>,
    pub revocation_pending: bool,
    pub observed_at: Timestamp,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRevocationScope {
    StopNewOrders,
    RevokeCurrentAndNext,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentRevocationRequest {
    pub session: SessionHandle,
    pub client_request_id: Blob,
    pub scope: AgentRevocationScope,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentGenerationRequest {
    pub session: SessionHandle,
    pub client_request_id: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AgentStatusQuery {
    pub session: SessionHandle,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FundStatusQuery {
    pub session: SessionHandle,
}

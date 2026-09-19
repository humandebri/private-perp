//! 認証（challenge・セッション）。`docs/phase-0/api-contract.md` 2.1。

use crate::{Blob, Network, Timestamp};
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

/// challengeの用途。`withdrawal` のchallengeはセッション確立に使えない。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChallengePurpose {
    Login,
    Withdrawal,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChallengeRequest {
    /// EOAアドレス（20バイト）。
    pub eoa_address: Blob,
    /// ブラウザが生成する短命IC署名IdentityのPrincipal。
    pub principal: Principal,
    pub purpose: ChallengePurpose,
    pub network: Network,
    pub origin: String,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChallengeResponse {
    pub challenge_id: Blob,
    /// EIP-712 typed data（origin・network・canister・用途・nonce・期限を含む）。
    pub typed_data: Blob,
    pub nonce: Blob,
    pub expires_at: Timestamp,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenSessionRequest {
    pub challenge_id: Blob,
    /// 65バイトのEOA署名。
    pub eoa_signature: Blob,
}

/// 失効世代付きセッション。`trading_core` はvaultが発行したものだけを受け入れる。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SessionHandle {
    pub session_id: Blob,
    pub vault_principal: Principal,
    pub expires_at: Timestamp,
    pub revocation_generation: u64,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RevokeSessionRequest {
    pub session: SessionHandle,
}

//! HPKE封筒（`docs/phase-0/api-contract.md` 6節）。
//!
//! 個人APIの要求・応答は封筒で包む。封筒は機密性と`aad`束縛を与えるが、
//! 本人認証・再送防止の代わりではない（セッションと`request_id`の検証を別に行う）。

use crate::auth::SessionHandle;
use crate::{Blob, Network, Timestamp};
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

/// 個人APIの要求封筒。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HpkeRequest {
    /// サーバ公開鍵のID（現行世代の公開鍵そのもの）。
    pub key_id: Blob,
    pub network: Network,
    /// 呼び出し先のcanister principal（別canisterへの転用を拒否する）。
    pub canister: Principal,
    pub method: String,
    /// 要求の単回使用ID（再送は拒否する）。
    pub request_id: Blob,
    pub expires_at: Timestamp,
    /// 応答を暗号化するブラウザ公開鍵。
    pub client_public_key: Blob,
    /// `network`・`canister`・`method`・`caller`・`request_id`・期限を束縛する。
    pub aad: Blob,
    /// ChaCha20-Poly1305の暗号文（`enc || ciphertext`）。
    pub ciphertext: Blob,
}

/// 個人APIの応答封筒。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HpkeResponse {
    pub request_id: Blob,
    pub key_id: Blob,
    pub observed_at: Timestamp,
    /// `client_public_key`宛の暗号文（`enc || ciphertext`）。
    pub ciphertext: Blob,
}

/// `get_account_snapshot` の平文。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SnapshotQuery {
    pub session: SessionHandle,
}

/// `list_orders`・`list_fills` の平文。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ListQuery {
    pub session: SessionHandle,
    pub cursor: Option<Blob>,
    pub limit: u32,
}

/// `cancel_order` の平文。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CancelOrderQuery {
    pub session: SessionHandle,
    pub order_id: Blob,
}

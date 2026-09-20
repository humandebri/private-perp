//! 政策（allowlist・停止）の型。`docs/phase-0/api-contract.md` 5節。

use crate::Timestamp;
use candid::CandidType;
use serde::{Deserialize, Serialize};

/// 現在の政策。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub version: u64,
    /// 取引を許可する銘柄（初期は BTC・ETH perpsのみ）。
    pub markets: Vec<String>,
}

/// 停止状態（理由コードのみを公開する）。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StopStatus {
    pub stopped: bool,
    pub reason: Option<String>,
    pub since: Option<Timestamp>,
}

//! 変更予約と7日猶予。`docs/phase-0/api-contract.md` 4節。

use crate::Blob;
use crate::Timestamp;
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UpgradeRequest {
    pub target: Principal,
    /// 32バイトのWASMモジュールハッシュ。
    pub wasm_hash: Blob,
    pub arg_hash: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeState {
    Pending,
    Executable,
    Executed,
    Cancelled,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ScheduledUpgrade {
    pub request: UpgradeRequest,
    pub scheduled_at: Timestamp,
    /// `scheduled_at + 7日`。予約内容を変更する場合は取消＋新規予約とし、新しい猶予を開始する。
    pub executable_at: Timestamp,
    pub state: UpgradeState,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UpgradeStatus {
    pub scheduled: Option<ScheduledUpgrade>,
    pub guard_version: String,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ScheduleUpgradeArgs {
    pub request: UpgradeRequest,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CancelUpgradeArgs {
    pub target: Principal,
}

/// 猶予期間（秒）。`docs/phase-0/api-contract.md` 4節。
pub const UPGRADE_DELAY_SECONDS: u64 = 7 * 24 * 60 * 60;

/// 猶予期間（ミリ秒）。
pub const UPGRADE_DELAY_MS: u64 = UPGRADE_DELAY_SECONDS * 1000;

//! 内部Canister間の共有予算。個人識別子を公開状態へ含めない。

use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Local budget ceiling also bounds the live rows scanned in a rolling window.
pub const MAX_REST_BUDGET_CAPACITY: u32 = 10_000;
pub const REST_BUDGET_WINDOW_MS: u64 = 60_000;

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetClass {
    NewRisk,
    Exit,
    Reconcile,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RestBudgetConfig {
    pub capacity: u32,
    pub exit_reserve: u32,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RestBudgetRequest {
    /// 消費試行ごとの32-byte ID。先頭8 bytesはexpires_atのbig-endian表現。
    /// 残り24 bytesは試行ごとに一意。期限を書き換えて同じIDを再取得できない。
    pub request_id: crate::Blob,
    pub class: BudgetClass,
    pub weight: u32,
    /// 予算取得と送信開始の共通期限（exclusive、最大60秒）。
    /// consume_rest_budgetのOkを確認したworkerだけが、期限前に1回送信できる。
    /// workerは送信直前にvalid_atと業務状態を再検証し、awaitを挟まず送信開始する。
    /// 期限切れの許可は使わない。計上はこの期限の60秒後まで保持する（未送信でも返金なし）。
    pub expires_at: crate::Timestamp,
}

impl RestBudgetRequest {
    /// 取得時およびOk確認後の送信直前に使う。これ自体は予算取得の証明ではない。
    pub fn valid_at(&self, now: crate::Timestamp) -> bool {
        self.request_id.len() == 32
            && self.request_id[..8] == self.expires_at.to_be_bytes()
            && self.weight > 0
            && self.expires_at > now
            && self.expires_at - now <= REST_BUDGET_WINDOW_MS
    }
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RestBudgetStatus {
    pub config: Option<RestBudgetConfig>,
    pub used: u32,
    pub new_risk_used: u32,
    pub recovery_paused: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_request_binds_its_expiry_and_bounds_validity() {
        let expiry = 61_000u64;
        let mut id = vec![1; 32];
        id[..8].copy_from_slice(&expiry.to_be_bytes());
        let mut request = RestBudgetRequest {
            request_id: id.into(),
            class: BudgetClass::Exit,
            weight: 1,
            expires_at: expiry,
        };
        assert!(request.valid_at(1_000));
        assert!(request.valid_at(60_999));
        assert!(!request.valid_at(999));
        assert!(!request.valid_at(61_000));
        request.expires_at += 1;
        assert!(!request.valid_at(1_001));
        request.expires_at = expiry;
        request.weight = 0;
        assert!(!request.valid_at(1_000));
        request.request_id = vec![0; 7].into();
        assert!(!request.valid_at(1_000));
    }
}

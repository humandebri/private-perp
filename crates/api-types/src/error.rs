//! 共通エラー型。`docs/phase-0/api-contract.md` 7節。

use candid::CandidType;
use serde::{Deserialize, Serialize};

/// 入力不正の理由コード。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadRequestCode {
    MalformedPayload,
    TooLarge,
    MissingField,
    UnsupportedAsset,
    UnsupportedMarket,
    PrecisionExceeded,
    QuantityOutOfRange,
    PriceOutOfRange,
    InvalidSignature,
    ChallengeReused,
    ChallengeExpired,
    NetworkMismatch,
    OriginMismatch,
    DestinationNotAllowed,
    NonceReused,
    ExpiredIntent,
    AmountZero,
}

/// 権限・状態に起因する拒否の理由コード。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotAllowedCode {
    OrderNotFound,
    OrderNotCancellable,
    AssetNotAllowed,
    AccountNotOwned,
    CallerMismatch,
    SessionIssuedByUnregisteredVault,
    UpgradeNotScheduled,
    UpgradeContentMismatch,
    UpgradeTooEarly,
    UpgradeAlreadyExecuted,
    OperationNotAvailable,
}

/// API全体のエラー。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    Unauthenticated {
        reason: String,
    },
    SessionExpired,
    SessionRevoked,
    NotEligible {
        policy_version: u64,
    },
    PolicyUnavailable,
    /// 単一書込みフェンスの一時的な競合。受付ごとの再試行規則に従う。
    JournalWriterBusy,
    BadRequest {
        code: BadRequestCode,
        detail: String,
    },
    IdempotencyConflict {
        request_id: crate::Blob,
    },
    DuplicateIgnored {
        request_id: crate::Blob,
    },
    InsufficientFunds {
        available: crate::Micros,
        requested: crate::Micros,
    },
    ReservationConflict,
    RiskLimitExceeded {
        limit: crate::Micros,
    },
    StaleAccountState {
        observed_at: crate::Timestamp,
        max_age_ms: u64,
    },
    UpstreamUnavailable {
        venue: String,
    },
    VenueRateLimited {
        retry_after_ms: Option<u64>,
    },
    UpstreamRejected {
        code: String,
        retryable: bool,
    },
    UnknownPending {
        action_id: crate::Blob,
    },
    SigningQueueFull,
    NotAllowed {
        code: NotAllowedCode,
    },
    Internal {
        code: String,
    },
}

impl ErrorCode {
    /// 同一の冪等性キーで再試行してよいか（`api-contract.md` 7節の分類）。
    ///
    /// `Internal` は含めない。設定不備・DB不整合のような恒久エラーが大半で、
    /// 再送可と分類すると契約準拠のクライアントが無限に再送する。一時的な
    /// インフラ失敗は `UpstreamUnavailable` として返す。
    pub fn retry_same_request(&self) -> bool {
        matches!(
            self,
            Self::UpstreamUnavailable { .. }
                | Self::VenueRateLimited { .. }
                | Self::SigningQueueFull
        )
    }

    /// 状態を再取得してから再判断すべきか。
    pub fn retry_after_state_refresh(&self) -> bool {
        matches!(
            self,
            Self::StaleAccountState { .. } | Self::PolicyUnavailable | Self::ReservationConflict
        )
    }

    /// 自動再送が禁止され、照合のみを行う状態か。
    pub fn reconcile_only(&self) -> bool {
        matches!(self, Self::UnknownPending { .. })
    }

    /// 自動再送してはならないか。
    pub fn must_not_auto_resend(&self) -> bool {
        !self.retry_same_request() && !self.retry_after_state_refresh()
    }
}

#[cfg(test)]
mod tests {
    use super::{BadRequestCode, ErrorCode, NotAllowedCode};

    #[test]
    fn unknown_pending_is_reconcile_only() {
        let error = ErrorCode::UnknownPending {
            action_id: Vec::new().into(),
        };
        assert!(error.reconcile_only());
        assert!(error.must_not_auto_resend());
    }

    #[test]
    fn upstream_unavailable_is_retryable_with_same_key() {
        let error = ErrorCode::UpstreamUnavailable {
            venue: "hyperliquid".to_string(),
        };
        assert!(error.retry_same_request());
        assert!(!error.must_not_auto_resend());
    }

    #[test]
    fn bad_request_is_never_retried() {
        let error = ErrorCode::BadRequest {
            code: BadRequestCode::PrecisionExceeded,
            detail: "too many decimals".to_string(),
        };
        assert!(!error.retry_same_request());
        assert!(!error.retry_after_state_refresh());
        assert!(error.must_not_auto_resend());
    }

    #[test]
    fn not_allowed_is_never_retried() {
        let error = ErrorCode::NotAllowed {
            code: NotAllowedCode::UpgradeTooEarly,
        };
        assert!(error.must_not_auto_resend());
    }

    #[test]
    fn internal_is_never_retried_with_the_same_request() {
        // 設定不備・DB不整合は再送しても直らない。再送可と分類すると
        // 契約準拠のクライアントが恒久的に再送し続ける。
        let error = ErrorCode::Internal {
            code: "vault principal is not configured".to_string(),
        };
        assert!(!error.retry_same_request());
        assert!(error.must_not_auto_resend());
    }

    #[test]
    fn journal_writer_busy_does_not_authorize_reusing_a_consumed_challenge() {
        let error = ErrorCode::JournalWriterBusy;
        assert!(!error.retry_same_request());
        assert!(!error.retry_after_state_refresh());
        assert!(error.must_not_auto_resend());
    }
}

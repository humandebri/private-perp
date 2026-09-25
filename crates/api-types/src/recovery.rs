//! vault/core間だけで使う口座回収フェンスの契約。

use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrepareRecovery {
    pub account_id: crate::Blob,
    pub user_id: crate::Blob,
    pub master_address: crate::Blob,
    pub request_id: crate::Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RecoveryFenceToken {
    pub account_id: crate::Blob,
    pub user_id: crate::Blob,
    pub request_id: crate::Blob,
    pub epoch: u64,
}

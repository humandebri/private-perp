//! Signed synthetic-attribute eligibility for local/testnet admission.

use crate::{Blob, Network};
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EligibilityClaims {
    pub principal: Principal,
    pub user_id: Blob,
    pub account_id: Blob,
    pub network: Network,
    pub vault: Principal,
    pub terms_version: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub nonce: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EligibilityToken {
    pub claims: EligibilityClaims,
    pub signature: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EligibilityStatus {
    pub terms_version: u64,
    pub expires_at: Option<u64>,
    pub eligible: bool,
}

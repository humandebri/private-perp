use crate::{Blob, Network};
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

/// Local/testnet simulation. The only permitted charge is zero.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BuilderFeeConsentClaims {
    pub principal: Principal,
    pub user_id: Blob,
    pub account_id: Blob,
    pub vault: Principal,
    pub network: Network,
    pub builder_address: Blob,
    pub fee_decibps: u16,
    pub issued_at: u64,
    pub expires_at: u64,
    pub nonce: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BuilderFeeConsent {
    pub claims: BuilderFeeConsentClaims,
    pub eoa_signature: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BuilderFeeMockStatus {
    pub approved: bool,
    pub builder_address: Option<Blob>,
    pub expires_at: Option<u64>,
    pub approval_records: u64,
    pub charged_micros: u64,
}

//! 独立送信ジャーナルの契約。V2の本人・口座情報は登録worker専用で、
//! 平文の注文本文と署名秘密は含めない。

use crate::Blob;
use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SendIntent {
    pub kind: String,
    pub request_id: Blob,
    pub account_id: Blob,
    pub nonce: u64,
    pub digest: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JournalHead {
    pub sequence: u64,
    pub hash: Blob,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JournalRecord {
    pub sequence: u64,
    pub previous_hash: Blob,
    pub hash: Blob,
    pub intent: SendIntent,
}

/// V2 recovery stream. The payload is private to its registered worker.
/// No signing key, signed exchange request or plaintext order body belongs here.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum RecoveryPayload {
    Baseline {
        state_digest: Blob,
    },
    IdentityRegistration {
        user_id: Blob,
        owner: candid::Principal,
        eoa_address: Blob,
        network: String,
    },
    IdentityAccount {
        user_id: Blob,
        owner: candid::Principal,
        account_id: Blob,
        address: Blob,
    },
    CustodyAccount {
        /// None for the shared reserve; Some for a user-owned trading account.
        user_id: Option<Blob>,
        account_id: Blob,
        kind: String,
        derivation_path: String,
        address: Blob,
        network: String,
    },
    AllocationAccepted {
        action_id: Blob,
        request_id: Blob,
        user_id: Blob,
        account_id: Blob,
        destination: Blob,
        amount_micros: u64,
        body_hash: Blob,
        nonce: u64,
        accepted_at_ms: u64,
    },
    WithdrawalAccepted {
        action_id: Blob,
        request_id: Blob,
        user_id: Blob,
        reserve_account_id: Blob,
        destination: Blob,
        amount_micros: u64,
        body_hash: Blob,
        nonce: u64,
        intent_nonce: u64,
        intent_expires_at_ms: u64,
        accepted_at_ms: u64,
    },
    RecoveryAccepted {
        action_id: Blob,
        request_id: Blob,
        user_id: Blob,
        trading_account_id: Blob,
        reserve_account_id: Blob,
        destination: Blob,
        amount_micros: u64,
        body_hash: Blob,
        nonce: u64,
        accepted_at_ms: u64,
    },
    DepositCredit {
        /// Proven sender of an inbound HL internalTransfer; absent for unattributed deposits.
        sender: Option<Blob>,
        tx_hash: Blob,
        network: String,
        address: Blob,
        amount_micros: u64,
        observed_at_ms: u64,
    },
    TradingBalanceObserved {
        user_id: Blob,
        account_id: Blob,
        previous_equity: u64,
        equity: u64,
        observed_at_ms: u64,
    },
    LedgerPosting {
        posting_id: Blob,
        user_id: Blob,
        account_id: Blob,
        amount_micros: i64,
        category: String,
    },
    Reservation {
        request_id: Blob,
        user_id: Blob,
        account_id: Blob,
        amount_micros: u64,
        state: String,
    },
    OrderRisk {
        order_id: Blob,
        user_id: Blob,
        account_id: Blob,
        risk_micros: u64,
        state: String,
    },
    /// Pending order identity and its risk hold. The signed venue body and
    /// plaintext order fields are deliberately excluded from this record.
    OrderAccepted {
        order_id: Blob,
        request_id: Blob,
        user_id: Blob,
        account_id: Blob,
        cloid: Blob,
        body_hash: Blob,
        risk_micros: u64,
        reduce_only: bool,
        accepted_at_ms: u64,
    },
    Fill {
        fill_id: Blob,
        order_id: Blob,
        account_id: Blob,
        size_micros: u64,
        price_micros: u64,
    },
    /// Full venue fill evidence for replay. Kept separate from the legacy
    /// placeholder variant so older journal payloads remain decodable.
    FillObserved {
        tid: u64,
        hl_oid: u64,
        user_id: Blob,
        order_id: Blob,
        account_id: Blob,
        market: String,
        quantity: String,
        price: String,
        fee: i64,
        filled_at_ms: u64,
    },
    /// Venue-observed order state; terminal states release the risk hold.
    OrderStatusObserved {
        order_id: Blob,
        account_id: Blob,
        hl_oid: u64,
        state: String,
        evidence_digest: Blob,
        observed_at_ms: u64,
    },
    ExternalOutcome {
        request_id: Blob,
        account_id: Blob,
        kind: String,
        state: String,
        evidence_digest: Blob,
    },
    /// A history-derived recovery outcome. Its digest alone cannot prove a
    /// complete history scan, so replay also needs the persisted HL proof.
    /// The signed usdSend body and key material are excluded.
    RecoverySettlement {
        action_id: Blob,
        request_id: Blob,
        user_id: Blob,
        trading_account_id: Blob,
        amount_micros: u64,
        nonce: u64,
        accepted: bool,
        evidence_digest: Blob,
        observed_at_ms: u64,
    },
    /// Explicit response to the original usdSend POST. Historical matching
    /// remains RecoverySettlement and needs its separate persisted proof.
    RecoveryPostResult {
        action_id: Blob,
        request_id: Blob,
        user_id: Blob,
        trading_account_id: Blob,
        amount_micros: u64,
        nonce: u64,
        accepted: bool,
        evidence_digest: Blob,
        observed_at_ms: u64,
    },
    FundTransferResult {
        action_id: Blob,
        request_id: Blob,
        user_id: Blob,
        source_account_id: Blob,
        destination: Blob,
        kind: String,
        amount_micros: u64,
        nonce: u64,
        accepted: bool,
        evidence_digest: Blob,
        observed_at_ms: u64,
    },
    OrderActionResult {
        order_id: Blob,
        account_id: Blob,
        client_request_id: Blob,
        kind: String,
        accepted: bool,
        hl_oid: Option<u64>,
        filled: bool,
        observed_at_ms: u64,
    },
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RecoveryEvent {
    pub version: u16,
    pub logical_id: Blob,
    pub payload: RecoveryPayload,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub sequence: u64,
    pub previous_hash: Blob,
    pub hash: Blob,
    pub event: RecoveryEvent,
}

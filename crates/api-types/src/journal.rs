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
        /// Credited amount for old records; gross amount when a fee is present.
        amount_micros: u64,
        /// Missing in original journals; present in interim fee-bearing journals.
        fee_micros: Option<u64>,
        observed_at_ms: u64,
    },
    DepositCreditWithFee {
        /// Proven sender of an inbound HL internalTransfer; absent for unattributed deposits.
        sender: Option<Blob>,
        tx_hash: Blob,
        network: String,
        address: Blob,
        /// Gross sender debit; the recipient receives amount minus fee.
        amount_micros: u64,
        fee_micros: u64,
        observed_at_ms: u64,
    },
    DepositClaim {
        event_id: Blob,
        user_id: Blob,
        amount_micros: u64,
        claimed_at_ms: u64,
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
    /// Exact bytes hashed when this event was persisted. Optional for old peers.
    pub encoded_payload: Option<Blob>,
    pub sequence: u64,
    pub previous_hash: Blob,
    pub hash: Blob,
    pub event: RecoveryEvent,
}

impl RecoveryPayload {
    /// Normalize legacy credited amounts for replay without changing stored bytes.
    pub fn into_fee_aware(self) -> Self {
        match self {
            Self::DepositCredit {
                sender,
                tx_hash,
                network,
                address,
                amount_micros,
                fee_micros,
                observed_at_ms,
            } => Self::DepositCreditWithFee {
                sender,
                tx_hash,
                network,
                address,
                amount_micros,
                fee_micros: fee_micros.unwrap_or(0),
                observed_at_ms,
            },
            other => other,
        }
    }
}

impl RecoveryRecord {
    /// Verify the typed event corresponds to the original hash-chain payload.
    pub fn payload_bytes(&self) -> Result<Vec<u8>, &'static str> {
        let bytes = match &self.encoded_payload {
            Some(bytes) => bytes.as_ref().to_vec(),
            None => {
                candid::encode_one(&self.event.payload).map_err(|_| "invalid recovery payload")?
            }
        };
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err("invalid recovery payload length");
        }
        let decoded: RecoveryPayload =
            candid::decode_one(&bytes).map_err(|_| "invalid recovery payload")?;
        if decoded != self.event.payload {
            return Err("recovery payload does not match event");
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // An independently encoded old sender type lacks the new fee-bearing variant.
    #[derive(CandidType, Serialize)]
    enum LegacyPayload {
        DepositCredit {
            sender: Option<Blob>,
            tx_hash: Blob,
            network: String,
            address: Blob,
            amount_micros: u64,
            observed_at_ms: u64,
        },
    }

    #[derive(CandidType, Serialize)]
    enum InterimPayload {
        DepositCredit {
            sender: Option<Blob>,
            tx_hash: Blob,
            network: String,
            address: Blob,
            amount_micros: u64,
            fee_micros: u64,
            observed_at_ms: u64,
        },
    }

    #[test]
    fn interim_deposits_keep_their_recorded_fee() {
        let bytes = candid::encode_one(InterimPayload::DepositCredit {
            sender: None,
            tx_hash: vec![1; 32].into(),
            network: "local".into(),
            address: vec![2; 20].into(),
            amount_micros: 10_000_000,
            fee_micros: 1_000_000,
            observed_at_ms: 123,
        })
        .unwrap();
        let payload: RecoveryPayload = candid::decode_one(&bytes).unwrap();
        assert!(matches!(
            payload.into_fee_aware(),
            RecoveryPayload::DepositCreditWithFee {
                fee_micros: 1_000_000,
                ..
            }
        ));
    }

    #[test]
    fn legacy_deposits_decode_and_keep_original_hash_bytes() {
        let bytes = candid::encode_one(LegacyPayload::DepositCredit {
            sender: None,
            tx_hash: vec![1; 32].into(),
            network: "local".into(),
            address: vec![2; 20].into(),
            amount_micros: 10_000_000,
            observed_at_ms: 123,
        })
        .unwrap();
        let payload: RecoveryPayload = candid::decode_one(&bytes).unwrap();
        assert!(matches!(payload, RecoveryPayload::DepositCredit { .. }));
        assert_ne!(candid::encode_one(&payload).unwrap(), bytes);
        let mut record = RecoveryRecord {
            encoded_payload: Some(bytes.clone().into()),
            sequence: 1,
            previous_hash: vec![0; 32].into(),
            hash: vec![3; 32].into(),
            event: RecoveryEvent {
                version: 1,
                logical_id: vec![4; 32].into(),
                payload,
            },
        };
        assert_eq!(record.payload_bytes().unwrap(), bytes);
        assert!(matches!(
            record.event.payload.clone().into_fee_aware(),
            RecoveryPayload::DepositCreditWithFee { fee_micros: 0, .. }
        ));
        record.event.payload = RecoveryPayload::Baseline {
            state_digest: vec![5; 32].into(),
        };
        assert!(record.payload_bytes().is_err());
    }
}

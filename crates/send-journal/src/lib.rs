//! 独立した追記専用ジャーナル。送信意図は非公開IDとdigestのみ、
//! 復元用の本人・口座対応は登録workerだけが取得できる列へ保存する。
//! 署名秘密と署名済み送信本文は保持しない。

use api_types::error::{BadRequestCode, ErrorCode};
use api_types::journal::{
    JournalHead, JournalRecord, RecoveryEvent, RecoveryPayload, RecoveryRecord, SendIntent,
};
use candid::Principal;
use db::error::Error as DbError;
use sha2::{Digest, Sha256};
use sha3::Keccak256;

const MEMORY_ID: u8 = db::memory_id::SEND_JOURNAL_MAIN;

fn map_db(error: DbError) -> ErrorCode {
    match error {
        DbError::Conflict => ErrorCode::ReservationConflict,
        other => ErrorCode::Internal {
            code: format!("{other:?}"),
        },
    }
}

fn fixed32(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
    bytes.try_into().map_err(|_| ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: "journal field must be 32 bytes".into(),
    })
}

fn invalid_event() -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: "invalid recovery event".into(),
    }
}

fn valid_event(event: &RecoveryEvent) -> bool {
    if event.version != 1 || event.logical_id.len() != 32 {
        return false;
    }
    let id = |id: &api_types::Blob| id.len() == 32;
    let state = |s: &str| s.len() <= 32 && s.is_ascii();
    match &event.payload {
        RecoveryPayload::Baseline { state_digest } => id(state_digest),
        RecoveryPayload::IdentityRegistration {
            user_id,
            owner,
            eoa_address,
            network,
        } => {
            let mut logical = b"vault_identity".to_vec();
            logical.extend_from_slice(network.as_bytes());
            logical.extend_from_slice(eoa_address.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(user_id)
                && *owner != Principal::anonymous()
                && eoa_address.len() == 20
                && matches!(network.as_str(), "local" | "testnet")
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::IdentityAccount {
            user_id,
            owner,
            account_id,
            address,
        } => {
            id(user_id) && *owner != Principal::anonymous() && id(account_id) && address.len() == 20
        }
        RecoveryPayload::CustodyAccount {
            user_id,
            account_id,
            kind,
            derivation_path,
            address,
            network,
        } => {
            let mut logical = b"custody_account".to_vec();
            if let Some(user_id) = user_id {
                logical.extend_from_slice(user_id.as_ref());
            }
            logical.extend_from_slice(kind.as_bytes());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            (match (kind.as_str(), user_id) {
                ("reserve", None) => true,
                ("trading", Some(user_id)) => id(user_id),
                _ => false,
            }) && id(account_id)
                && derivation_path
                    == &format!("private-perp/{kind}/{}", hex::encode(account_id.as_ref()))
                && address.len() == 20
                && matches!(network.as_str(), "local" | "testnet")
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::AllocationAccepted {
            action_id,
            request_id,
            user_id,
            account_id,
            destination,
            amount_micros,
            body_hash,
            nonce,
            accepted_at_ms,
        } => {
            let mut logical = b"allocation_accepted".to_vec();
            logical.extend_from_slice(user_id.as_ref());
            logical.extend_from_slice(request_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(action_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(account_id)
                && destination.len() == 20
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && id(body_hash)
                && *nonce > 0
                && *accepted_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::WithdrawalAccepted {
            action_id,
            request_id,
            user_id,
            reserve_account_id,
            destination,
            amount_micros,
            body_hash,
            nonce,
            intent_nonce,
            intent_expires_at_ms,
            accepted_at_ms,
        } => {
            let mut logical = b"withdrawal_accepted".to_vec();
            logical.extend_from_slice(user_id.as_ref());
            logical.extend_from_slice(request_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(action_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(reserve_account_id)
                && destination.len() == 20
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && id(body_hash)
                && *nonce > 0
                && *intent_nonce <= i64::MAX as u64
                && *intent_expires_at_ms > *accepted_at_ms
                && *accepted_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::RecoveryAccepted {
            action_id,
            request_id,
            user_id,
            trading_account_id,
            reserve_account_id,
            destination,
            amount_micros,
            body_hash,
            nonce,
            accepted_at_ms,
        } => {
            let mut logical = b"recovery_accepted".to_vec();
            logical.extend_from_slice(user_id.as_ref());
            logical.extend_from_slice(request_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(action_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(trading_account_id)
                && id(reserve_account_id)
                && destination.len() == 20
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && id(body_hash)
                && *nonce > 0
                && *accepted_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::DepositCredit {
            sender,
            tx_hash,
            network,
            address,
            amount_micros,
            observed_at_ms,
        } => {
            let mut input = b"deposit".to_vec();
            input.extend_from_slice(tx_hash.as_ref());
            let event_id = Keccak256::digest(&input);
            let mut logical = b"deposit_credit".to_vec();
            logical.extend_from_slice(network.as_bytes());
            logical.extend_from_slice(&event_id);
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            event.logical_id.as_ref() == expected
                && sender.as_ref().is_none_or(|address| address.len() == 20)
                && !tx_hash.is_empty()
                && tx_hash.len() <= 64
                && matches!(network.as_str(), "local" | "testnet")
                && address.len() == 20
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && *observed_at_ms > 0
        }
        RecoveryPayload::DepositClaim {
            event_id,
            user_id,
            amount_micros,
            claimed_at_ms,
        } => {
            let mut logical = b"deposit_claim".to_vec();
            logical.extend_from_slice(event_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            event.logical_id.as_ref() == expected
                && id(event_id)
                && id(user_id)
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && *claimed_at_ms > 0
        }
        RecoveryPayload::TradingBalanceObserved {
            user_id,
            account_id,
            previous_equity,
            equity,
            observed_at_ms,
        } => {
            let mut logical = b"trading_balance_observed".to_vec();
            logical.extend_from_slice(account_id.as_ref());
            logical.extend_from_slice(&observed_at_ms.to_be_bytes());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            event.logical_id.as_ref() == expected
                && id(user_id)
                && id(account_id)
                && *previous_equity <= i64::MAX as u64
                && *equity <= i64::MAX as u64
                && *observed_at_ms > 0
                && *observed_at_ms <= i64::MAX as u64
        }
        RecoveryPayload::LedgerPosting {
            posting_id,
            user_id,
            account_id,
            category,
            ..
        } => id(posting_id) && id(user_id) && id(account_id) && state(category),
        RecoveryPayload::Reservation {
            request_id,
            user_id,
            account_id,
            state: status,
            ..
        } => id(request_id) && id(user_id) && id(account_id) && state(status),
        RecoveryPayload::OrderRisk {
            order_id,
            user_id,
            account_id,
            state: status,
            ..
        } => id(order_id) && id(user_id) && id(account_id) && state(status),
        RecoveryPayload::OrderAccepted {
            order_id,
            request_id,
            user_id,
            account_id,
            cloid,
            body_hash,
            risk_micros,
            reduce_only,
            accepted_at_ms,
        } => {
            let mut logical = b"order_accepted".to_vec();
            logical.extend_from_slice(user_id.as_ref());
            logical.extend_from_slice(request_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            let mut order_identity = cloid.as_ref().to_vec();
            order_identity.extend_from_slice(user_id.as_ref());
            let expected_order: [u8; 32] = Keccak256::digest(&order_identity).into();
            id(order_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(account_id)
                && cloid.len() == 16
                && id(body_hash)
                && *risk_micros <= i64::MAX as u64
                && (*reduce_only || *risk_micros > 0)
                && (!*reduce_only || *risk_micros == 0)
                && *accepted_at_ms > 0
                && event.logical_id.as_ref() == expected
                && order_id.as_ref() == expected_order
        }
        RecoveryPayload::Fill {
            fill_id,
            order_id,
            account_id,
            ..
        } => id(fill_id) && id(order_id) && id(account_id),
        RecoveryPayload::FillObserved {
            tid,
            hl_oid,
            user_id,
            order_id,
            account_id,
            market,
            quantity,
            price,
            fee: _,
            filled_at_ms,
        } => {
            let mut logical = b"fill_observed".to_vec();
            logical.extend_from_slice(account_id.as_ref());
            logical.extend_from_slice(&tid.to_be_bytes());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            *tid > 0
                && *tid <= i64::MAX as u64
                && *hl_oid > 0
                && *hl_oid <= i64::MAX as u64
                && id(user_id)
                && id(order_id)
                && id(account_id)
                && matches!(market.as_str(), "BTC" | "ETH")
                && !quantity.is_empty()
                && quantity.len() <= 32
                && !price.is_empty()
                && price.len() <= 32
                && *filled_at_ms > 0
                && *filled_at_ms <= i64::MAX as u64
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::OrderStatusObserved {
            order_id,
            account_id,
            hl_oid,
            state,
            evidence_digest,
            observed_at_ms,
        } => {
            let mut logical = b"order_status_observed".to_vec();
            logical.extend_from_slice(order_id.as_ref());
            logical.extend_from_slice(state.as_bytes());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(order_id)
                && id(account_id)
                && *hl_oid > 0
                && *hl_oid <= i64::MAX as u64
                && matches!(state.as_str(), "open" | "filled" | "cancelled" | "rejected")
                && id(evidence_digest)
                && *observed_at_ms > 0
                && *observed_at_ms <= i64::MAX as u64
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::ExternalOutcome {
            request_id,
            account_id,
            kind,
            state: status,
            evidence_digest,
        } => {
            id(request_id) && id(account_id) && id(evidence_digest) && state(kind) && state(status)
        }
        RecoveryPayload::RecoverySettlement {
            action_id,
            request_id,
            user_id,
            trading_account_id,
            amount_micros,
            nonce,
            evidence_digest,
            observed_at_ms,
            ..
        } => {
            let mut logical = b"recovery_settlement".to_vec();
            logical.extend_from_slice(action_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(action_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(trading_account_id)
                && id(evidence_digest)
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && *nonce > 0
                && *observed_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::RecoveryPostResult {
            action_id,
            request_id,
            user_id,
            trading_account_id,
            amount_micros,
            nonce,
            evidence_digest,
            observed_at_ms,
            ..
        } => {
            let mut logical = b"recovery_post_result".to_vec();
            logical.extend_from_slice(action_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(action_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(trading_account_id)
                && id(evidence_digest)
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && *nonce > 0
                && *observed_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::FundTransferResult {
            action_id,
            request_id,
            user_id,
            source_account_id,
            destination,
            kind,
            amount_micros,
            nonce,
            evidence_digest,
            observed_at_ms,
            ..
        } => {
            let mut logical = b"fund_transfer_result".to_vec();
            logical.extend_from_slice(action_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(action_id)
                && !request_id.is_empty()
                && request_id.len() <= 64
                && id(user_id)
                && id(source_account_id)
                && matches!(kind.as_str(), "allocation" | "withdrawal")
                && destination.len() == if kind == "allocation" { 32 } else { 20 }
                && id(evidence_digest)
                && *amount_micros > 0
                && *amount_micros <= i64::MAX as u64
                && *nonce > 0
                && *observed_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
        RecoveryPayload::OrderActionResult {
            order_id,
            account_id,
            client_request_id,
            kind,
            hl_oid,
            observed_at_ms,
            ..
        } => {
            let mut logical = b"order_action_result".to_vec();
            logical.extend_from_slice(kind.as_bytes());
            logical.extend_from_slice(order_id.as_ref());
            let expected: [u8; 32] = Keccak256::digest(&logical).into();
            id(order_id)
                && id(account_id)
                && !client_request_id.is_empty()
                && client_request_id.len() <= 64
                && matches!(kind.as_str(), "leverage" | "order" | "cancel")
                && hl_oid.is_none_or(|oid| oid <= i64::MAX as u64)
                && *observed_at_ms > 0
                && event.logical_id.as_ref() == expected
        }
    }
}

fn recovery_record(
    row: db::repo::send_journal::StoredRecoveryEvent,
) -> Result<RecoveryRecord, ErrorCode> {
    let payload: RecoveryPayload =
        candid::decode_one(&row.payload).map_err(|_| ErrorCode::Internal {
            code: "invalid stored recovery payload".into(),
        })?;
    Ok(RecoveryRecord {
        sequence: row.sequence,
        previous_hash: row.previous_hash.to_vec().into(),
        hash: row.hash.to_vec().into(),
        event: RecoveryEvent {
            version: row.version,
            logical_id: row.logical_id.to_vec().into(),
            payload,
        },
    })
}

fn journal_record(record: db::repo::send_journal::StoredRecord) -> JournalRecord {
    JournalRecord {
        sequence: record.sequence,
        previous_hash: record.previous_hash.to_vec().into(),
        hash: record.hash.to_vec().into(),
        intent: SendIntent {
            kind: record.kind,
            request_id: record.request_id.to_vec().into(),
            account_id: record.account_id.to_vec().into(),
            nonce: record.nonce,
            digest: record.digest.to_vec().into(),
        },
    }
}

fn require_worker() -> Result<Vec<u8>, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    #[cfg(feature = "embedded")]
    {
        let role = ACTIVE_JOURNAL_ROLE.with(|active| active.get());
        if caller == ic_cdk::api::canister_self()
            && let Some(role) = role
        {
            Ok(role.as_bytes().to_vec())
        } else {
            Err(ErrorCode::Unauthenticated {
                reason: "journal role requires a self-call".into(),
            })
        }
    }
    #[cfg(not(feature = "embedded"))]
    {
        if caller == Principal::anonymous()
            || !db::tx::query(|c| db::repo::send_journal::authorized(c, caller.as_slice()))
                .map_err(map_db)?
        {
            return Err(ErrorCode::Unauthenticated {
                reason: "registered journal worker required".into(),
            });
        }
        Ok(caller.as_slice().to_vec())
    }
}

#[cfg(feature = "embedded")]
thread_local! {
    static ACTIVE_JOURNAL_ROLE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

#[cfg(feature = "embedded")]
fn with_journal_role<T>(
    role: String,
    f: impl FnOnce() -> Result<T, ErrorCode>,
) -> Result<T, ErrorCode> {
    if ic_cdk::api::msg_caller() != ic_cdk::api::canister_self() {
        return Err(ErrorCode::Unauthenticated {
            reason: "journal role requires a self-call".into(),
        });
    }
    let role = match role.as_str() {
        "vault" => "vault",
        "core" => "core",
        _ => return Err(invalid_event()),
    };
    struct Reset(Option<&'static str>);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE_JOURNAL_ROLE.with(|active| active.set(self.0));
        }
    }
    let previous = ACTIVE_JOURNAL_ROLE.with(|active| active.replace(Some(role)));
    let _reset = Reset(previous);
    f()
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_head(role: String) -> Result<JournalHead, ErrorCode> {
    with_journal_role(role, head)
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_records(role: String, after: u64, limit: u32) -> Result<Vec<JournalRecord>, ErrorCode> {
    with_journal_role(role, || records(after, limit))
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_intent_record(
    role: String,
    kind: String,
    request_id: api_types::Blob,
) -> Result<Option<JournalRecord>, ErrorCode> {
    with_journal_role(role, || intent_record(kind, request_id))
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_append(role: String, intent: SendIntent) -> Result<JournalHead, ErrorCode> {
    with_journal_role(role, || append(intent))
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_recovery_head(role: String) -> Result<JournalHead, ErrorCode> {
    with_journal_role(role, recovery_head)
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_recovery_events(
    role: String,
    after: u64,
    limit: u32,
) -> Result<Vec<RecoveryRecord>, ErrorCode> {
    with_journal_role(role, || recovery_events(after, limit))
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_recovery_event(
    role: String,
    logical_id: api_types::Blob,
) -> Result<Option<RecoveryRecord>, ErrorCode> {
    with_journal_role(role, || recovery_event(logical_id))
}

#[cfg(feature = "embedded")]
#[ic_cdk::update]
fn role_append_recovery_event(
    role: String,
    event: RecoveryEvent,
) -> Result<JournalHead, ErrorCode> {
    with_journal_role(role, || append_recovery_event(event))
}

#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn register_worker(role: String, worker: Principal) -> Result<(), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    if !matches!(role.as_str(), "vault" | "core")
        || worker == Principal::anonymous()
        || worker.as_slice().is_empty()
    {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid journal worker".into(),
        });
    }
    db::tx::update(|c| db::repo::send_journal::register_worker(c, &role, worker.as_slice()))
        .map_err(map_db)
}

#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn head() -> Result<JournalHead, ErrorCode> {
    let worker = require_worker()?;
    let (sequence, hash) =
        db::tx::query(|c| db::repo::send_journal::head(c, &worker)).map_err(map_db)?;
    Ok(JournalHead {
        sequence,
        hash: hash.to_vec().into(),
    })
}

/// 復元照合用の範囲取得。呼出元自身の記録だけを返す。
#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn records(after: u64, limit: u32) -> Result<Vec<JournalRecord>, ErrorCode> {
    let worker = require_worker()?;
    if !(1..=100).contains(&limit) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "journal record limit must be 1..100".into(),
        });
    }
    let records = db::tx::query(|c| db::repo::send_journal::records(c, &worker, after, limit))
        .map_err(map_db)?;
    Ok(records.into_iter().map(journal_record).collect())
}

/// Idempotency lookup for a response lost after the append committed.
#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn intent_record(
    kind: String,
    request_id: api_types::Blob,
) -> Result<Option<JournalRecord>, ErrorCode> {
    let worker = require_worker()?;
    let request_id = fixed32(&request_id)?;
    if !matches!(
        kind.as_str(),
        "allocation" | "withdrawal" | "recovery" | "order" | "cancel" | "leverage" | "agent"
    ) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid journal kind".into(),
        });
    }
    db::tx::query(|c| {
        let Some(existing) = db::repo::send_journal::existing(c, &worker, &kind, &request_id)?
        else {
            return Ok(None);
        };
        let before = existing.sequence.checked_sub(1).ok_or(DbError::Overflow)?;
        let record = db::repo::send_journal::records(c, &worker, before, 1)?
            .into_iter()
            .next()
            .ok_or(DbError::NotFound)?;
        if record.request_id != request_id || record.kind != kind {
            return Err(DbError::Invariant("journal lookup mismatch"));
        }
        Ok(Some(journal_record(record)))
    })
    .map_err(map_db)
}

#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn append(intent: SendIntent) -> Result<JournalHead, ErrorCode> {
    let worker = require_worker()?;
    if !matches!(
        intent.kind.as_str(),
        "allocation" | "withdrawal" | "recovery" | "order" | "cancel" | "leverage" | "agent"
    ) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid journal kind".into(),
        });
    }
    let request_id = fixed32(&intent.request_id)?;
    let account_id = fixed32(&intent.account_id)?;
    let digest = fixed32(&intent.digest)?;
    if intent.nonce == 0 || intent.nonce > i64::MAX as u64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid journal nonce".into(),
        });
    }
    db::tx::update(|c| {
        if let Some(existing) =
            db::repo::send_journal::existing(c, &worker, &intent.kind, &request_id)?
        {
            if existing.account_id != account_id
                || existing.nonce != intent.nonce
                || existing.digest != digest
            {
                return Err(DbError::Conflict);
            }
            return Ok(JournalHead {
                sequence: existing.sequence,
                hash: existing.hash.to_vec().into(),
            });
        }
        let (prior, previous_hash) = db::repo::send_journal::head(c, &worker)?;
        let sequence = prior.checked_add(1).ok_or(DbError::Overflow)?;
        let mut hasher = Sha256::new();
        hasher.update(b"private-perp/send-journal/v1");
        hasher.update(previous_hash);
        hasher.update(sequence.to_be_bytes());
        hasher.update(&worker);
        hasher.update(intent.kind.as_bytes());
        hasher.update(request_id);
        hasher.update(account_id);
        hasher.update(intent.nonce.to_be_bytes());
        hasher.update(digest);
        let hash: [u8; 32] = hasher.finalize().into();
        db::repo::send_journal::append(
            c,
            &worker,
            sequence,
            &intent.kind,
            &request_id,
            &account_id,
            intent.nonce,
            &digest,
            &previous_hash,
            &hash,
        )?;
        Ok(JournalHead {
            sequence,
            hash: hash.to_vec().into(),
        })
    })
    .map_err(map_db)
}

#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn recovery_head() -> Result<JournalHead, ErrorCode> {
    let worker = require_worker()?;
    let (sequence, hash) =
        db::tx::query(|c| db::repo::send_journal::recovery_head(c, &worker)).map_err(map_db)?;
    Ok(JournalHead {
        sequence,
        hash: hash.to_vec().into(),
    })
}

/// Only the registered worker can read its own private recovery events.
#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn recovery_events(after: u64, limit: u32) -> Result<Vec<RecoveryRecord>, ErrorCode> {
    let worker = require_worker()?;
    if !(1..=100).contains(&limit) {
        return Err(invalid_event());
    }
    db::tx::query(|c| db::repo::send_journal::recovery_events(c, &worker, after, limit))
        .map_err(map_db)?
        .into_iter()
        .map(recovery_record)
        .collect()
}

#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn recovery_event(logical_id: api_types::Blob) -> Result<Option<RecoveryRecord>, ErrorCode> {
    let worker = require_worker()?;
    let logical_id = fixed32(&logical_id)?;
    db::tx::query(|c| db::repo::send_journal::recovery_event(c, &worker, &logical_id))
        .map_err(map_db)?
        .map(recovery_record)
        .transpose()
}

#[scoped_entrypoint::update(scope = Journal, prefix = "journal_")]
fn append_recovery_event(event: RecoveryEvent) -> Result<JournalHead, ErrorCode> {
    let worker = require_worker()?;
    if !valid_event(&event) {
        return Err(invalid_event());
    }
    let logical_id = fixed32(&event.logical_id)?;
    let payload = candid::encode_one(&event.payload).map_err(|_| invalid_event())?;
    if payload.len() > 4096 {
        return Err(invalid_event());
    }
    db::tx::update(|c| {
        if let Some(existing) = db::repo::send_journal::recovery_event(c, &worker, &logical_id)? {
            if existing.version != event.version || existing.payload != payload {
                return Err(DbError::Conflict);
            }
            return Ok(JournalHead {
                sequence: existing.sequence,
                hash: existing.hash.to_vec().into(),
            });
        }
        let (prior, previous_hash) = db::repo::send_journal::recovery_head(c, &worker)?;
        let sequence = prior.checked_add(1).ok_or(DbError::Overflow)?;
        let mut hasher = Sha256::new();
        hasher.update(b"private-perp/recovery-event/v1");
        hasher.update(previous_hash);
        hasher.update(sequence.to_be_bytes());
        hasher.update(&worker);
        hasher.update(logical_id);
        hasher.update(event.version.to_be_bytes());
        hasher.update(&payload);
        let hash: [u8; 32] = hasher.finalize().into();
        db::repo::send_journal::append_recovery_event(
            c,
            &worker,
            &db::repo::send_journal::StoredRecoveryEvent {
                sequence,
                logical_id,
                version: event.version,
                payload,
                previous_hash,
                hash,
            },
        )?;
        Ok(JournalHead {
            sequence,
            hash: hash.to_vec().into(),
        })
    })
    .map_err(map_db)
}

#[scoped_entrypoint::query(scope = Journal, prefix = "journal_")]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn init_db() {
    if let Err(error) = if cfg!(feature = "embedded") {
        db::init_scoped(db::DbScope::Journal, db::schema::send_journal::MIGRATIONS)
    } else {
        db::init(MEMORY_ID, db::schema::send_journal::MIGRATIONS)
    } {
        ic_cdk::trap(format!("journal init failed: {error}"));
    }
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::init)]
fn init() {
    init_db();
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::post_upgrade)]
fn post_upgrade() {
    init_db();
}

#[cfg(feature = "embedded")]
pub fn embedded_init() {
    db::tx::with_scope(db::DbScope::Journal, init);
}

#[cfg(feature = "embedded")]
pub fn embedded_post_upgrade() {
    db::tx::with_scope(db::DbScope::Journal, post_upgrade);
}

#[cfg(not(feature = "embedded"))]
ic_cdk::export_candid!();

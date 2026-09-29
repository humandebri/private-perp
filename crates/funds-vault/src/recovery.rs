//! vaultからcoreの口座フェンスを操作する。どの失敗も送信許可には変換しない。

use api_types::error::ErrorCode;
use api_types::journal::{RecoveryEvent, RecoveryPayload};
use api_types::recovery::{PrepareRecovery, RecoveryFenceToken};
use candid::Principal;
use db::repo::actions::RecoveryCheck;
use db::worker_permissions;
use ic_cdk::call::Call;

const NONCE_ACCEPT_MS: u64 = 2 * 24 * 60 * 60 * 1_000;
const CLOCK_MARGIN_MS: u64 = 60 * 60 * 1_000;
const EARLY_MATCH_MS: u64 = 60 * 1_000;
const PAGE_LIMIT: usize = 500;

fn map_db(error: db::error::Error) -> ErrorCode {
    crate::auth::map_db(error, None)
}

fn event_address(value: &str) -> Option<[u8; 20]> {
    let bytes = hex::decode(value.strip_prefix("0x").unwrap_or(value)).ok()?;
    bytes.try_into().ok()
}

fn matching_hash(
    entry: &serde_json::Value,
    source: &[u8; 20],
    destination: &[u8; 20],
    amount: u64,
    start: u64,
    end: u64,
) -> Result<Option<[u8; 32]>, ()> {
    let time = entry
        .get("time")
        .and_then(|value| value.as_u64())
        .ok_or(())?;
    if time < start || time > end {
        return Err(());
    }
    let delta = entry.get("delta").ok_or(())?;
    let kind = delta
        .get("type")
        .and_then(|value| value.as_str())
        .ok_or(())?;
    let hash = entry
        .get("hash")
        .and_then(|value| value.as_str())
        .ok_or(())?;
    let hash_bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(hash)).map_err(|_| ())?;
    let hash: [u8; 32] = hash_bytes.try_into().map_err(|_| ())?;
    if kind != "internalTransfer" {
        return Ok(None);
    }
    let from = delta
        .get("user")
        .and_then(|value| value.as_str())
        .and_then(event_address)
        .ok_or(())?;
    let to = delta
        .get("destination")
        .and_then(|value| value.as_str())
        .and_then(event_address)
        .ok_or(())?;
    let usdc = delta.get("usdc").and_then(crate::amount::parse).ok_or(())?;
    Ok((from == *source && to == *destination && usdc.micros == amount).then_some(hash))
}

/// 1回のsweepにつき1ページを確認する。履歴の欠落や曖昧さは永続unknownに残す。
pub async fn reconcile_recoveries(now: u64) -> Result<(), ErrorCode> {
    let rows = db::tx::query(|connection| db::repo::actions::recovery_checks(connection, 1, now))
        .map_err(map_db)?;
    for row in rows {
        let Some(attempt) =
            worker_permissions::begin("fund", &row.action_id, &row.user_id).map_err(map_db)?
        else {
            continue;
        };
        let action_id = row.action_id;
        let epoch = row.worker_epoch;
        if reconcile_one(row, now).await.is_err() {
            db::tx::update(|connection| {
                db::repo::actions::defer_recovery_check(connection, &action_id, epoch, now)
            })
            .map_err(map_db)?;
        } else if db::tx::query(|c| db::repo::actions::finished(c, &action_id)).map_err(map_db)? {
            attempt.completed().map_err(map_db)?;
        }
    }
    Ok(())
}

async fn reconcile_one(row: RecoveryCheck, now: u64) -> Result<(), ErrorCode> {
    if row.ambiguous {
        defer(&row, now)?;
        return Ok(());
    }
    let (source, destination) = db::tx::query(|connection| {
        if row.kind != "recovery" {
            let reserve = db::repo::ledger::custody_account(
                connection,
                &row.user_id,
                api_types::AccountKind::Reserve,
            )?
            .ok_or(db::error::Error::NotFound)?;
            let destination = event_address(&row.destination).ok_or(db::error::Error::Conflict)?;
            return Ok((reserve.master_address, destination));
        }
        let trading = db::repo::ledger::custody_account(
            connection,
            &row.user_id,
            api_types::AccountKind::Trading,
        )?
        .ok_or(db::error::Error::NotFound)?;
        let reserve = db::repo::ledger::custody_account(
            connection,
            &row.user_id,
            api_types::AccountKind::Reserve,
        )?
        .ok_or(db::error::Error::NotFound)?;
        if trading.account_id != row.account_id {
            return Err(db::error::Error::Conflict);
        }
        Ok((trading.master_address, reserve.master_address))
    })
    .map_err(map_db)?;
    if event_address(&row.destination) != Some(destination) {
        return Err(ErrorCode::PolicyUnavailable);
    }
    // History has no action nonce. Never assign the same evidence to two sends.
    if db::tx::query(|c| {
        db::repo::actions::transfer_has_competitor(
            c,
            &row,
            NONCE_ACCEPT_MS + CLOCK_MARGIN_MS + EARLY_MATCH_MS,
        )
    })
    .map_err(map_db)?
    {
        defer(&row, now)?;
        return Ok(());
    }
    let start = row.nonce.saturating_sub(EARLY_MATCH_MS);
    let end = row
        .nonce
        .saturating_add(NONCE_ACCEPT_MS)
        .saturating_add(CLOCK_MARGIN_MS);
    let cursor = row
        .checked_until
        .map_or(start, |last| last.saturating_add(1));
    if cursor > end {
        return finalize_if_proven(&row, now, end).await;
    }
    let window = row.window_ms.max(1);
    let early_end = row.nonce.saturating_add(EARLY_MATCH_MS);
    let phase_end = if row.checked_until.unwrap_or(0) < early_end {
        early_end
    } else {
        end
    };
    let page_end = cursor
        .saturating_add(window.saturating_sub(1))
        .min(phase_end)
        .min(now);
    if page_end < cursor {
        defer(&row, now)?;
        return Ok(());
    }
    let body = crate::deposits::fetch_ledger_updates_range(
        &format!("0x{}", hex::encode(source)),
        cursor,
        Some(page_end),
    )
    .await?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&body).map_err(|_| ErrorCode::PolicyUnavailable)?;
    if entries.len() >= PAGE_LIMIT {
        if window <= 1 {
            defer(&row, now)?;
            return Ok(());
        }
        // capに達した区間は完全とみなさない。次回、同じcursorを狭い窓で取得する。
        db::tx::update(|connection| {
            db::repo::actions::advance_recovery_check(
                connection,
                &row.action_id,
                row.worker_epoch,
                row.checked_until.unwrap_or(start.saturating_sub(1)),
                window / 2,
                now,
            )
        })
        .map_err(map_db)?;
        return Ok(());
    }
    let mut found = row.match_hash;
    let mut ambiguous = false;
    for entry in &entries {
        match matching_hash(entry, &source, &destination, row.amount, cursor, page_end) {
            Ok(Some(hash)) if found.is_some_and(|previous| previous != hash) => ambiguous = true,
            Ok(Some(hash)) => found = Some(hash),
            Ok(None) => {}
            Err(()) => return Err(ErrorCode::PolicyUnavailable),
        }
    }
    db::tx::update(|connection| {
        if ambiguous {
            db::repo::actions::mark_recovery_ambiguous(
                connection,
                &row.action_id,
                row.worker_epoch,
                now,
            )?;
        }
        if row.match_hash.is_none()
            && let Some(hash) = found
        {
            db::repo::actions::set_recovery_match_hash(
                connection,
                &row.action_id,
                row.worker_epoch,
                &hash,
            )?;
        }
        db::repo::actions::advance_recovery_check(
            connection,
            &row.action_id,
            row.worker_epoch,
            page_end,
            window,
            now,
        )
    })
    .map_err(map_db)?;
    if found.is_none() && page_end >= now.saturating_sub(1_000) {
        defer(&row, now)?;
    }
    if !ambiguous
        && page_end == early_end
        && let Some(hash) = found
    {
        return settle_positive(&row, hash, now).await;
    }
    if page_end == end && !ambiguous {
        finalize_if_proven(&row, now, end).await?;
    }
    Ok(())
}

fn settlement_event(
    row: &RecoveryCheck,
    accepted: bool,
    evidence_digest: [u8; 32],
    now: u64,
) -> RecoveryEvent {
    let mut logical_id = b"recovery_settlement".to_vec();
    logical_id.extend_from_slice(&row.action_id);
    RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical_id).to_vec().into(),
        payload: RecoveryPayload::RecoverySettlement {
            action_id: row.action_id.to_vec().into(),
            request_id: row.request_id.clone().into(),
            user_id: row.user_id.to_vec().into(),
            trading_account_id: row.account_id.to_vec().into(),
            amount_micros: row.amount,
            nonce: row.nonce,
            accepted,
            evidence_digest: evidence_digest.to_vec().into(),
            observed_at_ms: now,
        },
    }
}

async fn settle_positive(row: &RecoveryCheck, hash: [u8; 32], now: u64) -> Result<(), ErrorCode> {
    let proven = db::tx::query(|connection| {
        let proof = db::repo::actions::recovery_proof(connection, &row.action_id)?;
        Ok(proof.is_some_and(|proof| {
            proof.worker_epoch == row.worker_epoch
                && !proof.ambiguous
                && proof.match_hash == Some(hash)
        }))
    })
    .map_err(map_db)?;
    if !proven {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if row.kind != "recovery" {
        return settle_transfer(row, true, hash, now).await;
    }
    let settlement = settlement_event(row, true, hash, now);
    let ack = journal_client::append_recovery_event("vault", settlement.clone()).await?;
    let mut event_input = b"recovery".to_vec();
    event_input.extend_from_slice(&hash);
    let event_id = hl_sign::keccak256(&event_input);
    db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &settlement, &ack)?;
        let proof = db::repo::actions::recovery_proof(connection, &row.action_id)?;
        if !proof.is_some_and(|proof| {
            proof.worker_epoch == row.worker_epoch
                && !proof.ambiguous
                && proof.match_hash == Some(hash)
        }) {
            return Err(db::error::Error::Conflict);
        }
        db::repo::ledger::recovery_confirm(
            connection,
            &row.user_id,
            &row.account_id,
            row.amount,
            now,
            &event_id,
        )?;
        db::repo::funds::consume_reservation(connection, &row.user_id, &row.request_id)?;
        db::repo::funds::set_request_state(
            connection,
            &row.user_id,
            &row.request_id,
            api_types::fund::FundRequestState::Settled,
            now,
        )?;
        if row.action_state == "unknown" {
            db::repo::actions::reconcile_unknown(
                connection,
                &row.action_id,
                row.worker_epoch,
                now,
            )?;
        }
        Ok(())
    })
    .map_err(map_db)
}

async fn finalize_if_proven(row: &RecoveryCheck, now: u64, end: u64) -> Result<(), ErrorCode> {
    if row.ambiguous || row.checked_until.unwrap_or(0) < end || now <= end {
        return Ok(());
    }
    if let Some(hash) = row.match_hash {
        return settle_positive(row, hash, now).await;
    }
    if !db::tx::query(db::repo::vault_config::recovery_history_verified).map_err(map_db)? {
        defer(row, now)?;
        return Ok(());
    }
    let proven = db::tx::query(|connection| {
        let proof = db::repo::actions::recovery_proof(connection, &row.action_id)?;
        Ok(proof.is_some_and(|proof| {
            proof.worker_epoch == row.worker_epoch
                && !proof.ambiguous
                && proof.match_hash.is_none()
                && proof.checked_until.is_some_and(|cursor| cursor >= end)
        }))
    })
    .map_err(map_db)?;
    if !proven {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let mut proof_bytes = b"recovery_absent".to_vec();
    proof_bytes.extend_from_slice(&row.action_id);
    proof_bytes.extend_from_slice(&end.to_be_bytes());
    if row.kind != "recovery" {
        return settle_transfer(row, false, hl_sign::keccak256(&proof_bytes), now).await;
    }
    let settlement = settlement_event(row, false, hl_sign::keccak256(&proof_bytes), now);
    let ack = journal_client::append_recovery_event("vault", settlement.clone()).await?;
    db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &settlement, &ack)?;
        let proof = db::repo::actions::recovery_proof(connection, &row.action_id)?;
        if !proof.is_some_and(|proof| {
            proof.worker_epoch == row.worker_epoch
                && !proof.ambiguous
                && proof.match_hash.is_none()
                && proof.checked_until.is_some_and(|cursor| cursor >= end)
        }) {
            return Err(db::error::Error::Conflict);
        }
        db::repo::funds::release_reservation(connection, &row.user_id, &row.request_id, now)?;
        db::repo::funds::set_request_state(
            connection,
            &row.user_id,
            &row.request_id,
            api_types::fund::FundRequestState::Rejected,
            now,
        )?;
        if row.action_state == "unknown" {
            db::repo::actions::reconcile_unknown(
                connection,
                &row.action_id,
                row.worker_epoch,
                now,
            )?;
        }
        Ok(())
    })
    .map_err(map_db)
}

fn defer(row: &RecoveryCheck, now: u64) -> Result<(), ErrorCode> {
    db::tx::update(|connection| {
        db::repo::actions::defer_recovery_check(connection, &row.action_id, row.worker_epoch, now)
    })
    .map_err(map_db)
}

fn core_principal() -> Result<Principal, ErrorCode> {
    db::tx::query(db::repo::vault_config::core_principal)
        .map_err(|error| crate::auth::map_db(error, None))?
        .map(|value| Principal::from_slice(&value))
        .ok_or(ErrorCode::PolicyUnavailable)
}

fn call_error() -> ErrorCode {
    ErrorCode::PolicyUnavailable
}

pub async fn prepare(
    account_id: &[u8; 32],
    user_id: &[u8; 32],
    address: &[u8; 20],
    request_id: &[u8],
) -> Result<RecoveryFenceToken, ErrorCode> {
    let args = PrepareRecovery {
        account_id: account_id.to_vec().into(),
        user_id: user_id.to_vec().into(),
        master_address: address.to_vec().into(),
        request_id: request_id.to_vec().into(),
    };
    let response = Call::bounded_wait(core_principal()?, "prepare_recovery")
        .with_arg(args)
        .await
        .map_err(|_| call_error())?;
    response
        .candid::<Result<RecoveryFenceToken, ErrorCode>>()
        .map_err(|_| call_error())?
}

pub async fn commit(token: RecoveryFenceToken) -> Result<(), ErrorCode> {
    let response = Call::bounded_wait(core_principal()?, "commit_recovery")
        .with_arg(token)
        .await
        .map_err(|_| call_error())?;
    response
        .candid::<Result<(), ErrorCode>>()
        .map_err(|_| call_error())?
}

pub async fn mark_unknown(token: RecoveryFenceToken) -> Result<(), ErrorCode> {
    let response = Call::bounded_wait(core_principal()?, "mark_recovery_unknown")
        .with_arg(token)
        .await
        .map_err(|_| call_error())?;
    response
        .candid::<Result<(), ErrorCode>>()
        .map_err(|_| call_error())?
}

pub async fn release(token: RecoveryFenceToken, aborted: bool) -> Result<(), ErrorCode> {
    let method = if aborted {
        "abort_recovery"
    } else {
        "finish_recovery"
    };
    let response = Call::bounded_wait(core_principal()?, method)
        .with_arg(token)
        .await
        .map_err(|_| call_error())?;
    response
        .candid::<Result<(), ErrorCode>>()
        .map_err(|_| call_error())?
}

pub fn token(
    account_id: [u8; 32],
    user_id: [u8; 32],
    request_id: &[u8],
    epoch: u64,
) -> RecoveryFenceToken {
    RecoveryFenceToken {
        account_id: account_id.to_vec().into(),
        user_id: user_id.to_vec().into(),
        request_id: request_id.to_vec().into(),
        epoch,
    }
}

pub async fn release_finished(now: u64) -> Result<(), ErrorCode> {
    let rows =
        db::tx::query(|connection| db::repo::actions::recovery_release_candidates(connection, 4))
            .map_err(|error| crate::auth::map_db(error, None))?;
    for row in rows {
        let Some(attempt) =
            worker_permissions::begin("release", &row.action_id, &row.user_id).map_err(map_db)?
        else {
            continue;
        };
        let token = token(
            row.account_id,
            row.user_id,
            &row.request_id,
            row.fence_epoch,
        );
        if release(token, row.state == "aborted").await.is_ok() {
            db::tx::update(|connection| {
                db::repo::actions::mark_recovery_fence_released(
                    connection,
                    &row.action_id,
                    row.fence_epoch,
                    now,
                )
            })
            .map_err(|error| crate::auth::map_db(error, None))?;
            attempt.completed().map_err(map_db)?;
        }
    }
    Ok(())
}

/// upgrade前に送信された回収を、coreの全体停止中に永続フェンスへ移す。
async fn settle_transfer(
    row: &RecoveryCheck,
    accepted: bool,
    evidence: [u8; 32],
    now: u64,
) -> Result<(), ErrorCode> {
    let (action, request, reserve) = db::tx::query(|c| {
        Ok((
            db::repo::actions::action_row(c, &row.action_id)?.ok_or(db::error::Error::NotFound)?,
            db::repo::funds::fund_request(c, &row.user_id, &row.request_id)?
                .ok_or(db::error::Error::NotFound)?,
            db::repo::ledger::custody_account(c, &row.user_id, api_types::AccountKind::Reserve)?
                .ok_or(db::error::Error::NotFound)?,
        ))
    })
    .map_err(map_db)?;
    let mut event = crate::outbox::fund_transfer_result_event(
        &action,
        &request,
        &row.request_id,
        &reserve.account_id,
        accepted,
        &evidence,
    )?;
    if let RecoveryPayload::FundTransferResult { observed_at_ms, .. } = &mut event.payload {
        *observed_at_ms = now;
    }
    crate::outbox::persist_transfer_result(&event).await
}

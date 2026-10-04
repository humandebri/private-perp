//! 資金API（参照系）。`docs/phase-0/api-contract.md` 2.2。
//!
//! 送金を伴う操作（配分・回収・払出し）はoutboxの署名送信（2C）と合わせて実装する。
//! ここでは認証済みセッションでの参照（資金状態・履歴・入金案内）を提供する。

use crate::auth::{VerifiedSession, map_db};
use crate::clock;
use crate::config;
use api_types::error::{BadRequestCode, ErrorCode, NotAllowedCode};
use api_types::fund::{
    AgentGeneration, AgentState, AllocationRequest, FundEvent, FundRequestAccepted,
    FundRequestState, FundStatus, FundingInstructions, WithdrawalRequest,
};
use api_types::journal::{RecoveryEvent, RecoveryPayload};
use api_types::{AccountKind, Blob, Paged};
use db::error::Error as DbError;
use db::repo::funds::{AcceptOutcome, NewFundRequest, RequestKind};

/// 入金案内。共通保管口座が未作成の間は利用できない（鍵導出は2C）。
pub fn funding_instructions(session: &VerifiedSession) -> Result<FundingInstructions, ErrorCode> {
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &session.user_id, AccountKind::Reserve)
    })
    .map_err(|error| map_db(error, None))?;

    let identity =
        db::tx::query(|connection| db::repo::auth::identity_by_user(connection, &session.user_id))
            .map_err(|error| map_db(error, None))?
            .ok_or(ErrorCode::PolicyUnavailable)?;
    match account {
        Some(account) => Ok(FundingInstructions {
            account_kind: AccountKind::Reserve,
            hl_account_address: account.master_address.to_vec().into(),
            source_hl_account_address: identity.eoa_address.to_vec().into(),
            asset: api_types::AssetId::Usdc,
            network: crate::environment::network()?,
            minimum_amount: None,
            memo_required: false,
        }),
        None => Err(ErrorCode::NotAllowed {
            code: NotAllowedCode::OperationNotAvailable,
        }),
    }
}

/// 資金状態。残高は仕訳から導出し、未確定額を確定残高へ含めない。
pub fn fund_status(session: &VerifiedSession) -> Result<FundStatus, ErrorCode> {
    let now = clock::now_ms();
    let (balances, unknowns, recovery_fence) = db::tx::query(|connection| {
        let balances = db::repo::ledger::user_balances(connection, &session.user_id)?;
        let mut unknowns = db::repo::actions::unresolved_actions(connection, &session.user_id)?;
        let identity = db::repo::auth::identity_by_user(connection, &session.user_id)?
            .ok_or(DbError::NotFound)?;
        unknowns.extend(db::repo::spot_deposits::unresolved(
            connection,
            &identity.eoa_address,
        )?);
        let recovery_fence =
            db::repo::actions::recovery_fence_status(connection, &session.user_id)?;
        Ok((balances, unknowns, recovery_fence))
    })
    .map_err(|error| map_db(error, None))?;

    Ok(FundStatus {
        reserve_unallocated: balances.reserve_unallocated,
        in_transit: balances.in_transit,
        reserved_for_withdrawal: balances.reserved_for_withdrawal,
        // 入出金仕訳と、最後に確認したHL残高観測から導出する。
        trading_equity: balances.trading_equity,
        trading_unrealized_pnl: 0,
        withdrawable: balances.withdrawable,
        observed_at: now,
        revision: 0,
        unknowns: unknowns
            .into_iter()
            .map(|action| api_types::fund::UnresolvedAction {
                action_id: action.action_id.to_vec().into(),
                kind: action.kind,
                state: action.state,
                since: action.since,
            })
            .collect(),
        recovery_fence: recovery_fence.map(|state| {
            if state == "reconciling" {
                api_types::fund::RecoveryFenceStatus::Reconciling
            } else {
                api_types::fund::RecoveryFenceStatus::Preparing
            }
        }),
    })
}

/// 資金履歴（カーソル方式、上限 `MAX_PAGE_SIZE`）。
pub fn fund_events(
    session: &VerifiedSession,
    cursor: Option<Blob>,
    limit: u32,
) -> Result<Paged<FundEvent>, ErrorCode> {
    let limit = limit.clamp(1, config::MAX_PAGE_SIZE);
    let cursor_value = match cursor {
        Some(blob) => Some(decode_cursor(&blob)?),
        None => None,
    };
    let now = clock::now_ms();
    let rows = db::tx::query(|connection| {
        db::repo::events::list_fund_events(connection, &session.user_id, limit, cursor_value)
    })
    .map_err(|error| map_db(error, None))?;

    let next_cursor = if rows.len() as u32 == limit {
        rows.last()
            .map(|row| encode_cursor(row.at, &row.request_id).into())
    } else {
        None
    };

    Ok(Paged {
        items: rows
            .into_iter()
            .map(|row| FundEvent {
                event_id: row.request_id.into(),
                kind: request_kind(row.kind.as_str()),
                amount: row.amount,
                state: row.state,
                at: row.at,
            })
            .collect(),
        next_cursor,
        observed_at: now,
        revision: 0,
    })
}

fn request_kind(value: &str) -> api_types::fund::FundActionKind {
    match value {
        "recovery" => api_types::fund::FundActionKind::Recovery,
        "withdrawal" => api_types::fund::FundActionKind::Withdrawal,
        _ => api_types::fund::FundActionKind::Allocation,
    }
}

fn encode_cursor(at: u64, request_id: &[u8]) -> Vec<u8> {
    let mut bytes = at.to_be_bytes().to_vec();
    bytes.extend_from_slice(request_id);
    bytes
}

fn decode_cursor(blob: &[u8]) -> Result<(u64, Vec<u8>), ErrorCode> {
    if !(9..=72).contains(&blob.len()) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "cursor must contain timestamp and request ID".into(),
        });
    }
    let at = u64::from_be_bytes(blob[..8].try_into().expect("checked cursor length"));
    Ok((at, blob[8..].to_vec()))
}

/// 受付の結果をAPI型へ写す。
fn accepted(request_id: &[u8], state: FundRequestState, now: u64) -> FundRequestAccepted {
    FundRequestAccepted {
        request_id: request_id.to_vec().into(),
        fund_action_id: None,
        state,
        accepted_at: now,
    }
}

/// 要求本文のfingerprint（冪等性の判定に使う）。
fn body_hash(parts: &[&[u8]]) -> [u8; 32] {
    hl_sign::keccak256_concat(parts)
}

fn map_accept(error: DbError, request_id: &[u8]) -> ErrorCode {
    match error {
        DbError::Conflict => ErrorCode::IdempotencyConflict {
            request_id: request_id.to_vec().into(),
        },
        // 出金intentのnonce再利用は「本文相違」ではなく期限・単回使用の違反として返す。
        DbError::Invariant(message) if message.contains("nonce") => ErrorCode::BadRequest {
            code: BadRequestCode::NonceReused,
            detail: message.to_string(),
        },
        other => map_db(other, Some(request_id)),
    }
}

fn allocation_preflight(
    connection: &ic_sqlite_vfs::db::connection::Connection,
    user_id: &[u8; 32],
    request_id: &[u8],
    fingerprint: &[u8; 32],
    amount: u64,
) -> Result<AcceptOutcome, DbError> {
    if let Some(existing) = db::repo::funds::fund_request(connection, user_id, request_id)? {
        return Ok(if existing.body_hash == *fingerprint {
            AcceptOutcome::Duplicate
        } else {
            AcceptOutcome::Conflict
        });
    }
    let available = db::repo::ledger::user_balances(connection, user_id)?
        .reserve_unallocated
        .checked_sub(db::repo::funds::held_allocation_total(connection, user_id)?)
        .ok_or(DbError::Invariant("allocation holds exceed balance"))?;
    if available < amount {
        return Err(DbError::InsufficientFunds {
            available: i64::try_from(available).unwrap_or(i64::MAX),
            requested: i64::try_from(amount).unwrap_or(i64::MAX),
        });
    }
    Ok(AcceptOutcome::Accepted)
}

fn withdrawal_preflight(
    connection: &ic_sqlite_vfs::db::connection::Connection,
    user_id: &[u8; 32],
    request_id: &[u8],
    fingerprint: &[u8; 32],
    amount: u64,
    intent_nonce: u64,
) -> Result<AcceptOutcome, DbError> {
    if let Some(existing) = db::repo::funds::fund_request(connection, user_id, request_id)? {
        return Ok(if existing.body_hash == *fingerprint {
            AcceptOutcome::Duplicate
        } else {
            AcceptOutcome::Conflict
        });
    }
    if db::repo::auth::intent_nonce_used(connection, user_id, intent_nonce)? {
        return Err(DbError::Invariant("intent nonce is already used"));
    }
    let available = db::repo::ledger::user_balances(connection, user_id)?.withdrawable;
    if available < amount {
        return Err(DbError::InsufficientFunds {
            available: i64::try_from(available).unwrap_or(i64::MAX),
            requested: i64::try_from(amount).unwrap_or(i64::MAX),
        });
    }
    Ok(AcceptOutcome::Accepted)
}

fn recovery_preflight(
    connection: &ic_sqlite_vfs::db::connection::Connection,
    user_id: &[u8; 32],
    request_id: &[u8],
    fingerprint: &[u8; 32],
    amount: u64,
    trading_account_id: &[u8; 32],
    reserve_account_id: &[u8; 32],
) -> Result<AcceptOutcome, DbError> {
    if let Some(existing) = db::repo::funds::fund_request(connection, user_id, request_id)? {
        return Ok(if existing.body_hash == *fingerprint {
            AcceptOutcome::Duplicate
        } else {
            AcceptOutcome::Conflict
        });
    }
    let trading = db::repo::ledger::custody_account(connection, user_id, AccountKind::Trading)?
        .ok_or(DbError::NotFound)?;
    let reserve = db::repo::ledger::custody_account(connection, user_id, AccountKind::Reserve)?
        .ok_or(DbError::NotFound)?;
    if trading.account_id != *trading_account_id || reserve.account_id != *reserve_account_id {
        return Err(DbError::Conflict);
    }
    let available = db::repo::ledger::user_balances(connection, user_id)?
        .trading_equity
        .checked_sub(db::repo::funds::held_recovery_total(connection, user_id)?)
        .ok_or(DbError::Invariant("recovery holds exceed trading equity"))?;
    if available < amount {
        return Err(DbError::InsufficientFunds {
            available: i64::try_from(available).unwrap_or(i64::MAX),
            requested: i64::try_from(amount).unwrap_or(i64::MAX),
        });
    }
    Ok(AcceptOutcome::Accepted)
}

/// 配分を要求する（受付＋予約＋actionの作成。署名・送信はoutboxのsweep）。
pub async fn request_allocation(
    session: &VerifiedSession,
    request: &AllocationRequest,
) -> Result<FundRequestAccepted, ErrorCode> {
    if request.amount == 0 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::AmountZero,
            detail: "amount must be positive".to_string(),
        });
    }
    if request.amount > i64::MAX as u64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "amount exceeds ledger range".to_string(),
        });
    }
    if !matches!(request.target, AccountKind::Trading) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "allocation target must be the trading account".to_string(),
        });
    }

    let request_id = request.client_request_id.as_ref();
    if request_id.is_empty() || request_id.len() > 64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "client_request_id must be 1..=64 bytes".to_string(),
        });
    }
    let now = clock::now_ms();
    let fingerprint = body_hash(&[b"allocation", &request.amount.to_be_bytes(), request_id]);

    // 払出し先（取引口座）を先に用意する。宛先を受付行へ保存し、送信時は必ずこの値を
    // 使う（受付後に口座を用意すると、予約確定後に失敗して資金が拘束されたまま残る）。
    let trading = crate::outbox::ensure_trading_account(&session.user_id, now).await?;
    crate::cycles::require_new()?;
    crate::eligibility::require(
        &session.user_id,
        ic_cdk::api::msg_caller(),
        &trading.account_id,
    )?;
    let destination_address = format!("0x{}", hex::encode(trading.master_address));
    let action_id = crate::random::random32().await?;
    // The random source crosses an async boundary. Recheck the conditions that
    // can expire while the action ID is being generated.
    let verified = crate::auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    if verified != *session {
        return Err(ErrorCode::SessionRevoked);
    }
    crate::cycles::require_new()?;
    crate::eligibility::require(
        &session.user_id,
        ic_cdk::api::msg_caller(),
        &trading.account_id,
    )?;
    let now = clock::now_ms();

    match db::tx::query(|c| {
        allocation_preflight(
            c,
            &session.user_id,
            request_id,
            &fingerprint,
            request.amount,
        )
    })
    .map_err(|error| map_accept(error, request_id))?
    {
        AcceptOutcome::Duplicate => {
            return Ok(accepted(request_id, FundRequestState::Accepted, now));
        }
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            });
        }
        AcceptOutcome::Accepted => {}
    }
    let nonce = db::tx::query(|c| db::repo::actions::next_master_nonce(c, "reserve", now))
        .map_err(|error| map_accept(error, request_id))?;
    let mut logical = b"allocation_accepted".to_vec();
    logical.extend_from_slice(&session.user_id);
    logical.extend_from_slice(request_id);
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::AllocationAccepted {
            action_id: action_id.to_vec().into(),
            request_id: request_id.to_vec().into(),
            user_id: session.user_id.to_vec().into(),
            account_id: trading.account_id.to_vec().into(),
            destination: trading.master_address.to_vec().into(),
            amount_micros: request.amount,
            body_hash: fingerprint.to_vec().into(),
            nonce,
            accepted_at_ms: now,
        },
    };
    let ack = journal_client::append_recovery_event_if("vault", event.clone(), |c| {
        if db::repo::actions::next_master_nonce(c, "reserve", now)? != nonce {
            return Ok(false);
        }
        match allocation_preflight(
            c,
            &session.user_id,
            request_id,
            &fingerprint,
            request.amount,
        )? {
            AcceptOutcome::Accepted => Ok(true),
            AcceptOutcome::Duplicate | AcceptOutcome::Conflict => Ok(false),
        }
    })
    .await?;
    let Some(ack) = ack else {
        let existing =
            db::tx::query(|c| db::repo::funds::fund_request(c, &session.user_id, request_id))
                .map_err(|error| map_accept(error, request_id))?;
        return match existing {
            Some(existing) if existing.body_hash == fingerprint => {
                Ok(accepted(request_id, existing.state, clock::now_ms()))
            }
            Some(_) => Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            }),
            None => Err(ErrorCode::PolicyUnavailable),
        };
    };
    let still_valid = crate::auth::verify_session(&request.session, ic_cdk::api::msg_caller())
        .and_then(|verified| {
            if verified != *session {
                Err(ErrorCode::SessionRevoked)
            } else {
                crate::cycles::require_new()?;
                crate::eligibility::require(
                    &session.user_id,
                    ic_cdk::api::msg_caller(),
                    &trading.account_id,
                )
            }
        });
    let result = db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &event, &ack)?;
        let admitted = still_valid.is_ok()
            && matches!(
                allocation_preflight(
                    connection,
                    &session.user_id,
                    request_id,
                    &fingerprint,
                    request.amount
                ),
                Ok(AcceptOutcome::Accepted)
            );
        let accepted = db::repo::funds::accept_fund_request(
            connection,
            &NewFundRequest {
                user_id: &session.user_id,
                client_request_id: request_id,
                body_hash: &fingerprint,
                kind: RequestKind::Allocation,
                account_id: Some(&trading.account_id),
                amount: request.amount,
                destination: Some(destination_address.as_str()),
            },
            now,
        )?;
        match accepted {
            AcceptOutcome::Accepted => {
                if !admitted {
                    db::repo::funds::set_request_state(
                        connection,
                        &session.user_id,
                        request_id,
                        FundRequestState::Rejected,
                        now,
                    )?;
                    return Ok(AcceptOutcome::Accepted);
                }
                db::repo::funds::reserve_funds(
                    connection,
                    &session.user_id,
                    request_id,
                    &db::repo::ledger::user_reserve(&session.user_id),
                    request.amount,
                    now,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &session.user_id,
                    request_id,
                    FundRequestState::Reserved,
                    now,
                )?;
                let allocated_nonce =
                    db::repo::actions::allocate_master_nonce(connection, "reserve", now)?;
                if allocated_nonce != nonce {
                    return Err(DbError::Conflict);
                }
                let payload = crate::venue::UsdSend {
                    destination: destination_address.clone(),
                    amount_micros: request.amount,
                    time: nonce,
                };
                let action = db::repo::actions::NewFundAction {
                    action_id,
                    user_id: session.user_id,
                    client_request_id: Some(request_id.to_vec()),
                    kind: "allocation".to_string(),
                    signer_id: "reserve".to_string(),
                    canonical_action: payload
                        .body(&test_signature_placeholder())
                        .map_err(|_| DbError::Invariant("invalid allocation action"))?,
                    digest: payload
                        .digest()
                        .map_err(|_| DbError::Invariant("invalid allocation digest"))?,
                    nonce,
                };
                db::repo::actions::insert_fund_action(connection, &action, now)?;
                db::repo::events::insert_audit(
                    connection,
                    "user",
                    "request_allocation",
                    None,
                    Some("reserved"),
                    now,
                )?;
                Ok(AcceptOutcome::Accepted)
            }
            AcceptOutcome::Duplicate | AcceptOutcome::Conflict => Err(DbError::Conflict),
        }
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            journal_client::lock()?;
            return Err(map_accept(error, request_id));
        }
    };

    still_valid?;
    let rejected = db::tx::query(|connection| {
        Ok(
            db::repo::funds::fund_request(connection, &session.user_id, request_id)?
                .is_some_and(|request| request.state == FundRequestState::Rejected),
        )
    })
    .map_err(|error| map_accept(error, request_id))?;
    if rejected {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let state = match outcome {
        AcceptOutcome::Accepted => FundRequestState::Reserved,
        AcceptOutcome::Duplicate => FundRequestState::Accepted,
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            });
        }
    };

    Ok(accepted(request_id, state, now))
}

/// 受付時点のcanonical action（署名前のため署名欄は空のまま保存する）。
fn test_signature_placeholder() -> hl_sign::Signature {
    hl_sign::Signature {
        r: [0u8; 32],
        s: [0u8; 32],
        v: 27,
    }
}

/// 出金を要求する（本人署名の検証＋受付＋予約。払出しは署名段階）。
pub async fn request_withdrawal(
    session: &VerifiedSession,
    request: &WithdrawalRequest,
) -> Result<FundRequestAccepted, ErrorCode> {
    if request.amount == 0 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::AmountZero,
            detail: "amount must be positive".to_string(),
        });
    }
    if request.amount > i64::MAX as u64 || request.nonce > i64::MAX as u64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "amount or intent nonce exceeds storage range".to_string(),
        });
    }
    let request_id = request.client_request_id.as_ref();
    if request_id.is_empty() || request_id.len() > 64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "client_request_id must be 1..=64 bytes".to_string(),
        });
    }
    if !matches!(
        request.destination,
        api_types::fund::Destination::AuthenticatedEoaHlAccount
    ) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::DestinationNotAllowed,
            detail: "destination must be the authenticated EOA's Hyperliquid account".to_string(),
        });
    }
    let now = clock::now_ms();
    if request.expires_at <= now {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::ExpiredIntent,
            detail: "intent has expired".to_string(),
        });
    }
    if request.network != crate::environment::network()? {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::NetworkMismatch,
            detail: "intent network does not match".to_string(),
        });
    }

    // 本人のEOAを解決し、EOA束縛のintentを検証する。
    let eoa =
        db::tx::query(|connection| db::repo::auth::identity_by_user(connection, &session.user_id))
            .map_err(|error| map_db(error, None))?
            .ok_or(ErrorCode::Unauthenticated {
                reason: "identity not found".to_string(),
            })?
            .eoa_address;

    let intent = hl_sign::private_perp::Withdrawal {
        eoa,
        amount: request.amount,
        asset: "usdc".to_string(),
        destination: format!("0x{}", hex::encode(eoa)),
        network: config::network_name(request.network).to_string(),
        nonce: request.nonce,
        expires_at: request.expires_at,
        canister: ic_cdk::api::canister_self().as_slice().to_vec(),
    };
    let digest = intent.digest().map_err(|error| ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: error.to_string(),
    })?;
    let signature_bytes: [u8; 65] =
        request
            .intent_signature
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::BadRequest {
                code: BadRequestCode::InvalidSignature,
                detail: "intent_signature must be 65 bytes".to_string(),
            })?;
    let signature = hl_sign::Signature::from_bytes65(&signature_bytes).map_err(|error| {
        ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: error.to_string(),
        }
    })?;
    let recovered = hl_sign::recover_address(&digest, &signature, None).map_err(|error| {
        ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: error.to_string(),
        }
    })?;
    if recovered != eoa {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: "intent signature does not match the EOA".to_string(),
        });
    }
    let reserve = crate::outbox::ensure_custody_account(
        &session.user_id,
        AccountKind::Reserve,
        clock::now_ms(),
    )
    .await?;

    let fingerprint = body_hash(&[
        b"withdrawal",
        &request.amount.to_be_bytes(),
        &request.nonce.to_be_bytes(),
        &request.expires_at.to_be_bytes(),
        &eoa,
        request_id,
    ]);

    // 払出し先は認証済みEOAのHL口座（`0x`＋アドレス）。
    let destination_address = format!("0x{}", hex::encode(eoa));
    let action_id = crate::random::random32().await?;
    let verified = crate::auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    if verified != *session {
        return Err(ErrorCode::SessionRevoked);
    }
    let now = clock::now_ms();
    if request.expires_at <= now {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::ExpiredIntent,
            detail: "intent has expired".to_string(),
        });
    }

    match db::tx::query(|connection| {
        withdrawal_preflight(
            connection,
            &session.user_id,
            request_id,
            &fingerprint,
            request.amount,
            request.nonce,
        )
    })
    .map_err(|error| map_accept(error, request_id))?
    {
        AcceptOutcome::Duplicate => {
            return Ok(accepted(request_id, FundRequestState::Accepted, now));
        }
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            });
        }
        AcceptOutcome::Accepted => {}
    }
    let nonce = db::tx::query(|connection| {
        db::repo::actions::next_master_nonce(connection, "reserve", now)
    })
    .map_err(|error| map_accept(error, request_id))?;
    let mut logical = b"withdrawal_accepted".to_vec();
    logical.extend_from_slice(&session.user_id);
    logical.extend_from_slice(request_id);
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::WithdrawalAccepted {
            action_id: action_id.to_vec().into(),
            request_id: request_id.to_vec().into(),
            user_id: session.user_id.to_vec().into(),
            reserve_account_id: reserve.account_id.to_vec().into(),
            destination: eoa.to_vec().into(),
            amount_micros: request.amount,
            body_hash: fingerprint.to_vec().into(),
            nonce,
            intent_nonce: request.nonce,
            intent_expires_at_ms: request.expires_at,
            accepted_at_ms: now,
        },
    };
    let ack = journal_client::append_recovery_event_if("vault", event.clone(), |connection| {
        if db::repo::actions::next_master_nonce(connection, "reserve", now)? != nonce {
            return Ok(false);
        }
        match withdrawal_preflight(
            connection,
            &session.user_id,
            request_id,
            &fingerprint,
            request.amount,
            request.nonce,
        )? {
            AcceptOutcome::Accepted => Ok(true),
            AcceptOutcome::Duplicate | AcceptOutcome::Conflict => Ok(false),
        }
    })
    .await?;
    let Some(ack) = ack else {
        let existing = db::tx::query(|connection| {
            db::repo::funds::fund_request(connection, &session.user_id, request_id)
        })
        .map_err(|error| map_accept(error, request_id))?;
        return match existing {
            Some(existing) if existing.body_hash == fingerprint => {
                Ok(accepted(request_id, existing.state, clock::now_ms()))
            }
            Some(_) => Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            }),
            None => Err(ErrorCode::PolicyUnavailable),
        };
    };
    let still_valid = crate::auth::verify_session(&request.session, ic_cdk::api::msg_caller())
        .and_then(|verified| {
            if verified != *session {
                Err(ErrorCode::SessionRevoked)
            } else if request.expires_at <= clock::now_ms() {
                Err(ErrorCode::BadRequest {
                    code: BadRequestCode::ExpiredIntent,
                    detail: "intent has expired".to_string(),
                })
            } else {
                Ok(())
            }
        });
    let result = db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &event, &ack)?;
        let admitted = still_valid.is_ok()
            && matches!(
                withdrawal_preflight(
                    connection,
                    &session.user_id,
                    request_id,
                    &fingerprint,
                    request.amount,
                    request.nonce
                ),
                Ok(AcceptOutcome::Accepted)
            );
        let accepted = db::repo::funds::accept_fund_request(
            connection,
            &NewFundRequest {
                user_id: &session.user_id,
                client_request_id: request_id,
                body_hash: &fingerprint,
                kind: RequestKind::Withdrawal,
                account_id: None,
                amount: request.amount,
                destination: Some(destination_address.as_str()),
            },
            now,
        )?;
        match accepted {
            AcceptOutcome::Accepted => {
                if !admitted {
                    db::repo::funds::set_request_state(
                        connection,
                        &session.user_id,
                        request_id,
                        FundRequestState::Rejected,
                        now,
                    )?;
                    return Ok(AcceptOutcome::Accepted);
                }
                // 署名済みintentのnonceは単回使用にする（同じ署名を別の受付IDで
                // 再送して二重に資金移動させない）。
                if let Err(error) = db::repo::auth::use_intent_nonce(
                    connection,
                    &session.user_id,
                    request.nonce,
                    request_id,
                    now,
                ) {
                    return Err(match error {
                        DbError::Conflict => DbError::Invariant("intent nonce is already used"),
                        other => other,
                    });
                }
                db::repo::funds::reserve_funds(
                    connection,
                    &session.user_id,
                    request_id,
                    &db::repo::ledger::user_reserve(&session.user_id),
                    request.amount,
                    now,
                )?;
                // 複式台帳でも予約へ移す（`user_reserve`→`user_reserved_for_withdrawal`）。
                db::repo::ledger::withdrawal_reserve(
                    connection,
                    &session.user_id,
                    request.amount,
                    now,
                    request_id,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &session.user_id,
                    request_id,
                    FundRequestState::Reserved,
                    now,
                )?;
                let allocated_nonce =
                    db::repo::actions::allocate_master_nonce(connection, "reserve", now)?;
                if allocated_nonce != nonce {
                    return Err(DbError::Conflict);
                }
                let payload = crate::venue::UsdSend {
                    destination: destination_address.clone(),
                    amount_micros: request.amount,
                    time: nonce,
                };
                let action = db::repo::actions::NewFundAction {
                    action_id,
                    user_id: session.user_id,
                    client_request_id: Some(request_id.to_vec()),
                    kind: "withdrawal".to_string(),
                    signer_id: "reserve".to_string(),
                    canonical_action: payload
                        .body(&test_signature_placeholder())
                        .map_err(|_| DbError::Invariant("invalid withdrawal action"))?,
                    digest: payload
                        .digest()
                        .map_err(|_| DbError::Invariant("invalid withdrawal digest"))?,
                    nonce,
                };
                db::repo::actions::insert_fund_action(connection, &action, now)?;
                db::repo::events::insert_audit(
                    connection,
                    "user",
                    "request_withdrawal",
                    None,
                    Some("reserved"),
                    now,
                )?;
                Ok(AcceptOutcome::Accepted)
            }
            AcceptOutcome::Duplicate | AcceptOutcome::Conflict => Err(DbError::Conflict),
        }
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            journal_client::lock()?;
            return Err(map_accept(error, request_id));
        }
    };

    still_valid?;
    let rejected = db::tx::query(|connection| {
        Ok(
            db::repo::funds::fund_request(connection, &session.user_id, request_id)?
                .is_some_and(|request| request.state == FundRequestState::Rejected),
        )
    })
    .map_err(|error| map_accept(error, request_id))?;
    if rejected {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let state = match outcome {
        AcceptOutcome::Accepted => FundRequestState::Reserved,
        AcceptOutcome::Duplicate => FundRequestState::Accepted,
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            });
        }
    };
    Ok(accepted(request_id, state, now))
}

/// 拘束中の予約を差し引いた出金可能額を含む資金状態を返す。
///
/// 拘束は `ledger::user_balances` が種別ごとに一度だけ数える（配分は`reservations`表、
/// 出金は台帳）。ここで再度引かない。
pub fn fund_status_with_holds(session: &VerifiedSession) -> Result<FundStatus, ErrorCode> {
    fund_status(session)
}

/// テスト専用の入金計上（`test-venue` featureでのみコンパイルされる）。
#[cfg(feature = "test-venue")]
pub fn test_credit_deposit(
    session: &VerifiedSession,
    amount: u64,
    event_id: &[u8; 32],
) -> Result<(), ErrorCode> {
    let now = clock::now_ms();
    let network = crate::environment::network_name()?;
    db::tx::update(|connection| {
        let event = db::repo::events::ExternalEvent {
            event_id: *event_id,
            network: network.clone(),
            account_address: [0u8; 20],
            counterparty: [1u8; 20],
            asset: "usdc".to_string(),
            amount,
            kind: "deposit".to_string(),
            at: now,
            evidence_ref: Some("test-venue".to_string()),
        };
        if !db::repo::events::ingest_external_event(connection, &event, now)? {
            return Err(DbError::Invariant("duplicate test deposit"));
        }
        db::repo::ledger::deposit_confirmed(connection, &session.user_id, amount, now, event_id)?;
        Ok(())
    })
    .map_err(|error| map_db(error, None))
}

/// Agentアドレスを承認する（master鍵で`approveAgent`を署名して送信する）。
///
/// 鍵は `trading_core` が導出・保管するため、vaultは**渡されたアドレス**をmaster署名で
/// 承認する（`Implementation.md` 7章）。承認結果は `agent_generations` へ**永続化**する。
/// 取引所の応答が不明な場合は `Active` と偽らず、`requested` のまま失敗を返す
/// （同じアドレスの再承認は取引所側で冪等）。
pub async fn approve_agent_generation(
    session: &VerifiedSession,
    generation: u64,
    agent_address: [u8; 20],
) -> Result<AgentGeneration, ErrorCode> {
    if generation == 0 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "generation must be positive".to_string(),
        });
    }
    let now = clock::now_ms();
    // Agent承認は取引口座のmaster鍵で署名する（署名者＝保存済みの取引口座）。
    let (account, master_path, master_public_key) =
        crate::outbox::signer_material(&session.user_id, AccountKind::Trading, now).await?;
    let account_id = account.account_id;

    // 既に同じ世代の行があれば、同じアドレスの再承認だけを冪等に扱う。
    if let Some(existing) = db::tx::query(|connection| {
        db::repo::agents::generation(connection, &account_id, generation)
    })
    .map_err(|error| map_db(error, None))?
    {
        if existing.agent_address.as_ref() != agent_address.as_slice() {
            return Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                detail: "generation is already used by another agent address".to_string(),
            });
        }
        if existing.state == AgentState::Active {
            return Ok(existing);
        }
        if existing.state == AgentState::Approving {
            return Err(ErrorCode::ReservationConflict);
        }
    } else {
        db::tx::update(|connection| {
            db::repo::agents::insert_generation(
                connection,
                &account_id,
                generation,
                &agent_address,
                "private-perp/agent",
                now,
            )
        })
        .map_err(|error| map_db(error, None))?;
    }

    let payload = crate::venue::ApproveAgent {
        agent: agent_address,
        name: "private-perp".to_string(),
        time: now,
    };
    let digest = payload.digest()?;
    let signature = crate::crypto::sign_with_key(&digest, master_path, &master_public_key).await?;

    let permit =
        crate::rest_budget::acquire(api_types::operations::BudgetClass::NewRisk, 1).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    // A definitive venue rejection permits a new attempt. It must have a new
    // journal request ID even when the canister clock returns the same ms.
    // An uncertain POST stays `approving` and cannot reach this branch again.
    let request_id = crate::random::random32().await?;
    let intent = journal_client::intent("agent", &request_id, &account_id, now, &digest);
    let ack = journal_client::append("vault", intent.clone()).await?;
    db::tx::update(|connection| {
        journal_client::record(connection, &intent, &ack)?;
        db::repo::agents::mark_approving(connection, &account_id, generation)
    })
    .map_err(|error| map_db(error, None))?;
    match crate::venue::post_approve_agent(&payload, &signature, &permit).await {
        Ok((crate::venue::ExchangeOutcome::Accepted, _response)) => {
            db::tx::update(|connection| {
                db::repo::agents::mark_active(connection, &account_id, generation, now)?;
                db::repo::events::insert_audit(
                    connection,
                    "user",
                    "approve_agent_generation",
                    None,
                    Some("active"),
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
            let stored = db::tx::query(|connection| {
                db::repo::agents::generation(connection, &account_id, generation)
            })
            .map_err(|error| map_db(error, None))?
            .ok_or_else(|| ErrorCode::Internal {
                code: "approved generation disappeared".to_string(),
            })?;
            Ok(stored)
        }
        Ok((crate::venue::ExchangeOutcome::Rejected { message }, _response)) => {
            db::tx::update(|connection| {
                db::repo::agents::mark_failed(connection, &account_id, generation)?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "approve_agent_generation",
                    None,
                    Some("rejected"),
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
            Err(ErrorCode::UpstreamRejected {
                code: message,
                retryable: false,
            })
        }
        // 送信した可能性がある。`Active` とは扱わず、`requested` のまま失敗を返す。
        Err(error) => Err(error),
    }
}

/// 回収（trading口座→準備口座）を要求する（受付時にactionを登録する）。
pub async fn request_recovery(
    session: &VerifiedSession,
    session_handle: &api_types::auth::SessionHandle,
    client_request_id: &[u8],
    amount: u64,
) -> Result<FundRequestAccepted, ErrorCode> {
    if amount == 0 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::AmountZero,
            detail: "amount must be positive".to_string(),
        });
    }
    if amount > i64::MAX as u64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "amount exceeds ledger range".to_string(),
        });
    }
    if client_request_id.is_empty() || client_request_id.len() > 64 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "client_request_id must be 1..=64 bytes".to_string(),
        });
    }
    // Preserve idempotent retries without another external observation.
    if db::tx::query(|c| db::repo::funds::fund_request(c, &session.user_id, client_request_id))
        .map_err(|e| map_db(e, None))?
        .is_none()
    {
        crate::balance::refresh(&session.user_id).await?;
    }
    let trading = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &session.user_id, AccountKind::Trading)
    })
    .map_err(|error| map_db(error, None))?
    .ok_or(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::OperationNotAvailable,
    })?;
    let reserve = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &session.user_id, AccountKind::Reserve)
    })
    .map_err(|error| map_db(error, None))?
    .ok_or(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::OperationNotAvailable,
    })?;
    let destination = format!("0x{}", hex::encode(reserve.master_address));

    let fingerprint = body_hash(&[b"recovery", &amount.to_be_bytes(), client_request_id]);
    let action_id = crate::random::random32().await?;
    let verified = crate::auth::verify_session(session_handle, ic_cdk::api::msg_caller())?;
    if verified != *session {
        return Err(ErrorCode::SessionRevoked);
    }
    let now = clock::now_ms();
    match db::tx::query(|connection| {
        recovery_preflight(
            connection,
            &session.user_id,
            client_request_id,
            &fingerprint,
            amount,
            &trading.account_id,
            &reserve.account_id,
        )
    })
    .map_err(|error| map_accept(error, client_request_id))?
    {
        AcceptOutcome::Duplicate => {
            return Ok(accepted(client_request_id, FundRequestState::Accepted, now));
        }
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: client_request_id.to_vec().into(),
            });
        }
        AcceptOutcome::Accepted => {}
    }
    let signer_id = format!("trading:{}", hex::encode(trading.account_id));
    let nonce = db::tx::query(|connection| {
        db::repo::actions::next_master_nonce(connection, &signer_id, now)
    })
    .map_err(|error| map_accept(error, client_request_id))?;
    let mut logical = b"recovery_accepted".to_vec();
    logical.extend_from_slice(&session.user_id);
    logical.extend_from_slice(client_request_id);
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::RecoveryAccepted {
            action_id: action_id.to_vec().into(),
            request_id: client_request_id.to_vec().into(),
            user_id: session.user_id.to_vec().into(),
            trading_account_id: trading.account_id.to_vec().into(),
            reserve_account_id: reserve.account_id.to_vec().into(),
            destination: reserve.master_address.to_vec().into(),
            amount_micros: amount,
            body_hash: fingerprint.to_vec().into(),
            nonce,
            accepted_at_ms: now,
        },
    };
    let ack = journal_client::append_recovery_event_if("vault", event.clone(), |connection| {
        if db::repo::actions::next_master_nonce(connection, &signer_id, now)? != nonce {
            return Ok(false);
        }
        match recovery_preflight(
            connection,
            &session.user_id,
            client_request_id,
            &fingerprint,
            amount,
            &trading.account_id,
            &reserve.account_id,
        )? {
            AcceptOutcome::Accepted => Ok(true),
            AcceptOutcome::Duplicate | AcceptOutcome::Conflict => Ok(false),
        }
    })
    .await?;
    let Some(ack) = ack else {
        let existing = db::tx::query(|connection| {
            db::repo::funds::fund_request(connection, &session.user_id, client_request_id)
        })
        .map_err(|error| map_accept(error, client_request_id))?;
        return match existing {
            Some(existing) if existing.body_hash == fingerprint => {
                Ok(accepted(client_request_id, existing.state, clock::now_ms()))
            }
            Some(_) => Err(ErrorCode::IdempotencyConflict {
                request_id: client_request_id.to_vec().into(),
            }),
            None => Err(ErrorCode::PolicyUnavailable),
        };
    };
    let still_valid = crate::auth::verify_session(session_handle, ic_cdk::api::msg_caller())
        .and_then(|verified| {
            if verified == *session {
                Ok(())
            } else {
                Err(ErrorCode::SessionRevoked)
            }
        });
    let result = db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &event, &ack)?;
        let admitted = still_valid.is_ok()
            && matches!(
                recovery_preflight(
                    connection,
                    &session.user_id,
                    client_request_id,
                    &fingerprint,
                    amount,
                    &trading.account_id,
                    &reserve.account_id
                ),
                Ok(AcceptOutcome::Accepted)
            );
        let accepted = db::repo::funds::accept_fund_request(
            connection,
            &NewFundRequest {
                user_id: &session.user_id,
                client_request_id,
                body_hash: &fingerprint,
                kind: RequestKind::Recovery,
                account_id: Some(&trading.account_id),
                amount,
                destination: Some(destination.as_str()),
            },
            now,
        )?;
        if accepted == AcceptOutcome::Accepted && !admitted {
            db::repo::funds::set_request_state(
                connection,
                &session.user_id,
                client_request_id,
                FundRequestState::Rejected,
                now,
            )?;
            return Ok(AcceptOutcome::Accepted);
        }
        if accepted == AcceptOutcome::Accepted {
            // 回収は取引口座から出るため、取引口座のequityに対して拘束する
            // （拘束しないと同じequityへ複数の回収が同時に送信され得る）。
            db::repo::funds::reserve_trading_funds(
                connection,
                &session.user_id,
                client_request_id,
                "user_trading",
                amount,
                now,
            )?;
            db::repo::funds::set_request_state(
                connection,
                &session.user_id,
                client_request_id,
                FundRequestState::Reserved,
                now,
            )?;
            let allocated_nonce =
                db::repo::actions::allocate_master_nonce(connection, &signer_id, now)?;
            if allocated_nonce != nonce {
                return Err(DbError::Conflict);
            }
            let payload = crate::venue::UsdSend {
                destination: destination.clone(),
                amount_micros: amount,
                time: nonce,
            };
            let action = db::repo::actions::NewFundAction {
                action_id,
                user_id: session.user_id,
                client_request_id: Some(client_request_id.to_vec()),
                kind: "recovery".to_string(),
                signer_id: "trading".to_string(),
                canonical_action: payload
                    .body(&test_signature_placeholder())
                    .map_err(|_| DbError::Invariant("invalid recovery action"))?,
                digest: payload
                    .digest()
                    .map_err(|_| DbError::Invariant("invalid recovery digest"))?,
                nonce,
            };
            db::repo::actions::insert_fund_action(connection, &action, now)?;
            db::repo::events::insert_audit(
                connection,
                "user",
                "request_recovery",
                None,
                Some("reserved"),
                now,
            )?;
        }
        match accepted {
            AcceptOutcome::Accepted => Ok(AcceptOutcome::Accepted),
            AcceptOutcome::Duplicate | AcceptOutcome::Conflict => Err(DbError::Conflict),
        }
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            journal_client::lock()?;
            return Err(map_accept(error, client_request_id));
        }
    };

    still_valid?;
    let rejected = db::tx::query(|connection| {
        Ok(
            db::repo::funds::fund_request(connection, &session.user_id, client_request_id)?
                .is_some_and(|request| request.state == FundRequestState::Rejected),
        )
    })
    .map_err(|error| map_accept(error, client_request_id))?;
    if rejected {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let state = match outcome {
        AcceptOutcome::Accepted => FundRequestState::Reserved,
        AcceptOutcome::Duplicate => FundRequestState::Accepted,
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: client_request_id.to_vec().into(),
            });
        }
    };

    Ok(accepted(client_request_id, state, now))
}

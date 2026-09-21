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
use api_types::{AccountKind, Blob, Paged};
use db::error::Error as DbError;
use db::repo::funds::{AcceptOutcome, NewFundRequest, RequestKind};

/// 入金案内。共通保管口座が未作成の間は利用できない（鍵導出は2C）。
pub fn funding_instructions(session: &VerifiedSession) -> Result<FundingInstructions, ErrorCode> {
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &session.user_id, AccountKind::Reserve)
    })
    .map_err(|error| map_db(error, None))?;

    match account {
        Some(account) => Ok(FundingInstructions {
            account_kind: AccountKind::Reserve,
            hl_account_address: account.master_address.to_vec().into(),
            asset: api_types::AssetId::Usdc,
            network: config::NETWORK,
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
    let (balances, unknowns) = db::tx::query(|connection| {
        let balances = db::repo::ledger::user_balances(connection, &session.user_id)?;
        let unknowns = db::repo::actions::unresolved_actions(connection, &session.user_id)?;
        Ok((balances, unknowns))
    })
    .map_err(|error| map_db(error, None))?;

    Ok(FundStatus {
        reserve_unallocated: balances.reserve_unallocated,
        in_transit: balances.in_transit,
        reserved_for_withdrawal: balances.reserved_for_withdrawal,
        // ローカルキャッシュ値（モックベニューの照合値）。実HLの照合値ではない。
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
        rows.last().map(|row| encode_cursor(row.at).to_vec().into())
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

fn encode_cursor(at: u64) -> [u8; 8] {
    at.to_be_bytes()
}

fn decode_cursor(blob: &[u8]) -> Result<u64, ErrorCode> {
    let bytes: [u8; 8] = blob.try_into().map_err(|_| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: "cursor must be 8 bytes".to_string(),
    })?;
    Ok(u64::from_be_bytes(bytes))
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
    let destination_address = format!("0x{}", hex::encode(trading.master_address));

    let outcome = db::tx::update(|connection| {
        let accepted = db::repo::funds::accept_fund_request(
            connection,
            &NewFundRequest {
                user_id: &session.user_id,
                client_request_id: request_id,
                body_hash: &fingerprint,
                kind: RequestKind::Allocation,
                account_id: None,
                amount: request.amount,
                destination: Some(destination_address.as_str()),
            },
            now,
        )?;
        match accepted {
            AcceptOutcome::Accepted => {
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
            other => Ok(other),
        }
    })
    .map_err(|error| map_accept(error, request_id))?;

    let state = match outcome {
        AcceptOutcome::Accepted => FundRequestState::Reserved,
        AcceptOutcome::Duplicate => FundRequestState::Accepted,
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            });
        }
    };

    // 新規受付のときだけ、準備口座からの配分actionを登録する（署名・送信はsweep）。
    if state == FundRequestState::Reserved {
        let nonce = db::tx::update(|connection| {
            db::repo::actions::allocate_master_nonce(connection, "reserve", now)
        })
        .map_err(|error| map_db(error, None))?;
        let payload = crate::venue::UsdSend {
            destination: destination_address,
            amount_micros: request.amount,
            time: nonce,
        };
        let digest = payload.digest()?;
        let action_id = crate::random::random32().await?;
        let action = db::repo::actions::NewFundAction {
            action_id,
            user_id: session.user_id,
            client_request_id: Some(request_id.to_vec()),
            kind: "allocation".to_string(),
            signer_id: "reserve".to_string(),
            canonical_action: payload.body(&test_signature_placeholder())?,
            digest,
            nonce,
        };
        db::tx::update(|connection| {
            db::repo::actions::insert_fund_action(connection, &action, now)
        })
        .map_err(|error| map_accept(error, request_id))?;
    }

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
    if request.network != config::NETWORK {
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

    let outcome = db::tx::update(|connection| {
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
            other => Ok(other),
        }
    })
    .map_err(|error| map_accept(error, request_id))?;

    let state = match outcome {
        AcceptOutcome::Accepted => FundRequestState::Reserved,
        AcceptOutcome::Duplicate => FundRequestState::Accepted,
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: request_id.to_vec().into(),
            });
        }
    };
    // 新規受付のときだけ、準備口座からの払出しactionを登録する（sweepが署名・送信する）。
    if state == FundRequestState::Reserved {
        let nonce = db::tx::update(|connection| {
            db::repo::actions::allocate_master_nonce(connection, "reserve", now)
        })
        .map_err(|error| map_db(error, None))?;
        let payload = crate::venue::UsdSend {
            destination: destination_address.clone(),
            amount_micros: request.amount,
            time: nonce,
        };
        let digest = payload.digest()?;
        let action_id = crate::random::random32().await?;
        let action = db::repo::actions::NewFundAction {
            action_id,
            user_id: session.user_id,
            client_request_id: Some(request_id.to_vec()),
            kind: "withdrawal".to_string(),
            signer_id: "reserve".to_string(),
            canonical_action: payload.body(&test_signature_placeholder())?,
            digest,
            nonce,
        };
        db::tx::update(|connection| {
            db::repo::actions::insert_fund_action(connection, &action, now)
        })
        .map_err(|error| map_accept(error, request_id))?;
    }

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
    db::tx::update(|connection| {
        let event = db::repo::events::ExternalEvent {
            event_id: *event_id,
            network: config::network_name(config::NETWORK).to_string(),
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
/// 承認する（`Implementation.md` 7章）。
pub async fn approve_agent_generation(
    session: &VerifiedSession,
    agent_address: [u8; 20],
) -> Result<AgentGeneration, ErrorCode> {
    let now = clock::now_ms();
    // Agent承認は取引口座のmaster鍵で署名する（署名者＝保存済みの取引口座）。
    let (account, master_path, master_public_key) =
        crate::outbox::signer_material(&session.user_id, AccountKind::Trading, now).await?;
    let account_id = account.account_id;

    let latest = db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
        .map_err(|error| map_db(error, None))?;
    let generation = latest.as_ref().map(|row| row.generation).unwrap_or(1);

    let payload = crate::venue::ApproveAgent {
        agent: agent_address,
        name: format!("private-perp gen {generation}"),
        time: now,
    };
    let digest = payload.digest()?;
    let signature = crate::crypto::sign_with_key(&digest, master_path, &master_public_key).await?;

    match crate::venue::post_approve_agent(&payload, &signature).await {
        Ok((crate::venue::ExchangeOutcome::Accepted, _response)) => {
            db::tx::update(|connection| {
                if let Some(row) = db::repo::agents::latest(connection, &account_id)? {
                    db::repo::agents::mark_active(connection, &account_id, row.generation, now)?;
                }
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
            Ok(AgentGeneration {
                account_id: account_id.to_vec().into(),
                generation,
                agent_address: agent_address.to_vec().into(),
                approved_at: Some(now),
                expires_at: None,
                state: AgentState::Active,
            })
        }
        Ok((crate::venue::ExchangeOutcome::Rejected { message }, _response)) => {
            Err(ErrorCode::UpstreamRejected {
                code: message,
                retryable: false,
            })
        }
        Err(error) => Err(error),
    }
}

/// 回収（trading口座→準備口座）を要求する（受付時にactionを登録する）。
pub async fn request_recovery(
    session: &VerifiedSession,
    client_request_id: &[u8],
    amount: u64,
) -> Result<FundRequestAccepted, ErrorCode> {
    let now = clock::now_ms();
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
    let outcome = db::tx::update(|connection| {
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
        if accepted == AcceptOutcome::Accepted {
            db::repo::funds::set_request_state(
                connection,
                &session.user_id,
                client_request_id,
                FundRequestState::Reserved,
                now,
            )?;
            db::repo::events::insert_audit(
                connection,
                "user",
                "request_recovery",
                None,
                Some("reserved"),
                now,
            )?;
        }
        Ok(accepted)
    })
    .map_err(|error| map_accept(error, client_request_id))?;

    let state = match outcome {
        AcceptOutcome::Accepted => FundRequestState::Reserved,
        AcceptOutcome::Duplicate => FundRequestState::Accepted,
        AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: client_request_id.to_vec().into(),
            });
        }
    };

    if state == FundRequestState::Reserved {
        let nonce = db::tx::update(|connection| {
            db::repo::actions::allocate_master_nonce(connection, "trading", now)
        })
        .map_err(|error| map_db(error, None))?;
        let payload = crate::venue::UsdSend {
            destination,
            amount_micros: amount,
            time: nonce,
        };
        let digest = payload.digest()?;
        let action_id = crate::random::random32().await?;
        let action = db::repo::actions::NewFundAction {
            action_id,
            user_id: session.user_id,
            client_request_id: Some(client_request_id.to_vec()),
            kind: "recovery".to_string(),
            signer_id: "trading".to_string(),
            canonical_action: payload.body(&test_signature_placeholder())?,
            digest,
            nonce,
        };
        db::tx::update(|connection| {
            db::repo::actions::insert_fund_action(connection, &action, now)
        })
        .map_err(|error| map_accept(error, client_request_id))?;
    }

    Ok(accepted(client_request_id, state, now))
}

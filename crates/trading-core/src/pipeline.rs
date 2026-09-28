//! 注文・取消の送信と取引所状態の照合（sweep）。
//!
//! `docs/phase-0/state-machines.md` 4節のとおり、受付（`submit_order`）は
//! `pending`を書くだけで、署名・送信・照合はここが担う。グローバルtimer（本番）と
//! 明示的なsweep（`sweep`・`test_sweep_now`）が同じ経路を通る。
//!
//! 送信結果の分類を守る：
//! - 受理 → `open`＋oid（oidが無い即時約定は`filled`の照合に委ねる）
//! - 拒否 → `rejected`＋リスク予約の解放
//! - 不明 → `unknown`（**再送しない**。リスク予約も解放しない。解消は照合で行う）

use crate::venue::{self, ExchangeOutcome};
use crate::{
    agent_approval, agent_derivation_path, bad, bad_decimal, ecdsa_key_id, internal, map_db,
};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::journal::{RecoveryEvent, RecoveryPayload};
use api_types::operations::BudgetClass;
use ic_cdk_management_canister::{SignWithEcdsaArgs, sign_with_ecdsa};

/// 1回のsweepで送る注文・取消の上限（outcallの回数を抑える）。
const MAX_DISPATCH_PER_SWEEP: u32 = 4;
const MAX_CANCEL_PER_SWEEP: u32 = 4;
/// 1回のsweepで照合する口座の上限。
pub const MAX_RECONCILE_PER_SWEEP: u32 = 2;
/// 1口座あたり1回のsweepで問い合わせる注文状態の上限。
const MAX_STATUS_CHECKS_PER_ACCOUNT: u32 = 4;
const ACTION_LEASE_MS: u64 = 30_000;
const RETRY_DELAY_MS: u64 = 5_000;
const PAYLOAD_RETENTION_MS: u64 = 24 * 60 * 60 * 1_000;
const DETAIL_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const PRUNE_PER_SWEEP: u32 = 100;

fn order_action_event(
    order: &db::repo::orders::SignableOrder,
    order_id: &[u8; 32],
    kind: &str,
    outcome: &ExchangeOutcome,
    now: u64,
) -> RecoveryEvent {
    let mut logical_id = b"order_action_result".to_vec();
    logical_id.extend_from_slice(kind.as_bytes());
    logical_id.extend_from_slice(order_id);
    let (accepted, hl_oid, filled) = match outcome {
        ExchangeOutcome::Accepted { oid, filled } => (true, *oid, *filled),
        ExchangeOutcome::Rejected { .. } => (false, None, false),
    };
    RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical_id).to_vec().into(),
        payload: RecoveryPayload::OrderActionResult {
            order_id: order_id.to_vec().into(),
            account_id: order.account_id.to_vec().into(),
            client_request_id: order.client_request_id.clone().into(),
            kind: kind.to_string(),
            accepted,
            hl_oid,
            filled,
            observed_at_ms: now,
        },
    }
}
/// 自動sweep（heartbeat）の間隔（ミリ秒）。建玉の鮮度（10秒）より短くし、
/// 新規リスクの受付を止めない。試験ビルドは自動sweepを行わないため定数も持たない。
#[cfg(not(feature = "test-venue"))]
pub const SWEEP_INTERVAL_MS: u64 = 5_000;

/// 1回のsweep（送信・取消・照合）。件数の内訳を返す。
pub async fn sweep_once(now: u64) -> Result<api_types::order::SweepOutcome, ErrorCode> {
    let _ = crate::cycles::status();
    db::tx::update(|connection| db::repo::orders::recover_expired_dispatches(connection, now))
        .map_err(map_db)?;
    // Preserve capacity for exits and evidence gathering before new risk.
    let cancels = dispatch_cancels(now).await?;
    let reconciled = reconcile_accounts(now).await?;
    // Market polling consumes reconciliation budget after exit work. A failed
    // observation closes new risk through the stored reason/age, not exits.
    let _ = crate::market::poll_if_due(now).await;
    let dispatched = dispatch_queued(now).await?;
    let outcome = api_types::order::SweepOutcome {
        dispatched,
        cancels,
        reconciled,
    };
    db::tx::update(|connection| {
        db::repo::orders::prune_terminal_history(
            connection,
            now.saturating_sub(PAYLOAD_RETENTION_MS),
            now.saturating_sub(DETAIL_RETENTION_MS),
            PRUNE_PER_SWEEP,
        )
    })
    .map_err(map_db)?;
    Ok(outcome)
}

/// 受付済み（`queued`）の注文へ署名して送信する。
async fn dispatch_queued(now: u64) -> Result<u32, ErrorCode> {
    let ids = db::tx::query(|connection| {
        db::repo::orders::queued_orders(connection, now, MAX_DISPATCH_PER_SWEEP)
    })
    .map_err(map_db)?;
    let mut processed = 0;
    for order_id in ids {
        // 送信権を先に取る（同時実行でも二重送信しない）。
        let claimed = db::tx::update(|connection| {
            db::repo::orders::claim_for_dispatch(connection, &order_id, now, ACTION_LEASE_MS)
        })
        .map_err(map_db)?;
        let Some(worker_epoch) = claimed else {
            continue;
        };
        db::tx::update(|connection| {
            db::repo::orders::ensure_action_nonces(connection, &order_id, now)
        })
        .map_err(map_db)?;
        let order = db::tx::query(|connection| db::repo::orders::signable(connection, &order_id))
            .map_err(map_db)?
            .ok_or_else(|| internal("missing order".to_string()))?;

        if order.expires_after.is_some_and(|expiry| expiry <= now) {
            db::tx::update(|connection| {
                db::repo::orders::abort_before_dispatch(
                    connection,
                    &order_id,
                    worker_epoch,
                    "order expired before dispatch",
                    now,
                )
            })
            .map_err(map_db)?;
            continue;
        }

        let needs_preflight = if order.preflight_state == api_types::fund::ActionState::Queued {
            match db::tx::update(|connection| {
                let decision = db::repo::leverage::prepare(
                    connection,
                    &order_id,
                    &order.account_id,
                    order.asset_index,
                    order.effective_leverage,
                    now,
                )?;
                if decision == db::repo::leverage::Decision::Skip {
                    db::repo::orders::mark_preflight_skipped(
                        connection,
                        &order_id,
                        worker_epoch,
                        now,
                    )?;
                }
                Ok(decision)
            })
            .map_err(map_db)?
            {
                db::repo::leverage::Decision::Send => true,
                db::repo::leverage::Decision::Skip => false,
                db::repo::leverage::Decision::Wait => {
                    retry_order(&order_id, worker_epoch, &ErrorCode::PolicyUnavailable, now)?;
                    continue;
                }
            }
        } else {
            false
        };

        if needs_preflight {
            let preflight = match sign_and_build_leverage(&order).await {
                Ok(value) => value,
                Err(error) => {
                    retry_order(&order_id, worker_epoch, &error, now)?;
                    continue;
                }
            };
            let permit = match crate::rest_budget::acquire(BudgetClass::NewRisk, 1).await {
                Ok(permit) => permit,
                Err(error) => {
                    retry_order(&order_id, worker_epoch, &error, now)?;
                    continue;
                }
            };
            let current = ic_cdk::api::time() / 1_000_000;
            if !revalidate_before_post(&order, &order_id, worker_epoch, current).await? {
                continue;
            }
            if !permit.valid_now() {
                retry_order(
                    &order_id,
                    worker_epoch,
                    &ErrorCode::PolicyUnavailable,
                    current,
                )?;
                continue;
            }
            let intent = journal_client::intent(
                "leverage",
                &order_id,
                &order.account_id,
                order.preflight_nonce,
                &hl_sign::keccak256(&preflight.1),
            );
            let ack = journal_client::append("core", intent.clone()).await?;
            let send_now = ic_cdk::api::time() / 1_000_000;
            let operational = permit.valid_now()
                && (order.reduce_only
                    || (crate::cycles::require_new().is_ok()
                        && crate::market::require(&order.market).is_ok()));
            let should_send = db::tx::update(|connection| {
                journal_client::record(connection, &intent, &ack)?;
                let blocker = db::repo::orders::dispatch_blocker(
                    connection,
                    &order_id,
                    worker_epoch,
                    send_now,
                )?;
                if !operational || blocker.is_some() {
                    db::repo::orders::abort_before_dispatch(
                        connection,
                        &order_id,
                        worker_epoch,
                        blocker.unwrap_or("send permit or market admission expired"),
                        send_now,
                    )?;
                    return Ok(false);
                }
                if !db::repo::leverage::owns_reservation(
                    connection,
                    &order_id,
                    &order.account_id,
                    order.asset_index,
                )? {
                    db::repo::orders::retry_before_dispatch(
                        connection,
                        &order_id,
                        worker_epoch,
                        "leverage reservation changed",
                        send_now.saturating_add(RETRY_DELAY_MS),
                        send_now,
                    )?;
                    return Ok(false);
                }
                db::repo::orders::mark_preflight_dispatching(
                    connection,
                    &order_id,
                    worker_epoch,
                    &preflight.1,
                    &preflight.0.to_bytes65(),
                    send_now,
                )?;
                Ok(true)
            })
            .map_err(map_db)?;
            if !should_send {
                continue;
            }
            match venue::post_exchange(&preflight.1, &permit).await {
                Ok(outcome @ ExchangeOutcome::Accepted { .. }) => {
                    let result = order_action_event(&order, &order_id, "leverage", &outcome, now);
                    let result_ack =
                        match journal_client::append_recovery_event("core", result.clone()).await {
                            Ok(ack) => ack,
                            Err(_) => {
                                db::tx::update(|connection| {
                                    db::repo::orders::stop_after_preflight(
                                        connection,
                                        &order_id,
                                        worker_epoch,
                                        false,
                                        "result_unknown",
                                        now,
                                    )
                                })
                                .map_err(map_db)?;
                                processed += 1;
                                continue;
                            }
                        };
                    db::tx::update(|connection| {
                        journal_client::record_recovery_event(connection, &result, &result_ack)?;
                        db::repo::orders::mark_preflight_applied(
                            connection,
                            &order_id,
                            worker_epoch,
                            now,
                        )
                    })
                    .map_err(map_db)?;
                }
                Ok(outcome @ ExchangeOutcome::Rejected { .. }) => {
                    let result = order_action_event(&order, &order_id, "leverage", &outcome, now);
                    let result_ack =
                        match journal_client::append_recovery_event("core", result.clone()).await {
                            Ok(ack) => ack,
                            Err(_) => {
                                db::tx::update(|connection| {
                                    db::repo::orders::stop_after_preflight(
                                        connection,
                                        &order_id,
                                        worker_epoch,
                                        false,
                                        "result_unknown",
                                        now,
                                    )
                                })
                                .map_err(map_db)?;
                                processed += 1;
                                continue;
                            }
                        };
                    db::tx::update(|connection| {
                        journal_client::record_recovery_event(connection, &result, &result_ack)?;
                        db::repo::orders::stop_after_preflight(
                            connection,
                            &order_id,
                            worker_epoch,
                            true,
                            "venue_rejected",
                            now,
                        )
                    })
                    .map_err(map_db)?;
                    processed += 1;
                    continue;
                }
                Err(_error) => {
                    db::tx::update(|connection| {
                        db::repo::orders::stop_after_preflight(
                            connection,
                            &order_id,
                            worker_epoch,
                            false,
                            "result_unknown",
                            now,
                        )
                    })
                    .map_err(map_db)?;
                    processed += 1;
                    continue;
                }
            }
        }

        let (_digest, signature, body) = match sign_and_build(&order).await {
            Ok(value) => value,
            Err(error) => {
                retry_order(&order_id, worker_epoch, &error, now)?;
                continue;
            }
        };
        let class = if order.reduce_only {
            BudgetClass::Exit
        } else {
            BudgetClass::NewRisk
        };
        let permit = match crate::rest_budget::acquire(class, 1).await {
            Ok(permit) => permit,
            Err(error) => {
                retry_order(&order_id, worker_epoch, &error, now)?;
                continue;
            }
        };
        let current = ic_cdk::api::time() / 1_000_000;
        if !revalidate_before_post(&order, &order_id, worker_epoch, current).await? {
            continue;
        }
        if !permit.valid_now() {
            retry_order(
                &order_id,
                worker_epoch,
                &ErrorCode::PolicyUnavailable,
                current,
            )?;
            continue;
        }
        let intent = journal_client::intent(
            "order",
            &order_id,
            &order.account_id,
            order.order_nonce,
            &hl_sign::keccak256(&body),
        );
        let ack = journal_client::append("core", intent.clone()).await?;
        let send_now = ic_cdk::api::time() / 1_000_000;
        let operational = permit.valid_now()
            && (order.reduce_only
                || (crate::cycles::require_new().is_ok()
                    && crate::market::require(&order.market).is_ok()));
        let should_send = db::tx::update(|connection| {
            journal_client::record(connection, &intent, &ack)?;
            let blocker =
                db::repo::orders::dispatch_blocker(connection, &order_id, worker_epoch, send_now)?;
            if !operational || blocker.is_some() {
                db::repo::orders::abort_before_dispatch(
                    connection,
                    &order_id,
                    worker_epoch,
                    blocker.unwrap_or("send permit or market admission expired"),
                    send_now,
                )?;
                return Ok(false);
            }
            db::repo::orders::mark_dispatching(
                connection,
                &order_id,
                &body,
                &signature.to_bytes65(),
                worker_epoch,
                send_now,
            )?;
            Ok(true)
        })
        .map_err(map_db)?;
        if !should_send {
            continue;
        }

        match venue::post_exchange(&body, &permit).await {
            Ok(ExchangeOutcome::Accepted { oid, filled }) => {
                let outcome = ExchangeOutcome::Accepted { oid, filled };
                let result = order_action_event(&order, &order_id, "order", &outcome, now);
                let result_ack =
                    match journal_client::append_recovery_event("core", result.clone()).await {
                        Ok(ack) => ack,
                        Err(_) => {
                            db::tx::update(|connection| {
                                db::repo::orders::mark_unknown(connection, &order_id, now)
                            })
                            .map_err(map_db)?;
                            processed += 1;
                            continue;
                        }
                    };
                db::tx::update(|connection| {
                    journal_client::record_recovery_event(connection, &result, &result_ack)?;
                    db::repo::orders::mark_venue_accepted(connection, &order_id, oid, filled, now)
                })
                .map_err(map_db)?;
            }
            Ok(outcome @ ExchangeOutcome::Rejected { .. }) => {
                let result = order_action_event(&order, &order_id, "order", &outcome, now);
                let result_ack =
                    match journal_client::append_recovery_event("core", result.clone()).await {
                        Ok(ack) => ack,
                        Err(_) => {
                            db::tx::update(|connection| {
                                db::repo::orders::mark_unknown(connection, &order_id, now)
                            })
                            .map_err(map_db)?;
                            processed += 1;
                            continue;
                        }
                    };
                db::tx::update(|connection| {
                    journal_client::record_recovery_event(connection, &result, &result_ack)?;
                    db::repo::orders::mark_venue_rejected(connection, &order_id, now)?;
                    db::repo::orders::release_risk(
                        connection,
                        &order.account_id,
                        &order.client_request_id,
                    )?;
                    Ok(())
                })
                .map_err(map_db)?;
            }
            Err(_) => {
                // 送信した可能性がある。再送せず、**リスク予約も解放しない**
                // （解放すると同一資金で追加の注文ができ、二重エクスポージャになる）。
                db::tx::update(|connection| {
                    db::repo::orders::mark_unknown(connection, &order_id, now)?;
                    Ok(())
                })
                .map_err(map_db)?;
            }
        }
        processed += 1;
    }
    Ok(processed)
}

fn retry_order(
    order_id: &[u8; 32],
    worker_epoch: u64,
    _error: &ErrorCode,
    now: u64,
) -> Result<(), ErrorCode> {
    db::tx::update(|connection| {
        db::repo::orders::retry_before_dispatch(
            connection,
            order_id,
            worker_epoch,
            "retry_before_dispatch",
            now.saturating_add(RETRY_DELAY_MS),
            now,
        )
    })
    .map_err(map_db)
}

async fn revalidate_before_post(
    order: &db::repo::orders::SignableOrder,
    order_id: &[u8; 32],
    worker_epoch: u64,
    now: u64,
) -> Result<bool, ErrorCode> {
    if !order.reduce_only && crate::cycles::require_new().is_err() {
        retry_order(order_id, worker_epoch, &ErrorCode::PolicyUnavailable, now)?;
        return Ok(false);
    }
    if !order.reduce_only && crate::market::require(&order.market).is_err() {
        retry_order(order_id, worker_epoch, &ErrorCode::PolicyUnavailable, now)?;
        return Ok(false);
    }
    let account_user = db::tx::query(|c| db::repo::accounts::identity(c, &order.account_id))
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?
        .0;
    if !order.reduce_only
        && crate::vault_eligibility_account(&account_user, &order.account_id)
            .await
            .is_err()
    {
        db::tx::update(|connection| {
            db::repo::orders::abort_before_dispatch(
                connection,
                order_id,
                worker_epoch,
                "eligibility_expired",
                now,
            )
        })
        .map_err(map_db)?;
        return Ok(false);
    }
    if !order.reduce_only
        && let Err(error) = crate::require_not_stopped().await
    {
        retry_order(order_id, worker_epoch, &error, now)?;
        return Ok(false);
    }
    let allowed = if order.reduce_only {
        true
    } else {
        match crate::policy_markets().await {
            Ok(markets) => markets.iter().any(|market| market == &order.market),
            Err(error) => {
                retry_order(order_id, worker_epoch, &error, now)?;
                return Ok(false);
            }
        }
    };
    if !allowed {
        db::tx::update(|connection| {
            db::repo::orders::abort_before_dispatch(
                connection,
                order_id,
                worker_epoch,
                "market is no longer allowed",
                now,
            )
        })
        .map_err(map_db)?;
        return Ok(false);
    }
    let blocker = db::tx::query(|connection| {
        db::repo::orders::dispatch_blocker(connection, order_id, worker_epoch, now)
    })
    .map_err(map_db)?;
    if let Some(reason) = blocker {
        db::tx::update(|connection| {
            db::repo::orders::abort_before_dispatch(connection, order_id, worker_epoch, reason, now)
        })
        .map_err(map_db)?;
        return Ok(false);
    }
    Ok(true)
}

/// 取消要求済みの注文へ署名して送信する（受理で`cancelled`へ）。
async fn dispatch_cancels(now: u64) -> Result<u32, ErrorCode> {
    let ids = db::tx::query(|connection| {
        db::repo::orders::cancel_candidates(connection, now, MAX_CANCEL_PER_SWEEP)
    })
    .map_err(map_db)?;
    let mut processed = 0;
    for order_id in ids {
        if dispatch_cancel(&order_id, now).await? {
            processed += 1;
        }
    }
    Ok(processed)
}

/// 取消actionをAgent鍵で署名して送信する。
async fn dispatch_cancel(order_id: &[u8; 32], now: u64) -> Result<bool, ErrorCode> {
    let claimed = db::tx::update(|connection| {
        db::repo::orders::claim_cancel(connection, order_id, now, ACTION_LEASE_MS)
    })
    .map_err(map_db)?;
    let Some((worker_epoch, nonce)) = claimed else {
        return Ok(false);
    };
    let (account_id, asset_index, oid) =
        db::tx::query(|connection| db::repo::orders::cancel_target(connection, order_id))
            .map_err(map_db)?
            .ok_or_else(|| bad(BadRequestCode::MalformedPayload, "unknown order"))?;

    let action = hl_types::action::CancelAction {
        cancels: vec![(asset_index, oid)],
    };
    let msgpack = action.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &msgpack,
        nonce,
        vault_address: None,
        expires_after: None,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);
    let signature = match sign_with_agent_key(&account_id, digest).await {
        Ok(signature) => signature,
        Err(_error) => {
            db::tx::update(|connection| {
                db::repo::orders::retry_cancel_before_dispatch(
                    connection,
                    order_id,
                    worker_epoch,
                    "agent_signing_unavailable",
                    now,
                )
            })
            .map_err(map_db)?;
            return Ok(false);
        }
    };

    let body = serde_json::json!({
        "action": {
            "type": "cancel",
            "cancels": [{ "a": asset_index, "o": oid }],
        },
        "nonce": nonce,
        "signature": {
            "r": format!("0x{}", hex::encode(signature.r)),
            "s": format!("0x{}", hex::encode(signature.s)),
            "v": signature.v,
        },
    });
    let body = serde_json::to_vec(&body).map_err(|error| internal(error.to_string()))?;
    let permit = match crate::rest_budget::acquire(BudgetClass::Exit, 1).await {
        Ok(permit) => permit,
        Err(_error) => {
            db::tx::update(|connection| {
                db::repo::orders::retry_cancel_before_dispatch(
                    connection,
                    order_id,
                    worker_epoch,
                    "rest_budget_unavailable",
                    now,
                )
            })
            .map_err(map_db)?;
            return Ok(false);
        }
    };
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let intent = journal_client::intent(
        "cancel",
        order_id,
        &account_id,
        nonce,
        &hl_sign::keccak256(&body),
    );
    let ack = journal_client::append("core", intent.clone()).await?;
    db::tx::update(|connection| {
        journal_client::record(connection, &intent, &ack)?;
        db::repo::orders::mark_cancel_dispatching(
            connection,
            order_id,
            &body,
            &signature.to_bytes65(),
            worker_epoch,
            now,
        )
    })
    .map_err(map_db)?;
    let order = db::tx::query(|connection| db::repo::orders::signable(connection, order_id))
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;

    match venue::post_exchange(&body, &permit).await {
        Ok(outcome @ ExchangeOutcome::Accepted { .. }) => {
            let result = order_action_event(&order, order_id, "cancel", &outcome, now);
            let result_ack =
                match journal_client::append_recovery_event("core", result.clone()).await {
                    Ok(ack) => ack,
                    Err(_) => {
                        db::tx::update(|connection| {
                            db::repo::orders::mark_cancel_unknown(
                                connection,
                                order_id,
                                worker_epoch,
                                now,
                            )
                        })
                        .map_err(map_db)?;
                        return Ok(true);
                    }
                };
            db::tx::update(|connection| {
                journal_client::record_recovery_event(connection, &result, &result_ack)?;
                db::repo::orders::mark_cancel_sent(connection, order_id, worker_epoch, now)
            })
            .map_err(map_db)?;
        }
        Ok(outcome @ ExchangeOutcome::Rejected { .. }) => {
            let result = order_action_event(&order, order_id, "cancel", &outcome, now);
            let result_ack =
                match journal_client::append_recovery_event("core", result.clone()).await {
                    Ok(ack) => ack,
                    Err(_) => {
                        db::tx::update(|connection| {
                            db::repo::orders::mark_cancel_unknown(
                                connection,
                                order_id,
                                worker_epoch,
                                now,
                            )
                        })
                        .map_err(map_db)?;
                        return Ok(true);
                    }
                };
            db::tx::update(|connection| {
                journal_client::record_recovery_event(connection, &result, &result_ack)?;
                db::repo::orders::mark_cancel_unknown(connection, order_id, worker_epoch, now)
            })
            .map_err(map_db)?;
        }
        Err(_) => {
            // 応答不明は「送ったか不明」として保持する（再送しない）。
            db::tx::update(|connection| {
                db::repo::orders::mark_cancel_unknown(connection, order_id, worker_epoch, now)
            })
            .map_err(map_db)?;
        }
    }
    Ok(true)
}

/// 有効な口座を巡回し、建玉・約定・注文状態を取り込む。
async fn reconcile_accounts(now: u64) -> Result<u32, ErrorCode> {
    let urgent_cursor =
        db::tx::query(db::repo::accounts::priority_reconcile_cursor).map_err(map_db)?;
    let urgent = db::tx::query(|connection| {
        db::repo::accounts::urgent_reconcile_candidate(connection, urgent_cursor.as_ref())
    })
    .map_err(map_db)?;
    let urgent = if urgent.is_none() && urgent_cursor.is_some() {
        db::tx::query(|connection| db::repo::accounts::urgent_reconcile_candidate(connection, None))
            .map_err(map_db)?
    } else {
        urgent
    };
    let cursor = db::tx::query(db::repo::accounts::reconcile_cursor).map_err(map_db)?;
    let mut candidates = db::tx::query(|connection| {
        db::repo::accounts::reconcile_candidates(
            connection,
            cursor.as_ref(),
            MAX_RECONCILE_PER_SWEEP,
        )
    })
    .map_err(map_db)?;
    if candidates.is_empty() && cursor.is_some() {
        // 末尾まで来たら先頭から巡回し直す（最後の口座が永久に対象外にならない）。
        candidates = db::tx::query(|connection| {
            db::repo::accounts::reconcile_candidates(connection, None, MAX_RECONCILE_PER_SWEEP)
        })
        .map_err(map_db)?;
    }
    let mut scheduled = Vec::new();
    if let Some(account) = urgent.as_ref() {
        scheduled.push(account.clone());
    }
    scheduled.extend(
        candidates
            .into_iter()
            .filter(|account| {
                urgent
                    .as_ref()
                    .is_none_or(|u| u.account_id != account.account_id)
            })
            .take(MAX_RECONCILE_PER_SWEEP as usize - scheduled.len()),
    );
    let mut processed = 0;
    for account in scheduled {
        // 1口座の失敗で巡回全体を止めない（次のsweepで再試行する）。
        if reconcile_account(&account, now).await.is_ok() {
            processed += 1;
        }
        if urgent
            .as_ref()
            .is_some_and(|u| u.account_id == account.account_id)
        {
            db::tx::update(|connection| {
                db::repo::accounts::set_priority_reconcile_cursor(
                    connection,
                    &account.account_id,
                    now,
                )
            })
            .map_err(map_db)?;
        } else {
            db::tx::update(|connection| {
                db::repo::accounts::set_reconcile_cursor(connection, &account.account_id, now)
            })
            .map_err(map_db)?;
        }
    }
    Ok(processed)
}

/// 1口座分の照合（建玉の全量・約定・未終端注文の状態）。
async fn reconcile_account(
    account: &db::repo::accounts::AccountRow,
    now: u64,
) -> Result<(), ErrorCode> {
    let address = format!("0x{}", hex::encode(account.master_address));

    // 建玉は観測の全量で置き換える（消えた建玉を残さない）。
    let state = venue::clearinghouse_state(&address).await?;
    db::tx::update(|connection| {
        ingest_positions_json(connection, &account.account_id, &state, now)
    })
    .map_err(map_db)?;

    // Recover exchange ids before ingesting fills; fills are matched by oid.
    let cloids = db::tx::query(|c| db::repo::orders::unknown_cloids(c, &account.account_id))
        .map_err(map_db)?;
    let mut recovered_order = false;
    for cloid in cloids {
        db::tx::update(|c| db::repo::orders::note_cloid_check(c, &account.account_id, &cloid, now))
            .map_err(map_db)?;
        let key = format!("0x{}", hex::encode(&cloid));
        let status = venue::order_status(&address, &key).await?;
        recovered_order |=
            apply_order_status_for_cloid(&account.account_id, &status, now, Some(&cloid)).await?;
    }

    // userFills has a large variable weight. Poll active accounts at most every
    // two minutes, idle accounts every ten minutes; the schedule survives upgrade.
    if recovered_order
        || db::tx::query(|connection| {
            db::repo::accounts::fills_due(connection, &account.account_id, now)
        })
        .map_err(map_db)?
    {
        let fills = venue::user_fills(&address).await?;
        ingest_fills_json(&account.user_id, &account.account_id, &fills, now).await?;
        db::tx::update(|connection| {
            db::repo::accounts::mark_fills_checked(connection, &account.account_id, now)
        })
        .map_err(map_db)?;
    }

    // 未終端でoidが分かっている注文の状態を問い合わせる。
    let oids = db::tx::query(|connection| {
        db::repo::orders::oids_awaiting_status(
            connection,
            &account.account_id,
            MAX_STATUS_CHECKS_PER_ACCOUNT,
        )
    })
    .map_err(map_db)?;
    for oid in oids {
        let status = venue::order_status(&address, oid).await?;
        apply_order_status_json(&account.account_id, &status, now).await?;
    }
    Ok(())
}

/// 建玉の全量を取り込む（`clearinghouseState`の本文。テスト専用フックと共用）。
pub fn ingest_positions_json(
    connection: &mut ic_sqlite_vfs::db::UpdateConnection<'_>,
    account_id: &[u8; 32],
    body: &str,
    now: u64,
) -> Result<u32, db::error::Error> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| db::error::Error::Invariant("invalid json"))?;
    let entries = value
        .get("assetPositions")
        .and_then(|value| value.as_array())
        .ok_or(db::error::Error::Invariant("missing assetPositions"))?;
    let mut observed = Vec::new();
    for entry in entries {
        let position = entry
            .get("position")
            .ok_or(db::error::Error::Invariant("missing position"))?;
        let coin = position
            .get("coin")
            .and_then(|value| value.as_str())
            .ok_or(db::error::Error::Invariant("missing position coin"))?;
        let size = position
            .get("szi")
            .and_then(|value| value.as_str())
            .ok_or(db::error::Error::Invariant("missing position size"))?;
        let entry_price = position
            .get("entryPx")
            .and_then(|value| value.as_str())
            .ok_or(db::error::Error::Invariant("missing entry price"))?;
        let unrealized_pnl = parse_signed_usdc_micros(
            position
                .get("unrealizedPnl")
                .and_then(|value| value.as_str())
                .ok_or(db::error::Error::Invariant("missing unrealized pnl"))?,
        )?;
        observed.push(api_types::order::PositionView {
            market: coin.to_string(),
            size: size.to_string(),
            entry_price: entry_price.to_string(),
            liquidation_price: position
                .get("liquidationPx")
                .and_then(|value| value.as_str())
                .map(|text| text.to_string()),
            unrealized_pnl,
            leverage: position
                .get("leverage")
                .and_then(|value| value.get("value"))
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            margin_mode: position
                .get("marginMode")
                .and_then(|value| value.as_str())
                .unwrap_or("cross")
                .to_string(),
            stop_loss: None,
            take_profit: None,
        });
    }
    let margin_used = match value
        .get("marginSummary")
        .and_then(|summary| summary.get("totalMarginUsed"))
        .and_then(|value| value.as_str())
    {
        Some(text) => parse_unsigned_usdc_micros(text)?,
        None if observed.is_empty() => 0,
        None => return Err(db::error::Error::Invariant("missing margin used")),
    };
    let count = u32::try_from(observed.len()).map_err(|_| db::error::Error::Overflow)?;
    db::repo::positions::replace_all(connection, account_id, &observed, now)?;
    let total_unrealized = observed.iter().try_fold(0i64, |total, position| {
        total
            .checked_add(position.unrealized_pnl)
            .ok_or(db::error::Error::Overflow)
    })?;
    db::repo::positions::set_account_metrics(
        connection,
        account_id,
        margin_used,
        total_unrealized,
        now,
    )?;
    Ok(count)
}

fn parse_unsigned_usdc_micros(text: &str) -> Result<u64, db::error::Error> {
    let (integer, fraction) = text.split_once('.').unwrap_or((text, ""));
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(db::error::Error::Invariant("invalid usdc decimal"));
    }
    let whole = integer
        .parse::<u64>()
        .map_err(|_| db::error::Error::Overflow)?;
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .map_err(|_| db::error::Error::Invariant("invalid usdc decimal"))?
    };
    let scale =
        10u64.pow(6 - u32::try_from(fraction.len()).map_err(|_| db::error::Error::Overflow)?);
    whole
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(fraction_value * scale))
        .ok_or(db::error::Error::Overflow)
}

fn parse_signed_usdc_micros(text: &str) -> Result<i64, db::error::Error> {
    let (negative, magnitude) = match text.strip_prefix('-') {
        Some(magnitude) => (true, magnitude),
        None => (false, text),
    };
    let magnitude = parse_unsigned_usdc_micros(magnitude)?;
    if negative {
        if magnitude == i64::MAX as u64 + 1 {
            Ok(i64::MIN)
        } else {
            let value = i64::try_from(magnitude).map_err(|_| db::error::Error::Overflow)?;
            Ok(-value)
        }
    } else {
        i64::try_from(magnitude).map_err(|_| db::error::Error::Overflow)
    }
}

/// 約定の一覧を取り込む（`userFills`の本文。テスト専用フックと共用）。
/// V2の独立記録を先に確定し、受領番号と約定・リスク更新を同じDB transactionへ入れる。
pub async fn ingest_fills_json(
    user_id: &[u8; 32],
    account_id: &[u8; 32],
    body: &str,
    now: u64,
) -> Result<u32, ErrorCode> {
    let fills: Vec<serde_json::Value> =
        serde_json::from_str(body).map_err(|_| internal("invalid fills json".into()))?;
    let mut ingested = 0;
    for fill in fills {
        let tid = fill
            .get("tid")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let oid = fill
            .get("oid")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let coin = fill
            .get("coin")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let price = fill
            .get("px")
            .and_then(|value| value.as_str())
            .unwrap_or("0")
            .to_string();
        let parsed_price = hl_types::decimal::Decimal::parse(&price)
            .map_err(|_| internal("invalid fill price".into()))?;
        if parsed_price.as_str() == "0" || parsed_price.as_str().starts_with('-') {
            return Err(internal("invalid fill price".into()));
        }
        let quantity = fill
            .get("sz")
            .and_then(|value| value.as_str())
            .unwrap_or("0")
            .to_string();
        let size = hl_types::decimal::Decimal::parse(&quantity)
            .map_err(|_| internal("invalid fill size".into()))?;
        if size.as_str() == "0" || size.as_str().starts_with('-') || size.scale() > 18 {
            return Err(internal("invalid fill size".into()));
        }
        let fee = parse_signed_usdc_micros(
            fill.get("fee")
                .and_then(|value| value.as_str())
                .ok_or_else(|| internal("missing decimal fill fee".into()))?,
        )
        .map_err(map_db)?;
        let at = fill
            .get("time")
            .and_then(|value| value.as_u64())
            .unwrap_or(now);
        let order_id = db::tx::query(|connection| {
            db::repo::orders::pending_fill_order(connection, user_id, account_id, tid, oid, &coin)
        })
        .map_err(map_db)?;
        let Some(order_id) = order_id else {
            continue;
        };
        let mut logical = b"fill_observed".to_vec();
        logical.extend_from_slice(account_id);
        logical.extend_from_slice(&tid.to_be_bytes());
        let event = RecoveryEvent {
            version: 1,
            logical_id: hl_sign::keccak256(&logical).to_vec().into(),
            payload: RecoveryPayload::FillObserved {
                tid,
                hl_oid: oid,
                user_id: user_id.to_vec().into(),
                order_id: order_id.to_vec().into(),
                account_id: account_id.to_vec().into(),
                market: coin.clone(),
                quantity: quantity.clone(),
                price: price.clone(),
                fee,
                filled_at_ms: at,
            },
        };
        let ack = journal_client::append_recovery_event("core", event.clone()).await?;
        let saved = db::tx::update(|connection| {
            journal_client::record_recovery_event(connection, &event, &ack)?;
            if db::repo::orders::pending_fill_order(
                connection, user_id, account_id, tid, oid, &coin,
            )? != Some(order_id)
                || !db::repo::orders::ingest_fill(
                    connection,
                    user_id,
                    account_id,
                    &db::repo::orders::NewFill {
                        tid,
                        hl_oid: oid,
                        market: &coin,
                        price: &price,
                        quantity: &quantity,
                        fee,
                        filled_at: at,
                    },
                )?
            {
                return Err(db::error::Error::Conflict);
            }
            Ok(())
        });
        if let Err(error) = saved {
            journal_client::lock()?;
            return Err(map_db(error));
        }
        ingested += 1;
    }
    Ok(ingested)
}

/// `orderStatus`の本文を注文へ反映する（テスト専用フックと共用）。
/// 終端状態がリスク予約を解放する前に、非公開の独立証跡を記録する。
pub async fn apply_order_status_json(
    account_id: &[u8; 32],
    body: &str,
    now: u64,
) -> Result<bool, ErrorCode> {
    apply_order_status_for_cloid(account_id, body, now, None).await
}

async fn apply_order_status_for_cloid(
    account_id: &[u8; 32],
    body: &str,
    now: u64,
    cloid: Option<&[u8]>,
) -> Result<bool, ErrorCode> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| internal("invalid order status json".into()))?;
    let status = value
        .get("status")
        .and_then(|status| status.as_str())
        .unwrap_or("unknown")
        .to_string();
    let oid = value
        .get("order")
        .and_then(|order| order.get("oid"))
        .and_then(|oid| oid.as_u64())
        .ok_or_else(|| internal("orderStatus without oid".into()))?;
    // 取引所の語彙をこちらの状態へ写す（未知はunknownとして保持）。
    let state = match status.as_str() {
        "open" => "open",
        "filled" => "filled",
        "canceled" | "cancelled" => "cancelled",
        "rejected" => "rejected",
        _ => "unknown",
    };
    // An unrecognized venue status is not evidence for replacing an existing
    // state. In particular it cannot release a risk hold.
    if state == "unknown" {
        return Ok(false);
    }
    if let Some(expected) = cloid {
        let actual = value
            .get("order")
            .and_then(|v| v.get("cloid"))
            .and_then(|v| v.as_str())
            .and_then(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok());
        if actual.as_deref() != Some(expected) {
            return Err(ErrorCode::PolicyUnavailable);
        }
    }
    let target = db::tx::query(|connection| match cloid {
        Some(key) => db::repo::orders::cloid_status_target(connection, account_id, key),
        None => db::repo::orders::order_status_target(connection, account_id, oid),
    })
    .map_err(map_db)?;
    let Some((order_id, prior_state)) = target else {
        return Ok(false);
    };
    if matches!(prior_state.as_str(), "filled" | "cancelled" | "rejected")
        || prior_state == state
        || (state == "open" && !matches!(prior_state.as_str(), "pending" | "unknown"))
    {
        return Ok(false);
    }
    let mut logical = b"order_status_observed".to_vec();
    logical.extend_from_slice(&order_id);
    logical.extend_from_slice(state.as_bytes());
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::OrderStatusObserved {
            order_id: order_id.to_vec().into(),
            account_id: account_id.to_vec().into(),
            hl_oid: oid,
            state: state.into(),
            evidence_digest: hl_sign::keccak256(body.as_bytes()).to_vec().into(),
            observed_at_ms: now,
        },
    };
    let ack = journal_client::append_recovery_event("core", event.clone()).await?;
    let saved = db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &event, &ack)?;
        if cloid.is_some() {
            db::repo::orders::bind_observed_oid(connection, account_id, &order_id, oid)?;
        }
        if db::repo::orders::order_status_target(connection, account_id, oid)?
            != Some((order_id, prior_state.clone()))
            || !db::repo::orders::apply_order_status(connection, account_id, oid, state, now)?
        {
            return Err(db::error::Error::Conflict);
        }
        Ok(true)
    });
    match saved {
        Ok(applied) => Ok(applied),
        Err(error) => {
            journal_client::lock()?;
            Err(map_db(error))
        }
    }
}

/// 注文の署名actionを組み立てる（通常注文・トリガ注文の共通経路）。
///
/// トリガは建玉単位（`positionTpsl`）として送る（`docs/phase-0/api-contract.md` 3.1）。
pub fn order_action(
    order: &db::repo::orders::SignableOrder,
    price: &str,
) -> Result<hl_types::action::OrderAction, ErrorCode> {
    let cloid = format!("0x{}", hex::encode(order.cloid));
    let (order_type, grouping) = match &order.trigger {
        Some(trigger) => {
            let tpsl = match trigger.kind.as_str() {
                "stop_loss" => hl_types::action::Tpsl::StopLoss,
                "take_profit" => hl_types::action::Tpsl::TakeProfit,
                other => return Err(internal(format!("unknown trigger kind: {other}"))),
            };
            (
                hl_types::action::OrderType::Trigger(hl_types::action::TriggerOrder {
                    is_market: trigger.is_market,
                    trigger_price: hl_types::decimal::Decimal::parse(&trigger.price)
                        .map_err(bad_decimal)?,
                    tpsl,
                }),
                hl_types::action::Grouping::PositionTpsl,
            )
        }
        None => (
            hl_types::action::OrderType::Limit {
                tif: if order.kind == "market_ioc" {
                    hl_types::action::TimeInForce::Ioc
                } else {
                    hl_types::action::TimeInForce::Gtc
                },
            },
            hl_types::action::Grouping::Na,
        ),
    };
    Ok(hl_types::action::OrderAction {
        orders: vec![hl_types::action::OrderRequest {
            asset_index: order.asset_index,
            is_buy: order.is_buy,
            price: hl_types::decimal::Decimal::parse(price).map_err(bad_decimal)?,
            size: hl_types::decimal::Decimal::parse(&order.quantity).map_err(bad_decimal)?,
            reduce_only: order.reduce_only,
            order_type,
            cloid: Some(cloid),
        }],
        grouping,
    })
}

/// 注文のaction JSON（HLの`/exchange`はJSON actionを取る）。
pub fn order_action_json(
    order: &db::repo::orders::SignableOrder,
    price: &str,
) -> serde_json::Value {
    let order_type = match &order.trigger {
        Some(trigger) => serde_json::json!({
            "trigger": {
                "isMarket": trigger.is_market,
                "triggerPx": trigger.price,
                "tpsl": if trigger.kind == "stop_loss" { "sl" } else { "tp" },
            }
        }),
        None => serde_json::json!({
            "limit": {
                "tif": if order.kind == "market_ioc" { "Ioc" } else { "Gtc" },
            }
        }),
    };
    serde_json::json!({
        "type": "order",
        "orders": [{
            "a": order.asset_index,
            "b": order.is_buy,
            "p": price,
            "s": order.quantity,
            "r": order.reduce_only,
            "t": order_type,
            "c": format!("0x{}", hex::encode(order.cloid)),
        }],
        "grouping": if order.trigger.is_some() { "positionTpsl" } else { "na" },
    })
}

/// 署名と送信本文を作る（action msgpackのハッシュへAgent鍵で署名する）。
pub async fn sign_and_build(
    order: &db::repo::orders::SignableOrder,
) -> Result<([u8; 32], hl_sign::Signature, Vec<u8>), ErrorCode> {
    let price = order.price.clone().ok_or_else(|| {
        bad(
            BadRequestCode::MissingField,
            "price is required for signing",
        )
    })?;
    let action = order_action(order, &price)?;
    let msgpack = action.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &msgpack,
        nonce: order.order_nonce,
        vault_address: None,
        expires_after: order.expires_after,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);
    let signature = sign_with_agent_key(&order.account_id, digest).await?;

    let mut body = serde_json::json!({
        "action": order_action_json(order, &price),
        "nonce": order.order_nonce,
        "signature": {
            "r": format!("0x{}", hex::encode(signature.r)),
            "s": format!("0x{}", hex::encode(signature.s)),
            "v": signature.v,
        },
    });
    if let Some(expires_after) = order.expires_after {
        body.as_object_mut()
            .expect("exchange body is an object")
            .insert("expiresAfter".to_string(), expires_after.into());
    }
    let body = serde_json::to_vec(&body).map_err(|error| internal(error.to_string()))?;
    Ok((digest, signature, body))
}

/// 注文前にcross leverageを確定するactionへ署名する。
async fn sign_and_build_leverage(
    order: &db::repo::orders::SignableOrder,
) -> Result<(hl_sign::Signature, Vec<u8>), ErrorCode> {
    let action = hl_types::action::UpdateLeverageAction {
        asset_index: order.asset_index,
        is_cross: true,
        leverage: order.effective_leverage,
    };
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &action.to_value().encode(),
        nonce: order.preflight_nonce,
        vault_address: None,
        expires_after: order.expires_after,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);
    let signature = sign_with_agent_key(&order.account_id, digest).await?;
    let mut body = serde_json::json!({
        "action": {
            "type": "updateLeverage",
            "asset": order.asset_index,
            "isCross": true,
            "leverage": order.effective_leverage,
        },
        "nonce": order.preflight_nonce,
        "signature": {
            "r": format!("0x{}", hex::encode(signature.r)),
            "s": format!("0x{}", hex::encode(signature.s)),
            "v": signature.v,
        },
    });
    if let Some(expires_after) = order.expires_after {
        body.as_object_mut()
            .expect("exchange body is an object")
            .insert("expiresAfter".to_string(), expires_after.into());
    }
    let body = serde_json::to_vec(&body).map_err(|error| internal(error.to_string()))?;
    Ok((signature, body))
}

/// 口座の現在世代のAgent鍵でdigestへ署名する（`v`は公開鍵から復元する）。
///
/// 未承認の世代では署名しない（取引所に拒否される署名を送らない）。
pub(crate) async fn sign_with_agent_key(
    account_id: &[u8; 32],
    digest: [u8; 32],
) -> Result<hl_sign::Signature, ErrorCode> {
    let generation = db::tx::query(|connection| db::repo::agents::latest(connection, account_id))
        .map_err(map_db)?
        .ok_or(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        })?;
    let approved = agent_approval(account_id, generation.generation).await?;
    match approved {
        Some(approved)
            if approved.agent_address.as_ref() == generation.agent_address.as_slice() => {}
        _ => {
            return Err(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            });
        }
    }
    let path = agent_derivation_path(account_id, generation.generation);
    let signature = sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: digest.to_vec(),
        derivation_path: path,
        key_id: ecdsa_key_id()?,
    })
    .await
    .map_err(|error| internal(format!("sign_with_ecdsa failed: {error}")))?
    .signature;
    let bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| internal("unexpected signature length".to_string()))?;
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&bytes[0..32]);
    s.copy_from_slice(&bytes[32..64]);
    let expected_address: [u8; 20] = generation
        .agent_address
        .as_ref()
        .try_into()
        .map_err(|_| internal("unexpected agent address length".to_string()))?;
    let v = hl_sign::recover_v_for_address(&digest, r, s, &expected_address)
        .map_err(|error| internal(format!("cannot recover v: {error}")))?;
    Ok(hl_sign::Signature { r, s, v })
}

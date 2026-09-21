//! 資金actionの永続outbox（`Implementation.md` 2.3・14.2、`docs/phase-0/state-machines.md` 2節）。
//!
//! 1つのactionを `claim → 署名 → signed → dispatching永続化 → POST → reconciled/unknown`
//! の順で処理する。**送信前に `dispatching` をCASで永続化**し、結果不明は再送しない。
//! 同期ブロック（DB更新）を完結させてから `await` する順序を守る。

use crate::auth::map_db;
use crate::config;
use crate::crypto;
use crate::venue::{self, ExchangeOutcome};
use api_types::error::ErrorCode;
use api_types::fund::FundRequestState;
use db::repo::actions::FundActionRow;
use db::repo::funds::fund_request;
use db::repo::ledger::NewCustodyAccount;
use db::repo::ledger::Posting;

/// 1回のsweepで処理するaction数の上限。
const MAX_ACTIONS_PER_SWEEP: u32 = 4;

/// actionのリース期間（他workerとの競合を避ける）。
const ACTION_LEASE_MS: u64 = 30_000;

/// heartbeatの間隔（`#[ic_cdk::heartbeat]`は毎ラウンド呼ばれる）。
pub const SWEEP_INTERVAL_MS: u64 = 5_000;

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

/// 未処理のactionを有界に処理する。
pub async fn sweep(now: u64) -> Result<u32, ErrorCode> {
    let mut processed = 0;
    for _ in 0..MAX_ACTIONS_PER_SWEEP {
        let claimed = db::tx::update(|connection| {
            db::repo::actions::claim_action(connection, now, ACTION_LEASE_MS)
        })
        .map_err(|error| map_db(error, None))?;
        let Some(action) = claimed else {
            break;
        };
        dispatch(&action, now).await?;
        processed += 1;
    }
    Ok(processed)
}

fn account_kind_name(kind: api_types::AccountKind) -> &'static str {
    match kind {
        api_types::AccountKind::Reserve => "reserve",
        api_types::AccountKind::Trading => "trading",
    }
}

/// 保管口座の鍵導出経路（**保存済み** `account_id` から作る）。
fn custody_path(kind: &str, account_id: &[u8; 32]) -> Vec<Vec<u8>> {
    crypto::derivation_path(&[
        b"private-perp",
        kind.as_bytes(),
        hex::encode(account_id).as_bytes(),
    ])
}

/// 保管口座を取得する（無ければ作成する）。
///
/// 鍵導出経路は常に保存済みの `account_id` から作る。乱数で新しいIDを作ってから
/// 既存行を返す実装にすると、保存アドレスと署名鍵が食い違う（署名者が口座と一致しない）。
pub(crate) async fn ensure_custody_account(
    user_id: &[u8; 32],
    kind: api_types::AccountKind,
    now: u64,
) -> Result<db::repo::ledger::CustodyAccount, ErrorCode> {
    if let Some(existing) =
        db::tx::query(|connection| db::repo::ledger::custody_account(connection, user_id, kind))
            .map_err(|error| map_db(error, None))?
    {
        return Ok(existing);
    }

    let account_id = crate::random::random32().await?;
    let kind_name = account_kind_name(kind);
    let path = custody_path(kind_name, &account_id);
    let public_key = crypto::public_key(path).await?;
    let address = hl_sign::address_from_public_key(&public_key)
        .map_err(|error| internal(error.to_string()))?;
    let derivation_path = format!("private-perp/{kind_name}/{}", hex::encode(account_id));

    let account = NewCustodyAccount {
        account_id: &account_id,
        user_id,
        kind,
        derivation_path: &derivation_path,
        master_address: &address,
        network: config::network_name(config::NETWORK),
    };
    db::tx::update(|connection| db::repo::ledger::ensure_custody_account(connection, &account, now))
        .map_err(|error| map_db(error, None))
}

/// 署名材料（保存済み口座・導出経路・公開鍵）。
///
/// 導出アドレスが保存アドレスと一致することを検証する（経路や設定の変更を検知する）。
pub(crate) async fn signer_material(
    user_id: &[u8; 32],
    kind: api_types::AccountKind,
    now: u64,
) -> Result<(db::repo::ledger::CustodyAccount, Vec<Vec<u8>>, [u8; 33]), ErrorCode> {
    let account = ensure_custody_account(user_id, kind, now).await?;
    let path = custody_path(account_kind_name(kind), &account.account_id);
    let public_key = crypto::public_key(path.clone()).await?;
    let derived = hl_sign::address_from_public_key(&public_key)
        .map_err(|error| internal(error.to_string()))?;
    if derived != account.master_address {
        return Err(internal(format!(
            "derived {} address does not match the stored address",
            account_kind_name(kind)
        )));
    }
    Ok((account, path, public_key))
}

/// 取引口座を用意する（払出し先・Agent承認の主体）。
pub(crate) async fn ensure_trading_account(
    user_id: &[u8; 32],
    now: u64,
) -> Result<db::repo::ledger::CustodyAccount, ErrorCode> {
    ensure_custody_account(user_id, api_types::AccountKind::Trading, now).await
}

/// 1つのactionを実行する。
async fn dispatch(action: &FundActionRow, now: u64) -> Result<(), ErrorCode> {
    if action.kind != "allocation" && action.kind != "withdrawal" && action.kind != "recovery" {
        // 未対応の種別は安全側で中止する。
        db::tx::update(|connection| {
            db::repo::actions::abort_unsent(
                connection,
                &action.action_id,
                action.worker_epoch,
                action.dispatch_state,
                "unsupported_action_kind",
                now,
            )
        })
        .map_err(|error| map_db(error, None))?;
        return Ok(());
    }

    let request_id = action
        .client_request_id
        .clone()
        .ok_or_else(|| internal("action without client_request_id".to_string()))?;
    let request =
        db::tx::query(|connection| fund_request(connection, &action.user_id, &request_id))
            .map_err(|error| map_db(error, None))?
            .ok_or_else(|| internal("fund request not found".to_string()))?;

    if action.kind == "withdrawal" {
        return dispatch_withdrawal(action, &request, &request_id, now).await;
    }
    if action.kind == "recovery" {
        return dispatch_recovery(action, &request, &request_id, now).await;
    }

    // 資金は準備口座から出る。署名者は**準備口座**のmaster鍵で、宛先は受付時に保存した
    // 取引口座アドレス（保存値と一致しなければ下のdigest照合で拒否される）。
    let destination = request
        .destination
        .clone()
        .ok_or_else(|| internal("allocation request without a destination".to_string()))?;
    let (_reserve, path, public_key) =
        signer_material(&action.user_id, api_types::AccountKind::Reserve, now).await?;

    let payload = venue::UsdSend {
        destination,
        amount_micros: request.amount,
        time: action.nonce,
    };
    let digest = payload.digest()?;
    // 受付時に記録したダイジェストと一致しないpayloadは署名しない（保存した監査証跡と
    // 実署名が乖離するのを防ぐ）。導出経路や設定が変わった場合はここで恒久エラーになる。
    if digest != action.digest {
        db::tx::update(|connection| {
            db::repo::events::insert_audit(
                connection,
                "system",
                "action_digest_mismatch",
                None,
                Some("aborted"),
                now,
            )
        })
        .map_err(|error| map_db(error, None))?;
        return Err(ErrorCode::Internal {
            code: "action digest mismatch".to_string(),
        });
    }
    let signature = crypto::sign_with_key(&digest, path, &public_key).await?;
    let wire_payload = payload.body(&signature)?;

    // 署名とpayloadを保存してから dispatching を永続化する（送信前に確定させる）。
    db::tx::update(|connection| {
        db::repo::actions::mark_signed(
            connection,
            &action.action_id,
            action.worker_epoch,
            &signature.to_bytes65(),
            &wire_payload,
            now,
        )
    })
    .map_err(|error| map_db(error, None))?;
    db::tx::update(|connection| {
        db::repo::actions::mark_dispatching(connection, &action.action_id, action.worker_epoch, now)
    })
    .map_err(|error| map_db(error, None))?;

    match venue::post_usd_send(&payload, &signature).await {
        Ok((ExchangeOutcome::Accepted, _response)) => {
            // 共通保管から出た（移動中）。着金の確定は照合で行う。
            let amount = i64::try_from(request.amount).map_err(|_| internal("overflow".into()))?;
            db::tx::update(|connection| {
                db::repo::ledger::post_journal(
                    connection,
                    "allocation_start",
                    now,
                    None,
                    Some(&request_id),
                    &[
                        Posting {
                            account: db::repo::ledger::CASH_IN_TRANSIT.to_string(),
                            kind: db::repo::ledger::AccountKind::Asset,
                            amount,
                        },
                        Posting {
                            account: db::repo::ledger::CASH_RESERVE.to_string(),
                            kind: db::repo::ledger::AccountKind::Asset,
                            amount: -amount,
                        },
                        Posting {
                            account: db::repo::ledger::user_reserve(&action.user_id),
                            kind: db::repo::ledger::AccountKind::Liability,
                            amount,
                        },
                        Posting {
                            account: db::repo::ledger::user_in_transit(&action.user_id),
                            kind: db::repo::ledger::AccountKind::Liability,
                            amount: -amount,
                        },
                    ],
                )?;
                // 仕訳で資金が動いたので予約は消費する。
                db::repo::funds::consume_reservation(connection, &action.user_id, &request_id)?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    &request_id,
                    FundRequestState::Executing,
                    now,
                )?;
                db::repo::actions::mark_reconciled(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
        Ok((ExchangeOutcome::Rejected { message }, _response)) => {
            // 取引所が拒否した＝外部効果は無い。予約を解放して終端する。
            db::tx::update(|connection| {
                db::repo::funds::release_reservation(
                    connection,
                    &action.user_id,
                    &request_id,
                    now,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    &request_id,
                    FundRequestState::Rejected,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "allocation_rejected",
                    None,
                    Some(&message),
                    now,
                )?;
                db::repo::actions::mark_reconciled(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
        Err(error) => {
            // 送信した可能性がある。再送せず unknown として照合対象に残す。
            let reason = format!("{error:?}");
            db::tx::update(|connection| {
                db::repo::actions::mark_unknown(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    &reason,
                    now,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    &request_id,
                    FundRequestState::Unknown,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "allocation_unknown",
                    None,
                    Some("result_unknown"),
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
    }

    Ok(())
}

/// 準備口座（入金先）を導出・登録する。`funding_instructions` の前提を作る。
pub(crate) async fn provision_reserve_account(
    user_id: &[u8; 32],
    now: u64,
) -> Result<[u8; 20], ErrorCode> {
    let account = ensure_custody_account(user_id, api_types::AccountKind::Reserve, now).await?;
    Ok(account.master_address)
}

/// 払出し（`usdSend`）を送出する。資金は準備口座から出る。
async fn dispatch_withdrawal(
    action: &FundActionRow,
    request: &db::repo::funds::FundRequestRow,
    request_id: &[u8],
    now: u64,
) -> Result<(), ErrorCode> {
    let destination = request
        .destination
        .clone()
        .ok_or_else(|| internal("withdrawal request without a destination".to_string()))?;
    // 資金は準備口座から出る。署名者は準備口座のmaster鍵（保存済みaccount_idから導出）。
    let (_reserve, path, public_key) =
        signer_material(&action.user_id, api_types::AccountKind::Reserve, now).await?;
    let payload = venue::UsdSend {
        destination,
        amount_micros: request.amount,
        time: action.nonce,
    };
    let digest = payload.digest()?;
    if digest != action.digest {
        db::tx::update(|connection| {
            db::repo::events::insert_audit(
                connection,
                "system",
                "action_digest_mismatch",
                None,
                Some("aborted"),
                now,
            )
        })
        .map_err(|error| map_db(error, None))?;
        return Err(ErrorCode::Internal {
            code: "action digest mismatch".to_string(),
        });
    }
    let signature = crypto::sign_with_key(&digest, path, &public_key).await?;
    let wire_payload = payload.body(&signature)?;
    db::tx::update(|connection| {
        db::repo::actions::mark_signed(
            connection,
            &action.action_id,
            action.worker_epoch,
            &signature.to_bytes65(),
            &wire_payload,
            now,
        )
    })
    .map_err(|error| map_db(error, None))?;
    db::tx::update(|connection| {
        db::repo::actions::mark_dispatching(connection, &action.action_id, action.worker_epoch, now)
    })
    .map_err(|error| map_db(error, None))?;

    match venue::post_usd_send(&payload, &signature).await {
        Ok((ExchangeOutcome::Accepted, _response)) => {
            // 払出しの証跡IDはactionのダイジェストから決定的に作る（実運用では取引所のtx）。
            let mut evidence = b"payout".to_vec();
            evidence.extend_from_slice(&action.digest);
            let event_id = hl_sign::keccak256(&evidence);
            db::tx::update(|connection| {
                db::repo::ledger::payout_settled(
                    connection,
                    &action.user_id,
                    request.amount,
                    now,
                    &event_id,
                )?;
                // 台帳で資金が動いたので予約（reservations表）も消費する。
                db::repo::funds::consume_reservation(connection, &action.user_id, request_id)?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Settled,
                    now,
                )?;
                db::repo::actions::mark_reconciled(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
        Ok((ExchangeOutcome::Rejected { message }, _response)) => {
            db::tx::update(|connection| {
                db::repo::funds::release_reservation(connection, &action.user_id, request_id, now)?;
                // 台帳の予約も戻す（受付時の`withdrawal_reserve`の逆仕訳）。
                db::repo::ledger::withdrawal_release(
                    connection,
                    &action.user_id,
                    request.amount,
                    now,
                    request_id,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Rejected,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "payout_rejected",
                    None,
                    Some(&message),
                    now,
                )?;
                db::repo::actions::mark_reconciled(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
        Err(error) => {
            let reason = format!("{error:?}");
            db::tx::update(|connection| {
                db::repo::actions::mark_unknown(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    &reason,
                    now,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Unknown,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "payout_unknown",
                    None,
                    Some("result_unknown"),
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
    }
    Ok(())
}

/// 回収（trading口座→準備口座）を送出する。
async fn dispatch_recovery(
    action: &FundActionRow,
    request: &db::repo::funds::FundRequestRow,
    request_id: &[u8],
    now: u64,
) -> Result<(), ErrorCode> {
    let trading_account_id = request
        .account_id
        .ok_or_else(|| internal("recovery without a trading account".to_string()))?;
    let reserve =
        ensure_custody_account(&action.user_id, api_types::AccountKind::Reserve, now).await?;
    let destination = format!("0x{}", hex::encode(reserve.master_address));

    // 資金は取引口座から出る。署名者は取引口座のmaster鍵（保存済みaccount_idから導出）。
    let (signer, path, public_key) =
        signer_material(&action.user_id, api_types::AccountKind::Trading, now).await?;
    if signer.account_id != trading_account_id {
        return Err(internal(
            "recovery request does not match the user's trading account".to_string(),
        ));
    }
    let payload = venue::UsdSend {
        destination,
        amount_micros: request.amount,
        time: action.nonce,
    };
    let digest = payload.digest()?;
    if digest != action.digest {
        db::tx::update(|connection| {
            db::repo::events::insert_audit(
                connection,
                "system",
                "action_digest_mismatch",
                None,
                Some("aborted"),
                now,
            )
        })
        .map_err(|error| map_db(error, None))?;
        return Err(ErrorCode::Internal {
            code: "action digest mismatch".to_string(),
        });
    }
    let signature = crypto::sign_with_key(&digest, path, &public_key).await?;
    let wire_payload = payload.body(&signature)?;
    db::tx::update(|connection| {
        db::repo::actions::mark_signed(
            connection,
            &action.action_id,
            action.worker_epoch,
            &signature.to_bytes65(),
            &wire_payload,
            now,
        )
    })
    .map_err(|error| map_db(error, None))?;
    db::tx::update(|connection| {
        db::repo::actions::mark_dispatching(connection, &action.action_id, action.worker_epoch, now)
    })
    .map_err(|error| map_db(error, None))?;

    match venue::post_usd_send(&payload, &signature).await {
        Ok((ExchangeOutcome::Accepted, _response)) => {
            let mut evidence = b"recovery".to_vec();
            evidence.extend_from_slice(&action.digest);
            let event_id = hl_sign::keccak256(&evidence);
            db::tx::update(|connection| {
                db::repo::ledger::recovery_confirm(
                    connection,
                    &action.user_id,
                    &trading_account_id,
                    request.amount,
                    now,
                    &event_id,
                )?;
                // 台帳で資金が動いたので予約も消費する。
                db::repo::funds::consume_reservation(connection, &action.user_id, request_id)?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Settled,
                    now,
                )?;
                db::repo::actions::mark_reconciled(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
        Ok((ExchangeOutcome::Rejected { message }, _response)) => {
            db::tx::update(|connection| {
                db::repo::funds::release_reservation(connection, &action.user_id, request_id, now)?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Rejected,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "recovery_rejected",
                    None,
                    Some(&message),
                    now,
                )?;
                db::repo::actions::mark_reconciled(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
        Err(error) => {
            let reason = format!("{error:?}");
            db::tx::update(|connection| {
                db::repo::actions::mark_unknown(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    &reason,
                    now,
                )?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Unknown,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "recovery_unknown",
                    None,
                    Some("result_unknown"),
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
        }
    }
    Ok(())
}

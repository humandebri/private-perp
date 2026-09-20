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

/// 取引口座（払出し先）を用意する。導出鍵の公開アドレスを保存する。
pub(crate) async fn provision_trading_account(
    user_id: &[u8; 32],
    now: u64,
) -> Result<([u8; 32], Vec<Vec<u8>>, [u8; 33], [u8; 20]), ErrorCode> {
    let account_id = crate::random::random32().await?;
    let path = crypto::derivation_path(&[
        b"private-perp",
        b"trading",
        hex::encode(account_id).as_bytes(),
    ]);
    let public_key = crypto::public_key(path.clone()).await?;
    let address = hl_sign::address_from_public_key(&public_key)
        .map_err(|error| internal(error.to_string()))?;

    let account = NewCustodyAccount {
        account_id: &account_id,
        user_id,
        kind: api_types::AccountKind::Trading,
        derivation_path: "private-perp/trading",
        master_address: &address,
        network: config::network_name(config::NETWORK),
    };
    let stored = db::tx::update(|connection| {
        db::repo::ledger::ensure_custody_account(connection, &account, now)
    })
    .map_err(|error| map_db(error, None))?;

    Ok((stored.account_id, path, public_key, stored.master_address))
}

/// 1つのactionを実行する。
async fn dispatch(action: &FundActionRow, now: u64) -> Result<(), ErrorCode> {
    if action.kind != "allocation" {
        // 払出し・回収は後続の段階で扱う（未対応は安全側で中止する）。
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

    let (account_id, path, public_key, address) =
        provision_trading_account(&action.user_id, now).await?;

    let payload = venue::UsdSend {
        destination: format!("0x{}", hex::encode(address)),
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
                let _ = account_id;
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
    if let Some(existing) = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, user_id, api_types::AccountKind::Reserve)
    })
    .map_err(|error| map_db(error, None))?
    {
        return Ok(existing.master_address);
    }

    let account_id = crate::random::random32().await?;
    let path = crypto::derivation_path(&[
        b"private-perp",
        b"reserve",
        hex::encode(account_id).as_bytes(),
    ]);
    let public_key = crypto::public_key(path).await?;
    let address = hl_sign::address_from_public_key(&public_key)
        .map_err(|error| internal(error.to_string()))?;
    let account = NewCustodyAccount {
        account_id: &account_id,
        user_id,
        kind: api_types::AccountKind::Reserve,
        derivation_path: "private-perp/reserve",
        master_address: &address,
        network: config::network_name(config::NETWORK),
    };
    db::tx::update(|connection| {
        db::repo::ledger::ensure_custody_account(connection, &account, now)
    })
    .map_err(|error| map_db(error, None))?;
    Ok(address)
}

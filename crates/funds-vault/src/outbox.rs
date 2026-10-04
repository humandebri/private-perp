//! 資金actionの永続outbox（`Implementation.md` 2.3・14.2、`docs/phase-0/state-machines.md` 2節）。
//!
//! 1つのactionを `claim → 署名 → signed → dispatching永続化 → POST → reconciled/unknown`
//! の順で処理する。**送信前に `dispatching` をCASで永続化**し、結果不明は再送しない。
//! 同期ブロック（DB更新）を完結させてから `await` する順序を守る。

use crate::auth::map_db;
use crate::crypto;
use crate::venue::{self, ExchangeOutcome};
use api_types::error::ErrorCode;
use api_types::fund::FundRequestState;
use api_types::journal::{RecoveryEvent, RecoveryPayload};
use api_types::operations::BudgetClass;
use db::repo::actions::FundActionRow;
use db::repo::funds::fund_request;
use db::repo::ledger::NewCustodyAccount;
use db::worker_permissions;

/// 1回のsweepで処理するaction数の上限。
const MAX_ACTIONS_PER_SWEEP: u32 = 4;

/// actionのリース期間（他workerとの競合を避ける）。
const ACTION_LEASE_MS: u64 = 30_000;

/// heartbeatの間隔（`#[ic_cdk::heartbeat]`は毎ラウンド呼ばれる）。
/// 自動sweep（timer）の間隔。試験ビルドは自動sweepを組まないため定数も持たない。
#[cfg(not(feature = "test-venue"))]
pub const SWEEP_INTERVAL_MS: u64 = 5_000;

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

// A POST may have reached HL even when journaling its response fails. Keep the
// reservation and make the action visible to the normal reconciliation path.
pub(crate) fn mark_post_unknown(
    action: &FundActionRow,
    request_id: &[u8],
    now: u64,
) -> Result<(), ErrorCode> {
    db::tx::update(|connection| {
        db::repo::actions::mark_unknown(
            connection,
            &action.action_id,
            action.worker_epoch,
            "result_unknown",
            now,
        )?;
        db::repo::funds::set_request_state(
            connection,
            &action.user_id,
            request_id,
            FundRequestState::Unknown,
            now,
        )
    })
    .map_err(|error| map_db(error, None))
}

fn recovery_settlement_event(
    action: &FundActionRow,
    request_id: &[u8],
    trading_account_id: &[u8; 32],
    amount_micros: u64,
    accepted: bool,
    response: &[u8],
) -> RecoveryEvent {
    let mut logical_id = b"recovery_post_result".to_vec();
    logical_id.extend_from_slice(&action.action_id);
    RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical_id).to_vec().into(),
        payload: RecoveryPayload::RecoveryPostResult {
            action_id: action.action_id.to_vec().into(),
            request_id: request_id.to_vec().into(),
            user_id: action.user_id.to_vec().into(),
            trading_account_id: trading_account_id.to_vec().into(),
            amount_micros,
            nonce: action.nonce,
            accepted,
            evidence_digest: hl_sign::keccak256(response).to_vec().into(),
            observed_at_ms: crate::clock::now_ms(),
        },
    }
}

pub(crate) fn fund_transfer_result_event(
    action: &FundActionRow,
    request: &db::repo::funds::FundRequestRow,
    request_id: &[u8],
    source_account_id: &[u8; 32],
    accepted: bool,
    response: &[u8],
) -> Result<RecoveryEvent, ErrorCode> {
    let destination = match action.kind.as_str() {
        "allocation" => request
            .account_id
            .ok_or(ErrorCode::PolicyUnavailable)?
            .to_vec(),
        "withdrawal" => {
            let address = request
                .destination
                .as_deref()
                .ok_or(ErrorCode::PolicyUnavailable)?;
            let bytes = hex::decode(address.strip_prefix("0x").unwrap_or(address))
                .map_err(|_| ErrorCode::PolicyUnavailable)?;
            if bytes.len() != 20 {
                return Err(ErrorCode::PolicyUnavailable);
            }
            bytes
        }
        _ => return Err(ErrorCode::PolicyUnavailable),
    };
    let mut logical_id = b"fund_transfer_result".to_vec();
    logical_id.extend_from_slice(&action.action_id);
    Ok(RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical_id).to_vec().into(),
        payload: RecoveryPayload::FundTransferResult {
            action_id: action.action_id.to_vec().into(),
            request_id: request_id.to_vec().into(),
            user_id: action.user_id.to_vec().into(),
            source_account_id: source_account_id.to_vec().into(),
            destination: destination.into(),
            kind: action.kind.clone(),
            amount_micros: request.amount,
            nonce: action.nonce,
            accepted,
            evidence_digest: hl_sign::keccak256(response).to_vec().into(),
            observed_at_ms: crate::clock::now_ms(),
        },
    })
}

/// 未処理のactionを有界に処理する。
pub async fn sweep(now: u64) -> Result<u32, ErrorCode> {
    let _ = crate::cycles::status();
    retry_transfer_results().await?;
    crate::recovery::sync_legacy(now).await?;
    crate::recovery::release_finished(now).await?;
    crate::recovery::reconcile_recoveries(now).await?;
    #[cfg(not(feature = "test-venue"))]
    for (action_id, epoch, address) in
        db::tx::query(|c| db::repo::actions::allocation_arrivals(c, now))
            .map_err(|e| map_db(e, None))?
    {
        let action = db::tx::query(|c| db::repo::actions::action_row(c, &action_id))
            .map_err(|e| map_db(e, None))?
            .ok_or(ErrorCode::PolicyUnavailable)?;
        let Some(attempt) = worker_permissions::begin("fund", &action_id, &action.user_id)
            .map_err(|e| map_db(e, None))?
        else {
            continue;
        };
        // One observation only; an unconfirmed arrival requires the owner to check again.
        let result = crate::deposits::reconcile_address(&address).await;
        db::tx::update(|c| db::repo::actions::defer_recovery_check(c, &action_id, epoch, now))
            .map_err(|e| map_db(e, None))?;
        if let Err(error) = result {
            ic_cdk::println!("allocation arrival pending: {error:?}");
        } else if db::tx::query(|c| db::repo::actions::finished(c, &action_id))
            .map_err(|e| map_db(e, None))?
        {
            attempt.completed().map_err(|e| map_db(e, None))?;
        }
    }

    let mut processed = 0;
    let mut first_error = None;
    for _ in 0..MAX_ACTIONS_PER_SWEEP {
        let claimed = db::tx::update(|connection| {
            db::repo::actions::claim_action(connection, now, ACTION_LEASE_MS)
        })
        .map_err(|error| map_db(error, None))?;
        let Some(action) = claimed else {
            break;
        };
        let Some(attempt) = worker_permissions::begin("fund", &action.action_id, &action.user_id)
            .map_err(|e| map_db(e, None))?
        else {
            continue;
        };
        let result = dispatch(&action, now).await;
        let state = db::tx::query(|c| db::repo::actions::action_state(c, &action.action_id))
            .map_err(|e| map_db(e, None))?;
        if result.is_ok()
            && matches!(
                state,
                Some(
                    api_types::fund::ActionState::Reconciled
                        | api_types::fund::ActionState::Aborted
                )
            )
        {
            attempt.completed().map_err(|e| map_db(e, None))?;
        }
        if let Err(error) = result {
            ic_cdk::println!("fund action stopped: {error:?}");
            first_error.get_or_insert(error);
        }
        processed += 1;
    }
    crate::recovery::release_finished(now).await?;
    if let Some(error) = first_error {
        return Err(error);
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
    let network = crate::environment::network_name()?;

    // Another call can finish creation while key derivation is awaiting. Only
    // the call that still needs an insert writes a recovery event.
    if let Some(existing) =
        db::tx::query(|connection| db::repo::ledger::custody_account(connection, user_id, kind))
            .map_err(|error| map_db(error, None))?
    {
        return Ok(existing);
    }

    let mut logical_id = b"custody_account".to_vec();
    if kind == api_types::AccountKind::Trading {
        logical_id.extend_from_slice(user_id);
    }
    logical_id.extend_from_slice(kind_name.as_bytes());
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical_id).to_vec().into(),
        payload: RecoveryPayload::CustodyAccount {
            user_id: (kind == api_types::AccountKind::Trading).then(|| user_id.to_vec().into()),
            account_id: account_id.to_vec().into(),
            kind: kind_name.to_string(),
            derivation_path: derivation_path.clone(),
            address: address.to_vec().into(),
            network: network.clone(),
        },
    };
    let ack = journal_client::append_recovery_event_if("vault", event.clone(), |connection| {
        Ok(db::repo::ledger::custody_account(connection, user_id, kind)?.is_none())
    })
    .await?;
    let Some(ack) = ack else {
        return db::tx::query(|connection| {
            db::repo::ledger::custody_account(connection, user_id, kind)
        })
        .map_err(|error| map_db(error, None))?
        .ok_or(ErrorCode::PolicyUnavailable);
    };

    let account = NewCustodyAccount {
        account_id: &account_id,
        user_id,
        kind,
        derivation_path: &derivation_path,
        master_address: &address,
        network: &network,
    };
    let result = db::tx::update(|connection| {
        if db::repo::ledger::custody_account(connection, user_id, kind)?.is_some() {
            return Err(db::error::Error::Conflict);
        }
        journal_client::record_recovery_event(connection, &event, &ack)?;
        db::repo::ledger::ensure_custody_account(connection, &account, now)
    });
    match result {
        Ok(account) => Ok(account),
        Err(error) => {
            journal_client::lock()?;
            Err(map_db(error, None))
        }
    }
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
// The external permission call is an await boundary. A concurrent resume,
// state change, or stale worker must still prevent this callback from posting.
fn require_current_dispatch(action: &FundActionRow) -> Result<(), ErrorCode> {
    db::tx::query(|c| {
        if db::repo::send_journal_client::locked(c)? {
            return Err(db::error::Error::Conflict);
        }
        let current = db::repo::actions::action_row(c, &action.action_id)?
            .ok_or(db::error::Error::NotFound)?;
        if current.worker_epoch != action.worker_epoch
            || current.dispatch_state != api_types::fund::ActionState::Dispatching
        {
            return Err(db::error::Error::Conflict);
        }
        Ok(())
    })
    .map_err(|error| map_db(error, None))
}

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

    let trading_account_id = request
        .account_id
        .ok_or_else(|| internal("allocation without trading account".into()))?;
    if crate::cycles::require_new().is_err() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if crate::eligibility::require_current(&action.user_id, &trading_account_id).is_err() {
        db::tx::update(|connection| {
            db::repo::actions::abort_unsent(
                connection,
                &action.action_id,
                action.worker_epoch,
                action.dispatch_state,
                "eligibility_expired",
                now,
            )?;
            db::repo::funds::set_request_state(
                connection,
                &action.user_id,
                &request_id,
                api_types::fund::FundRequestState::Rejected,
                now,
            )?;
            db::repo::funds::release_reservation(connection, &action.user_id, &request_id, now)
        })
        .map_err(|error| map_db(error, None))?;
        return Ok(());
    }

    // 資金は準備口座から出る。署名者は**準備口座**のmaster鍵で、宛先は受付時に保存した
    // 取引口座アドレス（保存値と一致しなければ下のdigest照合で拒否される）。
    let destination = request
        .destination
        .clone()
        .ok_or_else(|| internal("allocation request without a destination".to_string()))?;
    let (reserve, path, public_key) =
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
    let permit = crate::rest_budget::acquire(BudgetClass::NewRisk, 1).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if crate::cycles::require_new().is_err() {
        return Err(ErrorCode::PolicyUnavailable);
    }

    // The token can expire during signing or while waiting for REST budget.
    if crate::eligibility::require_current(&action.user_id, &trading_account_id).is_err() {
        db::tx::update(|connection| {
            db::repo::actions::abort_unsent(
                connection,
                &action.action_id,
                action.worker_epoch,
                action.dispatch_state,
                "eligibility_expired",
                now,
            )?;
            db::repo::funds::set_request_state(
                connection,
                &action.user_id,
                &request_id,
                api_types::fund::FundRequestState::Rejected,
                now,
            )?;
            db::repo::funds::release_reservation(connection, &action.user_id, &request_id, now)
        })
        .map_err(|error| map_db(error, None))?;
        return Ok(());
    }

    let intent = journal_client::intent(
        "allocation",
        &action.action_id,
        &reserve.account_id,
        action.nonce,
        &action.digest,
    );
    let ack = journal_client::prepare_send("vault", intent.clone()).await?;
    // 受領証跡と dispatching を同一transactionで永続化する。
    db::tx::update(|connection| {
        let current = db::repo::funds::fund_request(connection, &action.user_id, &request_id)?
            .ok_or(db::error::Error::NotFound)?;
        if current.state != FundRequestState::Reserved {
            return Err(db::error::Error::Conflict);
        }
        db::repo::actions::mark_signed(
            connection,
            &action.action_id,
            action.worker_epoch,
            &signature.to_bytes65(),
            &wire_payload,
            now,
        )?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(1)?;
        journal_client::record(connection, &intent, &ack)?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(2)?;
        db::repo::actions::mark_dispatching(
            connection,
            &action.action_id,
            action.worker_epoch,
            now,
        )?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(3)?;
        Ok(())
    })
    .map_err(|error| map_db(error, None))?;

    // A durable, single-use external permission is required even after local commit.
    // Cancellation during manual recovery permanently prevents a delayed send.
    journal_client::authorize_send(&intent).await?;
    require_current_dispatch(action)?;
    match venue::post_usd_send(&payload, &signature, &permit).await {
        Ok((ExchangeOutcome::Accepted, response)) => {
            let result = fund_transfer_result_event(
                action,
                &request,
                &request_id,
                &reserve.account_id,
                true,
                &response,
            )?;
            persist_transfer_result(&result).await?;
        }
        Ok((ExchangeOutcome::Rejected { .. }, response)) => {
            let result = fund_transfer_result_event(
                action,
                &request,
                &request_id,
                &reserve.account_id,
                false,
                &response,
            )?;
            persist_transfer_result(&result).await?;
        }
        Err(_error) => {
            // 送信した可能性がある。再送せず unknown として照合対象に残す。
            let reason = "result_unknown";
            db::tx::update(|connection| {
                db::repo::actions::mark_unknown(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    reason,
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
    let (reserve, path, public_key) =
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
    let permit = crate::rest_budget::acquire(BudgetClass::Exit, 1).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let intent = journal_client::intent(
        "withdrawal",
        &action.action_id,
        &reserve.account_id,
        action.nonce,
        &action.digest,
    );
    let ack = journal_client::prepare_send("vault", intent.clone()).await?;
    db::tx::update(|connection| {
        db::repo::actions::mark_signed(
            connection,
            &action.action_id,
            action.worker_epoch,
            &signature.to_bytes65(),
            &wire_payload,
            now,
        )?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(1)?;
        journal_client::record(connection, &intent, &ack)?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(2)?;
        db::repo::actions::mark_dispatching(
            connection,
            &action.action_id,
            action.worker_epoch,
            now,
        )?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(3)?;
        Ok(())
    })
    .map_err(|error| map_db(error, None))?;

    // A durable, single-use external permission is required even after local commit.
    // Cancellation during manual recovery permanently prevents a delayed send.
    journal_client::authorize_send(&intent).await?;
    require_current_dispatch(action)?;
    match venue::post_usd_send(&payload, &signature, &permit).await {
        Ok((ExchangeOutcome::Accepted, response)) => {
            let result = fund_transfer_result_event(
                action,
                request,
                request_id,
                &reserve.account_id,
                true,
                &response,
            )?;
            persist_transfer_result(&result).await?;
        }
        Ok((ExchangeOutcome::Rejected { .. }, response)) => {
            let result = fund_transfer_result_event(
                action,
                request,
                request_id,
                &reserve.account_id,
                false,
                &response,
            )?;
            persist_transfer_result(&result).await?;
        }
        Err(_error) => {
            let reason = "result_unknown";
            db::tx::update(|connection| {
                db::repo::actions::mark_unknown(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    reason,
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
    let fence = match crate::recovery::prepare(
        &trading_account_id,
        &action.user_id,
        &signer.master_address,
        request_id,
    )
    .await
    {
        Ok(token) => token,
        Err(ErrorCode::ReservationConflict) => {
            db::tx::update(|connection| {
                db::repo::funds::release_reservation(connection, &action.user_id, request_id, now)?;
                db::repo::funds::set_request_state(
                    connection,
                    &action.user_id,
                    request_id,
                    FundRequestState::Rejected,
                    now,
                )?;
                db::repo::actions::abort_unsent(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    api_types::fund::ActionState::Signing,
                    "account_not_flat",
                    now,
                )
            })
            .map_err(|error| map_db(error, None))?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    db::tx::update(|connection| {
        db::repo::actions::record_recovery_fence(
            connection,
            &action.action_id,
            action.worker_epoch,
            fence.epoch,
            now,
        )
    })
    .map_err(|error| map_db(error, None))?;
    let signature = crypto::sign_with_key(&digest, path, &public_key).await?;
    let wire_payload = payload.body(&signature)?;
    let permit = crate::rest_budget::acquire(BudgetClass::Exit, 1).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    crate::recovery::commit(fence.clone()).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let intent = journal_client::intent(
        "recovery",
        &action.action_id,
        &signer.account_id,
        action.nonce,
        &action.digest,
    );
    let ack = journal_client::prepare_send("vault", intent.clone()).await?;
    let send_now = crate::clock::now_ms();
    db::tx::update(|connection| {
        let current = db::repo::funds::fund_request(connection, &action.user_id, request_id)?
            .ok_or(db::error::Error::NotFound)?;
        if current.state != FundRequestState::Reserved
            || !db::repo::actions::recovery_lease_valid(
                connection,
                &action.action_id,
                action.worker_epoch,
                send_now,
            )?
        {
            return Err(db::error::Error::Conflict);
        }
        db::repo::actions::mark_signed(
            connection,
            &action.action_id,
            action.worker_epoch,
            &signature.to_bytes65(),
            &wire_payload,
            send_now,
        )?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(1)?;
        journal_client::record(connection, &intent, &ack)?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(2)?;
        db::repo::actions::mark_dispatching(
            connection,
            &action.action_id,
            action.worker_epoch,
            send_now,
        )?;
        #[cfg(feature = "test-venue")]
        crate::test_atomicity::checkpoint(3)?;
        Ok(())
    })
    .map_err(|error| map_db(error, None))?;

    // A durable, single-use external permission is required even after local commit.
    // Cancellation during manual recovery permanently prevents a delayed send.
    journal_client::authorize_send(&intent).await?;
    require_current_dispatch(action)?;
    match venue::post_usd_send(&payload, &signature, &permit).await {
        Ok((ExchangeOutcome::Accepted, response)) => {
            let settlement = recovery_settlement_event(
                action,
                request_id,
                &trading_account_id,
                request.amount,
                true,
                &response,
            );
            let settlement_ack =
                match journal_client::append_recovery_event("vault", settlement.clone()).await {
                    Ok(ack) => ack,
                    Err(_) => {
                        mark_post_unknown(action, request_id, now)?;
                        let _ = crate::recovery::mark_unknown(fence).await;
                        return Ok(());
                    }
                };
            let mut event_bytes = b"recovery_ack".to_vec();
            event_bytes.extend_from_slice(&action.action_id);
            let event_id = hl_sign::keccak256(&event_bytes);
            db::tx::update(|connection| {
                journal_client::record_recovery_event(connection, &settlement, &settlement_ack)?;
                db::repo::ledger::recovery_confirm(
                    connection,
                    &action.user_id,
                    &trading_account_id,
                    request.amount,
                    now,
                    &event_id,
                )?;
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
        Ok((ExchangeOutcome::Rejected { .. }, response)) => {
            let settlement = recovery_settlement_event(
                action,
                request_id,
                &trading_account_id,
                request.amount,
                false,
                &response,
            );
            let settlement_ack =
                match journal_client::append_recovery_event("vault", settlement.clone()).await {
                    Ok(ack) => ack,
                    Err(_) => {
                        mark_post_unknown(action, request_id, now)?;
                        let _ = crate::recovery::mark_unknown(fence).await;
                        return Ok(());
                    }
                };
            db::tx::update(|connection| {
                journal_client::record_recovery_event(connection, &settlement, &settlement_ack)?;
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
                    Some("venue_rejected"),
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
        Err(_error) => {
            let reason = "result_unknown";
            db::tx::update(|connection| {
                db::repo::actions::mark_unknown(
                    connection,
                    &action.action_id,
                    action.worker_epoch,
                    reason,
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
            let _ = crate::recovery::mark_unknown(fence).await;
        }
    }
    Ok(())
}

pub(crate) async fn persist_transfer_result(event: &RecoveryEvent) -> Result<(), ErrorCode> {
    let RecoveryPayload::FundTransferResult { action_id, .. } = &event.payload else {
        return Err(ErrorCode::PolicyUnavailable);
    };
    let id: [u8; 32] = action_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let encoded = candid::encode_one(event).map_err(|_| ErrorCode::PolicyUnavailable)?;
    db::tx::update(|c| db::repo::actions::save_transfer_result(c, &id, &encoded))
        .map_err(|e| map_db(e, None))?;
    retry_transfer_results().await
}
pub async fn retry_transfer_results() -> Result<(), ErrorCode> {
    let pending =
        db::tx::query(db::repo::actions::pending_transfer_results).map_err(|e| map_db(e, None))?;
    for encoded in pending {
        let event: RecoveryEvent =
            candid::decode_one(&encoded).map_err(|_| ErrorCode::PolicyUnavailable)?;
        let RecoveryPayload::FundTransferResult { action_id, .. } = &event.payload else {
            return Err(ErrorCode::PolicyUnavailable);
        };
        let id: [u8; 32] = action_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let action = db::tx::query(|c| db::repo::actions::action_row(c, &id))
            .map_err(|e| map_db(e, None))?
            .ok_or(ErrorCode::PolicyUnavailable)?;
        let Some(attempt) = worker_permissions::begin("result", &id, &action.user_id)
            .map_err(|e| map_db(e, None))?
        else {
            continue;
        };
        let ack = match journal_client::append_recovery_event("vault", event.clone()).await {
            Ok(ack) => ack,
            Err(ErrorCode::JournalWriterBusy) => return Ok(()),
            Err(error) => return Err(error),
        };
        let result = db::tx::update(|c| {
            journal_client::record_recovery_event(c, &event, &ack)?;
            journal_client::apply_fund_transfer_result(c, &event)?;
            let RecoveryPayload::FundTransferResult { action_id, .. } = &event.payload else {
                return Err(db::error::Error::Conflict);
            };
            db::repo::actions::delete_transfer_result(c, action_id.as_ref())
        });
        if let Err(error) = result {
            journal_client::lock()?;
            return Err(map_db(error, None));
        }
        attempt.completed().map_err(|e| map_db(e, None))?;
    }
    Ok(())
}

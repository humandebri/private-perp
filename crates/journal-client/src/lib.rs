//! 送信直前に独立ジャーナルの永続化を確認する共通クライアント。

use api_types::error::ErrorCode;
use api_types::journal::{
    JournalHead, JournalRecord, RecoveryEvent, RecoveryPayload, RecoveryRecord, SendIntent,
};
use candid::Principal;

fn scoped_role() -> Result<&'static str, ErrorCode> {
    match db::tx::active_scope() {
        Some(db::DbScope::Vault) => Ok("vault"),
        Some(db::DbScope::Core) => Ok("core"),
        _ => Err(ErrorCode::PolicyUnavailable),
    }
}

fn journal_worker_bytes() -> Result<Vec<u8>, ErrorCode> {
    if cfg!(feature = "embedded") {
        Ok(scoped_role()?.as_bytes().to_vec())
    } else {
        Ok(ic_cdk::api::canister_self().as_slice().to_vec())
    }
}
use ic_cdk::call::Call;
use ic_sqlite_vfs::db::UpdateConnection;
use sha2::{Digest, Sha256};

/// The journal receipt is tied to the durable single-writer epoch that began
/// the append. A callback from an older epoch cannot authorize a POST.
pub struct JournalAck {
    head: JournalHead,
    writer_epoch: u64,
}

fn map_db(error: db::error::Error) -> ErrorCode {
    ErrorCode::Internal {
        code: format!("{error:?}"),
    }
}

pub fn configure(principal: Principal) -> Result<(), ErrorCode> {
    if principal == Principal::anonymous() || principal.as_slice().is_empty() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    db::tx::update(|c| db::repo::send_journal_client::set_principal(c, principal.as_slice()))
        .map_err(map_db)
}

pub fn configured() -> Result<Option<Principal>, ErrorCode> {
    db::tx::query(db::repo::send_journal_client::principal)
        .map(|value| value.map(|bytes| Principal::from_slice(&bytes)))
        .map_err(map_db)
}

pub fn guard() -> Result<Option<Principal>, ErrorCode> {
    db::tx::query(db::repo::send_journal_client::guard_principal)
        .map(|value| value.map(|bytes| Principal::from_slice(&bytes)))
        .map_err(map_db)
}

/// 復元差分の取り込み状況。ステージ済みでも送信許可にはならない。
pub fn status() -> Result<(u64, u64, bool), ErrorCode> {
    db::tx::query(|c| {
        let (local, _, contiguous) = db::repo::send_journal_client::local_head(c)?;
        if !contiguous {
            return Err(db::error::Error::Invariant("receipt gap"));
        }
        let (staged, _) = db::repo::send_journal_client::stage_head(c)?;
        Ok((local, staged, db::repo::send_journal_client::locked(c)?))
    })
    .map_err(map_db)
}

/// Latest V2 sequence represented by local receipts or staged recovery data.
/// A staged record is not a replay receipt and never authorizes a send.
pub fn recovery_stage_status() -> Result<(u64, bool), ErrorCode> {
    db::tx::query(|c| {
        let (sequence, _) = db::repo::send_journal_client::recovery_stage_head(c)?;
        Ok((sequence, db::repo::send_journal_client::locked(c)?))
    })
    .map_err(map_db)
}

pub fn replay_pending_validation() -> Result<bool, ErrorCode> {
    db::tx::query(db::repo::send_journal_client::replay_pending_validation).map_err(map_db)
}

/// Read-only local stop state for user-facing status. It grants no resume
/// authority and may become stale before the next remote journal check.
pub fn public_status() -> Result<(bool, bool), ErrorCode> {
    db::tx::query(|c| {
        let pending = db::repo::send_journal_client::replay_pending_validation(c)?;
        let locked = db::repo::send_journal_client::locked(c)?
            || pending
            || db::repo::send_journal_client::has_staged(c)?
            || db::repo::send_journal_client::has_recovery_staged(c)?;
        Ok((locked, pending))
    })
    .map_err(map_db)
}

pub fn set_guard(principal: Principal) -> Result<(), ErrorCode> {
    if principal == Principal::anonymous() || principal.as_slice().is_empty() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    db::tx::update(|c| db::repo::send_journal_client::set_guard(c, principal.as_slice()))
        .map_err(map_db)
}

/// Unified builds require the administrator explicitly supplied at installation.
/// Standalone builds retain the existing guard authorization.
pub fn require_management() -> Result<(), ErrorCode> {
    #[cfg(feature = "embedded")]
    {
        let caller = ic_cdk::api::msg_caller();
        if caller != Principal::anonymous()
            && db::tx::is_application_admin(caller.as_slice()).map_err(map_db)?
        {
            Ok(())
        } else {
            Err(ErrorCode::Unauthenticated {
                reason: "application administrator required".into(),
            })
        }
    }
    #[cfg(not(feature = "embedded"))]
    {
        let guard = db::tx::query(db::repo::send_journal_client::guard_principal)
            .map_err(map_db)?
            .ok_or(ErrorCode::PolicyUnavailable)?;
        if ic_cdk::api::msg_caller().as_slice() != guard.as_slice() {
            return Err(ErrorCode::Unauthenticated {
                reason: "management requires guard".into(),
            });
        }
        Ok(())
    }
}

/// Reconcile both journal chains before lifting the post-upgrade send lock.
pub async fn resume(role: &str) -> Result<(), ErrorCode> {
    require_management()?;
    db::tx::update(|c| db::repo::send_journal_client::set_locked(c, true)).map_err(map_db)?;
    let principal = configured()?.ok_or(ErrorCode::PolicyUnavailable)?;
    let response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_head")
            .with_arg(role.to_string())
            .await
    } else {
        Call::bounded_wait(principal, "head").with_arg(()).await
    }
    .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let remote = response
        .candid::<Result<JournalHead, ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)??;
    // The V2 business stream has its own sequence. Stage and verify its next
    // batch, but do not treat staged events as a replayed business state.
    let recovery_response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_recovery_head")
            .with_arg(role.to_string())
            .await
    } else {
        Call::bounded_wait(principal, "recovery_head")
            .with_arg(())
            .await
    }
    .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let recovery_remote = recovery_response
        .candid::<Result<JournalHead, ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)??;
    let (mut recovery_sequence, mut recovery_hash, mut recovery_contiguous) =
        db::tx::query(db::repo::send_journal_client::recovery_local_head).map_err(map_db)?;
    if recovery_contiguous && recovery_sequence < recovery_remote.sequence {
        stage_missing_recovery(principal, &recovery_remote).await?;
    }
    let (sequence, hash, contiguous) =
        db::tx::query(db::repo::send_journal_client::local_head).map_err(map_db)?;
    let unresolved =
        db::tx::query(|c| db::repo::send_journal_client::unresolved_without_receipt(c, role))
            .map_err(map_db)?;
    if sequence < remote.sequence && contiguous {
        // 差分は受領済みPOSTへ昇格させず、検証したhash鎖だけを永続ステージへ取り込む。
        // 台帳・予約・外部状態の再構築証跡がまだ無い記録は送信停止のままにする。
        stage_missing(principal, &remote).await?;
    }
    // Both streams may be ahead of an older backup. Stage each independent
    // chain before returning the fail-closed result so a later recovery pass
    // has the complete pair of deltas.
    let has_staged = db::tx::query(db::repo::send_journal_client::has_staged).map_err(map_db)?;
    let (staged_send_sequence, staged_send_hash) =
        db::tx::query(db::repo::send_journal_client::stage_head).map_err(map_db)?;
    if matches!(role, "core" | "vault")
        && contiguous
        && !unresolved
        // A staged V1 POST is never promoted to a receipt here. A verified
        // complete V1 chain can coexist with replaying the earlier V2
        // acceptance, which restores its reservation while sends stay locked.
        && staged_send_sequence == remote.sequence
        && staged_send_hash.as_slice() == remote.hash.as_ref()
    {
        let (staged_sequence, staged_hash) =
            db::tx::query(db::repo::send_journal_client::recovery_stage_head).map_err(map_db)?;
        if staged_sequence == recovery_remote.sequence
            && staged_hash.as_slice() == recovery_remote.hash.as_ref()
        {
            if role == "core" {
                replay_core_prefix()?;
            } else {
                replay_vault_identity_registrations()?;
            }
            (recovery_sequence, recovery_hash, recovery_contiguous) =
                db::tx::query(db::repo::send_journal_client::recovery_local_head)
                    .map_err(map_db)?;
        }
    }
    let has_recovery_staged =
        db::tx::query(db::repo::send_journal_client::has_recovery_staged).map_err(map_db)?;
    let replay_pending =
        db::tx::query(db::repo::send_journal_client::replay_pending_validation).map_err(map_db)?;
    if !contiguous
        || !recovery_contiguous
        || unresolved
        || has_recovery_staged
        || has_staged
        || replay_pending
        || sequence != remote.sequence
        || hash.as_slice() != remote.hash.as_ref()
        || recovery_sequence != recovery_remote.sequence
        || recovery_hash.as_slice() != recovery_remote.hash.as_ref()
    {
        return Err(ErrorCode::PolicyUnavailable);
    }
    db::tx::update(db::repo::send_journal_client::unlock_after_resume).map_err(map_db)
}

fn validate_staged_recovery_event(
    c: &UpdateConnection<'_>,
    event: &db::repo::send_journal_client::StagedRecoveryEvent,
) -> Result<(), db::error::Error> {
    if event.version != 1 {
        return Err(db::error::Error::Invariant("bad staged recovery version"));
    }
    let (prior_sequence, prior_hash, contiguous) =
        db::repo::send_journal_client::recovery_local_head(c)?;
    if !contiguous
        || prior_sequence.checked_add(1) != Some(event.sequence)
        || prior_hash != event.previous_hash
    {
        return Err(db::error::Error::Invariant(
            "staged recovery predecessor mismatch",
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"private-perp/recovery-event/v1");
    hasher.update(event.previous_hash);
    hasher.update(event.sequence.to_be_bytes());
    hasher.update(
        journal_worker_bytes().map_err(|_| db::error::Error::Invariant("missing journal role"))?,
    );
    hasher.update(event.logical_id);
    hasher.update(event.version.to_be_bytes());
    hasher.update(&event.payload);
    let computed: [u8; 32] = hasher.finalize().into();
    if computed != event.hash {
        return Err(db::error::Error::Invariant("staged recovery hash mismatch"));
    }
    Ok(())
}

fn kind_name(kind: api_types::AccountKind) -> &'static str {
    match kind {
        api_types::AccountKind::Reserve => "reserve",
        api_types::AccountKind::Trading => "trading",
    }
}

/// Reconstruct the unsigned usdSend action from the private, typed event.
/// The value must match the vault's queued action exactly; signatures are
/// never copied to the recovery stream.
fn recovered_usd_send(
    c: &ic_sqlite_vfs::db::connection::Connection,
    destination: &str,
    amount_micros: u64,
    nonce: u64,
) -> Result<(Vec<u8>, [u8; 32]), db::error::Error> {
    let configured = db::repo::vault_config::environment(c)?;
    let network =
        hl_types::environment::parse_network(configured.network.as_deref().unwrap_or("local"))
            .map_err(|_| db::error::Error::Invariant("bad recovery network"))?;
    let action = hl_sign::usd_send::UsdSend {
        destination,
        amount_micros,
        time: nonce,
    };
    let digest = action
        .digest(network)
        .map_err(|_| db::error::Error::Invariant("bad recovery usdSend digest"))?;
    let canonical_action = action
        .body(
            network,
            &hl_sign::Signature {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 27,
            },
        )
        .map_err(|_| db::error::Error::Invariant("bad recovery usdSend body"))?;
    Ok((canonical_action, digest))
}

/// Replay a verified prefix of vault identity, custody, and claim state.
/// The restored balances and external venue evidence still require validation.
fn replay_vault_identity_registrations() -> Result<(), ErrorCode> {
    for _ in 0..100 {
        let replayed = db::tx::update(|c| {
            let Some(event) = db::repo::send_journal_client::next_recovery_event(c)? else {
                return Ok(false);
            };
            let payload: RecoveryPayload = candid::decode_one(&event.payload)
                .map_err(|_| db::error::Error::Invariant("invalid staged recovery payload"))?;
            validate_staged_recovery_event(c, &event)?;
            match payload {
                RecoveryPayload::IdentityRegistration {
                    user_id,
                    owner,
                    eoa_address,
                    network,
                } => {
                    if owner == Principal::anonymous() {
                        return Err(db::error::Error::Invariant("invalid staged identity owner"));
                    }
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let eoa_address: [u8; 20] = eoa_address
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged eoa"))?;
                    let configured = db::repo::vault_config::environment(c)?;
                    if configured.network.as_deref().unwrap_or("local") != network {
                        return Err(db::error::Error::Invariant(
                            "staged identity network mismatch",
                        ));
                    }
                    let mut id_material = b"vault_identity".to_vec();
                    id_material.extend_from_slice(network.as_bytes());
                    id_material.extend_from_slice(&eoa_address);
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged identity id mismatch"));
                    }
                    if let Some(existing) = db::repo::auth::find_identity_by_eoa(c, &eoa_address)? {
                        if existing.user_id != user_id {
                            return Err(db::error::Error::Conflict);
                        }
                    } else {
                        let created = db::repo::auth::ensure_identity(
                            c,
                            &eoa_address,
                            &user_id,
                            ic_cdk::api::time() / 1_000_000,
                        )?;
                        if created.user_id != user_id {
                            return Err(db::error::Error::Conflict);
                        }
                    }
                }
                RecoveryPayload::CustodyAccount {
                    user_id,
                    account_id,
                    kind,
                    derivation_path,
                    address,
                    network,
                } => {
                    let owner_id: Option<[u8; 32]> = user_id
                        .map(|id| {
                            id.as_ref()
                                .try_into()
                                .map_err(|_| db::error::Error::Invariant("bad staged user"))
                        })
                        .transpose()?;
                    let user_id = owner_id.unwrap_or([0; 32]);
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged account"))?;
                    let address: [u8; 20] = address
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged address"))?;
                    let kind = match kind.as_str() {
                        "reserve" => api_types::AccountKind::Reserve,
                        "trading" => api_types::AccountKind::Trading,
                        _ => return Err(db::error::Error::Invariant("bad staged account kind")),
                    };
                    if (kind == api_types::AccountKind::Reserve) != owner_id.is_none() {
                        return Err(db::error::Error::Invariant("bad custody ownership"));
                    }
                    let configured = db::repo::vault_config::environment(c)?;
                    if configured.network.as_deref().unwrap_or("local") != network {
                        return Err(db::error::Error::Invariant(
                            "staged account network mismatch",
                        ));
                    }
                    let expected_path = format!(
                        "private-perp/{}/{}",
                        kind_name(kind),
                        hex::encode(account_id)
                    );
                    if derivation_path != expected_path {
                        return Err(db::error::Error::Invariant("staged account path mismatch"));
                    }
                    let mut id_material = b"custody_account".to_vec();
                    if let Some(owner_id) = owner_id {
                        id_material.extend_from_slice(&owner_id);
                    }
                    id_material.extend_from_slice(kind_name(kind).as_bytes());
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged account id mismatch"));
                    }
                    if let Some(owner) = db::repo::ledger::custody_account_by_address(c, &address)?
                        && (owner.user_id != owner_id
                            || owner.account_id != account_id
                            || owner.kind != kind_name(kind))
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    match db::repo::ledger::custody_account(c, &user_id, kind)? {
                        Some(existing)
                            if existing.account_id == account_id
                                && existing.derivation_path == derivation_path
                                && existing.master_address == address
                                && existing.network == network => {}
                        Some(_) => return Err(db::error::Error::Conflict),
                        None => {
                            db::repo::ledger::ensure_custody_account(
                                c,
                                &db::repo::ledger::NewCustodyAccount {
                                    account_id: &account_id,
                                    user_id: &user_id,
                                    kind,
                                    derivation_path: &derivation_path,
                                    master_address: &address,
                                    network: &network,
                                },
                                ic_cdk::api::time() / 1_000_000,
                            )?;
                        }
                    }
                }
                RecoveryPayload::DepositCredit {
                    sender,
                    tx_hash,
                    network,
                    address,
                    amount_micros,
                    observed_at_ms,
                } => {
                    let sender: Option<[u8; 20]> = sender
                        .map(|value| {
                            value
                                .as_ref()
                                .try_into()
                                .map_err(|_| db::error::Error::Invariant("bad deposit sender"))
                        })
                        .transpose()?;
                    let tx_hash = tx_hash.as_ref();
                    let address: [u8; 20] = address
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged deposit address"))?;
                    if tx_hash.is_empty()
                        || tx_hash.len() > 64
                        || amount_micros == 0
                        || amount_micros > i64::MAX as u64
                        || observed_at_ms == 0
                        || observed_at_ms > i64::MAX as u64
                    {
                        return Err(db::error::Error::Invariant("bad staged deposit"));
                    }
                    let configured = db::repo::vault_config::environment(c)?;
                    if configured.network.as_deref().unwrap_or("local") != network {
                        return Err(db::error::Error::Invariant(
                            "staged deposit network mismatch",
                        ));
                    }
                    let mut event_material = b"deposit".to_vec();
                    event_material.extend_from_slice(tx_hash);
                    let event_id = hl_sign::keccak256(&event_material);
                    let mut id_material = b"deposit_credit".to_vec();
                    id_material.extend_from_slice(network.as_bytes());
                    id_material.extend_from_slice(&event_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged deposit id mismatch"));
                    }
                    // A missing account mapping could turn a user's deposit
                    // into suspense. Trading credits additionally depend on
                    // allocation state. Keep those events staged until the
                    // corresponding state can be reconstructed and checked.
                    let Some(owner) = db::repo::ledger::custody_account_by_address(c, &address)?
                    else {
                        return Ok(false);
                    };
                    if owner.kind != "reserve" {
                        return Ok(false);
                    }
                    let account = db::repo::ledger::custody_account(
                        c,
                        &[0; 32],
                        api_types::AccountKind::Reserve,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    if account.account_id != owner.account_id
                        || account.master_address != address
                        || account.network != network
                        || account.state != "active"
                        || !db::repo::deposits::credit_external_deposit(
                            c,
                            &event_id,
                            &network,
                            tx_hash,
                            amount_micros,
                            &address,
                            "usdc",
                            observed_at_ms,
                            sender.as_ref(),
                        )?
                    {
                        return Err(db::error::Error::Conflict);
                    }
                }
                RecoveryPayload::DepositClaim {
                    event_id,
                    user_id,
                    amount_micros,
                    claimed_at_ms,
                } => {
                    let event_id: [u8; 32] = event_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged event"))?;
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    if amount_micros == 0
                        || amount_micros > i64::MAX as u64
                        || claimed_at_ms == 0
                        || claimed_at_ms > i64::MAX as u64
                        || db::repo::auth::identity_by_user(c, &user_id)?.is_none()
                    {
                        return Err(db::error::Error::Invariant("bad staged deposit claim"));
                    }
                    let mut id_material = b"deposit_claim".to_vec();
                    id_material.extend_from_slice(&event_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged claim id mismatch"));
                    }
                    let configured = db::repo::vault_config::environment(c)?;
                    let network = configured.network.as_deref().unwrap_or("local");
                    let external = db::repo::events::find_external_event(c, network, &event_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    let kind = db::repo::ledger::journal_kind_by_external_event(c, &event_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    if external.amount != amount_micros
                        || kind != "deposit_unmatched"
                        || db::repo::ledger::unmatched_deposit_claimed(c, &event_id)?
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::ledger::claim_unmatched_deposit(
                        c,
                        &user_id,
                        amount_micros,
                        claimed_at_ms,
                        &event_id,
                    )?;
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
                    let action_id: [u8; 32] = action_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged action"))?;
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged account"))?;
                    let destination: [u8; 20] = destination
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged destination"))?;
                    let body_hash: [u8; 32] = body_hash
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged body hash"))?;
                    let request_id = request_id.as_ref();
                    if request_id.is_empty()
                        || request_id.len() > 64
                        || amount_micros == 0
                        || amount_micros > i64::MAX as u64
                        || nonce == 0
                        || nonce > i64::MAX as u64
                        || accepted_at_ms == 0
                        || accepted_at_ms > i64::MAX as u64
                    {
                        return Err(db::error::Error::Invariant("bad staged allocation"));
                    }
                    let mut id_material = b"allocation_accepted".to_vec();
                    id_material.extend_from_slice(&user_id);
                    id_material.extend_from_slice(request_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id
                        || hl_sign::keccak256_concat(&[
                            b"allocation",
                            &amount_micros.to_be_bytes(),
                            request_id,
                        ]) != body_hash
                    {
                        return Err(db::error::Error::Invariant(
                            "staged allocation identity mismatch",
                        ));
                    }
                    let identity = db::repo::auth::identity_by_user(c, &user_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    if identity.status != "active"
                        || db::repo::funds::fund_request(c, &user_id, request_id)?.is_some()
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    let reserve = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Reserve,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let trading = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Trading,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let configured = db::repo::vault_config::environment(c)?;
                    let network = configured.network.as_deref().unwrap_or("local");
                    if reserve.state != "active"
                        || trading.state != "active"
                        || reserve.network != network
                        || trading.network != network
                        || trading.account_id != account_id
                        || trading.master_address != destination
                        || db::repo::actions::next_master_nonce(c, "reserve", accepted_at_ms)?
                            != nonce
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    let destination_text = format!("0x{}", hex::encode(destination));
                    let (canonical_action, digest) =
                        recovered_usd_send(c, &destination_text, amount_micros, nonce)?;
                    let accepted = db::repo::funds::accept_fund_request(
                        c,
                        &db::repo::funds::NewFundRequest {
                            user_id: &user_id,
                            client_request_id: request_id,
                            body_hash: &body_hash,
                            kind: db::repo::funds::RequestKind::Allocation,
                            account_id: Some(&account_id),
                            amount: amount_micros,
                            destination: Some(&destination_text),
                        },
                        accepted_at_ms,
                    )?;
                    if accepted != db::repo::funds::AcceptOutcome::Accepted {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::funds::reserve_funds(
                        c,
                        &user_id,
                        request_id,
                        &db::repo::ledger::user_reserve(&user_id),
                        amount_micros,
                        accepted_at_ms,
                    )?;
                    db::repo::funds::set_request_state(
                        c,
                        &user_id,
                        request_id,
                        api_types::fund::FundRequestState::Reserved,
                        accepted_at_ms,
                    )?;
                    if db::repo::actions::allocate_master_nonce(c, "reserve", accepted_at_ms)?
                        != nonce
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::actions::insert_fund_action(
                        c,
                        &db::repo::actions::NewFundAction {
                            action_id,
                            user_id,
                            client_request_id: Some(request_id.to_vec()),
                            kind: "allocation".to_string(),
                            signer_id: "reserve".to_string(),
                            canonical_action,
                            digest,
                            nonce,
                        },
                        accepted_at_ms,
                    )?;
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
                    let action_id: [u8; 32] = action_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged action"))?;
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let reserve_account_id: [u8; 32] = reserve_account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged reserve"))?;
                    let destination: [u8; 20] = destination
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged destination"))?;
                    let body_hash: [u8; 32] = body_hash
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged body hash"))?;
                    let request_id = request_id.as_ref();
                    if request_id.is_empty()
                        || request_id.len() > 64
                        || amount_micros == 0
                        || amount_micros > i64::MAX as u64
                        || nonce == 0
                        || nonce > i64::MAX as u64
                        || intent_nonce > i64::MAX as u64
                        || accepted_at_ms == 0
                        || accepted_at_ms > i64::MAX as u64
                        || intent_expires_at_ms > i64::MAX as u64
                        || intent_expires_at_ms <= accepted_at_ms
                    {
                        return Err(db::error::Error::Invariant("bad staged withdrawal"));
                    }
                    let mut id_material = b"withdrawal_accepted".to_vec();
                    id_material.extend_from_slice(&user_id);
                    id_material.extend_from_slice(request_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id
                        || hl_sign::keccak256_concat(&[
                            b"withdrawal",
                            &amount_micros.to_be_bytes(),
                            &intent_nonce.to_be_bytes(),
                            &intent_expires_at_ms.to_be_bytes(),
                            &destination,
                            request_id,
                        ]) != body_hash
                    {
                        return Err(db::error::Error::Invariant(
                            "staged withdrawal identity mismatch",
                        ));
                    }
                    let identity = db::repo::auth::identity_by_user(c, &user_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    let reserve = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Reserve,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let configured = db::repo::vault_config::environment(c)?;
                    if identity.status != "active"
                        || identity.eoa_address != destination
                        || reserve.account_id != reserve_account_id
                        || reserve.state != "active"
                        || reserve.network != configured.network.as_deref().unwrap_or("local")
                        || db::repo::funds::fund_request(c, &user_id, request_id)?.is_some()
                        || db::repo::auth::intent_nonce_used(c, &user_id, intent_nonce)?
                        || db::repo::actions::next_master_nonce(c, "reserve", accepted_at_ms)?
                            != nonce
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    let destination_text = format!("0x{}", hex::encode(destination));
                    let (canonical_action, digest) =
                        recovered_usd_send(c, &destination_text, amount_micros, nonce)?;
                    let accepted = db::repo::funds::accept_fund_request(
                        c,
                        &db::repo::funds::NewFundRequest {
                            user_id: &user_id,
                            client_request_id: request_id,
                            body_hash: &body_hash,
                            kind: db::repo::funds::RequestKind::Withdrawal,
                            account_id: None,
                            amount: amount_micros,
                            destination: Some(&destination_text),
                        },
                        accepted_at_ms,
                    )?;
                    if accepted != db::repo::funds::AcceptOutcome::Accepted {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::auth::use_intent_nonce(
                        c,
                        &user_id,
                        intent_nonce,
                        request_id,
                        accepted_at_ms,
                    )?;
                    db::repo::funds::reserve_funds(
                        c,
                        &user_id,
                        request_id,
                        &db::repo::ledger::user_reserve(&user_id),
                        amount_micros,
                        accepted_at_ms,
                    )?;
                    db::repo::ledger::withdrawal_reserve(
                        c,
                        &user_id,
                        amount_micros,
                        accepted_at_ms,
                        request_id,
                    )?;
                    db::repo::funds::set_request_state(
                        c,
                        &user_id,
                        request_id,
                        api_types::fund::FundRequestState::Reserved,
                        accepted_at_ms,
                    )?;
                    if db::repo::actions::allocate_master_nonce(c, "reserve", accepted_at_ms)?
                        != nonce
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::actions::insert_fund_action(
                        c,
                        &db::repo::actions::NewFundAction {
                            action_id,
                            user_id,
                            client_request_id: Some(request_id.to_vec()),
                            kind: "withdrawal".to_string(),
                            signer_id: "reserve".to_string(),
                            canonical_action,
                            digest,
                            nonce,
                        },
                        accepted_at_ms,
                    )?;
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
                    let action_id: [u8; 32] = action_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged action"))?;
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let trading_account_id: [u8; 32] = trading_account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged trading account"))?;
                    let reserve_account_id: [u8; 32] = reserve_account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged reserve account"))?;
                    let destination: [u8; 20] = destination
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged destination"))?;
                    let body_hash: [u8; 32] = body_hash
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged body hash"))?;
                    let request_id = request_id.as_ref();
                    if request_id.is_empty()
                        || request_id.len() > 64
                        || amount_micros == 0
                        || amount_micros > i64::MAX as u64
                        || nonce == 0
                        || nonce > i64::MAX as u64
                        || accepted_at_ms == 0
                        || accepted_at_ms > i64::MAX as u64
                    {
                        return Err(db::error::Error::Invariant("bad staged recovery"));
                    }
                    let mut id_material = b"recovery_accepted".to_vec();
                    id_material.extend_from_slice(&user_id);
                    id_material.extend_from_slice(request_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id
                        || hl_sign::keccak256_concat(&[
                            b"recovery",
                            &amount_micros.to_be_bytes(),
                            request_id,
                        ]) != body_hash
                    {
                        return Err(db::error::Error::Invariant(
                            "staged recovery identity mismatch",
                        ));
                    }
                    let identity = db::repo::auth::identity_by_user(c, &user_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    let trading = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Trading,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let reserve = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Reserve,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let configured = db::repo::vault_config::environment(c)?;
                    let network = configured.network.as_deref().unwrap_or("local");
                    let signer_nonce_key = format!("trading:{}", hex::encode(trading_account_id));
                    if identity.status != "active"
                        || trading.account_id != trading_account_id
                        || reserve.account_id != reserve_account_id
                        || reserve.master_address != destination
                        || trading.state != "active"
                        || reserve.state != "active"
                        || trading.network != network
                        || reserve.network != network
                        || db::repo::funds::fund_request(c, &user_id, request_id)?.is_some()
                        || db::repo::actions::next_master_nonce(
                            c,
                            &signer_nonce_key,
                            accepted_at_ms,
                        )? != nonce
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    let destination_text = format!("0x{}", hex::encode(destination));
                    let (canonical_action, digest) =
                        recovered_usd_send(c, &destination_text, amount_micros, nonce)?;
                    let accepted = db::repo::funds::accept_fund_request(
                        c,
                        &db::repo::funds::NewFundRequest {
                            user_id: &user_id,
                            client_request_id: request_id,
                            body_hash: &body_hash,
                            kind: db::repo::funds::RequestKind::Recovery,
                            account_id: Some(&trading_account_id),
                            amount: amount_micros,
                            destination: Some(&destination_text),
                        },
                        accepted_at_ms,
                    )?;
                    if accepted != db::repo::funds::AcceptOutcome::Accepted {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::funds::reserve_trading_funds(
                        c,
                        &user_id,
                        request_id,
                        "user_trading",
                        amount_micros,
                        accepted_at_ms,
                    )?;
                    db::repo::funds::set_request_state(
                        c,
                        &user_id,
                        request_id,
                        api_types::fund::FundRequestState::Reserved,
                        accepted_at_ms,
                    )?;
                    if db::repo::actions::allocate_master_nonce(
                        c,
                        &signer_nonce_key,
                        accepted_at_ms,
                    )? != nonce
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::actions::insert_fund_action(
                        c,
                        &db::repo::actions::NewFundAction {
                            action_id,
                            user_id,
                            client_request_id: Some(request_id.to_vec()),
                            kind: "recovery".to_string(),
                            signer_id: "trading".to_string(),
                            canonical_action,
                            digest,
                            nonce,
                        },
                        accepted_at_ms,
                    )?;
                }
                RecoveryPayload::TradingBalanceObserved {
                    user_id,
                    account_id,
                    previous_equity,
                    equity,
                    observed_at_ms,
                } => {
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Conflict)?;
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Conflict)?;
                    let mut logical = b"trading_balance_observed".to_vec();
                    logical.extend_from_slice(&account_id);
                    logical.extend_from_slice(&observed_at_ms.to_be_bytes());
                    if hl_sign::keccak256(&logical) != event.logical_id {
                        return Err(db::error::Error::Conflict);
                    }
                    db::repo::ledger::observe_trading_balance(
                        c,
                        &user_id,
                        &account_id,
                        previous_equity,
                        equity,
                        observed_at_ms,
                        &event.logical_id,
                    )?;
                }
                RecoveryPayload::FundTransferResult { .. } => {
                    let payload = candid::decode_one(&event.payload)
                        .map_err(|_| db::error::Error::Invariant("invalid transfer payload"))?;
                    apply_fund_transfer_result(
                        c,
                        &RecoveryEvent {
                            version: 1,
                            logical_id: event.logical_id.to_vec().into(),
                            payload,
                        },
                    )?;
                }
                RecoveryPayload::RecoveryPostResult {
                    action_id,
                    request_id,
                    user_id,
                    trading_account_id,
                    amount_micros,
                    nonce,
                    accepted,
                    evidence_digest,
                    observed_at_ms,
                } => {
                    let action_id: [u8; 32] = action_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged action"))?;
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let trading_account_id: [u8; 32] = trading_account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged trading account"))?;
                    let request_id = request_id.as_ref();
                    if request_id.is_empty()
                        || request_id.len() > 64
                        || evidence_digest.as_ref().len() != 32
                        || amount_micros == 0
                        || amount_micros > i64::MAX as u64
                        || nonce == 0
                        || observed_at_ms == 0
                        || observed_at_ms > i64::MAX as u64
                    {
                        return Err(db::error::Error::Invariant("bad staged recovery result"));
                    }
                    let mut id_material = b"recovery_post_result".to_vec();
                    id_material.extend_from_slice(&action_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged recovery id mismatch"));
                    }
                    let action = db::repo::actions::action_row(c, &action_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    // Historical settlements use a separate event variant and
                    // require their persisted HL history proof.
                    if action.dispatch_state == api_types::fund::ActionState::Unknown {
                        return Ok(false);
                    }
                    let request = db::repo::funds::fund_request(c, &user_id, request_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    let trading = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Trading,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let reserve = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Reserve,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    let destination = format!("0x{}", hex::encode(reserve.master_address));
                    if action.user_id != user_id
                        || action.client_request_id.as_deref() != Some(request_id)
                        || action.kind != "recovery"
                        || action.signer_id != "trading"
                        || action.nonce != nonce
                        || action.dispatch_state != api_types::fund::ActionState::Dispatching
                        || request.kind != db::repo::funds::RequestKind::Recovery
                        || request.state != api_types::fund::FundRequestState::Reserved
                        || request.amount != amount_micros
                        || request.account_id != Some(trading_account_id)
                        || request.destination.as_deref() != Some(destination.as_str())
                        || trading.account_id != trading_account_id
                        || trading.state != "active"
                        || reserve.state != "active"
                        || !db::repo::send_journal_client::receipt_matches(
                            c,
                            "recovery",
                            &action_id,
                            &trading_account_id,
                            nonce,
                            &action.digest,
                        )?
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    if accepted {
                        let mut evidence = b"recovery_ack".to_vec();
                        evidence.extend_from_slice(&action_id);
                        let event_id = hl_sign::keccak256(&evidence);
                        db::repo::ledger::recovery_confirm(
                            c,
                            &user_id,
                            &trading_account_id,
                            amount_micros,
                            observed_at_ms,
                            &event_id,
                        )?;
                        db::repo::funds::consume_reservation(c, &user_id, request_id)?;
                        db::repo::funds::set_request_state(
                            c,
                            &user_id,
                            request_id,
                            api_types::fund::FundRequestState::Settled,
                            observed_at_ms,
                        )?;
                    } else {
                        db::repo::funds::release_reservation(
                            c,
                            &user_id,
                            request_id,
                            observed_at_ms,
                        )?;
                        db::repo::funds::set_request_state(
                            c,
                            &user_id,
                            request_id,
                            api_types::fund::FundRequestState::Rejected,
                            observed_at_ms,
                        )?;
                    }
                    db::repo::actions::mark_reconciled(
                        c,
                        &action_id,
                        action.worker_epoch,
                        observed_at_ms,
                    )?;
                }
                _ => return Ok(false),
            }
            db::repo::send_journal_client::record_recovery_event(
                c,
                event.sequence,
                &event.logical_id,
                event.version,
                &event.payload,
                &event.hash,
            )?;
            db::repo::send_journal_client::delete_replayed_event(c, &event)?;
            db::repo::send_journal_client::mark_replay_pending_validation(c)?;
            Ok(true)
        })
        .map_err(map_db)?;
        if !replayed {
            break;
        }
    }
    Ok(())
}

/// Replay only a verified, contiguous prefix of core's staged V2 stream.
/// The replayed business state still requires external validation, so the
/// worker remains locked after these transactions.
fn replay_core_prefix() -> Result<(), ErrorCode> {
    for _ in 0..100 {
        let replayed = db::tx::update(|c| {
            let Some(event) = db::repo::send_journal_client::next_recovery_event(c)? else {
                return Ok(false);
            };
            let payload: RecoveryPayload = candid::decode_one(&event.payload)
                .map_err(|_| db::error::Error::Invariant("invalid staged recovery payload"))?;
            validate_staged_recovery_event(c, &event)?;
            match payload {
                RecoveryPayload::IdentityAccount {
                    user_id,
                    owner,
                    account_id,
                    address,
                } => {
                    if owner == Principal::anonymous() {
                        return Err(db::error::Error::Invariant("invalid staged account event"));
                    }
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged account"))?;
                    let address: [u8; 20] = address
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged address"))?;
                    let mut id_material = b"core_account_identity".to_vec();
                    id_material.extend_from_slice(&account_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged account id mismatch"));
                    }
                    // Mapping evidence cannot reactivate a stopped account.
                    match db::repo::accounts::identity(c, &account_id)? {
                        Some((existing_user, existing_address))
                            if existing_user == user_id && existing_address == address => {}
                        Some(_) => return Err(db::error::Error::Conflict),
                        None => db::repo::accounts::upsert(
                            c,
                            &account_id,
                            &user_id,
                            &address,
                            ic_cdk::api::time() / 1_000_000,
                        )?,
                    }
                }
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
                    let order_id: [u8; 32] = order_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged order"))?;
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged account"))?;
                    let cloid: [u8; 16] = cloid
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged cloid"))?;
                    let body_hash: [u8; 32] = body_hash
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged body hash"))?;
                    let request_id = request_id.as_ref();
                    if request_id.is_empty()
                        || request_id.len() > 64
                        || accepted_at_ms == 0
                        || accepted_at_ms > i64::MAX as u64
                        || (reduce_only && risk_micros != 0)
                        || (!reduce_only && (risk_micros == 0 || risk_micros > i64::MAX as u64))
                    {
                        return Err(db::error::Error::Invariant("bad staged order acceptance"));
                    }
                    let mut id_material = b"order_accepted".to_vec();
                    id_material.extend_from_slice(&user_id);
                    id_material.extend_from_slice(request_id);
                    let mut order_material = cloid.to_vec();
                    order_material.extend_from_slice(&user_id);
                    if hl_sign::keccak256(&id_material) != event.logical_id
                        || hl_sign::keccak256(&order_material) != order_id
                    {
                        return Err(db::error::Error::Invariant(
                            "staged order identity mismatch",
                        ));
                    }
                    let (account_user, _) = db::repo::accounts::identity(c, &account_id)?
                        .ok_or(db::error::Error::NotFound)?;
                    if account_user != user_id
                        || db::repo::core_requests::request_status(
                            c, &user_id, request_id, &body_hash,
                        )? != db::repo::core_requests::AcceptOutcome::Accepted
                        || db::repo::orders::order_by_request(c, &user_id, request_id)?.is_some()
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    if db::repo::core_requests::accept_request(
                        c,
                        &user_id,
                        request_id,
                        &body_hash,
                        accepted_at_ms,
                    )? != db::repo::core_requests::AcceptOutcome::Accepted
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    // The journal intentionally excludes the plaintext order
                    // body, so it cannot recreate a sendable order row. The
                    // request and risk hold are recovered while the worker
                    // remains locked for external order reconciliation.
                    if !reduce_only {
                        db::repo::orders::reserve_risk(
                            c,
                            &account_id,
                            request_id,
                            risk_micros,
                            accepted_at_ms,
                        )?;
                    }
                }
                RecoveryPayload::OrderStatusObserved {
                    order_id,
                    account_id,
                    hl_oid,
                    state,
                    evidence_digest,
                    observed_at_ms,
                } => {
                    let order_id: [u8; 32] = order_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged order"))?;
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged account"))?;
                    if evidence_digest.as_ref().len() != 32
                        || observed_at_ms == 0
                        || observed_at_ms > i64::MAX as u64
                        || !matches!(state.as_str(), "open" | "filled" | "cancelled" | "rejected")
                    {
                        return Err(db::error::Error::Invariant("bad staged order status"));
                    }
                    let mut id_material = b"order_status_observed".to_vec();
                    id_material.extend_from_slice(&order_id);
                    id_material.extend_from_slice(state.as_bytes());
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant(
                            "staged order status id mismatch",
                        ));
                    }
                    db::repo::orders::bind_observed_oid(c, &account_id, &order_id, hl_oid)?;
                    let Some((matched_id, prior_state)) =
                        db::repo::orders::order_status_target(c, &account_id, hl_oid)?
                    else {
                        return Err(db::error::Error::NotFound);
                    };
                    if matched_id != order_id
                        || prior_state == state
                        || matches!(prior_state.as_str(), "filled" | "cancelled" | "rejected")
                        || (state == "open"
                            && !matches!(prior_state.as_str(), "pending" | "unknown"))
                        || !db::repo::orders::apply_order_status(
                            c,
                            &account_id,
                            hl_oid,
                            &state,
                            observed_at_ms,
                        )?
                    {
                        return Err(db::error::Error::Conflict);
                    }
                }
                RecoveryPayload::FillObserved {
                    tid,
                    hl_oid,
                    user_id,
                    order_id,
                    account_id,
                    market,
                    quantity,
                    price,
                    fee,
                    filled_at_ms,
                } => {
                    let user_id: [u8; 32] = user_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
                    let order_id: [u8; 32] = order_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged order"))?;
                    let account_id: [u8; 32] = account_id
                        .as_ref()
                        .try_into()
                        .map_err(|_| db::error::Error::Invariant("bad staged account"))?;
                    if tid == 0
                        || tid > i64::MAX as u64
                        || hl_oid == 0
                        || hl_oid > i64::MAX as u64
                        || filled_at_ms == 0
                        || filled_at_ms > i64::MAX as u64
                        || !matches!(market.as_str(), "BTC" | "ETH")
                        || quantity.is_empty()
                        || price.is_empty()
                    {
                        return Err(db::error::Error::Invariant("bad staged fill"));
                    }
                    let mut id_material = b"fill_observed".to_vec();
                    id_material.extend_from_slice(&account_id);
                    id_material.extend_from_slice(&tid.to_be_bytes());
                    if hl_sign::keccak256(&id_material) != event.logical_id {
                        return Err(db::error::Error::Invariant("staged fill id mismatch"));
                    }
                    if db::repo::orders::pending_fill_order(
                        c,
                        &user_id,
                        &account_id,
                        tid,
                        hl_oid,
                        &market,
                    )? != Some(order_id)
                        || !db::repo::orders::ingest_fill(
                            c,
                            &user_id,
                            &account_id,
                            &db::repo::orders::NewFill {
                                tid,
                                hl_oid,
                                market: &market,
                                price: &price,
                                quantity: &quantity,
                                fee,
                                filled_at: filled_at_ms,
                            },
                        )?
                    {
                        return Err(db::error::Error::Conflict);
                    }
                }
                _ => return Ok(false),
            }
            db::repo::send_journal_client::record_recovery_event(
                c,
                event.sequence,
                &event.logical_id,
                event.version,
                &event.payload,
                &event.hash,
            )?;
            db::repo::send_journal_client::delete_replayed_event(c, &event)?;
            db::repo::send_journal_client::mark_replay_pending_validation(c)?;
            Ok(true)
        })
        .map_err(map_db)?;
        if !replayed {
            break;
        }
    }
    Ok(())
}

/// Stages at most 100 V2 events per authorized resume attempt. It checks the
/// independent worker hash chain before persisting each event and never unlocks.
async fn stage_missing_recovery(
    principal: Principal,
    remote: &JournalHead,
) -> Result<(), ErrorCode> {
    let (after, prior) =
        db::tx::query(db::repo::send_journal_client::recovery_stage_head).map_err(map_db)?;
    if after > remote.sequence
        || (after == remote.sequence && prior.as_slice() != remote.hash.as_ref())
    {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if after == remote.sequence {
        return Ok(());
    }
    let response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_recovery_events")
            .with_args(&(scoped_role()?.to_string(), after, 100u32))
            .await
    } else {
        Call::bounded_wait(principal, "recovery_events")
            .with_args(&(after, 100u32))
            .await
    }
    .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let records = response
        .candid::<Result<Vec<RecoveryRecord>, ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)??;
    if records.is_empty() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let mut sequence = after;
    let mut previous_hash = prior;
    for record in records {
        sequence = sequence
            .checked_add(1)
            .ok_or(ErrorCode::PolicyUnavailable)?;
        if record.sequence != sequence
            || sequence > remote.sequence
            || record.previous_hash.as_ref() != previous_hash.as_slice()
            || record.event.version != 1
        {
            return Err(ErrorCode::PolicyUnavailable);
        }
        let logical_id: [u8; 32] = record
            .event
            .logical_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let hash: [u8; 32] = record
            .hash
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let payload =
            candid::encode_one(&record.event.payload).map_err(|_| ErrorCode::PolicyUnavailable)?;
        if payload.len() > 4096 {
            return Err(ErrorCode::PolicyUnavailable);
        }
        let mut hasher = Sha256::new();
        hasher.update(b"private-perp/recovery-event/v1");
        hasher.update(previous_hash);
        hasher.update(sequence.to_be_bytes());
        hasher.update(journal_worker_bytes()?);
        hasher.update(logical_id);
        hasher.update(record.event.version.to_be_bytes());
        hasher.update(&payload);
        let computed: [u8; 32] = hasher.finalize().into();
        if computed != hash {
            return Err(ErrorCode::PolicyUnavailable);
        }
        db::tx::update(|c| {
            db::repo::send_journal_client::stage_recovery_event(
                c,
                sequence,
                &logical_id,
                record.event.version,
                &payload,
                &previous_hash,
                &hash,
            )
        })
        .map_err(map_db)?;
        previous_hash = hash;
    }
    if sequence == remote.sequence && previous_hash.as_slice() != remote.hash.as_ref() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    Ok(())
}

async fn stage_missing(principal: Principal, remote: &JournalHead) -> Result<(), ErrorCode> {
    let (after, prior) =
        db::tx::query(db::repo::send_journal_client::stage_head).map_err(map_db)?;
    if after >= remote.sequence {
        return Ok(());
    }
    let response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_records")
            .with_args(&(scoped_role()?.to_string(), after, 100u32))
            .await
    } else {
        Call::bounded_wait(principal, "records")
            .with_args(&(after, 100u32))
            .await
    }
    .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let records = response
        .candid::<Result<Vec<JournalRecord>, ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)??;
    if records.is_empty() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let mut sequence = after;
    let mut previous_hash = prior;
    for record in records {
        sequence = sequence
            .checked_add(1)
            .ok_or(ErrorCode::PolicyUnavailable)?;
        if record.sequence != sequence
            || sequence > remote.sequence
            || record.previous_hash.as_ref() != previous_hash.as_slice()
        {
            return Err(ErrorCode::PolicyUnavailable);
        }
        let request_id: [u8; 32] = record
            .intent
            .request_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let account_id: [u8; 32] = record
            .intent
            .account_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let digest: [u8; 32] = record
            .intent
            .digest
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let hash: [u8; 32] = record
            .hash
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
        let mut hasher = Sha256::new();
        hasher.update(b"private-perp/send-journal/v1");
        hasher.update(previous_hash);
        hasher.update(sequence.to_be_bytes());
        hasher.update(journal_worker_bytes()?);
        hasher.update(record.intent.kind.as_bytes());
        hasher.update(request_id);
        hasher.update(account_id);
        hasher.update(record.intent.nonce.to_be_bytes());
        hasher.update(digest);
        let computed: [u8; 32] = hasher.finalize().into();
        if computed != hash {
            return Err(ErrorCode::PolicyUnavailable);
        }
        db::tx::update(|c| {
            db::repo::send_journal_client::stage_record(
                c,
                sequence,
                &record.intent.kind,
                &request_id,
                &account_id,
                record.intent.nonce,
                &digest,
                &previous_hash,
                &hash,
            )
        })
        .map_err(map_db)?;
        previous_hash = hash;
    }
    if sequence == remote.sequence && previous_hash.as_slice() != remote.hash.as_ref() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    Ok(())
}

pub fn lock() -> Result<(), ErrorCode> {
    // An old DB without a configured journal is already fail-closed.
    if configured()?.is_none() {
        return Ok(());
    }
    db::tx::update(|c| db::repo::send_journal_client::set_locked(c, true)).map_err(map_db)
}

/// 毎送信前に独立canisterと照合する。受領証跡の連番に穴があれば停止する。
pub async fn ensure_ready(role: &str) -> Result<(), ErrorCode> {
    let principal = configured()?.ok_or(ErrorCode::PolicyUnavailable)?;
    if db::tx::query(db::repo::send_journal_client::locked).map_err(map_db)? {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if db::tx::query(db::repo::send_journal_client::has_staged).map_err(map_db)? {
        lock()?;
        return Err(ErrorCode::PolicyUnavailable);
    }
    if db::tx::query(db::repo::send_journal_client::has_recovery_staged).map_err(map_db)? {
        lock()?;
        return Err(ErrorCode::PolicyUnavailable);
    }
    if db::tx::query(|c| db::repo::send_journal_client::unresolved_without_receipt(c, role))
        .map_err(map_db)?
    {
        lock()?;
        return Err(ErrorCode::PolicyUnavailable);
    }
    let response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_head")
            .with_arg(role.to_string())
            .await
    } else {
        Call::bounded_wait(principal, "head").with_arg(()).await
    }
    .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let remote = response
        .candid::<Result<JournalHead, ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)??;
    // The business stream must match locally committed receipts. A remote
    // event without its local transaction is treated as an unapplied delta.
    let recovery_response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_recovery_head")
            .with_arg(role.to_string())
            .await
    } else {
        Call::bounded_wait(principal, "recovery_head")
            .with_arg(())
            .await
    }
    .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let recovery_remote = recovery_response
        .candid::<Result<JournalHead, ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)??;
    let (recovery_sequence, recovery_hash, recovery_contiguous) =
        db::tx::query(db::repo::send_journal_client::recovery_local_head).map_err(map_db)?;
    if !recovery_contiguous
        || recovery_sequence != recovery_remote.sequence
        || recovery_hash.as_slice() != recovery_remote.hash.as_ref()
    {
        lock()?;
        return Err(ErrorCode::PolicyUnavailable);
    }
    let (sequence, hash, contiguous) =
        db::tx::query(db::repo::send_journal_client::local_head).map_err(map_db)?;
    if !contiguous || sequence != remote.sequence || hash.as_slice() != remote.hash.as_ref() {
        lock()?;
        return Err(ErrorCode::PolicyUnavailable);
    }
    Ok(())
}

/// 応答喪失時は送信せず、独立ジャーナルとの差を照合するまで停止する。
pub async fn append(role: &str, intent: SendIntent) -> Result<JournalAck, ErrorCode> {
    let request_id: [u8; 32] = intent
        .request_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let writer_epoch = db::tx::update(|c| {
        db::repo::send_journal_client::claim_writer(c, &intent.kind, &request_id)
    })
    .map_err(|error| match error {
        db::error::Error::Conflict => ErrorCode::PolicyUnavailable,
        db::error::Error::WriterBusy => ErrorCode::JournalWriterBusy,
        other => map_db(other),
    })?;
    if let Err(error) = ensure_ready(role).await {
        db::tx::update(|c| {
            db::repo::send_journal_client::release_writer(
                c,
                writer_epoch,
                &intent.kind,
                &request_id,
            )
        })
        .map_err(map_db)?;
        return Err(error);
    }
    let principal = configured()?.ok_or(ErrorCode::PolicyUnavailable)?;
    let response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_append")
            .with_args(&(role.to_string(), intent.clone()))
            .await
    } else {
        Call::bounded_wait(principal, "append")
            .with_arg(intent.clone())
            .await
    };
    let decoded = response
        .map_err(|_| ErrorCode::PolicyUnavailable)
        .and_then(|response| {
            response
                .candid::<Result<JournalHead, ErrorCode>>()
                .map_err(|_| ErrorCode::PolicyUnavailable)
        });
    match decoded {
        Ok(Ok(head)) => Ok(JournalAck { head, writer_epoch }),
        Ok(Err(error)) => {
            db::tx::update(|c| {
                db::repo::send_journal_client::release_writer(
                    c,
                    writer_epoch,
                    &intent.kind,
                    &request_id,
                )
            })
            .map_err(map_db)?;
            Err(error)
        }
        Err(error) => {
            let recovered = if cfg!(feature = "embedded") {
                Call::bounded_wait(principal, "role_intent_record")
                    .with_args(&(
                        role.to_string(),
                        intent.kind.clone(),
                        intent.request_id.clone(),
                    ))
                    .await
            } else {
                Call::bounded_wait(principal, "intent_record")
                    .with_args(&(intent.kind.clone(), intent.request_id.clone()))
                    .await
            }
            .ok()
            .and_then(|response| {
                response
                    .candid::<Result<Option<JournalRecord>, ErrorCode>>()
                    .ok()
            })
            .and_then(Result::ok)
            .flatten();
            if let Some(record) = recovered
                && record.intent == intent
                && record.hash.len() == 32
            {
                return Ok(JournalAck {
                    head: JournalHead {
                        sequence: record.sequence,
                        hash: record.hash,
                    },
                    writer_epoch,
                });
            }
            lock()?;
            Err(error)
        }
    }
}

/// Appends a typed business event before its local mutation. The matching
/// receipt must be committed inside that mutation's DB transaction.
pub async fn append_recovery_event(
    role: &str,
    event: RecoveryEvent,
) -> Result<JournalAck, ErrorCode> {
    append_recovery_event_if(role, event, |_| Ok(true))
        .await?
        .ok_or(ErrorCode::PolicyUnavailable)
}

/// Run the final local precondition under the same durable writer fence that
/// owns the append. A false result performs no remote write.
pub async fn append_recovery_event_if<F>(
    role: &str,
    event: RecoveryEvent,
    preflight: F,
) -> Result<Option<JournalAck>, ErrorCode>
where
    F: Fn(&ic_sqlite_vfs::db::connection::Connection) -> Result<bool, db::error::Error>,
{
    // An already-completed identity registration needs no writer or remote
    // append. This also lets an existing user authenticate while sends are
    // intentionally locked for recovery.
    if !db::tx::query(|c| preflight(c)).map_err(map_db)? {
        return Ok(None);
    }
    let logical_id: [u8; 32] = event
        .logical_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let writer_epoch = db::tx::update(|c| {
        let epoch = db::repo::send_journal_client::claim_writer(c, "recovery_event", &logical_id)?;
        if !preflight(c)? {
            db::repo::send_journal_client::release_writer(c, epoch, "recovery_event", &logical_id)?;
            return Ok(None);
        }
        Ok(Some(epoch))
    })
    .map_err(|error| match error {
        db::error::Error::Conflict => ErrorCode::PolicyUnavailable,
        db::error::Error::WriterBusy => ErrorCode::JournalWriterBusy,
        other => map_db(other),
    })?;
    let Some(writer_epoch) = writer_epoch else {
        return Ok(None);
    };
    if let Err(error) = ensure_ready(role).await {
        db::tx::update(|c| {
            db::repo::send_journal_client::release_writer(
                c,
                writer_epoch,
                "recovery_event",
                &logical_id,
            )
        })
        .map_err(map_db)?;
        return Err(error);
    }
    let principal = configured()?.ok_or(ErrorCode::PolicyUnavailable)?;
    let response = if cfg!(feature = "embedded") {
        Call::bounded_wait(principal, "role_append_recovery_event")
            .with_args(&(scoped_role()?.to_string(), event.clone()))
            .await
    } else {
        Call::bounded_wait(principal, "append_recovery_event")
            .with_arg(event.clone())
            .await
    };
    let decoded = response
        .map_err(|_| ErrorCode::PolicyUnavailable)
        .and_then(|response| {
            response
                .candid::<Result<JournalHead, ErrorCode>>()
                .map_err(|_| ErrorCode::PolicyUnavailable)
        });
    match decoded {
        Ok(Ok(head)) => Ok(Some(JournalAck { head, writer_epoch })),
        Ok(Err(error)) => {
            db::tx::update(|c| {
                db::repo::send_journal_client::release_writer(
                    c,
                    writer_epoch,
                    "recovery_event",
                    &logical_id,
                )
            })
            .map_err(map_db)?;
            Err(error)
        }
        Err(error) => {
            // An append response may be lost after the independent journal
            // committed it. Resolve by the stable logical ID, never by
            // re-appending with a new ID or assuming no external effect.
            let recovered = if cfg!(feature = "embedded") {
                Call::bounded_wait(principal, "role_recovery_event")
                    .with_args(&(scoped_role()?.to_string(), event.logical_id.clone()))
                    .await
            } else {
                Call::bounded_wait(principal, "recovery_event")
                    .with_arg(event.logical_id.clone())
                    .await
            }
            .ok()
            .and_then(|response| {
                response
                    .candid::<Result<Option<RecoveryRecord>, ErrorCode>>()
                    .ok()
            })
            .and_then(Result::ok)
            .flatten();
            if let Some(record) = recovered
                && record.event == event
                && record.hash.len() == 32
            {
                return Ok(Some(JournalAck {
                    head: JournalHead {
                        sequence: record.sequence,
                        hash: record.hash,
                    },
                    writer_epoch,
                }));
            }
            lock()?;
            Err(error)
        }
    }
}

pub fn record_recovery_event(
    c: &mut UpdateConnection<'_>,
    event: &RecoveryEvent,
    ack: &JournalAck,
) -> Result<(), db::error::Error> {
    if db::repo::send_journal_client::locked(c)?
        || db::repo::send_journal_client::has_staged(c)?
        || db::repo::send_journal_client::has_recovery_staged(c)?
        || event.version != 1
    {
        return Err(db::error::Error::Conflict);
    }
    let logical_id: [u8; 32] = event
        .logical_id
        .as_ref()
        .try_into()
        .map_err(|_| db::error::Error::Invariant("bad recovery event id"))?;
    if !db::repo::send_journal_client::writer_matches(
        c,
        ack.writer_epoch,
        "recovery_event",
        &logical_id,
    )? {
        return Err(db::error::Error::Conflict);
    }
    let (local_sequence, previous_hash, contiguous) =
        db::repo::send_journal_client::recovery_local_head(c)?;
    if !contiguous || local_sequence.checked_add(1) != Some(ack.head.sequence) {
        return Err(db::error::Error::Conflict);
    }
    let payload = candid::encode_one(&event.payload)
        .map_err(|_| db::error::Error::Invariant("bad recovery event payload"))?;
    if payload.is_empty() || payload.len() > 4096 {
        return Err(db::error::Error::Invariant("bad recovery event length"));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"private-perp/recovery-event/v1");
    hasher.update(previous_hash);
    hasher.update(ack.head.sequence.to_be_bytes());
    hasher.update(
        journal_worker_bytes().map_err(|_| db::error::Error::Invariant("missing journal role"))?,
    );
    hasher.update(logical_id);
    hasher.update(event.version.to_be_bytes());
    hasher.update(&payload);
    let expected: [u8; 32] = hasher.finalize().into();
    if ack.head.hash.as_ref() != expected {
        return Err(db::error::Error::Conflict);
    }
    db::repo::send_journal_client::record_recovery_event(
        c,
        ack.head.sequence,
        &logical_id,
        event.version,
        &payload,
        &expected,
    )?;
    db::repo::send_journal_client::release_writer(
        c,
        ack.writer_epoch,
        "recovery_event",
        &logical_id,
    )
}

/// `dispatching`状態と同一DB transactionで保存する。
pub fn record(
    c: &mut UpdateConnection<'_>,
    intent: &SendIntent,
    ack: &JournalAck,
) -> Result<(), db::error::Error> {
    // resume/upgradeがawait中に送信停止へ移った場合、受領証跡を業務状態へ昇格させない。
    if db::repo::send_journal_client::locked(c)?
        || db::repo::send_journal_client::has_staged(c)?
        || db::repo::send_journal_client::has_recovery_staged(c)?
    {
        return Err(db::error::Error::Conflict);
    }
    let request_id: [u8; 32] = intent
        .request_id
        .as_ref()
        .try_into()
        .map_err(|_| db::error::Error::Invariant("bad journal request"))?;
    if !db::repo::send_journal_client::writer_matches(
        c,
        ack.writer_epoch,
        &intent.kind,
        &request_id,
    )? {
        return Err(db::error::Error::Conflict);
    }
    let account_id: [u8; 32] = intent
        .account_id
        .as_ref()
        .try_into()
        .map_err(|_| db::error::Error::Invariant("bad journal account"))?;
    let digest: [u8; 32] = intent
        .digest
        .as_ref()
        .try_into()
        .map_err(|_| db::error::Error::Invariant("bad journal digest"))?;
    let hash: [u8; 32] = ack
        .head
        .hash
        .as_ref()
        .try_into()
        .map_err(|_| db::error::Error::Invariant("bad journal ack"))?;
    let (local_sequence, previous_hash, contiguous) = db::repo::send_journal_client::local_head(c)?;
    if !contiguous || local_sequence.checked_add(1) != Some(ack.head.sequence) {
        return Err(db::error::Error::Conflict);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"private-perp/send-journal/v1");
    hasher.update(previous_hash);
    hasher.update(ack.head.sequence.to_be_bytes());
    hasher.update(
        journal_worker_bytes().map_err(|_| db::error::Error::Invariant("missing journal role"))?,
    );
    hasher.update(intent.kind.as_bytes());
    hasher.update(request_id);
    hasher.update(account_id);
    hasher.update(intent.nonce.to_be_bytes());
    hasher.update(digest);
    let expected: [u8; 32] = hasher.finalize().into();
    if expected != hash {
        return Err(db::error::Error::Conflict);
    }
    db::repo::send_journal_client::record(
        c,
        ack.head.sequence,
        &intent.kind,
        &request_id,
        &account_id,
        intent.nonce,
        &digest,
        &hash,
    )?;
    db::repo::send_journal_client::release_writer(c, ack.writer_epoch, &intent.kind, &request_id)
}

pub fn intent(
    kind: &str,
    request_id: &[u8; 32],
    account_id: &[u8; 32],
    nonce: u64,
    digest: &[u8; 32],
) -> SendIntent {
    SendIntent {
        kind: kind.into(),
        request_id: request_id.to_vec().into(),
        account_id: account_id.to_vec().into(),
        nonce,
        digest: digest.to_vec().into(),
    }
}

/// Apply a journaled result without posting the external transfer again.
pub fn apply_fund_transfer_result(
    c: &mut ic_sqlite_vfs::db::UpdateConnection<'_>,
    event: &RecoveryEvent,
) -> Result<(), db::error::Error> {
    match event.payload.clone() {
        RecoveryPayload::FundTransferResult {
            action_id,
            request_id,
            user_id,
            source_account_id,
            destination,
            kind,
            amount_micros,
            nonce,
            accepted,
            evidence_digest,
            observed_at_ms,
        } => {
            let action_id: [u8; 32] = action_id
                .as_ref()
                .try_into()
                .map_err(|_| db::error::Error::Invariant("bad staged action"))?;
            let user_id: [u8; 32] = user_id
                .as_ref()
                .try_into()
                .map_err(|_| db::error::Error::Invariant("bad staged user"))?;
            let source_account_id: [u8; 32] = source_account_id
                .as_ref()
                .try_into()
                .map_err(|_| db::error::Error::Invariant("bad staged source"))?;
            let request_id = request_id.as_ref();
            if request_id.is_empty()
                || request_id.len() > 64
                || evidence_digest.as_ref().len() != 32
                || amount_micros == 0
                || amount_micros > i64::MAX as u64
                || nonce == 0
                || observed_at_ms == 0
                || observed_at_ms > i64::MAX as u64
                || !matches!(kind.as_str(), "allocation" | "withdrawal")
            {
                return Err(db::error::Error::Invariant("bad staged transfer result"));
            }
            let mut id_material = b"fund_transfer_result".to_vec();
            id_material.extend_from_slice(&action_id);
            if hl_sign::keccak256(&id_material) != event.logical_id.as_ref() {
                return Err(db::error::Error::Invariant("staged transfer id mismatch"));
            }
            let action =
                db::repo::actions::action_row(c, &action_id)?.ok_or(db::error::Error::NotFound)?;
            let request = db::repo::funds::fund_request(c, &user_id, request_id)?
                .ok_or(db::error::Error::NotFound)?;
            let source =
                db::repo::ledger::custody_account(c, &user_id, api_types::AccountKind::Reserve)?
                    .ok_or(db::error::Error::NotFound)?;
            if action.user_id != user_id
                || action.client_request_id.as_deref() != Some(request_id)
                || action.kind != kind
                || action.nonce != nonce
                || !matches!(
                    action.dispatch_state,
                    api_types::fund::ActionState::Dispatching
                        | api_types::fund::ActionState::Unknown
                )
                || request.kind.as_str() != kind
                || !matches!(
                    request.state,
                    api_types::fund::FundRequestState::Reserved
                        | api_types::fund::FundRequestState::Unknown
                )
                || request.amount != amount_micros
                || source.account_id != source_account_id
                || source.state != "active"
                || !db::repo::send_journal_client::receipt_matches(
                    c,
                    &kind,
                    &action_id,
                    &source_account_id,
                    nonce,
                    &action.digest,
                )?
            {
                return Err(db::error::Error::Conflict);
            }
            match kind.as_str() {
                "allocation" => {
                    let trading_id = request.account_id.ok_or(db::error::Error::Conflict)?;
                    let trading = db::repo::ledger::custody_account(
                        c,
                        &user_id,
                        api_types::AccountKind::Trading,
                    )?
                    .ok_or(db::error::Error::NotFound)?;
                    if destination.as_ref() != trading_id.as_slice()
                        || trading.account_id != trading_id
                        || trading.state != "active"
                    {
                        return Err(db::error::Error::Conflict);
                    }
                    if accepted {
                        db::repo::ledger::allocation_start(
                            c,
                            &user_id,
                            amount_micros,
                            observed_at_ms,
                            request_id,
                        )?;
                        db::repo::funds::consume_reservation(c, &user_id, request_id)?;
                        db::repo::funds::set_request_state(
                            c,
                            &user_id,
                            request_id,
                            api_types::fund::FundRequestState::Executing,
                            observed_at_ms,
                        )?;
                    } else {
                        db::repo::funds::release_reservation(
                            c,
                            &user_id,
                            request_id,
                            observed_at_ms,
                        )?;
                        db::repo::funds::set_request_state(
                            c,
                            &user_id,
                            request_id,
                            api_types::fund::FundRequestState::Rejected,
                            observed_at_ms,
                        )?;
                    }
                }
                "withdrawal" => {
                    let text = request
                        .destination
                        .as_deref()
                        .ok_or(db::error::Error::Conflict)?;
                    let bytes = hex::decode(text.strip_prefix("0x").unwrap_or(text))
                        .map_err(|_| db::error::Error::Invariant("bad payout destination"))?;
                    if bytes.as_slice() != destination.as_ref() || bytes.len() != 20 {
                        return Err(db::error::Error::Conflict);
                    }
                    if accepted {
                        let mut evidence = b"payout".to_vec();
                        evidence.extend_from_slice(&action.digest);
                        let event_id = hl_sign::keccak256(&evidence);
                        db::repo::ledger::payout_settled(
                            c,
                            &user_id,
                            amount_micros,
                            observed_at_ms,
                            &event_id,
                        )?;
                        db::repo::funds::consume_reservation(c, &user_id, request_id)?;
                        db::repo::funds::set_request_state(
                            c,
                            &user_id,
                            request_id,
                            api_types::fund::FundRequestState::Settled,
                            observed_at_ms,
                        )?;
                    } else {
                        db::repo::funds::release_reservation(
                            c,
                            &user_id,
                            request_id,
                            observed_at_ms,
                        )?;
                        db::repo::ledger::withdrawal_release(
                            c,
                            &user_id,
                            amount_micros,
                            observed_at_ms,
                            request_id,
                        )?;
                        db::repo::funds::set_request_state(
                            c,
                            &user_id,
                            request_id,
                            api_types::fund::FundRequestState::Rejected,
                            observed_at_ms,
                        )?;
                    }
                }
                _ => return Err(db::error::Error::Invariant("bad transfer kind")),
            }
            if action.dispatch_state == api_types::fund::ActionState::Unknown {
                db::repo::actions::reconcile_unknown(
                    c,
                    &action_id,
                    action.worker_epoch,
                    observed_at_ms,
                )?;
            } else {
                db::repo::actions::mark_reconciled(
                    c,
                    &action_id,
                    action.worker_epoch,
                    observed_at_ms,
                )?;
            }
        }
        _ => return Err(db::error::Error::Invariant("not a transfer result")),
    }
    Ok(())
}

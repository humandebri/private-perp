//! `funds_vault` Canister。
//!
//! 責務はEOA認証、共通保管口座とユーザー別取引口座のmaster鍵管理、複式台帳、
//! 資金要求・予約・outbox、Agent承認・失効である
//! （`docs/phase-0/authority-matrix.md` 2節、`docs/phase-0/api-contract.md` 2節）。
//!
//! **任意のダイジェストへの署名APIを追加しない。** 資金移動は目的・金額・宛先・
//! 本人認可・残高を検証してから行う。
//!
//! S2の2B時点で実装しているのは認証（challenge・セッション）である。資金API・outboxの
//! 署名送信・HPKEは後続の段階で追加する。

mod amount;
mod auth;
mod balance;
mod builder_fee;
mod clock;
mod config;
mod crypto;
mod cycles;
mod deposits;
mod eligibility;
mod environment;
mod fund;
mod outbox;
mod private_api;
mod random;
mod recovery;
mod rest_budget;
mod venue;

/// HPKE封筒は共有クレートへ移設した（`trading_core`も同じ封筒を使う）。
use hpke_envelope as hpke;

use api_types::auth::{ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle};
use api_types::error::ErrorCode;
use candid::Principal;

const MEMORY_ID: u8 = db::memory_id::FUNDS_VAULT_MAIN;

thread_local! {
    static SWEEP_TIMER: std::cell::RefCell<Option<ic_cdk_timers::TimerId>> = const {
        std::cell::RefCell::new(None)
    };
}

/// このビルドのバージョン。デプロイ確認用。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// ログインchallengeを発行する。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn issue_challenge(request: ChallengeRequest) -> Result<ChallengeResponse, ErrorCode> {
    auth::issue_challenge(request, ic_cdk::api::canister_self()).await
}

/// challengeを消費してセッションを発行する。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn open_session(request: OpenSessionRequest) -> Result<SessionHandle, ErrorCode> {
    auth::open_session(request, ic_cdk::api::msg_caller()).await
}

/// セッションを失効させる。
fn revoke_session(session: SessionHandle) -> Result<(), ErrorCode> {
    auth::revoke_session(&session, ic_cdk::api::msg_caller())
}

/// HPKEの鍵世代を更新する（controllerのみ）。
///
/// 秘密鍵はcanister内のDBに留め、公開鍵のみを配布する（`Plan.md` 16.5）。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn rotate_hpke_key() -> Result<api_types::Blob, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can rotate the HPKE key".to_string(),
        });
    }
    let ikm = crate::random::random32().await?;
    let (secret, public) = hpke::derive_keypair(&ikm);
    let secret: [u8; 32] = secret.try_into().map_err(|_| ErrorCode::Internal {
        code: "unexpected secret length".to_string(),
    })?;
    let public: [u8; 32] = public.try_into().map_err(|_| ErrorCode::Internal {
        code: "unexpected public length".to_string(),
    })?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::hpke::insert_key(connection, &secret, &public, now))
        .map_err(|error| auth::map_db(error, None))?;
    Ok(public.to_vec().into())
}

/// 取引所の入金（ledger update）を記録する（controllerのみ）。
///
/// 正規化したイベントID（`keccak256("deposit" ‖ tx_hash)`）で**二重計上を防ぐ**。
/// 本番では非replicatedな`/info`照合がこの経路を呼ぶ。ユーザーへの紐付け（宛先アドレス→
/// 利用者）と`deposit_confirmed`の起票は次段階（アドレス写像の実装後）に行う。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn ingest_venue_deposit(
    tx_hash: api_types::Blob,
    amount: u64,
    asset: String,
) -> Result<bool, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can ingest venue deposits".to_string(),
        });
    }
    if tx_hash.as_ref().is_empty() {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "tx_hash must not be empty".to_string(),
        });
    }
    let mut input = b"deposit".to_vec();
    input.extend_from_slice(tx_hash.as_ref());
    let event_id = hl_sign::keccak256(&input);
    let now = clock::now_ms();
    let network = environment::network_name()?;
    db::tx::update(|connection| {
        let event = db::repo::events::ExternalEvent {
            event_id,
            network: network.clone(),
            account_address: [0u8; 20],
            counterparty: [0u8; 20],
            asset: asset.clone(),
            amount,
            kind: "deposit".to_string(),
            at: now,
            evidence_ref: Some(hex::encode(tx_hash.as_ref())),
        };
        db::repo::events::ingest_external_event(connection, &event, now)
    })
    .map_err(|error| auth::map_db(error, None))
}

/// テスト専用：queuedなactionのダイジェストを壊す（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn test_corrupt_action_digest() -> Result<u32, ErrorCode> {
    db::tx::update(|connection| db::repo::actions::overwrite_queued_digest(connection, &[0u8; 32]))
        .map(|changed| changed as u32)
        .map_err(|error| auth::map_db(error, None))
}

/// テスト専用：現行鍵で封筒を作る（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn test_hpke_seal(plaintext: api_types::Blob) -> Result<api_types::Blob, ErrorCode> {
    let public = db::tx::query(db::repo::hpke::active_public)
        .map_err(|error| auth::map_db(error, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let seed = crate::random::random32().await?;
    let aad = test_aad(0);
    hpke::seal(
        &public,
        b"private-perp/envelope/v1",
        &aad,
        plaintext.as_ref(),
        &seed,
    )
    .map(|envelope| envelope.into())
    .map_err(|error| ErrorCode::Internal { code: error })
}

/// テスト専用：封筒を開ける（`aad`の`expires_at`を変えると失敗する）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn test_hpke_open(
    envelope: api_types::Blob,
    expires_at: u64,
) -> Result<api_types::Blob, ErrorCode> {
    let secret = db::tx::query(db::repo::hpke::active_secret)
        .map_err(|error| auth::map_db(error, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let aad = test_aad(expires_at);
    hpke::open(
        &secret,
        b"private-perp/envelope/v1",
        &aad,
        envelope.as_ref(),
    )
    .map(|plaintext| plaintext.into())
    .map_err(|error| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: error,
    })
}

/// テスト用の`aad`（呼び出し元と期限を束縛する）。
#[cfg(feature = "test-venue")]
fn test_aad(expires_at: u64) -> Vec<u8> {
    let caller = ic_cdk::api::msg_caller();
    hpke::envelope_aad(
        "local",
        ic_cdk::api::canister_self().as_slice(),
        "test_hpke",
        caller.as_slice(),
        &[],
        expires_at,
    )
}

/// controllerのみ許可する（設定変更の共通チェック）。
fn require_controller(reason: &str) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if ic_cdk::api::is_controller(&caller) {
        Ok(())
    } else {
        Err(ErrorCode::Unauthenticated {
            reason: reason.to_string(),
        })
    }
}

/// 環境のnetworkを設定する（controllerのみ）。
///
/// mainnetはPhase 2では拒否する（`docs/phase-0/environments.md` E-2）。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_network(network: String) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the network")?;
    hl_types::environment::parse_network(&network).map_err(environment::map_environment)?;
    let now = clock::now_ms();
    db::tx::update(|connection| db::repo::vault_config::set_network(connection, &network, now))
        .map_err(|error| auth::map_db(error, None))
}

/// Hyperliquidのendpointを設定する（controllerのみ）。
///
/// 設定済みのnetworkと整合しないhost（例：testnet設定にmainnet endpoint）は拒否する。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_venue_endpoints(exchange_url: String, info_url: String) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the venue endpoints")?;
    let network: hl_types::Network = environment::network()?.into();
    hl_types::environment::validate_endpoints(network, &exchange_url, &info_url)
        .map_err(environment::map_environment)?;
    let now = clock::now_ms();
    db::tx::update(|connection| {
        db::repo::vault_config::set_venue_endpoints(connection, &exchange_url, &info_url, now)
    })
    .map_err(|error| auth::map_db(error, None))
}

/// 閾値ECDSAのkey IDを設定する（controllerのみ）。
///
/// testnetの鍵名はデプロイ後に実測して確定する（`docs/phase-0/environments.md` 2節）。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_ecdsa_key_id(key_id: String) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the ecdsa key id")?;
    hl_types::environment::validate_key_id(&key_id).map_err(environment::map_environment)?;
    let now = clock::now_ms();
    db::tx::update(|connection| db::repo::vault_config::set_ecdsa_key_id(connection, &key_id, now))
        .map_err(|error| auth::map_db(error, None))?;
    Ok(())
}

/// 共有REST予算のpolicy principal（controllerのみ）。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_policy_principal(policy: Principal) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the policy principal")?;
    let bytes = policy.as_slice();
    if policy == Principal::anonymous() || bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "invalid policy principal".to_string(),
        });
    }
    db::tx::update(|connection| {
        db::repo::vault_config::set_policy_principal(connection, bytes, clock::now_ms())
    })
    .map_err(|error| auth::map_db(error, None))
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_policy_principal() -> Option<Principal> {
    db::tx::query(db::repo::vault_config::policy_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_send_journal(principal: Principal) -> Result<(), ErrorCode> {
    require_controller("only a controller can configure the send journal")?;
    journal_client::configure(principal)
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_journal_guard(principal: Principal) -> Result<(), ErrorCode> {
    require_controller("only a controller can configure the journal guard")?;
    journal_client::set_guard(principal)
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn resume_journal() -> Result<(), ErrorCode> {
    journal_client::resume("vault").await
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_send_journal() -> Result<Option<Principal>, ErrorCode> {
    journal_client::configured()
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_journal_send_status() -> Result<(bool, bool), ErrorCode> {
    journal_client::public_status()
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_journal_guard() -> Result<Option<Principal>, ErrorCode> {
    require_controller("only a controller can inspect the journal guard")?;
    journal_client::guard()
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn journal_restore_status() -> Result<(u64, u64, bool), ErrorCode> {
    require_controller("only a controller can inspect journal restore")?;
    journal_client::status()
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn recovery_stage_status() -> Result<(u64, bool), ErrorCode> {
    require_controller("only a controller can inspect journal restore")?;
    journal_client::recovery_stage_status()
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn recovery_replay_pending() -> Result<bool, ErrorCode> {
    require_controller("only a controller can inspect journal restore")?;
    journal_client::replay_pending_validation()
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_core_principal(core: Principal) -> Result<(), ErrorCode> {
    require_controller("only a controller can set the core principal")?;
    let bytes = core.as_slice();
    if core == Principal::anonymous() || bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "invalid core principal".to_string(),
        });
    }
    db::tx::update(|connection| {
        let previous = db::repo::vault_config::core_principal(connection)?;
        if previous.is_some()
            && previous.as_deref() != Some(bytes)
            && db::repo::actions::active_recovery_exists(connection)?
        {
            return Err(db::error::Error::Conflict);
        }
        db::repo::vault_config::set_core_principal(connection, bytes, clock::now_ms())
    })
    .map_err(|error| auth::map_db(error, None))
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_core_principal() -> Option<Principal> {
    db::tx::query(db::repo::vault_config::core_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn set_recovery_history_verified(verified: bool) -> Result<(), ErrorCode> {
    require_controller("only a controller can confirm recovery history completeness")?;
    db::tx::update(|connection| {
        db::repo::vault_config::set_recovery_history_verified(connection, verified, clock::now_ms())
    })
    .map_err(|error| auth::map_db(error, None))
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_recovery_history_verified() -> Result<bool, ErrorCode> {
    db::tx::query(db::repo::vault_config::recovery_history_verified)
        .map_err(|error| auth::map_db(error, None))
}

/// 現在の環境設定（診断用・公開）。秘密は含まない。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_environment() -> Result<api_types::environment::EnvironmentView, ErrorCode> {
    environment::resolved()
}

/// 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_hpke_public_key() -> Result<api_types::Blob, ErrorCode> {
    let public =
        db::tx::query(db::repo::hpke::active_public).map_err(|error| auth::map_db(error, None))?;
    public
        .map(|public| public.into())
        .ok_or(ErrorCode::PolicyUnavailable)
}

/// 本人の取引口座アドレス（着金確認や照合に使う）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_trading_address(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] =
        status
            .user_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "user_id must be 32 bytes".to_string(),
            })?;
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &user_id, api_types::AccountKind::Trading)
    })
    .map_err(|error| auth::map_db(error, None))?;
    account
        .map(|account| account.master_address.to_vec().into())
        .ok_or(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        })
}

/// 本人の残高（`trading_core` がsnapshotを作るための参照）。
///
/// 戻り値は `(取引口座の残高, 出金可能額)`。認可のcaller束縛は呼び出し側（core）が
/// `session_status` で行う。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_balances(session: SessionHandle) -> Result<(u64, u64), ErrorCode> {
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] =
        status
            .user_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "user_id must be 32 bytes".to_string(),
            })?;
    let session_id: [u8; 32] =
        session
            .session_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "session_id must be 32 bytes".to_string(),
            })?;
    let verified = auth::VerifiedSession {
        user_id,
        session_id,
    };
    // 取引口座のequityは **口座ID** で導出する（`user_trading:<account_id>`）。
    // 利用者IDで引くと常に0になる。出金可能額と同じ導出（`user_balances`）を使う。
    let status = fund::fund_status_with_holds(&verified)?;
    Ok((status.trading_equity, status.withdrawable))
}

/// Only the authenticated core flow may request a venue equity observation.
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn refresh_trading_balance(session: SessionHandle) -> Result<(), ErrorCode> {
    if get_core_principal() != Some(ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only trading core may refresh balances".into(),
        });
    }
    let status = auth::session_status(&session)?;
    let user: [u8; 32] = status
        .user_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    balance::refresh(&user).await
}

/// 本人の取引口座ID（`trading_core` が所有権の確認に使う）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_trading_account(session: SessionHandle) -> Result<Option<api_types::Blob>, ErrorCode> {
    // canister間（trading_core）からの呼び出しを想定し、caller束縛は呼び出し側で行う
    // （coreは先に session_status で本人のprincipalを確認する）。
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] =
        status
            .user_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::Internal {
                code: "user_id must be 32 bytes".to_string(),
            })?;
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &user_id, api_types::AccountKind::Trading)
    })
    .map_err(|error| auth::map_db(error, None))?;
    Ok(account.map(|account| account.account_id.to_vec().into()))
}

/// 入金・配分前に本人の取引口座を確定する。秘密の口座対応はvault内に保持する。
async fn prepare_trading_account(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let account = outbox::ensure_trading_account(&verified.user_id, clock::now_ms()).await?;
    Ok(account.account_id.to_vec().into())
}

/// 渡されたAgentアドレスを世代へ承認する（master鍵で署名して送信し、結果を永続化する）。
///
/// `generation` は `trading_core` が採番した世代を渡す。承認はvaultの
/// `agent_generations` に保存され、取引所の応答が不明な場合は `active` にしない。
async fn approve_agent_generation(
    session: SessionHandle,
    generation: u64,
    agent_address: api_types::Blob,
) -> Result<api_types::fund::AgentGeneration, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let agent_address: [u8; 20] =
        agent_address
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::BadRequest {
                code: api_types::error::BadRequestCode::MalformedPayload,
                detail: "agent_address must be 20 bytes".to_string(),
            })?;
    fund::approve_agent_generation(&verified, generation, agent_address).await
}

/// 口座・世代の承認状態（`trading_core` が状態表示と署名可否の判断に使う）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_agent_approval(
    account_id: api_types::Blob,
    generation: u64,
) -> Result<Option<api_types::fund::AgentGeneration>, ErrorCode> {
    let account_id: [u8; 32] =
        account_id
            .as_ref()
            .try_into()
            .map_err(|_| ErrorCode::BadRequest {
                code: api_types::error::BadRequestCode::MalformedPayload,
                detail: "account_id must be 32 bytes".to_string(),
            })?;
    db::tx::query(|connection| db::repo::agents::generation(connection, &account_id, generation))
        .map_err(|error| auth::map_db(error, None))
}

/// セッションの有効性（canister間の検証経路。呼び出し元は返却されたprincipalを検証する）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn session_status(session: SessionHandle) -> Result<api_types::auth::SessionStatus, ErrorCode> {
    auth::session_status(&session)
}

/// 入金案内（認証済みセッションが必要）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_funding_instructions(
    session: SessionHandle,
) -> Result<api_types::fund::FundingInstructions, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    cycles::require_new()?;
    let account = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, &verified.user_id, api_types::AccountKind::Trading)
    })
    .map_err(|e| auth::map_db(e, None))?
    .ok_or(ErrorCode::PolicyUnavailable)?;
    eligibility::require(
        &verified.user_id,
        ic_cdk::api::msg_caller(),
        &account.account_id,
    )?;
    fund::funding_instructions(&verified)
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn configure_cycles(daily_floor: u128, exit_reserve: u128) -> Result<(), ErrorCode> {
    cycles::configure(daily_floor, exit_reserve)
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn get_cycles_status() -> Result<api_types::operations_status::CyclesStatus, ErrorCode> {
    cycles::status()
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn configure_eligibility(
    terms_version: u64,
    issuer_address: api_types::Blob,
    mock_issuer: bool,
) -> Result<(), ErrorCode> {
    eligibility::configure(terms_version, issuer_address, mock_issuer)
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_eligibility_configuration() -> Result<Option<(u64, api_types::Blob)>, ErrorCode> {
    db::tx::query(db::repo::eligibility::config)
        .map(|config| {
            config.map(|config| (config.terms_version, config.issuer_address.to_vec().into()))
        })
        .map_err(|e| auth::map_db(e, None))
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn eligibility_status(
    session: SessionHandle,
) -> Result<api_types::eligibility::EligibilityStatus, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let account = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, &verified.user_id, api_types::AccountKind::Trading)
    })
    .map_err(|e| auth::map_db(e, None))?
    .ok_or(ErrorCode::PolicyUnavailable)?;
    eligibility::status(
        &verified.user_id,
        ic_cdk::api::msg_caller(),
        &account.account_id,
    )
}

#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn builder_fee_mock_status(
    session: SessionHandle,
) -> Result<api_types::builder_fee::BuilderFeeMockStatus, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let account = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, &verified.user_id, api_types::AccountKind::Trading)
    })
    .map_err(|e| auth::map_db(e, None))?
    .ok_or(ErrorCode::PolicyUnavailable)?;
    builder_fee::status(
        &verified.user_id,
        ic_cdk::api::msg_caller(),
        &account.account_id,
    )
}

/// Core-only admission check. Core must separately bind the session principal to its caller.
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn check_eligibility_for_core(
    session: SessionHandle,
    account_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let configured_core = db::tx::query(db::repo::vault_config::core_principal)
        .map_err(|e| auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if ic_cdk::api::msg_caller().as_slice() != configured_core.as_slice() {
        return Err(ErrorCode::Unauthenticated {
            reason: "configured core required".into(),
        });
    }
    let status = auth::session_status(&session)?;
    let user_id: [u8; 32] = status
        .user_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let account_id: [u8; 32] = account_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    eligibility::require(&user_id, status.principal, &account_id)
}

#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn check_eligibility_account_for_core(
    user_id: api_types::Blob,
    account_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let configured_core = db::tx::query(db::repo::vault_config::core_principal)
        .map_err(|e| auth::map_db(e, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if ic_cdk::api::msg_caller().as_slice() != configured_core.as_slice() {
        return Err(ErrorCode::Unauthenticated {
            reason: "configured core required".into(),
        });
    }
    let user_id: [u8; 32] = user_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let account_id: [u8; 32] = account_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    eligibility::require_current(&user_id, &account_id)
}

/// 資金状態（認証済みセッションが必要）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn get_fund_status(session: SessionHandle) -> Result<api_types::fund::FundStatus, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::fund_status_with_holds(&verified)
}

/// 資金履歴（認証済みセッションが必要）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn list_fund_events(
    session: SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::fund_events(&verified, cursor, limit)
}

/// 配分を要求する（受付＋予約）。
async fn request_allocation(
    request: api_types::fund::AllocationRequest,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    fund::request_allocation(&verified, &request).await
}

/// 出金を要求する（本人署名の検証＋受付＋予約）。
async fn request_withdrawal(
    request: api_types::fund::WithdrawalRequest,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&request.session, ic_cdk::api::msg_caller())?;
    fund::request_withdrawal(&verified, &request).await
}

/// テスト専用のECDSA往復（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn test_ecdsa_roundtrip(
    seed: u32,
    digest: api_types::Blob,
) -> Result<(api_types::Blob, api_types::Blob), ErrorCode> {
    let path = crypto::derivation_path(&[b"test", &seed.to_be_bytes()]);
    let public_key = crypto::public_key(path.clone()).await?;
    let digest: [u8; 32] = digest
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "digest must be 32 bytes".to_string(),
        })?;
    let signature = crypto::sign_with_key(&digest, path, &public_key).await?;
    Ok((
        public_key.to_vec().into(),
        signature.to_bytes65().to_vec().into(),
    ))
}

/// テスト専用の入金計上（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn test_credit_deposit(
    session: SessionHandle,
    amount: u64,
    event_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let event_id: [u8; 32] = event_id
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "event_id must be 32 bytes".to_string(),
        })?;
    fund::test_credit_deposit(&verified, amount, &event_id)
}

/// Transactions wake the worker; the serial timer exists only while durable
/// work remains. Keeping the timer until completion also recovers callback traps.
#[cfg(not(feature = "test-venue"))]
fn schedule_sweep() {
    if SWEEP_TIMER.with(|timer| timer.borrow().is_some()) {
        return;
    }
    // Status sampling and session/envelope writes must not wake an idle worker.
    // This callback runs after the transaction releases its connection, so the
    // committed work predicate is safe to read. On storage errors keep retrying.
    if !db::tx::query(db::repo::actions::has_unfinished_work).unwrap_or(true) {
        return;
    }
    let timer_id = ic_cdk_timers::set_timer_interval_serial(
        core::time::Duration::from_millis(outbox::SWEEP_INTERVAL_MS),
        async || {
            db::tx::with_optional_scope_future(
                cfg!(feature = "embedded").then_some(db::DbScope::Vault),
                async {
                    if !stop_idle_worker() {
                        let now = ic_cdk::api::time() / 1_000_000;
                        if let Err(error) = outbox::sweep(now).await {
                            ic_cdk::println!("funds-vault sweep failed: {error:?}");
                        }
                        stop_idle_worker();
                    }
                },
            )
            .await
        },
    );
    SWEEP_TIMER.with(|timer| *timer.borrow_mut() = Some(timer_id));
}

#[cfg(not(feature = "test-venue"))]
fn stop_idle_worker() -> bool {
    // A storage failure must never silently abandon pending work.
    if db::tx::query(db::repo::actions::has_unfinished_work).unwrap_or(true) {
        return false;
    }
    // CDK 1.0 serial callbacks restore their slot after returning. Clearing that
    // slot inside the callback traps, so cancel in a separate message and
    // recheck durable work to avoid losing a concurrent request's wake-up.
    if SWEEP_TIMER.with(|timer| timer.borrow().is_some()) {
        ic_cdk_timers::set_timer(core::time::Duration::ZERO, async {
            db::tx::with_optional_scope_future(
                cfg!(feature = "embedded").then_some(db::DbScope::Vault),
                async {
                    if !db::tx::query(db::repo::actions::has_unfinished_work).unwrap_or(true)
                        && let Some(timer) = SWEEP_TIMER.with(|timer| timer.borrow_mut().take())
                    {
                        ic_cdk_timers::clear_timer(timer);
                    }
                },
            )
            .await;
        });
    }
    true
}

#[cfg(not(feature = "test-venue"))]
fn init_worker() {
    db::tx::on_update(schedule_sweep);
    if !stop_idle_worker() {
        schedule_sweep();
    }
}

/// テスト専用のsweep（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn test_sweep_now() -> Result<u32, ErrorCode> {
    outbox::sweep(clock::now_ms()).await
}

/// 呼び出し元のPrincipal（診断用。認可の判断は各メソッド内で行う）。
#[scoped_entrypoint::query(scope = Vault, prefix = "vault_")]
fn caller_principal() -> Principal {
    ic_cdk::api::msg_caller()
}

/// ローカル試験専用の入金注入。実運用はHL履歴の照合を経由する。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn credit_venue_deposit(
    tx_hash: api_types::Blob,
    amount: u64,
    address: api_types::Blob,
    asset: String,
    sender: Option<api_types::Blob>,
) -> Result<bool, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can credit venue deposits".to_string(),
        });
    }
    if tx_hash.as_ref().is_empty() {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "tx_hash must not be empty".to_string(),
        });
    }
    let address: [u8; 20] = address
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "address must be 20 bytes".to_string(),
        })?;
    let sender: Option<[u8; 20]> = sender
        .map(|value| {
            value
                .as_ref()
                .try_into()
                .map_err(|_| ErrorCode::PolicyUnavailable)
        })
        .transpose()?;
    let now = clock::now_ms();
    let network = environment::network_name()?;
    if asset != "usdc" {
        return Err(ErrorCode::PolicyUnavailable);
    }
    db::tx::update(|connection| {
        deposits::credit(
            connection,
            &network,
            tx_hash.as_ref(),
            amount,
            &address,
            "usdc",
            now,
            sender.as_ref(),
        )
    })
    .map_err(|error| auth::map_db(error, None))
}

/// 入金先（準備口座）を用意する。`get_funding_instructions` の前提を作る。
async fn provision_reserve_account(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let now = clock::now_ms();
    let address = outbox::provision_reserve_account(&verified.user_id, now).await?;
    Ok(address.to_vec().into())
}

/// Authenticated, bounded confirmation of the shared reserve. Ownership and
/// deduplication still come exclusively from the venue's transfer evidence.
async fn confirm_deposit(session: SessionHandle) -> Result<(), ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    let instructions = fund::funding_instructions(&verified)?;
    let address: [u8; 20] = instructions
        .hl_account_address
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    deposits::reconcile_address(&address).await?;
    Ok(())
}

/// 取引所の入金を取得して取り込む（controllerのみ）。
///
/// 取得はHTTP v2の非replicated outcall（変換関数で検証・正規化）、取り込みは検証済みの`deposits::credit`。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn reconcile_deposits(address: api_types::Blob) -> Result<u32, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can reconcile deposits".to_string(),
        });
    }
    let address: [u8; 20] = address
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "address must be 20 bytes".to_string(),
        })?;
    deposits::reconcile_address(&address).await
}

/// 不明なactionを「未実行」として解消する（controllerのみ）。
///
/// 未確認の外部送信を自由文の申告だけで「未実行」と確定してはならない。
/// 取引所履歴との照合による証明経路ができるまで手動解消を停止する。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
fn resolve_unknown_action(
    _action_id: api_types::Blob,
    _executed: bool,
    _evidence: String,
) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can resolve unknown actions".to_string(),
        });
    }
    Err(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::OperationNotAvailable,
    })
}

/// 回収（trading口座→準備口座）を要求する。
async fn request_recovery(
    session: SessionHandle,
    client_request_id: api_types::Blob,
    amount: u64,
) -> Result<api_types::fund::FundRequestAccepted, ErrorCode> {
    let verified = auth::verify_session(&session, ic_cdk::api::msg_caller())?;
    fund::request_recovery(&verified, &session, client_request_id.as_ref(), amount).await
}

/// 個人向け書込みの唯一の公開入口。エラーを含む業務結果も暗号化して返す。
#[scoped_entrypoint::update(scope = Vault, prefix = "vault_")]
async fn private_call(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    use api_types::error::BadRequestCode;
    if !matches!(
        envelope.method.as_str(),
        "revoke_session"
            | "approve_agent_generation"
            | "request_allocation"
            | "request_withdrawal"
            | "confirm_deposit"
            | "get_manual_work"
            | "resume_manual_work"
            | "provision_reserve_account"
            | "prepare_trading_account"
            | "eligibility_signing_claims"
            | "register_eligibility"
            | "builder_fee_signing_claims"
            | "register_builder_fee_mock_consent"
            | "request_recovery"
    ) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "unknown private method".into(),
        });
    }
    let (plaintext, request_id, caller) = private_api::open(&envelope).await?;
    let bad_payload = || ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: "cannot decode private payload".into(),
    };
    match envelope.method.as_str() {
        "get_manual_work" => {
            let session =
                candid::decode_one::<SessionHandle>(&plaintext).map_err(|_| bad_payload())?;
            let result = auth::verify_session(&session, caller).and_then(|verified| {
                db::tx::query(|c| db::worker_permissions::list(c, &verified.user_id, true))
                    .map_err(|e| auth::map_db(e, None))
            });
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "resume_manual_work" => {
            let (session, kind, id, generation) =
                candid::decode_args::<(SessionHandle, String, api_types::Blob, u64)>(&plaintext)
                    .map_err(|_| bad_payload())?;
            let result = (|| {
                let verified = auth::verify_session(&session, caller)?;
                let id: [u8; 32] = id.as_ref().try_into().map_err(|_| bad_payload())?;
                if !matches!(kind.as_str(), "fund" | "result" | "release") {
                    return Err(bad_payload());
                }
                // Only execution permission changes; unknown/dispatching is never queued again.
                db::worker_permissions::resume(
                    &kind,
                    &id,
                    &verified.user_id,
                    generation,
                    clock::now_ms(),
                )
                .map_err(|error| match error {
                    db::worker_permissions::ResumeError::Database(error) => {
                        auth::map_db(error, None)
                    }
                    db::worker_permissions::ResumeError::Blocked(reason) => ErrorCode::BadRequest {
                        code: BadRequestCode::MalformedPayload,
                        detail: reason.into(),
                    },
                })?;
                if kind == "fund" {
                    // A lost callback may leave the existing send in dispatching.
                    // Forward it to unknown for observation only, never queued.
                    let action = db::tx::query(|c| db::repo::actions::action_row(c, &id))
                        .map_err(|e| auth::map_db(e, None))?
                        .ok_or_else(bad_payload)?;
                    if action.user_id != verified.user_id {
                        return Err(bad_payload());
                    }
                    if action.dispatch_state == api_types::fund::ActionState::Dispatching {
                        let request_id = action
                            .client_request_id
                            .as_deref()
                            .ok_or_else(bad_payload)?;
                        outbox::mark_post_unknown(&action, request_id, clock::now_ms())?;
                    }
                }
                Ok(())
            })();
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "revoke_session" => {
            let session =
                candid::decode_one::<SessionHandle>(&plaintext).map_err(|_| bad_payload())?;
            let result = revoke_session(session);
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "approve_agent_generation" => {
            let (session, generation, address) =
                candid::decode_args::<(SessionHandle, u64, api_types::Blob)>(&plaintext)
                    .map_err(|_| bad_payload())?;
            let result = approve_agent_generation(session, generation, address).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "request_allocation" => {
            let request = candid::decode_one::<api_types::fund::AllocationRequest>(&plaintext)
                .map_err(|_| bad_payload())?;
            let result = request_allocation(request).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "request_withdrawal" => {
            let request = candid::decode_one::<api_types::fund::WithdrawalRequest>(&plaintext)
                .map_err(|_| bad_payload())?;
            let result = request_withdrawal(request).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "confirm_deposit" => {
            let session =
                candid::decode_one::<SessionHandle>(&plaintext).map_err(|_| bad_payload())?;
            let result = confirm_deposit(session).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "provision_reserve_account" => {
            let session =
                candid::decode_one::<SessionHandle>(&plaintext).map_err(|_| bad_payload())?;
            let result = provision_reserve_account(session).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "prepare_trading_account" => {
            let session =
                candid::decode_one::<SessionHandle>(&plaintext).map_err(|_| bad_payload())?;
            let result = prepare_trading_account(session).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "eligibility_signing_claims" => {
            let (session, expires_at) = candid::decode_args::<(SessionHandle, u64)>(&plaintext)
                .map_err(|_| bad_payload())?;
            let result = eligibility::signing_claims(&session, expires_at).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "register_eligibility" => {
            let (session, token) = candid::decode_args::<(
                SessionHandle,
                api_types::eligibility::EligibilityToken,
            )>(&plaintext)
            .map_err(|_| bad_payload())?;
            let result = eligibility::register(&session, token);
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "builder_fee_signing_claims" => {
            let (session, builder_address, expires_at) =
                candid::decode_args::<(SessionHandle, api_types::Blob, u64)>(&plaintext)
                    .map_err(|_| bad_payload())?;
            let result = builder_fee::signing_claims(&session, builder_address, expires_at).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "register_builder_fee_mock_consent" => {
            let (session, consent) = candid::decode_args::<(
                SessionHandle,
                api_types::builder_fee::BuilderFeeConsent,
            )>(&plaintext)
            .map_err(|_| bad_payload())?;
            let result = builder_fee::register(&session, consent);
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        "request_recovery" => {
            let (session, request_id_inner, amount) =
                candid::decode_args::<(SessionHandle, api_types::Blob, u64)>(&plaintext)
                    .map_err(|_| bad_payload())?;
            let result = request_recovery(session, request_id_inner, amount).await;
            private_api::seal(&envelope, &request_id, caller, &result).await
        }
        _ => unreachable!(),
    }
}

fn init_db() {
    if let Err(error) = if cfg!(feature = "embedded") {
        db::init_scoped(db::DbScope::Vault, db::schema::vault::MIGRATIONS)
    } else {
        db::init(MEMORY_ID, db::schema::vault::MIGRATIONS)
    } {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::init)]
fn init() {
    init_db();
    #[cfg(not(feature = "test-venue"))]
    init_worker();
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::post_upgrade)]
fn post_upgrade() {
    init_db();
    if let Err(error) = journal_client::lock() {
        ic_cdk::trap(format!("send journal lock failed: {error:?}"));
    }
    // グローバルtimerはアップグレードで失われるため予約し直す。
    #[cfg(not(feature = "test-venue"))]
    init_worker();
}

#[cfg(feature = "embedded")]
pub fn embedded_init() {
    db::tx::with_scope(db::DbScope::Vault, init);
}

#[cfg(feature = "embedded")]
pub fn embedded_post_upgrade() {
    db::tx::with_scope(db::DbScope::Vault, post_upgrade);
}

#[cfg(not(feature = "embedded"))]
ic_cdk::export_candid!();

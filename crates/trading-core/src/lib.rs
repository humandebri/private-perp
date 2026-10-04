//! `trading_core` Canister。
//!
//! 責務は注文・取消・決済の認可と状態機械、口座別・世代別Agent鍵、署名actionの構築と
//! 送信、Hyperliquid照合、口座snapshot配信である
//! （`docs/phase-0/api-contract.md` 3節、`docs/phase-0/state-machines.md`）。
//!
//! このCanisterはmaster鍵と出金署名を持たない。`funds_vault` へ出金や任意digest署名を
//! 要求する経路を追加しない（`docs/phase-0/authority-matrix.md` 5節）。
//!
//! 認可は `funds_vault` の `session_status` に問い合わせ、返却されたprincipalをこの
//! Canisterが受け取ったcallerと比較する（vaultはcoreのcallerを知らないため）。

mod close_price;
mod cycles;
mod environment;
mod market;
mod pipeline;
mod recovery;
mod rest_budget;
mod venue;

use api_types::auth::{SessionHandle, SessionStatus};
use api_types::error::{BadRequestCode, ErrorCode};
use candid::Principal;
use db::error::Error as DbError;
use ic_cdk::call::Call;
use ic_cdk_management_canister::{EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs, ecdsa_public_key};

const MEMORY_ID: u8 = db::memory_id::TRADING_CORE_MAIN;

thread_local! {
    static SWEEP_TIMER: std::cell::RefCell<Option<ic_cdk_timers::TimerId>> = const {
        std::cell::RefCell::new(None)
    };
}

/// 取引所データを「古い」とみなす閾値（ミリ秒）。
const STALE_DATA_MS: u64 = 10_000;

/// 1口座あたりの未終端注文（pending・open・partially_filled・unknown）の上限。
const MAX_PENDING_ORDERS: u64 = 50;

/// 決済（反対売買のIOC指値）で価格を導出するときのスリッページ許容幅（bps）。
const DEFAULT_SLIPPAGE_BPS: u32 = 50;

/// HPKE封筒の用途分離ラベル（vaultと同じ封筒実装を使う）。
const ENVELOPE_INFO: &[u8] = b"private-perp/envelope/v1";

/// 封筒の受付期限の上限（`now`から先の許容幅）。時計のずれと使い回しを抑える。
const MAX_ENVELOPE_TTL_MS: u64 = 300_000;

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

fn map_db(error: DbError) -> ErrorCode {
    match error {
        DbError::Sql(message) => internal(message),
        DbError::NotFound => internal("not configured".to_string()),
        DbError::Conflict => ErrorCode::ReservationConflict,
        DbError::WriterBusy => ErrorCode::JournalWriterBusy,
        DbError::Overflow => internal("integer overflow".to_string()),
        DbError::Invariant(message) => internal(message.to_string()),
        DbError::InsufficientFunds { .. } => internal("unexpected funds error".to_string()),
        DbError::RiskLimitExceeded { limit } => ErrorCode::RiskLimitExceeded { limit },
        DbError::StateConflict { .. } => ErrorCode::ReservationConflict,
    }
}

/// このビルドのバージョン。デプロイ確認用。
#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// vaultのprincipalを設定する（controllerのみ）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_vault_principal(vault: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the vault principal".to_string(),
        });
    }
    let bytes = vault.as_slice().to_vec();
    if vault == Principal::anonymous() || bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid vault principal".to_string(),
        });
    }
    db::tx::update(|connection| {
        let previous = db::repo::core_config::vault_principal(connection)?;
        if previous.is_some()
            && previous.as_deref() != Some(bytes.as_slice())
            && (db::repo::recovery_fences::migration_locked(connection)?
                || db::repo::recovery_fences::any_active(connection)?)
        {
            return Err(db::error::Error::Conflict);
        }
        db::repo::core_config::set_vault_principal(connection, &bytes)
    })
    .map_err(map_db)
}

/// 政策Canisterのprincipalを設定する（controllerのみ）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_policy_principal(policy: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the policy principal".to_string(),
        });
    }
    let bytes = policy.as_slice().to_vec();
    if bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid policy principal".to_string(),
        });
    }
    db::tx::update(|connection| db::repo::core_config::set_policy_principal(connection, &bytes))
        .map_err(map_db)
}

/// 政策Canisterのprincipal（診断用）。
#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_policy_principal() -> Option<Principal> {
    db::tx::query(db::repo::core_config::policy_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// 政策が停止中なら拒否する。**未設定・照会失敗はfail-closed**で拒否する
/// （`authority-matrix.md`: 読み取り失敗時は新規受付・新規リスク増加を停止する）。
async fn require_not_stopped() -> Result<(), ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::policy_principal).map_err(map_db)?;
    let Some(bytes) = bytes else {
        return Err(ErrorCode::PolicyUnavailable);
    };
    let policy = Principal::from_slice(&bytes);
    let response = Call::bounded_wait(policy, "get_stop_status")
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("policy get_stop_status: {error}"),
        })?;
    // `get_stop_status` は`Result`ではなく`StopStatus`を返す。
    let status: api_types::policy::StopStatus = response
        .candid()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    if status.stopped {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    Ok(())
}

/// 取引可能な銘柄のallowlist（policy_registry）。未設定・照会失敗はfail-closed。
async fn policy_markets() -> Result<Vec<String>, ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::policy_principal).map_err(map_db)?;
    let Some(bytes) = bytes else {
        return Err(ErrorCode::PolicyUnavailable);
    };
    let policy = Principal::from_slice(&bytes);
    let response = Call::bounded_wait(policy, "get_policy")
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("policy get_policy: {error}"),
        })?;
    let policy: Result<api_types::policy::Policy, ErrorCode> = response
        .candid()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    Ok(policy?.markets)
}

/// vaultのprincipal（診断用）。
#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_vault_principal() -> Option<Principal> {
    db::tx::query(db::repo::core_config::vault_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn prepare_recovery(
    request: api_types::recovery::PrepareRecovery,
) -> Result<api_types::recovery::RecoveryFenceToken, ErrorCode> {
    recovery::prepare(request).await
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn commit_recovery(token: api_types::recovery::RecoveryFenceToken) -> Result<(), ErrorCode> {
    recovery::commit(token).await
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn mark_recovery_unknown(token: api_types::recovery::RecoveryFenceToken) -> Result<(), ErrorCode> {
    recovery::mark_unknown(token)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn abort_recovery(token: api_types::recovery::RecoveryFenceToken) -> Result<(), ErrorCode> {
    recovery::abort(token)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn finish_recovery(token: api_types::recovery::RecoveryFenceToken) -> Result<(), ErrorCode> {
    recovery::finish(token)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn migrate_recovery(
    request: api_types::recovery::PrepareRecovery,
) -> Result<api_types::recovery::RecoveryFenceToken, ErrorCode> {
    recovery::migrate(request)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn finish_recovery_migration() -> Result<(), ErrorCode> {
    recovery::finish_migration()
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn begin_recovery_migration() -> Result<(), ErrorCode> {
    recovery::begin_migration()
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn recovery_migration_locked() -> Result<bool, ErrorCode> {
    db::tx::query(db::repo::recovery_fences::migration_locked).map_err(map_db)
}

/// HPKEの鍵世代を更新する（controllerのみ）。
///
/// 秘密鍵はcanister内のDBに留め、公開鍵のみを配布する（`Plan.md` 16.5、
/// `docs/phase-0/api-contract.md` 6節）。更新すると以前の世代は退役し、
/// 旧鍵で作られた封筒は復号できない（クライアントは公開鍵を取得し直す）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn rotate_hpke_key() -> Result<api_types::Blob, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can rotate the HPKE key".to_string(),
        });
    }
    let ikm = raw_rand32().await?;
    let (secret, public) = hpke_envelope::derive_keypair(&ikm);
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| internal("unexpected secret length".to_string()))?;
    let public: [u8; 32] = public
        .try_into()
        .map_err(|_| internal("unexpected public length".to_string()))?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::hpke::insert_key(connection, &secret, &public, now))
        .map_err(map_db)?;
    Ok(public.to_vec().into())
}

/// 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_hpke_public_key() -> Result<api_types::Blob, ErrorCode> {
    db::tx::query(db::repo::hpke::active_public)
        .map_err(map_db)?
        .map(|public| public.into())
        .ok_or(ErrorCode::PolicyUnavailable)
}

/// 封筒を開いて平文を取り出し、`request_id`を消費する（`api-contract.md` 6節）。
///
/// 束縛する値：鍵ID（現行世代）・`network`・`canister`・`method`・`caller`・
/// `request_id`・期限。`aad`は再計算した値と比較し、復号にも同じ値を使うため
/// 改竄は復号失敗になる。`request_id`は復号に成功した要求だけ消費する。
async fn open_envelope<T: serde::de::DeserializeOwned + candid::CandidType>(
    envelope: &api_types::envelope::HpkeRequest,
    method: &str,
) -> Result<(T, [u8; 32], Principal), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let now = ic_cdk::api::time() / 1_000_000;
    let malformed = |detail: &str| bad(BadRequestCode::MalformedPayload, detail);

    let public = db::tx::query(db::repo::hpke::active_public)
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if envelope.key_id.as_ref() != public.as_slice() {
        return Err(malformed("unknown hpke key id"));
    }
    if envelope.canister != ic_cdk::api::canister_self() {
        return Err(malformed("envelope canister mismatch"));
    }
    if envelope.method != method {
        return Err(malformed("envelope method mismatch"));
    }
    let network = network_name()?;
    if !network_matches(envelope.network, &network) {
        return Err(malformed("envelope network mismatch"));
    }
    if envelope.expires_at < now {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::ExpiredIntent,
            detail: "the envelope has expired".to_string(),
        });
    }
    if envelope.expires_at > now.saturating_add(MAX_ENVELOPE_TTL_MS) {
        return Err(malformed("the envelope expiry is too far ahead"));
    }
    let request_id: [u8; 32] = envelope
        .request_id
        .as_ref()
        .try_into()
        .map_err(|_| malformed("request_id must be 32 bytes"))?;
    let aad = envelope_aad(method, caller, &request_id, envelope.expires_at, &network)?;
    if envelope.aad.as_ref() != aad.as_slice() {
        return Err(malformed("envelope aad mismatch"));
    }
    let secret = db::tx::query(db::repo::hpke::active_secret)
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let plaintext = hpke_envelope::open(&secret, ENVELOPE_INFO, &aad, envelope.ciphertext.as_ref())
        .map_err(|_| malformed("cannot open the envelope"))?;
    // 復号できた要求だけを単回使用として記録する（改竄された要求でIDを消費しない）。
    let consumed = db::tx::update(|connection| {
        db::repo::hpke_requests::consume(
            connection,
            &request_id,
            method,
            caller.as_slice(),
            now,
            envelope.expires_at,
        )
    })
    .map_err(map_db)?;
    if !consumed {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::NonceReused,
            detail: "request_id was already used".to_string(),
        });
    }
    let payload =
        candid::decode_one(&plaintext).map_err(|_| malformed("cannot decode the payload"))?;
    Ok((payload, request_id, caller))
}

/// 応答を封筒へ入れる（`client_public_key`宛。`aad`は要求と同じ束縛）。
async fn seal_envelope<T: candid::CandidType>(
    envelope: &api_types::envelope::HpkeRequest,
    method: &str,
    request_id: &[u8; 32],
    caller: Principal,
    response: &T,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let network = network_name()?;
    let plaintext = candid::encode_one(response).map_err(|error| internal(error.to_string()))?;
    let seed = raw_rand32().await?;
    let aad = envelope_aad(method, caller, request_id, envelope.expires_at, &network)?;
    let ciphertext = hpke_envelope::seal(
        envelope.client_public_key.as_ref(),
        ENVELOPE_INFO,
        &aad,
        &plaintext,
        &seed,
    )
    .map_err(|_| bad(BadRequestCode::MalformedPayload, "cannot seal the response"))?;
    Ok(api_types::envelope::HpkeResponse {
        request_id: request_id.to_vec().into(),
        key_id: envelope.key_id.clone(),
        observed_at: ic_cdk::api::time() / 1_000_000,
        ciphertext: ciphertext.into(),
    })
}

/// 要求・応答の`aad`（設定されたnetworkと自分のprincipalを束縛する）。
fn envelope_aad(
    method: &str,
    caller: Principal,
    request_id: &[u8; 32],
    expires_at: u64,
    network: &str,
) -> Result<Vec<u8>, ErrorCode> {
    let canister = ic_cdk::api::canister_self();
    let canister = canister.as_slice();
    Ok(hpke_envelope::envelope_aad(
        network,
        canister,
        method,
        caller.as_slice(),
        request_id,
        expires_at,
    ))
}

/// 設定されたnetwork名（封筒の束縛に使う）。既定はlocal。
fn network_name() -> Result<String, ErrorCode> {
    environment::network_name()
}

/// 封筒の`Network`と設定値の対応。
fn network_matches(network: api_types::Network, name: &str) -> bool {
    matches!(
        (network, name),
        (api_types::Network::Local, "local")
            | (api_types::Network::Testnet, "testnet")
            | (api_types::Network::Mainnet, "mainnet")
    )
}

/// `raw_rand`から32バイトを取る。
async fn raw_rand32() -> Result<[u8; 32], ErrorCode> {
    let bytes = ic_cdk_management_canister::raw_rand()
        .await
        .map_err(|error| internal(format!("raw_rand failed: {error}")))?;
    bytes
        .get(..32)
        .ok_or_else(|| internal("raw_rand too short".to_string()))?
        .try_into()
        .map_err(|_| internal("raw_rand length".to_string()))
}

/// セッションを検証し、本人のuser_idを返す（認可境界の試験用）。///
/// vaultに問い合わせ、返却されたprincipalが「このメッセージのcaller」と一致する場合だけ
/// user_idを返す。順序を逆にしない（callerを信用しない）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn whoami(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let vault_bytes = db::tx::query(db::repo::core_config::vault_principal)
        .map_err(map_db)?
        .ok_or_else(|| internal("vault principal is not configured".to_string()))?;
    let vault = Principal::from_slice(&vault_bytes);

    let response = Call::bounded_wait(vault, "session_status")
        .with_arg(session)
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault session_status: {error}"),
        })?;
    let status: Result<SessionStatus, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let status = status?;

    if status.principal != caller {
        return Err(ErrorCode::Unauthenticated {
            reason: "session does not belong to this caller".to_string(),
        });
    }
    Ok(status.user_id)
}

/// 銘柄解決に使うnetwork・dexを設定する（controllerのみ）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_market_context(network: String, dex: String) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the market context".to_string(),
        });
    }
    // 環境の判定は起動時に検証可能な値で行う（mainnetはPhase 2で拒否。E-2）。
    hl_types::environment::parse_network(&network).map_err(environment::map_environment)?;
    db::tx::update(|connection| {
        db::repo::core_config::set_market_context(connection, &network, &dex)
    })
    .map_err(map_db)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn configure_cycles(daily_floor: u128, exit_reserve: u128) -> Result<(), ErrorCode> {
    cycles::configure(daily_floor, exit_reserve)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn get_cycles_status() -> Result<api_types::operations_status::CyclesStatus, ErrorCode> {
    cycles::status()
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn configure_market_threshold(
    input: api_types::operations_status::MarketThreshold,
) -> Result<(), ErrorCode> {
    market::configure(input)
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_market_status(
    market: String,
) -> Result<api_types::operations_status::MarketStatus, ErrorCode> {
    market::status(&market)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn refresh_market() -> Result<(), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    market::poll_if_due(ic_cdk::api::time() / 1_000_000).await
}

/// `meta`の`universe`を登録する（ローカルのブートストラップ。本番はHL `/info` から取得する）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_meta_cache(network: String, dex: String, universe: String) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can seed the meta cache".to_string(),
        });
    }
    let digest = hl_sign::keccak256(universe.as_bytes());
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::meta::set_universe(connection, &network, &dex, &digest, &universe, now)
    })
    .map_err(map_db)
}

/// Hyperliquidのendpointを設定する（controllerのみ）。
///
/// 設定済みのnetworkと整合しないhost（例：testnet設定にmainnet endpoint）は拒否する。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_venue_endpoints(exchange_url: String, info_url: String) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the venue endpoints".to_string(),
        });
    }
    let network = environment::resolved()?.network.into();
    hl_types::environment::validate_endpoints(network, &exchange_url, &info_url)
        .map_err(environment::map_environment)?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::core_config::set_venue_endpoints(connection, &exchange_url, &info_url, now)
    })
    .map_err(map_db)
}

/// 閾値ECDSAのkey IDを設定する（controllerのみ）。
///
/// testnetの鍵名はデプロイ後に実測して確定する（`docs/phase-0/environments.md` 2節）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_ecdsa_key_id(key_id: String) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the ecdsa key id".to_string(),
        });
    }
    hl_types::environment::validate_key_id(&key_id).map_err(environment::map_environment)?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::core_config::set_ecdsa_key_id(connection, &key_id, now))
        .map_err(map_db)?;
    Ok(())
}

/// 現在の環境設定（診断用・公開）。秘密は含まない。
#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_environment() -> Result<api_types::environment::EnvironmentView, ErrorCode> {
    environment::resolved()
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_send_journal(principal: Principal) -> Result<(), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    journal_client::configure(principal)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn set_journal_guard(principal: Principal) -> Result<(), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    journal_client::set_guard(principal)
}

#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn resume_journal() -> Result<(), ErrorCode> {
    journal_client::resume("core").await
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_send_journal() -> Result<Option<Principal>, ErrorCode> {
    journal_client::configured()
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_journal_send_status() -> Result<(bool, bool), ErrorCode> {
    journal_client::public_status()
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn get_journal_guard() -> Result<Option<Principal>, ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    journal_client::guard()
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn journal_restore_status() -> Result<(u64, u64, bool), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    journal_client::status()
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn recovery_stage_status() -> Result<(u64, bool), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    journal_client::recovery_stage_status()
}

#[scoped_entrypoint::query(scope = Core, prefix = "core_")]
fn recovery_replay_pending() -> Result<bool, ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    journal_client::replay_pending_validation()
}

/// 外部確認済みのleverage preflight不明状態をcontrollerが解決する。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
fn resolve_unknown_order_preflight(
    order_id: api_types::Blob,
    resolution: api_types::order::PreflightResolution,
) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can resolve an unknown order preflight".to_string(),
        });
    }
    let order_id: [u8; 32] = order_id.as_ref().try_into().map_err(|_| {
        bad(
            BadRequestCode::MalformedPayload,
            "order_id must be 32 bytes",
        )
    })?;
    let applied = matches!(resolution, api_types::order::PreflightResolution::Applied);
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::orders::resolve_unknown_preflight(
            connection,
            &order_id,
            applied,
            caller.as_slice(),
            now,
        )
    })
    .map_err(map_db)
}

/// 受付を1件処理する（認可・検証・冪等性・pending注文の登録）。
///
/// 署名・送信・照合はパイプライン（次段階）が行う。ここでは受付だけを確定させる。
async fn submit_order(
    session: SessionHandle,
    args: api_types::order::SubmitOrderArgs,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    let user_id = authorize(&session).await?;
    submit_inner(&session, user_id, args).await
}

struct OrderAcceptance<'a> {
    user_id: &'a [u8; 32],
    account_id: &'a [u8; 32],
    request_id: &'a [u8],
    fingerprint: &'a [u8; 32],
    reduce_only: bool,
    notional: u64,
    equity: u64,
}

fn order_acceptance_preflight(
    connection: &ic_sqlite_vfs::db::connection::Connection,
    order: &OrderAcceptance<'_>,
) -> Result<db::repo::core_requests::AcceptOutcome, DbError> {
    if !order.reduce_only
        && (db::repo::recovery_fences::migration_locked(connection)?
            || db::repo::recovery_fences::active(connection, order.account_id)?)
    {
        return Err(DbError::Conflict);
    }
    let accepted = db::repo::core_requests::request_status(
        connection,
        order.user_id,
        order.request_id,
        order.fingerprint,
    )?;
    if accepted != db::repo::core_requests::AcceptOutcome::Accepted {
        return Ok(accepted);
    }
    if db::repo::orders::pending_order_count(connection, order.account_id)? >= MAX_PENDING_ORDERS {
        return Err(DbError::Invariant("too many pending orders"));
    }
    if !order.reduce_only {
        db::repo::orders::ensure_risk_within_equity(
            connection,
            order.account_id,
            order.notional,
            order.equity,
        )?;
    }
    Ok(db::repo::core_requests::AcceptOutcome::Accepted)
}

/// 認可済みの受付（`submit_order`・決済の共通経路）。
///
/// 検証・冪等性・リスク予約・`pending`注文の登録を行う。`reduce_only`は
/// 新規リスクを増やさないため、リスク予約と鮮度ゲートの対象外とする
/// （建玉があるときに保護・決済を打てなくなる方が危険である）。
async fn submit_inner(
    session: &SessionHandle,
    user_id: [u8; 32],
    args: api_types::order::SubmitOrderArgs,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    let now = ic_cdk::api::time() / 1_000_000;
    if args.client_request_id.is_empty() || args.client_request_id.len() > 64 {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "client_request_id must be 1..=64 bytes",
        ));
    }

    // 銘柄は初期allowlistのみ。asset indexはmetaから解決する（固定値を埋め込まない）。
    // 緊急停止中は新規受付を行わない（fail-closed）。
    if !args.reduce_only {
        cycles::require_new()?;
        require_not_stopped().await?;
    }

    let market = args.market.to_uppercase();
    // 銘柄はpolicy_registryのallowlistで判定する（照会失敗・未設定はfail-closed）。
    if !args.reduce_only
        && !policy_markets()
            .await?
            .iter()
            .any(|allowed| allowed == &market)
    {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::AssetNotAllowed,
        });
    }
    if !args.reduce_only {
        market::require(&market)?;
    }

    // 数量・価格は正規化十進で検証し、丸めない。
    let quantity = hl_types::decimal::Decimal::parse(&args.quantity).map_err(bad_decimal)?;
    if quantity.as_str().starts_with('-') || quantity.as_str() == "0" {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "quantity must be positive",
        ));
    }
    let price = match (&args.kind, &args.limit_price) {
        (api_types::order::OrderKind::MarketIoc, None) => {
            return Err(bad(
                BadRequestCode::MissingField,
                "market orders require a slippage-bounded limit price",
            ));
        }
        (_, Some(price)) => {
            let price = hl_types::decimal::Decimal::parse(price).map_err(bad_decimal)?;
            if price.as_str().starts_with('-') || price.as_str() == "0" {
                return Err(bad(
                    BadRequestCode::PriceOutOfRange,
                    "price must be positive",
                ));
            }
            Some(price)
        }
        _ => {
            // 成行（Market IOC）も指値（スリッページ上限）と同様に価格を必須とする。
            return Err(bad(BadRequestCode::MissingField, "limit price is required"));
        }
    };
    let effective_leverage = args.leverage.unwrap_or(3);
    if !(1..=5).contains(&effective_leverage) {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "leverage must be between 1 and 5",
        ));
    }
    let effective_slippage_bps = match args.kind {
        api_types::order::OrderKind::MarketIoc => {
            let value = args.slippage_tolerance_bps.unwrap_or(DEFAULT_SLIPPAGE_BPS);
            if !(1..=10_000).contains(&value) {
                return Err(bad(
                    BadRequestCode::QuantityOutOfRange,
                    "slippage tolerance must be between 1 and 10000 bps",
                ));
            }
            Some(value)
        }
        api_types::order::OrderKind::LimitGtc => {
            if args.slippage_tolerance_bps.is_some() {
                return Err(bad(
                    BadRequestCode::MalformedPayload,
                    "slippage tolerance only applies to market IOC orders",
                ));
            }
            None
        }
    };
    if args.expires_after.is_some_and(|expires| expires <= now) {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "expires_after must be in the future",
        ));
    }

    // 数量・価格の精度を銘柄の `szDecimals` で検査する（HLはperpsで有効数字5桁まで、
    // 価格は 6-szDecimals 桁まで）。metaに無い場合は設定不備としてfail-closedで拒否する。
    let (asset_index, sz_decimals) = resolve_asset(&market)?;
    // 数量は `szDecimals` まで。reduce-only（決済・保護）は建玉の数量をそのまま送れる
    // 必要があるため桁数のみを検査する（有効数字の制限で決済不能にしない）。
    let quantity_limit = if args.reduce_only { None } else { Some(5) };
    match quantity_limit {
        Some(max_significant) => quantity
            .validate_precision(sz_decimals, max_significant)
            .map_err(|error| bad(BadRequestCode::PrecisionExceeded, &error.to_string()))?,
        None => {
            if quantity.scale() > sz_decimals {
                return Err(bad(
                    BadRequestCode::PrecisionExceeded,
                    "quantity exceeds the market scale",
                ));
            }
        }
    }
    if let Some(price) = &price {
        price
            .validate_precision(6u32.saturating_sub(sz_decimals), 5)
            .map_err(|error| bad(BadRequestCode::PrecisionExceeded, &error.to_string()))?;
    }
    // 本文fingerprint（受付の冪等性）。**本文全体**を含める。side・reduce_only・
    // leverage・kind を除くと、同一IDで反対売買やreduce_onlyを変えた再送が
    // 「同一本文」と誤判定され、黙って捨てられる。
    let mut body = Vec::new();
    body.extend_from_slice(&(market.len() as u64).to_be_bytes());
    body.extend_from_slice(market.as_bytes());
    body.extend_from_slice(&(quantity.as_str().len() as u64).to_be_bytes());
    body.extend_from_slice(quantity.as_str().as_bytes());
    body.extend_from_slice(&(args.limit_price.as_deref().unwrap_or("").len() as u64).to_be_bytes());
    body.extend_from_slice(args.limit_price.as_deref().unwrap_or("").as_bytes());
    body.push(u8::from(matches!(args.side, api_types::order::Side::Buy)));
    body.push(u8::from(args.reduce_only));
    body.push(match args.kind {
        api_types::order::OrderKind::MarketIoc => 1,
        api_types::order::OrderKind::LimitGtc => 2,
    });
    body.extend_from_slice(&effective_leverage.to_be_bytes());
    body.extend_from_slice(&effective_slippage_bps.unwrap_or(0).to_be_bytes());
    body.extend_from_slice(&args.expires_after.unwrap_or(0).to_be_bytes());
    body.extend_from_slice(args.account_id.as_ref());
    // トリガの有無と内容も本文に含める（SL/TPだけを差し替えた再送を別本文とする）。
    if let Some(trigger) = &args.trigger {
        body.push(u8::from(matches!(
            trigger.kind,
            api_types::order::TriggerKind::StopLoss
        )));
        body.extend_from_slice(&(trigger.trigger_price.len() as u64).to_be_bytes());
        body.extend_from_slice(trigger.trigger_price.as_bytes());
        body.push(u8::from(trigger.is_market));
    }
    let fingerprint = body_fingerprint(&body);

    let account_id = trading_account(session).await?;
    if args.account_id.as_ref() != account_id.as_slice() {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::AccountNotOwned,
        });
    }
    if !args.reduce_only {
        vault_eligibility_session(session, &account_id).await?;
    }
    // 照合（sweep）はセッションを持たないため、取引所アドレスをここで保存しておく。
    cache_trading_address(session, &account_id, &user_id).await?;
    // 取引所データが古い、または一度も観測できていない場合は新規リスクを増やさない。
    // reduce-onlyはリスクを減らす方向にしか作用しないため、鮮度に関わらず受け付ける
    // （建玉があるときに保護・決済を打てなくなる方が危険である）。
    if !args.reduce_only {
        let observed = db::tx::query(|connection| {
            db::repo::positions::latest_observed(connection, &account_id)
        })
        .map_err(map_db)?;
        if observed.is_none_or(|observed| now.saturating_sub(observed) > STALE_DATA_MS) {
            return Err(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            });
        }
    }

    // SL/TPは建玉単位（positionTpsl）のreduce-only注文としてのみ受け付ける。
    if let Some(trigger) = &args.trigger {
        validate_trigger(&account_id, &market, &args, trigger, sz_decimals)?;
    }

    // 想定元本とequityは新規リスクの上限判断にのみ使う（reduce-onlyは予約しない）。
    let (notional, equity) = if args.reduce_only {
        (0, 0)
    } else {
        (
            notional_micros(args.limit_price.as_deref().unwrap_or("0"), &args.quantity)?,
            vault_trading_equity(session).await?,
        )
    };

    let cloid = ic_cdk_management_canister::raw_rand()
        .await
        .map_err(|error| internal(format!("raw_rand failed: {error}")))?;
    let cloid: [u8; 16] = cloid
        .get(..16)
        .ok_or_else(|| internal("raw_rand too short".to_string()))?
        .try_into()
        .map_err(|_| internal("cloid length".to_string()))?;

    let order_id = {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&cloid);
        bytes.extend_from_slice(&user_id);
        body_fingerprint(&bytes)
    };

    // 上の外部照会中にstopやallowlistが変わり得るため、永続化直前に再検証する。
    if !args.reduce_only {
        cycles::require_new()?;
        market::require(&market)?;
        require_not_stopped().await?;
    }
    if !args.reduce_only {
        vault_eligibility_session(session, &account_id).await?;
    }
    if !args.reduce_only
        && !policy_markets()
            .await?
            .iter()
            .any(|allowed| allowed == &market)
    {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::AssetNotAllowed,
        });
    }

    if notional > i64::MAX as u64 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "notional exceeds storage range",
        ));
    }
    let now = ic_cdk::api::time() / 1_000_000;
    if args.expires_after.is_some_and(|expires| expires <= now) {
        return Err(bad(BadRequestCode::MalformedPayload, "order has expired"));
    }
    let request_id = args.client_request_id.as_ref();
    let acceptance = OrderAcceptance {
        user_id: &user_id,
        account_id: &account_id,
        request_id,
        fingerprint: &fingerprint,
        reduce_only: args.reduce_only,
        notional,
        equity,
    };
    match db::tx::query(|connection| order_acceptance_preflight(connection, &acceptance))
        .map_err(map_db)?
    {
        db::repo::core_requests::AcceptOutcome::Duplicate => {
            let (existing_id, existing_cloid) = db::tx::query(|connection| {
                db::repo::orders::order_by_request(connection, &user_id, request_id)
            })
            .map_err(map_db)?
            .ok_or_else(|| internal("duplicate request without an order".to_string()))?;
            return Ok(api_types::order::SubmitOrderResult {
                request_id: args.client_request_id,
                order_id: existing_id.to_vec().into(),
                cloid: existing_cloid.to_vec().into(),
                accepted_at: now,
            });
        }
        db::repo::core_requests::AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: args.client_request_id,
            });
        }
        db::repo::core_requests::AcceptOutcome::Accepted => {}
    }
    let mut logical = b"order_accepted".to_vec();
    logical.extend_from_slice(&user_id);
    logical.extend_from_slice(request_id);
    let event = api_types::journal::RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: api_types::journal::RecoveryPayload::OrderAccepted {
            order_id: order_id.to_vec().into(),
            request_id: request_id.to_vec().into(),
            user_id: user_id.to_vec().into(),
            account_id: account_id.to_vec().into(),
            cloid: cloid.to_vec().into(),
            body_hash: fingerprint.to_vec().into(),
            risk_micros: notional,
            reduce_only: args.reduce_only,
            accepted_at_ms: now,
        },
    };
    let ack = journal_client::append_recovery_event_if("core", event.clone(), |connection| {
        match order_acceptance_preflight(connection, &acceptance)? {
            db::repo::core_requests::AcceptOutcome::Accepted => Ok(true),
            db::repo::core_requests::AcceptOutcome::Duplicate
            | db::repo::core_requests::AcceptOutcome::Conflict => Ok(false),
        }
    })
    .await?;
    let Some(ack) = ack else {
        let status = db::tx::query(|connection| {
            db::repo::core_requests::request_status(connection, &user_id, request_id, &fingerprint)
        })
        .map_err(map_db)?;
        return match status {
            db::repo::core_requests::AcceptOutcome::Duplicate => {
                let (existing_id, existing_cloid) = db::tx::query(|connection| {
                    db::repo::orders::order_by_request(connection, &user_id, request_id)
                })
                .map_err(map_db)?
                .ok_or_else(|| internal("duplicate request without an order".to_string()))?;
                Ok(api_types::order::SubmitOrderResult {
                    request_id: args.client_request_id,
                    order_id: existing_id.to_vec().into(),
                    cloid: existing_cloid.to_vec().into(),
                    accepted_at: now,
                })
            }
            db::repo::core_requests::AcceptOutcome::Conflict => {
                Err(ErrorCode::IdempotencyConflict {
                    request_id: args.client_request_id,
                })
            }
            db::repo::core_requests::AcceptOutcome::Accepted => Err(ErrorCode::PolicyUnavailable),
        };
    };

    let revalidated = async {
        if authorize(session).await? != user_id || trading_account(session).await? != account_id {
            return Err(ErrorCode::SessionRevoked);
        }
        if args
            .expires_after
            .is_some_and(|expires| expires <= ic_cdk::api::time() / 1_000_000)
        {
            return Err(bad(BadRequestCode::MalformedPayload, "order has expired"));
        }
        if !args.reduce_only {
            cycles::require_new()?;
            market::require(&market)?;
            require_not_stopped().await?;
            vault_eligibility_session(session, &account_id).await?;
            if !policy_markets()
                .await?
                .iter()
                .any(|allowed| allowed == &market)
            {
                return Err(ErrorCode::NotAllowed {
                    code: api_types::error::NotAllowedCode::AssetNotAllowed,
                });
            }
            if resolve_asset(&market)?.0 != asset_index {
                return Err(ErrorCode::PolicyUnavailable);
            }
            let observed = db::tx::query(|connection| {
                db::repo::positions::latest_observed(connection, &account_id)
            })
            .map_err(map_db)?;
            if observed.is_none_or(|observed| {
                (ic_cdk::api::time() / 1_000_000).saturating_sub(observed) > STALE_DATA_MS
            }) {
                return Err(ErrorCode::NotAllowed {
                    code: api_types::error::NotAllowedCode::OperationNotAvailable,
                });
            }
        }
        Ok(())
    }
    .await;
    let result = db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &event, &ack)?;
        let admitted = revalidated.is_ok()
            && matches!(
                order_acceptance_preflight(connection, &acceptance),
                Ok(db::repo::core_requests::AcceptOutcome::Accepted)
            );
        let accepted = db::repo::core_requests::accept_request(
            connection,
            &user_id,
            request_id,
            &fingerprint,
            now,
        )?;
        if accepted != db::repo::core_requests::AcceptOutcome::Accepted {
            return Err(DbError::Conflict);
        }
        // reduce-onlyはエクスポージャを増やさないため、リスク予約を取らない
        // （予約すると建玉を閉じるための資金が無い状態で決済できなくなる）。
        if admitted && !args.reduce_only {
            db::repo::orders::reserve_risk(
                connection,
                &account_id,
                args.client_request_id.as_ref(),
                notional,
                now,
            )?;
        }
        db::repo::orders::insert_pending_order(
            connection,
            &db::repo::orders::NewOrder {
                order_id,
                user_id,
                account_id,
                client_request_id: args.client_request_id.as_ref().to_vec(),
                cloid,
                market: market.clone(),
                asset_index,
                is_buy: matches!(args.side, api_types::order::Side::Buy),
                kind: match args.kind {
                    api_types::order::OrderKind::MarketIoc => "market_ioc",
                    api_types::order::OrderKind::LimitGtc => "limit_gtc",
                }
                .to_string(),
                price: args.limit_price.clone(),
                quantity: quantity.as_str().to_string(),
                reduce_only: args.reduce_only,
                effective_leverage,
                slippage_tolerance_bps: effective_slippage_bps,
                expires_after: args.expires_after,
                trigger: args
                    .trigger
                    .as_ref()
                    .map(|trigger| db::repo::orders::NewTrigger {
                        kind: db::states::trigger_kind_str(trigger.kind).to_string(),
                        price: trigger.trigger_price.clone(),
                        is_market: trigger.is_market,
                    }),
            },
            now,
        )?;
        if !admitted {
            db::repo::orders::reject_queued_acceptance(connection, &order_id, now)?;
        }
        Ok(admitted)
    });
    let admitted = match result {
        Ok(admitted) => admitted,
        Err(error) => {
            journal_client::lock()?;
            return Err(map_db(error));
        }
    };
    if !admitted {
        return Err(revalidated.err().unwrap_or(ErrorCode::PolicyUnavailable));
    }

    Ok(api_types::order::SubmitOrderResult {
        request_id: args.client_request_id,
        order_id: order_id.to_vec().into(),
        cloid: cloid.to_vec().into(),
        accepted_at: now,
    })
}

/// 建玉を閉じる（全量または比率指定）。反対売買のreduce-only IOC指値として受付ける。
///
/// `limit_price`はスリッページ上限（公開市況から画面が決める）。省略時は観測した
/// 建玉からmark価格を近似して`DEFAULT_SLIPPAGE_BPS`の幅を付ける。
/// `ratio_bps`は建玉に対する比率（10000 = 全量）。
async fn close_position(
    session: SessionHandle,
    client_request_id: api_types::Blob,
    market: String,
    ratio_bps: u32,
    limit_price: Option<String>,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    let user_id = authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let market = market.to_uppercase();
    let position = open_position(&account_id, &market)?;
    let args = close_order_args(
        &session,
        &account_id,
        client_request_id.as_ref().to_vec(),
        &position,
        ratio_bps,
        limit_price,
    )?;
    submit_inner(&session, user_id, args).await
}

/// 建玉をすべて閉じる（建玉ごとに`close_position`と同じ反対売買を送る）。
///
/// 1件の失敗で全体を止めない（建玉ごとの結果を返す）。受付IDは
/// `client_request_id`と銘柄から導出するため、同じIDの再送は同じ注文として扱われる
/// （建玉が変わっている場合は`IdempotencyConflict`になる）。
async fn close_all(
    session: SessionHandle,
    client_request_id: api_types::Blob,
) -> Result<api_types::order::CloseAllOutcome, ErrorCode> {
    let user_id = authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let positions = db::tx::query(|connection| db::repo::positions::list(connection, &account_id))
        .map_err(map_db)?;

    let mut outcome = api_types::order::CloseAllOutcome {
        submitted: Vec::new(),
        failed: Vec::new(),
    };
    for position in positions {
        if !is_open_size(&position.size) {
            continue;
        }
        // 銘柄ごとに決定的な受付IDを作り、再送を冪等にする。
        let mut seed = client_request_id.as_ref().to_vec();
        seed.extend_from_slice(b"close_all");
        seed.extend_from_slice(position.market.as_bytes());
        let request_id = body_fingerprint(&seed).to_vec();
        let result =
            match close_order_args(&session, &account_id, request_id, &position, 10_000, None) {
                Ok(args) => submit_inner(&session, user_id, args).await,
                Err(error) => Err(error),
            };
        match result {
            Ok(result) => outcome.submitted.push(result),
            Err(error) => outcome.failed.push(api_types::order::CloseFailure {
                market: position.market,
                error,
            }),
        }
    }
    Ok(outcome)
}

/// 建玉の数量が0でない（建玉が開いている）。
fn is_open_size(size: &str) -> bool {
    size.chars()
        .any(|character| character.is_ascii_digit() && character != '0')
}

/// 口座の開いている建玉（無ければ拒否）。
fn open_position(
    account_id: &[u8; 32],
    market: &str,
) -> Result<api_types::order::PositionView, ErrorCode> {
    let position =
        db::tx::query(|connection| db::repo::positions::find(connection, account_id, market))
            .map_err(map_db)?
            .ok_or(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            })?;
    if !is_open_size(&position.size) {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    Ok(position)
}

/// 決済注文（反対売買のreduce-only IOC指値）を組み立てる。
///
/// 数量は建玉数量×比率を銘柄の`szDecimals`で切り捨てる（建玉を超えない）。
/// 価格は呼び出し元の指定（公開市況から決めるスリッページ上限）を優先し、
/// 省略時は観測した建玉からmark価格を近似する。
fn close_order_args(
    session: &SessionHandle,
    account_id: &[u8; 32],
    client_request_id: Vec<u8>,
    position: &api_types::order::PositionView,
    ratio_bps: u32,
    limit_price: Option<String>,
) -> Result<api_types::order::SubmitOrderArgs, ErrorCode> {
    if ratio_bps == 0 || ratio_bps > 10_000 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "ratio must be within 1..=10000 bps",
        ));
    }
    let market = position.market.to_uppercase();
    let (_, sz_decimals) = resolve_asset(&market)?;
    let is_long = !position.size.starts_with('-');
    let size_micros = signed_micros(&position.size)?;
    let magnitude = size_micros.unsigned_abs();
    // `szDecimals`より細かい桁は送れないため、切り捨てる（建玉を超えない方向）。
    let step = if sz_decimals >= 6 {
        1u128
    } else {
        10u128.pow(6 - sz_decimals)
    };
    let quantity_micros = magnitude * u128::from(ratio_bps) / 10_000 / step * step;
    if quantity_micros == 0 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "the position is too small to close at this ratio",
        ));
    }
    let quantity = micros_to_decimal(quantity_micros);

    let price = match limit_price {
        Some(price) => price,
        None => slippage_bounded_price(position, is_long, sz_decimals)?,
    };
    Ok(api_types::order::SubmitOrderArgs {
        session: session.clone(),
        client_request_id: client_request_id.into(),
        account_id: account_id.to_vec().into(),
        market,
        side: if is_long {
            api_types::order::Side::Sell
        } else {
            api_types::order::Side::Buy
        },
        kind: api_types::order::OrderKind::MarketIoc,
        quantity,
        limit_price: Some(price),
        slippage_tolerance_bps: Some(DEFAULT_SLIPPAGE_BPS),
        reduce_only: true,
        leverage: None,
        trigger: None,
        expires_after: None,
    })
}

/// 観測した建玉からスリッページ上限つきのIOC指値を作る。
///
/// mark価格は`entry_price + unrealized_pnl / size`で近似する（coreはmark配信を
/// 持たない）。reduce-onlyでは価格はスリッページ上限としてのみ作用し、建玉を
/// 反転できないため、近似でも安全側に働く。価格は市場の小数桁数と有効数字5桁に丸める。
fn slippage_bounded_price(
    position: &api_types::order::PositionView,
    is_long: bool,
    sz_decimals: u32,
) -> Result<String, ErrorCode> {
    let entry = decimal_micros(&position.entry_price)?;
    let size = signed_micros(&position.size)?;
    if size == 0 {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    let adjustment = i128::from(position.unrealized_pnl)
        .checked_mul(1_000_000)
        .ok_or_else(|| internal("mark price overflow".to_string()))?
        / size;
    let mark = entry
        .checked_add(adjustment)
        .ok_or_else(|| internal("mark price overflow".to_string()))?;
    let bounded = if is_long {
        mark * i128::from(10_000 - DEFAULT_SLIPPAGE_BPS) / 10_000
    } else {
        mark * i128::from(10_000 + DEFAULT_SLIPPAGE_BPS) / 10_000
    };
    if bounded <= 0 {
        return Err(bad(
            BadRequestCode::PriceOutOfRange,
            "cannot derive a positive limit price",
        ));
    }
    let rounded = close_price::round_price_micros(bounded.unsigned_abs(), !is_long, sz_decimals);
    Ok(micros_to_decimal(rounded))
}

/// マイクロ単位の整数を正規化した十進文字列へ戻す（末尾ゼロを残さない）。
fn micros_to_decimal(micros: u128) -> String {
    let whole = micros / 1_000_000;
    let fraction = micros % 1_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    let mut text = format!("{fraction:06}");
    while text.ends_with('0') {
        text.pop();
    }
    format!("{whole}.{text}")
}

/// 符号付きの十進文字列をマイクロ（1e-6）単位へ変換する（丸めない）。
///
/// `decimal_micros`は整数部の符号だけを見るため`-0.02`の符号を落とす。
/// 建玉の符号（ロング・ショート）を扱う経路ではこちらを使う。
fn signed_micros(text: &str) -> Result<i128, ErrorCode> {
    match text.strip_prefix('-') {
        Some(magnitude) => Ok(-decimal_micros(magnitude)?),
        None => decimal_micros(text),
    }
}

/// Agent世代を要求する（**coreが鍵を導出・保管**し、vaultはmaster署名でアドレスを承認する）。
///
/// 未承認の世代があるうちは同じ世代を返す。注文はこの世代の鍵で署名する。
async fn request_agent_generation(
    session: SessionHandle,
) -> Result<api_types::fund::AgentGeneration, ErrorCode> {
    // 認可（caller束縛）を確認してから口座を解決する。
    authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;

    if let Some(latest) =
        db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
            .map_err(map_db)?
        && latest.state == api_types::fund::AgentState::Requested
    {
        return Ok(latest);
    }

    let generation =
        db::tx::update(|connection| db::repo::agents::latest_generation(connection, &account_id))
            .map_err(map_db)?
            .saturating_add(1);

    let path = agent_derivation_path(&account_id, generation);
    let public_key = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: path.clone(),
        key_id: ecdsa_key_id()?,
    })
    .await
    .map_err(|error| internal(format!("ecdsa_public_key failed: {error}")))?
    .public_key;
    let public_key: [u8; 33] = public_key
        .try_into()
        .map_err(|_| internal("unexpected public key length".to_string()))?;
    let address = hl_sign::address_from_public_key(&public_key)
        .map_err(|error| internal(error.to_string()))?;

    db::tx::update(|connection| {
        db::repo::agents::insert_generation(
            connection,
            &account_id,
            generation,
            &address,
            "private-perp/agent",
            now,
        )
    })
    .map_err(map_db)?;

    Ok(api_types::fund::AgentGeneration {
        account_id: account_id.to_vec().into(),
        generation,
        agent_address: address.to_vec().into(),
        approved_at: None,
        expires_at: None,
        state: api_types::fund::AgentState::Requested,
    })
}

/// Agent世代の状態（承認済みは`current`、要求中は`next`）。
///
/// 認可にvaultへのinter-canister呼び出しが必要なためqueryにはできない（updateで提供）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn get_agent_status(
    session: SessionHandle,
) -> Result<api_types::fund::AgentStatus, ErrorCode> {
    authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    let next = db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
        .map_err(map_db)?;
    // 承認はvaultがmaster署名で行い、vaultのDBへ永続化される。coreは自分の行を
    // 「承認済み」と推測せず、vaultの状態を正とする。
    let current = match next.as_ref() {
        Some(generation) if generation.state != api_types::fund::AgentState::Active => {
            agent_approval(&account_id, generation.generation).await?
        }
        Some(generation) => Some(generation.clone()),
        None => None,
    };
    let next = next.filter(|generation| {
        !matches!(
            current.as_ref(),
            Some(approved) if approved.generation == generation.generation
        )
    });
    Ok(api_types::fund::AgentStatus {
        current,
        next,
        revocation_pending: false,
        observed_at: now,
    })
}

/// vaultに永続化された承認状態（未承認・照会失敗は `None`）。
///
/// 署名可否の判断にも使うため、照会できない場合は「承認済み」と扱わない。
async fn agent_approval(
    account_id: &[u8; 32],
    generation: u64,
) -> Result<Option<api_types::fund::AgentGeneration>, ErrorCode> {
    let vault = vault_principal()?;
    // 2引数は `with_args`（`with_arg` は1引数としてエンコードする）。
    let response = Call::bounded_wait(vault, "get_agent_approval")
        .with_args(&(api_types::Blob::from(account_id.to_vec()), generation))
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_agent_approval: {error}"),
        })?;
    let approval: Result<Option<api_types::fund::AgentGeneration>, ErrorCode> =
        response
            .candid()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let approval = approval?;
    Ok(approval.filter(|row| row.state == api_types::fund::AgentState::Active))
}

/// Agent鍵の導出経路（口座と世代で分離する）。
fn agent_derivation_path(account_id: &[u8; 32], generation: u64) -> Vec<Vec<u8>> {
    vec![
        b"private-perp".to_vec(),
        b"agent".to_vec(),
        hex::encode(account_id).into_bytes(),
        generation.to_be_bytes().to_vec(),
    ]
}

/// 閾値ECDSAのkey ID（起動時の環境設定から解決する）。
pub(crate) fn ecdsa_key_id() -> Result<EcdsaKeyId, ErrorCode> {
    Ok(EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: environment::resolved()?.ecdsa_key_id,
    })
}

/// 注文の取消を要求する（**封筒必須**。署名・送信はパイプラインが行う）。
///
/// 認証は封筒の`aad`と本文のセッションで行う（`api-contract.md` 6節）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn cancel_order(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::CancelOrderQuery>(&envelope, "cancel_order").await?;
    cancel_order_inner(&query.session, query.order_id).await?;
    seal_envelope(&envelope, "cancel_order", &request_id, caller, &()).await
}

/// 取消の本体（受付と同様に冪等で、既に取消要求済み・終端状態なら何もしない）。
async fn cancel_order_inner(
    session: &SessionHandle,
    order_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let user_id = authorize(session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    let order_id: [u8; 32] = order_id.as_ref().try_into().map_err(|_| {
        bad(
            BadRequestCode::MalformedPayload,
            "order_id must be 32 bytes",
        )
    })?;

    let owner = db::tx::query(|connection| db::repo::orders::order_owner(connection, &order_id))
        .map_err(map_db)?
        .ok_or_else(|| bad(BadRequestCode::MalformedPayload, "unknown order"))?;

    let (order_user, state, cancel_requested) = owner;
    if order_user != user_id {
        return Err(ErrorCode::Unauthenticated {
            reason: "order does not belong to this caller".to_string(),
        });
    }
    if cancel_requested || is_terminal(state) {
        return Ok(());
    }

    db::tx::update(|connection| db::repo::orders::mark_cancel_requested(connection, &order_id, now))
        .map_err(map_db)
}

/// 終端状態（これ以上状態が進まない）。
fn is_terminal(state: api_types::order::OrderState) -> bool {
    use api_types::order::OrderState;
    matches!(
        state,
        OrderState::Filled | OrderState::Cancelled | OrderState::Rejected
    )
}

/// 未終端の注文すべてに取消要求を付ける（送信はsweepが行う）。
async fn cancel_all(session: SessionHandle) -> Result<u64, ErrorCode> {
    let user_id = authorize(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::orders::mark_all_cancel_requested(connection, &user_id, now)
    })
    .map_err(map_db)
}

/// 本人向け書込みの封筒入口。業務エラーも暗号化した結果として返す。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn private_call(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let method = envelope.method.clone();
    if !matches!(
        method.as_str(),
        "submit_order" | "close_position" | "close_all" | "request_agent_generation" | "cancel_all"
    ) {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "unknown private method",
        ));
    }
    // 外側のBlobが複数引数Candidの生バイト列を保持する。
    let (payload, request_id, caller) =
        open_envelope::<api_types::Blob>(&envelope, &method).await?;
    let malformed = || bad(BadRequestCode::MalformedPayload, "invalid private payload");
    match method.as_str() {
        "submit_order" => {
            let (session, args) = candid::decode_args::<(
                SessionHandle,
                api_types::order::SubmitOrderArgs,
            )>(payload.as_ref())
            .map_err(|_| malformed())?;
            let result = submit_order(session, args).await;
            seal_envelope(&envelope, &method, &request_id, caller, &result).await
        }
        "close_position" => {
            let (session, id, market, ratio, price) = candid::decode_args::<(
                SessionHandle,
                api_types::Blob,
                String,
                u32,
                Option<String>,
            )>(payload.as_ref())
            .map_err(|_| malformed())?;
            let result = close_position(session, id, market, ratio, price).await;
            seal_envelope(&envelope, &method, &request_id, caller, &result).await
        }
        "close_all" => {
            let (session, id) =
                candid::decode_args::<(SessionHandle, api_types::Blob)>(payload.as_ref())
                    .map_err(|_| malformed())?;
            let result = close_all(session, id).await;
            seal_envelope(&envelope, &method, &request_id, caller, &result).await
        }
        "request_agent_generation" => {
            let session =
                candid::decode_one::<SessionHandle>(payload.as_ref()).map_err(|_| malformed())?;
            let result = request_agent_generation(session).await;
            seal_envelope(&envelope, &method, &request_id, caller, &result).await
        }
        "cancel_all" => {
            let session =
                candid::decode_one::<SessionHandle>(payload.as_ref()).map_err(|_| malformed())?;
            let result = cancel_all(session).await;
            seal_envelope(&envelope, &method, &request_id, caller, &result).await
        }
        _ => unreachable!(),
    }
}

/// 約定一覧（新しい順。**封筒必須**。認可にvaultへの問い合わせが必要なためupdate）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn list_fills(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::ListQuery>(&envelope, "list_fills").await?;
    let page = fills_page(&query.session, query.cursor, query.limit).await?;
    seal_envelope(&envelope, "list_fills", &request_id, caller, &page).await
}

async fn fills_page(
    session: &SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::order::FillView>, ErrorCode> {
    let user_id = authorize(session).await?;
    let limit = limit.clamp(1, 100);
    let now = ic_cdk::api::time() / 1_000_000;
    let before = match cursor.as_ref() {
        Some(cursor) => {
            let bytes: [u8; 8] = cursor
                .as_ref()
                .try_into()
                .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid cursor"))?;
            Some(i64::from_be_bytes(bytes))
        }
        None => None,
    };
    let rows = db::tx::query(|connection| {
        db::repo::orders::list_fills(connection, &user_id, before, limit)
    })
    .map_err(map_db)?;
    let next_cursor = if rows.len() == limit as usize {
        rows.last()
            .map(|(rowid, _)| rowid.to_be_bytes().to_vec().into())
    } else {
        None
    };
    Ok(api_types::Paged {
        items: rows.into_iter().map(|(_, fill)| fill).collect(),
        next_cursor,
        observed_at: now,
        revision: 1,
    })
}

/// 口座snapshot（残高はvault、注文はcore。**封筒必須**）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn get_account_snapshot(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::SnapshotQuery>(&envelope, "get_account_snapshot")
            .await?;
    let snapshot = account_snapshot(&query.session).await?;
    seal_envelope(
        &envelope,
        "get_account_snapshot",
        &request_id,
        caller,
        &snapshot,
    )
    .await
}

async fn account_snapshot(
    session: &SessionHandle,
) -> Result<api_types::order::AccountSnapshot, ErrorCode> {
    let user_id = authorize(session).await?;
    let account_id = trading_account(session).await?;
    cache_trading_address(session, &account_id, &user_id).await?;
    let now = ic_cdk::api::time() / 1_000_000;

    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "get_balances")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_balances: {error}"),
        })?;
    let balances: Result<(u64, u64), ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let (trading, withdrawable) = balances?;

    let rows =
        db::tx::query(|connection| db::repo::orders::list_orders(connection, &user_id, None, 50))
            .map_err(map_db)?;
    let revision =
        db::tx::query(|connection| db::repo::orders::account_revision(connection, &user_id))
            .map_err(map_db)?;
    let metrics =
        db::tx::query(|connection| db::repo::positions::account_metrics(connection, &account_id))
            .map_err(map_db)?;

    let mut open_orders = Vec::new();
    let mut pending_orders = Vec::new();
    for (_, order) in rows {
        match order.state {
            api_types::order::OrderState::Open | api_types::order::OrderState::PartiallyFilled => {
                open_orders.push(api_types::order::OrderView {
                    order_id: order.order_id.clone(),
                    cloid: Some(order.cloid.clone()),
                    market: order.market.clone(),
                    side: if order.is_buy {
                        api_types::order::Side::Buy
                    } else {
                        api_types::order::Side::Sell
                    },
                    kind: if order.kind == "market_ioc" {
                        api_types::order::OrderKind::MarketIoc
                    } else {
                        api_types::order::OrderKind::LimitGtc
                    },
                    price: order.price.clone(),
                    quantity: order.quantity.clone(),
                    filled_quantity: order.filled_quantity.clone(),
                    state: order.state,
                    venue_state: None,
                    hl_oid: order.hl_oid,
                    cancel_requested: order.cancel_requested,
                    trigger: order.trigger.clone(),
                    updated_at: order.updated_at,
                });
            }
            api_types::order::OrderState::Pending | api_types::order::OrderState::Unknown => {
                pending_orders.push(api_types::order::PendingOrderView {
                    request_id: order.order_id.clone(),
                    cloid: Some(order.cloid.clone()),
                    order_id: Some(order.order_id.clone()),
                    action_state: order.dispatch_state,
                    since: order.created_at,
                    last_error: None,
                });
            }
            _ => {}
        }
    }

    let mut positions =
        db::tx::query(|connection| db::repo::positions::list(connection, &account_id))
            .map_err(map_db)?;
    // HLのclearinghouseStateは保護注文を建玉へ埋め込まないため、照合済みの
    // positionTpsl注文から現在のSL/TP表示を導出する。
    for order in &open_orders {
        let Some(trigger) = &order.trigger else {
            continue;
        };
        let Some(position) = positions
            .iter_mut()
            .find(|position| position.market == order.market)
        else {
            continue;
        };
        match trigger.kind {
            api_types::order::TriggerKind::StopLoss => {
                position.stop_loss = Some(trigger.trigger_price.clone());
            }
            api_types::order::TriggerKind::TakeProfit => {
                position.take_profit = Some(trigger.trigger_price.clone());
            }
        }
    }

    Ok(api_types::order::AccountSnapshot {
        account_id: account_id.to_vec().into(),
        equity: trading,
        margin_used: metrics.map_or(0, |value| value.margin_used),
        open_order_risk_reserved: db::tx::query(|connection| {
            db::repo::orders::held_risk(connection, &account_id)
        })
        .map_err(map_db)?,
        withdrawable,
        unrealized_pnl: metrics.map_or(0, |value| value.unrealized_pnl),
        positions,
        open_orders,
        pending_orders,
        observed_at: now,
        revision,
        data_age_ms: metrics.map_or(u64::MAX, |value| now.saturating_sub(value.observed_at)),
    })
}

/// 注文一覧（新しい順。**封筒必須**）。
///
/// **updateである理由**：認可に `funds_vault` へのinter-canister呼び出しが必要だが、
/// queryでは他Canisterを呼べない。最終設計では、(a) 個人向け読み取りをupdateのまま
/// 提供する、(b) vaultからセッション写像をcoreへ同期してqueryで返す、のいずれかを選ぶ
/// （`docs/phase-1/README.md` の残課題）。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn list_orders(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::ListQuery>(&envelope, "list_orders").await?;
    let page = orders_page(&query.session, query.cursor, query.limit).await?;
    seal_envelope(&envelope, "list_orders", &request_id, caller, &page).await
}

/// 受付結果を再送せずに照合する。不存在と他人の要求は区別しない。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn get_order_by_request(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::OrderRequestQuery>(&envelope, "get_order_by_request")
            .await?;
    let user_id = authorize(&query.session).await?;
    if query.client_request_id.len() != 32 {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "expected 32-byte request id",
        ));
    }
    let order = db::tx::query(|connection| {
        db::repo::orders::summary_by_request(connection, &user_id, query.client_request_id.as_ref())
    })
    .map_err(map_db)?;
    let status = api_types::envelope::OrderRequestStatus {
        order,
        observed_at: ic_cdk::api::time() / 1_000_000,
    };
    seal_envelope(
        &envelope,
        "get_order_by_request",
        &request_id,
        caller,
        &status,
    )
    .await
}

async fn orders_page(
    session: &SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::order::OrderSummary>, ErrorCode> {
    let user_id = authorize(session).await?;
    let limit = limit.clamp(1, 100);
    let now = ic_cdk::api::time() / 1_000_000;

    let before = match cursor.as_ref() {
        Some(cursor) => {
            let bytes: [u8; 8] = cursor
                .as_ref()
                .try_into()
                .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid cursor"))?;
            Some(i64::from_be_bytes(bytes))
        }
        None => None,
    };

    let rows = db::tx::query(|connection| {
        db::repo::orders::list_orders(connection, &user_id, before, limit)
    })
    .map_err(map_db)?;

    let next_cursor = if rows.len() == limit as usize {
        rows.last()
            .map(|(rowid, _)| rowid.to_be_bytes().to_vec().into())
    } else {
        None
    };

    Ok(api_types::Paged {
        items: rows.into_iter().map(|(_, order)| order).collect(),
        next_cursor,
        observed_at: now,
        revision: 1,
    })
}

/// 十進文字列をマイクロ（1e-6）単位の整数へ変換する（丸めない）。
fn decimal_micros(text: &str) -> Result<i128, ErrorCode> {
    let (integer, fraction) = match text.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (text, ""),
    };
    if integer.is_empty() && fraction.is_empty() {
        return Err(bad(BadRequestCode::MalformedPayload, "empty decimal"));
    }
    if fraction.len() > 6 {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "more than 6 decimals",
        ));
    }
    let integer: i128 = integer
        .parse()
        .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid decimal"))?;
    let mut padded = fraction.to_string();
    while padded.len() < 6 {
        padded.push('0');
    }
    let fraction: i128 = padded
        .parse()
        .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid decimal"))?;
    Ok(integer * 1_000_000 + fraction)
}

/// 価格と数量から想定元本（マイクロUSDC）を求める。
fn notional_micros(price: &str, quantity: &str) -> Result<u64, ErrorCode> {
    let notional = decimal_micros(price)?
        .checked_mul(decimal_micros(quantity)?)
        .ok_or_else(|| internal("notional overflow".to_string()))?
        / 1_000_000;
    u64::try_from(notional).map_err(|_| internal("notional out of range".to_string()))
}

fn bad(code: BadRequestCode, detail: &str) -> ErrorCode {
    ErrorCode::BadRequest {
        code,
        detail: detail.to_string(),
    }
}

fn bad_decimal(error: hl_types::decimal::DecimalError) -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: error.to_string(),
    }
}

/// 受付fingerprint（本文の同一性判定）。暗号学的ハッシュを使う。
fn body_fingerprint(bytes: &[u8]) -> [u8; 32] {
    hl_sign::keccak256(bytes)
}

/// vaultに問い合わせてセッションを検証する（caller束縛はここで行う）。
async fn authorize(session: &SessionHandle) -> Result<[u8; 32], ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let status = vault_session_status(session).await?;
    if status.principal != caller {
        return Err(ErrorCode::Unauthenticated {
            reason: "session does not belong to this caller".to_string(),
        });
    }
    status
        .user_id
        .as_ref()
        .try_into()
        .map_err(|_| internal("user_id must be 32 bytes".to_string()))
}

/// 本人の取引口座IDをvaultから取得する（所有権の確認を兼ねる）。
async fn trading_account(session: &SessionHandle) -> Result<[u8; 32], ErrorCode> {
    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "get_trading_account")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_trading_account: {error}"),
        })?;
    let account: Result<Option<api_types::Blob>, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let account = account?.ok_or(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::AccountNotOwned,
    })?;
    account
        .as_ref()
        .try_into()
        .map_err(|_| internal("account_id must be 32 bytes".to_string()))
}

/// 取引所アドレスを一度だけvaultへ問い合わせて保存する（照合に必要）。
///
/// 署名済み要求の処理中に呼ぶ（sweep中はセッションが無いため事前に保存しておく）。
/// 取得に失敗しても受付は続ける（照合は次回の要求で保存できてから始まる）。
async fn cache_trading_address(
    session: &SessionHandle,
    account_id: &[u8; 32],
    user_id: &[u8; 32],
) -> Result<(), ErrorCode> {
    let cached = db::tx::query(|connection| db::repo::accounts::identity(connection, account_id))
        .map_err(map_db)?;
    if let Some((cached_user, _)) = cached {
        return if cached_user == *user_id {
            Ok(())
        } else {
            Err(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::AccountNotOwned,
            })
        };
    }
    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "get_trading_address")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_trading_address: {error}"),
        })?;
    let address: Result<api_types::Blob, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let address: [u8; 20] = address?
        .as_ref()
        .try_into()
        .map_err(|_| internal("trading address must be 20 bytes".to_string()))?;
    let mut id_material = b"core_account_identity".to_vec();
    id_material.extend_from_slice(account_id);
    let event = api_types::journal::RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&id_material).to_vec().into(),
        payload: api_types::journal::RecoveryPayload::IdentityAccount {
            user_id: user_id.to_vec().into(),
            owner: ic_cdk::api::msg_caller(),
            account_id: account_id.to_vec().into(),
            address: address.to_vec().into(),
        },
    };
    let ack = journal_client::append_recovery_event("core", event.clone()).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    let saved = db::tx::update(|connection| {
        journal_client::record_recovery_event(connection, &event, &ack)?;
        match db::repo::accounts::identity(connection, account_id)? {
            Some((existing_user, existing_address))
                if existing_user == *user_id && existing_address == address =>
            {
                Ok(())
            }
            Some(_) => Err(DbError::Conflict),
            None => db::repo::accounts::upsert(connection, account_id, user_id, &address, now),
        }
    });
    if saved.is_err() {
        journal_client::lock()?;
    }
    saved.map_err(map_db)
}

async fn vault_session_status(session: &SessionHandle) -> Result<SessionStatus, ErrorCode> {
    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "session_status")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault session_status: {error}"),
        })?;
    let status: Result<SessionStatus, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    status
}

async fn vault_eligibility_session(
    session: &SessionHandle,
    account_id: &[u8; 32],
) -> Result<(), ErrorCode> {
    let response = Call::bounded_wait(vault_principal()?, "check_eligibility_for_core")
        .with_args(&(session.clone(), account_id.to_vec()))
        .await
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    response
        .candid::<Result<(), ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)?
}

async fn vault_eligibility_account(
    user_id: &[u8; 32],
    account_id: &[u8; 32],
) -> Result<(), ErrorCode> {
    let response = Call::bounded_wait(vault_principal()?, "check_eligibility_account_for_core")
        .with_args(&(user_id.to_vec(), account_id.to_vec()))
        .await
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    response
        .candid::<Result<(), ErrorCode>>()
        .map_err(|_| ErrorCode::PolicyUnavailable)?
}

/// vaultから本人の取引口座equityを取得する（リスク上限の判断に使う）。
async fn vault_trading_equity(session: &SessionHandle) -> Result<u64, ErrorCode> {
    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "get_balances")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_balances: {error}"),
        })?;
    let balances: Result<(u64, u64), ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    Ok(balances?.0)
}

fn vault_principal() -> Result<Principal, ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::vault_principal)
        .map_err(map_db)?
        .ok_or_else(|| internal("vault principal is not configured".to_string()))?;
    Ok(Principal::from_slice(&bytes))
}

/// metaからasset indexと `szDecimals` を解決する（未取得・欠落はfail-closed）。
fn resolve_asset(market: &str) -> Result<(u32, u32), ErrorCode> {
    // network・dexは設定から解決する（固定値を埋め込まない）。未設定はfail-closed。
    let (network, dex) = db::tx::query(db::repo::core_config::market_context)
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let universe =
        db::tx::query(|connection| db::repo::meta::universe_json(connection, &network, &dex))
            .map_err(map_db)?
            .ok_or(ErrorCode::PolicyUnavailable)?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&universe).map_err(|error| internal(error.to_string()))?;
    for (index, entry) in entries.iter().enumerate() {
        if entry.get("name").and_then(|name| name.as_str()) == Some(market) {
            let index = u32::try_from(index).map_err(|_| internal("index overflow".to_string()))?;
            let sz_decimals = entry
                .get("szDecimals")
                .and_then(|value| value.as_u64())
                .ok_or(ErrorCode::PolicyUnavailable)?;
            let sz_decimals = u32::try_from(sz_decimals)
                .map_err(|_| internal("szDecimals overflow".to_string()))?;
            return Ok((index, sz_decimals));
        }
    }
    Err(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::AssetNotAllowed,
    })
}

/// SL/TPトリガの受付検証（`docs/phase-0/api-contract.md` 3.1）。
///
/// 建玉単位（`positionTpsl`）の保護注文としてのみ受け付ける。
/// - `reduce_only`必須（トリガで建玉を増やさない）。
/// - トリガ価格は正で、価格と同じ精度条件に収まること。
/// - 建玉が存在し、`side`がその建玉の反対売買であること（建玉の向きとの整合）。
///
/// 市場価格に対する上下（longのSLは下・TPは上）は検証しない。coreはmark価格を
/// 持たず、entry価格で代用すると含み益のある建玉の逆指値を誤って拒否する。
/// 上下の妥当性は取引所が最終的に判定する。
fn validate_trigger(
    account_id: &[u8; 32],
    market: &str,
    args: &api_types::order::SubmitOrderArgs,
    trigger: &api_types::order::Trigger,
    sz_decimals: u32,
) -> Result<(), ErrorCode> {
    if !args.reduce_only {
        return Err(bad(
            BadRequestCode::MissingField,
            "trigger orders must be reduce-only",
        ));
    }
    let trigger_price =
        hl_types::decimal::Decimal::parse(&trigger.trigger_price).map_err(bad_decimal)?;
    if trigger_price.as_str().starts_with('-') || trigger_price.as_str() == "0" {
        return Err(bad(
            BadRequestCode::PriceOutOfRange,
            "trigger price must be positive",
        ));
    }
    trigger_price
        .validate_precision(6u32.saturating_sub(sz_decimals), 5)
        .map_err(|error| bad(BadRequestCode::PrecisionExceeded, &error.to_string()))?;

    let position = open_position(account_id, market)?;
    let is_long = !position.size.starts_with('-');
    let is_buy = matches!(args.side, api_types::order::Side::Buy);
    if is_buy == is_long {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "trigger side must reduce the position",
        ));
    }
    Ok(())
}

/// テスト専用：注文actionへAgent鍵で署名する（`test-venue` featureでのみ存在）。
///
/// 戻り値は `(署名対象ダイジェスト, 65バイト署名)`。署名経路の検証に使う。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn test_sign_order_action(
    order_id: api_types::Blob,
) -> Result<(api_types::Blob, api_types::Blob), ErrorCode> {
    let order_id: [u8; 32] = order_id.as_ref().try_into().map_err(|_| {
        bad(
            BadRequestCode::MalformedPayload,
            "order_id must be 32 bytes",
        )
    })?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::orders::ensure_action_nonces(connection, &order_id, now))
        .map_err(map_db)?;
    let order = db::tx::query(|connection| db::repo::orders::signable(connection, &order_id))
        .map_err(map_db)?
        .ok_or_else(|| bad(BadRequestCode::MalformedPayload, "unknown order"))?;
    let price = order.price.clone().ok_or_else(|| {
        bad(
            BadRequestCode::MissingField,
            "price is required for signing",
        )
    })?;
    let action = pipeline::order_action(&order, &price)?;
    let msgpack = action.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &msgpack,
        nonce: order.order_nonce,
        vault_address: None,
        expires_after: order.expires_after,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);
    let signature = pipeline::sign_with_agent_key(&order.account_id, digest).await?;
    Ok((
        digest.to_vec().into(),
        signature.to_bytes65().to_vec().into(),
    ))
}

/// テスト専用：待ち注文を今すぐ送信する（`test-venue` featureでのみ存在）。
///
/// 本番の送信経路（`heartbeat`・`sweep`）と同じ`pipeline::sweep_once`を呼ぶ。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn test_sweep_now() -> Result<api_types::order::SweepOutcome, ErrorCode> {
    pipeline::sweep_once(ic_cdk::api::time() / 1_000_000).await
}

/// 未処理の注文・取消を送信し、取引所状態を照合する（controllerのみ）。
///
/// 本番は `heartbeat` が間隔を空けて呼ぶ。停止した場合の手動実行の入口でもある。
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn sweep() -> Result<api_types::order::SweepOutcome, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can sweep".to_string(),
        });
    }
    pipeline::sweep_once(ic_cdk::api::time() / 1_000_000).await
}

/// sweepの起動を予約する（本番のみ・5秒間隔）。
///
/// **heartbeatではなくグローバルtimer**を使う（heartbeatはメッセージが無くても
/// 毎ラウンド呼ばれ、アイドル時もコストが乗る）。timerはアップグレードで失われる
/// ため`init`と`post_upgrade`の両方で予約する。正しさは永続状態（`queued`／
/// `dispatching`）と手動`sweep`が担保し、timerの継続には依存しない
/// （`state-machines.md` 5節）。
#[cfg(not(feature = "test-venue"))]
fn schedule_sweep() {
    if SWEEP_TIMER.with(|timer| timer.borrow().is_some()) {
        return;
    }
    let timer_id = ic_cdk_timers::set_timer_interval_serial(
        core::time::Duration::from_millis(pipeline::SWEEP_INTERVAL_MS),
        async || {
            db::tx::with_optional_scope_future(
                cfg!(feature = "embedded").then_some(db::DbScope::Core),
                async {
                    // 1回の失敗でtimerを止めない（次の間隔で再試行する）。
                    let now = ic_cdk::api::time() / 1_000_000;
                    if let Err(error) = pipeline::sweep_once(now).await {
                        ic_cdk::println!("trading sweep failed: {error:?}");
                    }
                },
            )
            .await
        },
    );
    SWEEP_TIMER.with(|timer| *timer.borrow_mut() = Some(timer_id));
}

/// テスト専用：`/info`の`userFills`相当を取り込む（`test-venue`のみ）。
///
/// 本番は`heartbeat`の照合が取引所から取得する。同じ取り込み関数を使う。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn test_ingest_fills(
    session: api_types::auth::SessionHandle,
    fills_json: String,
) -> Result<u32, ErrorCode> {
    let user_id = authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    pipeline::ingest_fills_json(&user_id, &account_id, &fills_json, now).await
}

/// テスト専用：`/info`の`orderStatus`相当を反映する（`test-venue`のみ）。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn test_apply_order_status(
    session: api_types::auth::SessionHandle,
    status_json: String,
) -> Result<bool, ErrorCode> {
    authorize(&session).await?;
    // 本人の取引口座の注文だけを更新対象にする（oidは口座ごとに採番される）。
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    pipeline::apply_order_status_json(&account_id, &status_json, now).await
}

/// テスト専用：`clearinghouseState`相当の建玉を取り込む（`test-venue`のみ）。
///
/// 本番は`heartbeat`の照合が取引所から取得する。同じ取り込み関数を使う。
#[cfg(feature = "test-venue")]
#[scoped_entrypoint::update(scope = Core, prefix = "core_")]
async fn test_ingest_positions(
    session: api_types::auth::SessionHandle,
    positions_json: String,
) -> Result<u32, ErrorCode> {
    authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        pipeline::ingest_positions_json(connection, &account_id, &positions_json, now)
    })
    .map_err(map_db)
}

fn init_db() {
    if let Err(error) = if cfg!(feature = "embedded") {
        db::init_scoped(db::DbScope::Core, db::schema::core::MIGRATIONS)
    } else {
        db::init(MEMORY_ID, db::schema::core::MIGRATIONS)
    } {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::init)]
fn init() {
    init_db();
    #[cfg(not(feature = "test-venue"))]
    schedule_sweep();
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::post_upgrade)]
fn post_upgrade() {
    init_db();
    if let Err(error) = journal_client::lock() {
        ic_cdk::trap(format!("send journal lock failed: {error:?}"));
    }
    if let Err(error) =
        db::tx::update(|connection| db::repo::recovery_fences::set_migration_lock(connection, true))
    {
        ic_cdk::trap(format!("recovery migration lock failed: {error}"));
    }
    // グローバルtimerはアップグレードで失われるため予約し直す。
    #[cfg(not(feature = "test-venue"))]
    schedule_sweep();
}

#[cfg(feature = "embedded")]
pub fn embedded_init() {
    db::tx::with_scope(db::DbScope::Core, init);
}

#[cfg(feature = "embedded")]
pub fn embedded_post_upgrade() {
    db::tx::with_scope(db::DbScope::Core, post_upgrade);
}

#[cfg(not(feature = "embedded"))]
ic_cdk::export_candid!();

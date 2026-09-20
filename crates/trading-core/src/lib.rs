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

use api_types::auth::{SessionHandle, SessionStatus};
use api_types::error::{BadRequestCode, ErrorCode};
use candid::Principal;
use db::error::Error as DbError;
use ic_cdk::call::Call;

const MEMORY_ID: u8 = db::memory_id::TRADING_CORE_MAIN;

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

fn map_db(error: DbError) -> ErrorCode {
    match error {
        DbError::Sql(message) => internal(message),
        DbError::NotFound => internal("not configured".to_string()),
        DbError::Conflict => ErrorCode::ReservationConflict,
        DbError::Overflow => internal("integer overflow".to_string()),
        DbError::Invariant(message) => internal(message.to_string()),
        DbError::InsufficientFunds { .. } => internal("unexpected funds error".to_string()),
        DbError::StateConflict { .. } => ErrorCode::ReservationConflict,
    }
}

/// このビルドのバージョン。デプロイ確認用。
#[ic_cdk::query]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// vaultのprincipalを設定する（controllerのみ）。
#[ic_cdk::update]
fn set_vault_principal(vault: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the vault principal".to_string(),
        });
    }
    let bytes = vault.as_slice().to_vec();
    if bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid vault principal".to_string(),
        });
    }
    db::tx::update(|connection| db::repo::core_config::set_vault_principal(connection, &bytes))
        .map_err(map_db)
}

/// vaultのprincipal（診断用）。
#[ic_cdk::query]
fn get_vault_principal() -> Option<Principal> {
    db::tx::query(db::repo::core_config::vault_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// セッションを検証し、本人のuser_idを返す（認可境界の試験用）。
///
/// vaultに問い合わせ、返却されたprincipalが「このメッセージのcaller」と一致する場合だけ
/// user_idを返す。順序を逆にしない（callerを信用しない）。
#[ic_cdk::update]
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

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, db::schema::core::MIGRATIONS) {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[ic_cdk::init]
fn init() {
    init_db();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    init_db();
}

ic_cdk::export_candid!();

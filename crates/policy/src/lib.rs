//! `policy_registry` Canisterの雛形。
//!
//! 責務は国・規約版・検証鍵・緊急停止・allowlistの読み出しである
//! （`docs/phase-0/authority-matrix.md` 2節）。**読み取り失敗はfail-closed**とし、
//! 呼び出し側は新規受付・新規リスク増加を停止する。
//!
//! 緊急操作は停止方向のみを提供し、解除・制限緩和は記録したSNS経路で行う
//! （`docs/phase-0/api-contract.md` 5節）。任意送金・即時upgrade・出金先変更は提供しない。

use api_types::error::{BadRequestCode, ErrorCode};
use api_types::policy::{Policy, StopStatus};
use candid::Principal;
use db::error::Error as DbError;

const MEMORY_ID: u8 = db::memory_id::POLICY_REGISTRY;

/// allowlistの上限（銘柄数の暴発を防ぐ）。
const MAX_MARKETS: usize = 20;

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

fn map_db(error: DbError) -> ErrorCode {
    match error {
        DbError::Sql(message) => internal(message),
        DbError::NotFound => internal("policy is not configured".to_string()),
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

/// 現在の政策（allowlist）。未設定はエラー（fail-closed）。
#[ic_cdk::query]
fn get_policy() -> Result<Policy, ErrorCode> {
    db::tx::query(db::repo::policy::policy).map_err(map_db)
}

/// 停止状態（理由コードのみを公開する）。
#[ic_cdk::query]
fn get_stop_status() -> StopStatus {
    db::tx::query(db::repo::policy::stop_status).unwrap_or(StopStatus {
        stopped: true,
        reason: Some("policy_unavailable".to_string()),
        since: None,
    })
}

/// 運営principalを設定する（controllerのみ、初期化時）。
#[ic_cdk::update]
fn set_operator(operator: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the operator".to_string(),
        });
    }
    let bytes = operator.as_slice().to_vec();
    check_principal(&bytes, "invalid operator principal")?;
    db::tx::update(|connection| db::repo::policy::set_operator(connection, &bytes)).map_err(map_db)
}

/// 政策（版とallowlist）を設定する。本番ではcontrol_guard経由に限定する。
#[ic_cdk::update]
fn set_policy_version(version: u64, markets: Vec<String>) -> Result<(), ErrorCode> {
    require_operator(ic_cdk::api::msg_caller())?;
    if markets.is_empty() || markets.len() > MAX_MARKETS {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::TooLarge,
            detail: format!("markets must be 1..={MAX_MARKETS}"),
        });
    }
    db::tx::update(|connection| db::repo::policy::set_policy(connection, version, &markets))
        .map_err(map_db)
}

/// 緊急停止（停止方向のみ）。解除は同じ運営principalによる記録済み操作として扱う。
#[ic_cdk::update]
fn set_emergency_stop(stopped: bool) -> Result<(), ErrorCode> {
    require_operator(ic_cdk::api::msg_caller())?;
    let now = ic_cdk::api::time() / 1_000_000;
    let reason = if stopped {
        "operator_stop"
    } else {
        "operator_clear"
    };
    db::tx::update(|connection| db::repo::policy::set_stop(connection, stopped, Some(reason), now))
        .map_err(map_db)
}

fn check_principal(bytes: &[u8], what: &str) -> Result<(), ErrorCode> {
    if bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: what.to_string(),
        });
    }
    Ok(())
}

fn require_operator(caller: Principal) -> Result<(), ErrorCode> {
    let operator = db::tx::query(db::repo::policy::operator).map_err(map_db)?;
    match operator {
        Some(bytes) if bytes == caller.as_slice() => Ok(()),
        Some(_) => Err(ErrorCode::Unauthenticated {
            reason: "only the operator may change policy".to_string(),
        }),
        None => Err(internal("operator is not configured".to_string())),
    }
}

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, db::schema::policy::MIGRATIONS) {
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

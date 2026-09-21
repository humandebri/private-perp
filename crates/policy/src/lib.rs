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
        DbError::RiskLimitExceeded { limit } => ErrorCode::RiskLimitExceeded { limit },
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
    set_role_principal("operator", operator, "operator")
}

/// SNS governanceのprincipalを設定する（controllerのみ、初期化時）。
///
/// 緊急停止の**解除**はこのprincipalだけが行える（契約: 解除は記録したSNS経路）。
#[ic_cdk::update]
fn set_sns_principal(sns: Principal) -> Result<(), ErrorCode> {
    set_role_principal("sns", sns, "SNS governance")
}

/// `control_guard` のprincipalを設定する（controllerのみ、初期化時）。
///
/// 政策（版とallowlist）の変更はこのprincipalだけが行える（契約: control_guard経由のみ）。
#[ic_cdk::update]
fn set_guard_principal(guard: Principal) -> Result<(), ErrorCode> {
    set_role_principal("guard", guard, "control guard")
}

fn set_role_principal(role: &str, principal: Principal, what: &str) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: format!("only a controller can set the {what} principal"),
        });
    }
    let principal = check_principal(principal, &format!("invalid {what} principal"))?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::policy::set_role(connection, role, &principal, now))
        .map_err(map_db)
}

/// 役割別のprincipal（診断用）。
#[ic_cdk::query]
fn get_role_principal(role: String) -> Option<Principal> {
    db::tx::query(|connection| db::repo::policy::role(connection, &role))
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// 政策（版とallowlist）を設定する（`control_guard`のみ）。版は厳密に増加させる。
#[ic_cdk::update]
fn set_policy_version(version: u64, markets: Vec<String>) -> Result<(), ErrorCode> {
    require_role(
        ic_cdk::api::msg_caller(),
        "guard",
        "only the control guard may change policy",
    )?;
    if markets.is_empty() || markets.len() > MAX_MARKETS {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::TooLarge,
            detail: format!("markets must be 1..={MAX_MARKETS}"),
        });
    }
    let mut seen: Vec<&str> = Vec::new();
    for market in &markets {
        // 空・区切り文字入り・長すぎる銘柄を拒否する（保存はカンマ区切りのため）。
        if market.is_empty() || market.len() > 32 || market.contains(',') {
            return Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                detail: "each market must be 1..=32 characters without commas".to_string(),
            });
        }
        if seen.contains(&market.as_str()) {
            return Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                detail: "markets must not contain duplicates".to_string(),
            });
        }
        seen.push(market.as_str());
    }
    db::tx::update(|connection| db::repo::policy::set_policy(connection, version, &markets))
        .map_err(map_db)
}

/// 緊急停止（**停止方向のみ**）。解除は `clear_emergency_stop`（SNS経路）だけが行える。
#[ic_cdk::update]
fn set_emergency_stop() -> Result<(), ErrorCode> {
    require_role(
        ic_cdk::api::msg_caller(),
        "operator",
        "only the operator may stop the service",
    )?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::policy::set_stop(connection, true, Some("operator_stop"), now)
    })
    .map_err(map_db)
}

/// 緊急停止の解除（SNS governanceのみ）。即時の緩和を運営鍵では行えないようにする。
#[ic_cdk::update]
fn clear_emergency_stop() -> Result<(), ErrorCode> {
    require_role(
        ic_cdk::api::msg_caller(),
        "sns",
        "only the SNS governance principal may clear the stop",
    )?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::policy::set_stop(connection, false, Some("sns_clear"), now)
    })
    .map_err(map_db)
}

fn check_principal(principal: Principal, what: &str) -> Result<Vec<u8>, ErrorCode> {
    let bytes = principal.as_slice().to_vec();
    if principal == Principal::anonymous() || bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: what.to_string(),
        });
    }
    Ok(bytes)
}

fn require_role(caller: Principal, role: &str, reason: &str) -> Result<(), ErrorCode> {
    let principal =
        db::tx::query(|connection| db::repo::policy::role(connection, role)).map_err(map_db)?;
    match principal {
        Some(bytes) if bytes == caller.as_slice() => Ok(()),
        Some(_) => Err(ErrorCode::Unauthenticated {
            reason: reason.to_string(),
        }),
        None => Err(internal(format!("{role} principal is not configured"))),
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

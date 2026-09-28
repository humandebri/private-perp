//! `policy_registry` Canisterの雛形。
//!
//! 責務は国・規約版・検証鍵・緊急停止・allowlistの読み出しである
//! （`docs/phase-0/authority-matrix.md` 2節）。**読み取り失敗はfail-closed**とし、
//! 呼び出し側は新規受付・新規リスク増加を停止する。
//!
//! 緊急操作は停止方向のみを提供し、解除・制限緩和は記録したSNS経路で行う
//! （`docs/phase-0/api-contract.md` 5節）。任意送金・即時upgrade・出金先変更は提供しない。

use api_types::error::{BadRequestCode, ErrorCode};
use api_types::operations::{
    BudgetClass, MAX_REST_BUDGET_CAPACITY, RestBudgetConfig, RestBudgetRequest, RestBudgetStatus,
};
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
        DbError::WriterBusy => ErrorCode::JournalWriterBusy,
        DbError::Overflow => internal("integer overflow".to_string()),
        DbError::Invariant(message) => internal(message.to_string()),
        DbError::InsufficientFunds { .. } => internal("unexpected funds error".to_string()),
        DbError::RiskLimitExceeded { limit } => ErrorCode::RiskLimitExceeded { limit },
        DbError::StateConflict { .. } => ErrorCode::ReservationConflict,
    }
}

/// このビルドのバージョン。デプロイ確認用。
#[scoped_entrypoint::query(scope = Policy, prefix = "policy_")]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 共有予算の利用者を初期登録する。登録済みの別principalへの変更は拒否する。
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn register_budget_worker(role: String, principal: Principal) -> Result<(), ErrorCode> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(ErrorCode::Unauthenticated {
            reason: "controller required".into(),
        });
    }
    if role != "vault" && role != "core" {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "role must be vault or core".into(),
        });
    }
    let bytes = check_principal(principal, "invalid budget worker")?;
    db::tx::update(|c| db::repo::budget::register_worker(c, &role, &bytes)).map_err(map_db)
}

#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn configure_rest_budget(config: RestBudgetConfig) -> Result<(), ErrorCode> {
    require_role(ic_cdk::api::msg_caller(), "guard", "guard required")?;
    if !(4..=MAX_REST_BUDGET_CAPACITY).contains(&config.capacity)
        || config.exit_reserve == 0
        || config.exit_reserve >= config.capacity
    {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid REST budget".into(),
        });
    }
    db::tx::update(|c| db::repo::budget::configure(c, &config)).map_err(map_db)
}

/// 公開するのは集計のみ。口座・要求ID・callerは公開しない。
#[scoped_entrypoint::query(scope = Policy, prefix = "policy_")]
fn get_rest_budget_status() -> Result<RestBudgetStatus, ErrorCode> {
    db::tx::query(|c| db::repo::budget::status(c, ic_cdk::api::time() / 1_000_000)).map_err(map_db)
}

#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn consume_rest_budget(request: RestBudgetRequest) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !db::tx::query(|c| db::repo::budget::is_worker(c, caller.as_slice())).map_err(map_db)? {
        return Err(ErrorCode::Unauthenticated {
            reason: "registered budget worker required".into(),
        });
    }
    let now = ic_cdk::api::time() / 1_000_000;
    if !request.valid_at(now) {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid budget request".into(),
        });
    }
    // Emergency stop still permits read-only reconciliation and safe exits.
    if request.class == BudgetClass::NewRisk && get_stop_status().stopped {
        return Err(ErrorCode::PolicyUnavailable);
    }
    match db::tx::update(|c| db::repo::budget::consume(c, caller.as_slice(), &request, now))
        .map_err(|error| match error {
            DbError::NotFound => ErrorCode::PolicyUnavailable,
            other => map_db(other),
        })? {
        true => Ok(()),
        false => Err(ErrorCode::VenueRateLimited {
            retry_after_ms: Some(db::repo::budget::WINDOW_MS),
        }),
    }
}

/// 復旧停止はoperatorだけ。解除はSNS経路とし、controller向けの迂回口を設けない。
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn pause_for_recovery() -> Result<(), ErrorCode> {
    require_role(ic_cdk::api::msg_caller(), "operator", "operator required")?;
    db::tx::update(|c| db::repo::budget::pause(c, true)).map_err(map_db)
}

#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn clear_recovery_pause() -> Result<(), ErrorCode> {
    require_role(ic_cdk::api::msg_caller(), "sns", "SNS required")?;
    db::tx::update(|c| db::repo::budget::pause(c, false)).map_err(map_db)
}

/// 現在の政策（allowlist）。未設定はエラー（fail-closed）。
#[scoped_entrypoint::query(scope = Policy, prefix = "policy_")]
fn get_policy() -> Result<Policy, ErrorCode> {
    db::tx::query(db::repo::policy::policy).map_err(map_db)
}

/// 停止状態（理由コードのみを公開する）。
#[scoped_entrypoint::query(scope = Policy, prefix = "policy_")]
fn get_stop_status() -> StopStatus {
    db::tx::query(db::repo::policy::stop_status).unwrap_or(StopStatus {
        stopped: true,
        reason: Some("policy_unavailable".to_string()),
        since: None,
    })
}

/// 運営principalを設定する（controllerのみ、初期化時）。
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn set_operator(operator: Principal) -> Result<(), ErrorCode> {
    set_role_principal("operator", operator, "operator")
}

/// SNS governanceのprincipalを設定する（controllerのみ、初期化時）。
///
/// 緊急停止の**解除**はこのprincipalだけが行える（契約: 解除は記録したSNS経路）。
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn set_sns_principal(sns: Principal) -> Result<(), ErrorCode> {
    set_role_principal("sns", sns, "SNS governance")
}

/// `control_guard` のprincipalを設定する（controllerのみ、初期化時）。
///
/// 政策（版とallowlist）の変更はこのprincipalだけが行える（契約: control_guard経由のみ）。
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
fn set_guard_principal(guard: Principal) -> Result<(), ErrorCode> {
    set_role_principal("guard", guard, "control guard")
}

#[cfg(not(feature = "embedded"))]
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
#[scoped_entrypoint::query(scope = Policy, prefix = "policy_")]
fn get_role_principal(role: String) -> Option<Principal> {
    db::tx::query(|connection| db::repo::policy::role(connection, &role))
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// 政策（版とallowlist）を設定する（`control_guard`のみ）。版は厳密に増加させる。
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
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
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
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
#[scoped_entrypoint::update(scope = Policy, prefix = "policy_")]
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

#[cfg(not(feature = "embedded"))]
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
    #[cfg(feature = "embedded")]
    {
        let _ = (role, reason);
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
}

fn init_db() {
    if let Err(error) = if cfg!(feature = "embedded") {
        db::init_scoped(db::DbScope::Policy, db::schema::policy::MIGRATIONS)
    } else {
        db::init(MEMORY_ID, db::schema::policy::MIGRATIONS)
    } {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::init)]
fn init() {
    init_db();
}

#[cfg_attr(not(feature = "embedded"), ic_cdk::post_upgrade)]
fn post_upgrade() {
    init_db();
}

#[cfg(feature = "embedded")]
pub fn embedded_init() {
    db::tx::with_scope(db::DbScope::Policy, init);
}

#[cfg(feature = "embedded")]
pub fn embedded_post_upgrade() {
    db::tx::with_scope(db::DbScope::Policy, post_upgrade);
}

#[cfg(not(feature = "embedded"))]
ic_cdk::export_candid!();

//! `control_guard` Canister。
//!
//! 責務はSNS governanceからの変更予約、7日猶予、予約内容と一致するupgradeの実行である
//! （`docs/phase-0/api-contract.md` 4節、`docs/phase-0/authority-matrix.md` 4節）。
//!
//! 次を実装してはならない。
//!
//! - 顧客資金の署名（このCanisterは署名鍵を持たない）
//! - 顧客情報の保存
//! - 任意のmanagement call、controller追加・移管、reinstall、削除、単独停止、猶予短縮
//!
//! 予約内容（対象・wasm hash・引数hash）の変更は取消＋新規予約とし、新しい7日を開始する。

mod clock;

use api_types::error::{ErrorCode, NotAllowedCode};
use api_types::guard::{
    ScheduleUpgradeArgs, ScheduledUpgrade, UPGRADE_DELAY_MS, UpgradeRequest, UpgradeState,
    UpgradeStatus,
};
use candid::Principal;
use db::error::Error as DbError;
use ic_cdk_management_canister::{CanisterInstallMode, InstallCodeArgs, install_code};
use sha2::{Digest, Sha256};

const MEMORY_ID: u8 = db::memory_id::CONTROL_GUARD_MAIN;

fn not_allowed(code: NotAllowedCode) -> ErrorCode {
    ErrorCode::NotAllowed { code }
}

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

fn map_db(error: DbError) -> ErrorCode {
    match error {
        DbError::Sql(message) => internal(message),
        DbError::NotFound => not_allowed(NotAllowedCode::UpgradeNotScheduled),
        DbError::Conflict => ErrorCode::ReservationConflict,
        DbError::Overflow => internal("integer overflow".to_string()),
        DbError::Invariant(message) => internal(message.to_string()),
        DbError::InsufficientFunds { .. } => internal("unexpected funds error".to_string()),
        DbError::StateConflict { .. } => not_allowed(NotAllowedCode::UpgradeAlreadyExecuted),
    }
}

/// Principalのバイト列表現を検証する（1〜29バイト）。
fn check_principal(bytes: &[u8], what: &str) -> Result<(), ErrorCode> {
    if bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: what.to_string(),
        });
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn to_fixed<const N: usize>(bytes: &[u8], what: &str) -> Result<[u8; N], ErrorCode> {
    bytes.try_into().map_err(|_| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: what.to_string(),
    })
}

/// このビルドのバージョン。デプロイ確認用。
#[ic_cdk::query]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// SNS governanceのPrincipalを設定する（controllerのみ、初期化時）。
#[ic_cdk::update]
fn set_sns_principal(principal: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the SNS principal".to_string(),
        });
    }
    let principal = {
        let bytes = principal.as_slice().to_vec();
        check_principal(&bytes, "principal")?;
        bytes
    };
    db::tx::update(|connection| db::repo::guard::set_sns_principal(connection, &principal))
        .map_err(map_db)
}

/// SNS governanceのPrincipal（診断用）。
#[ic_cdk::query]
fn get_sns_principal() -> Option<Principal> {
    db::tx::query(db::repo::guard::sns_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// 変更を予約する（SNS governanceのみ）。7日後にだけ実行できる。
#[ic_cdk::update]
fn schedule_upgrade(args: ScheduleUpgradeArgs) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let now = clock::now_ms();

    let wasm_hash = to_fixed::<32>(&args.request.wasm_hash, "wasm_hash must be 32 bytes")?;
    let arg_hash = to_fixed::<32>(&args.request.arg_hash, "arg_hash must be 32 bytes")?;
    let target = {
        let bytes = args.request.target.as_slice().to_vec();
        check_principal(&bytes, "invalid target principal")?;
        bytes
    };

    require_sns(caller)?;
    db::tx::update(|connection| {
        db::repo::guard::insert_upgrade(
            connection,
            &target,
            &wasm_hash,
            &arg_hash,
            now,
            now.saturating_add(UPGRADE_DELAY_MS),
        )
    })
    .map_err(map_db)
}

/// 予約を取り消す（SNS governanceのみ）。次の予約は新しい7日を開始する。
#[ic_cdk::update]
fn cancel_upgrade() -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let now = clock::now_ms();
    require_sns(caller)?;
    db::tx::update(|connection| {
        let cancelled = db::repo::guard::cancel_active(connection, now)?;
        if !cancelled {
            return Err(DbError::NotFound);
        }
        Ok(())
    })
    .map_err(map_db)
}

/// 呼び出し元が設定済みのSNS governance principalであることを要求する。
fn require_sns(caller: Principal) -> Result<(), ErrorCode> {
    let sns = db::tx::query(db::repo::guard::sns_principal)
        .map_err(map_db)?
        .ok_or_else(|| internal("SNS principal is not configured".to_string()))?;
    if sns != caller.as_slice() {
        return Err(ErrorCode::Unauthenticated {
            reason: "only the SNS governance principal may reserve changes".to_string(),
        });
    }
    Ok(())
}

/// 予約済みの内容と一致するupgradeを実行する（実行者は誰でもよい）。
#[ic_cdk::update]
async fn execute_upgrade(
    target: Principal,
    wasm_module: Vec<u8>,
    arg: Vec<u8>,
) -> Result<(), ErrorCode> {
    let now = clock::now_ms();
    let target_bytes = {
        let bytes = target.as_slice().to_vec();
        check_principal(&bytes, "invalid target principal")?;
        bytes
    };

    let reserved = db::tx::query(db::repo::guard::active_upgrade)
        .map_err(map_db)?
        .ok_or(not_allowed(NotAllowedCode::UpgradeNotScheduled))?;

    if reserved.target != target_bytes {
        return Err(not_allowed(NotAllowedCode::UpgradeNotScheduled));
    }
    if now < reserved.executable_at {
        return Err(not_allowed(NotAllowedCode::UpgradeTooEarly));
    }
    if sha256(&wasm_module) != reserved.wasm_hash || sha256(&arg) != reserved.arg_hash {
        return Err(not_allowed(NotAllowedCode::UpgradeContentMismatch));
    }

    // `install_code`はawaitするため、その前に実行権をCASで確定する（同時呼び出しで
    // 二重にインストールしない）。
    let claimed = db::tx::update(|connection| {
        db::repo::guard::claim_upgrade(connection, reserved.upgrade_id)
    })
    .map_err(map_db)?;
    if !claimed {
        return Err(not_allowed(NotAllowedCode::UpgradeAlreadyExecuted));
    }

    install_code(&InstallCodeArgs {
        mode: CanisterInstallMode::Upgrade(None),
        canister_id: target,
        wasm_module,
        arg,
    })
    .await
    .map_err(|error| internal(format!("install_code failed: {error}")))?;

    db::tx::update(|connection| {
        db::repo::guard::mark_executed(connection, reserved.upgrade_id, clock::now_ms())
    })
    .map_err(map_db)?;
    Ok(())
}

/// 予約状況（公開）。
#[ic_cdk::query]
fn get_upgrade_status() -> UpgradeStatus {
    let active = db::tx::query(db::repo::guard::active_upgrade)
        .ok()
        .flatten();
    let scheduled = active.map(|row| ScheduledUpgrade {
        request: UpgradeRequest {
            target: Principal::from_slice(&row.target),
            wasm_hash: row.wasm_hash.to_vec().into(),
            arg_hash: row.arg_hash.to_vec().into(),
        },
        scheduled_at: row.scheduled_at,
        executable_at: row.executable_at,
        // 猶予の経過は保存状態ではなく時刻から判定して返す（予約行は書き換えない）。
        state: match row.state.as_str() {
            "executable" => UpgradeState::Executable,
            "executed" => UpgradeState::Executed,
            "cancelled" => UpgradeState::Cancelled,
            _ if clock::now_ms() >= row.executable_at => UpgradeState::Executable,
            _ => UpgradeState::Pending,
        },
    });
    UpgradeStatus {
        scheduled,
        guard_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, db::schema::guard::MIGRATIONS) {
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

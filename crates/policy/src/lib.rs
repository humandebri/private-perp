//! `policy_registry` Canisterの雛形。
//!
//! 責務は国・規約版・検証鍵・緊急停止・allowlistの読み出しである
//! （`docs/phase-0/authority-matrix.md` 2節）。読み取り失敗はfail-closedとし、
//! 新規受付・新規リスク増加を停止する。
//!
//! Phase 0 では `version` とDB初期化のみを実装する。実データと
//! APIはPhase 1以降（`docs/phase-0/api-contract.md` 5節、Phase 3-5）。

const MEMORY_ID: u8 = db::memory_id::POLICY_REGISTRY;

/// このビルドのバージョン。デプロイ確認用。
#[ic_cdk::query]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, &[]) {
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

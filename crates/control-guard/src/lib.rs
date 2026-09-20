//! `control_guard` Canisterの雛形。
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
//! Phase 0 では `version` とDB初期化のみを実装する。予約・猶予の実装はPhase 1（1-10）。

const MEMORY_ID: u8 = db::memory_id::CONTROL_GUARD_MAIN;

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

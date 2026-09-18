//! `funds_vault` Canisterの雛形。
//!
//! 責務はEOA認証、共通保管口座とユーザー別取引口座のmaster鍵管理、複式台帳、
//! 資金要求・予約・outbox、Agent承認・失効である
//! （`docs/phase-0/authority-matrix.md` 2節、`docs/phase-0/api-contract.md` 2節）。
//!
//! **任意のダイジェストへの署名APIを追加しない。** 資金移動は目的・金額・宛先・
//! 本人認可・残高を検証してから行う。
//!
//! Phase 0 では `version` とDB初期化のみを実装する。認証・台帳・署名はPhase 1以降。

const MEMORY_ID: u8 = db::memory_id::FUNDS_VAULT_MAIN;

/// このビルドのバージョン。デプロイ確認用。
#[ic_cdk::query]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID) {
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

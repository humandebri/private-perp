//! PocketICハーネスの成立確認（P1-005）。
//!
//! 4つのCanisterをdeployし、`version` queryが応答することと、`#[ic_cdk::init]` の
//! DB初期化がtrapしないことを確認する。

use pocket_ic_tests::{CONTROL_GUARD_WASM, FUNDS_VAULT_WASM, POLICY_WASM, TRADING_CORE_WASM};
use pocket_ic_tests::{deploy_default, pic, principal, query};

#[test]
fn canisters_report_version() {
    let pic = pic();
    let caller = principal(1);

    for wasm in [
        POLICY_WASM,
        FUNDS_VAULT_WASM,
        CONTROL_GUARD_WASM,
        TRADING_CORE_WASM,
    ] {
        let canister = deploy_default(&pic, wasm);
        let version: String = query(&pic, canister, caller, "version", ()).expect("version query");
        assert_eq!(version, env!("CARGO_PKG_VERSION"), "{wasm} のversion");
    }
}

//! `policy_registry` のfail-closedと緊急停止の試験（`docs/phase-0/api-contract.md` 5節）。

use api_types::error::ErrorCode;
use api_types::policy::{Policy, StopStatus};
use candid::Principal;
use pocket_ic_tests::{POLICY_WASM, deploy, pic, principal, query, update, update_args};

fn set_operator(
    pic: &pocket_ic::PocketIc,
    policy: Principal,
    caller: Principal,
    operator: Principal,
) -> Result<(), ErrorCode> {
    update(pic, policy, caller, "set_operator", operator).expect("call")
}

fn set_role(
    pic: &pocket_ic::PocketIc,
    policy: Principal,
    caller: Principal,
    method: &str,
    principal: Principal,
) -> Result<(), ErrorCode> {
    update(pic, policy, caller, method, principal).expect("call")
}

#[test]
fn policy_is_fail_closed_until_configured() {
    let pic = pic();
    let controller = principal(60);
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    // 政策未設定: allowlistはエラー、停止状態は安全側（停止）として扱う。
    let unset: Result<Policy, ErrorCode> =
        query(&pic, policy, principal(61), "get_policy", ()).expect("call");
    assert!(unset.is_err(), "fail-closed: {unset:?}");

    let status: StopStatus =
        query(&pic, policy, principal(61), "get_stop_status", ()).expect("call");
    assert!(status.stopped);
    assert_eq!(status.reason.as_deref(), Some("policy_not_configured"));
}

#[test]
fn only_the_operator_can_stop_and_the_guard_can_configure() {
    let pic = pic();
    let controller = principal(62);
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let operator = principal(63);
    let guard = principal(67);
    let sns = principal(68);

    // controllerが役割を設定できる（運営・guard・SNS）。
    set_operator(&pic, policy, controller, operator).expect("set_operator");
    set_role(&pic, policy, controller, "set_guard_principal", guard).expect("set_guard_principal");
    set_role(&pic, policy, controller, "set_sns_principal", sns).expect("set_sns_principal");

    // 政策の変更はguardのみ（運営鍵では変更できない）。
    let denied: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        principal(64),
        "set_policy_version",
        (1u64, vec!["BTC".to_string()]),
    )
    .expect("call");
    let denied = denied.expect_err("non-guard must be rejected");
    assert!(
        matches!(denied, ErrorCode::Unauthenticated { .. }),
        "{denied:?}"
    );

    // 停止は運営のみ。guardは停止できない。
    let denied_stop: Result<(), ErrorCode> =
        update(&pic, policy, guard, "set_emergency_stop", ()).expect("call");
    let denied_stop = denied_stop.expect_err("non-operator must be rejected");
    assert!(matches!(denied_stop, ErrorCode::Unauthenticated { .. }));

    // guardは政策を設定できる。版は厳密に増加させる。
    let set: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        guard,
        "set_policy_version",
        (1u64, vec!["BTC".to_string(), "ETH".to_string()]),
    )
    .expect("call");
    set.expect("set_policy_version");

    let configured: Result<Policy, ErrorCode> =
        query(&pic, policy, principal(65), "get_policy", ()).expect("call");
    let configured = configured.expect("policy");
    assert_eq!(configured.version, 1);
    assert_eq!(
        configured.markets,
        vec!["BTC".to_string(), "ETH".to_string()]
    );

    // 空のallowlist、区切り文字入り、重複、版の巻き戻しは拒否する。
    let empty: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        guard,
        "set_policy_version",
        (2u64, Vec::<String>::new()),
    )
    .expect("call");
    assert!(empty.is_err());
    let comma: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        guard,
        "set_policy_version",
        (2u64, vec!["BTC,ETH".to_string()]),
    )
    .expect("call");
    assert!(comma.is_err());
    let rollback: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        guard,
        "set_policy_version",
        (1u64, vec!["BTC".to_string()]),
    )
    .expect("call");
    assert!(rollback.is_err(), "版の巻き戻しは拒否する");

    // 停止は運営、解除はSNSのみ（運営鍵では即時緩和できない）。
    let stop: Result<(), ErrorCode> =
        update(&pic, policy, operator, "set_emergency_stop", ()).expect("call");
    stop.expect("stop");
    let stopped: StopStatus =
        query(&pic, policy, principal(66), "get_stop_status", ()).expect("call");
    assert!(stopped.stopped);
    assert_eq!(stopped.reason.as_deref(), Some("operator_stop"));

    let operator_clear: Result<(), ErrorCode> =
        update(&pic, policy, operator, "clear_emergency_stop", ()).expect("call");
    assert!(
        operator_clear.is_err(),
        "運営鍵による解除は拒否する: {operator_clear:?}"
    );

    let clear: Result<(), ErrorCode> =
        update(&pic, policy, sns, "clear_emergency_stop", ()).expect("call");
    clear.expect("clear");
    let cleared: StopStatus =
        query(&pic, policy, principal(66), "get_stop_status", ()).expect("call");
    assert!(!cleared.stopped);
    assert_eq!(cleared.reason.as_deref(), Some("sns_clear"));
}

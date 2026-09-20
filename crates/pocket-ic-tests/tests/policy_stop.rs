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
fn only_the_operator_can_stop_and_configure() {
    let pic = pic();
    let controller = principal(62);
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let operator = principal(63);

    // controllerが運営を設定できる。
    set_operator(&pic, policy, controller, operator).expect("set_operator");

    // operator以外の設定・停止は拒否する。
    let denied: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        principal(64),
        "set_policy_version",
        (1u64, vec!["BTC".to_string()]),
    )
    .expect("call");
    let denied = denied.expect_err("non-operator must be rejected");
    assert!(
        matches!(denied, ErrorCode::Unauthenticated { .. }),
        "{denied:?}"
    );
    let denied_stop: Result<(), ErrorCode> =
        update(&pic, policy, principal(64), "set_emergency_stop", true).expect("call");
    let denied_stop = denied_stop.expect_err("non-operator must be rejected");
    assert!(matches!(denied_stop, ErrorCode::Unauthenticated { .. }));

    // operatorは政策を設定できる。
    let set: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        operator,
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

    // 空のallowlistは拒否する。
    let empty: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        operator,
        "set_policy_version",
        (2u64, Vec::<String>::new()),
    )
    .expect("call");
    assert!(empty.is_err());

    // 停止と解除は運営の判断として記録される。
    let stop: Result<(), ErrorCode> =
        update(&pic, policy, operator, "set_emergency_stop", true).expect("call");
    stop.expect("stop");
    let stopped: StopStatus =
        query(&pic, policy, principal(66), "get_stop_status", ()).expect("call");
    assert!(stopped.stopped);
    assert_eq!(stopped.reason.as_deref(), Some("operator_stop"));

    let clear: Result<(), ErrorCode> =
        update(&pic, policy, operator, "set_emergency_stop", false).expect("call");
    clear.expect("clear");
    let cleared: StopStatus =
        query(&pic, policy, principal(66), "get_stop_status", ()).expect("call");
    assert!(!cleared.stopped);
}

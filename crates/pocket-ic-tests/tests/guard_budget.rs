use api_types::error::ErrorCode;
use api_types::operations::{RestBudgetConfig, RestBudgetStatus};
use api_types::policy::Policy;
use candid::encode_one;
use pocket_ic_tests::{
    CONTROL_GUARD_WASM, POLICY_WASM, deploy, pic, principal, query, update, update_args,
};

#[test]
fn only_sns_can_configure_budget_through_the_registered_guard() {
    let pic = pic();
    let controller = principal(210);
    let sns = principal(211);
    let outsider = principal(212);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![controller]),
        encode_one(()).unwrap(),
    );
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![controller]),
        encode_one(()).unwrap(),
    );
    let set_sns: Result<(), ErrorCode> =
        update(&pic, guard, controller, "set_sns_principal", sns).unwrap();
    set_sns.unwrap();
    let set_guard: Result<(), ErrorCode> =
        update(&pic, policy, controller, "set_guard_principal", guard).unwrap();
    set_guard.unwrap();

    let config = RestBudgetConfig {
        capacity: 1200,
        exit_reserve: 300,
    };
    let direct: Result<(), ErrorCode> =
        update(&pic, policy, sns, "configure_rest_budget", config.clone()).unwrap();
    assert!(matches!(direct, Err(ErrorCode::Unauthenticated { .. })));
    let wrong: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        outsider,
        "configure_rest_budget",
        (policy, config.clone()),
    )
    .unwrap();
    assert!(matches!(wrong, Err(ErrorCode::Unauthenticated { .. })));
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_rest_budget",
        (policy, config.clone()),
    )
    .unwrap();
    configured.unwrap();
    let status: Result<RestBudgetStatus, ErrorCode> =
        query(&pic, policy, outsider, "get_rest_budget_status", ()).unwrap();
    assert_eq!(status.unwrap().config, Some(config));

    let markets = vec!["BTC".to_string(), "ETH".to_string()];
    let direct_policy: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        sns,
        "set_policy_version",
        (1_u64, markets.clone()),
    )
    .unwrap();
    assert!(matches!(
        direct_policy,
        Err(ErrorCode::Unauthenticated { .. })
    ));
    let outsider_policy: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        outsider,
        "configure_policy_version",
        (policy, 1_u64, markets.clone()),
    )
    .unwrap();
    assert!(matches!(
        outsider_policy,
        Err(ErrorCode::Unauthenticated { .. })
    ));
    let set_policy: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_policy_version",
        (policy, 1_u64, markets.clone()),
    )
    .unwrap();
    set_policy.unwrap();
    let policy_state: Result<Policy, ErrorCode> =
        query(&pic, policy, outsider, "get_policy", ()).unwrap();
    assert_eq!(policy_state.unwrap().markets, markets);
}

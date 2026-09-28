use api_types::error::ErrorCode;
use api_types::operations::RestBudgetStatus;
use api_types::operations_status::{MarketStatus, MarketThreshold};
use pocket_ic_tests::{
    CONTROL_GUARD_WASM, POLICY_WASM, TRADING_CORE_WASM, call_with_routed_outcalls, deploy, pic,
    principal, query, update, update_args,
};

fn configure(
    pic: &pocket_ic::PocketIc,
    core: candid::Principal,
    guard: candid::Principal,
    sns: candid::Principal,
) {
    for (market, index, depth) in [("BTC", 2, 10_000), ("ETH", 1, 1_000)] {
        let input = MarketThreshold {
            market: market.into(),
            expected_index: index,
            min_day_notional_usdc: 1_000_000,
            max_spread_bps: 20,
            min_each_side_depth_usdc: depth,
        };
        let result: Result<(), ErrorCode> =
            update_args(pic, guard, sns, "configure_market_threshold", (core, input)).unwrap();
        result.unwrap();
    }
}

#[test]
fn market_monitor_accounts_24_weight_and_closes_on_index_change() {
    let pic = pic();
    let sns = principal(201);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, core, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    configure(&pic, core, guard, sns);
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        sns,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .unwrap();
    configured.unwrap();
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    for method in ["set_operator", "set_sns_principal", "set_guard_principal"] {
        let configured: Result<(), ErrorCode> = update(&pic, policy, sns, method, sns).unwrap();
        configured.unwrap();
    }
    let registered: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        sns,
        "register_budget_worker",
        ("core".to_string(), core),
    )
    .unwrap();
    registered.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, core, sns, "set_policy_principal", policy).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> = update(
        &pic,
        policy,
        sns,
        "configure_rest_budget",
        api_types::operations::RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .unwrap();
    configured.unwrap();
    // Real HL rolling volumes can have more than six fractional digits.
    let meta = br#"[{"universe":[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]},[{"dayNtlVlm":"10000000"},{"dayNtlVlm":"1125741.0633699989"},{"dayNtlVlm":"2945429.7254399993"}]]"#;
    let (result, calls): (Result<(), ErrorCode>, _) = call_with_routed_outcalls(&pic, core, sns, "refresh_market", (), |call| {
        let query: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
        match query.get("type").and_then(|v| v.as_str()) {
            Some("metaAndAssetCtxs") => Ok((200, meta.to_vec())),
            Some("l2Book") => {
                let coin = query.get("coin").and_then(|v| v.as_str()).unwrap();
                let mid = if coin == "BTC" { 60000 } else { 3000 };
                Ok((200, format!(r#"{{"levels":[[{{"px":"{}","sz":"1.25"}}],[{{"px":"{}","sz":"1.10"}}]]}}"#, mid-1, mid+1).into_bytes()))
            }
            other => Err((1, format!("unexpected {other:?}"))),
        }
    }).unwrap();
    result.unwrap();
    assert_eq!(calls.len(), 3);
    let btc: Result<MarketStatus, ErrorCode> =
        query(&pic, core, sns, "get_market_status", "BTC".to_string()).unwrap();
    assert!(btc.unwrap().eligible_for_new_risk);
    let status: Result<RestBudgetStatus, ErrorCode> =
        query(&pic, policy, sns, "get_rest_budget_status", ()).unwrap();
    assert_eq!(status.unwrap().used, 24);
    let changed: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_market_threshold",
        (
            core,
            MarketThreshold {
                market: "BTC".into(),
                expected_index: 3,
                min_day_notional_usdc: 1_000_000,
                max_spread_bps: 20,
                min_each_side_depth_usdc: 10_000,
            },
        ),
    )
    .unwrap();
    changed.unwrap();
    // Existing observation cannot certify a newly configured expectation.
    let btc: Result<MarketStatus, ErrorCode> =
        query(&pic, core, sns, "get_market_status", "BTC".to_string()).unwrap();
    assert!(!btc.unwrap().eligible_for_new_risk);
}

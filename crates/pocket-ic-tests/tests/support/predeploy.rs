use api_types::error::ErrorCode;
use candid::Principal;
use pocket_ic::{
    PocketIc,
    common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse},
};
use pocket_ic_tests::{update, update_args};
use serde_json::json;
use std::collections::BTreeMap;

/// Ten simulated minutes, with one prepared user and an empty HL account.
/// PocketIC fees are a local comparison, not a reconstruction of live billing.
pub fn profile(pic: &PocketIc, canister: Principal, admin: Principal) {
    let result: Result<(), ErrorCode> = update(
        pic,
        canister,
        Principal::anonymous(),
        "set_network",
        "testnet".to_string(),
    )
    .unwrap();
    result.unwrap();
    let result: Result<(), ErrorCode> = update_args(
        pic,
        canister,
        Principal::anonymous(),
        "set_market_context",
        ("testnet".to_string(), "hyperliquid".to_string()),
    )
    .unwrap();
    result.unwrap();
    for (market, index) in [("BTC", 0), ("ETH", 1)] {
        let result: Result<(), ErrorCode> = update(
            pic,
            canister,
            admin,
            "core_configure_market_threshold",
            api_types::operations_status::MarketThreshold {
                market: market.into(),
                expected_index: index,
                min_day_notional_usdc: 1_000_000,
                max_spread_bps: 20,
                min_each_side_depth_usdc: 1000,
            },
        )
        .unwrap();
        result.unwrap();
    }
    let failing = std::env::var("PRIVATE_PERP_PROFILE").unwrap() == "failure";
    let before = pic.cycle_balance(canister);
    let mut calls = BTreeMap::<String, usize>::new();
    for _ in 0..120 {
        pic.advance_time(std::time::Duration::from_secs(5));
        for _ in 0..50 {
            pic.tick();
            for request in pic.get_canister_http() {
                assert!(
                    request.url.ends_with("/info"),
                    "idle fixture must never send funds/orders"
                );
                let query: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let kind = query["type"].as_str().unwrap();
                *calls.entry(kind.to_string()).or_default() += 1;
                let body = match kind {
                    "metaAndAssetCtxs" => json!([{"universe":[{"name":"BTC"},{"name":"ETH"}]},
                        [{"dayNtlVlm":"2000000"},{"dayNtlVlm":"2000000"}]]),
                    "l2Book" => json!({"coin":query["coin"],
                        "time":pic.get_time().as_nanos_since_unix_epoch()/1_000_000,
                        "levels":[[{"px":"59999","sz":"1"}],[{"px":"60001","sz":"1"}]]}),
                    "clearinghouseState" => json!({"assetPositions":[],
                        "marginSummary":{"accountValue":"0","totalMarginUsed":"0","totalNtlPos":"0","totalRawUsd":"0"},
                        "withdrawable":"0"}),
                    "userFillsByTime" | "userNonFundingLedgerUpdates" | "openOrders" => json!([]),
                    other => panic!("unexpected idle request {other}"),
                };
                pic.mock_canister_http_response(MockCanisterHttpResponse {
                    subnet_id: request.subnet_id,
                    request_id: request.request_id,
                    response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                        status: if failing { 503 } else { 200 },
                        headers: vec![],
                        body: if failing {
                            b"unavailable".to_vec()
                        } else {
                            body.to_string().into_bytes()
                        },
                    }),
                    additional_responses: vec![],
                });
            }
        }
    }
    assert!(pic.get_canister_http().is_empty());
    if std::env::var_os("PRIVATE_PERP_EXPECT_IDLE").is_some() {
        assert!(
            calls.is_empty(),
            "idle canister must not poll external APIs: {calls:?}"
        );
        assert!(
            before - pic.cycle_balance(canister) < 100_000_000,
            "idle timers must stop, including callback traps"
        );
    }
    println!(
        "PREDEPLOY_PROFILE {}",
        json!({
            "case":if failing {"failure"} else {"healthy"},
            "seconds":600, "cycles":before-pic.cycle_balance(canister), "calls":calls,
        })
    );
}

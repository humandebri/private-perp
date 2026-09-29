use candid::{Principal, encode_args};
use ic_cdk_management_canister::{HttpHeader, HttpRequestResult, TransformArgs};
use pocket_ic_tests::{principal, query};
use serde_json::{Value, json};

#[test]
fn market_reads_are_non_replicated_and_reject_invalid_observations() {
    use api_types::{
        error::ErrorCode,
        operations_status::{MarketStatus, MarketThreshold},
    };
    use pocket_ic::common::rest::{
        CanisterHttpReplication, CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse,
    };
    use pocket_ic_tests::update;
    let pic = pocket_ic_tests::pic();
    let canister = pic.create_canister();
    let admin = principal(51);
    pic.add_cycles(canister, 10_000_000_000_000_000);
    pic.install_canister(
        canister,
        std::fs::read(std::env::var("PRIVATE_PERP_UNIFIED_WASM").unwrap()).unwrap(),
        encode_args((admin,)).unwrap(),
        None,
    );
    let configured: Result<(), ErrorCode> = update(
        &pic,
        canister,
        admin,
        "policy_configure_rest_budget",
        api_types::operations::RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .unwrap();
    configured.unwrap();
    for case in ["safe", "volume_low", "all_malformed", "stale_book"] {
        for (market, expected_index) in [("BTC", 0), ("ETH", 1)] {
            let configured: Result<(), ErrorCode> = update(
                &pic,
                canister,
                admin,
                "core_configure_market_threshold",
                MarketThreshold {
                    market: market.into(),
                    expected_index,
                    min_day_notional_usdc: 1_000_000,
                    max_spread_bps: 20,
                    min_each_side_depth_usdc: 1000,
                },
            )
            .unwrap();
            configured.unwrap();
        }
        let before = pic.cycle_balance(canister);
        let id = pic
            .submit_call(
                canister,
                Principal::anonymous(),
                "refresh_market",
                encode_args(()).unwrap(),
            )
            .unwrap();
        let mut count = 0;
        for _ in 0..200 {
            pic.tick();
            for request in pic.get_canister_http() {
                assert_eq!(request.replication, CanisterHttpReplication::NonReplicated);
                count += 1;
                let q: Value = serde_json::from_slice(&request.body).unwrap();
                let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000;
                let body = if q["type"] == "metaAndAssetCtxs" {
                    let volume = if case == "volume_low" {
                        "999999"
                    } else {
                        "2000000"
                    };
                    json!([{"universe":[{"name":"BTC"},{"name":"ETH"}]},
                        [{"dayNtlVlm":volume},{"dayNtlVlm":volume}]])
                } else {
                    json!({"coin":q["coin"],"time":if case=="stale_book" {now-120_000} else {now},
                        "levels":[[{"px":"59999","sz":"1"}],[{"px":"60001","sz":"1"}]]})
                };
                let response = CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status: 200,
                    headers: vec![],
                    body: if case == "all_malformed" {
                        b"malformed".to_vec()
                    } else {
                        body.to_string().into_bytes()
                    },
                });
                pic.mock_canister_http_response(MockCanisterHttpResponse {
                    subnet_id: request.subnet_id,
                    request_id: request.request_id,
                    response,
                    additional_responses: vec![],
                });
            }
            if pic.ingress_status(id.clone()).is_some() {
                break;
            }
        }
        assert!(pic.ingress_status(id.clone()).is_some(), "{case} timed out");
        let result: Result<(), ErrorCode> =
            candid::decode_one(&pic.await_call(id).unwrap()).unwrap();
        let status: Result<MarketStatus, ErrorCode> = query(
            &pic,
            canister,
            admin,
            "get_market_status",
            "BTC".to_string(),
        )
        .unwrap();
        let allowed = case == "safe";
        assert_eq!(
            status.unwrap().eligible_for_new_risk,
            allowed,
            "{case}: {result:?}"
        );
        result.unwrap();
        println!(
            "MARKET_NON_REPLICATED {}",
            json!({"case":case,"requests":count,
            "cycles":before-pic.cycle_balance(canister)})
        );
    }
}

#[test]
fn market_poll_rechecks_configuration_and_fails_closed_on_http_errors() {
    use api_types::{
        error::ErrorCode,
        operations_status::{MarketStatus, MarketThreshold},
    };
    use pocket_ic_tests::{call_with_routed_outcalls, update};
    let pic = pocket_ic_tests::pic();
    let admin = principal(51);
    let canister = pic.create_canister();
    pic.add_cycles(canister, 10_000_000_000_000_000);
    pic.install_canister(
        canister,
        std::fs::read(std::env::var("PRIVATE_PERP_UNIFIED_WASM").unwrap()).unwrap(),
        encode_args((admin,)).unwrap(),
        None,
    );
    let result: Result<(), ErrorCode> = update(
        &pic,
        canister,
        admin,
        "policy_configure_rest_budget",
        api_types::operations::RestBudgetConfig {
            capacity: 1200,
            exit_reserve: 300,
        },
    )
    .unwrap();
    result.unwrap();
    let threshold = |market: &str, index: u32, volume: u64| MarketThreshold {
        market: market.into(),
        expected_index: index,
        min_day_notional_usdc: volume,
        max_spread_bps: 20,
        min_each_side_depth_usdc: 1000,
    };
    let configure = |t: MarketThreshold| {
        let result: Result<(), ErrorCode> =
            update(&pic, canister, admin, "core_configure_market_threshold", t).unwrap();
        result.unwrap();
    };
    let status = |market: &str| {
        let result: Result<MarketStatus, ErrorCode> = query(
            &pic,
            canister,
            admin,
            "get_market_status",
            market.to_string(),
        )
        .unwrap();
        result.unwrap()
    };
    for phase in ["healthy", "http_error", "config_race", "reconfigured"] {
        configure(threshold("BTC", 0, 1_000_000));
        configure(threshold("ETH", 1, 1_000_000));
        let changed = std::cell::Cell::new(false);
        let (result, _): (Result<(), ErrorCode>, _) = call_with_routed_outcalls(
            &pic,
            canister,
            Principal::anonymous(),
            "refresh_market",
            (),
            |call| {
                if phase == "http_error" {
                    return Ok((503, b"unavailable".to_vec()));
                }
                if phase == "config_race" && !changed.replace(true) {
                    configure(threshold("BTC", 0, 9_000_000));
                }
                let request: Value = serde_json::from_slice(&call.body).unwrap();
                let body = if request["type"] == "metaAndAssetCtxs" {
                    json!([{"universe":[{"name":"BTC"},{"name":"ETH"}]},
                        [{"dayNtlVlm":"2000000"},{"dayNtlVlm":"2000000"}]])
                } else {
                    json!({"coin":request["coin"],
                        "time":pic.get_time().as_nanos_since_unix_epoch() / 1_000_000,
                        "levels":[[{"px":"59999","sz":"1"}],[{"px":"60001","sz":"1"}]]})
                };
                Ok((200, serde_json::to_vec(&body).unwrap()))
            },
        )
        .unwrap();
        match phase {
            "healthy" | "reconfigured" => {
                result.unwrap();
                assert!(status("BTC").eligible_for_new_risk);
                assert!(status("ETH").eligible_for_new_risk);
            }
            "http_error" => {
                assert!(result.is_err());
                assert!(!status("BTC").eligible_for_new_risk);
                assert_eq!(
                    status("BTC").reason_code.as_deref(),
                    Some("market_observation_failed")
                );
            }
            "config_race" => {
                assert!(result.is_err());
                assert!(!status("BTC").eligible_for_new_risk);
                assert_eq!(
                    status("BTC").reason_code.as_deref(),
                    Some("threshold_changed")
                );
            }
            _ => unreachable!(),
        }
    }
    // Force admission to be due, but prohibit new risk via the cycle reserve.
    // The production timer must not spend cycles on another market outcall.
    configure(threshold("BTC", 0, 1_000_000));
    let configured: Result<(), ErrorCode> = pocket_ic_tests::update_args(
        &pic,
        canister,
        admin,
        "core_configure_cycles",
        (1u128, 20_000_000_000_000_000u128),
    )
    .unwrap();
    configured.unwrap();
    pic.advance_time(std::time::Duration::from_secs(6));
    for _ in 0..40 {
        pic.tick();
    }
    assert!(
        pic.get_canister_http().is_empty(),
        "automatic market polling must stop when cycles prohibit new risk"
    );
}

#[test]
fn market_transform_validates_admission_without_hiding_unsafe_data() {
    let pic = pocket_ic_tests::pic();
    let canister = pic.create_canister();
    pic.add_cycles(canister, 10_000_000_000_000_000);
    let path = std::env::var("PRIVATE_PERP_UNIFIED_WASM").unwrap();
    pic.install_canister(
        canister,
        std::fs::read(path).unwrap(),
        encode_args((principal(51),)).unwrap(),
        None,
    );
    let now = 1_800_000_000_000u64;
    let context = json!({
        "threshold": {
            "market": "BTC", "expected_index": 0,
            "min_day_notional_usdc": 1_000_000,
            "max_spread_bps": 20, "min_each_side_depth_usdc": 10_000
        },
        "book": false, "requested_at": now
    });
    let transform = |body: Vec<u8>, context: &Value, status: u16| {
        query::<_, HttpRequestResult>(
            &pic,
            canister,
            Principal::anonymous(),
            "transform_market_info",
            TransformArgs {
                response: HttpRequestResult {
                    status: status.into(),
                    headers: vec![HttpHeader {
                        name: "date".into(),
                        value: "variable".into(),
                    }],
                    body,
                },
                context: serde_json::to_vec(context).unwrap(),
            },
        )
        .unwrap()
    };
    let check = |body: &Value, context: &Value| {
        let result = transform(serde_json::to_vec(body).unwrap(), context, 200);
        assert!(result.headers.is_empty());
        serde_json::from_slice::<String>(&result.body).unwrap()
    };
    let mut meta =
        json!([{"universe":[{"name":"BTC"}]},[{"dayNtlVlm":"1000001.123456789","markPx":"60000"}]]);
    assert_eq!(check(&meta, &context), "Eligible");
    meta[1][0]["dayNtlVlm"] = json!("1000002.999999999");
    meta[1][0]["markPx"] = json!("60099");
    assert_eq!(check(&meta, &context), "Eligible");
    meta[1][0]["dayNtlVlm"] = json!("999999.999999999");
    assert_eq!(check(&meta, &context), "VolumeLow");
    meta[1][0]["dayNtlVlm"] = json!("1000000");
    meta[0]["universe"][0]["isDelisted"] = json!(true);
    assert_eq!(check(&meta, &context), "AssetDelisted");
    meta[0]["universe"][0]["isDelisted"] = json!(false);
    let mut wrong_index = context.clone();
    wrong_index["threshold"]["expected_index"] = json!(1);
    assert_eq!(check(&meta, &wrong_index), "AssetIndexChanged");
    meta[0]["universe"][0]["isDelisted"] = json!("false");
    assert_eq!(check(&meta, &context), "Invalid");

    let mut book_context = context.clone();
    book_context["book"] = json!(true);
    let book = json!({"coin":"BTC","time":now,
        "levels":[[{"px":"59999","sz":"1"}],[{"px":"60001","sz":"1"}]]});
    assert_eq!(check(&book, &book_context), "Eligible");
    let mut varied = book.clone();
    varied["time"] = json!(now + 150);
    varied["levels"][0][0]["sz"] = json!("1.01");
    varied["levels"][1][0]["px"] = json!("60002");
    assert_eq!(check(&varied, &book_context), "Eligible");
    for (field, value) in [
        ("coin", json!("ETH")),
        ("time", json!(now - 60_001)),
        ("time", json!(now + 60_001)),
    ] {
        let mut invalid = book.clone();
        invalid[field] = value;
        assert_eq!(check(&invalid, &book_context), "Invalid");
    }
    let mut shallow = book.clone();
    shallow["levels"][0][0]["sz"] = json!("0.01");
    assert_eq!(check(&shallow, &book_context), "BookShallow");
    let mut wide = book.clone();
    wide["levels"] = json!([[{"px":"999","sz":"100"}],[{"px":"1001.01","sz":"100"}]]);
    assert_eq!(
        check(&wide, &book_context),
        "SpreadWide",
        "20.0999 bps must not truncate to the allowed 20"
    );
    let mut malformed = book.clone();
    malformed["levels"][0]
        .as_array_mut()
        .unwrap()
        .push(json!({"px":"60000","sz":"1"}));
    assert_eq!(check(&malformed, &book_context), "Invalid");
    assert_eq!(
        transform(b"not json".to_vec(), &book_context, 200).body,
        b"\"Invalid\""
    );
    assert_eq!(check(&book, &json!({})), "Invalid");
    let error = transform(serde_json::to_vec(&book).unwrap(), &book_context, 500);
    assert_eq!(
        error.status,
        candid::Nat::from(500u16),
        "HTTP error cannot become success"
    );
}

#[test]
fn pinned_http_builder_selects_v2_and_disables_replication() {
    let request =
        ic_cdk_management_canister::HttpRequest::new("https://api.hyperliquid-testnet.xyz/info")
            .non_replicated();
    assert_eq!(request.args().pricing_version, Some(2));
    assert_eq!(request.args().is_replicated, Some(false));
}

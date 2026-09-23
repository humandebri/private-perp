//! 環境設定の一般化とE-2（mainnet拒否・endpoint不一致拒否・設定が使われること）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::environment::EnvironmentView;
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::fund::AgentGeneration;
use api_types::order::{OrderKind, Side, SubmitOrderArgs};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, call_with_routed_outcalls,
    configure_policy, deploy, fund_trading_account, pic, principal, rotate_hpke_key, update,
    update_args, venue_router_default,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;
const ACCEPTED: &[u8] =
    br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":4242}}]}}}"#;

fn secret(seed: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes
}

fn blob(value: &[u8]) -> Blob {
    value.to_vec().into()
}

fn open_session(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    key: &[u8; 32],
) -> SessionHandle {
    let eoa = address_from_secret(key).expect("address");
    let issued: Result<ChallengeResponse, ErrorCode> = update(
        pic,
        vault,
        caller,
        "issue_challenge",
        ChallengeRequest {
            eoa_address: eoa.to_vec().into(),
            principal: caller,
            purpose: ChallengePurpose::Login,
            network: Network::Local,
            origin: ORIGIN.to_string(),
        },
    )
    .expect("call");
    let issued = issued.expect("challenge");
    let challenge = private_perp::Challenge {
        purpose: "login".to_string(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".to_string(),
        origin: ORIGIN.to_string(),
        nonce: issued.nonce.as_ref().try_into().expect("nonce"),
        expires_at: issued.expires_at,
    };
    let signature = challenge.sign_for_tests(key).expect("sign");
    let session: Result<SessionHandle, ErrorCode> = update(
        pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: issued.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("call");
    session.expect("session")
}

fn setup(pic: &PocketIc) -> (Principal, Principal, Principal, SessionHandle) {
    let controller = principal(220);
    let vault = deploy(
        pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    configure_policy(pic, core, controller, &["BTC", "ETH"]);
    let meta: Result<(), ErrorCode> = update_args(
        pic,
        core,
        controller,
        "set_meta_cache",
        (
            "local".to_string(),
            "hyperliquid".to_string(),
            UNIVERSE.to_string(),
        ),
    )
    .expect("call");
    meta.expect("set_meta_cache");
    let context: Result<(), ErrorCode> = update_args(
        pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");
    rotate_hpke_key(pic, core, controller);

    let caller = principal(221);
    let session = open_session(pic, vault, caller, &secret(222));
    (controller, vault, core, session)
}

/// mainnetは拒否し、networkとendpointの不一致も拒否する（E-2）。
#[test]
fn mainnet_is_refused_and_endpoints_must_match_the_network() {
    let pic = pic();
    let (controller, _vault, core, _session) = setup(&pic);
    let outsider = principal(223);

    // 非controllerは設定できない。
    let denied: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        outsider,
        "set_market_context",
        ("testnet".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );

    // E-2: mainnetはPhase 2では拒否する。
    let mainnet: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("mainnet".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    assert!(
        matches!(
            mainnet,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::NetworkMismatch,
                ..
            })
        ),
        "{mainnet:?}"
    );

    // testnetへの切り替えは許可する。
    let testnet: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("testnet".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    testnet.expect("set_market_context(testnet)");

    // E-2: testnet設定にmainnet endpointを指定すると拒否する。
    let mismatch: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_venue_endpoints",
        (
            "https://api.hyperliquid.xyz/exchange".to_string(),
            "https://api.hyperliquid.xyz/info".to_string(),
        ),
    )
    .expect("call");
    assert!(
        matches!(
            mismatch,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::NetworkMismatch,
                ..
            })
        ),
        "{mismatch:?}"
    );

    // 別ドメイン（接尾辞一致のなりすまし）も拒否する。
    let lookalike: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_venue_endpoints",
        (
            "https://api.hyperliquid-testnet.xyz.evil.test/exchange".to_string(),
            "https://api.hyperliquid-testnet.xyz/info".to_string(),
        ),
    )
    .expect("call");
    assert!(lookalike.is_err(), "{lookalike:?}");

    // testnetの正しいendpointは通る。
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_venue_endpoints",
        (
            "https://api.hyperliquid-testnet.xyz/exchange".to_string(),
            "https://api.hyperliquid-testnet.xyz/info".to_string(),
        ),
    )
    .expect("call");
    configured.expect("set_venue_endpoints(testnet)");

    let environment: Result<EnvironmentView, ErrorCode> =
        pocket_ic_tests::query(&pic, core, outsider, "get_environment", ()).expect("call");
    let environment = environment.expect("environment");
    assert_eq!(environment.network, Network::Testnet);
    assert_eq!(
        environment.exchange_url,
        "https://api.hyperliquid-testnet.xyz/exchange"
    );
    assert_eq!(environment.ecdsa_key_id, "test_key_1", "既定のkey ID");

    // key IDは設定できるが、不正な値は拒否する。
    let invalid: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_ecdsa_key_id", String::new()).expect("call");
    assert!(
        matches!(
            invalid,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                ..
            })
        ),
        "{invalid:?}"
    );
    let key: Result<(), ErrorCode> = update(
        &pic,
        core,
        controller,
        "set_ecdsa_key_id",
        "test_key_1".to_string(),
    )
    .expect("call");
    key.expect("set_ecdsa_key_id");

    // networkをlocalへ戻すとendpointもlocalの整合が必要になる。
    let back: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    back.expect("set_market_context(local)");
    let stale: Result<EnvironmentView, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_environment", ()).expect("call");
    assert!(
        stale.is_err(),
        "local設定にtestnet endpointが残っていれば拒否する: {stale:?}"
    );
}

/// 設定したendpointが送信・照合に使われる（ビルド定数ではない）。
#[test]
fn the_configured_endpoints_are_used_for_venue_calls() {
    let pic = pic();
    let (controller, vault, core, session) = setup(&pic);
    let caller = principal(221);

    // ローカルはmockのURLを明示できる（実venueのhostは拒否される）。
    let rejected: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_venue_endpoints",
        (
            "https://api.hyperliquid-testnet.xyz/exchange".to_string(),
            "http://127.0.0.1:9911/info".to_string(),
        ),
    )
    .expect("call");
    assert!(rejected.is_err(), "ローカルの設定で実testnetへ出さない");

    let exchange_url = "http://127.0.0.1:9911/exchange";
    let info_url = "http://127.0.0.1:9911/info";
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_venue_endpoints",
        (exchange_url.to_string(), info_url.to_string()),
    )
    .expect("call");
    configured.expect("set_venue_endpoints(local mock)");

    // 注文を1件送信し、照合まで同じsweepで走らせる。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"env-alloc",
        5_000_000_000,
        71,
    );
    let agent: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");
    pocket_ic_tests::observe_empty_account(&pic, core, caller, &session);
    let submitted: Result<api_types::order::SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            SubmitOrderArgs {
                session: session.clone(),
                client_request_id: blob(b"env-order"),
                account_id: pocket_ic_tests::trading_account_id(&pic, vault, caller, &session),
                market: "ETH".to_string(),
                side: Side::Buy,
                kind: OrderKind::LimitGtc,
                quantity: "0.05".to_string(),
                limit_price: Some("2500".to_string()),
                slippage_tolerance_bps: None,
                reduce_only: false,
                leverage: Some(3),
                trigger: None,
                expires_after: None,
            },
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let (swept, captured) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            venue_router_default(ACCEPTED),
        )
        .expect("call");
    swept.expect("sweep");
    assert!(!captured.is_empty(), "outcallが出る");
    for call in &captured {
        if call.url.contains("/exchange") {
            assert_eq!(call.url, exchange_url, "設定した/exchangeへ送る");
        } else {
            assert_eq!(call.url, info_url, "設定した/infoで照合する");
        }
    }
}

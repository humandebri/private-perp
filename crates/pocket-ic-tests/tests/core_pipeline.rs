//! 本番の注文パイプライン（送信・取消・`/info`照合）の試験。
//!
//! 署名・送信・照合は `test-venue` featureではなく本番の`pipeline`が行う。
//! 試験は明示的な `test_sweep_now`（feature付きビルドのみ）で駆動し、outcallは
//! 送信内容で振り分けてmockする。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::AgentGeneration;
use api_types::order::{
    AccountSnapshot, OrderKind, OrderState, OrderSummary, Side, SubmitOrderArgs,
};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, call_with_routed_outcalls,
    configure_policy, deploy, envelope, fund_trading_account, pic, principal, rotate_hpke_key,
    update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;
/// 取引所の受理応答（resting注文のoid）。
const ACCEPTED: &[u8] =
    br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":4242}}]}}}"#;
/// 建玉1件の照合応答。
const POSITIONS: &[u8] = br#"{"marginSummary":{"accountValue":"100"},"assetPositions":[{"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","liquidationPx":"2000","unrealizedPnl":"1.5","leverage":{"value":3},"marginMode":"cross"},"type":"oneWay"}],"withdrawable":"50"}"#;
/// 約定1件の照合応答。
const FILLS: &[u8] = br#"[{"tid":9001,"oid":4242,"coin":"ETH","px":"2500","sz":"0.05","fee":120,"time":1700000000000,"dir":"Open Long","users":["0xaa"],"extra":true}]"#;
/// 注文が約定した状態の照合応答。
const STATUS_FILLED: &[u8] =
    br#"{"status":"filled","order":{"oid":4242,"coin":"ETH","sz":"0.05"}}"#;

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

/// 取引可能なユーザー1人分の準備（口座・equity・Agent承認）。
struct User {
    caller: Principal,
    session: SessionHandle,
    trading_address: [u8; 20],
}

fn provision_user(
    pic: &PocketIc,
    vault: Principal,
    core: Principal,
    controller: Principal,
    seed: u8,
    tag: &[u8],
) -> User {
    let caller = principal(seed);
    let session = open_session(pic, vault, caller, &secret(seed.wrapping_add(20)));
    let mut request_id = tag.to_vec();
    request_id.push(seed);
    let trading_address = fund_trading_account(
        pic,
        vault,
        controller,
        caller,
        &session,
        &request_id,
        5_000_000_000,
        seed,
    );
    let agent: Result<AgentGeneration, ErrorCode> = update(
        pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");
    User {
        caller,
        session,
        trading_address,
    }
}

fn setup(pic: &PocketIc) -> (Principal, Principal) {
    let controller = principal(170);
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
    (vault, core)
}

fn submit(
    pic: &PocketIc,
    core: Principal,
    user: &User,
    request_id: &[u8],
    quantity: &str,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    update_args(
        pic,
        core,
        user.caller,
        "submit_order",
        (
            user.session.clone(),
            SubmitOrderArgs {
                session: user.session.clone(),
                client_request_id: blob(request_id),
                account_id: blob(&[0u8; 32]),
                market: "ETH".to_string(),
                side: Side::Buy,
                kind: OrderKind::LimitGtc,
                quantity: quantity.to_string(),
                limit_price: Some("2500".to_string()),
                slippage_tolerance_bps: None,
                reduce_only: false,
                leverage: Some(3),
                trigger: None,
                expires_after: None,
            },
        ),
    )
    .expect("call")
}

/// 送信内容から応答を選ぶ（URLと本文の`type`で振り分ける）。
fn route(
    exchange: &[u8],
    positions: &[u8],
    fills: &[u8],
    status: &[u8],
) -> impl Fn(&pocket_ic_tests::CapturedHttpCall) -> Result<(u16, Vec<u8>), (u64, String)> {
    let exchange = exchange.to_vec();
    let positions = positions.to_vec();
    let fills = fills.to_vec();
    let status = status.to_vec();
    move |call| {
        let body = String::from_utf8_lossy(&call.body).to_string();
        if call.url.contains("/exchange") {
            return Ok((200, exchange.clone()));
        }
        let query: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        match query.get("type").and_then(|value| value.as_str()) {
            Some("clearinghouseState") => Ok((200, positions.clone())),
            Some("userFills") => Ok((200, fills.clone())),
            Some("orderStatus") => Ok((200, status.clone())),
            other => Err((1, format!("unexpected info query: {other:?}"))),
        }
    }
}

fn list_orders(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> Vec<OrderSummary> {
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(pic, core, caller, session, None::<Blob>, 10).expect("call");
    listed.expect("list_orders").items
}

fn account_snapshot(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> AccountSnapshot {
    let snapshot: Result<AccountSnapshot, ErrorCode> =
        envelope::get_account_snapshot(pic, core, caller, session).expect("call");
    snapshot.expect("snapshot")
}

/// 受付→署名・送信→`/info`照合（建玉・約定・注文状態）まで本番経路で通る。
#[test]
fn the_pipeline_dispatches_and_reconciles_orders() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 171, b"pipeline-alloc");

    let submitted = submit(&pic, core, &user, b"pipeline-1", "0.05").expect("accepted");

    // 1回のsweepで送信（/exchange）と照合（/infoが3種）が出る。
    let (swept, captured) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, FILLS, STATUS_FILLED),
        )
        .expect("call");
    let swept = swept.expect("sweep");
    assert_eq!(
        (swept.dispatched, swept.reconciled),
        (1, 1),
        "送信1件＋照合1口座"
    );

    // 送信本文はAgent鍵の署名つきで、署名actionが入っている。
    let exchange = captured
        .iter()
        .find(|call| call.url.contains("/exchange"))
        .expect("exchange outcall");
    let body: serde_json::Value = serde_json::from_slice(&exchange.body).expect("json");
    assert_eq!(body["action"]["type"], "order");
    assert_eq!(body["action"]["orders"][0]["s"], "0.05");
    assert!(body["signature"]["r"].as_str().is_some(), "署名が入る");

    // `/info`の照会先はvaultが持つ取引口座のアドレス。
    let expected_user = format!("0x{}", hex::encode(user.trading_address));
    let info = captured
        .iter()
        .filter(|call| call.url.contains("/info"))
        .collect::<Vec<_>>();
    assert!(!info.is_empty(), "照合のoutcallが出る");
    for call in &info {
        let query: serde_json::Value = serde_json::from_slice(&call.body).expect("json");
        assert_eq!(query["user"], expected_user, "取引所アドレスで照会する");
    }

    // 注文はoidつきで、注文状態の反映（filled）と約定の取り込みが効いている。
    let orders = list_orders(&pic, core, user.caller, &user.session);
    let order = orders
        .iter()
        .find(|order| order.order_id.as_ref() == submitted.order_id.as_ref())
        .expect("order is listed");
    assert_eq!(order.hl_oid, Some(4242));
    assert_eq!(order.state, OrderState::Filled);
    assert_eq!(order.filled_quantity, "0.05");

    // 建玉は観測の全量で反映され、約定した注文のリスク予約は残らない
    // （残すとequityに対する新規注文の枠を永久に食い潰す）。
    let snapshot = account_snapshot(&pic, core, user.caller, &user.session);
    assert_eq!(snapshot.margin_used, 0, "約定した注文の予約を解放する");
    assert_eq!(snapshot.positions.len(), 1);
    assert_eq!(snapshot.positions[0].market, "ETH");
    assert_eq!(snapshot.positions[0].size, "0.05");
    assert_eq!(snapshot.positions[0].unrealized_pnl, 1_500_000);

    let fills: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(&pic, core, user.caller, &user.session, None::<Blob>, 10)
            .expect("call");
    assert_eq!(fills.expect("fills").items.len(), 1);
}

/// 拒否はリスク予約を解放し、結果不明は再送しない。
#[test]
fn rejected_orders_release_risk_and_unknown_sends_are_not_resent() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 181, b"pipeline-alloc-2");

    // 取引所が拒否した注文は`rejected`になり、予約は解放される。
    let rejected = submit(&pic, core, &user, b"pipeline-rejected", "0.05").expect("accepted");
    let venue_rejection = br#"{"status":"err","response":"insufficient margin"}"#;
    let (swept, _) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(
                venue_rejection,
                br#"{"assetPositions":[]}"#,
                b"[]",
                br#"{"status":"open","order":{"oid":4242}}"#,
            ),
        )
        .expect("call");
    assert_eq!(
        swept.expect("sweep").dispatched,
        1,
        "拒否も送信として数える"
    );
    let orders = list_orders(&pic, core, user.caller, &user.session);
    let order = orders
        .iter()
        .find(|order| order.order_id.as_ref() == rejected.order_id.as_ref())
        .expect("order is listed");
    assert_eq!(order.state, OrderState::Rejected);
    let snapshot = account_snapshot(&pic, core, user.caller, &user.session);
    assert_eq!(snapshot.margin_used, 0, "拒否でリスク予約を解放する");

    // 送信結果が不明な注文は`unknown`にし、再送しない。
    let uncertain = submit(&pic, core, &user, b"pipeline-uncertain", "0.05").expect("accepted");
    let (swept, _) = call_with_routed_outcalls::<
        (),
        Result<api_types::order::SweepOutcome, ErrorCode>,
        _,
    >(&pic, core, controller, "test_sweep_now", (), |call| {
        if call.url.contains("/exchange") {
            // 応答が得られない（送信したか不明）。
            Err((1, "timeout".to_string()))
        } else {
            route(
                br#"{"assetPositions":[]}"#,
                br#"{"assetPositions":[]}"#,
                b"[]",
                br#"{"status":"open","order":{"oid":0}}"#,
            )(call)
        }
    })
    .expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);
    let orders = list_orders(&pic, core, user.caller, &user.session);
    let order = orders
        .iter()
        .find(|order| order.order_id.as_ref() == uncertain.order_id.as_ref())
        .expect("order is listed");
    assert_eq!(order.state, OrderState::Unknown);
    assert_eq!(order.hl_oid, None, "oidは分からないまま");
    assert_ne!(
        account_snapshot(&pic, core, user.caller, &user.session).margin_used,
        0,
        "不明な送信の予約は解放しない（二重エクスポージャを防ぐ）"
    );

    // 2回目のsweepで再送しない（/exchangeのoutcallが出ない）。
    let (swept, captured) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(
                ACCEPTED,
                br#"{"assetPositions":[]}"#,
                b"[]",
                br#"{"status":"open","order":{"oid":0}}"#,
            ),
        )
        .expect("call");
    let again = swept.expect("sweep");
    assert_eq!((again.dispatched, again.cancels), (0, 0), "自動再送しない");
    assert!(
        captured.iter().all(|call| !call.url.contains("/exchange")),
        "不明な注文は再送しない"
    );
}

/// 巡回は3口座以上でも全口座を対象にする（固定窓で古い口座を落とさない）。
#[test]
fn the_sweep_rotates_over_every_active_account() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let users: Vec<User> = [191u8, 192, 193]
        .into_iter()
        .map(|seed| provision_user(&pic, vault, core, controller, seed, b"pipeline-alloc-3"))
        .collect();
    for (index, user) in users.iter().enumerate() {
        let request_id = format!("pipeline-rotate-{index}");
        submit(&pic, core, user, request_id.as_bytes(), "0.01").expect("accepted");
    }

    // 1回のsweepは2口座までなので、2回で全3口座を巡回する。
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..2 {
        let (_, captured) =
            call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
                &pic,
                core,
                controller,
                "test_sweep_now",
                (),
                route(
                    ACCEPTED,
                    br#"{"assetPositions":[]}"#,
                    b"[]",
                    br#"{"status":"open","order":{"oid":4242}}"#,
                ),
            )
            .expect("call");
        for call in captured.iter().filter(|call| call.url.contains("/info")) {
            let query: serde_json::Value = serde_json::from_slice(&call.body).expect("json");
            if let Some(user) = query.get("user").and_then(|value| value.as_str()) {
                seen.insert(user.to_string());
            }
        }
    }
    for user in &users {
        let address = format!("0x{}", hex::encode(user.trading_address));
        assert!(seen.contains(&address), "{address} も照合の対象になる");
    }
}

/// 手動sweepはcontrollerのみ。停止したパイプラインの運用入口として使う。
#[test]
fn only_a_controller_can_sweep_manually() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 201, b"pipeline-alloc-4");

    let denied: Result<api_types::order::SweepOutcome, ErrorCode> =
        update(&pic, core, user.caller, "sweep", ()).expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );

    let allowed: Result<api_types::order::SweepOutcome, ErrorCode> =
        update(&pic, core, controller, "sweep", ()).expect("call");
    // 送信も取消も対象が無ければ0件（無害に完了する）。
    let allowed = allowed.expect("sweep");
    assert_eq!((allowed.dispatched, allowed.cancels), (0, 0));
}

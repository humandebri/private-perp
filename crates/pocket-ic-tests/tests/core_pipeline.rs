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
    AccountSnapshot, OrderKind, OrderState, OrderSummary, PreflightResolution, Side,
    SubmitOrderArgs,
};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, call_with_mocked_outcall,
    call_with_routed_outcalls, configure_policy, deploy, envelope, fund_trading_account, pic,
    principal, query, rotate_hpke_key, update, update_args,
};
use std::cell::Cell;

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;
/// 取引所の受理応答（resting注文のoid）。
const ACCEPTED: &[u8] =
    br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":4242}}]}}}"#;
/// 建玉1件の照合応答。
const POSITIONS: &[u8] = br#"{"marginSummary":{"accountValue":"100","totalMarginUsed":"0"},"assetPositions":[{"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","liquidationPx":"2000","unrealizedPnl":"1.5","leverage":{"value":3},"marginMode":"cross"},"type":"oneWay"}],"withdrawable":"50"}"#;
/// 約定1件の照合応答。
const FILLS: &[u8] = br#"[{"tid":9001,"oid":4242,"coin":"ETH","px":"2500","sz":"0.05","fee":"0.000120","time":1700000000000,"dir":"Open Long","users":["0xaa"],"extra":true}]"#;
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
    let session = session.expect("session");
    pocket_ic_tests::activate_local_user(pic, vault, caller, &session);
    session
}

/// 取引可能なユーザー1人分の準備（口座・equity・Agent承認）。
struct User {
    account_id: Blob,
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
    pocket_ic_tests::observe_empty_account(pic, core, caller, &session);
    User {
        account_id: pocket_ic_tests::trading_account_id(pic, vault, caller, &session),
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
    submit_with_leverage(pic, core, user, request_id, quantity, 3)
}

fn submit_with_leverage(
    pic: &PocketIc,
    core: Principal,
    user: &User,
    request_id: &[u8],
    quantity: &str,
    leverage: u32,
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
                account_id: user.account_id.clone(),
                market: "ETH".to_string(),
                side: Side::Buy,
                kind: OrderKind::LimitGtc,
                quantity: quantity.to_string(),
                limit_price: Some("2500".to_string()),
                slippage_tolerance_bps: None,
                reduce_only: false,
                leverage: Some(leverage),
                trigger: None,
                expires_after: None,
            },
        ),
    )
    .expect("call")
}

fn exchange_types(calls: &[pocket_ic_tests::CapturedHttpCall]) -> Vec<String> {
    calls
        .iter()
        .filter(|call| call.url.contains("/exchange"))
        .map(|call| {
            let body: serde_json::Value =
                serde_json::from_slice(&call.body).expect("exchange JSON");
            body["action"]["type"]
                .as_str()
                .expect("action type")
                .to_string()
        })
        .collect()
}

#[test]
fn confirmed_leverage_is_reused_per_account_and_asset() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 210, b"leverage-cache");

    let sweep = |pic: &PocketIc| {
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .expect("sweep call")
    };

    submit(&pic, core, &user, b"leverage-first", "0.01").expect("first order");
    let (first, first_calls) = sweep(&pic);
    assert_eq!(first.expect("first sweep").dispatched, 1);
    assert_eq!(exchange_types(&first_calls), ["updateLeverage", "order"]);

    submit(&pic, core, &user, b"leverage-same", "0.01").expect("same leverage");
    let (second, second_calls) = sweep(&pic);
    assert_eq!(second.expect("second sweep").dispatched, 1);
    assert_eq!(exchange_types(&second_calls), ["order"]);

    submit_with_leverage(&pic, core, &user, b"leverage-changed", "0.01", 4)
        .expect("changed leverage");
    let (third, third_calls) = sweep(&pic);
    assert_eq!(third.expect("third sweep").dispatched, 1);
    assert_eq!(exchange_types(&third_calls), ["updateLeverage", "order"]);
}

#[test]
fn uncertain_leverage_blocks_a_different_setting() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 211, b"leverage-unknown");
    let first = submit(&pic, core, &user, b"leverage-unknown-first", "0.01").expect("first order");
    submit_with_leverage(&pic, core, &user, b"leverage-unknown-second", "0.01", 4)
        .expect("second order accepted");
    let normal = route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED);
    let (outcome, first_calls) = call_with_routed_outcalls::<
        (),
        Result<api_types::order::SweepOutcome, ErrorCode>,
        _,
    >(&pic, core, controller, "test_sweep_now", (), |call| {
        if call.url.contains("/exchange") {
            Err((1, "leverage result unavailable".to_string()))
        } else {
            normal(call)
        }
    })
    .expect("first sweep call");
    assert_eq!(outcome.expect("first sweep").dispatched, 1);
    assert_eq!(exchange_types(&first_calls), ["updateLeverage"]);
    pocket_ic_tests::assert_manual_work_blocked(
        &pic,
        core,
        user.caller,
        &user.session,
        true,
        ("order", Some(first.order_id.as_ref())),
        "レバレッジ設定",
    );

    let (outcome, second_calls) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .expect("second sweep call");
    assert_eq!(outcome.expect("second sweep").dispatched, 0);
    assert!(exchange_types(&second_calls).is_empty());

    let resolved: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "resolve_unknown_order_preflight",
        (first.order_id, PreflightResolution::Rejected),
    )
    .expect("resolution call");
    resolved.expect("resolve first leverage change");
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, core, user.caller, &user.session, true, "order"),
        1,
        "the second pre-send failure needs explicit permission"
    );
    pic.advance_time(std::time::Duration::from_secs(6));
    pic.tick();
    let (outcome, third_calls) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .expect("third sweep call");
    assert_eq!(outcome.expect("third sweep").dispatched, 1);
    assert_eq!(exchange_types(&third_calls), ["updateLeverage", "order"]);
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
            let request: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            if request["action"]["type"] == "updateLeverage" {
                return Ok((
                    200,
                    br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
                ));
            }
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

    // 照合は新規注文の送信より先に走る。受理したoidは次のsweepで確認する。
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
        .find(|call| {
            call.url.contains("/exchange")
                && serde_json::from_slice::<serde_json::Value>(&call.body)
                    .is_ok_and(|body| body["action"]["type"] == "order")
        })
        .expect("exchange outcall");
    assert_eq!(
        exchange.replication,
        pocket_ic::common::rest::CanisterHttpReplication::NonReplicated,
    );
    let body: serde_json::Value = serde_json::from_slice(&exchange.body).expect("json");
    assert_eq!(body["action"]["type"], "order");
    assert!(
        body["action"].get("builder").is_none(),
        "builder fee remains zero"
    );
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
        assert_eq!(
            call.replication,
            pocket_ic::common::rest::CanisterHttpReplication::NonReplicated,
        );
        let query: serde_json::Value = serde_json::from_slice(&call.body).expect("json");
        assert_eq!(query["user"], expected_user, "取引所アドレスで照会する");
    }

    pic.advance_time(std::time::Duration::from_secs(121));
    pic.tick();
    let (second, _) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, FILLS, STATUS_FILLED),
        )
        .expect("second sweep");
    assert_eq!(second.expect("second sweep result").reconciled, 1);

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
    assert_eq!(
        snapshot.open_order_risk_reserved, 0,
        "約定した注文の予約を解放する"
    );
    assert_eq!(snapshot.positions.len(), 1);
    assert_eq!(snapshot.positions[0].market, "ETH");
    assert_eq!(snapshot.positions[0].size, "0.05");
    assert_eq!(snapshot.positions[0].unrealized_pnl, 1_500_000);

    let fills: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(&pic, core, user.caller, &user.session, None::<Blob>, 10)
            .expect("call");
    assert_eq!(fills.expect("fills").items.len(), 1);
}

/// coreだけを古いsnapshotへ戻しても、独立journalの高水位がPOSTを止める。
#[test]
fn restored_core_snapshot_cannot_send_against_newer_journal() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 191, b"restore-alloc");
    let snapshot = pic
        .take_canister_snapshot(core, Some(controller), None)
        .expect("take core snapshot");

    submit(&pic, core, &user, b"sent-after-snapshot", "0.05").expect("accepted");
    let (first, first_calls) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, br#"{"assetPositions":[]}"#, b"[]", STATUS_FILLED),
        )
        .expect("first sweep");
    assert_eq!(first.expect("first result").dispatched, 1);
    assert!(
        first_calls
            .iter()
            .any(|call| call.url.contains("/exchange"))
    );

    pic.load_canister_snapshot(core, Some(controller), snapshot.id)
        .expect("restore core snapshot");
    let attempted = submit(&pic, core, &user, b"after-restore", "0.05");
    assert!(
        matches!(attempted, Err(ErrorCode::PolicyUnavailable)),
        "the missing account event must stop intake before another POST"
    );
    let (_restored, calls) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, br#"{"assetPositions":[]}"#, b"[]", STATUS_FILLED),
        )
        .expect("restored sweep call");
    let status: Result<(u64, u64, bool), ErrorCode> =
        query(&pic, core, controller, "journal_restore_status", ()).unwrap();
    assert!(status.unwrap().2, "journal mismatch must lock dispatch");
    assert!(
        calls.iter().all(|call| !call.url.contains("/exchange")),
        "no second POST"
    );
}

#[test]
fn journal_outage_blocks_exchange_post() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 192, b"journal-outage-alloc");
    submit(&pic, core, &user, b"journal-outage-order", "0.05").expect("accepted");
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, core, controller, "get_send_journal", ()).unwrap();
    pic.stop_canister(journal.unwrap().unwrap(), Some(controller))
        .expect("stop journal");
    let prior_risk =
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved;
    let prior_orders = list_orders(&pic, core, user.caller, &user.session).len();
    assert!(submit(&pic, core, &user, b"journal-down-new-order", "0.05").is_err());
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        prior_risk,
        "journal outage must not create a new risk reservation"
    );
    assert_eq!(
        list_orders(&pic, core, user.caller, &user.session).len(),
        prior_orders
    );

    let (result, calls) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, br#"{"assetPositions":[]}"#, b"[]", STATUS_FILLED),
        )
        .expect("sweep call");
    assert!(result.is_err());
    assert!(
        calls.iter().all(|call| !call.url.contains("/exchange")),
        "journal outage must stop all POSTs"
    );
}

#[test]
fn journal_outage_after_leverage_post_keeps_order_unknown() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 193, b"result-outage-alloc");
    let submitted = submit(&pic, core, &user, b"result-outage-order", "0.05").expect("accepted");
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, core, controller, "get_send_journal", ()).expect("journal query");
    let journal = journal.expect("configured journal").expect("principal");
    let stopped = Cell::new(false);
    let normal = route(ACCEPTED, POSITIONS, FILLS, STATUS_FILLED);
    let (result, calls) = call_with_routed_outcalls::<
        (),
        Result<api_types::order::SweepOutcome, ErrorCode>,
        _,
    >(&pic, core, controller, "test_sweep_now", (), |call| {
        if call.url.contains("/exchange") && !stopped.get() {
            pic.stop_canister(journal, Some(controller))
                .expect("stop journal after exchange POST");
            stopped.set(true);
        }
        normal(call)
    })
    .expect("sweep call");
    assert_eq!(result.expect("sweep").dispatched, 1);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.contains("/exchange"))
            .count(),
        1
    );
    let order = list_orders(&pic, core, user.caller, &user.session)
        .into_iter()
        .find(|order| order.order_id == submitted.order_id)
        .expect("order");
    assert_eq!(order.state, OrderState::Unknown);
    assert!(account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved > 0);
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
    // HTTP/outer status が成功でも、子statusのerrorは受理として扱わない。
    let venue_rejection = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"error":"insufficient margin"}]}}}"#;
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
    assert_eq!(
        snapshot.open_order_risk_reserved, 0,
        "拒否でリスク予約を解放する"
    );

    // 送信結果が不明な注文は`unknown`にし、再送しない。
    let uncertain = submit_with_leverage(&pic, core, &user, b"pipeline-uncertain", "0.05", 4)
        .expect("accepted");
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
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
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

    // leverage preflightが不明な場合はcontrollerの外部確認でのみ解決する。
    let resolved: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "resolve_unknown_order_preflight",
        (uncertain.order_id.clone(), PreflightResolution::Rejected),
    )
    .expect("call");
    resolved.expect("resolve unknown preflight");
    let orders = list_orders(&pic, core, user.caller, &user.session);
    let order = orders
        .iter()
        .find(|order| order.order_id.as_ref() == uncertain.order_id.as_ref())
        .expect("order is listed");
    assert_eq!(order.state, OrderState::Rejected);
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        0,
        "外部確認済みの拒否で予約を解放する"
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

#[test]
fn pending_order_prevents_recovery_before_any_transfer_post() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 190, b"fence-pending-alloc");
    let allocated: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "test_sweep_now",
        (),
        Ok((
            200,
            br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
        )),
    )
    .expect("allocation sweep");
    assert_eq!(allocated.expect("allocation"), 1);
    submit(&pic, core, &user, b"fence-pending-order", "0.05").expect("pending order");
    let arrived: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[224; 32]),
            5_000_000_000u64,
            blob(&user.trading_address),
            "usdc".to_string(),
        ),
    )
    .unwrap();
    arrived.unwrap();
    let accepted: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        user.caller,
        "request_recovery",
        (user.session.clone(), blob(b"fence-recovery"), 1_000_000u64),
    )
    .expect("recovery request");
    accepted.expect("reserved recovery");
    let swept: Result<u32, ErrorCode> =
        update_args(&pic, vault, controller, "test_sweep_now", ()).expect("recovery sweep");
    assert_eq!(swept.expect("blocked recovery"), 1);
    let status: Result<api_types::fund::FundStatus, ErrorCode> = update(
        &pic,
        vault,
        user.caller,
        "get_fund_status",
        user.session.clone(),
    )
    .expect("status");
    let status = status.expect("fund status");
    assert!(status.recovery_fence.is_none());
    assert_eq!(status.trading_equity, 10_000_000_000);
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        user.caller,
        "list_fund_events",
        (user.session.clone(), None::<Blob>, 10u32),
    )
    .expect("events");
    assert_eq!(
        events.expect("fund events").items[0].state,
        api_types::fund::FundRequestState::Rejected
    );
    assert_eq!(
        list_orders(&pic, core, user.caller, &user.session)[0].state,
        OrderState::Pending
    );
}

#[test]
fn lost_order_reply_recovers_by_cloid_and_ingests_decimal_rebate() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 169, b"lost-reply");
    let submitted = submit(&pic, core, &user, b"lost-order", "0.05").unwrap();
    let cloid = std::cell::RefCell::new(String::new());
    let (outcome, _): (Result<api_types::order::SweepOutcome, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, core, controller, "test_sweep_now", (), |call| {
            let q: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            if q["action"]["type"] == "order" {
                *cloid.borrow_mut() = q["action"]["orders"][0]["c"].as_str().unwrap().into();
                return Err((4, "lost order response".into()));
            }
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED)(call)
        })
        .unwrap();
    outcome.unwrap();
    assert_eq!(
        list_orders(&pic, core, user.caller, &user.session)[0].state,
        OrderState::Unknown
    );
    pic.advance_time(std::time::Duration::from_secs(130));
    let (automatic, calls): (Result<api_types::order::SweepOutcome, ErrorCode>, _) =
        call_with_routed_outcalls(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .unwrap();
    automatic.unwrap();
    assert!(
        !calls
            .iter()
            .any(|c| String::from_utf8_lossy(&c.body).contains("orderStatus")),
        "unknown order must not be checked automatically"
    );
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, core, user.caller, &user.session, true, "order"),
        1
    );
    let (outcome,calls): (Result<api_types::order::SweepOutcome,ErrorCode>,_) = call_with_routed_outcalls(
        &pic,core,controller,"test_sweep_now",(),|call| {
            let q: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            assert!(q.get("action").is_none(),"must not resend exchange action");
            if q["type"] == "orderStatus" {
                assert_eq!(q["oid"],*cloid.borrow());
                return Ok((200,serde_json::json!({"status":"order","order":{"status":"filled","order":{"oid":4242,"cloid":*cloid.borrow()}}}).to_string().into_bytes()));
            }
            route(ACCEPTED,POSITIONS,&String::from_utf8(FILLS.to_vec()).unwrap().replace("0.000120","-0.000120").into_bytes(),STATUS_FILLED)(call)
        }).unwrap();
    outcome.unwrap();
    assert!(
        calls
            .iter()
            .any(|c| String::from_utf8_lossy(&c.body).contains("orderStatus"))
    );
    let orders = list_orders(&pic, core, user.caller, &user.session);
    assert_eq!(orders[0].order_id, submitted.order_id);
    assert_eq!(orders[0].state, OrderState::Filled);
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        0
    );
    let fills = envelope::list_fills(&pic, core, user.caller, &user.session, None, 10)
        .unwrap()
        .unwrap();
    assert_eq!(fills.items.len(), 1);
    assert_eq!(fills.items[0].fee, -120);
}

#[test]
fn failed_monitor_stops_until_owner_resumes_without_stopping_other_accounts() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let a = provision_user(&pic, vault, core, controller, 217, b"monitor-a");
    let b = provision_user(&pic, vault, core, controller, 218, b"monitor-b");
    for user in [&a, &b] {
        let _ = account_snapshot(&pic, core, user.caller, &user.session);
        let seeded: Result<u32, ErrorCode> = update_args(
            &pic,
            core,
            user.caller,
            "test_ingest_positions",
            (
                user.session.clone(),
                String::from_utf8(POSITIONS.to_vec()).unwrap(),
            ),
        )
        .unwrap();
        seeded.unwrap();
    }
    let address = format!("0x{}", hex::encode(a.trading_address));
    let (first, calls): (Result<api_types::order::SweepOutcome, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, core, controller, "test_sweep_now", (), |call| {
            let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            if body["user"] == address {
                return Ok((503, b"unavailable".to_vec()));
            }
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED)(call)
        })
        .unwrap();
    first.unwrap();
    assert!(!calls.is_empty());
    pic.advance_time(std::time::Duration::from_secs(10));
    let (second, calls): (Result<api_types::order::SweepOutcome, ErrorCode>, _) =
        call_with_routed_outcalls(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .unwrap();
    second.unwrap();
    assert!(!calls.is_empty(), "other account must keep monitoring");
    assert!(calls.iter().all(
        |call| serde_json::from_slice::<serde_json::Value>(&call.body).unwrap()["user"] != address
    ));
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, core, a.caller, &a.session, true, "monitor"),
        1
    );
    let (third, calls): (Result<api_types::order::SweepOutcome, ErrorCode>, _) =
        call_with_routed_outcalls(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .unwrap();
    third.unwrap();
    assert!(calls.iter().any(
        |call| serde_json::from_slice::<serde_json::Value>(&call.body).unwrap()["user"] == address
    ));
}

#[test]
fn unknown_cancel_requires_one_manual_observation_and_never_resends() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 197, b"cancel-manual-alloc");
    let submitted = submit(&pic, core, &user, b"cancel-manual-order", "0.05").expect("accepted");
    let open = br#"{"status":"open","order":{"oid":4242}}"#;
    let (result, _) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", open),
        )
        .expect("dispatch");
    result.expect("sweep");
    let cancel: Result<(), ErrorCode> =
        envelope::cancel_order(&pic, core, user.caller, &user.session, submitted.order_id)
            .expect("cancel call");
    cancel.expect("cancel accepted");
    let normal = route(ACCEPTED, POSITIONS, b"[]", open);
    let (result, calls) = call_with_routed_outcalls::<
        (),
        Result<api_types::order::SweepOutcome, ErrorCode>,
        _,
    >(&pic, core, controller, "test_sweep_now", (), |call| {
        if call.url.contains("/exchange") {
            Err((1, "lost cancel reply".to_string()))
        } else {
            normal(call)
        }
    })
    .expect("cancel sweep");
    assert_eq!(result.expect("sweep").cancels, 1);
    assert_eq!(
        calls.iter().filter(|c| c.url.contains("/exchange")).count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|c| String::from_utf8_lossy(&c.body).contains("orderStatus"))
    );
    for manual in [false, true, false] {
        if manual {
            assert_eq!(
                pocket_ic_tests::resume_manual_work(
                    &pic,
                    core,
                    user.caller,
                    &user.session,
                    true,
                    "cancel"
                ),
                1
            );
        }
        let (result, calls) =
            call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
                &pic,
                core,
                controller,
                "test_sweep_now",
                (),
                route(ACCEPTED, POSITIONS, b"[]", open),
            )
            .expect("observe");
        result.expect("sweep");
        assert!(
            !calls.iter().any(|c| c.url.contains("/exchange")),
            "never resend a cancel"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| String::from_utf8_lossy(&c.body).contains("orderStatus"))
                .count(),
            usize::from(manual)
        );
    }
}

#[test]
fn lost_preflight_callback_after_upgrade_stays_stopped_and_can_be_resolved() {
    // Snapshot restoration plus immediate upgrade exceeds the install-code rate
    // limit. This case tests state recovery, not instruction-budget admission.
    let pic = pocket_ic::PocketIcBuilder::new()
        .with_application_subnet()
        .with_test_threshold_keys_subnet()
        .with_icp_config(pocket_ic::common::rest::IcpConfig {
            canister_execution_rate_limiting: Some(
                pocket_ic::common::rest::IcpConfigFlag::Disabled,
            ),
            ..Default::default()
        })
        .build();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(
        &pic,
        vault,
        core,
        controller,
        219,
        b"upgrade-preflight-alloc",
    );
    let submitted =
        submit(&pic, core, &user, b"upgrade-preflight-order", "0.01").expect("accepted");
    let saved = std::cell::RefCell::new(None);
    let normal = route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED);
    let (_, calls) = call_with_routed_outcalls::<
        (),
        Result<api_types::order::SweepOutcome, ErrorCode>,
        _,
    >(&pic, core, controller, "test_sweep_now", (), |call| {
        if call.url.contains("/exchange") {
            let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            assert_eq!(body["action"]["type"], "updateLeverage");
            *saved.borrow_mut() = Some(
                pic.take_canister_snapshot(core, Some(controller), None)
                    .expect("snapshot while awaiting POST")
                    .id,
            );
            Err((1, "lost callback".into()))
        } else {
            normal(call)
        }
    })
    .expect("initial sweep");
    assert_eq!(exchange_types(&calls), ["updateLeverage"]);
    pic.load_canister_snapshot(
        core,
        Some(controller),
        saved.into_inner().expect("pending snapshot"),
    )
    .expect("restore dispatching state");
    pic.upgrade_canister(
        core,
        pocket_ic_tests::wasm(TRADING_CORE_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .expect("upgrade resets live attempt guards");
    let before = list_orders(&pic, core, user.caller, &user.session);
    assert_eq!(
        before
            .iter()
            .find(|o| o.order_id == submitted.order_id)
            .unwrap()
            .state,
        OrderState::Pending
    );
    let risk_before =
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved;
    assert!(risk_before > 0);
    // With no sweep after upgrade, the abandoned dispatch must be normalized
    // without granting permission. Repeated requests preserve the generation.
    pocket_ic_tests::assert_manual_work_blocked(
        &pic,
        core,
        user.caller,
        &user.session,
        true,
        ("order", Some(submitted.order_id.as_ref())),
        "レバレッジ設定",
    );
    let after = list_orders(&pic, core, user.caller, &user.session);
    assert_eq!(
        after
            .iter()
            .find(|o| o.order_id == submitted.order_id)
            .unwrap()
            .state,
        OrderState::Unknown
    );
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        risk_before
    );
    pic.advance_time(std::time::Duration::from_secs(60));
    let (_, calls) =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED),
        )
        .expect("later sweep");
    assert!(
        exchange_types(&calls).is_empty(),
        "never resend the preflight or order"
    );
    let resolved: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "resolve_unknown_order_preflight",
        (submitted.order_id.clone(), PreflightResolution::Rejected),
    )
    .expect("controller resolution");
    resolved.expect("abandoned dispatch remains resolvable");
    assert_eq!(
        list_orders(&pic, core, user.caller, &user.session)
            .iter()
            .find(|o| o.order_id == submitted.order_id)
            .unwrap()
            .state,
        OrderState::Rejected
    );
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        0
    );
}

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
            Some("userFillsByTime") => {
                let mut rows: Vec<serde_json::Value> = serde_json::from_slice(&fills).unwrap();
                for fill in &mut rows {
                    fill["time"] = query["endTime"].clone();
                }
                Ok((200, serde_json::to_vec(&rows).unwrap()))
            }
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
            pocket_ic::common::rest::CanisterHttpReplication::FullyReplicated,
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
fn ambiguous_field_boundaries_are_idempotency_conflicts() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let user = provision_user(&pic, vault, core, principal(170), 222, b"fingerprint");
    let make = |quantity: &str, price: &str| SubmitOrderArgs {
        session: user.session.clone(),
        client_request_id: blob(b"same-id"),
        account_id: user.account_id.clone(),
        market: "ETH".into(),
        side: Side::Buy,
        kind: OrderKind::LimitGtc,
        quantity: quantity.into(),
        limit_price: Some(price.into()),
        slippage_tolerance_bps: None,
        reduce_only: false,
        leverage: Some(3),
        trigger: None,
        expires_after: None,
    };
    let first: Result<api_types::order::SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        user.caller,
        "submit_order",
        (user.session.clone(), make("1", "23")),
    )
    .unwrap();
    first.unwrap();
    let changed: Result<api_types::order::SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        user.caller,
        "submit_order",
        (user.session.clone(), make("12", "3")),
    )
    .unwrap();
    assert!(matches!(
        changed,
        Err(ErrorCode::IdempotencyConflict { .. })
    ));
}

#[test]
fn status_polling_rotates_and_releases_venue_terminal_orders() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 223, b"rotate-status");
    for i in 0..6 {
        submit(&pic, core, &user, &[i], "0.01").unwrap();
    }
    let next_oid = Cell::new(0u64);
    let terminal = Cell::new(false);
    let sweep = || {
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
        &pic, core, controller, "test_sweep_now", (), |call| {
            let q: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            let response = if call.url.contains("/exchange") {
                if q["action"]["type"] == "updateLeverage" { serde_json::json!({"status":"ok","response":{"type":"default"}}) }
                else { next_oid.set(next_oid.get()+1); serde_json::json!({"status":"ok","response":{"data":{"statuses":[{"resting":{"oid":next_oid.get()}}]}}}) }
            } else { match q["type"].as_str().unwrap() {
                "clearinghouseState" => serde_json::json!({"assetPositions":[],"marginSummary":{"totalMarginUsed":"0","totalNtlPos":"0"}}),
                "userFillsByTime" => serde_json::json!([]),
                "orderStatus" => { let oid=q["oid"].as_u64().unwrap(); serde_json::json!({"status": if terminal.get() && oid==5 {"marginCanceled"} else if terminal.get() && oid==6 {"iocCancelRejected"} else {"open"},"order":{"oid":oid}}) },
                other => panic!("unexpected {other}"),
            }};
            Ok((200, serde_json::to_vec(&response).unwrap()))
        }).unwrap()
    };
    sweep().0.unwrap();
    sweep().0.unwrap();
    assert_eq!(next_oid.get(), 6);
    terminal.set(true);
    for _ in 0..3 {
        pic.advance_time(std::time::Duration::from_millis(1));
        sweep().0.unwrap();
    }
    let orders: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, user.caller, &user.session, None, 10).unwrap();
    let orders = orders.unwrap().items;
    assert_eq!(
        orders.iter().find(|o| o.hl_oid == Some(5)).unwrap().state,
        OrderState::Cancelled
    );
    assert_eq!(
        orders.iter().find(|o| o.hl_oid == Some(6)).unwrap().state,
        OrderState::Rejected
    );
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        100_000_000
    );
}

#[test]
fn large_fill_history_is_ingested_and_cursor_survives_upgrade() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 225, b"large-fills");
    submit(&pic, core, &user, b"fill-order", "0.05").unwrap();
    let empty =
        br#"{"assetPositions":[],"marginSummary":{"totalMarginUsed":"0","totalNtlPos":"0"}}"#;
    let status = br#"{"status":"open","order":{"oid":4242}}"#;
    let first =
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            route(ACCEPTED, empty, b"[]", status),
        )
        .unwrap();
    first.0.unwrap();
    pic.advance_time(std::time::Duration::from_secs(121));
    let observed_start = Cell::new(0u64);
    let observed_end = Cell::new(0u64);
    let raw_bytes = Cell::new(0usize);
    let sweep = || {
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
        &pic, core, controller, "test_sweep_now", (), |call| {
            let q: serde_json::Value=serde_json::from_slice(&call.body).unwrap();
            if q["type"]=="userFillsByTime" {
                observed_start.set(q["startTime"].as_u64().unwrap());
                observed_end.set(q["endTime"].as_u64().unwrap());
                let rows: Vec<_>=(1..=200).map(|tid| serde_json::json!({"tid":tid,"oid":4242,"coin":"ETH","px":"2500","sz":"0.0001","fee":"0.000001","time":q["endTime"],"hash":"a".repeat(64),"closedPnl":"0","dir":"Open Long"})).collect();
                let bytes=serde_json::to_vec(&rows).unwrap(); raw_bytes.set(bytes.len());
                Ok((200,bytes))
            } else { route(ACCEPTED, empty, b"[]", status)(call) }
        }).unwrap()
    };
    let checked = sweep();
    checked.0.unwrap();
    assert!(raw_bytes.get() > 32 * 1024);
    let page = envelope::list_fills(&pic, core, user.caller, &user.session, None, 100)
        .unwrap()
        .unwrap();
    assert_eq!(page.items.len(), 100);
    let next = envelope::list_fills(
        &pic,
        core,
        user.caller,
        &user.session,
        page.next_cursor,
        100,
    )
    .unwrap()
    .unwrap();
    assert_eq!(next.items.len(), 100);
    let previous_end = observed_end.get();
    pic.upgrade_canister(
        core,
        pocket_ic_tests::wasm(TRADING_CORE_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .unwrap();
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, core, controller, "get_journal_guard", ()).unwrap();
    let resumed: Result<(), ErrorCode> =
        update(&pic, core, guard.unwrap().unwrap(), "resume_journal", ()).unwrap();
    resumed.unwrap();
    pic.advance_time(std::time::Duration::from_secs(121));
    sweep().0.unwrap();
    assert_eq!(observed_start.get(), previous_end);
    assert_eq!(
        account_snapshot(&pic, core, user.caller, &user.session).open_order_risk_reserved,
        125_000_000
    );
}

#[test]
fn full_fill_pages_keep_an_inclusive_boundary_and_continue_next_sweep() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 226, b"paged-fills");
    submit(&pic, core, &user, b"page-order", "0.05").unwrap();
    let empty =
        br#"{"assetPositions":[],"marginSummary":{"totalMarginUsed":"0","totalNtlPos":"0"}}"#;
    let status = br#"{"status":"open","order":{"oid":4242}}"#;
    call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
        &pic,
        core,
        controller,
        "test_sweep_now",
        (),
        route(ACCEPTED, empty, b"[]", status),
    )
    .unwrap()
    .0
    .unwrap();
    pic.advance_time(std::time::Duration::from_secs(121));
    let first_start = Cell::new(0u64);
    let page_number = Cell::new(0u32);
    let sweep = || {
        call_with_routed_outcalls::<(),Result<api_types::order::SweepOutcome,ErrorCode>,_>(
        &pic,core,controller,"test_sweep_now",(),|call| {
            let q:serde_json::Value=serde_json::from_slice(&call.body).unwrap();
            if q["type"]=="userFillsByTime" {
                let start=q["startTime"].as_u64().unwrap();
                let rows:Vec<_>=if page_number.get()==0 {
                    first_start.set(start);
                    (0..2000).map(|i|serde_json::json!({"tid":i+1,"oid":999,"coin":"ETH","px":"2500","sz":"0.000001","fee":"0","time":start+i})).collect()
                } else {
                    assert_eq!(start,first_start.get()+1999);
                    vec![serde_json::json!({"tid":2000,"oid":999,"coin":"ETH","px":"2500","sz":"0.000001","fee":"0","time":start}),serde_json::json!({"tid":2001,"oid":999,"coin":"ETH","px":"2500","sz":"0.000001","fee":"0","time":start+1})]
                };
                page_number.set(page_number.get()+1);
                Ok((200,serde_json::to_vec(&rows).unwrap()))
            } else { route(ACCEPTED,empty,b"[]",status)(call) }
        }).unwrap()
    };
    sweep().0.unwrap();
    sweep().0.unwrap();
    assert_eq!(
        page_number.get(),
        2,
        "full pages continue without the regular polling delay"
    );
}

#[test]
fn recovered_order_rewind_survives_failed_fetch_and_upgrade() {
    let pic = pic();
    let (vault, core) = setup(&pic);
    let controller = principal(170);
    let user = provision_user(&pic, vault, core, controller, 226, b"durable-rewind");
    let submitted = submit(&pic, core, &user, b"lost-rewind-order", "0.05").unwrap();
    let cloid = std::cell::RefCell::new(String::new());
    let phase = Cell::new(0u8);
    let recent_cursor = Cell::new(0u64);
    let rewind_start = Cell::new(0u64);
    let fill_requests = Cell::new(0u32);
    let sweep = || {
        call_with_routed_outcalls::<(), Result<api_types::order::SweepOutcome, ErrorCode>, _>(
            &pic,
            core,
            controller,
            "test_sweep_now",
            (),
            |call| {
                let q: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                if q["action"]["type"] == "order" {
                    assert_eq!(phase.get(), 0, "must not resend order");
                    *cloid.borrow_mut() = q["action"]["orders"][0]["c"].as_str().unwrap().into();
                    return Err((4, "lost order response".into()));
                }
                if q["type"] == "orderStatus" {
                    return Ok((
                        200,
                        serde_json::to_vec(&serde_json::json!({
                            "status": if phase.get() < 2 { "unknown" } else { "filled" },
                            "order": { "oid":4242, "cloid":*cloid.borrow() }
                        }))
                        .unwrap(),
                    ));
                }
                if q["type"] == "userFillsByTime" {
                    fill_requests.set(fill_requests.get() + 1);
                    let start = q["startTime"].as_u64().unwrap();
                    if phase.get() < 2 {
                        recent_cursor.set(q["endTime"].as_u64().unwrap());
                        return Ok((200, b"[]".to_vec()));
                    }
                    if phase.get() == 2 {
                        assert!(start < recent_cursor.get(), "recovered oid must rewind");
                        rewind_start.set(start);
                        return Err((4, "fill fetch unavailable".into()));
                    }
                    assert_eq!(
                        start,
                        rewind_start.get(),
                        "failed rewind must survive upgrade"
                    );
                    return Ok((
                        200,
                        serde_json::to_vec(&serde_json::json!([{
                            "tid":9001, "oid":4242, "coin":"ETH", "px":"2500",
                            "sz":"0.05", "fee":"0.000120", "time":start
                        }]))
                        .unwrap(),
                    ));
                }
                route(ACCEPTED, POSITIONS, b"[]", STATUS_FILLED)(call)
            },
        )
        .unwrap()
        .0
        .unwrap()
    };
    sweep();
    phase.set(1);
    pic.advance_time(std::time::Duration::from_secs(130));
    sweep();
    assert!(recent_cursor.get() > 0);
    phase.set(2);
    pic.advance_time(std::time::Duration::from_millis(1));
    sweep();
    assert!(rewind_start.get() > 0);
    assert_eq!(
        list_orders(&pic, core, user.caller, &user.session)[0].state,
        OrderState::Filled
    );
    pic.upgrade_canister(
        core,
        pocket_ic_tests::wasm(TRADING_CORE_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .unwrap();
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, core, controller, "get_journal_guard", ()).unwrap();
    let resumed: Result<(), ErrorCode> =
        update(&pic, core, guard.unwrap().unwrap(), "resume_journal", ()).unwrap();
    resumed.unwrap();
    phase.set(3);
    let before = fill_requests.get();
    sweep();
    assert_eq!(
        fill_requests.get(),
        before + 1,
        "rewound history remains due immediately"
    );
    let fills = envelope::list_fills(&pic, core, user.caller, &user.session, None, 10)
        .unwrap()
        .unwrap();
    assert_eq!(fills.items.len(), 1);
    assert_eq!(fills.items[0].order_id, submitted.order_id);
}

//! 決済（部分・全決済）の試験。反対売買のreduce-only注文として送る。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::fund::{AgentGeneration, AllocationRequest, FundRequestAccepted};
use api_types::order::{
    AccountSnapshot, CloseAllOutcome, OrderState, OrderSummary, SubmitOrderResult,
};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, configure_policy, deploy,
    envelope, pic, principal, rotate_hpke_key, sweep_with_venue_outcalls, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;
/// ETHロングとBTCショートの建玉（`/info`のclearinghouseState相当）。
const STATE: &str = r#"{"marginSummary":{"totalMarginUsed":"0"},"assetPositions":[
  {"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","unrealizedPnl":"0","leverage":{"value":3},"marginMode":"cross"}},
  {"position":{"coin":"BTC","szi":"-0.02","entryPx":"60000","unrealizedPnl":"0","leverage":{"value":3},"marginMode":"cross"}}
]}"#;

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

/// vault・core・policy・metaを用意し、取引可能な口座とセッションを返す。
fn setup(pic: &PocketIc) -> (Principal, SessionHandle) {
    let controller = principal(250);
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
    rotate_hpke_key(pic, core, controller);
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

    let caller = principal(251);
    let session = open_session(pic, vault, caller, &secret(252));
    let credit: Result<(), ErrorCode> = update_args(
        pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 5_000_000_000u64, blob(&[253u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<FundRequestAccepted, ErrorCode> = update(
        pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"close-alloc"),
            amount: 3_000_000_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");
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
    (core, session)
}

fn ingest_positions(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    session: &SessionHandle,
    state: &str,
) {
    let ingested: Result<u32, ErrorCode> = update_args(
        pic,
        core,
        caller,
        "test_ingest_positions",
        (session.clone(), state.to_string()),
    )
    .expect("call");
    ingested.expect("ingest positions");
}

#[allow(clippy::too_many_arguments)]
fn close(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    session: &SessionHandle,
    request_id: &[u8],
    market: &str,
    ratio_bps: u32,
    limit_price: Option<&str>,
) -> Result<SubmitOrderResult, ErrorCode> {
    update_args(
        pic,
        core,
        caller,
        "close_position",
        (
            session.clone(),
            blob(request_id),
            market.to_string(),
            ratio_bps,
            limit_price.map(|price| price.to_string()),
        ),
    )
    .expect("call")
}

fn list_orders(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> Vec<OrderSummary> {
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(pic, core, caller, session, None::<Blob>, 10u32).expect("call");
    listed.expect("list_orders").items
}

fn snapshot(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> AccountSnapshot {
    let snapshot: Result<AccountSnapshot, ErrorCode> =
        envelope::get_account_snapshot(pic, core, caller, session).expect("call");
    snapshot.expect("snapshot")
}

fn order_of(orders: &[OrderSummary], order_id: &Blob) -> OrderSummary {
    orders
        .iter()
        .find(|order| order.order_id.as_ref() == order_id.as_ref())
        .cloned()
        .expect("the order is listed")
}

/// 決済は建玉の符号から反対売買のreduce-only IOC注文になる。
#[test]
fn closing_a_position_sends_an_opposite_reduce_only_order() {
    let pic = pic();
    let (core, session) = setup(&pic);
    let caller = principal(251);
    ingest_positions(&pic, core, caller, &session, STATE);

    // ロングの決済は売り。価格は観測した建玉から導出する（entry 2500 − 50bps）。
    let long_close = close(
        &pic,
        core,
        caller,
        &session,
        b"close-eth-long",
        "ETH",
        10_000,
        None,
    )
    .expect("close long");
    let order = order_of(
        &list_orders(&pic, core, caller, &session),
        &long_close.order_id,
    );
    assert!(!order.is_buy, "ロングの決済は売り");
    assert!(order.reduce_only);
    assert_eq!(order.kind, "market_ioc");
    assert_eq!(order.quantity, "0.05", "全量決済");
    assert_eq!(order.price.as_deref(), Some("2487.5"));

    // ショートの決済は買い（スリッページ上限は上側）。
    let short_close = close(
        &pic,
        core,
        caller,
        &session,
        b"close-btc-short",
        "BTC",
        10_000,
        None,
    )
    .expect("close short");
    let order = order_of(
        &list_orders(&pic, core, caller, &session),
        &short_close.order_id,
    );
    assert!(order.is_buy, "ショートの決済は買い");
    assert_eq!(order.quantity, "0.02");
    assert_eq!(order.price.as_deref(), Some("60300"));

    // 画面が公開市況から決めた価格を渡した場合はそれを使う。
    let explicit = close(
        &pic,
        core,
        caller,
        &session,
        b"close-eth-explicit",
        "ETH",
        10_000,
        Some("2510"),
    )
    .expect("close with an explicit price");
    let order = order_of(
        &list_orders(&pic, core, caller, &session),
        &explicit.order_id,
    );
    assert_eq!(order.price.as_deref(), Some("2510"));

    // 建玉が無い銘柄は決済しない（SOLは建玉なし）。
    let no_position = close(
        &pic,
        core,
        caller,
        &session,
        b"close-sol",
        "SOL",
        10_000,
        None,
    );
    assert!(
        matches!(no_position, Err(ErrorCode::NotAllowed { .. })),
        "{no_position:?}"
    );

    // 比率は1..=10000に限る。
    let zero = close(&pic, core, caller, &session, b"close-zero", "ETH", 0, None);
    assert!(
        matches!(
            zero,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::QuantityOutOfRange,
                ..
            })
        ),
        "{zero:?}"
    );
    let over = close(
        &pic,
        core,
        caller,
        &session,
        b"close-over",
        "ETH",
        10_001,
        None,
    );
    assert!(over.is_err(), "{over:?}");
}

/// 部分決済は指定した比率の数量を`szDecimals`で切り捨てて送る。
#[test]
fn partial_close_uses_the_requested_ratio_and_truncates() {
    let pic = pic();
    let (core, session) = setup(&pic);
    let caller = principal(251);
    ingest_positions(&pic, core, caller, &session, STATE);

    let half = close(
        &pic,
        core,
        caller,
        &session,
        b"close-half",
        "ETH",
        5_000,
        None,
    )
    .expect("close half");
    let order = order_of(&list_orders(&pic, core, caller, &session), &half.order_id);
    assert_eq!(order.quantity, "0.025", "0.05の50%");

    // 0.05 × 3333bps = 0.016665 → szDecimals=5で切り捨てて0.01666。
    let third = close(
        &pic,
        core,
        caller,
        &session,
        b"close-third",
        "ETH",
        3_333,
        None,
    )
    .expect("close a third");
    let order = order_of(&list_orders(&pic, core, caller, &session), &third.order_id);
    assert_eq!(order.quantity, "0.01666");

    // 切り捨てて0になる比率は受付しない。
    ingest_positions(
        &pic,
        core,
        caller,
        &session,
        r#"{"marginSummary":{"totalMarginUsed":"0"},"assetPositions":[{"position":{"coin":"ETH","szi":"0.00001","entryPx":"2500","unrealizedPnl":"0","leverage":{"value":3}}}]}"#,
    );
    let too_small = close(
        &pic,
        core,
        caller,
        &session,
        b"close-too-small",
        "ETH",
        5_000,
        None,
    );
    assert!(
        matches!(
            too_small,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::QuantityOutOfRange,
                ..
            })
        ),
        "{too_small:?}"
    );
}

/// 全決済は建玉ごとに反対売買を送り、建玉が0になると対象が無くなる。
#[test]
fn close_all_submits_one_reduce_only_order_per_open_position() {
    let pic = pic();
    let (core, session) = setup(&pic);
    let caller = principal(251);
    ingest_positions(&pic, core, caller, &session, STATE);

    let outcome: Result<CloseAllOutcome, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "close_all",
        (session.clone(), blob(b"close-all-1")),
    )
    .expect("call");
    let outcome = outcome.expect("close_all");
    assert_eq!(outcome.submitted.len(), 2, "建玉2件を決済する");
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);

    // 同じ受付IDの再送は同じ注文を返す（冪等）。
    let again: Result<CloseAllOutcome, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "close_all",
        (session.clone(), blob(b"close-all-1")),
    )
    .expect("call");
    let again = again.expect("close_all");
    assert_eq!(again.submitted.len(), 2);
    let mut first: Vec<Vec<u8>> = outcome
        .submitted
        .iter()
        .map(|result| result.order_id.as_ref().to_vec())
        .collect();
    let mut second: Vec<Vec<u8>> = again
        .submitted
        .iter()
        .map(|result| result.order_id.as_ref().to_vec())
        .collect();
    first.sort();
    second.sort();
    assert_eq!(first, second, "同じ受付IDは同じ注文");

    let orders = list_orders(&pic, core, caller, &session);
    for result in &outcome.submitted {
        let order = order_of(&orders, &result.order_id);
        assert!(order.reduce_only);
        assert_eq!(order.kind, "market_ioc");
    }
    // 建玉が消えた後の全決済は対象が無い（空の結果）。
    ingest_positions(&pic, core, caller, &session, r#"{"assetPositions":[]}"#);
    let empty: Result<CloseAllOutcome, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "close_all",
        (session.clone(), blob(b"close-all-2")),
    )
    .expect("call");
    let empty = empty.expect("close_all");
    assert!(empty.submitted.is_empty());
    assert!(empty.failed.is_empty());
}

/// 決済注文の送信と約定の取り込みで建玉が0になる。
#[test]
fn a_closed_position_reaches_zero_after_dispatch_and_fills() {
    let pic = pic();
    let (core, session) = setup(&pic);
    let caller = principal(251);
    // ETHのみ（送信は1件ずつmockする）。
    ingest_positions(
        &pic,
        core,
        caller,
        &session,
        r#"{"marginSummary":{"totalMarginUsed":"0"},"assetPositions":[{"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","unrealizedPnl":"0","leverage":{"value":3},"marginMode":"cross"}}]}"#,
    );

    let closed = close(
        &pic,
        core,
        caller,
        &session,
        b"close-roundtrip",
        "ETH",
        10_000,
        None,
    )
    .expect("close");
    let before = snapshot(&pic, core, caller, &session);
    assert_eq!(before.positions.len(), 1);
    assert_eq!(before.positions[0].size, "0.05");

    let venue_body =
        br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":4242}}]}}}"#
            .to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, venue_body).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    let ingested: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (
            session.clone(),
            r#"[{"tid":9001,"oid":4242,"coin":"ETH","px":"2480","sz":"0.05","fee":120,"time":1700000000000}]"#
                .to_string(),
        ),
    )
    .expect("call");
    assert_eq!(ingested.expect("ingest fills"), 1);

    // 取引所は決済済みの建玉を返さない（全量観測）。
    ingest_positions(&pic, core, caller, &session, r#"{"assetPositions":[]}"#);
    let after = snapshot(&pic, core, caller, &session);
    assert!(after.positions.is_empty(), "建玉が0になる");
    let order = order_of(&list_orders(&pic, core, caller, &session), &closed.order_id);
    assert_eq!(order.state, OrderState::Filled);
    assert_eq!(order.filled_quantity, "0.05");
}

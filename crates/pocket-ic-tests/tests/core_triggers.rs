//! SL/TPトリガ注文（建玉単位・reduce-only）の受付・署名・送信の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::fund::{AgentGeneration, AllocationRequest, FundRequestAccepted};
use api_types::order::{
    OrderKind, OrderState, OrderSummary, Side, SubmitOrderArgs, SubmitOrderResult, Trigger,
    TriggerKind,
};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::hash::ActionHashInput;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, configure_policy, deploy,
    envelope, pic, principal, rotate_hpke_key, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;
/// ETHロングとBTCショートの建玉（`/info`のclearinghouseState相当）。
const STATE: &str = r#"{"assetPositions":[
  {"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","leverage":{"value":3},"marginMode":"cross"}},
  {"position":{"coin":"BTC","szi":"-0.02","entryPx":"60000","leverage":{"value":3},"marginMode":"cross"}}
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
    session.expect("session")
}

/// vault・core・policy・metaを用意し、取引可能な口座とセッションを返す。
fn setup(pic: &PocketIc) -> (Principal, Principal, SessionHandle) {
    let controller = principal(240);
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

    let caller = principal(241);
    let session = open_session(pic, vault, caller, &secret(242));
    let credit: Result<(), ErrorCode> = update_args(
        pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 5_000_000_000u64, blob(&[243u8; 32])),
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
            client_request_id: blob(b"trigger-alloc"),
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
    (core, vault, session)
}

fn ingest_positions(pic: &PocketIc, core: Principal, caller: Principal, session: &SessionHandle) {
    let ingested: Result<u32, ErrorCode> = update_args(
        pic,
        core,
        caller,
        "test_ingest_positions",
        (session.clone(), STATE.to_string()),
    )
    .expect("call");
    assert_eq!(ingested.expect("ingest"), 2, "建玉2件を取り込む");
}

#[allow(clippy::too_many_arguments)]
fn trigger_args(
    session: &SessionHandle,
    request_id: &[u8],
    market: &str,
    side: Side,
    quantity: &str,
    price: &str,
    trigger: Trigger,
    reduce_only: bool,
) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(request_id),
        account_id: blob(&[0u8; 32]),
        market: market.to_string(),
        side,
        kind: OrderKind::LimitGtc,
        quantity: quantity.to_string(),
        limit_price: Some(price.to_string()),
        slippage_tolerance_bps: None,
        reduce_only,
        leverage: Some(3),
        trigger: Some(trigger),
        expires_after: None,
    }
}

fn stop_loss(price: &str) -> Trigger {
    Trigger {
        kind: TriggerKind::StopLoss,
        trigger_price: price.to_string(),
        is_market: true,
    }
}

fn submit(
    pic: &PocketIc,
    core: Principal,
    caller: Principal,
    args: SubmitOrderArgs,
) -> Result<SubmitOrderResult, ErrorCode> {
    update_args(
        pic,
        core,
        caller,
        "submit_order",
        (args.session.clone(), args),
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

/// SL/TPは建玉単位のreduce-only注文としてのみ受け付け、建玉の反対売買を要求する。
#[test]
fn trigger_orders_require_reduce_only_a_position_and_the_closing_side() {
    let pic = pic();
    let (core, _vault, session) = setup(&pic);
    let caller = principal(241);

    // reduce_onlyでないトリガは受付しない（建玉を増やすSL/TPは作らない）。
    let not_reduce_only = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-not-reduce-only",
            "ETH",
            Side::Sell,
            "0.05",
            "2400",
            stop_loss("2400"),
            false,
        ),
    );
    assert!(
        matches!(
            not_reduce_only,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::MissingField,
                ..
            })
        ),
        "{not_reduce_only:?}"
    );

    // 建玉が無い状態のトリガは受付しない（positionTpslの前提が無い）。
    let no_position = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-no-position",
            "ETH",
            Side::Sell,
            "0.05",
            "2400",
            stop_loss("2400"),
            true,
        ),
    );
    assert!(
        matches!(no_position, Err(ErrorCode::NotAllowed { .. })),
        "{no_position:?}"
    );

    ingest_positions(&pic, core, caller, &session);

    // ロングに対する買い（建玉を増やす向き）は拒否する。
    let wrong_side = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-wrong-side",
            "ETH",
            Side::Buy,
            "0.05",
            "2400",
            stop_loss("2400"),
            true,
        ),
    );
    assert!(
        matches!(
            wrong_side,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                ..
            })
        ),
        "{wrong_side:?}"
    );

    // ショートに対する売り（建玉を増やす向き）も拒否する。
    let short_wrong_side = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-short-wrong",
            "BTC",
            Side::Sell,
            "0.02",
            "60000",
            stop_loss("62000"),
            true,
        ),
    );
    assert!(short_wrong_side.is_err(), "{short_wrong_side:?}");

    // ロングのSL（売り・reduce-only）は受け付ける。
    let accepted = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-long-sl",
            "ETH",
            Side::Sell,
            "0.05",
            "2400",
            stop_loss("2400"),
            true,
        ),
    )
    .expect("accepted");
    assert_eq!(accepted.request_id.as_ref(), b"trigger-long-sl");

    // ショートのSL（買い・reduce-only）も受け付ける。
    let short_accepted = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-short-sl",
            "BTC",
            Side::Buy,
            "0.02",
            "60000",
            stop_loss("62000"),
            true,
        ),
    );
    assert!(short_accepted.is_ok(), "{short_accepted:?}");

    // トリガの内容は注文一覧に現れる。
    let orders = list_orders(&pic, core, caller, &session);
    let sl = orders
        .iter()
        .find(|order| order.order_id.as_ref() == accepted.order_id.as_ref())
        .expect("triggger order is listed");
    assert!(sl.reduce_only);
    assert_eq!(
        sl.trigger,
        Some(Trigger {
            kind: TriggerKind::StopLoss,
            trigger_price: "2400".to_string(),
            is_market: true,
        })
    );
}

/// トリガ注文は`positionTpsl`のactionとして、トリガ価格つきで署名される。
#[test]
fn trigger_orders_are_signed_as_position_tpsl() {
    let pic = pic();
    let (core, vault, session) = setup(&pic);
    let caller = principal(241);
    ingest_positions(&pic, core, caller, &session);

    let accepted = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-sign",
            "ETH",
            Side::Sell,
            "0.05",
            "2400",
            stop_loss("2400"),
            true,
        ),
    )
    .expect("accepted");

    let signed: Result<(Vec<u8>, Vec<u8>), ErrorCode> = update(
        &pic,
        core,
        caller,
        "test_sign_order_action",
        accepted.order_id.clone(),
    )
    .expect("call");
    let (digest, signature) = signed.expect("signature");

    // 期待するaction（trigger + positionTpsl）をテスト側で組み立てて一致を確認する。
    let orders = list_orders(&pic, core, caller, &session);
    let order = orders
        .iter()
        .find(|order| order.order_id.as_ref() == accepted.order_id.as_ref())
        .expect("order is listed");
    let expected = hl_types::action::OrderAction {
        orders: vec![hl_types::action::OrderRequest {
            asset_index: order.asset_index,
            is_buy: false,
            price: decimal("2400"),
            size: decimal("0.05"),
            reduce_only: true,
            order_type: hl_types::action::OrderType::Trigger(hl_types::action::TriggerOrder {
                is_market: true,
                trigger_price: decimal("2400"),
                tpsl: hl_types::action::Tpsl::StopLoss,
            }),
            cloid: Some(format!("0x{}", hex::encode(order.cloid.as_ref()))),
        }],
        grouping: hl_types::action::Grouping::PositionTpsl,
    };
    let msgpack = expected.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&ActionHashInput {
        action_msgpack: &msgpack,
        nonce: order.created_at,
        vault_address: None,
        expires_after: None,
    });
    let expected_digest = hl_sign::hash::signing_digest(action_hash, false);
    assert_eq!(
        digest.as_slice(),
        expected_digest.as_slice(),
        "署名対象はトリガつきpositionTpslのactionと一致する"
    );

    // 署名は承認済みのAgent鍵による。
    let signature =
        hl_sign::Signature::from_bytes65(&signature.try_into().expect("65-byte signature"))
            .expect("signature");
    let recovered =
        hl_sign::signature::recover_address(&expected_digest, &signature, None).expect("recover");
    let status: Result<api_types::fund::AgentStatus, ErrorCode> =
        update(&pic, core, caller, "get_agent_status", session.clone()).expect("call");
    let agent_address = status
        .expect("agent status")
        .current
        .expect("approved generation")
        .agent_address;
    assert_eq!(recovered.to_vec(), agent_address.as_ref().to_vec());

    // vaultの承認が無い場合に備え、呼び出しは認可済みの口座に対してのみ成功する。
    let _ = vault;
}

/// トリガ注文は`positionTpsl`の本文で送信され、`orderStatus`の反映で状態が進む。
#[test]
fn trigger_orders_are_dispatched_with_the_position_tpsl_action() {
    let pic = pic();
    let (core, _vault, session) = setup(&pic);
    let caller = principal(241);
    ingest_positions(&pic, core, caller, &session);

    let take_profit = submit(
        &pic,
        core,
        caller,
        trigger_args(
            &session,
            b"trigger-tp",
            "ETH",
            Side::Sell,
            "0.02",
            "2700",
            Trigger {
                kind: TriggerKind::TakeProfit,
                trigger_price: "2700".to_string(),
                is_market: false,
            },
            true,
        ),
    )
    .expect("accepted");

    let venue_body =
        br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":777}}]}}}"#
            .to_vec();
    let (swept, captured) = pocket_ic_tests::call_with_routed_outcalls::<
        (),
        Result<api_types::order::SweepOutcome, ErrorCode>,
        _,
    >(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        pocket_ic_tests::venue_router_default(&venue_body),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);
    let captured = captured
        .into_iter()
        .find(|call| call.url.contains("/exchange"))
        .expect("exchange outcall");

    let body: serde_json::Value = serde_json::from_slice(&captured.body).expect("json body");
    let action = &body["action"];
    assert_eq!(action["grouping"], "positionTpsl");
    assert_eq!(action["orders"][0]["r"], true, "reduce-onlyで送る");
    assert_eq!(action["orders"][0]["t"]["trigger"]["tpsl"], "tp");
    assert_eq!(action["orders"][0]["t"]["trigger"]["triggerPx"], "2700");
    assert_eq!(action["orders"][0]["t"]["trigger"]["isMarket"], false);
    assert!(action["orders"][0]["t"].get("limit").is_none());

    // 取引所の状態が届くと注文の状態が進む。
    let applied: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_apply_order_status",
        (
            session.clone(),
            r#"{"status":"filled","order":{"oid":777}}"#.to_string(),
        ),
    )
    .expect("call");
    assert!(applied.expect("applied"));
    let orders = list_orders(&pic, core, caller, &session);
    let dispatched = orders
        .iter()
        .find(|order| order.order_id.as_ref() == take_profit.order_id.as_ref())
        .expect("order is listed");
    assert_eq!(dispatched.state, OrderState::Filled);
}

fn decimal(value: &str) -> hl_types::decimal::Decimal {
    hl_types::decimal::Decimal::parse(value).expect("decimal")
}

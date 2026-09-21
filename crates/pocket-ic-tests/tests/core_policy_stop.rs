//! 緊急停止中の新規受付拒否（`policy_registry` 参照）の試験。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AgentGeneration, AllocationRequest, FundRequestAccepted};
use api_types::order::{OrderKind, Side, SubmitOrderArgs, SubmitOrderResult};
use api_types::{AccountKind, Blob};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, POLICY_WASM, TRADING_CORE_WASM, deploy, deploy_default, pic, principal,
    update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]"#;

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

fn order_args(session: &SessionHandle, request_id: &[u8], market: &str) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(request_id),
        account_id: blob(&[0u8; 32]),
        market: market.to_string(),
        side: Side::Buy,
        kind: OrderKind::LimitGtc,
        quantity: "0.05".to_string(),
        limit_price: Some("2500".to_string()),
        slippage_tolerance_bps: None,
        reduce_only: false,
        leverage: Some(3),
        trigger: None,
        expires_after: None,
    }
}

#[test]
fn a_stopped_policy_blocks_new_orders() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(150);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_policy_principal", policy).expect("call");
    set.expect("set_policy_principal");
    let operator: Result<(), ErrorCode> =
        update(&pic, policy, controller, "set_operator", controller).expect("call");
    operator.expect("set_operator");
    let sns: Result<(), ErrorCode> =
        update(&pic, policy, controller, "set_sns_principal", controller).expect("call");
    sns.expect("set_sns_principal");
    // 政策未設定は安全側（停止）を返すため、まずSNS経路で解除して受付可能にする。
    let armed: Result<(), ErrorCode> =
        update(&pic, policy, controller, "clear_emergency_stop", ()).expect("call");
    armed.expect("armed");

    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(151);
    let session = open_session(&pic, vault, caller, &secret(200));
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[151u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"stop-alloc"),
            amount: 300_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");
    let agent: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");

    // 停止前は受け付ける。
    let before: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (session.clone(), order_args(&session, b"stop-before", "ETH")),
    )
    .expect("call");
    before.expect("accepted before stop");

    // 停止中は拒否する。
    let stop: Result<(), ErrorCode> =
        update(&pic, policy, controller, "set_emergency_stop", ()).expect("call");
    stop.expect("stop");
    let blocked: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (session.clone(), order_args(&session, b"stop-during", "ETH")),
    )
    .expect("call");
    assert!(
        matches!(blocked, Err(ErrorCode::NotAllowed { .. })),
        "{blocked:?}"
    );

    // 解除後は再び受け付ける。
    let clear: Result<(), ErrorCode> =
        update(&pic, policy, controller, "clear_emergency_stop", ()).expect("call");
    clear.expect("clear");
    let after: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (session.clone(), order_args(&session, b"stop-after", "ETH")),
    )
    .expect("call");
    after.expect("accepted after clear");
}

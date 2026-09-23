//! 緊急停止中の新規受付拒否（`policy_registry` 参照）の試験。

use api_types::Blob;
use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::AgentGeneration;
use api_types::order::{OrderKind, Side, SubmitOrderArgs, SubmitOrderResult};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, POLICY_WASM, TRADING_CORE_WASM, deploy, fund_trading_account, pic, principal,
    update, update_args,
};
use pocket_ic_tests::{observe_empty_account, trading_account_id};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;

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

fn order_args(
    account_id: &Blob,
    session: &SessionHandle,
    request_id: &[u8],
    market: &str,
) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(request_id),
        account_id: account_id.clone(),
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
    let controller = principal(150);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
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
    let guard: Result<(), ErrorCode> =
        update(&pic, policy, controller, "set_guard_principal", controller).expect("call");
    guard.expect("set_guard_principal");
    // allowlistもpolicyから取得する（未設定はfail-closed）。
    let version: Result<(), ErrorCode> = update_args(
        &pic,
        policy,
        controller,
        "set_policy_version",
        (1u64, vec!["BTC".to_string(), "ETH".to_string()]),
    )
    .expect("call");
    version.expect("set_policy_version");
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
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"stop-alloc-fund",
        5_000_000_000,
        151,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
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
        (
            session.clone(),
            order_args(&account_id, &session, b"stop-before", "ETH"),
        ),
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
        (
            session.clone(),
            order_args(&account_id, &session, b"stop-during", "ETH"),
        ),
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
        (
            session.clone(),
            order_args(&account_id, &session, b"stop-after", "ETH"),
        ),
    )
    .expect("call");
    after.expect("accepted after clear");
}

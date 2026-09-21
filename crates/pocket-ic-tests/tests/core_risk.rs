//! リスク予約（受付時の確保と拒否時の解放）の試験。

use api_types::Blob;
use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::AgentGeneration;
use api_types::order::{AccountSnapshot, OrderKind, Side, SubmitOrderArgs, SubmitOrderResult};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, call_with_mocked_outcall, configure_policy, deploy,
    fund_trading_account, pic, principal, update, update_args,
};

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

fn order_args(session: &SessionHandle) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(b"risk-1"),
        account_id: blob(&[0u8; 32]),
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
    }
}

#[test]
fn risk_is_reserved_on_acceptance_and_released_on_rejection() {
    let pic = pic();
    let controller = principal(160);
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
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
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

    let caller = principal(161);
    let session = open_session(&pic, vault, caller, &secret(210));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"risk-alloc-fund",
        5_000_000_000,
        161,
    );
    let agent: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (session.clone(), order_args(&session)),
    )
    .expect("call");
    submitted.expect("accepted order");

    // 受付時に 2500 × 0.05 = 125 USDC が予約される。
    let snapshot: Result<AccountSnapshot, ErrorCode> =
        update(&pic, core, caller, "get_account_snapshot", session.clone()).expect("call");
    assert_eq!(
        snapshot.expect("snapshot").margin_used,
        125_000_000,
        "価格×数量の想定元本を予約する"
    );

    // 取引所が拒否したら解放する。
    let rejected = br#"{"status":"err","response":"insufficient margin"}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, rejected)),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let snapshot: Result<AccountSnapshot, ErrorCode> =
        update(&pic, core, caller, "get_account_snapshot", session.clone()).expect("call");
    assert_eq!(
        snapshot.expect("snapshot").margin_used,
        0,
        "拒否されたら予約を解放する"
    );
}

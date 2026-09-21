//! 注文受付の入力検証（2E）の試験。

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
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, configure_policy, deploy, deploy_default, pic, principal,
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

fn args(
    session: &SessionHandle,
    request_id: &[u8],
    market: &str,
    quantity: &str,
    price: Option<&str>,
    leverage: u32,
) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(request_id),
        account_id: blob(&[0u8; 32]),
        market: market.to_string(),
        side: Side::Buy,
        kind: OrderKind::LimitGtc,
        quantity: quantity.to_string(),
        limit_price: price.map(|value| value.to_string()),
        slippage_tolerance_bps: None,
        reduce_only: false,
        leverage: Some(leverage),
        trigger: None,
        expires_after: None,
    }
}

#[test]
fn invalid_orders_are_rejected_without_side_effects() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(180);
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
    // allowlist外の銘柄（UNIVERSEにはあるがallowlistに無いSOL）を検証するため、
    // BTCとETHだけを許可する。
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

    let caller = principal(181);
    let session = open_session(&pic, vault, caller, &secret(230));
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 10_000_000_000u64, blob(&[171u8; 32])),
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
            client_request_id: blob(b"validate-alloc"),
            amount: 5_000_000_000,
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

    // 7桁の小数は拒否する（マイクロ単位へ丸めない）。
    let too_precise: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            args(&session, b"v-1", "ETH", "0.1234567", Some("2500"), 3),
        ),
    )
    .expect("call");
    assert!(
        matches!(too_precise, Err(ErrorCode::BadRequest { .. })),
        "{too_precise:?}"
    );

    // 価格の無い注文は拒否する（Market IOCもスリッページ上限の指値が必須）。
    let no_price: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            args(&session, b"v-2", "ETH", "0.05", None, 3),
        ),
    )
    .expect("call");
    assert!(
        matches!(no_price, Err(ErrorCode::BadRequest { .. })),
        "{no_price:?}"
    );

    // UI上限を超えるレバレッジは拒否する。
    let too_much_leverage: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            args(&session, b"v-3", "ETH", "0.05", Some("2500"), 6),
        ),
    )
    .expect("call");
    assert!(
        matches!(too_much_leverage, Err(ErrorCode::BadRequest { .. })),
        "{too_much_leverage:?}"
    );

    // allowlist外の銘柄は拒否する。
    let not_allowed: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            args(&session, b"v-4", "SOL", "1", Some("100"), 3),
        ),
    )
    .expect("call");
    assert!(
        matches!(not_allowed, Err(ErrorCode::NotAllowed { .. })),
        "{not_allowed:?}"
    );

    // いずれも予約を残さない。
    let snapshot: Result<api_types::order::AccountSnapshot, ErrorCode> =
        update(&pic, core, caller, "get_account_snapshot", session.clone()).expect("call");
    let snapshot = snapshot.expect("snapshot");
    assert_eq!(snapshot.margin_used, 0, "拒否された注文は予約を残さない");
    assert!(snapshot.pending_orders.is_empty());
}

/// 政策が未設定なら新規受注を拒否する（fail-closed）。
///
/// 以前は policy principal 未設定時に停止判定をskipしていたため、統制が
/// 効かないまま注文を受け付けていた（`authority-matrix.md` は読み取り失敗時の
/// 停止を要求する）。
#[test]
fn orders_are_rejected_when_the_policy_is_not_configured() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(181);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");

    // policy principal を設定しない（既定の fail-closed を検証する）。
    let caller = principal(182);
    let session = open_session(&pic, vault, caller, &secret(191));
    let denied: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            args(&session, b"no-policy", "ETH", "0.01", Some("2500"), 3),
        ),
    )
    .expect("call");
    assert_eq!(
        denied.expect_err("must be rejected"),
        ErrorCode::PolicyUnavailable,
        "政策未設定はfail-closedで拒否する"
    );
}

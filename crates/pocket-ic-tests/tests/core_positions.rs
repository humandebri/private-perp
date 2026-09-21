//! 建玉の取り込みとsnapshot反映の試験。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AgentGeneration, AllocationRequest, FundRequestAccepted};
use api_types::order::AccountSnapshot;
use api_types::{AccountKind, Blob};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, deploy, deploy_default, pic, principal, update,
    update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]"#;
const STATE: &str = r#"{"assetPositions":[{"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","liquidationPx":"2000","unrealizedPnl":"12.5","leverage":{"value":3},"marginMode":"cross"}}]}"#;
const STATE_UPDATED: &str = r#"{"assetPositions":[{"position":{"coin":"ETH","szi":"0.02","entryPx":"2500","unrealizedPnl":"-3.25","leverage":{"value":3},"marginMode":"cross"}}]}"#;

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

#[test]
fn positions_are_ingested_and_exposed_in_the_snapshot() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(210);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
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

    let caller = principal(211);
    let session = open_session(&pic, vault, caller, &secret(232));
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[221u8; 32])),
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
            client_request_id: blob(b"pos-alloc"),
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

    let ingested: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_positions",
        (session.clone(), STATE.to_string()),
    )
    .expect("call");
    assert_eq!(ingested.expect("ingested"), 1);

    let snapshot: Result<AccountSnapshot, ErrorCode> =
        update(&pic, core, caller, "get_account_snapshot", session.clone()).expect("call");
    let snapshot = snapshot.expect("snapshot");
    assert_eq!(snapshot.positions.len(), 1);
    let position = &snapshot.positions[0];
    assert_eq!(position.market, "ETH");
    assert_eq!(position.size, "0.05");
    assert_eq!(position.entry_price, "2500");
    assert_eq!(position.liquidation_price.as_deref(), Some("2000"));
    assert_eq!(position.leverage, 3);
    assert_eq!(position.unrealized_pnl, 12_500_000);
    assert_eq!(snapshot.data_age_ms, 0, "取り込んだ直後は新しい");

    // 同じ銘柄の再取り込みは更新（増えない）。
    let ingested: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_positions",
        (session.clone(), STATE_UPDATED.to_string()),
    )
    .expect("call");
    assert_eq!(ingested.expect("ingested"), 1);
    let snapshot: Result<AccountSnapshot, ErrorCode> =
        update(&pic, core, caller, "get_account_snapshot", session.clone()).expect("call");
    let snapshot = snapshot.expect("snapshot");
    assert_eq!(snapshot.positions.len(), 1, "銘柄ごとに1件");
    assert_eq!(snapshot.positions[0].size, "0.02");
    assert_eq!(snapshot.positions[0].unrealized_pnl, -3_250_000);
}

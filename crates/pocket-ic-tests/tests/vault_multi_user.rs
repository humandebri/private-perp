//! 複数利用者が同一 `funds_vault` で一連の資金操作を行えることの試験。
//!
//! 以前は `custody_accounts.derivation_path` が全利用者で同じ定数だったため、
//! 2人目の口座作成がUNIQUE違反で失敗し、入金案内・配分・出金が一切できなかった
//! （PocketICの試験はすべて1 Canister・1利用者だったため検出できていなかった）。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::fund::{
    AllocationRequest, Destination, FundRequestAccepted, FundRequestState, FundStatus,
    WithdrawalRequest,
};
use api_types::{AccountKind, AssetId, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, call_with_mocked_outcall, call_with_routed_outcalls,
    configure_policy, deploy, pic, principal, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const ACCEPTED: &[u8] = br#"{"status":"ok","response":{"type":"default"}}"#;

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
    secret_key: &[u8; 32],
) -> (SessionHandle, [u8; 20]) {
    let eoa = address_from_secret(secret_key).expect("address");
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
    let signature = challenge.sign_for_tests(secret_key).expect("sign");
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
    (session, eoa)
}

fn status(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> FundStatus {
    let value: Result<FundStatus, ErrorCode> =
        update(pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    value.expect("status")
}

/// 本人へ合成の入金を計上する（test-venue）。
fn credit(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    amount: u64,
    seed: u8,
) {
    let outcome: Result<(), ErrorCode> = update_args(
        pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), amount, blob(&[seed; 32])),
    )
    .expect("call");
    outcome.expect("credit");
}

/// 入金先（準備口座）を用意してアドレスを返す。
fn provision(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> [u8; 20] {
    let provisioned: Result<Blob, ErrorCode> = update(
        pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    provisioned
        .expect("provisioned")
        .as_ref()
        .try_into()
        .expect("20-byte address")
}

fn trading_address(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
) -> [u8; 20] {
    let trading: Result<Blob, ErrorCode> =
        update(pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    trading
        .expect("trading address")
        .as_ref()
        .try_into()
        .expect("20-byte address")
}

fn allocate(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    request: &[u8],
    amount: u64,
) -> Result<FundRequestAccepted, ErrorCode> {
    update(
        pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(request),
            amount,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call")
}

/// 1回のsweepで1件だけ送信されることを前提に、mockで受理させる。
fn sweep(pic: &PocketIc, vault: Principal, caller: Principal) {
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1, "1件だけ送信する");
}

fn arrive(
    pic: &PocketIc,
    vault: Principal,
    controller: Principal,
    tx_seed: u8,
    amount: u64,
    address: &[u8; 20],
) {
    let credited: Result<bool, ErrorCode> = update_args(
        pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[tx_seed; 32]),
            amount,
            blob(address),
            "usdc".to_string(),
        ),
    )
    .expect("call");
    assert!(credited.expect("arrival"), "着金を取り込む");
}

/// 出金（準備口座→本人EOA）を要求して送信する。
#[allow(clippy::too_many_arguments)]
fn withdraw(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    key: &[u8; 32],
    eoa: [u8; 20],
    request: &[u8],
    nonce: u64,
    amount: u64,
) -> Result<FundRequestAccepted, ErrorCode> {
    let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000;
    let intent = private_perp::Withdrawal {
        eoa,
        amount,
        asset: "usdc".to_string(),
        destination: format!("0x{}", hex::encode(eoa)),
        network: "local".to_string(),
        nonce,
        expires_at: now + 600_000,
        canister: vault.as_slice().to_vec(),
    };
    update(
        pic,
        vault,
        caller,
        "request_withdrawal",
        WithdrawalRequest {
            session: session.clone(),
            client_request_id: blob(request),
            amount,
            asset: AssetId::Usdc,
            destination: Destination::AuthenticatedEoaHlAccount,
            network: Network::Local,
            nonce,
            expires_at: intent.expires_at,
            intent_signature: intent
                .sign_for_tests(key)
                .expect("sign")
                .to_bytes65()
                .to_vec()
                .into(),
        },
    )
    .expect("call")
}

/// 2人の利用者がそれぞれ入金→配分→着金→回収→出金まで完了できる。
#[test]
fn two_users_run_the_full_funds_flow() {
    let pic = pic();
    let controller = principal(200);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let key_a = secret(251);
    let key_b = secret(252);
    let caller_a = principal(201);
    let caller_b = principal(202);
    let (session_a, eoa_a) = open_session(&pic, vault, caller_a, &key_a);
    let (session_b, eoa_b) = open_session(&pic, vault, caller_b, &key_b);

    // 送金前に取引口座を準備でき、本人以外のセッションでは準備できない。
    let prepared_a: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller_a,
        "prepare_trading_account",
        session_a.clone(),
    )
    .expect("prepare account");
    let prepared_b: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller_b,
        "prepare_trading_account",
        session_b.clone(),
    )
    .expect("prepare account");
    assert_ne!(prepared_a.as_ref().unwrap(), prepared_b.as_ref().unwrap());
    let other: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller_b,
        "prepare_trading_account",
        session_a.clone(),
    )
    .expect("foreign account call");
    assert!(other.is_err());

    // 口座作成（以前は2人目がUNIQUE違反で失敗した）。
    let reserve_a = provision(&pic, vault, caller_a, &session_a);
    let reserve_b = provision(&pic, vault, caller_b, &session_b);
    assert_ne!(reserve_a, reserve_b, "利用者ごとに入金先が異なる");

    // 入金。
    credit(&pic, vault, caller_a, &session_a, 1_000_000, 61);
    credit(&pic, vault, caller_b, &session_b, 2_000_000, 62);
    assert_eq!(
        status(&pic, vault, caller_a, &session_a).reserve_unallocated,
        1_000_000
    );
    assert_eq!(
        status(&pic, vault, caller_b, &session_b).reserve_unallocated,
        2_000_000
    );

    // 配分（利用者A）。
    allocate(&pic, vault, caller_a, &session_a, b"multi-alloc-a", 400_000).expect("accepted");
    sweep(&pic, vault, caller_a);
    let trading_a = trading_address(&pic, vault, caller_a, &session_a);
    arrive(&pic, vault, controller, 71, 400_000, &trading_a);
    let after_a = status(&pic, vault, caller_a, &session_a);
    assert_eq!(after_a.trading_equity, 400_000);
    assert_eq!(after_a.reserve_unallocated, 600_000);

    // 配分（利用者B）。取引口座は利用者ごとに別である。
    allocate(&pic, vault, caller_b, &session_b, b"multi-alloc-b", 400_000).expect("accepted");
    sweep(&pic, vault, caller_b);
    let trading_b = trading_address(&pic, vault, caller_b, &session_b);
    assert_ne!(trading_a, trading_b, "利用者ごとに取引口座が異なる");
    arrive(&pic, vault, controller, 72, 400_000, &trading_b);
    let after_b_alloc = status(&pic, vault, caller_b, &session_b);
    assert_eq!(after_b_alloc.trading_equity, 400_000);
    assert_eq!(after_b_alloc.reserve_unallocated, 1_600_000);

    // 回収（利用者A）→ 出金（利用者A）。
    let recovered: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller_a,
        "request_recovery",
        (session_a.clone(), blob(b"multi-recover-a"), 400_000u64),
    )
    .expect("call");
    recovered.expect("recovery accepted");
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let configured: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    configured.expect("vault principal");
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    let (swept, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller_a, "test_sweep_now", (), |call| {
            let request: serde_json::Value = serde_json::from_slice(&call.body).expect("request");
            if call.url.ends_with("/info") {
                match request["type"].as_str() {
                    Some("openOrders") => Ok((200, b"[]".to_vec())),
                    Some("clearinghouseState") => Ok((200, br#"{"assetPositions":[]}"#.to_vec())),
                    other => panic!("unexpected info request: {other:?}"),
                }
            } else {
                Ok((200, ACCEPTED.to_vec()))
            }
        })
        .expect("recovery call");
    assert_eq!(swept.expect("recovery sweep"), 1);
    let send = calls
        .iter()
        .find(|call| call.url.ends_with("/exchange"))
        .expect("send");
    let payload: serde_json::Value = serde_json::from_slice(&send.body).expect("send body");
    assert_eq!(payload["action"]["type"], "usdSend");
    assert_eq!(
        status(&pic, vault, caller_a, &session_a).reserve_unallocated,
        1_000_000
    );
    assert_eq!(
        status(&pic, vault, caller_b, &session_b).reserve_unallocated,
        1_600_000
    );

    let withdrawn = withdraw(
        &pic,
        vault,
        caller_a,
        &session_a,
        &key_a,
        eoa_a,
        b"multi-withdraw-a",
        1,
        250_000,
    );
    assert_eq!(
        withdrawn.expect("withdrawal accepted").state,
        FundRequestState::Reserved
    );
    sweep(&pic, vault, caller_a);
    let after_withdraw = status(&pic, vault, caller_a, &session_a);
    assert_eq!(after_withdraw.reserve_unallocated, 750_000);
    assert_eq!(
        after_withdraw.reserved_for_withdrawal, 0,
        "決済で拘束が解ける"
    );

    // 利用者Bも同じ流れを完了できる（出金のみ）。
    let withdrawn_b = withdraw(
        &pic,
        vault,
        caller_b,
        &session_b,
        &key_b,
        eoa_b,
        b"multi-withdraw-b",
        1,
        1_000_000,
    );
    assert_eq!(
        withdrawn_b.expect("withdrawal accepted").state,
        FundRequestState::Reserved
    );
    sweep(&pic, vault, caller_b);
    let after_b = status(&pic, vault, caller_b, &session_b);
    assert_eq!(after_b.reserve_unallocated, 600_000);
    assert_eq!(after_b.reserved_for_withdrawal, 0);

    // 出金intentのnonceは利用者ごとに独立している（Bは同じnonce=1を使えた）。
    let again_b = withdraw(
        &pic,
        vault,
        caller_b,
        &session_b,
        &key_b,
        eoa_b,
        b"multi-withdraw-b2",
        1,
        1_000,
    );
    let reused = again_b.expect_err("must reject a reused nonce");
    assert!(
        matches!(
            reused,
            ErrorCode::BadRequest {
                code: BadRequestCode::NonceReused,
                ..
            }
        ),
        "{reused:?}"
    );
}

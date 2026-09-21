//! 配分の着金から回収までの往復の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AllocationRequest, FundRequestAccepted, FundRequestState, FundStatus};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, deploy, pic, principal, update, update_args,
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
fn allocated_funds_arrive_and_can_be_recovered() {
    let pic = pic();
    let controller = principal(200);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(201);
    let session = open_session(&pic, vault, caller, &secret(250));

    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    provisioned.expect("provisioned");

    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[201u8; 32])),
    )
    .expect("call");
    credit.expect("credit");

    // 配分を要求すると取引口座が用意される。
    let allocated: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"recv-alloc"),
            amount: 400_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

    // 配分の送信（準備口座→移動中）を先に進める。
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(swept.expect("allocation sweep"), 1);

    // 取引口座への着金を取り込む（搬送路はmock、取り込みは検証済みの経路）。
    let trading: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading_address = blob(&trading.expect("trading address"));
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[1u8; 32]),
            400_000u64,
            trading_address.clone(),
            "usdc".to_string(),
        ),
    )
    .expect("call");
    assert!(credited.expect("arrival"), "着金を取り込む");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 400_000, "着金で取引口座残高が増える");
    assert_eq!(status.in_transit, 0, "移動中から取引へ移る");

    // 回収（取引口座→準備口座）を要求して送信する。
    let recovered: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-recovery"), 400_000u64),
    )
    .expect("call");
    recovered.expect("recovery accepted");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 0, "回収で取引口座が空になる");
    assert_eq!(status.reserve_unallocated, 1_000_000, "準備口座へ戻る");

    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    assert_eq!(
        events.expect("events").items[0].state,
        FundRequestState::Settled
    );
}

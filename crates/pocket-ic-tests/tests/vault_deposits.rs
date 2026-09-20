//! 取引所入金の取り込み（宛先写像・本人計上・二重計上防止）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{FundStatus, FundingInstructions};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, pic, principal, update, update_args};

const ORIGIN: &str = "https://app.example.test";

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

fn credit(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    tx: &[u8],
    amount: u64,
    address: &Blob,
) -> Result<bool, ErrorCode> {
    update_args(
        pic,
        vault,
        caller,
        "credit_venue_deposit",
        (blob(tx), amount, address.clone(), "usdc".to_string()),
    )
    .expect("call")
}

#[test]
fn venue_deposits_credit_the_owner_once() {
    let pic = pic();
    let controller = principal(170);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let caller = principal(171);
    let session = open_session(&pic, vault, caller, &secret(220));

    // 入金先（準備口座）を用意すると入金案内が得られる。
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    let provisioned = provisioned.expect("provisioned");
    let instructions: Result<FundingInstructions, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "get_funding_instructions",
        session.clone(),
    )
    .expect("call");
    let address = instructions.expect("instructions").hl_account_address;
    assert_eq!(address.as_ref(), provisioned.as_slice());

    // 本人の入金先宛なら計上される。
    let first = credit(&pic, vault, controller, &[7u8; 32], 1_000_000, &address);
    assert!(first.expect("credited"), "新規の入金を計上する");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(
        status.expect("status").reserve_unallocated,
        1_000_000,
        "本人の未配分残高へ計上される"
    );

    // 同じtx_hashは二重計上しない。
    let again = credit(&pic, vault, controller, &[7u8; 32], 1_000_000, &address);
    assert!(!again.expect("deduped"), "同じtx_hashは計上しない");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 1_000_000);

    // 未知の宛先は記録のみ。
    let unknown = credit(
        &pic,
        vault,
        controller,
        &[8u8; 32],
        500_000,
        &blob(&[9u8; 20]),
    );
    assert!(unknown.expect("recorded"));
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 1_000_000);

    // 非controllerは取り込めない。
    let denied = credit(&pic, vault, principal(172), &[10u8; 32], 1, &address);
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

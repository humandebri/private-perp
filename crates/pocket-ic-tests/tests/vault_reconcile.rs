//! 入金の取得（replicated `/info`＋変換）と取り込みの試験。

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
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, deploy, pic, principal, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const INFO_BODY: &[u8] =
    br#"[{"time":1758000000000,"hash":"0xaa","delta":{"type":"deposit","usdc":"100.5"},"extra":1}]"#;

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
fn fetched_deposits_are_credited_once() {
    let pic = pic();
    let controller = principal(190);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let caller = principal(191);
    let session = open_session(&pic, vault, caller, &secret(240));
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    let address = provisioned.expect("provisioned");
    let instructions: Result<FundingInstructions, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "get_funding_instructions",
        session.clone(),
    )
    .expect("call");
    assert_eq!(
        instructions
            .expect("instructions")
            .hl_account_address
            .as_ref(),
        address.as_slice()
    );

    // 取得（replicated outcall＋変換）→ 本人へ計上。
    let first: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (blob(&address),),
        Ok((200, INFO_BODY.to_vec())),
    )
    .expect("call");
    assert_eq!(first.expect("reconciled"), 1);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(
        status.expect("status").reserve_unallocated,
        100_500_000,
        "取得した入金を本人へ計上する"
    );

    // 同じ入金（同じhash）は二重計上しない。
    let second: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (blob(&address),),
        Ok((200, INFO_BODY.to_vec())),
    )
    .expect("call");
    assert_eq!(second.expect("reconciled"), 0);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 100_500_000);

    // 非controllerは取り込めない。
    let denied: Result<u32, ErrorCode> = update_args(
        &pic,
        vault,
        principal(192),
        "reconcile_deposits",
        (blob(&address),),
    )
    .expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

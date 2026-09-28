//! upgradeを通した資金層の保存と、未解決送金の再送禁止（T-407・T-705のローカル部分）。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AllocationRequest, FundRequestAccepted, FundStatus};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, call_with_routed_outcalls, deploy_default, pic,
    principal, update, update_args, upgrade,
};

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
    let session = session.expect("session");
    pocket_ic_tests::activate_local_user(pic, vault, caller, &session);
    session
}

#[test]
fn an_upgrade_keeps_credentials_ledger_and_unresolved_actions() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(70);
    let key = secret(141);
    let session = open_session(&pic, vault, caller, &key);

    // 入金して配分を要求し、送信は不明（応答喪失）にする。
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[21u8; 32])),
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
            client_request_id: blob(b"up-1"),
            amount: 200_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Err((3, "outcall failed".to_string())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let before: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let before = before.expect("status");
    assert_eq!(before.unknowns.len(), 1);
    assert_eq!(before.withdrawable, 800_000);

    // アップグレードしても認証・台帳・未解決actionが保存される。
    upgrade(
        &pic,
        vault,
        FUNDS_VAULT_WASM,
        candid::encode_one(()).unwrap(),
    );

    let after: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let after = after.expect("status after upgrade");
    assert_eq!(after.unknowns.len(), 1, "未解決actionを失わない");
    assert_eq!(after.withdrawable, 800_000, "残高と予約が保存される");

    // アップグレード後のsweepでも再送しない。
    let (swept_again, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |call| {
            assert!(
                call.url.ends_with("/info"),
                "an unknown send must not be resent"
            );
            let query: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            assert_eq!(query["type"], "userNonFundingLedgerUpdates");
            Ok((200, b"[]".to_vec()))
        })
        .expect("call");
    assert!(
        !calls.is_empty(),
        "the unknown send is checked against history"
    );
    assert_eq!(
        swept_again.expect("sweep"),
        0,
        "dispatching/unknownは再送しない"
    );
}

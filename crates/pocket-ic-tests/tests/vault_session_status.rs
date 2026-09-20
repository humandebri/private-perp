//! canister間のセッション検証（`trading_core` が使う経路）の試験。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
    SessionStatus,
};
use api_types::error::ErrorCode;
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy_default, pic, principal, query, update};
use std::time::Duration;

const ORIGIN: &str = "https://app.example.test";

fn secret(seed: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes
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

fn fetch_status(
    pic: &PocketIc,
    vault: Principal,
    requester: Principal,
    session: &SessionHandle,
) -> Result<SessionStatus, ErrorCode> {
    query(pic, vault, requester, "session_status", session.clone()).expect("call")
}

#[test]
fn session_status_is_verifiable_by_another_canister() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(80);
    let key = secret(151);
    let session = open_session(&pic, vault, caller, &key);

    // 別principal（canister役）から問い合わせても、束縛されたprincipalが返る。
    let requester = principal(81);
    let status = fetch_status(&pic, vault, requester, &session).expect("valid session");
    assert_eq!(
        status.principal, caller,
        "呼び出し側が比較するためのprincipalを返す"
    );
    assert_eq!(status.user_id.len(), 32);

    // 失効後はSessionRevoked。
    let revoked: Result<(), ErrorCode> =
        update(&pic, vault, caller, "revoke_session", session.clone()).expect("call");
    revoked.expect("revoke");
    let after = fetch_status(&pic, vault, requester, &session).expect_err("revoked");
    assert_eq!(after, ErrorCode::SessionRevoked);

    // 期限切れはSessionExpired（30分）。
    let session2 = open_session(&pic, vault, principal(82), &secret(152));
    pic.advance_time(Duration::from_secs(31 * 60));
    pic.tick();
    let expired = fetch_status(&pic, vault, principal(83), &session2).expect_err("expired");
    assert_eq!(expired, ErrorCode::SessionExpired);
}

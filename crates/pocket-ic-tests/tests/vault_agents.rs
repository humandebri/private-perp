//! Agent承認（master署名）の試験。
//!
//! 鍵の導出・保管は `trading_core` が担い、vaultは**渡されたアドレス**をmaster署名で
//! 承認するだけである（`Implementation.md` 7章）。取引所が受理した場合のみ`Active`を返す。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AgentGeneration, AgentState};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, deploy_default, pic, principal, update,
};

const ORIGIN: &str = "https://app.example.test";
const ACCEPTED: &[u8] = br#"{"status":"ok","response":{"type":"default"}}"#;
const REJECTED: &[u8] = br#"{"status":"err","response":"agent already exists"}"#;

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

/// vaultへ「このアドレスを承認せよ」と依頼する（outcallはmockで応答させる）。
fn approve_agent(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    reply: Result<(u16, Vec<u8>), (u64, String)>,
) -> Result<AgentGeneration, ErrorCode> {
    call_with_mocked_outcall(
        pic,
        vault,
        caller,
        "approve_agent_generation",
        (session.clone(), api_types::Blob::from(vec![7u8; 20])),
        reply,
    )
    .expect("call")
}

#[test]
fn approving_a_generation_activates_it_only_when_the_venue_accepts() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(92);
    let session = open_session(&pic, vault, caller, &secret(162));
    let address = api_types::Blob::from(vec![7u8; 20]);

    // 取引所が拒否した場合はactiveにしない。
    let rejected = approve_agent(&pic, vault, caller, &session, Ok((200, REJECTED.to_vec())));
    assert!(
        matches!(rejected, Err(ErrorCode::UpstreamRejected { .. })),
        "{rejected:?}"
    );

    // 受理された場合はactiveとして返る（秘密鍵はcoreが保管する）。
    let approved = approve_agent(&pic, vault, caller, &session, Ok((200, ACCEPTED.to_vec())));
    let approved = approved.expect("approved");
    assert_eq!(approved.state, AgentState::Active);
    assert_eq!(approved.generation, 1);
    assert_eq!(approved.agent_address, address);
    assert!(approved.approved_at.is_some());
}

//! Agent世代の要求と状態表示（`Implementation.md` 7章）の試験。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AgentGeneration, AgentState, AgentStatus};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, deploy_default, pic, principal, query, update,
    update_args,
};

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

#[test]
fn agent_generations_are_requested_with_a_derived_address() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(90);
    let key = secret(161);
    let session = open_session(&pic, vault, caller, &key);

    let requested: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let requested = requested.expect("generation");
    assert_eq!(requested.generation, 1);
    assert_eq!(requested.state, AgentState::Requested);
    assert_eq!(
        requested.agent_address.len(),
        20,
        "導出した公開鍵のアドレス"
    );

    // 未承認の世代があるうちは同じ世代を返す（世代を無駄に増やさない）。
    let again: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let again = again.expect("generation");
    assert_eq!(again.generation, 1);
    assert_eq!(again.agent_address, requested.agent_address);

    // 状態表示: 承認前なのでcurrentは無く、nextに要求中の世代が入る。
    let status: Result<AgentStatus, ErrorCode> =
        query(&pic, vault, caller, "get_agent_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert!(status.current.is_none(), "承認はまだ実装していない");
    assert_eq!(status.next.expect("next").generation, 1);

    // 他principalのセッションでは要求できない。
    let denied: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        vault,
        principal(91),
        "request_agent_generation",
        session,
    )
    .expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

const ACCEPTED: &[u8] = br#"{"status":"ok","response":{"type":"default"}}"#;
const REJECTED: &[u8] = br#"{"status":"err","response":"agent already exists"}"#;

#[test]
fn approving_a_generation_activates_it_only_when_the_venue_accepts() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(92);
    let session = open_session(&pic, vault, caller, &secret(162));

    let requested: Result<AgentGeneration, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    assert_eq!(requested.expect("generation").generation, 1);

    // 取引所が拒否した場合はrequestedのまま残る。
    let rejected: Result<AgentGeneration, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "approve_agent_generation",
        (session.clone(),),
        Ok((200, REJECTED.to_vec())),
    )
    .expect("call");
    assert!(
        matches!(rejected, Err(ErrorCode::UpstreamRejected { .. })),
        "{rejected:?}"
    );
    let still: Result<AgentStatus, ErrorCode> =
        query(&pic, vault, caller, "get_agent_status", session.clone()).expect("call");
    let still = still.expect("status");
    assert!(still.current.is_none());
    assert_eq!(still.next.expect("next").generation, 1);

    // 受理された場合はactiveへ遷移し、currentになる。
    let approved: Result<AgentGeneration, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "approve_agent_generation",
        (session.clone(),),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    let approved = approved.expect("approved");
    assert_eq!(approved.state, AgentState::Active);
    assert!(approved.approved_at.is_some());

    let status: Result<AgentStatus, ErrorCode> =
        query(&pic, vault, caller, "get_agent_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(status.current.expect("current").generation, 1);
    assert!(status.next.is_none());

    // 二重承認は要求中の世代が無いため拒否する。
    let again: Result<AgentGeneration, ErrorCode> =
        update_args(&pic, vault, caller, "approve_agent_generation", (session,)).expect("call");
    assert!(
        again.is_err(),
        "要求中の世代が無ければ承認しない: {again:?}"
    );
}

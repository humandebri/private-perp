//! outboxの署名送信・照合（モックベニュー）の試験。
//!
//! `docs/phase-0/state-machines.md` 2〜3節と、ロードマップ5章の必須検証
//! 「送金成功後の応答喪失でも自動再送で二重払出ししない」を対象にする。

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
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, deploy_default, pic, principal, update, update_args,
};
use std::time::Duration;

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
) -> SessionHandle {
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
    session.expect("session")
}

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

fn allocate(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    request_id: &[u8],
    amount: u64,
) -> Result<FundRequestAccepted, ErrorCode> {
    update(
        pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(request_id),
            amount,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call")
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

#[test]
fn a_successful_allocation_moves_funds_to_in_transit() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(40);
    let key = secret(131);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 11);

    allocate(&pic, vault, caller, &session, b"out-1", 400_000).expect("accepted");
    assert_eq!(status(&pic, vault, caller, &session).in_transit, 0);

    // sweepがusdSendを署名してPOSTし、mockが受理を返す。
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

    let after = status(&pic, vault, caller, &session);
    assert_eq!(after.in_transit, 400_000, "移動中へ移る");
    assert_eq!(
        after.reserve_unallocated, 600_000,
        "未配分は配分した分だけ減る（複式仕訳）"
    );
    assert_eq!(
        after.withdrawable, 600_000,
        "予約は消費され、二重に拘束されない"
    );
}

#[test]
fn an_uncertain_send_is_not_resent() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(41);
    let key = secret(132);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 12);

    allocate(&pic, vault, caller, &session, b"out-2", 300_000).expect("accepted");

    // 応答を返さない（outcallのreject相当）→ 結果不明として保持する。
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

    let after = status(&pic, vault, caller, &session);
    assert_eq!(after.in_transit, 0, "不明な送金を確定残高へ含めない");
    assert_eq!(after.withdrawable, 700_000, "予約は保持したまま");
    assert_eq!(after.unknowns.len(), 1, "未解決actionとして残る");

    // 再sweepしても送信しない（未解決actionはclaimされない）。
    let swept_again: Result<u32, ErrorCode> =
        update_args(&pic, vault, caller, "test_sweep_now", ()).expect("call");
    assert_eq!(swept_again.expect("sweep"), 0, "自動再送しない");

    // 時間が経過しても解放・確定しない（T-206: 不明を勝手に解消しない）。
    pic.advance_time(Duration::from_secs(10 * 60));
    pic.tick();
    let after_wait = status(&pic, vault, caller, &session);
    assert_eq!(after_wait.unknowns.len(), 1, "不明なactionを保持し続ける");
    assert_eq!(after_wait.withdrawable, 700_000, "予約は保持されたまま");
    assert_eq!(after_wait.in_transit, 0, "確定残高へ含めない");
    let swept_after_wait: Result<u32, ErrorCode> =
        update_args(&pic, vault, caller, "test_sweep_now", ()).expect("call");
    assert_eq!(
        swept_after_wait.expect("sweep"),
        0,
        "時間経過でも自動再送しない"
    );
}

#[test]
fn a_venue_rejection_releases_the_reservation() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(42);
    let key = secret(133);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 13);

    allocate(&pic, vault, caller, &session, b"out-3", 250_000).expect("accepted");

    let rejected = br#"{"status":"err","response":"insufficient margin"}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, rejected)),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let after = status(&pic, vault, caller, &session);
    assert_eq!(
        after.reserve_unallocated, 1_000_000,
        "拒否なら資金は動かない"
    );
    assert_eq!(after.withdrawable, 1_000_000, "予約は解放される");
    assert!(after.unknowns.is_empty());

    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let events = events.expect("events");
    assert_eq!(events.items[0].state, FundRequestState::Rejected);
}

#[test]
fn concurrent_sweeps_dispatch_the_action_once() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(43);
    let key = secret(134);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 14);
    allocate(&pic, vault, caller, &session, b"out-4", 400_000).expect("accepted");

    // 同じactionを2つのsweepが同時に処理しようとする。
    let payload = candid::encode_args(()).expect("encode");
    let first = pic
        .submit_call(vault, caller, "test_sweep_now", payload.clone())
        .expect("submit 1");
    let second = pic
        .submit_call(vault, caller, "test_sweep_now", payload)
        .expect("submit 2");

    let mut mocked = 0;
    for _ in 0..60 {
        pic.tick();
        for request in pic.get_canister_http() {
            pic.mock_canister_http_response(MockCanisterHttpResponse {
                subnet_id: request.subnet_id,
                request_id: request.request_id,
                response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status: 200,
                    headers: Vec::new(),
                    body: ACCEPTED.to_vec(),
                }),
                additional_responses: Vec::new(),
            });
            mocked += 1;
        }
        if pic.ingress_status(first.clone()).is_some()
            && pic.ingress_status(second.clone()).is_some()
        {
            break;
        }
    }

    let results: Vec<Result<u32, ErrorCode>> = [first, second]
        .into_iter()
        .map(|id| {
            let bytes = pic.await_call(id).expect("await");
            candid::decode_one::<Result<u32, ErrorCode>>(&bytes).expect("decode")
        })
        .collect();

    let processed: u32 = results
        .iter()
        .map(|result| result.as_ref().copied().unwrap_or(0))
        .sum();
    assert_eq!(processed, 1, "actionは一度だけ処理される: {results:?}");
    assert_eq!(mocked, 1, "送信は一度だけ行われる");

    let after = status(&pic, vault, caller, &session);
    assert_eq!(after.in_transit, 400_000, "二重計上しない");
    assert_eq!(after.reserve_unallocated, 600_000);
    assert!(after.unknowns.is_empty());
}

//! outboxの署名送信・照合（モックベニュー）の試験。
//!
//! `docs/phase-0/state-machines.md` 2〜3節と、ロードマップ5章の必須検証
//! 「送金成功後の応答喪失でも自動再送で二重払出ししない」を対象にする。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{
    AllocationRequest, Destination, FundRequestAccepted, FundRequestState, FundStatus,
    WithdrawalRequest,
};
use api_types::journal::{JournalHead, JournalRecord, RecoveryPayload, RecoveryRecord};
use api_types::{AccountKind, AssetId, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, call_with_mocked_outcall_captured, deploy,
    deploy_default, pic, principal, query, update, update_args,
};
use std::time::Duration;

const ORIGIN: &str = "https://app.example.test";
const ACCEPTED: &[u8] = br#"{"status":"ok","response":{"type":"default"}}"#;

#[test]
fn budget_denial_before_post_retries_without_losing_the_action() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(240);
    let policy: Option<Principal> =
        query(&pic, vault, controller, "get_policy_principal", ()).expect("policy query");
    let policy = policy.expect("policy configured");
    let role: Result<(), ErrorCode> =
        update(&pic, policy, controller, "set_operator", controller).expect("call");
    role.expect("set_operator");
    let pause: Result<(), ErrorCode> =
        update(&pic, policy, controller, "pause_for_recovery", ()).expect("call");
    pause.expect("pause_for_recovery");

    let caller = principal(48);
    let session = open_session(&pic, vault, caller, &secret(148));
    credit(&pic, vault, caller, &session, 1_000_000, 48);
    allocate(&pic, vault, caller, &session, b"budget-denied", 400_000).expect("accepted");
    let denied: Result<u32, ErrorCode> =
        update(&pic, vault, caller, "test_sweep_now", ()).expect("call");
    assert!(denied.is_err(), "policy denial must prevent an HTTP POST");

    let clear: Result<(), ErrorCode> =
        update(&pic, policy, controller, "clear_recovery_pause", ()).expect("call");
    clear.expect("clear_recovery_pause");
    pic.advance_time(Duration::from_secs(31));
    pic.tick();
    let still_stopped: Result<u32, ErrorCode> =
        update(&pic, vault, caller, "test_sweep_now", ()).unwrap();
    assert_eq!(
        still_stopped.unwrap(),
        0,
        "budget recovery does not auto-retry a failed send"
    );
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, vault, caller, &session, false, "fund"),
        1
    );
    let sent: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(sent.expect("sweep"), 1);
    let again: Result<u32, ErrorCode> =
        update(&pic, vault, caller, "test_sweep_now", ()).expect("call");
    assert_eq!(again.expect("second sweep"), 0);
}

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
    let session = session.expect("session");
    pocket_ic_tests::activate_local_user(pic, vault, caller, &session);
    session
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
fn allocation_acceptance_replays_from_snapshot_before_request() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(242);
    let session = open_session(&pic, vault, caller, &secret(152));
    credit(&pic, vault, caller, &session, 1_000_000, 52);
    let reserve: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("reserve account");
    reserve.expect("reserve provisioned");
    let trading: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone())
            .expect("trading account");
    trading.expect("trading provisioned");
    let controller = pic.get_controllers(vault)[0];
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
    let guard = guard.expect("guard result").expect("configured guard");
    let before_request = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot before allocation acceptance");

    allocate(
        &pic,
        vault,
        caller,
        &session,
        b"replay-accepted-allocation",
        400_000,
    )
    .expect("allocation accepted");
    let accepted = status(&pic, vault, caller, &session);
    assert_eq!(accepted.reserve_unallocated, 1_000_000);
    assert_eq!(accepted.withdrawable, 600_000);

    pic.load_canister_snapshot(vault, Some(controller), before_request.id)
        .expect("restore before allocation acceptance");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
    assert!(resumed.is_err(), "external validation is still required");
    let restored = status(&pic, vault, caller, &session);
    assert_eq!(restored.reserve_unallocated, 1_000_000);
    assert_eq!(restored.withdrawable, 600_000);
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("restored events");
    assert_eq!(
        events.expect("events").items[0].state,
        FundRequestState::Reserved
    );
    let pending_validation: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).expect("pending query");
    assert!(pending_validation.expect("pending validation"));
    let send_status: Result<(bool, bool), ErrorCode> =
        query(&pic, vault, caller, "get_journal_send_status", ()).expect("send status query");
    assert_eq!(send_status.expect("send status"), (true, true));
    let (_, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("locked sweep call");
    assert!(
        captured.is_none(),
        "replayed acceptance cannot start a POST"
    );
}

#[test]
fn allocation_reservation_replays_when_send_intent_is_ahead_of_backup() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(240);
    let session = open_session(&pic, vault, caller, &secret(150));
    credit(&pic, vault, caller, &session, 1_000_000, 50);
    let reserve: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("reserve account");
    reserve.expect("reserve provisioned");
    let trading: Result<Vec<u8>, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone())
            .expect("trading account");
    trading.expect("trading provisioned");
    let controller = pic.get_controllers(vault)[0];
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
    let guard = guard.expect("guard result").expect("configured guard");
    let before_request = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot before allocation acceptance");

    allocate(
        &pic,
        vault,
        caller,
        &session,
        b"replay-post-allocation",
        400_000,
    )
    .expect("allocation accepted");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("allocation sweep");
    assert_eq!(swept.expect("allocation sent"), 1);
    assert_eq!(status(&pic, vault, caller, &session).in_transit, 400_000);

    pic.load_canister_snapshot(vault, Some(controller), before_request.id)
        .expect("restore before allocation and POST");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
    assert!(
        resumed.is_err(),
        "the staged POST must not authorize sending"
    );
    let restored = status(&pic, vault, caller, &session);
    assert_eq!(restored.reserve_unallocated, 1_000_000);
    assert_eq!(restored.withdrawable, 600_000, "hold must be replayed");
    assert_eq!(restored.in_transit, 0, "POST result is not yet proven");
    let pending: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).expect("pending query");
    assert!(pending.expect("pending validation"));
    let (_, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("locked sweep call");
    assert!(captured.is_none(), "restored action must not be resent");
}

#[test]
fn allocation_result_replays_from_snapshot_taken_before_http_reply() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(241);
    let session = open_session(&pic, vault, caller, &secret(151));
    credit(&pic, vault, caller, &session, 1_000_000, 51);
    allocate(&pic, vault, caller, &session, b"replay-allocation", 400_000).expect("accepted");
    let controller = pic.get_controllers(vault)[0];
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).unwrap();
    let guard = guard.unwrap().expect("configured recovery guard");

    let message_id = pic
        .submit_call(
            vault,
            caller,
            "test_sweep_now",
            candid::encode_one(()).unwrap(),
        )
        .expect("submit sweep");
    let mut pending = None;
    for _ in 0..50 {
        pic.tick();
        if let Some(request) = pic.get_canister_http().into_iter().next() {
            pending = Some(request);
            break;
        }
    }
    let pending = pending.expect("usdSend outcall");
    assert!(pending.url.contains("/exchange"));
    let before_reply = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot after durable dispatch, before venue reply");
    pic.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: pending.subnet_id,
        request_id: pending.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: ACCEPTED.to_vec(),
        }),
        additional_responses: Vec::new(),
    });
    let reply = pic.await_call(message_id).expect("sweep reply");
    let swept: Result<u32, ErrorCode> = candid::decode_one(&reply).expect("decode sweep");
    assert_eq!(swept.expect("dispatched"), 1);
    assert_eq!(status(&pic, vault, caller, &session).in_transit, 400_000);

    pic.load_canister_snapshot(vault, Some(controller), before_reply.id)
        .expect("restore before result");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).unwrap();
    assert!(resumed.is_err(), "external validation is still required");
    let restored = status(&pic, vault, caller, &session);
    assert_eq!(restored.in_transit, 400_000);
    assert_eq!(restored.reserve_unallocated, 600_000);
    let pending_validation: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).unwrap();
    assert!(pending_validation.unwrap());
}

#[test]
fn journal_outage_after_allocation_post_keeps_reservation_for_reconciliation() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = pic.get_controllers(vault)[0];
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal query");
    let journal = journal.expect("configured journal").expect("principal");
    let caller = principal(244);
    let session = open_session(&pic, vault, caller, &secret(154));
    credit(&pic, vault, caller, &session, 1_000_000, 54);
    allocate(
        &pic,
        vault,
        caller,
        &session,
        b"result-journal-outage",
        400_000,
    )
    .expect("accepted");

    let message_id = pic
        .submit_call(
            vault,
            caller,
            "test_sweep_now",
            candid::encode_one(()).unwrap(),
        )
        .expect("submit sweep");
    let pending = (0..50)
        .find_map(|_| {
            pic.tick();
            pic.get_canister_http().into_iter().next()
        })
        .expect("exchange POST");
    assert!(pending.url.ends_with("/exchange"));
    pic.stop_canister(journal, Some(controller))
        .expect("stop journal after POST");
    pic.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: pending.subnet_id,
        request_id: pending.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: ACCEPTED.to_vec(),
        }),
        additional_responses: Vec::new(),
    });
    let reply = pic.await_call(message_id).expect("sweep reply");
    let swept: Result<u32, ErrorCode> = candid::decode_one(&reply).expect("decode sweep");
    assert!(
        swept.is_err(),
        "journal outage is surfaced; POST evidence remains durable"
    );
    let funds = status(&pic, vault, caller, &session);
    assert_eq!(funds.in_transit, 0);
    assert_eq!(funds.withdrawable, 600_000);
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("events call");
    assert_eq!(
        events.expect("events").items[0].state,
        FundRequestState::Reserved
    );
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, vault, caller, &session, false, "fund"),
        1
    );
    assert_eq!(
        status(&pic, vault, caller, &session).unknowns.len(),
        1,
        "lost callback moves forward to unknown, never queued"
    );
    pic.start_canister(journal, Some(controller)).unwrap();
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, vault, caller, &session, false, "result"),
        1
    );
    let retried: Result<u32, ErrorCode> =
        update(&pic, vault, caller, "test_sweep_now", ()).unwrap();
    assert_eq!(
        retried.unwrap(),
        0,
        "retry persists result without another POST"
    );
    let funds = status(&pic, vault, caller, &session);
    assert_eq!(funds.in_transit, 400_000);
    assert_eq!(funds.withdrawable, 600_000);
}

#[test]
fn committed_v2_receipt_blocks_post_if_independent_journal_rolls_back() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = pic.get_controllers(vault)[0];
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let empty_journal = pic
        .take_canister_snapshot(journal, Some(controller), None)
        .expect("snapshot empty independent journal");
    let caller = principal(39);
    let session = open_session(&pic, vault, caller, &secret(130));
    credit(&pic, vault, caller, &session, 1_000_000, 10);
    allocate(&pic, vault, caller, &session, b"before-rollback", 100_000).expect("accepted");
    let sent: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(sent.expect("sweep"), 1);
    let local: Result<(u64, bool), ErrorCode> =
        query(&pic, vault, controller, "recovery_stage_status", ())
            .expect("local recovery receipt");
    let (sequence, locked) = local.expect("receipt");
    assert!(
        sequence >= 2,
        "account creation and transfer result are recorded"
    );
    assert!(!locked);
    let prior: Result<Vec<JournalRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "records", (0u64, 10u32)).expect("prior V1 intent");
    let prior = prior.expect("V1 records");
    assert_eq!(prior.len(), 1);
    pic.load_canister_snapshot(journal, Some(controller), empty_journal.id)
        .expect("restore independent journal behind worker");
    let replayed: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "append", prior[0].intent.clone())
            .expect("restore identical V1 intent");
    assert_eq!(replayed.expect("V1 head").hash, prior[0].hash);
    let before: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    assert!(matches!(
        allocate(&pic, vault, caller, &session, b"after-rollback", 100_000),
        Err(ErrorCode::PolicyUnavailable)
    ));
    let after: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    assert_eq!(
        after.unwrap().withdrawable,
        before.unwrap().withdrawable,
        "rolled-back journal must block a new reservation"
    );
    let (swept, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(swept.unwrap(), 0, "no new action can reach the outbox");
    assert!(captured.is_none(), "rolled-back journal must block HL POST");
    let local: Result<(u64, bool), ErrorCode> =
        query(&pic, vault, controller, "recovery_stage_status", ()).unwrap();
    assert!(local.unwrap().1, "journal rollback must leave sends locked");
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
    let swept_again: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, b"[]".to_vec())),
    )
    .expect("history query");
    assert_eq!(swept_again.expect("sweep"), 0, "自動再送しない");

    // 時間が経過しても解放・確定しない（T-206: 不明を勝手に解消しない）。
    pic.advance_time(Duration::from_secs(10 * 60));
    pic.tick();
    let after_wait = status(&pic, vault, caller, &session);
    assert_eq!(after_wait.unknowns.len(), 1, "不明なactionを保持し続ける");
    assert_eq!(after_wait.withdrawable, 700_000, "予約は保持されたまま");
    assert_eq!(after_wait.in_transit, 0, "確定残高へ含めない");
    let swept_after_wait: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, b"[]".to_vec())),
    )
    .expect("history query");
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

#[test]
fn a_tampered_digest_is_never_signed() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(44);
    let key = secret(135);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 15);
    allocate(&pic, vault, caller, &session, b"out-5", 250_000).expect("accepted");

    // 受付時に記録したダイジェストを壊す。
    let corrupted: Result<u32, ErrorCode> =
        update_args(&pic, vault, caller, "test_corrupt_action_digest", ()).expect("call");
    assert_eq!(corrupted.expect("corrupt"), 1);

    // sweepは署名せずに恒久エラーで止まる（outcallは発生しない）。
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    let error = swept.expect_err("digest mismatch must stop the sweep");
    assert!(matches!(error, ErrorCode::Internal { .. }), "{error:?}");

    // 資金は動かず、予約は保持されたまま（送信していないので不明でもない）。
    let after = status(&pic, vault, caller, &session);
    assert_eq!(after.in_transit, 0, "送信しないので移動中にならない");
    assert_eq!(after.withdrawable, 750_000, "予約は保持されたまま");
    assert!(
        after.unknowns.is_empty(),
        "送信していないのでunknownではない"
    );
}

#[test]
fn withdrawal_acceptance_replays_from_snapshot_before_request() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(243);
    let key = secret(153);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 53);
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("reserve account");
    provisioned.expect("reserve provisioned");
    let controller = pic.get_controllers(vault)[0];
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
    let guard = guard.expect("guard result").expect("configured guard");
    let before_request = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot before withdrawal acceptance");

    let eoa = address_from_secret(&key).expect("address");
    let expires_at = 1_700_000_600_000u64;
    let intent = private_perp::Withdrawal {
        eoa,
        amount: 300_000,
        asset: "usdc".to_string(),
        destination: format!("0x{}", hex::encode(eoa)),
        network: "local".to_string(),
        nonce: 1,
        expires_at,
        canister: vault.as_slice().to_vec(),
    };
    let signature = intent.sign_for_tests(&key).expect("sign");
    let accepted: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        WithdrawalRequest {
            session: session.clone(),
            client_request_id: blob(b"replay-accepted-withdrawal"),
            amount: 300_000,
            asset: AssetId::Usdc,
            destination: Destination::AuthenticatedEoaHlAccount,
            network: Network::Local,
            nonce: 1,
            expires_at,
            intent_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("withdrawal call");
    accepted.expect("withdrawal accepted");
    assert_eq!(
        status(&pic, vault, caller, &session).reserved_for_withdrawal,
        300_000
    );

    pic.load_canister_snapshot(vault, Some(controller), before_request.id)
        .expect("restore before withdrawal acceptance");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
    assert!(resumed.is_err(), "external validation is still required");
    let restored = status(&pic, vault, caller, &session);
    assert_eq!(restored.reserve_unallocated, 700_000);
    assert_eq!(restored.reserved_for_withdrawal, 300_000);
    assert_eq!(restored.withdrawable, 700_000);
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("restored events");
    assert_eq!(
        events.expect("events").items[0].state,
        FundRequestState::Reserved
    );
    let (_, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("locked sweep call");
    assert!(
        captured.is_none(),
        "replayed withdrawal cannot start a POST"
    );
}

#[test]
fn a_withdrawal_is_dispatched_from_the_reserve() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(45);
    let key = secret(136);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 16);
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    provisioned.expect("provisioned");

    let eoa = address_from_secret(&key).expect("address");
    let expires_at = 1_700_000_600_000u64;
    let intent = private_perp::Withdrawal {
        eoa,
        amount: 300_000,
        asset: "usdc".to_string(),
        destination: format!("0x{}", hex::encode(eoa)),
        network: "local".to_string(),
        nonce: 1,
        expires_at,
        canister: vault.as_slice().to_vec(),
    };
    let signature = intent.sign_for_tests(&key).expect("sign");
    let accepted: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        WithdrawalRequest {
            session: session.clone(),
            client_request_id: blob(b"payout-1"),
            amount: 300_000,
            asset: AssetId::Usdc,
            destination: Destination::AuthenticatedEoaHlAccount,
            network: Network::Local,
            nonce: 1,
            expires_at,
            intent_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("call");
    accepted.expect("withdrawal accepted");

    let controller = pic.get_controllers(vault)[0];
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).unwrap();
    let guard = guard.unwrap().expect("configured recovery guard");
    let message_id = pic
        .submit_call(
            vault,
            caller,
            "test_sweep_now",
            candid::encode_one(()).unwrap(),
        )
        .expect("submit payout sweep");
    let mut pending = None;
    for _ in 0..50 {
        pic.tick();
        if let Some(request) = pic.get_canister_http().into_iter().next() {
            pending = Some(request);
            break;
        }
    }
    let pending = pending.expect("payout outcall");
    assert!(pending.url.contains("/exchange"));
    let before_reply = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot before payout response");
    pic.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: pending.subnet_id,
        request_id: pending.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: ACCEPTED.to_vec(),
        }),
        additional_responses: Vec::new(),
    });
    let reply = pic.await_call(message_id).expect("payout sweep reply");
    let swept: Result<u32, ErrorCode> = candid::decode_one(&reply).expect("decode payout sweep");
    assert_eq!(swept.expect("sweep"), 1);

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
    let settled = status(&pic, vault, caller, &session);
    assert_eq!(settled.reserved_for_withdrawal, 0);
    assert_eq!(
        settled.withdrawable, 700_000,
        "払出し済みの分は出金可能額から除かれ、予約は二重に拘束しない"
    );
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    let records = records.expect("private recovery events");
    for kind in ["reserve", "trading"] {
        assert!(records.iter().any(|record| matches!(
            &record.event.payload,
            RecoveryPayload::CustodyAccount {
                kind: recorded_kind,
                address,
                ..
            } if recorded_kind == kind && address.len() == 20
        )));
    }
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(
                &record.event.payload,
                RecoveryPayload::FundTransferResult { .. }
            ))
            .count(),
        1
    );
    assert!(matches!(
        &records.last().expect("transfer result").event.payload,
        RecoveryPayload::FundTransferResult {
            kind,
            amount_micros: 300_000,
            accepted: true,
            ..
        } if kind == "withdrawal"
    ));
    pic.load_canister_snapshot(vault, Some(controller), before_reply.id)
        .expect("restore before payout result");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).unwrap();
    assert!(resumed.is_err(), "external validation is still required");
    let restored = status(&pic, vault, caller, &session);
    assert_eq!(restored.reserved_for_withdrawal, 0);
    assert_eq!(restored.withdrawable, 700_000);
    let pending_validation: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).unwrap();
    assert!(pending_validation.unwrap());
}

#[test]
fn payout_rejection_and_unknown_are_handled() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(46);
    let key = secret(137);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 17);
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    let reserve = provisioned.expect("provisioned");

    let eoa = address_from_secret(&key).expect("address");
    let withdraw = |request_id: &[u8], nonce: u64| -> Result<FundRequestAccepted, ErrorCode> {
        let intent = private_perp::Withdrawal {
            eoa,
            amount: 250_000,
            asset: "usdc".to_string(),
            destination: format!("0x{}", hex::encode(eoa)),
            network: "local".to_string(),
            nonce,
            expires_at: 1_700_000_600_000u64,
            canister: vault.as_slice().to_vec(),
        };
        let signature = intent.sign_for_tests(&key).expect("sign");
        update(
            &pic,
            vault,
            caller,
            "request_withdrawal",
            WithdrawalRequest {
                session: session.clone(),
                client_request_id: blob(request_id),
                amount: 250_000,
                asset: AssetId::Usdc,
                destination: Destination::AuthenticatedEoaHlAccount,
                network: Network::Local,
                nonce,
                expires_at: 1_700_000_600_000u64,
                intent_signature: signature.to_bytes65().to_vec().into(),
            },
        )
        .expect("call")
    };

    // 取引所が拒否 → 予約は解放され、資金は元に戻る。
    withdraw(b"payout-reject", 1).expect("accepted");
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
    assert_eq!(after.reserved_for_withdrawal, 0, "予約と台帳の拘束が解ける");

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
        FundRequestState::Rejected
    );

    // 応答喪失 → unknownとして保持（再送しない）。
    withdraw(b"payout-unknown", 2).expect("accepted");
    let (swept, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
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
    assert_eq!(
        after.reserved_for_withdrawal, 250_000,
        "不明な払出しは拘束を保持"
    );
    assert_eq!(after.unknowns.len(), 1);

    let wire: serde_json::Value = serde_json::from_slice(&captured.unwrap().body).unwrap();
    let nonce = wire["nonce"].as_u64().unwrap();
    pic.advance_time(Duration::from_secs(61));
    assert_eq!(
        pocket_ic_tests::resume_manual_work(&pic, vault, caller, &session, false, "fund"),
        1
    );
    let history=serde_json::json!([{"time":nonce,"hash":format!("0x{}",hex::encode([68;32])),"delta":{"type":"internalTransfer","user":format!("0x{}",hex::encode(reserve)),"destination":format!("0x{}",hex::encode(eoa)),"usdc":"0.25"}}]).to_string().into_bytes();
    let (swept_again, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, history)),
    )
    .unwrap();
    assert_eq!(swept_again.unwrap(), 0, "never resend the payout");
    assert!(captured.unwrap().url.ends_with("/info"));
    let after = status(&pic, vault, caller, &session);
    assert!(after.unknowns.is_empty());
    assert_eq!(after.reserved_for_withdrawal, 0);
    assert_eq!(after.withdrawable, 750_000);
}

#[test]
fn free_text_cannot_release_an_unknown_action() {
    let pic = pic();
    let controller = principal(47);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(48);
    let key = secret(138);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 18);
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    provisioned.expect("provisioned");

    let eoa = address_from_secret(&key).expect("address");
    let intent = private_perp::Withdrawal {
        eoa,
        amount: 250_000,
        asset: "usdc".to_string(),
        destination: format!("0x{}", hex::encode(eoa)),
        network: "local".to_string(),
        nonce: 9,
        expires_at: 1_700_000_600_000u64,
        canister: vault.as_slice().to_vec(),
    };
    let signature = intent.sign_for_tests(&key).expect("sign");
    let accepted: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        WithdrawalRequest {
            session: session.clone(),
            client_request_id: blob(b"resolve-1"),
            amount: 250_000,
            asset: AssetId::Usdc,
            destination: Destination::AuthenticatedEoaHlAccount,
            network: Network::Local,
            nonce: 9,
            expires_at: 1_700_000_600_000u64,
            intent_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("call");
    accepted.expect("accepted");

    // 応答喪失でunknownにする。
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
    let unknown = status(&pic, vault, caller, &session);
    assert_eq!(unknown.unknowns.len(), 1);
    let action_id = unknown.unknowns[0].action_id.clone();

    // 証跡が空の解消は拒否する。
    let no_evidence: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "resolve_unknown_action",
        (action_id.clone(), false, String::new()),
    )
    .expect("call");
    assert!(no_evidence.is_err(), "証跡なしの解消は拒否する");

    // controllerの自由文も未実行の証明にならない。予約を保持する。
    let resolved: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "resolve_unknown_action",
        (
            action_id.clone(),
            false,
            "testnetの/api照会でcloidの注文が存在しないことを確認".to_string(),
        ),
    )
    .expect("call");
    assert!(matches!(
        resolved,
        Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable
        })
    ));

    let after = status(&pic, vault, caller, &session);
    assert_eq!(after.unknowns.len(), 1, "未解決actionを保持する");
    assert_eq!(after.reserve_unallocated, 750_000, "資金を解放しない");
    assert_eq!(after.reserved_for_withdrawal, 250_000);

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
        FundRequestState::Unknown
    );

    // 繰り返しの自由文でも解消できない。
    let again: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "resolve_unknown_action",
        (action_id, false, "再確認".to_string()),
    )
    .expect("call");
    assert!(again.is_err(), "解消済みは再度解消できない");
}

/// vaultのconfig（`HL_CHAIN_NAME` / `HL_USER_SIGNED_CHAIN_ID`）と同じ値。
/// 送信内容を独立に検証するため、テスト側にも同じ値を置く（不一致なら署名検証が落ちる）。
const HL_CHAIN_NAME: &str = "Testnet";
const HL_USER_SIGNED_CHAIN_ID: u64 = 421_614;
/// ローカル（テスト）の既定`/exchange`。環境設定から解決される（ビルド定数ではない）。
const HL_EXCHANGE_URL: &str = "http://localhost:8080/exchange";

/// 送信された `usdSend` のbodyから署名対象ダイジェストを独立に再構成する。
fn usd_send_digest(action: &serde_json::Value) -> [u8; 32] {
    use hl_sign::user_signed::{self, TypedValue};
    let destination = action["destination"].as_str().expect("destination");
    let amount = action["amount"].as_str().expect("amount");
    let time = action["time"].as_u64().expect("time");
    let values = vec![
        TypedValue::String(HL_CHAIN_NAME.to_string()),
        TypedValue::String(destination.to_string()),
        TypedValue::String(amount.to_string()),
        TypedValue::Uint64(time),
    ];
    user_signed::digest(
        HL_USER_SIGNED_CHAIN_ID,
        user_signed::USD_SEND_PRIMARY_TYPE,
        user_signed::USD_SEND_FIELDS,
        &values,
    )
    .expect("digest")
}

fn blob20(value: &Blob) -> [u8; 20] {
    value.as_ref().try_into().expect("20 bytes")
}

/// `usdSend` の署名から復元した送信元アドレスを返す。
fn recover_signer(action: &serde_json::Value, signature: &serde_json::Value) -> [u8; 20] {
    let digest = usd_send_digest(action);
    let signature = hl_sign::Signature {
        r: hex::decode(signature["r"].as_str().expect("r").trim_start_matches("0x"))
            .expect("r hex")
            .try_into()
            .expect("r len"),
        s: hex::decode(signature["s"].as_str().expect("s").trim_start_matches("0x"))
            .expect("s hex")
            .try_into()
            .expect("s len"),
        v: signature["v"].as_u64().expect("v") as u8,
    };
    hl_sign::recover_address(&digest, &signature, None).expect("recover")
}

/// 配分の送信内容を検証する（宛先が取引口座、署名者が**準備口座**、非replicated）。
///
/// 以前は署名者が保存済み口座と一致しない新しい導出鍵で、宛先も取引口座自身だった。
/// mockはどの鍵で署名しても受理を返すため、送信内容を検査しないと検出できない。
#[test]
fn an_allocation_is_sent_from_the_reserve_to_the_trading_account() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(45);
    let key = secret(141);
    let session = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 15);

    // 入金先（準備口座）を用意する。署名者はこの口座でなければならない。
    let provisioned: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    let reserve_address = blob20(&provisioned.expect("provisioned"));

    allocate(&pic, vault, caller, &session, b"out-send", 400_000).expect("accepted");
    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading_address = blob20(&trading.expect("trading address"));

    let (swept, captured): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);
    let captured = captured.expect("outcallが送信される");

    assert_eq!(captured.url, HL_EXCHANGE_URL);
    assert_eq!(
        captured.replication,
        pocket_ic::common::rest::CanisterHttpReplication::NonReplicated,
        "状態変更POSTは非replicatedで送る"
    );

    let body: serde_json::Value = serde_json::from_slice(&captured.body).expect("body json");
    let action = &body["action"];
    assert_eq!(action["type"], "usdSend");
    assert_eq!(
        action["destination"].as_str().expect("destination"),
        format!("0x{}", hex::encode(trading_address)),
        "宛先は本人の取引口座"
    );
    assert_eq!(action["amount"].as_str().expect("amount"), "0.4");
    assert_eq!(
        recover_signer(action, &body["signature"]),
        reserve_address,
        "配分は準備口座のmaster鍵で署名する"
    );
    assert_ne!(
        recover_signer(action, &body["signature"]),
        trading_address,
        "署名者は取引口座であってはならない（資金は準備口座から出る）"
    );
}

// Exercise the real async outbox, not a duplicate transaction implementation.
// Each stage starts with a fresh canister and proves the injected fault was reached.
fn atomicity_snapshot(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    method: &str,
    request: Vec<u8>,
) -> Result<(String, bool, bool, i64, i64, i64), String> {
    query::<_, Result<(String, bool, bool, i64, i64, i64), String>>(
        pic, vault, caller, method, request,
    )?
}

fn outbox_atomicity_faults(trap: bool, withdrawal: bool) {
    type Snapshot = (String, bool, bool, i64, i64, i64);
    for stage in 1u8..=3 {
        let pic = pic();
        let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
        let controller = pic.get_controllers(vault)[0];
        let caller = principal(47);
        let session = open_session(&pic, vault, caller, &secret(147));
        credit(&pic, vault, caller, &session, 1_000_000, 47);
        let request = b"atomicity-allocation".to_vec();
        if withdrawal {
            let provisioned: Result<Vec<u8>, ErrorCode> = update(
                &pic,
                vault,
                caller,
                "provision_reserve_account",
                session.clone(),
            )
            .unwrap();
            provisioned.unwrap();
            let eoa = address_from_secret(&secret(147)).unwrap();
            let expires_at = pocket_ic_tests::envelope::now_ms(&pic) + 600_000;
            let intent = private_perp::Withdrawal {
                eoa,
                amount: 400_000,
                asset: "usdc".into(),
                destination: format!("0x{}", hex::encode(eoa)),
                network: "local".into(),
                nonce: 1,
                expires_at,
                canister: vault.as_slice().to_vec(),
            };
            let accepted: Result<FundRequestAccepted, ErrorCode> = update(
                &pic,
                vault,
                caller,
                "request_withdrawal",
                WithdrawalRequest {
                    session: session.clone(),
                    client_request_id: blob(&request),
                    amount: 400_000,
                    asset: AssetId::Usdc,
                    destination: Destination::AuthenticatedEoaHlAccount,
                    network: Network::Local,
                    nonce: 1,
                    expires_at,
                    intent_signature: intent
                        .sign_for_tests(&secret(147))
                        .unwrap()
                        .to_bytes65()
                        .to_vec()
                        .into(),
                },
            )
            .unwrap();
            accepted.unwrap();
        } else {
            allocate(&pic, vault, caller, &session, &request, 400_000).expect("accepted");
        }
        let before: Snapshot = atomicity_snapshot(
            &pic,
            vault,
            controller,
            "test_outbox_atomicity_snapshot",
            request.clone(),
        )
        .expect("snapshot before sweep");
        assert_eq!(before, ("queued".into(), false, false, 0, 0, 0));
        let _: () = update_args(
            &pic,
            vault,
            controller,
            "test_set_outbox_fault",
            (stage, trap),
        )
        .expect("enable fault");

        let message = pic
            .submit_call(
                vault,
                caller,
                "test_sweep_now",
                candid::encode_one(()).unwrap(),
            )
            .expect("submit faulted sweep");
        let mut completed = None;
        for _ in 0..200 {
            pic.tick();
            // Never mock/consume a POST: even a subsequently trapped call must
            // fail this assertion if it issued an HTTP request before commit.
            assert!(
                pic.get_canister_http().is_empty(),
                "stage {stage}: unexpected HTTP"
            );
            if let Some(result) = pic.ingress_status(message.clone()) {
                completed = Some(result);
                break;
            }
        }
        let result = completed.expect("faulted sweep completed");
        if trap {
            let error = result.expect_err("trap must reject the call");
            assert!(
                format!("{error:?}").contains("outbox atomicity fault"),
                "stage {stage}: {error:?}"
            );
        } else {
            let bytes = result.expect("application error reply");
            let result: Result<u32, ErrorCode> = candid::decode_one(&bytes).expect("decode error");
            let error = result.expect_err("injected application error");
            assert!(format!("{error:?}").contains("outbox atomicity fault"));
        }
        assert!(
            pic.get_canister_http().is_empty(),
            "no pending HTTP request"
        );
        let failed: Snapshot = atomicity_snapshot(
            &pic,
            vault,
            controller,
            "test_outbox_atomicity_snapshot",
            request.clone(),
        )
        .expect("snapshot after failed transaction");
        // Claim ran in an earlier message/transaction and remains committed.
        assert_eq!(
            failed,
            ("signing".into(), false, false, 1, 0, 0),
            "stage {stage}"
        );
        let funds = status(&pic, vault, caller, &session);
        assert_eq!(
            funds.reserve_unallocated,
            if withdrawal { 600_000 } else { 1_000_000 }
        );
        assert_eq!(funds.withdrawable, 600_000);
        assert_eq!(funds.in_transit, 0);

        let _: () = update_args(
            &pic,
            vault,
            controller,
            "test_set_outbox_fault",
            (0u8, false),
        )
        .expect("disable fault in a separate message");
        pic.advance_time(Duration::from_secs(31));
        let (stopped, sent): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
            &pic,
            vault,
            caller,
            "test_sweep_now",
            (),
            Ok((200, ACCEPTED.to_vec())),
        )
        .expect("stopped sweep");
        assert_eq!(stopped.unwrap(), 0, "faulted work needs owner permission");
        assert!(sent.is_none());
        assert_eq!(
            pocket_ic_tests::resume_manual_work(&pic, vault, caller, &session, false, "fund"),
            1
        );
        let (retry, sent): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
            &pic,
            vault,
            caller,
            "test_sweep_now",
            (),
            Ok((200, ACCEPTED.to_vec())),
        )
        .expect("retry call");
        // The durable journal writer was claimed BEFORE the await. Its release
        // belongs to the rolled-back transaction, so ordinary retry stays blocked.
        assert!(
            matches!(retry, Err(ErrorCode::JournalWriterBusy)),
            "{retry:?}"
        );
        assert!(sent.is_none());
        let retried: Snapshot = atomicity_snapshot(
            &pic,
            vault,
            controller,
            "test_outbox_atomicity_snapshot",
            request.clone(),
        )
        .expect("snapshot after blocked retry");
        assert_eq!(retried, ("signing".into(), false, false, 2, 0, 0));

        // Exercise the existing authorized manual recovery, not a raw DB unlock.
        let guard: Result<Option<Principal>, ErrorCode> =
            query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
        let guard = guard.expect("guard result").expect("configured guard");
        // Unauthorized callers cannot initiate cancellation or clear the writer.
        let denied: Result<(), ErrorCode> =
            update(&pic, vault, caller, "resume_journal", ()).unwrap();
        assert!(matches!(denied, Err(ErrorCode::Unauthenticated { .. })));
        for _ in 0..2 {
            let resumed: Result<(), ErrorCode> =
                update(&pic, guard, principal(239), "resume_journal", vault)
                    .expect("authorized SNS resume call");
            resumed.expect("manual recovery; repeated calls are harmless");
        }
        let journal_status: Result<(bool, bool), ErrorCode> =
            query(&pic, vault, controller, "get_journal_send_status", ()).expect("journal status");
        assert_eq!(journal_status.unwrap(), (false, false));
        let restored = status(&pic, vault, caller, &session);
        assert_eq!(restored.reserve_unallocated, 1_000_000);
        assert_eq!(
            restored.withdrawable, 1_000_000,
            "cancelled send releases its hold once"
        );
        assert_eq!(restored.in_transit, 0);
        let after_resume: Snapshot = atomicity_snapshot(
            &pic,
            vault,
            controller,
            "test_outbox_atomicity_snapshot",
            request.clone(),
        )
        .unwrap();
        assert_eq!(after_resume, ("aborted".into(), false, false, 2, 1, 0));
        // Other/new requests proceed, but the cancelled action is never re-sent.
        allocate(
            &pic,
            vault,
            caller,
            &session,
            b"after-manual-recovery",
            100_000,
        )
        .unwrap();
        let (sent, http): (Result<u32, ErrorCode>, _) = call_with_mocked_outcall_captured(
            &pic,
            vault,
            caller,
            "test_sweep_now",
            (),
            Ok((200, ACCEPTED.to_vec())),
        )
        .expect("new request after recovery");
        assert_eq!(sent.unwrap(), 1);
        assert!(http.is_some());
        assert_eq!(status(&pic, vault, caller, &session).in_transit, 100_000);

        // Reinitialize DB connections from stable memory, then inspect before any
        // journal replay or sweep. An upgrade also locks sending by design.
        pocket_ic_tests::upgrade(
            &pic,
            vault,
            FUNDS_VAULT_WASM,
            candid::encode_one(()).unwrap(),
        );
        let reopened: Snapshot = atomicity_snapshot(
            &pic,
            vault,
            controller,
            "test_outbox_atomicity_snapshot",
            request,
        )
        .expect("snapshot after upgrade");
        assert_eq!(
            reopened, after_resume,
            "stage {stage}: persisted rollback state"
        );
    }
}

#[test]
fn outbox_atomicity_application_errors_rollback_all_send_writes() {
    outbox_atomicity_faults(false, false);
}

#[test]
fn outbox_atomicity_traps_rollback_all_send_writes() {
    outbox_atomicity_faults(true, false);
}

#[test]
fn withdrawal_atomicity_errors_are_manually_recoverable() {
    outbox_atomicity_faults(false, true);
}

#[test]
fn withdrawal_atomicity_traps_are_manually_recoverable() {
    outbox_atomicity_faults(true, true);
}

#[test]
fn manual_recovery_refuses_a_send_with_remote_authorization() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = pic.get_controllers(vault)[0];
    let caller = principal(47);
    let session = open_session(&pic, vault, caller, &secret(147));
    credit(&pic, vault, caller, &session, 1_000_000, 47);
    allocate(
        &pic,
        vault,
        caller,
        &session,
        b"authorized-ambiguity",
        400_000,
    )
    .unwrap();
    let _: () = update_args(
        &pic,
        vault,
        controller,
        "test_set_outbox_fault",
        (1u8, false),
    )
    .unwrap();
    let failed: Result<u32, ErrorCode> = update(&pic, vault, caller, "test_sweep_now", ()).unwrap();
    assert!(format!("{failed:?}").contains("outbox atomicity fault"));
    let _: () = update_args(
        &pic,
        vault,
        controller,
        "test_set_outbox_fault",
        (0u8, false),
    )
    .unwrap();
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let records: Result<Vec<JournalRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "records", (0u64, 100u32)).unwrap();
    let record = records.unwrap().pop().unwrap();
    // Model the ambiguity of a local rollback: remote authorization is durable,
    // while the local row appears unsigned. No assertion that a POST happened.
    let authorized: Result<bool, ErrorCode> = update_args(
        &pic,
        journal,
        vault,
        "authorize_send",
        (record.intent.kind, record.intent.request_id),
    )
    .unwrap();
    assert!(authorized.unwrap());
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).unwrap();
    let resumed: Result<(), ErrorCode> = update(
        &pic,
        guard.unwrap().unwrap(),
        principal(239),
        "resume_journal",
        vault,
    )
    .unwrap();
    assert!(matches!(resumed, Err(ErrorCode::PolicyUnavailable)));
    assert_eq!(status(&pic, vault, caller, &session).withdrawable, 600_000);
    let local = atomicity_snapshot(
        &pic,
        vault,
        controller,
        "test_outbox_atomicity_snapshot",
        b"authorized-ambiguity".to_vec(),
    )
    .unwrap();
    assert_eq!(local, ("signing".into(), false, false, 1, 0, 0));
    assert!(pic.get_canister_http().is_empty());
}

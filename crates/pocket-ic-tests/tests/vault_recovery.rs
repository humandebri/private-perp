//! 配分の着金から回収までの往復の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AllocationRequest, FundRequestAccepted, FundRequestState, FundStatus};
use api_types::journal::{JournalHead, RecoveryPayload, RecoveryRecord};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
use pocket_ic_tests::{
    CapturedHttpCall, FUNDS_VAULT_WASM, TRADING_CORE_WASM, call_with_mocked_outcall,
    call_with_routed_outcalls, configure_policy, deploy, pic, principal, query, update,
    update_args,
};

const ORIGIN: &str = "https://app.example.test";
const ACCEPTED: &[u8] = br#"{"status":"ok","response":{"type":"default"}}"#;
const REJECTED: &[u8] = br#"{"status":"err","response":"insufficient balance"}"#;

fn secret(seed: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes
}

fn blob(value: &[u8]) -> Blob {
    value.to_vec().into()
}

fn enable_recovery_core(pic: &PocketIc, vault: Principal, controller: Principal) -> Principal {
    let core = deploy(
        pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let configured: Result<(), ErrorCode> =
        update(pic, core, controller, "set_vault_principal", vault).expect("call");
    configured.expect("set vault principal");
    configure_policy(pic, core, controller, &["BTC", "ETH"]);
    core
}

fn recovery_route(
    call: &pocket_ic_tests::CapturedHttpCall,
    exchange: &[u8],
) -> Result<(u16, Vec<u8>), (u64, String)> {
    let body: serde_json::Value = serde_json::from_slice(&call.body).expect("JSON request");
    if call.url.ends_with("/info") {
        match body.get("type").and_then(|value| value.as_str()) {
            Some("openOrders") => Ok((200, b"[]".to_vec())),
            Some("clearinghouseState") => Ok((200, br#"{"assetPositions":[]}"#.to_vec())),
            other => panic!("unexpected info request: {other:?}"),
        }
    } else {
        assert_eq!(body["action"]["type"], "usdSend");
        Ok((200, exchange.to_vec()))
    }
}

/// テスト専用の入金計上（`test-venue`）。
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

/// 配分を要求する（受付のみ。署名・送信はsweep）。
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
fn recovery_does_not_reserve_when_journal_is_unavailable() {
    let pic = pic();
    let controller = principal(180);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(181);
    let session = open_session(&pic, vault, caller, &secret(182));
    credit(&pic, vault, caller, &session, 1_000_000, 183);
    allocate(&pic, vault, caller, &session, b"outage-allocation", 400_000).unwrap();
    let sent: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .unwrap();
    assert_eq!(sent.unwrap(), 1);
    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).unwrap();
    let arrival: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[184u8; 32]),
            400_000u64,
            trading.unwrap(),
            "usdc".to_string(),
        ),
    )
    .unwrap();
    assert!(arrival.unwrap());
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).unwrap();
    pic.stop_canister(configured.unwrap().unwrap(), Some(controller))
        .unwrap();

    let rejected: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recovery-journal-down"), 200_000u64),
    )
    .unwrap();
    assert!(rejected.is_err());
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).unwrap();
    let status = status.unwrap();
    assert_eq!(status.trading_equity, 400_000);
    assert!(status.recovery_fence.is_none());
    assert!(status.unknowns.is_empty());
}

#[test]
fn recovery_acceptance_replays_from_snapshot_before_request() {
    let pic = pic();
    let controller = principal(245);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    enable_recovery_core(&pic, vault, controller);
    let caller = principal(244);
    let session = open_session(&pic, vault, caller, &secret(154));
    credit(&pic, vault, caller, &session, 1_000_000, 54);
    allocate(
        &pic,
        vault,
        caller,
        &session,
        b"replay-recovery-alloc",
        400_000,
    )
    .expect("allocation");
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
    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone())
            .expect("trading address");
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[55u8; 32]),
            400_000u64,
            trading.expect("trading address"),
            "usdc".to_string(),
        ),
    )
    .expect("trading arrival");
    assert!(credited.expect("arrival accepted"));
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
    let guard = guard.expect("guard result").expect("configured guard");
    let before_request = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot before recovery acceptance");

    let accepted: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (
            session.clone(),
            blob(b"replay-accepted-recovery"),
            400_000u64,
        ),
    )
    .expect("recovery call");
    accepted.expect("recovery accepted");

    pic.load_canister_snapshot(vault, Some(controller), before_request.id)
        .expect("restore before recovery acceptance");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
    assert!(resumed.is_err(), "external validation is still required");
    let restored: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("restored status");
    let restored = restored.expect("status");
    assert_eq!(restored.trading_equity, 400_000);
    assert_eq!(restored.reserve_unallocated, 600_000);
    assert_eq!(
        restored.recovery_fence,
        Some(api_types::fund::RecoveryFenceStatus::Preparing)
    );
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session, None::<Blob>, 10u32),
    )
    .expect("restored events");
    let events = events.expect("events");
    let recovery = events
        .items
        .iter()
        .find(|event| event.kind == api_types::fund::FundActionKind::Recovery)
        .expect("replayed recovery request");
    assert_eq!(recovery.state, FundRequestState::Reserved);
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
    enable_recovery_core(&pic, vault, controller);
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
        FundRequestState::Settled,
        "取引口座への着金でallocation自体も完了する"
    );

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
    let repeated: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-recovery"), 400_000u64),
    )
    .expect("duplicate recovery");
    assert_eq!(
        repeated.expect("same request").state,
        FundRequestState::Accepted
    );
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 20u32))
            .expect("recovery events");
    let accepted_events: Vec<_> = records
        .expect("private recovery events")
        .into_iter()
        .filter(|record| {
            matches!(
                record.event.payload,
                RecoveryPayload::RecoveryAccepted { .. }
            )
        })
        .collect();
    assert_eq!(accepted_events.len(), 1, "duplicate must not append again");
    match &accepted_events[0].event.payload {
        RecoveryPayload::RecoveryAccepted {
            request_id,
            amount_micros,
            nonce,
            ..
        } => {
            assert_eq!(request_id.as_ref(), b"recv-recovery");
            assert_eq!(*amount_micros, 400_000);
            assert!(*nonce > 0);
        }
        _ => unreachable!(),
    }
    let message_id = pic
        .submit_call(
            vault,
            caller,
            "test_sweep_now",
            candid::encode_one(()).expect("sweep args"),
        )
        .expect("submit recovery sweep");
    let mut calls = Vec::new();
    let mut before_exchange = None;
    for _ in 0..200 {
        pic.tick();
        for request in pic.get_canister_http() {
            let call = CapturedHttpCall {
                url: request.url.clone(),
                method: request.http_method.clone(),
                body: request.body.clone(),
                replication: request.replication.clone(),
            };
            let (status, body) = recovery_route(&call, ACCEPTED).expect("mock route");
            if call.url.ends_with("/exchange") {
                before_exchange = Some(
                    pic.take_canister_snapshot(vault, Some(controller), None)
                        .expect("snapshot before recovery response"),
                );
            }
            calls.push(call);
            pic.mock_canister_http_response(MockCanisterHttpResponse {
                subnet_id: request.subnet_id,
                request_id: request.request_id,
                response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status,
                    headers: Vec::new(),
                    body,
                }),
                additional_responses: Vec::new(),
            });
        }
        if pic.ingress_status(message_id.clone()).is_some() {
            break;
        }
    }
    let reply = pic.await_call(message_id).expect("recovery sweep reply");
    let swept: Result<u32, ErrorCode> = candid::decode_one(&reply).expect("decode sweep");
    assert_eq!(swept.expect("sweep"), 1);
    assert_eq!(calls.len(), 5, "prepareとcommitで4照会、送金で1POST");
    let send = calls
        .iter()
        .find(|call| call.url.ends_with("/exchange"))
        .expect("usdSend");
    let sent: serde_json::Value = serde_json::from_slice(&send.body).expect("send body");
    assert_eq!(sent["action"]["type"], "usdSend");

    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 0);
    assert_eq!(status.reserve_unallocated, 1_000_000);
    assert!(status.recovery_fence.is_none());

    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let head: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "recovery_head", ()).expect("recovery head");
    let head = head.expect("head");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    let records = records.expect("private recovery events");
    assert_eq!(head.sequence as usize, records.len());
    assert!(head.sequence >= 2);
    assert!(records.iter().any(|record| matches!(
        &record.event.payload,
        RecoveryPayload::RecoveryPostResult {
            amount_micros: 400_000,
            accepted: true,
            ..
        }
    )));
    let local: Result<(u64, bool), ErrorCode> =
        query(&pic, vault, controller, "recovery_stage_status", ()).expect("local recovery head");
    assert_eq!(local.expect("local receipt"), (head.sequence, false));

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
    pic.load_canister_snapshot(
        vault,
        Some(controller),
        before_exchange.expect("recovery exchange outcall").id,
    )
    .expect("restore before recovery result");
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
    let guard = guard.expect("guard result").expect("configured guard");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
    assert!(resumed.is_err(), "external validation is still required");
    let restored: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).expect("restored status");
    let restored = restored.expect("status");
    assert_eq!(restored.trading_equity, 0);
    assert_eq!(restored.reserve_unallocated, 1_000_000);
    let pending_validation: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).expect("pending query");
    assert!(pending_validation.expect("pending validation"));
}

/// allocationは複数のledger updateで部分着金しても、累計額で完了する。
#[test]
fn allocation_settles_after_partial_arrivals() {
    let pic = pic();
    let controller = principal(94);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(93);
    let session = open_session(&pic, vault, caller, &secret(164));
    credit(&pic, vault, caller, &session, 1_000_000, 41);
    allocate(
        &pic,
        vault,
        caller,
        &session,
        b"partial-allocation",
        400_000,
    )
    .expect("allocation");
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

    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading_address = trading.expect("trading address");
    for (seed, amount) in [(42u8, 150_000u64), (43u8, 250_000u64)] {
        let credited: Result<bool, ErrorCode> = update_args(
            &pic,
            vault,
            controller,
            "credit_venue_deposit",
            (
                blob(&[seed; 32]),
                amount,
                trading_address.clone(),
                "usdc".to_string(),
            ),
        )
        .expect("call");
        assert!(credited.expect("arrival"));

        let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
            &pic,
            vault,
            caller,
            "list_fund_events",
            (session.clone(), None::<Blob>, 10u32),
        )
        .expect("call");
        let expected = if amount == 150_000 {
            FundRequestState::Executing
        } else {
            FundRequestState::Settled
        };
        assert_eq!(events.expect("events").items[0].state, expected);
    }

    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).expect("call");
    let status = status.expect("status");
    assert_eq!(status.in_transit, 0);
    assert_eq!(status.trading_equity, 400_000);
}

/// 1件の着金が複数要求を満たす場合もFIFOで全要求を完了し、超過分は直接与信する。
#[test]
fn one_arrival_settles_multiple_allocations_and_credits_excess() {
    let pic = pic();
    let controller = principal(92);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(91);
    let session = open_session(&pic, vault, caller, &secret(163));
    credit(&pic, vault, caller, &session, 1_000_000, 44);
    allocate(&pic, vault, caller, &session, b"allocation-a", 200_000).expect("allocation a");
    allocate(&pic, vault, caller, &session, b"allocation-b", 400_000).expect("allocation b");
    let (swept, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |_| {
            Ok((200, ACCEPTED.to_vec()))
        })
        .expect("call");
    assert_eq!(swept.expect("allocation sweep"), 2);
    assert_eq!(calls.len(), 2);

    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[45u8; 32]),
            700_000u64,
            trading.expect("trading address"),
            "usdc".to_string(),
        ),
    )
    .expect("call");
    assert!(credited.expect("arrival"));

    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let events = events.expect("events");
    assert_eq!(events.items.len(), 2);
    assert!(
        events
            .items
            .iter()
            .all(|event| event.state == FundRequestState::Settled)
    );
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).expect("call");
    let status = status.expect("status");
    assert_eq!(status.in_transit, 0);
    assert_eq!(status.trading_equity, 700_000);
}

/// 取引口座のequityを超える回収と、equityを超える二重の回収を拒否する。
///
/// 拘束が無いと、同じequityに対して複数の回収が同時に署名・送信され得る。
#[test]
fn a_recovery_over_the_trading_equity_is_rejected() {
    let pic = pic();
    let controller = principal(97);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    enable_recovery_core(&pic, vault, controller);
    let caller = principal(95);
    let session = open_session(&pic, vault, caller, &secret(165));
    credit(&pic, vault, caller, &session, 1_000_000, 5);
    allocate(&pic, vault, caller, &session, b"recv-alloc", 400_000).expect("allocation");

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

    // 着金を取り込んで取引口座のequityを400,000にする。
    let trading: Result<api_types::Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading_address = blob(&trading.expect("trading address"));
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[31u8; 32]),
            400_000u64,
            trading_address.clone(),
            "usdc".to_string(),
        ),
    )
    .expect("call");
    assert!(credited.expect("arrival"));

    // equityを超える回収は残高不足として拒否する。
    let too_much: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-too-much"), 500_000u64),
    )
    .expect("call");
    let error = too_much.expect_err("must reject over the trading equity");
    assert!(
        matches!(error, ErrorCode::InsufficientFunds { .. }),
        "{error:?}"
    );

    // 1件目の回収（全額）を予約した後、2件目は拘束により拒否する。
    let first: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-first"), 400_000u64),
    )
    .expect("call");
    first.expect("first recovery");
    let second: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-second"), 400_000u64),
    )
    .expect("call");
    let error = second.expect_err("must reject a second concurrent recovery");
    assert!(
        matches!(error, ErrorCode::InsufficientFunds { .. }),
        "{error:?}"
    );

    // 外部注文が残っていればprepareで未送信のまま中止し、予約を戻す。
    let (blocked, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |call| {
            let request: serde_json::Value = serde_json::from_slice(&call.body).expect("request");
            assert_eq!(request["type"], "openOrders");
            Ok((200, br#"[{"oid":42}]"#.to_vec()))
        })
        .expect("blocked sweep");
    assert_eq!(blocked.expect("blocked recovery"), 1);
    assert_eq!(calls.len(), 1, "回収POSTを送らない");
    let retry: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-retry"), 400_000u64),
    )
    .expect("retry request");
    retry.expect("未送信の回収なら予約が戻る");

    // 明示的な取引所拒否でも拘束が戻る。
    let message_id = pic
        .submit_call(
            vault,
            caller,
            "test_sweep_now",
            candid::encode_one(()).expect("sweep args"),
        )
        .expect("submit rejected recovery sweep");
    let mut calls = Vec::new();
    let mut before_exchange = None;
    for _ in 0..200 {
        pic.tick();
        for request in pic.get_canister_http() {
            let call = CapturedHttpCall {
                url: request.url.clone(),
                method: request.http_method.clone(),
                body: request.body.clone(),
                replication: request.replication.clone(),
            };
            let (status, body) = recovery_route(&call, REJECTED).expect("mock route");
            if call.url.ends_with("/exchange") {
                before_exchange = Some(
                    pic.take_canister_snapshot(vault, Some(controller), None)
                        .expect("snapshot before rejection response"),
                );
            }
            calls.push(call);
            pic.mock_canister_http_response(MockCanisterHttpResponse {
                subnet_id: request.subnet_id,
                request_id: request.request_id,
                response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status,
                    headers: Vec::new(),
                    body,
                }),
                additional_responses: Vec::new(),
            });
        }
        if pic.ingress_status(message_id.clone()).is_some() {
            break;
        }
    }
    let reply = pic
        .await_call(message_id)
        .expect("rejected recovery sweep reply");
    let swept: Result<u32, ErrorCode> = candid::decode_one(&reply).expect("decode sweep");
    assert_eq!(swept.expect("recovery sweep"), 1);
    assert_eq!(calls.len(), 5);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").trading_equity, 400_000);
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    assert!(
        records
            .expect("private recovery events")
            .iter()
            .any(|record| matches!(
                &record.event.payload,
                RecoveryPayload::RecoveryPostResult {
                    amount_micros: 400_000,
                    accepted: false,
                    ..
                }
            ))
    );
    let retry: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-after-rejection"), 400_000u64),
    )
    .expect("call");
    retry.expect("拒否後は拘束が解けて再要求できる");
    pic.load_canister_snapshot(
        vault,
        Some(controller),
        before_exchange.expect("recovery exchange outcall").id,
    )
    .expect("restore before rejected result");
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
    let guard = guard.expect("guard result").expect("configured guard");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
    assert!(resumed.is_err(), "external validation is still required");
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session, None::<Blob>, 10u32),
    )
    .expect("restored events");
    assert_eq!(
        events.expect("events").items[0].state,
        FundRequestState::Rejected
    );
    let pending_validation: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).expect("pending query");
    assert!(pending_validation.expect("pending validation"));
}

/// 取引口座への直接入金（保留中の配分が無い着金）も取引残高へ計上できる。
///
/// 以前は無条件に `allocation_confirm` を呼んでいたため `in_transit` が負債超過に
/// なり、以後の残高参照が不変条件違反で失敗した。
#[test]
fn a_direct_deposit_to_the_trading_account_is_credited() {
    let pic = pic();
    let controller = principal(98);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(96);
    let session = open_session(&pic, vault, caller, &secret(166));
    credit(&pic, vault, caller, &session, 1_000_000, 6);
    allocate(&pic, vault, caller, &session, b"recv-direct", 100_000).expect("allocation");

    let trading: Result<api_types::Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading_address = blob(&trading.expect("trading address"));

    // 配分の着金を取り込まずに、取引口座へ直接入金する。
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[32u8; 32]),
            250_000u64,
            trading_address,
            "usdc".to_string(),
        ),
    )
    .expect("call");
    assert!(credited.expect("direct deposit"));

    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(
        status.trading_equity, 250_000,
        "直接入金は取引口座のequityへ計上する"
    );
    assert_eq!(status.in_transit, 0, "保留中の配分は無い");
}

fn lost_recovery_response_keeps_reservation_and_fence_without_resending(
    history_usdc: serde_json::Value,
    ambiguous: bool,
) {
    let pic = pic();
    let controller = principal(90);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = enable_recovery_core(&pic, vault, controller);
    let caller = principal(89);
    let session = open_session(&pic, vault, caller, &secret(162));
    let reserve: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("reserve call");
    let reserve = reserve.expect("reserve address");
    credit(&pic, vault, caller, &session, 1_000_000, 80);
    allocate(&pic, vault, caller, &session, b"unknown-alloc", 400_000).expect("allocation");
    let sent: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("allocation call");
    assert_eq!(sent.expect("allocation sweep"), 1);
    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("call");
    let trading = trading.expect("trading address");
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[81u8; 32]),
            400_000u64,
            trading.clone(),
            "usdc".to_string(),
        ),
    )
    .expect("credit call");
    assert!(credited.expect("arrival"));
    let requested: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"unknown-recovery"), 400_000u64),
    )
    .expect("request call");
    requested.expect("recovery accepted");
    let (swept, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |call| {
            if call.url.ends_with("/exchange") {
                Err((4, "response lost".to_string()))
            } else {
                recovery_route(call, ACCEPTED)
            }
        })
        .expect("sweep call");
    assert_eq!(swept.expect("unknown sweep"), 1);
    let send = calls
        .iter()
        .find(|call| call.url.ends_with("/exchange"))
        .expect("recovery send");
    let send_body: serde_json::Value = serde_json::from_slice(&send.body).expect("send body");
    let nonce = send_body["nonce"].as_u64().expect("nonce");
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.ends_with("/exchange"))
            .count(),
        1
    );
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("status call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 400_000);
    assert_eq!(status.reserve_unallocated, 600_000);
    assert_eq!(
        status.recovery_fence,
        Some(api_types::fund::RecoveryFenceStatus::Reconciling)
    );
    assert_eq!(status.unknowns.len(), 1);
    let replace_core: Result<(), ErrorCode> =
        update(&pic, vault, controller, "set_core_principal", principal(85))
            .expect("replace core call");
    assert!(replace_core.is_err(), "フェンス中はcoreの参照先を変えない");
    let replace_vault: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", principal(84))
            .expect("replace vault call");
    assert!(
        replace_vault.is_err(),
        "フェンス中はvaultの参照先を変えない"
    );

    // 履歴照会が失敗してもカーソルは進まず、次のsweepで送金POSTを再送しない。
    let (next, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |_call| {
            Err((4, "ledger unavailable".to_string()))
        })
        .expect("next sweep");
    assert_eq!(next.expect("next sweep"), 0);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].url.ends_with("/info"));
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("status call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 400_000);
    assert_eq!(
        status.recovery_fence,
        Some(api_types::fund::RecoveryFenceStatus::Reconciling)
    );

    // 2日を過ぎても、上限500件に達したページは完全な履歴とみなさない。
    pic.advance_time(std::time::Duration::from_secs(
        2 * 24 * 60 * 60 + 2 * 60 * 60,
    ));
    let (checked, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |call| {
            let request: serde_json::Value =
                serde_json::from_slice(&call.body).expect("ledger request");
            let at = request["startTime"].as_u64().expect("startTime");
            let entries: Vec<_> = (0..500)
                .map(|index| {
                    serde_json::json!({
                        "hash": format!("0x{:064x}", index + 1), "time": at,
                        "delta": {"type": "withdraw"}
                    })
                })
                .collect();
            Ok((200, serde_json::to_vec(&entries).expect("entries")))
        })
        .expect("capped page call");
    assert_eq!(checked.expect("capped page sweep"), 0);
    assert_eq!(calls.len(), 1);
    let renewed = open_session(&pic, vault, caller, &secret(162));
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", renewed.clone()).expect("status call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 400_000);
    assert_eq!(
        status.recovery_fence,
        Some(api_types::fund::RecoveryFenceStatus::Reconciling)
    );

    // マイクロUSDCに丸められない履歴は証拠にならず、予約を解放しない。
    let invalid = serde_json::json!([{
        "hash": format!("0x{}", hex::encode([83u8; 32])), "time": nonce,
        "delta": {"type": "internalTransfer", "user": format!("0x{}", hex::encode(trading.as_ref())),
            "destination": format!("0x{}", hex::encode(reserve.as_ref())), "usdc": 0.0000001}
    }]);
    let (unchanged, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |_call| {
            Ok((200, invalid.to_string().into_bytes()))
        })
        .expect("invalid history call");
    assert_eq!(unchanged.expect("invalid history sweep"), 0);
    assert_eq!(calls.len(), 1);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", renewed.clone()).expect("status call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 400_000);
    assert_eq!(status.reserve_unallocated, 600_000);
    assert_eq!(
        status.recovery_fence,
        Some(api_types::fund::RecoveryFenceStatus::Reconciling)
    );

    let mut events = vec![serde_json::json!({
        "hash": format!("0x{}", hex::encode([82u8; 32])), "time": nonce,
        "delta": {"type": "internalTransfer", "user": format!("0x{}", hex::encode(trading.as_ref())),
            "destination": format!("0x{}", hex::encode(reserve.as_ref())), "usdc": history_usdc}
    })];
    if ambiguous {
        let mut second = events[0].clone();
        second["hash"] = serde_json::json!(format!("0x{}", hex::encode([84u8; 32])));
        events.push(second);
    }
    let before_history = if ambiguous {
        None
    } else {
        Some(
            pic.take_canister_snapshot(vault, Some(controller), None)
                .expect("snapshot before history resolution"),
        )
    };
    let (reconciled, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |_call| {
            Ok((200, serde_json::to_vec(&events).expect("events")))
        })
        .expect("reconcile call");
    assert_eq!(reconciled.expect("reconcile sweep"), 0);
    assert_eq!(calls.len(), 1);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", renewed.clone()).expect("status call");
    let status = status.expect("status");
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    let records = records.expect("private recovery events");
    if ambiguous {
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(
                    &record.event.payload,
                    RecoveryPayload::RecoverySettlement { .. }
                ))
                .count(),
            0,
            "ambiguous history cannot settle"
        );
        assert_eq!(status.trading_equity, 400_000);
        assert_eq!(status.reserve_unallocated, 600_000);
        assert_eq!(
            status.recovery_fence,
            Some(api_types::fund::RecoveryFenceStatus::Reconciling)
        );
    } else {
        assert!(records.iter().any(|record| matches!(
            &record.event.payload,
            RecoveryPayload::RecoverySettlement {
                amount_micros: 400_000,
                accepted: true,
                ..
            }
        )));
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(
                    &record.event.payload,
                    RecoveryPayload::RecoverySettlement { .. }
                ))
                .count(),
            1
        );
        assert_eq!(status.trading_equity, 0);
        assert_eq!(status.reserve_unallocated, 1_000_000);
        assert!(status.recovery_fence.is_none());
        pic.load_canister_snapshot(
            vault,
            Some(controller),
            before_history.expect("history snapshot").id,
        )
        .expect("restore before history result");
        let guard: Result<Option<Principal>, ErrorCode> =
            query(&pic, vault, controller, "get_journal_guard", ()).expect("guard query");
        let guard = guard.expect("guard result").expect("configured guard");
        let resumed: Result<(), ErrorCode> =
            update(&pic, guard, principal(239), "resume_journal", vault).expect("guard call");
        assert!(
            resumed.is_err(),
            "history proof cannot be replayed from its digest"
        );
        let restored: Result<FundStatus, ErrorCode> =
            update(&pic, vault, caller, "get_fund_status", renewed).expect("restored status");
        let restored = restored.expect("status");
        assert_eq!(restored.trading_equity, 400_000);
        assert_eq!(restored.reserve_unallocated, 600_000);
        assert_eq!(restored.unknowns.len(), 1);
        assert_eq!(
            restored.recovery_fence,
            Some(api_types::fund::RecoveryFenceStatus::Reconciling)
        );
        let stage: Result<(u64, bool), ErrorCode> =
            query(&pic, vault, controller, "recovery_stage_status", ()).expect("stage query");
        assert!(stage.expect("stage status").1, "sending remains locked");
    }
}

#[test]
fn lost_recovery_response_reconciles_string_amount() {
    lost_recovery_response_keeps_reservation_and_fence_without_resending(
        serde_json::json!("-0.4"),
        false,
    );
}

#[test]
fn lost_recovery_response_reconciles_numeric_amount() {
    lost_recovery_response_keeps_reservation_and_fence_without_resending(
        serde_json::json!(-0.4),
        false,
    );
}

#[test]
fn ambiguous_numeric_recovery_history_keeps_reservation_and_fence() {
    lost_recovery_response_keeps_reservation_and_fence_without_resending(
        serde_json::json!(-0.4),
        true,
    );
}

#[test]
fn core_upgrade_locks_new_risk_until_vault_migration_ack() {
    let pic = pic();
    let controller = principal(87);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = enable_recovery_core(&pic, vault, controller);
    let initial: Result<bool, ErrorCode> =
        query(&pic, core, controller, "recovery_migration_locked", ()).expect("query");
    assert!(!initial.expect("initial lock"));
    pic.upgrade_canister(
        core,
        pocket_ic_tests::wasm(TRADING_CORE_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .expect("upgrade core");
    let locked: Result<bool, ErrorCode> =
        query(&pic, core, controller, "recovery_migration_locked", ()).expect("query");
    assert!(locked.expect("upgrade lock"));
    let unauthorized: Result<(), ErrorCode> =
        update(&pic, core, principal(86), "finish_recovery_migration", ()).expect("call");
    assert!(matches!(
        unauthorized,
        Err(ErrorCode::Unauthenticated { .. })
    ));
    let swept: Result<u32, ErrorCode> =
        update_args(&pic, vault, controller, "test_sweep_now", ()).expect("sweep call");
    assert_eq!(swept.expect("migration sweep"), 0);
    let unlocked: Result<bool, ErrorCode> =
        query(&pic, core, controller, "recovery_migration_locked", ()).expect("query");
    assert!(!unlocked.expect("migration ack"));
}

#[test]
fn absent_transfer_is_released_only_after_full_history_and_nonce_window() {
    let pic = pic();
    let controller = principal(83);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    enable_recovery_core(&pic, vault, controller);
    let caller = principal(82);
    let session = open_session(&pic, vault, caller, &secret(161));
    credit(&pic, vault, caller, &session, 1_000_000, 83);
    allocate(&pic, vault, caller, &session, b"absent-alloc", 400_000).expect("allocation");
    let sent: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("allocation sweep");
    assert_eq!(sent.expect("allocation sent"), 1);
    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).expect("address");
    let credited: Result<bool, ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "credit_venue_deposit",
        (
            blob(&[84u8; 32]),
            400_000u64,
            trading.expect("trading"),
            "usdc".to_string(),
        ),
    )
    .expect("arrival");
    assert!(credited.expect("arrival credited"));
    let requested: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"absent-recovery"), 400_000u64),
    )
    .expect("request");
    requested.expect("reserved");
    let (unknown, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |call| {
            if call.url.ends_with("/exchange") {
                Err((4, "lost response".to_string()))
            } else {
                recovery_route(call, ACCEPTED)
            }
        })
        .expect("unknown call");
    assert_eq!(unknown.expect("unknown action"), 1);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.ends_with("/exchange"))
            .count(),
        1
    );
    let verified: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_recovery_history_verified",
        true,
    )
    .expect("history gate");
    verified.expect("mock history completeness");
    pic.advance_time(std::time::Duration::from_secs(
        2 * 24 * 60 * 60 + 2 * 60 * 60,
    ));
    let renewed = open_session(&pic, vault, caller, &secret(161));
    let mut released = false;
    for index in 0..110 {
        if index % 5 == 0 {
            pic.advance_time(std::time::Duration::from_secs(61));
        }
        let (swept, calls): (Result<u32, ErrorCode>, _) =
            call_with_routed_outcalls(&pic, vault, caller, "test_sweep_now", (), |call| {
                let request: serde_json::Value =
                    serde_json::from_slice(&call.body).expect("ledger request");
                assert_eq!(request["type"], "userNonFundingLedgerUpdates");
                Ok((200, b"[]".to_vec()))
            })
            .expect("page call");
        assert_eq!(swept.expect("page sweep"), 0);
        assert!(calls.len() <= 1);
        let status: Result<FundStatus, ErrorCode> =
            update(&pic, vault, caller, "get_fund_status", renewed.clone()).expect("status");
        if status.expect("fund status").recovery_fence.is_none() {
            released = true;
            break;
        }
    }
    assert!(
        released,
        "complete mock history releases the fence after the nonce window"
    );
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", renewed).expect("status");
    let status = status.expect("fund status");
    assert_eq!(status.trading_equity, 400_000);
    assert_eq!(status.reserve_unallocated, 600_000);
    assert!(status.unknowns.is_empty());
    let configured: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    let records = records.expect("private recovery events");
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(
                &record.event.payload,
                RecoveryPayload::RecoverySettlement { .. }
            ))
            .count(),
        1
    );
    assert!(records.iter().any(|record| matches!(
        &record.event.payload,
        RecoveryPayload::RecoverySettlement {
            amount_micros: 400_000,
            accepted: false,
            ..
        }
    )));
}

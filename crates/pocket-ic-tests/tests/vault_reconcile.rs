//! 入金の取得（replicated `/info`＋変換）と取り込みの試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{FundStatus, FundingInstructions};
use api_types::journal::{RecoveryPayload, RecoveryRecord};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, deploy, pic, principal, query, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const INFO_BODY: &[u8] =
    br#"[{"time":1758000000000,"hash":"0xaa","delta":{"type":"deposit","usdc":"100.5"},"extra":1}]"#;

fn inbound(body: &[u8], destination: &[u8]) -> Vec<u8> {
    let mut entries: serde_json::Value = serde_json::from_slice(body).unwrap();
    let sender = address_from_secret(&secret(240)).unwrap();
    for entry in entries.as_array_mut().unwrap() {
        if entry["delta"]["type"] == "deposit" {
            entry["delta"]["type"] = "internalTransfer".into();
            entry["delta"]["fee"] = "0".into();
            entry["delta"]["user"] = format!("0x{}", hex::encode(sender)).into();
            entry["delta"]["destination"] = format!("0x{}", hex::encode(destination)).into();
        }
    }
    serde_json::to_vec(&entries).unwrap()
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
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).unwrap();
    let guard = guard.unwrap().expect("configured recovery guard");
    let before_deposits = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .expect("snapshot before deposits");

    // 取得（replicated outcall＋変換）→ 本人へ計上。
    let (first, calls): (Result<u32, ErrorCode>, _) = pocket_ic_tests::call_with_routed_outcalls(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (blob(&address),),
        |call| {
            assert!(call.url.contains("/info"));
            Ok((200, inbound(INFO_BODY, &address)))
        },
    )
    .expect("call");
    assert!(!calls.is_empty());
    for call in calls {
        assert_eq!(
            call.replication,
            pocket_ic::common::rest::CanisterHttpReplication::FullyReplicated,
        );
    }
    assert_eq!(first.expect("reconciled"), 1);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(
        status.expect("status").reserve_unallocated,
        100_500_000,
        "取得した入金を本人へ計上する"
    );
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).expect("journal query");
    let journal = journal.expect("journal configured").expect("journal id");
    let evidence: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    let evidence = evidence.expect("private evidence");
    assert!(evidence.iter().any(|record| matches!(
        &record.event.payload,
        RecoveryPayload::DepositCreditWithFee {
            amount_micros: 100_500_000,
            ..
        }
    )));

    // 同じ入金（同じhash）は二重計上しない。
    let second: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (blob(&address),),
        Ok((200, inbound(INFO_BODY, &address))),
    )
    .expect("call");
    assert_eq!(second.expect("reconciled"), 0);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 100_500_000);
    let after_duplicate: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, vault, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    assert_eq!(
        after_duplicate.expect("private evidence").len(),
        evidence.len()
    );

    // 公式の履歴型と同じ JSON 数値も計上する。指数表記は正確にマイクロUSDCへ変換する。
    let numeric = br#"[
        {"time":1758000000001,"hash":"0xab","delta":{"type":"deposit","usdc":0.000001}},
        {"time":1758000000002,"hash":"0xac","delta":{"type":"deposit","usdc":1e-6}},
        {"time":1758000000003,"hash":"0xad","delta":{"type":"deposit","usdc":0.0000001}},
        {"time":1758000000004,"hash":"0xae","delta":{"type":"deposit","usdc":-1}},
        {"time":1758000000005,"hash":"0xaf","delta":{"type":"deposit","usdc":"0.0000001"}},
        {"time":1758000000006,"hash":"0xb0","delta":{"type":"deposit","usdc":"18446744073709551616"}},
        {"time":1758000000007,"hash":"0xb1","delta":{"type":"withdrawal","usdc":"5"}}
    ]"#;
    let numeric_result: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (blob(&address),),
        Ok((200, inbound(numeric, &address))),
    )
    .expect("numeric call");
    assert_eq!(numeric_result.expect("numeric deposits"), 2);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 100_500_002);

    // A new venue event must not change the ledger while its independent
    // journal is unavailable.
    pic.stop_canister(journal, Some(controller))
        .expect("stop journal");
    let blocked: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (blob(&address),),
        Ok((
            200,
            br#"[{"time":1758000000008,"hash":"0xb2","delta":{"type":"deposit","usdc":"10"}}]"#
                .to_vec(),
        )),
    )
    .expect("blocked call");
    assert!(blocked.is_err(), "journal outage must block credit");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 100_500_002);

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
    pic.start_canister(journal, Some(controller))
        .expect("restart journal");
    // Failed journal writes must leave the inclusive page cursor unchanged.
    let base = 1_758_000_000_100u64;
    let sender = format!(
        "0x{}",
        hex::encode(address_from_secret(&secret(240)).unwrap())
    );
    let destination = format!("0x{}", hex::encode(&address));
    let incoming = |hash: &str, time| {
        serde_json::json!({
            "time": time, "hash": hash,
            "delta": {"type": "internalTransfer", "user": sender, "destination": destination, "usdc": "1", "fee": "0"}
        })
    };
    // 499 unrelated updates followed by an inbound transfer on the page boundary.
    let mut page: Vec<_> = (0..499)
        .map(|i| {
            serde_json::json!({
                "time": base + i, "hash": format!("0x{:064x}", i + 1000),
                "delta": {"type": "withdrawal", "usdc": "1"}
            })
        })
        .collect();
    page.push(incoming("0xc1", base + 499));
    let reconcile = |expected_start: u64, entries: Vec<serde_json::Value>| {
        let (result, _): (Result<u32, ErrorCode>, _) = pocket_ic_tests::call_with_routed_outcalls(
            &pic,
            vault,
            controller,
            "reconcile_deposits",
            (blob(&address),),
            |call| {
                let request: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
                assert_eq!(request["startTime"].as_u64(), Some(expected_start));
                Ok((200, serde_json::to_vec(&entries).unwrap()))
            },
        )
        .unwrap();
        result
    };
    assert_eq!(reconcile(1_758_000_000_007, page).unwrap(), 1);
    // Inclusive boundary includes a duplicate plus another transfer at the SAME timestamp.
    let next = vec![
        incoming("0xc1", base + 499),
        incoming("0xc2", base + 499),
        incoming("0xc3", base + 500),
    ];
    assert_eq!(reconcile(base + 499, next).unwrap(), 2);
    assert_eq!(
        reconcile(base + 500, vec![incoming("0xc3", base + 500)]).unwrap(),
        0
    );
    assert!(reconcile(base + 500, vec![incoming("0xc4", base + 500); 500]).is_err());
    assert_eq!(
        reconcile(base + 500, vec![incoming("0xc4", base + 501)]).unwrap(),
        1
    );
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    assert_eq!(status.unwrap().reserve_unallocated, 104_500_002);
    // The page checkpoint survives a Wasm upgrade independently of the send lock.
    pic.upgrade_canister(
        vault,
        pocket_ic_tests::wasm(FUNDS_VAULT_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .unwrap();
    assert_eq!(reconcile(base + 501, vec![]).unwrap(), 0);
    pic.load_canister_snapshot(vault, Some(controller), before_deposits.id)
        .expect("restore before deposits");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).unwrap();
    assert!(resumed.is_err(), "external validation is still required");
    let restored: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).unwrap();
    assert_eq!(restored.unwrap().reserve_unallocated, 104_500_002);
    let pending: Result<bool, ErrorCode> =
        query(&pic, vault, controller, "recovery_replay_pending", ()).unwrap();
    assert!(pending.unwrap());
}

#[test]
fn activation_fees_credit_net_receipts_and_settle_gross_allocations() {
    let pic = pic();
    let controller = principal(193);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(194);
    let session = open_session(&pic, vault, caller, &secret(240));
    let reserve: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .unwrap();
    let reserve = reserve.unwrap();
    let before_receipts = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .unwrap();
    let guard: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_journal_guard", ()).unwrap();
    let guard = guard.unwrap().unwrap();
    let sender = address_from_secret(&secret(240)).unwrap();
    let receipt = |destination: &[u8],
                   sender: &[u8],
                   hash: &str,
                   gross: &str,
                   fee: serde_json::Value,
                   time: u64| {
        serde_json::to_vec(&serde_json::json!([{"time":time,"hash":hash,"delta":{
            "type":"internalTransfer","user":format!("0x{}",hex::encode(sender)),
            "destination":format!("0x{}",hex::encode(destination)),"usdc":gross,"fee":fee
        }}]))
        .unwrap()
    };
    let history = receipt(
        reserve.as_ref(),
        &sender,
        "0xfa01",
        "9",
        "1".into(),
        1758000000000,
    );
    for expected in [1, 0] {
        let result: Result<u32, ErrorCode> = call_with_mocked_outcall(
            &pic,
            vault,
            controller,
            "reconcile_deposits",
            (reserve.clone(),),
            Ok((200, history.clone())),
        )
        .unwrap();
        assert_eq!(result.unwrap(), expected);
    }
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    assert_eq!(status.unwrap().reserve_unallocated, 8_000_000);
    // Fail closed without advancing the cursor when fee evidence is missing or invalid.
    for fee in [
        serde_json::Value::Null,
        "-1".into(),
        "9".into(),
        "10".into(),
        "0.0000001".into(),
    ] {
        let result: Result<u32, ErrorCode> = call_with_mocked_outcall(
            &pic,
            vault,
            controller,
            "reconcile_deposits",
            (reserve.clone(),),
            Ok((
                200,
                receipt(reserve.as_ref(), &sender, "0xfa02", "9", fee, 1758000000001),
            )),
        )
        .unwrap();
        assert!(matches!(result, Err(ErrorCode::UpstreamRejected { .. })));
    }
    let accepted: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        api_types::fund::AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"fee-allocation"),
            amount: 5_000_000,
            target: api_types::AccountKind::Trading,
            intent_signature: None,
        },
    )
    .unwrap();
    accepted.unwrap();
    let sent: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((
            200,
            br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
        )),
    )
    .unwrap();
    assert_eq!(sent.unwrap(), 1);
    let before_allocation_receipt = pic
        .take_canister_snapshot(vault, Some(controller), None)
        .unwrap();
    let trading: Result<Blob, ErrorCode> =
        update(&pic, vault, caller, "get_trading_address", session.clone()).unwrap();
    let trading = trading.unwrap();
    let allocation_history = receipt(
        trading.as_ref(),
        reserve.as_ref(),
        "0xfa03",
        "5",
        "1".into(),
        1758000000002,
    );
    for expected in [1, 0] {
        let result: Result<u32, ErrorCode> = call_with_mocked_outcall(
            &pic,
            vault,
            controller,
            "reconcile_deposits",
            (trading.clone(),),
            Ok((200, allocation_history.clone())),
        )
        .unwrap();
        assert_eq!(result.unwrap(), expected);
    }
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    let status = status.unwrap();
    assert_eq!(status.reserve_unallocated, 3_000_000);
    assert_eq!(status.trading_equity, 4_000_000);
    assert_eq!(status.in_transit, 0);
    let events: Result<api_types::Paged<api_types::fund::FundEvent>, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "list_fund_events",
        (session.clone(), None::<Blob>, 10u32),
    )
    .unwrap();
    assert_eq!(
        events.unwrap().items[0].state,
        api_types::fund::FundRequestState::Settled
    );
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).unwrap();
    let evidence: Result<Vec<RecoveryRecord>, ErrorCode> = update_args(
        &pic,
        journal.unwrap().unwrap(),
        vault,
        "recovery_events",
        (0u64, 100u32),
    )
    .unwrap();
    assert_eq!(
        evidence
            .unwrap()
            .iter()
            .filter(|r| matches!(
                &r.event.payload,
                RecoveryPayload::DepositCreditWithFee {
                    fee_micros: 1_000_000,
                    ..
                }
            ))
            .count(),
        2
    );
    pic.load_canister_snapshot(vault, Some(controller), before_allocation_receipt.id)
        .unwrap();
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).unwrap();
    assert!(
        resumed.is_err(),
        "live evidence validation remains required"
    );
    let restored: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    let restored = restored.unwrap();
    assert_eq!(restored.reserve_unallocated, 3_000_000);
    assert_eq!(restored.trading_equity, 4_000_000);
    assert_eq!(restored.in_transit, 0);
    pic.load_canister_snapshot(vault, Some(controller), before_receipts.id)
        .unwrap();
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, principal(239), "resume_journal", vault).unwrap();
    assert!(
        resumed.is_err(),
        "old snapshot cannot promote an unverified send to a receipt"
    );
    let restored: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    assert_eq!(restored.unwrap().reserve_unallocated, 8_000_000);
}

#[test]
fn spot_receipt_converts_once_and_late_login_claims_only_its_sender() {
    let pic = pic();
    let controller = principal(201);
    let caller = principal(202);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let wrong_session = open_session(&pic, vault, caller, &secret(240));
    let reserve: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        wrong_session.clone(),
    )
    .unwrap();
    let reserve = reserve.unwrap();
    let sender = address_from_secret(&secret(238)).unwrap();
    let timestamp = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000).saturating_sub(1);
    let incoming = serde_json::json!({"time":timestamp,"hash":format!("0x{}",hex::encode([211;32])),"delta":{"type":"send","token":"USDC","amount":"10","fee":"1","sourceDex":"spot","destinationDex":"spot","user":format!("0x{}",hex::encode(sender)),"destination":format!("0x{}",hex::encode(reserve.as_ref()))}});
    // Fail after durable preparation but before any dispatch. The same receipt
    // must resume with its original nonce once the key configuration is repaired.
    let configured: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_ecdsa_key_id",
        "missing_test_key".to_string(),
    )
    .unwrap();
    configured.unwrap();
    let (failed, _): (Result<u32, ErrorCode>, _) = pocket_ic_tests::call_with_routed_outcalls(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (reserve.clone(),),
        |call| {
            assert!(
                !call.url.ends_with("/exchange"),
                "preparation must not dispatch"
            );
            let body: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            Ok((
                200,
                if body["type"] == "spotClearinghouseState" {
                    br#"{"balances":[{"coin":"USDC","token":0,"total":"10","hold":"0"}]}"#.to_vec()
                } else {
                    serde_json::to_vec(&vec![incoming.clone()]).unwrap()
                },
            ))
        },
    )
    .unwrap();
    assert!(failed.is_err());
    let configured: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_ecdsa_key_id",
        "test_key_1".to_string(),
    )
    .unwrap();
    configured.unwrap();
    let posts = std::cell::Cell::new(0);
    let nonce = std::cell::Cell::new(0);
    let class_receipts = std::cell::RefCell::new(Vec::<serde_json::Value>::new());
    for (iteration, expected) in [1, 0, 1].into_iter().enumerate() {
        let mut incoming = incoming.clone();
        if iteration == 2 {
            incoming["hash"] = format!("0x{}", hex::encode([213; 32])).into();
        }
        let(result,calls):(Result<u32,ErrorCode>,_)=pocket_ic_tests::call_with_routed_outcalls(&pic,vault,controller,"reconcile_deposits",(reserve.clone(),),|call|{
            let body:serde_json::Value=serde_json::from_slice(&call.body).unwrap();
            if call.url.ends_with("/exchange") {
                posts.set(posts.get()+1);nonce.set(body["nonce"].as_u64().unwrap());
                class_receipts.borrow_mut().push(serde_json::json!({"time":nonce.get(),"hash":format!("0x{}",hex::encode([212+posts.get() as u8;32])),"delta":{"type":"accountClassTransfer","usdc":"10","toPerp":true}}));
                assert_eq!(body["action"]["type"],"usdClassTransfer");assert_eq!(body["action"]["amount"],"10");assert_eq!(body["action"]["toPerp"],true);
                assert_eq!(call.replication,pocket_ic::common::rest::CanisterHttpReplication::NonReplicated);
                return Ok((200,br#"{"status":"ok","response":{"type":"default"}}"#.to_vec()));
            }
            assert_eq!(call.replication,pocket_ic::common::rest::CanisterHttpReplication::FullyReplicated);
            match body["type"].as_str().unwrap() {
                "spotClearinghouseState"=>Ok((200,br#"{"balances":[{"coin":"USDC","token":0,"total":"10","hold":"0"}]}"#.to_vec())),
                "userNonFundingLedgerUpdates" if body.get("endTime").is_some()=>Ok((200,serde_json::to_vec(&*class_receipts.borrow()).unwrap())),
                "userNonFundingLedgerUpdates"=>Ok((200,serde_json::to_vec(&vec![incoming.clone()]).unwrap())),
                other=>panic!("unexpected info {other}"),
            }
        }).unwrap();
        assert_eq!(result.unwrap(), expected);
        assert!(!calls.is_empty());
    }
    assert_eq!(posts.get(), 2);
    let wrong: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", wrong_session).unwrap();
    assert_eq!(wrong.unwrap().reserve_unallocated, 0);
    for _ in 0..2 {
        let session = open_session(&pic, vault, principal(203), &secret(238));
        let funds: Result<FundStatus, ErrorCode> =
            update(&pic, vault, principal(203), "get_fund_status", session).unwrap();
        assert_eq!(funds.unwrap().reserve_unallocated, 20_000_000);
    }
}

#[test]
fn lost_spot_conversion_response_reconciles_after_upgrade_without_reposting() {
    let pic = pic();
    let controller = principal(204);
    let caller = principal(205);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let session = open_session(&pic, vault, caller, &secret(240));
    let reserve: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .unwrap();
    let reserve = reserve.unwrap();
    let sender = address_from_secret(&secret(240)).unwrap();
    let timestamp = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000).saturating_sub(1);
    let incoming = serde_json::json!({"time":timestamp,"hash":format!("0x{}",hex::encode([215;32])),"delta":{"type":"send","token":"USDC","amount":"10","destinationDex":"spot","user":format!("0x{}",hex::encode(sender)),"destination":format!("0x{}",hex::encode(reserve.as_ref()))}});
    let nonce = std::cell::Cell::new(0);
    let posts = std::cell::Cell::new(0);
    let (first, _): (Result<u32, ErrorCode>, _) = pocket_ic_tests::call_with_routed_outcalls(
        &pic,
        vault,
        controller,
        "reconcile_deposits",
        (reserve.clone(),),
        |call| {
            let b: serde_json::Value = serde_json::from_slice(&call.body).unwrap();
            if call.url.ends_with("/exchange") {
                posts.set(posts.get() + 1);
                nonce.set(b["nonce"].as_u64().unwrap());
                return Ok((503, b"unknown".to_vec()));
            }
            if b["type"] == "spotClearinghouseState" {
                return Ok((
                    200,
                    br#"{"balances":[{"coin":"USDC","token":0,"total":"10","hold":"0"}]}"#.to_vec(),
                ));
            }
            Ok((
                200,
                if b.get("endTime").is_some() {
                    b"[]".to_vec()
                } else {
                    serde_json::to_vec(&vec![incoming.clone()]).unwrap()
                },
            ))
        },
    )
    .unwrap();
    assert!(first.is_err());
    let funds: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    let funds = funds.unwrap();
    assert_eq!(funds.reserve_unallocated, 0);
    assert_eq!(funds.unknowns.len(), 1);
    pic.upgrade_canister(
        vault,
        pocket_ic_tests::wasm(FUNDS_VAULT_WASM),
        candid::encode_one(()).unwrap(),
        Some(controller),
    )
    .unwrap();
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
    resumed.unwrap();
    let (second,_):(Result<u32,ErrorCode>,_)=pocket_ic_tests::call_with_routed_outcalls(&pic,vault,controller,"reconcile_deposits",(reserve,),|call|{
        assert!(!call.url.ends_with("/exchange"),"must not retry a POST after uncertain response or upgrade");
        let b:serde_json::Value=serde_json::from_slice(&call.body).unwrap();
        Ok((200,if b.get("endTime").is_some(){serde_json::to_vec(&serde_json::json!([{"time":nonce.get(),"hash":format!("0x{}",hex::encode([216;32])),"delta":{"type":"accountClassTransfer","usdc":"10","toPerp":true}}])).unwrap()}else{serde_json::to_vec(&vec![incoming.clone()]).unwrap()}))
    }).unwrap();
    assert_eq!(second.unwrap(), 1);
    assert_eq!(posts.get(), 1);
    let funds: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).unwrap();
    let funds = funds.unwrap();
    assert_eq!(funds.reserve_unallocated, 10_000_000);
    assert!(funds.unknowns.is_empty());
}

#[test]
fn invalid_spot_receipts_balances_and_ambiguous_conversions_never_credit() {
    for (amount, available, ambiguous, expected_posts) in [
        ("-10", "10", false, 0),
        ("10", "9", false, 0),
        ("10", "10", true, 1),
    ] {
        let pic = pic();
        let controller = principal(206);
        let caller = principal(207);
        let vault = deploy(
            &pic,
            FUNDS_VAULT_WASM,
            Some(vec![controller]),
            candid::encode_one(()).unwrap(),
        );
        let session = open_session(&pic, vault, caller, &secret(240));
        let reserve: Result<Blob, ErrorCode> = update(
            &pic,
            vault,
            caller,
            "provision_reserve_account",
            session.clone(),
        )
        .unwrap();
        let reserve = reserve.unwrap();
        let sender = address_from_secret(&secret(240)).unwrap();
        let time = (pic.get_time().as_nanos_since_unix_epoch() / 1_000_000).saturating_sub(1);
        let incoming = serde_json::json!({"time":time,"hash":format!("0x{}",hex::encode([218;32])),"delta":{"type":"send","token":"USDC","amount":amount,"destinationDex":"spot","user":format!("0x{}",hex::encode(sender)),"destination":format!("0x{}",hex::encode(reserve.as_ref()))}});
        let nonce = std::cell::Cell::new(0);
        let posts = std::cell::Cell::new(0);
        let (result,_): (Result<u32, ErrorCode>, _) = pocket_ic_tests::call_with_routed_outcalls(&pic,vault,controller,"reconcile_deposits",(reserve,),|call| {
            let body:serde_json::Value=serde_json::from_slice(&call.body).unwrap();
            if call.url.ends_with("/exchange") {
                posts.set(posts.get()+1);
                nonce.set(body["nonce"].as_u64().unwrap());
                return Ok((200, br#"{"status":"ok","response":{"type":"default"}}"#.to_vec()));
            }
            if body["type"]=="spotClearinghouseState" {
                return Ok((200,serde_json::to_vec(&serde_json::json!({"balances":[{"coin":"USDC","token":0,"total":available,"hold":"0"}]})).unwrap()));
            }
            if body.get("endTime").is_some() {
                assert!(ambiguous);
                return Ok((200,serde_json::to_vec(&(0..2).map(|i|serde_json::json!({"time":nonce.get(),"hash":format!("0x{}",hex::encode([219+i;32])),"delta":{"type":"accountClassTransfer","usdc":"10","toPerp":true}})).collect::<Vec<_>>()).unwrap()));
            }
            Ok((200,serde_json::to_vec(&vec![incoming.clone()]).unwrap()))
        }).unwrap();
        assert!(result.is_err());
        assert_eq!(posts.get(), expected_posts);
        let funds: Result<FundStatus, ErrorCode> =
            update(&pic, vault, caller, "get_fund_status", session).unwrap();
        assert_eq!(funds.unwrap().reserve_unallocated, 0);
    }
}

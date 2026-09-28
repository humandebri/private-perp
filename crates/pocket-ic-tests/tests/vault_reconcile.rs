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
        RecoveryPayload::DepositCredit {
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
            "delta": {"type": "internalTransfer", "user": sender, "destination": destination, "usdc": "1"}
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

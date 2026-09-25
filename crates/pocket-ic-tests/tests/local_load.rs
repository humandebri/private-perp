//! Fixed-seed synthetic vault load. Host latency is a local PocketIC measurement.

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use api_types::fund::{AllocationRequest, FundRequestAccepted, FundStatus};
use api_types::journal::{JournalHead, RecoveryEvent, RecoveryPayload};
use api_types::operations::RestBudgetStatus;
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_routed_outcalls, deploy, pic, principal, query, update, update_args,
};
use std::time::Instant;

fn session(
    pic: &pocket_ic::PocketIc,
    vault: Principal,
    caller: Principal,
    seed: u8,
) -> SessionHandle {
    let mut key = [0; 32];
    key[31] = seed;
    let eoa = address_from_secret(&key).unwrap();
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
            origin: "https://load.example.test".into(),
        },
    )
    .unwrap();
    let issued = issued.unwrap();
    let challenge = private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".into(),
        origin: "https://load.example.test".into(),
        nonce: issued.nonce.as_ref().try_into().unwrap(),
        expires_at: issued.expires_at,
    };
    let opened: Result<SessionHandle, ErrorCode> = update(
        pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: issued.challenge_id,
            eoa_signature: challenge
                .sign_for_tests(&key)
                .unwrap()
                .to_bytes65()
                .to_vec()
                .into(),
        },
    )
    .unwrap();
    let opened = opened.unwrap();
    pocket_ic_tests::activate_local_user(pic, vault, caller, &opened);
    opened
}

fn percentile(mut values: Vec<u128>, percentile: usize) -> u128 {
    values.sort_unstable();
    values[(values.len() * percentile).div_ceil(100).saturating_sub(1)]
}

fn run(users: usize) {
    let pic = pic();
    let controller = principal(200);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let policy: Option<Principal> =
        query(&pic, vault, controller, "get_policy_principal", ()).unwrap();
    let policy = policy.unwrap();
    let journal: Result<Option<Principal>, ErrorCode> =
        query(&pic, vault, controller, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    let balances_before = [
        pic.cycle_balance(vault),
        pic.cycle_balance(policy),
        pic.cycle_balance(journal),
    ];
    let mut accepted_ms = Vec::new();
    let mut failure_count = 0;
    let mut first_session = None;
    for index in 0..users {
        let caller = principal(20 + index as u8);
        let session = session(&pic, vault, caller, 20 + index as u8);
        if index == 0 {
            first_session = Some((caller, session.clone()));
        }
        let provisioned: Result<Blob, ErrorCode> = update(
            &pic,
            vault,
            caller,
            "provision_reserve_account",
            session.clone(),
        )
        .unwrap();
        assert_eq!(provisioned.unwrap().len(), 20);
        let credited: Result<(), ErrorCode> = update_args(
            &pic,
            vault,
            caller,
            "test_credit_deposit",
            (
                session.clone(),
                2_000_000u64,
                Blob::from(vec![index as u8 + 1; 32]),
            ),
        )
        .unwrap();
        credited.unwrap();
        let request_id = format!("load-fixed-seed-20260924-{index:03}");
        let start = Instant::now();
        let accepted: Result<FundRequestAccepted, ErrorCode> = update(
            &pic,
            vault,
            caller,
            "request_allocation",
            AllocationRequest {
                session,
                client_request_id: request_id.into_bytes().into(),
                amount: 1_000_000,
                target: AccountKind::Trading,
                intent_signature: None,
            },
        )
        .unwrap();
        accepted_ms.push(start.elapsed().as_millis());
        if accepted.is_err() {
            failure_count += 1;
        }
    }
    assert_eq!(failure_count, 0);
    let mut posts = 0;
    for _ in 0..users.div_ceil(4) {
        let swept: Result<u32, ErrorCode> =
            call_with_routed_outcalls(&pic, vault, controller, "test_sweep_now", (), |call| {
                assert!(call.url.ends_with("/exchange"));
                Ok((
                    200,
                    br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
                ))
            })
            .unwrap()
            .0;
        posts += swept.unwrap() as usize;
    }
    assert_eq!(posts, users, "one POST per accepted allocation");
    let budget: Result<RestBudgetStatus, ErrorCode> =
        query(&pic, policy, controller, "get_rest_budget_status", ()).unwrap();
    let budget = budget.unwrap();
    let head: Result<JournalHead, ErrorCode> = update(&pic, journal, vault, "head", ()).unwrap();
    assert_eq!(head.unwrap().sequence as usize, posts);
    assert_eq!(budget.used as usize, posts);
    let balances_after = [
        pic.cycle_balance(vault),
        pic.cycle_balance(policy),
        pic.cycle_balance(journal),
    ];
    let consumed: Vec<u128> = balances_before
        .iter()
        .zip(balances_after)
        .map(|(before, after)| before.saturating_sub(after))
        .collect();
    eprintln!(
        "LOCAL_LOAD {{\"seed\":20260924,\"users\":{users},\"rest_weight\":{},\"posts\":{posts},\"accept_p95_host_ms\":{},\"cycles_vault\":{},\"cycles_policy\":{},\"cycles_journal\":{},\"failures\":{failure_count}}}",
        budget.used,
        percentile(accepted_ms, 95),
        consumed[0],
        consumed[1],
        consumed[2]
    );

    // An unreplayed V2 business event must stop new acceptance and HL POST
    // even when the V1 send-intent head still matches local receipts.
    let before: Result<JournalHead, ErrorCode> =
        update(&pic, journal, vault, "recovery_head", ()).unwrap();
    let before = before.unwrap();
    let appended: Result<JournalHead, ErrorCode> = update(
        &pic,
        journal,
        vault,
        "append_recovery_event",
        RecoveryEvent {
            version: 1,
            logical_id: vec![0x7a; 32].into(),
            payload: RecoveryPayload::Reservation {
                request_id: vec![0x7b; 32].into(),
                user_id: vec![0x7c; 32].into(),
                account_id: vec![0x7d; 32].into(),
                amount_micros: 1_000_000,
                state: "reserved".into(),
            },
        },
    )
    .unwrap();
    assert_eq!(appended.unwrap().sequence, before.sequence + 1);
    let (caller, session) = first_session.unwrap();
    let status_before: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
    let accepted: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: b"unreplayed-event-must-stop-post".to_vec().into(),
            amount: 1_000_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .unwrap();
    assert!(matches!(accepted, Err(ErrorCode::PolicyUnavailable)));
    let status_after: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).unwrap();
    assert_eq!(
        status_after.unwrap().withdrawable,
        status_before.unwrap().withdrawable,
        "a rejected request must not reserve funds"
    );
    let (_, calls): (Result<u32, ErrorCode>, _) =
        call_with_routed_outcalls(&pic, vault, controller, "test_sweep_now", (), |call| {
            panic!("unreplayed event reached HL: {}", call.url)
        })
        .unwrap();
    assert!(calls.is_empty());
}

#[test]
fn twenty_users() {
    run(20);
}

#[test]
fn hundred_users() {
    run(100);
}

//! 配分の着金から回収までの往復の試験。

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
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, call_with_routed_outcalls, deploy, pic, principal,
    update, update_args,
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
    session.expect("session")
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

    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(status.trading_equity, 0, "回収で取引口座が空になる");
    assert_eq!(status.reserve_unallocated, 1_000_000, "準備口座へ戻る");

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

    // 取引所が拒否した場合は拘束が戻り、再要求できる。
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "test_sweep_now",
        (),
        Ok((200, REJECTED.to_vec())),
    )
    .expect("call");
    assert_eq!(swept.expect("recovery sweep"), 1);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").trading_equity, 400_000);
    let retry: Result<FundRequestAccepted, ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "request_recovery",
        (session.clone(), blob(b"recv-retry"), 400_000u64),
    )
    .expect("call");
    retry.expect("拒否後は拘束が解けて再要求できる");
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

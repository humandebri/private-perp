//! 取引所入金の取り込み（宛先写像・本人計上・二重計上防止）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
    SessionStatus,
};
use api_types::error::ErrorCode;
use api_types::fund::{FundStatus, FundingInstructions};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, pic, principal, query, update, update_args};

const ORIGIN: &str = "https://app.example.test";

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

fn credit(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    tx: &[u8],
    amount: u64,
    address: &Blob,
    sender: Option<Blob>,
) -> Result<bool, ErrorCode> {
    update_args(
        pic,
        vault,
        caller,
        "credit_venue_deposit",
        (
            blob(tx),
            amount,
            address.clone(),
            "usdc".to_string(),
            sender,
        ),
    )
    .expect("call")
}

#[test]
fn venue_deposits_credit_the_owner_once() {
    let pic = pic();
    let controller = principal(170);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let caller = principal(171);
    let session = open_session(&pic, vault, caller, &secret(220));

    // 入金先（準備口座）を用意すると入金案内が得られる。
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    let provisioned = provisioned.expect("provisioned");
    let instructions: Result<FundingInstructions, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "get_funding_instructions",
        session.clone(),
    )
    .expect("call");
    let address = instructions.expect("instructions").hl_account_address;
    assert_eq!(address.as_ref(), provisioned.as_slice());

    // 本人の入金先宛なら計上される。
    let first = credit(
        &pic,
        vault,
        controller,
        &[7u8; 32],
        1_000_000,
        &address,
        Some(blob(&address_from_secret(&secret(220)).unwrap())),
    );
    assert!(first.expect("credited"), "新規の入金を計上する");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(
        status.expect("status").reserve_unallocated,
        1_000_000,
        "本人の未配分残高へ計上される"
    );

    // 同じtx_hashは二重計上しない。
    let again = credit(
        &pic,
        vault,
        controller,
        &[7u8; 32],
        1_000_000,
        &address,
        Some(blob(&address_from_secret(&secret(220)).unwrap())),
    );
    assert!(!again.expect("deduped"), "同じtx_hashは計上しない");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 1_000_000);

    // 未知の宛先は記録のみ。
    let unknown = credit(
        &pic,
        vault,
        controller,
        &[8u8; 32],
        500_000,
        &blob(&[9u8; 20]),
        None,
    );
    assert!(unknown.expect("recorded"));
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 1_000_000);

    // 非controllerは取り込めない。
    let denied = credit(&pic, vault, principal(172), &[10u8; 32], 1, &address, None);
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

#[test]
fn shared_reserve_attributes_equal_deposits_by_sender_and_excludes_recoveries() {
    let pic = pic();
    let controller = principal(173);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let alice = principal(174);
    let bob = principal(175);
    let alice_session = open_session(&pic, vault, alice, &secret(231));
    let bob_session = open_session(&pic, vault, bob, &secret(232));
    let provision = |caller, session: &SessionHandle| {
        let result: Result<Vec<u8>, ErrorCode> = update(
            &pic,
            vault,
            caller,
            "provision_reserve_account",
            session.clone(),
        )
        .unwrap();
        blob(&result.unwrap())
    };
    let reserve = provision(alice, &alice_session);
    assert_eq!(reserve, provision(bob, &bob_session));
    let balance = |caller, session: &SessionHandle| {
        let result: Result<FundStatus, ErrorCode> =
            update(&pic, vault, caller, "get_fund_status", session.clone()).unwrap();
        result.unwrap().reserve_unallocated
    };
    let from_alice = blob(&address_from_secret(&secret(231)).unwrap());
    let from_bob = blob(&address_from_secret(&secret(232)).unwrap());
    assert!(
        credit(
            &pic,
            vault,
            controller,
            &[31; 32],
            1_000_000,
            &reserve,
            Some(from_alice.clone())
        )
        .unwrap()
    );
    assert_eq!(balance(alice, &alice_session), 1_000_000);
    assert_eq!(balance(bob, &bob_session), 0);
    assert!(
        credit(
            &pic,
            vault,
            controller,
            &[32; 32],
            1_000_000,
            &reserve,
            Some(from_bob.clone())
        )
        .unwrap()
    );
    assert_eq!(balance(bob, &bob_session), 1_000_000);
    // Replaying the same public hash with a different claimed sender cannot steal a deposit.
    assert!(
        !credit(
            &pic,
            vault,
            controller,
            &[31; 32],
            1_000_000,
            &reserve,
            Some(from_bob)
        )
        .unwrap()
    );
    // Bridge/CEX deposits without authenticated source evidence remain in suspense.
    assert!(
        credit(
            &pic, vault, controller, &[33; 32], 2_000_000, &reserve, None
        )
        .unwrap()
    );
    assert!(
        credit(
            &pic,
            vault,
            controller,
            &[34; 32],
            2_000_000,
            &reserve,
            Some(blob(&[9; 20]))
        )
        .unwrap()
    );
    let prepared: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        alice,
        "prepare_trading_account",
        alice_session.clone(),
    )
    .unwrap();
    prepared.unwrap();
    let trading: Result<Blob, ErrorCode> = update(
        &pic,
        vault,
        alice,
        "get_trading_address",
        alice_session.clone(),
    )
    .unwrap();
    assert!(
        !credit(
            &pic,
            vault,
            controller,
            &[35; 32],
            2_000_000,
            &reserve,
            Some(trading.unwrap())
        )
        .unwrap()
    );
    assert_eq!(balance(alice, &alice_session), 1_000_000);
    assert_eq!(balance(bob, &bob_session), 1_000_000);
}

/// 未知宛先の入金は未帰属のまま保持し、controllerも手動で割り当てられない。
#[test]
fn an_unmatched_deposit_cannot_be_assigned_by_the_controller() {
    let pic = pic();
    let controller = principal(180);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(181);
    let session = open_session(&pic, vault, caller, &secret(230));

    let session_status: Result<SessionStatus, ErrorCode> =
        query(&pic, vault, caller, "session_status", session.clone()).expect("call");
    let user_id = session_status.expect("status").user_id;

    // 未知の宛先への入金（500,000マイクロUSDC）。
    let tx_hash = [21u8; 32];
    let unknown_address = blob(&[9u8; 20]);
    let recorded = credit(
        &pic,
        vault,
        controller,
        &tx_hash,
        500_000,
        &unknown_address,
        None,
    );
    assert!(recorded.expect("recorded"), "未知宛先でも取り込む");

    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(
        status.reserve_unallocated, 0,
        "未知宛先の入金を本人へ与信しない（suspenseに留める）"
    );

    let mut input = b"deposit".to_vec();
    input.extend_from_slice(&tx_hash);
    let event_id = hl_sign::keccak256(&input);
    for sender in [controller, caller] {
        let result: Result<Result<(), ErrorCode>, String> = update_args(
            &pic,
            vault,
            sender,
            "claim_unmatched_deposit",
            (blob(&event_id), user_id.clone()),
        );
        let error = result.expect_err("manual assignment endpoint must not exist");
        assert!(error.contains("has no update method"), "{error}");
    }
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session).expect("call");
    assert_eq!(status.expect("status").reserve_unallocated, 0);
}

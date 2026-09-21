//! 資金API（受付・予約・冪等性・出金intent）の失敗試験。
//!
//! `test-venue` feature付きのwasm（`scripts/pocket-ic-test.sh`がビルド）を使い、
//! テスト専用の入金計上で残高を作ってから配分・出金を検証する。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::fund::{
    AllocationRequest, Destination, FundRequestAccepted, FundRequestState, FundStatus,
    WithdrawalRequest,
};
use api_types::{AccountKind, AssetId, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy_default, pic, principal, update, update_args};

const ORIGIN: &str = "https://app.example.test";

fn secret(seed: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes
}

fn request_id(value: &[u8]) -> Blob {
    value.to_vec().into()
}

/// challengeを発行して署名し、セッションを開く（クライアント役）。
fn open_session(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    secret_key: &[u8; 32],
) -> (SessionHandle, [u8; 20]) {
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
    .expect("issue_challenge call");
    let issued = issued.expect("issue_challenge");

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
    .expect("open_session call");
    (session.expect("session"), eoa)
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
        (session.clone(), amount, request_id(&[seed; 32])),
    )
    .expect("test_credit_deposit call");
    outcome.expect("credit");
}

fn allocation(
    pic: &PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    client_request_id: &[u8],
    amount: u64,
) -> Result<FundRequestAccepted, ErrorCode> {
    update(
        pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: request_id(client_request_id),
            amount,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("request_allocation call")
}

#[test]
fn allocation_without_funds_is_rejected() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(20);
    let key = secret(201);
    let (session, _eoa) = open_session(&pic, vault, caller, &key);

    let error = allocation(&pic, vault, caller, &session, b"alloc-1", 1_000_000)
        .expect_err("must reject without funds");
    assert!(
        matches!(error, ErrorCode::InsufficientFunds { .. }),
        "{error:?}"
    );
}

#[test]
fn allocation_is_idempotent_and_bounded_by_the_balance() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(21);
    let key = secret(202);
    let (session, _eoa) = open_session(&pic, vault, caller, &key);

    credit(&pic, vault, caller, &session, 1_000_000, 1);

    let first = allocation(&pic, vault, caller, &session, b"alloc-2", 400_000).expect("accepted");
    assert_eq!(first.state, FundRequestState::Reserved);
    assert!(first.fund_action_id.is_none());

    // 同一ID・同一本文の再送は既存結果を返す（二重予約しない）。
    let again = allocation(&pic, vault, caller, &session, b"alloc-2", 400_000).expect("duplicate");
    assert_eq!(again.request_id, first.request_id);

    // 同一ID・異なる本文は拒否する。
    let conflict = allocation(&pic, vault, caller, &session, b"alloc-2", 500_000)
        .expect_err("must reject a different body");
    assert!(
        matches!(conflict, ErrorCode::IdempotencyConflict { .. }),
        "{conflict:?}"
    );

    // 予約は出金可能額から差し引かれる。
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("status call");
    let status = status.expect("status");
    assert_eq!(status.reserve_unallocated, 1_000_000);
    assert_eq!(status.withdrawable, 600_000);

    // 残高を超える配分は拒否する。
    let error = allocation(&pic, vault, caller, &session, b"alloc-3", 700_000)
        .expect_err("must reject over the balance");
    assert!(
        matches!(error, ErrorCode::InsufficientFunds { .. }),
        "{error:?}"
    );
}

#[test]
fn withdrawal_requires_a_valid_intent_signature() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(22);
    let key = secret(203);
    let (session, eoa) = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 2);

    let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000;
    let intent = private_perp::Withdrawal {
        eoa,
        amount: 250_000,
        asset: "usdc".to_string(),
        destination: format!("0x{}", hex::encode(eoa)),
        network: "local".to_string(),
        nonce: 1,
        expires_at: now + 600_000,
        canister: vault.as_slice().to_vec(),
    };

    let signed = |intent: &private_perp::Withdrawal, key: &[u8; 32]| WithdrawalRequest {
        session: session.clone(),
        client_request_id: request_id(b"wd-1"),
        amount: intent.amount,
        asset: AssetId::Usdc,
        destination: Destination::AuthenticatedEoaHlAccount,
        network: Network::Local,
        nonce: intent.nonce,
        expires_at: intent.expires_at,
        intent_signature: intent
            .sign_for_tests(key)
            .expect("sign")
            .to_bytes65()
            .to_vec()
            .into(),
    };

    // 別の鍵で署名したintentは拒否する。
    let wrong: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        signed(&intent, &secret(204)),
    )
    .expect("call");
    let wrong = wrong.expect_err("must reject another key");
    assert!(
        matches!(
            wrong,
            ErrorCode::BadRequest {
                code: BadRequestCode::InvalidSignature,
                ..
            }
        ),
        "{wrong:?}"
    );

    // 正しい署名でも、サーバが束縛する宛先（本人EOA）と異なるintentは拒否する。
    let mut other_destination = intent.clone();
    other_destination.destination = format!("0x{}", hex::encode([9u8; 20]));
    let mismatch: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        signed(&other_destination, &key),
    )
    .expect("call");
    assert!(mismatch.is_err(), "must reject a mismatched destination");

    // 正しい署名は受付けて予約する。
    let accepted: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        signed(&intent, &key),
    )
    .expect("call");
    let accepted = accepted.expect("accepted");
    assert_eq!(accepted.state, FundRequestState::Reserved);

    // 期限切れintentは拒否する。
    let mut expired = intent.clone();
    expired.expires_at = now.saturating_sub(1);
    let expired_result: Result<FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_withdrawal",
        signed(&expired, &key),
    )
    .expect("call");
    let expired_error = expired_result.expect_err("must reject an expired intent");
    assert!(
        matches!(
            expired_error,
            ErrorCode::BadRequest {
                code: BadRequestCode::ExpiredIntent,
                ..
            }
        ),
        "{expired_error:?}"
    );
}

#[test]
fn a_withdrawal_reservation_over_half_the_balance_keeps_reads_working() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(25);
    let key = secret(206);
    let (session, eoa) = open_session(&pic, vault, caller, &key);
    credit(&pic, vault, caller, &session, 1_000_000, 3);

    let now = pic.get_time().as_nanos_since_unix_epoch() / 1_000_000;
    let withdraw =
        |amount: u64, nonce: u64, request: &[u8]| -> Result<FundRequestAccepted, ErrorCode> {
            let intent = private_perp::Withdrawal {
                eoa,
                amount,
                asset: "usdc".to_string(),
                destination: format!("0x{}", hex::encode(eoa)),
                network: "local".to_string(),
                nonce,
                expires_at: now + 600_000,
                canister: vault.as_slice().to_vec(),
            };
            update(
                &pic,
                vault,
                caller,
                "request_withdrawal",
                WithdrawalRequest {
                    session: session.clone(),
                    client_request_id: request_id(request),
                    amount,
                    asset: AssetId::Usdc,
                    destination: Destination::AuthenticatedEoaHlAccount,
                    network: Network::Local,
                    nonce,
                    expires_at: intent.expires_at,
                    intent_signature: intent
                        .sign_for_tests(&key)
                        .expect("sign")
                        .to_bytes65()
                        .to_vec()
                        .into(),
                },
            )
            .expect("call")
        };

    // 残高の6割を出金予約する（未配分から出金予約へ移る）。
    withdraw(600_000, 1, b"wd-big").expect("accepted");

    // 以前は withdrawable の計算が「予約を二重に引く」ため、ここで
    // BadRequest(MalformedPayload "withdrawal reservation exceeds the balance") になった。
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("出金予約後も資金状態を読める");
    assert_eq!(status.reserve_unallocated, 400_000);
    assert_eq!(status.reserved_for_withdrawal, 600_000);
    assert_eq!(
        status.withdrawable, 400_000,
        "出金予約は台帳側で拘束済み（二重に引かない）"
    );

    // 残りの全額は配分に出せる（出金予約は台帳、配分は予約表で別々に数える）。
    allocation(&pic, vault, caller, &session, b"alloc-after-wd", 400_000).expect("accepted");
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, caller, "get_fund_status", session.clone()).expect("call");
    let status = status.expect("status");
    assert_eq!(status.reserve_unallocated, 400_000);
    assert_eq!(status.withdrawable, 0, "配分の拘束だけが残る");

    // 拘束済みの額を超える出金は「内部エラー」ではなく残高不足として拒否する。
    let over = withdraw(400_000, 2, b"wd-over");
    let over = over.expect_err("must reject over the available balance");
    assert!(
        matches!(over, ErrorCode::InsufficientFunds { .. }),
        "{over:?}"
    );
}

#[test]
fn a_session_from_another_caller_is_rejected() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(23);
    let key = secret(205);
    let (session, _eoa) = open_session(&pic, vault, caller, &key);

    let other = principal(24);
    let status: Result<FundStatus, ErrorCode> =
        update(&pic, vault, other, "get_fund_status", session).expect("call");
    let error = status.expect_err("must reject another caller");
    assert!(
        matches!(error, ErrorCode::Unauthenticated { .. }),
        "{error:?}"
    );
}

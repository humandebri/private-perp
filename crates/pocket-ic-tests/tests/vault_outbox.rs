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
use api_types::{AccountKind, AssetId, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall, call_with_mocked_outcall_captured, deploy,
    deploy_default, pic, principal, update, update_args,
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
    provisioned.expect("provisioned");

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
    assert_eq!(
        after.reserved_for_withdrawal, 250_000,
        "不明な払出しは拘束を保持"
    );
    assert_eq!(after.unknowns.len(), 1);

    let swept_again: Result<u32, ErrorCode> =
        update_args(&pic, vault, caller, "test_sweep_now", ()).expect("call");
    assert_eq!(swept_again.expect("sweep"), 0, "自動再送しない");
}

#[test]
fn an_unknown_action_can_be_resolved_as_not_executed() {
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

    // controllerが「未実行」として解消すると、予約が戻り要求はrejectedになる。
    let resolved: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "resolve_unknown_action",
        (action_id.clone(), false),
    )
    .expect("call");
    resolved.expect("resolved");

    let after = status(&pic, vault, caller, &session);
    assert!(after.unknowns.is_empty(), "未解決actionが消える");
    assert_eq!(after.reserve_unallocated, 1_000_000, "資金が戻る");
    assert_eq!(after.reserved_for_withdrawal, 0);

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

    // 二重解消は拒否する。
    let again: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "resolve_unknown_action",
        (action_id, false),
    )
    .expect("call");
    assert!(again.is_err(), "解消済みは再度解消できない");
}

/// vaultのconfig（`HL_CHAIN_NAME` / `HL_USER_SIGNED_CHAIN_ID`）と同じ値。
/// 送信内容を独立に検証するため、テスト側にも同じ値を置く（不一致なら署名検証が落ちる）。
const HL_CHAIN_NAME: &str = "Testnet";
const HL_USER_SIGNED_CHAIN_ID: u64 = 421_614;
const HL_EXCHANGE_URL: &str = "https://api.hyperliquid-testnet.xyz/exchange";

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

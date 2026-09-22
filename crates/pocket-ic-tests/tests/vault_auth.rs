//! 認証の失敗試験（`docs/phase-0/threat-test-matrix.md` の T-101〜T-105）。
//!
//! テストはクライアント役として振る舞い、`hl_sign::private_perp` でchallengeへ署名する。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{BadRequestCode, ErrorCode};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy_default, pic, principal, update};
use std::time::Duration;

const ORIGIN: &str = "https://app.example.test";

fn secret(seed: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes
}

fn challenge_request(caller: Principal, eoa: [u8; 20]) -> ChallengeRequest {
    ChallengeRequest {
        eoa_address: eoa.to_vec().into(),
        principal: caller,
        purpose: ChallengePurpose::Login,
        network: Network::Local,
        origin: ORIGIN.to_string(),
    }
}

/// 発行されたchallengeを、サーバと同じ入力で再構成する。
fn rebuild(
    vault: Principal,
    caller: Principal,
    eoa: [u8; 20],
    response: &ChallengeResponse,
    origin: &str,
    network: &str,
) -> private_perp::Challenge {
    private_perp::Challenge {
        purpose: "login".to_string(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: network.to_string(),
        origin: origin.to_string(),
        nonce: response.nonce.as_ref().try_into().expect("32-byte nonce"),
        expires_at: response.expires_at,
    }
}

fn issue(
    pic: &pocket_ic::PocketIc,
    vault: Principal,
    caller: Principal,
    eoa: [u8; 20],
) -> ChallengeResponse {
    let outcome: Result<ChallengeResponse, ErrorCode> = update(
        pic,
        vault,
        caller,
        "issue_challenge",
        challenge_request(caller, eoa),
    )
    .expect("issue_challenge call");
    outcome.expect("issue_challenge")
}

#[test]
fn login_with_a_valid_signature_opens_a_session() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(10);
    let secret_key = secret(101);
    let eoa = address_from_secret(&secret_key).expect("address");

    let response = issue(&pic, vault, caller, eoa);
    assert!(!response.challenge_id.is_empty());
    // typed dataはEIP-712のJSONで、primaryTypeとnonceを含む。
    let typed: serde_json::Value =
        serde_json::from_slice(&response.typed_data).expect("typed data");
    assert_eq!(typed["primaryType"], "PrivatePerpChallenge");
    assert_eq!(typed["types"]["EIP712Domain"].as_array().unwrap().len(), 4);
    assert_eq!(typed["domain"]["name"], "private-perp");
    assert_eq!(typed["message"]["origin"], ORIGIN);

    let challenge = rebuild(vault, caller, eoa, &response, ORIGIN, "local");
    let signature = challenge.sign_for_tests(&secret_key).expect("sign");

    let outcome: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: response.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("open_session call");
    let handle = outcome.expect("session");
    assert_eq!(handle.vault_principal, vault);
    assert!(handle.expires_at > 0);
}

#[test]
fn a_signature_from_another_key_is_rejected() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(11);
    let eoa = address_from_secret(&secret(102)).expect("address");

    let response = issue(&pic, vault, caller, eoa);
    let challenge = rebuild(vault, caller, eoa, &response, ORIGIN, "local");
    // 別の鍵で署名する。
    let signature = challenge.sign_for_tests(&secret(103)).expect("sign");

    let outcome: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: response.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("open_session call");
    let error = outcome.expect_err("must reject another key");
    assert_eq!(
        error,
        ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: "signature does not match the EOA".to_string()
        }
    );
}

#[test]
fn a_challenge_can_only_be_used_once() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(12);
    let secret_key = secret(104);
    let eoa = address_from_secret(&secret_key).expect("address");

    let response = issue(&pic, vault, caller, eoa);
    let challenge = rebuild(vault, caller, eoa, &response, ORIGIN, "local");
    let signature = challenge.sign_for_tests(&secret_key).expect("sign");
    let request = OpenSessionRequest {
        challenge_id: response.challenge_id.clone(),
        eoa_signature: signature.to_bytes65().to_vec().into(),
    };

    let first: Result<SessionHandle, ErrorCode> =
        update(&pic, vault, caller, "open_session", request.clone()).expect("open_session call");
    first.expect("first session");

    let second: Result<SessionHandle, ErrorCode> =
        update(&pic, vault, caller, "open_session", request).expect("open_session call");
    let error = second.expect_err("must reject a reused challenge");
    assert_eq!(
        error,
        ErrorCode::BadRequest {
            code: BadRequestCode::ChallengeReused,
            detail: "challenge already used".to_string()
        }
    );
}

#[test]
fn an_expired_challenge_is_rejected() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(13);
    let secret_key = secret(105);
    let eoa = address_from_secret(&secret_key).expect("address");

    let response = issue(&pic, vault, caller, eoa);
    let challenge = rebuild(vault, caller, eoa, &response, ORIGIN, "local");
    let signature = challenge.sign_for_tests(&secret_key).expect("sign");

    // 5分の期限を過ぎる。
    pic.advance_time(Duration::from_secs(6 * 60));
    pic.tick();

    let outcome: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: response.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("open_session call");
    let error = outcome.expect_err("must reject an expired challenge");
    assert_eq!(
        error,
        ErrorCode::BadRequest {
            code: BadRequestCode::ChallengeExpired,
            detail: "challenge expired".to_string()
        }
    );
}

#[test]
fn a_challenge_signed_for_another_origin_is_rejected() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(14);
    let secret_key = secret(106);
    let eoa = address_from_secret(&secret_key).expect("address");

    let response = issue(&pic, vault, caller, eoa);
    // 攻撃者が別origin・別networkで署名した場合。
    let challenge = rebuild(
        vault,
        caller,
        eoa,
        &response,
        "https://evil.test",
        "mainnet",
    );
    let signature = challenge.sign_for_tests(&secret_key).expect("sign");

    let outcome: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: response.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("open_session call");
    let error = outcome.expect_err("must reject a mismatched origin");
    assert_eq!(
        error,
        ErrorCode::BadRequest {
            code: BadRequestCode::InvalidSignature,
            detail: "signature does not match the EOA".to_string()
        }
    );
}

/// T-102: challengeは発行時のprincipalへ束縛され、別principalではredeemできない。
///
/// 束縛を確認しないと、他人が取得した署名を自分のprincipalで使ってセッションを
/// 得られる（フロントエンドのJSは配信側で差し替えられうるという前提の防御）。
#[test]
fn a_challenge_bound_to_a_principal_cannot_be_redeemed_by_another() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(17);
    let secret_key = secret(112);
    let eoa = address_from_secret(&secret_key).expect("address");

    let response = issue(&pic, vault, caller, eoa);
    let challenge = rebuild(vault, caller, eoa, &response, ORIGIN, "local");
    let signature = challenge.sign_for_tests(&secret_key).expect("sign");

    // 別principalが同じ署名でセッションを開こうとしても拒否する。
    let attacker = principal(18);
    let stolen: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        attacker,
        "open_session",
        OpenSessionRequest {
            challenge_id: response.challenge_id.clone(),
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("call");
    let error = stolen.expect_err("must reject another principal");
    assert!(
        matches!(error, ErrorCode::Unauthenticated { .. }),
        "{error:?}"
    );

    // 束縛の確認は消費の前に行うため、正規のcallerは同じchallengeをまだ使える
    // （他人が消費してログインを妨害できない）。
    let legitimate: Result<SessionHandle, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: response.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .expect("call");
    legitimate.expect("正当なprincipalはセッションを開ける");
}

/// challengeの発行は呼び出し元principalの申告と一致することを要求する。
#[test]
fn a_challenge_principal_must_match_the_caller() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(19);
    let eoa = address_from_secret(&secret(113)).expect("address");

    let outcome: Result<ChallengeResponse, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "issue_challenge",
        challenge_request(principal(20), eoa),
    )
    .expect("call");
    let error = outcome.expect_err("must reject another principal");
    assert!(
        matches!(error, ErrorCode::Unauthenticated { .. }),
        "{error:?}"
    );
}

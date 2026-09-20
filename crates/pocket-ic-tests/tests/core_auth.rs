//! `trading_core` の認可境界（vaultのセッション検証を使う）の試験。

use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::ErrorCode;
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, deploy, deploy_default, pic, principal, update,
};

const ORIGIN: &str = "https://app.example.test";

fn secret(seed: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes
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
fn core_authorizes_through_the_vault_session() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(100);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    // vault principal未設定では認可できない。
    let caller = principal(101);
    let key = secret(171);
    let session = open_session(&pic, vault, caller, &key);
    let unset: Result<Vec<u8>, ErrorCode> =
        update(&pic, core, caller, "whoami", session.clone()).expect("call");
    let unset = unset.expect_err("vault principal is not configured");
    assert!(matches!(unset, ErrorCode::Internal { .. }), "{unset:?}");

    // controllerがvault principalを設定する。
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");

    // 本人のセッションではuser_idが返る。
    let who: Result<Vec<u8>, ErrorCode> =
        update(&pic, core, caller, "whoami", session.clone()).expect("call");
    let user_id = who.expect("whoami");
    assert_eq!(user_id.len(), 32);

    // 別principalが同じセッションを使うと拒否する（callerを信用しない）。
    let other: Result<Vec<u8>, ErrorCode> =
        update(&pic, core, principal(102), "whoami", session.clone()).expect("call");
    let other = other.expect_err("another caller");
    assert!(
        matches!(other, ErrorCode::Unauthenticated { .. }),
        "{other:?}"
    );

    // 失効したセッションは拒否する。
    let revoked: Result<(), ErrorCode> =
        update(&pic, vault, caller, "revoke_session", session.clone()).expect("call");
    revoked.expect("revoke");
    let after: Result<Vec<u8>, ErrorCode> =
        update(&pic, core, caller, "whoami", session).expect("call");
    assert_eq!(after.expect_err("revoked"), ErrorCode::SessionRevoked);

    // 非controllerはvault principalを変更できない。
    let denied: Result<(), ErrorCode> =
        update(&pic, core, principal(103), "set_vault_principal", vault).expect("call");
    assert!(matches!(
        denied.expect_err("denied"),
        ErrorCode::Unauthenticated { .. }
    ));
}

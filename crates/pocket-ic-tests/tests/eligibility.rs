use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::eligibility::{EligibilityClaims, EligibilityStatus, EligibilityToken};
use api_types::error::ErrorCode;
use api_types::operations_status::CyclesStatus;
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::{address_from_secret, sign_digest_for_tests};
use pocket_ic_tests::{
    CONTROL_GUARD_WASM, FUNDS_VAULT_WASM, deploy, envelope, pic, principal, update, update_args,
};
use std::io::Read;

fn random_key() -> [u8; 32] {
    loop {
        let mut key = [0; 32];
        std::fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut key)
            .unwrap();
        if address_from_secret(&key).is_ok() {
            return key;
        }
    }
}

fn session(
    pic: &pocket_ic::PocketIc,
    vault: Principal,
    caller: Principal,
    key: &[u8; 32],
) -> SessionHandle {
    let eoa = address_from_secret(key).unwrap();
    let challenge: Result<ChallengeResponse, ErrorCode> = update(
        pic,
        vault,
        caller,
        "issue_challenge",
        ChallengeRequest {
            eoa_address: eoa.to_vec().into(),
            principal: caller,
            purpose: ChallengePurpose::Login,
            network: Network::Local,
            origin: "https://app.example.test".into(),
        },
    )
    .unwrap();
    let challenge = challenge.unwrap();
    let intent = private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".into(),
        origin: "https://app.example.test".into(),
        nonce: challenge.nonce.as_ref().try_into().unwrap(),
        expires_at: challenge.expires_at,
    };
    let signature = intent.sign_for_tests(key).unwrap();
    let opened: Result<SessionHandle, ErrorCode> = update(
        pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: challenge.challenge_id,
            eoa_signature: signature.to_bytes65().to_vec().into(),
        },
    )
    .unwrap();
    opened.unwrap()
}

fn register(
    pic: &pocket_ic::PocketIc,
    vault: Principal,
    caller: Principal,
    session: &SessionHandle,
    token: EligibilityToken,
) -> Result<EligibilityStatus, ErrorCode> {
    let client = envelope::client(77);
    client
        .call_encoded(
            pic,
            vault,
            caller,
            "register_eligibility",
            &candid::encode_args((session.clone(), token)).unwrap(),
        )
        .unwrap()
}

#[test]
fn token_binds_identity_account_network_terms_and_nonce() {
    let pic = pic();
    let sns = principal(181);
    let alice = principal(182);
    let bob = principal(183);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    let issuer_key = random_key();
    let issuer = address_from_secret(&issuer_key).unwrap();
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_eligibility",
        (vault, 3u64, issuer.to_vec(), true),
    )
    .unwrap();
    configured.unwrap();
    let unauthenticated: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        bob,
        "configure_eligibility",
        (vault, 4u64, issuer.to_vec(), true),
    )
    .unwrap();
    assert!(matches!(
        unauthenticated,
        Err(ErrorCode::Unauthenticated { .. })
    ));

    let alice_session = session(&pic, vault, alice, &random_key());
    let bob_session = session(&pic, vault, bob, &random_key());
    let client = envelope::client(77);
    let prepared: Result<api_types::Blob, ErrorCode> = client
        .call_encoded(
            &pic,
            vault,
            alice,
            "prepare_trading_account",
            &candid::encode_one(alice_session.clone()).unwrap(),
        )
        .unwrap();
    let account = prepared.unwrap();
    let now = envelope::now_ms(&pic);
    let target: Result<EligibilityClaims, ErrorCode> = client
        .call_encoded(
            &pic,
            vault,
            alice,
            "eligibility_signing_claims",
            &candid::encode_args((alice_session.clone(), now + 60_000)).unwrap(),
        )
        .unwrap();
    let claims = target.unwrap();
    assert_eq!(claims.account_id, account);
    assert_eq!(claims.terms_version, 3);
    let signed = |claims: EligibilityClaims| {
        let digest = hl_sign::keccak256_concat(&[
            b"private-perp/eligibility/v1",
            &candid::encode_one(&claims).unwrap(),
        ]);
        EligibilityToken {
            claims,
            signature: sign_digest_for_tests(&digest, &issuer_key)
                .unwrap()
                .to_bytes65()
                .to_vec()
                .into(),
        }
    };
    let token = signed(claims.clone());
    assert!(matches!(
        register(&pic, vault, bob, &bob_session, token.clone()),
        Err(ErrorCode::NotEligible { .. })
    ));
    let mut wrong_account = claims.clone();
    wrong_account.account_id = vec![0x42; 32].into();
    assert!(matches!(
        register(&pic, vault, alice, &alice_session, signed(wrong_account)),
        Err(ErrorCode::NotEligible { .. })
    ));
    let mut wrong_network = claims.clone();
    wrong_network.network = Network::Testnet;
    assert!(matches!(
        register(&pic, vault, alice, &alice_session, signed(wrong_network)),
        Err(ErrorCode::NotEligible { .. })
    ));
    let mut wrong_terms = claims.clone();
    wrong_terms.terms_version = 4;
    assert!(matches!(
        register(&pic, vault, alice, &alice_session, signed(wrong_terms)),
        Err(ErrorCode::NotEligible { .. })
    ));
    let mut expired = claims.clone();
    expired.expires_at = now;
    assert!(matches!(
        register(&pic, vault, alice, &alice_session, signed(expired)),
        Err(ErrorCode::NotEligible { .. })
    ));
    let mut bad_signature = token.clone();
    bad_signature.signature = vec![0; 65].into();
    assert!(register(&pic, vault, alice, &alice_session, bad_signature).is_err());
    assert!(
        register(&pic, vault, alice, &alice_session, token.clone())
            .unwrap()
            .eligible
    );
    assert!(
        register(&pic, vault, alice, &alice_session, token)
            .unwrap()
            .eligible
    );
    let mut next = claims.clone();
    next.nonce = vec![0xA5; 32].into();
    next.issued_at += 1;
    assert!(
        register(&pic, vault, alice, &alice_session, signed(next))
            .unwrap()
            .eligible
    );
    assert!(register(&pic, vault, alice, &alice_session, signed(claims.clone())).is_err());
    let mut reused_nonce = claims.clone();
    reused_nonce.expires_at += 1_000;
    assert!(register(&pic, vault, alice, &alice_session, signed(reused_nonce)).is_err());

    let other_issuer = address_from_secret(&random_key()).unwrap();
    let unchanged_terms: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_eligibility",
        (vault, 3u64, other_issuer.to_vec(), true),
    )
    .unwrap();
    assert!(matches!(unchanged_terms, Err(ErrorCode::PolicyUnavailable)));
}

#[test]
fn cycles_floor_and_reserve_stop_new_admission_without_stopping_status() {
    let pic = pic();
    let sns = principal(191);
    let guard = deploy(
        &pic,
        CONTROL_GUARD_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![sns]),
        candid::encode_one(()).unwrap(),
    );
    let configured: Result<(), ErrorCode> =
        update(&pic, guard, sns, "set_sns_principal", sns).unwrap();
    configured.unwrap();
    let configured: Result<(), ErrorCode> =
        update(&pic, vault, sns, "set_journal_guard", guard).unwrap();
    configured.unwrap();
    let unconfigured: Result<CyclesStatus, ErrorCode> =
        update(&pic, vault, sns, "get_cycles_status", ()).unwrap();
    assert!(unconfigured.unwrap().new_risk_stopped);
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_cycles",
        (vault, 100_000_000_000u128, 1_000_000_000_000u128),
    )
    .unwrap();
    configured.unwrap();
    let healthy: Result<CyclesStatus, ErrorCode> =
        update(&pic, vault, sns, "get_cycles_status", ()).unwrap();
    let healthy = healthy.unwrap();
    assert!(!healthy.new_risk_stopped);
    assert!(healthy.refill_target.unwrap() > healthy.exit_reserve.unwrap());
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        guard,
        sns,
        "configure_cycles",
        (vault, 5_000_000_000_000_000u128, 1_000_000_000_000u128),
    )
    .unwrap();
    configured.unwrap();
    let low: Result<CyclesStatus, ErrorCode> =
        update(&pic, vault, sns, "get_cycles_status", ()).unwrap();
    assert!(low.unwrap().new_risk_stopped);
}

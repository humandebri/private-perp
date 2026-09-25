use api_types::Blob;
use api_types::Network;
use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::builder_fee::{BuilderFeeConsent, BuilderFeeConsentClaims, BuilderFeeMockStatus};
use api_types::error::ErrorCode;
use hl_sign::private_perp;
use hl_sign::signature::{address_from_secret, sign_digest_for_tests};
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy, envelope, pic, principal, query, update};

fn session(
    pic: &pocket_ic::PocketIc,
    vault: candid::Principal,
    caller: candid::Principal,
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
            origin: "https://fee.example.test".into(),
        },
    )
    .unwrap();
    let challenge = challenge.unwrap();
    let signed = private_perp::Challenge {
        purpose: "login".into(),
        eoa,
        principal: caller.as_slice().to_vec(),
        canister: vault.as_slice().to_vec(),
        network: "local".into(),
        origin: "https://fee.example.test".into(),
        nonce: challenge.nonce.as_ref().try_into().unwrap(),
        expires_at: challenge.expires_at,
    }
    .sign_for_tests(key)
    .unwrap();
    let opened: Result<SessionHandle, ErrorCode> = update(
        pic,
        vault,
        caller,
        "open_session",
        OpenSessionRequest {
            challenge_id: challenge.challenge_id,
            eoa_signature: signed.to_bytes65().to_vec().into(),
        },
    )
    .unwrap();
    let opened = opened.unwrap();
    pocket_ic_tests::activate_local_user(pic, vault, caller, &opened);
    opened
}

fn consent(claims: BuilderFeeConsentClaims, key: &[u8; 32]) -> BuilderFeeConsent {
    let encoded = candid::encode_one(&claims).unwrap();
    let digest = hl_sign::keccak256_concat(&[b"private-perp/builder-fee-consent/v1", &encoded]);
    let personal = hl_sign::keccak256_concat(&[b"\x19Ethereum Signed Message:\n32", &digest]);
    BuilderFeeConsent {
        claims,
        eoa_signature: sign_digest_for_tests(&personal, key)
            .unwrap()
            .to_bytes65()
            .to_vec()
            .into(),
    }
}

fn register(
    pic: &pocket_ic::PocketIc,
    vault: candid::Principal,
    caller: candid::Principal,
    session: &SessionHandle,
    signed: BuilderFeeConsent,
) -> Result<BuilderFeeMockStatus, ErrorCode> {
    envelope::client(70)
        .call_encoded(
            pic,
            vault,
            caller,
            "register_builder_fee_mock_consent",
            &candid::encode_args((session.clone(), signed)).unwrap(),
        )
        .unwrap()
}

#[test]
fn signed_zero_fee_consent_has_mock_approval_and_zero_accounting() {
    let pic = pic();
    let controller = principal(210);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let alice = principal(211);
    let bob = principal(212);
    let mut alice_key = [0; 32];
    alice_key[31] = 31;
    let mut bob_key = [0; 32];
    bob_key[31] = 32;
    let alice_session = session(&pic, vault, alice, &alice_key);
    let bob_session = session(&pic, vault, bob, &bob_key);
    let now = envelope::now_ms(&pic);
    let target: Result<(BuilderFeeConsentClaims, Blob), ErrorCode> = envelope::client(70)
        .call_encoded(
            &pic,
            vault,
            alice,
            "builder_fee_signing_claims",
            &candid::encode_args((alice_session.clone(), Blob::from(vec![9; 20]), now + 60_000))
                .unwrap(),
        )
        .unwrap();
    let (claims, digest) = target.unwrap();
    assert_eq!(claims.fee_decibps, 0);
    let expected = hl_sign::keccak256_concat(&[
        b"private-perp/builder-fee-consent/v1",
        &candid::encode_one(&claims).unwrap(),
    ]);
    assert_eq!(digest.as_ref(), expected);
    let signed = consent(claims.clone(), &alice_key);
    let other: Result<BuilderFeeMockStatus, ErrorCode> =
        query(&pic, vault, bob, "builder_fee_mock_status", bob_session).unwrap();
    assert!(!other.unwrap().approved);
    assert!(register(&pic, vault, bob, &alice_session, signed.clone()).is_err());
    assert!(
        register(
            &pic,
            vault,
            alice,
            &alice_session,
            consent(claims.clone(), &bob_key)
        )
        .is_err()
    );
    let mut paid = claims.clone();
    paid.fee_decibps = 1;
    assert!(
        register(
            &pic,
            vault,
            alice,
            &alice_session,
            consent(paid, &alice_key)
        )
        .is_err()
    );
    let mut wrong_account = claims.clone();
    wrong_account.account_id = vec![1; 32].into();
    assert!(
        register(
            &pic,
            vault,
            alice,
            &alice_session,
            consent(wrong_account, &alice_key)
        )
        .is_err()
    );
    let mut wrong_network = claims.clone();
    wrong_network.network = Network::Testnet;
    assert!(
        register(
            &pic,
            vault,
            alice,
            &alice_session,
            consent(wrong_network, &alice_key)
        )
        .is_err()
    );
    let mut expired = claims.clone();
    expired.expires_at = now;
    assert!(
        register(
            &pic,
            vault,
            alice,
            &alice_session,
            consent(expired, &alice_key)
        )
        .is_err()
    );
    let accepted = register(&pic, vault, alice, &alice_session, signed.clone()).unwrap();
    assert!(accepted.approved);
    assert_eq!((accepted.approval_records, accepted.charged_micros), (1, 0));
    let duplicate = register(&pic, vault, alice, &alice_session, signed).unwrap();
    assert_eq!(
        (duplicate.approval_records, duplicate.charged_micros),
        (1, 0)
    );
    let second: Result<(BuilderFeeConsentClaims, Blob), ErrorCode> = envelope::client(70)
        .call_encoded(
            &pic,
            vault,
            alice,
            "builder_fee_signing_claims",
            &candid::encode_args((alice_session.clone(), Blob::from(vec![9; 20]), now + 60_000))
                .unwrap(),
        )
        .unwrap();
    let newer = second.unwrap().0;
    assert_ne!(claims.nonce, newer.nonce);
    let newer_status = register(
        &pic,
        vault,
        alice,
        &alice_session,
        consent(newer, &alice_key),
    )
    .unwrap();
    assert_eq!(
        (newer_status.approval_records, newer_status.charged_micros),
        (2, 0)
    );
    assert!(
        register(
            &pic,
            vault,
            alice,
            &alice_session,
            consent(claims, &alice_key)
        )
        .is_err()
    );
}

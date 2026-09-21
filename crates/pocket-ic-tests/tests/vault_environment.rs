//! `funds_vault` の環境設定（network・endpoint・key ID）とE-2の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::environment::EnvironmentView;
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, call_with_mocked_outcall_captured, deploy, pic, principal, update,
    update_args,
};

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

/// mainnetは拒否し、networkとendpointの不一致も拒否する（E-2）。
#[test]
fn the_vault_refuses_mainnet_and_mismatched_endpoints() {
    let pic = pic();
    let controller = principal(230);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let outsider = principal(231);

    // 既定はlocal（実venueのhostは拒否する）。
    let default: Result<EnvironmentView, ErrorCode> =
        pocket_ic_tests::query(&pic, vault, outsider, "get_environment", ()).expect("call");
    let default = default.expect("environment");
    assert_eq!(default.network, Network::Local);
    assert_eq!(default.exchange_url, "http://localhost:8080/exchange");
    assert_eq!(default.ecdsa_key_id, "test_key_1");

    // 非controllerは変更できない。
    let denied: Result<(), ErrorCode> =
        update(&pic, vault, outsider, "set_network", "testnet".to_string()).expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );

    // E-2: mainnetは拒否する。
    let mainnet: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_network",
        "mainnet".to_string(),
    )
    .expect("call");
    assert!(
        matches!(
            mainnet,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::NetworkMismatch,
                ..
            })
        ),
        "{mainnet:?}"
    );

    // testnetへ切り替える。
    let testnet: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_network",
        "testnet".to_string(),
    )
    .expect("call");
    testnet.expect("set_network(testnet)");

    // E-2: testnet設定にmainnet endpointを指定すると拒否する。
    let mismatch: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "set_venue_endpoints",
        (
            "https://api.hyperliquid.xyz/exchange".to_string(),
            "https://api.hyperliquid-testnet.xyz/info".to_string(),
        ),
    )
    .expect("call");
    assert!(
        matches!(
            mismatch,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::NetworkMismatch,
                ..
            })
        ),
        "{mismatch:?}"
    );

    // 正しいendpointは通り、診断queryに反映される。
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "set_venue_endpoints",
        (
            "https://api.hyperliquid-testnet.xyz/exchange".to_string(),
            "https://api.hyperliquid-testnet.xyz/info".to_string(),
        ),
    )
    .expect("call");
    configured.expect("set_venue_endpoints(testnet)");
    let environment: Result<EnvironmentView, ErrorCode> =
        pocket_ic_tests::query(&pic, vault, outsider, "get_environment", ()).expect("call");
    let environment = environment.expect("environment");
    assert_eq!(environment.network, Network::Testnet);
    assert_eq!(
        environment.info_url,
        "https://api.hyperliquid-testnet.xyz/info"
    );

    // key IDの形式を検証する。
    let invalid: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_ecdsa_key_id",
        "key 1".to_string(),
    )
    .expect("call");
    assert!(
        matches!(
            invalid,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                ..
            })
        ),
        "{invalid:?}"
    );
    let key: Result<(), ErrorCode> = update(
        &pic,
        vault,
        controller,
        "set_ecdsa_key_id",
        "test_key_1".to_string(),
    )
    .expect("call");
    key.expect("set_ecdsa_key_id");
}

/// 設定したendpointが送信（`/exchange`）と取得（`/info`）に使われる。
#[test]
fn the_configured_endpoints_are_used_for_vault_calls() {
    let pic = pic();
    let controller = principal(232);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let caller = principal(233);
    let session = open_session(&pic, vault, caller, &secret(234));

    let exchange_url = "http://127.0.0.1:9922/exchange";
    let info_url = "http://127.0.0.1:9922/info";
    let configured: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        controller,
        "set_venue_endpoints",
        (exchange_url.to_string(), info_url.to_string()),
    )
    .expect("call");
    configured.expect("set_venue_endpoints(local mock)");

    // 送信（Agent承認）は設定した`/exchange`へ出る。
    let (approved, captured) = call_with_mocked_outcall_captured::<
        (SessionHandle, u64, Blob),
        Result<Option<api_types::fund::AgentGeneration>, ErrorCode>,
    >(
        &pic,
        vault,
        caller,
        "approve_agent_generation",
        (session.clone(), 1u64, blob(&[7u8; 20])),
        Ok((200, ACCEPTED.to_vec())),
    )
    .expect("call");
    let captured = captured.expect("outcall");
    assert_eq!(captured.url, exchange_url, "設定した/exchangeへ送る");
    // 承認は取引所の受理に依存するため、応答の解釈自体はここでは問わない。
    let _ = approved;

    // 入金の取得は設定した`/info`へ出る。
    let provisioned: Result<Vec<u8>, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "provision_reserve_account",
        session.clone(),
    )
    .expect("call");
    let address = provisioned.expect("reserve account");
    let (reconciled, captured) =
        call_with_mocked_outcall_captured::<(Blob,), Result<u32, ErrorCode>>(
            &pic,
            vault,
            controller,
            "reconcile_deposits",
            (blob(&address),),
            Ok((200, b"[]".to_vec())),
        )
        .expect("call");
    reconciled.expect("reconcile");
    let captured = captured.expect("outcall");
    assert_eq!(captured.url, info_url, "設定した/infoで取得する");
}

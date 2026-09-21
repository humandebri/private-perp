//! 個人APIのHPKE封筒（`docs/phase-0/api-contract.md` 6節）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::envelope::{CancelOrderQuery, ListQuery, SnapshotQuery};
use api_types::error::{BadRequestCode, ErrorCode};
use api_types::fund::AgentGeneration;
use api_types::order::FillView;
use api_types::order::{
    AccountSnapshot, OrderKind, OrderSummary, Side, SubmitOrderArgs, SubmitOrderResult,
};
use api_types::{Blob, Network, Paged};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::envelope::{self, EnvelopeClient};
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, approve_agent_at_vault, configure_policy, deploy,
    fund_trading_account, pic, principal, rotate_hpke_key, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]"#;

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

/// controller・core・セッションを用意する（HPKE鍵は生成しない）。
fn setup(pic: &PocketIc) -> (Principal, Principal, SessionHandle) {
    let controller = principal(160);
    let vault = deploy(
        pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    configure_policy(pic, core, controller, &["BTC", "ETH"]);
    let meta: Result<(), ErrorCode> = update_args(
        pic,
        core,
        controller,
        "set_meta_cache",
        (
            "local".to_string(),
            "hyperliquid".to_string(),
            UNIVERSE.to_string(),
        ),
    )
    .expect("call");
    meta.expect("set_meta_cache");
    let context: Result<(), ErrorCode> = update_args(
        pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(161);
    let session = open_session(pic, vault, caller, &secret(162));
    // 配分の受付と取引口座への着金（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        pic,
        vault,
        controller,
        caller,
        &session,
        b"hpke-alloc",
        3_000_000_000,
        163,
    );
    let agent: Result<AgentGeneration, ErrorCode> = update(
        pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");
    (controller, core, session)
}

/// 個人APIは正しい封筒でのみ通り、平文のない応答を返す。
#[test]
fn personal_apis_require_a_valid_envelope() {
    let pic = pic();
    let (controller, core, session) = setup(&pic);
    let caller = principal(161);
    let client = envelope::client(1);

    // 鍵が無いうちは公開鍵を配布しない（機密性の前提が欠けているためfail-closed）。
    let unset: Result<Vec<u8>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, caller, "get_hpke_public_key", ()).expect("call");
    assert!(unset.is_err(), "{unset:?}");

    // 生成はcontrollerのみ。
    let denied: Result<Vec<u8>, ErrorCode> =
        update(&pic, core, caller, "rotate_hpke_key", ()).expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
    let public = rotate_hpke_key(&pic, core, controller);
    assert_eq!(public.len(), 32);
    let served: Result<Vec<u8>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, caller, "get_hpke_public_key", ()).expect("call");
    assert_eq!(served.expect("public key"), public);

    // 注文を1件受付けてから、個人APIを封筒で叩く。
    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            SubmitOrderArgs {
                session: session.clone(),
                client_request_id: blob(b"hpke-order"),
                account_id: blob(&[0u8; 32]),
                market: "ETH".to_string(),
                side: Side::Buy,
                kind: OrderKind::LimitGtc,
                quantity: "0.05".to_string(),
                limit_price: Some("2500".to_string()),
                slippage_tolerance_bps: None,
                reduce_only: false,
                leverage: Some(3),
                trigger: None,
                expires_after: None,
            },
        ),
    )
    .expect("call");
    let submitted = submitted.expect("accepted order");

    let snapshot: Result<AccountSnapshot, ErrorCode> = client
        .call(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &SnapshotQuery {
                session: session.clone(),
            },
        )
        .expect("call");
    let snapshot = snapshot.expect("snapshot");
    assert_eq!(snapshot.pending_orders.len(), 1);

    let orders: Result<Paged<OrderSummary>, ErrorCode> = client
        .call(
            &pic,
            core,
            caller,
            "list_orders",
            &ListQuery {
                session: session.clone(),
                cursor: None,
                limit: 10,
            },
        )
        .expect("call");
    let orders = orders.expect("orders");
    assert_eq!(orders.items.len(), 1);
    assert_eq!(
        orders.items[0].order_id.as_ref(),
        submitted.order_id.as_ref()
    );

    let fills: Result<Paged<FillView>, ErrorCode> = client
        .call(
            &pic,
            core,
            caller,
            "list_fills",
            &ListQuery {
                session: session.clone(),
                cursor: None,
                limit: 10,
            },
        )
        .expect("call");
    assert!(fills.expect("fills").items.is_empty());

    let cancelled: Result<(), ErrorCode> = client
        .call(
            &pic,
            core,
            caller,
            "cancel_order",
            &CancelOrderQuery {
                session: session.clone(),
                order_id: submitted.order_id.clone(),
            },
        )
        .expect("call");
    assert!(cancelled.is_ok(), "{cancelled:?}");
    let orders: Result<Paged<OrderSummary>, ErrorCode> = client
        .call(
            &pic,
            core,
            caller,
            "list_orders",
            &ListQuery {
                session: session.clone(),
                cursor: None,
                limit: 10,
            },
        )
        .expect("call");
    assert!(orders.expect("orders").items[0].cancel_requested);
}

/// `aad`・期限・method・canister・networkの束縛を破った封筒は通らない。
#[test]
fn envelope_bindings_are_enforced() {
    let pic = pic();
    let (controller, core, session) = setup(&pic);
    let caller = principal(161);
    let client = envelope::client(2);
    rotate_hpke_key(&pic, core, controller);

    let query = SnapshotQuery {
        session: session.clone(),
    };
    let now = envelope::now_ms(&pic);

    // 期限切れ。
    let (request, aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [7u8; 32],
            now.saturating_sub(1_000),
        )
        .expect("build");
    let expired: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(
        matches!(
            expired,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::ExpiredIntent,
                ..
            })
        ),
        "{expired:?}"
    );

    // 期限が遠すぎる（時計ずれと使い回しの抑止）。
    let (request, aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [8u8; 32],
            now + 10 * 60_000,
        )
        .expect("build");
    let too_far: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(
        matches!(
            too_far,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::MalformedPayload,
                ..
            })
        ),
        "{too_far:?}"
    );

    // 別の呼び出し元に束縛した`aad`（caller束縛）。
    let (request, aad) = client
        .build_request(
            &pic,
            core,
            principal(199),
            "get_account_snapshot",
            &query,
            [9u8; 32],
            now + 60_000,
        )
        .expect("build");
    let other_caller: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(other_caller.is_err(), "{other_caller:?}");

    // 暗号文の改竄。
    let (mut request, aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [10u8; 32],
            now + 60_000,
        )
        .expect("build");
    let mut ciphertext = request.ciphertext.as_ref().to_vec();
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0x01;
    request.ciphertext = ciphertext.into();
    let tampered: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(tampered.is_err(), "{tampered:?}");

    // method束縛（別methodとして封をした封筒を送る）。
    let (request, aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "list_orders",
            &query,
            [11u8; 32],
            now + 60_000,
        )
        .expect("build");
    let wrong_method: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(wrong_method.is_err(), "{wrong_method:?}");

    // canister束縛（別canisterへの転用）。
    let (mut request, aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [12u8; 32],
            now + 60_000,
        )
        .expect("build");
    request.canister = principal(198);
    let wrong_canister: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(wrong_canister.is_err(), "{wrong_canister:?}");

    // network束縛（別環境への転用）。
    let mainnet = EnvelopeClient::new(3, "mainnet");
    let (request, aad) = mainnet
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [13u8; 32],
            now + 60_000,
        )
        .expect("build");
    let wrong_network: Result<AccountSnapshot, ErrorCode> = mainnet
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(wrong_network.is_err(), "{wrong_network:?}");

    // 平文でも正しい封筒でもない入力。
    let (mut request, aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [14u8; 32],
            now + 60_000,
        )
        .expect("build");
    request.ciphertext = vec![0u8; 48].into();
    let garbage: Result<AccountSnapshot, ErrorCode> = client
        .call_request(&pic, core, caller, "get_account_snapshot", &request, &aad)
        .expect("call");
    assert!(garbage.is_err(), "{garbage:?}");
}

/// 同じ`request_id`は1回しか使えない。
#[test]
fn a_request_id_cannot_be_reused() {
    let pic = pic();
    let (controller, core, session) = setup(&pic);
    let caller = principal(161);
    let client = envelope::client(4);
    rotate_hpke_key(&pic, core, controller);

    let query = SnapshotQuery {
        session: session.clone(),
    };
    let request_id = [21u8; 32];
    let first: Result<AccountSnapshot, ErrorCode> = client
        .call_with_request_id(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            request_id,
        )
        .expect("call");
    assert!(first.is_ok(), "{first:?}");

    let replay: Result<AccountSnapshot, ErrorCode> = client
        .call_with_request_id(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            request_id,
        )
        .expect("call");
    assert!(
        matches!(
            replay,
            Err(ErrorCode::BadRequest {
                code: BadRequestCode::NonceReused,
                ..
            })
        ),
        "{replay:?}"
    );

    // 別のIDなら通る。
    let fresh: Result<AccountSnapshot, ErrorCode> = client
        .call_with_request_id(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [22u8; 32],
        )
        .expect("call");
    assert!(fresh.is_ok(), "{fresh:?}");
}

/// 鍵を更新すると旧鍵の封筒は通らず、新しい公開鍵で作り直せば通る。
#[test]
fn rotating_the_key_invalidates_old_envelopes() {
    let pic = pic();
    let (controller, core, session) = setup(&pic);
    let caller = principal(161);
    let client = envelope::client(5);
    let first = rotate_hpke_key(&pic, core, controller);

    let query = SnapshotQuery {
        session: session.clone(),
    };
    // 旧鍵で封をする（更新前の公開鍵を使う）。
    let (old_request, old_aad) = client
        .build_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [31u8; 32],
            envelope::now_ms(&pic) + 60_000,
        )
        .expect("build");

    let second = rotate_hpke_key(&pic, core, controller);
    assert_ne!(first, second, "世代が進む");

    let stale: Result<AccountSnapshot, ErrorCode> = client
        .call_request(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &old_request,
            &old_aad,
        )
        .expect("call");
    assert!(stale.is_err(), "旧鍵の封筒は復号できない: {stale:?}");

    let fresh: Result<AccountSnapshot, ErrorCode> = client
        .call_with_request_id(
            &pic,
            core,
            caller,
            "get_account_snapshot",
            &query,
            [32u8; 32],
        )
        .expect("call");
    assert!(fresh.is_ok(), "{fresh:?}");
}

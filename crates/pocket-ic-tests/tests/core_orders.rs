//! `trading_core` の注文受付（認可・allowlist・精度・冪等性）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{ErrorCode, NotAllowedCode};
use api_types::journal::{RecoveryPayload, RecoveryRecord};
use api_types::order::{OrderKind, OrderSummary, Side, SubmitOrderArgs, SubmitOrderResult};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, POLICY_WASM, TRADING_CORE_WASM, approve_agent_at_vault,
    call_with_mocked_outcall, configure_policy, deploy, envelope, fund_trading_account, pic,
    principal, query_args, rotate_hpke_key, sweep_with_venue_outcalls, update, update_args,
};
use pocket_ic_tests::{observe_empty_account, trading_account_id};

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
    let session = session.expect("session");
    pocket_ic_tests::activate_local_user(pic, vault, caller, &session);
    session
}

fn order_args(
    account_id: &Blob,
    session: &SessionHandle,
    request_id: &[u8],
    market: &str,
    quantity: &str,
    price: &str,
) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(request_id),
        account_id: account_id.clone(),
        market: market.to_string(),
        side: Side::Buy,
        kind: OrderKind::LimitGtc,
        quantity: quantity.to_string(),
        limit_price: Some(price.to_string()),
        slippage_tolerance_bps: None,
        reduce_only: false,
        leverage: Some(3),
        trigger: None,
        expires_after: None,
    }
}

#[test]
fn order_risk_replays_from_snapshot_before_acceptance() {
    let pic = pic();
    let controller = principal(231);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).unwrap();
    set.unwrap();
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_meta_cache",
        (
            "local".to_string(),
            "hyperliquid".to_string(),
            UNIVERSE.to_string(),
        ),
    )
    .unwrap();
    meta.unwrap();
    let context: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .unwrap();
    context.unwrap();
    let caller = principal(232);
    let session = open_session(&pic, vault, caller, &secret(182));
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"order-risk-replay-fund",
        5_000_000_000,
        32,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
    let initial = envelope::get_account_snapshot(&pic, core, caller, &session)
        .expect("initial snapshot")
        .expect("account status");
    assert_eq!(initial.open_order_risk_reserved, 0);
    let guard: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_journal_guard", ()).unwrap();
    let guard = guard.unwrap().expect("configured guard");
    let before_order = pic
        .take_canister_snapshot(core, Some(controller), None)
        .expect("snapshot before order acceptance");

    let accepted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"replay-risk", "ETH", "0.05", "2500"),
        ),
    )
    .expect("order call");
    accepted.expect("order accepted");
    let before = envelope::get_account_snapshot(&pic, core, caller, &session)
        .expect("snapshot")
        .expect("account status");
    assert_eq!(before.open_order_risk_reserved, 125_000_000);

    pic.load_canister_snapshot(core, Some(controller), before_order.id)
        .expect("restore before order acceptance");
    let resumed: Result<(), ErrorCode> =
        update(&pic, guard, controller, "resume_journal", core).expect("guard call");
    let pending: Result<bool, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "recovery_replay_pending", ()).unwrap();
    assert!(resumed.is_err(), "external validation is still required");
    let send_status: Result<(bool, bool), ErrorCode> =
        pocket_ic_tests::query(&pic, core, caller, "get_journal_send_status", ()).unwrap();
    assert_eq!(send_status.unwrap(), (true, true));
    let restored = envelope::get_account_snapshot(&pic, core, caller, &session)
        .expect("snapshot")
        .expect("account status");
    assert_eq!(restored.open_order_risk_reserved, 125_000_000);
    assert!(
        restored.pending_orders.is_empty(),
        "plaintext order was not journaled"
    );
    assert!(pending.unwrap());
}

#[test]
fn orders_are_accepted_idempotently_after_authorization() {
    let pic = pic();
    let controller = principal(110);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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

    // 本人のセッションと取引口座を用意する（配分の受付で口座が導出される）。
    let caller = principal(111);
    let key = secret(181);
    let session = open_session(&pic, vault, caller, &key);
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"alloc-for-orders-fund",
        5_000_000_000,
        31,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);

    // 銘柄解決の設定が無い状態ではfail-closedで拒否する（固定値を使わない）。
    let no_context: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"order-ctx", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    assert_eq!(
        no_context.expect_err("must fail closed"),
        ErrorCode::PolicyUnavailable
    );

    let context: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    // 誤った口座と未観測口座は、他の入力が正常でも受付しない。
    for (id, expected) in [
        (blob(&[0u8; 32]), NotAllowedCode::AccountNotOwned),
        (account_id.clone(), NotAllowedCode::OperationNotAvailable),
    ] {
        let rejected: Result<SubmitOrderResult, ErrorCode> = update_args(
            &pic,
            core,
            caller,
            "submit_order",
            (
                session.clone(),
                order_args(&id, &session, b"unobserved", "ETH", "0.05", "2500"),
            ),
        )
        .expect("call");
        assert_eq!(
            rejected.expect_err("fail closed"),
            ErrorCode::NotAllowed { code: expected }
        );
    }
    observe_empty_account(&pic, core, caller, &session);

    // 受付できる（ETHはmetaの添字1）。
    let accepted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, &[71u8; 32], "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    let accepted = accepted.expect("accepted");
    assert_eq!(accepted.order_id.len(), 32);
    assert_eq!(accepted.cloid.len(), 16);

    // 同一ID・同一本文の再送は同じ結果（order_idとcloidが一致）。
    let duplicate: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, &[71u8; 32], "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    let duplicate = duplicate.expect("duplicate");
    assert_eq!(duplicate.order_id, accepted.order_id);
    assert_eq!(duplicate.cloid, accepted.cloid);
    let configured: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_send_journal", ())
            .expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let events: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, core, "recovery_events", (0u64, 100u32)).unwrap();
    let accepted_events: Vec<_> = events
        .unwrap()
        .into_iter()
        .filter(|record| matches!(record.event.payload, RecoveryPayload::OrderAccepted { .. }))
        .collect();
    assert_eq!(accepted_events.len(), 1, "duplicate must not append again");
    match &accepted_events[0].event.payload {
        RecoveryPayload::OrderAccepted {
            order_id,
            request_id,
            risk_micros,
            reduce_only,
            ..
        } => {
            assert_eq!(order_id, &accepted.order_id);
            assert_eq!(request_id.as_ref(), &[71u8; 32]);
            assert!(*risk_micros > 0);
            assert!(!*reduce_only);
        }
        _ => unreachable!(),
    }

    // 同一ID・異なる本文は拒否する。
    let conflict: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, &[71u8; 32], "ETH", "0.06", "2500"),
        ),
    )
    .expect("call");
    assert!(
        matches!(conflict, Err(ErrorCode::IdempotencyConflict { .. })),
        "{conflict:?}"
    );

    // allowlist外（SOL）は拒否する。
    let denied: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"order-2", "SOL", "1", "100"),
        ),
    )
    .expect("call");
    assert_eq!(
        denied.expect_err("denied"),
        ErrorCode::NotAllowed {
            code: NotAllowedCode::AssetNotAllowed
        }
    );

    // 再送せず照合する。不存在と他人の要求を区別しない。
    let lookup = envelope::EnvelopeClient::new(199, "local");
    let request = api_types::envelope::OrderRequestQuery {
        session: session.clone(),
        client_request_id: blob(&[71u8; 32]),
    };
    let status: api_types::envelope::OrderRequestStatus = lookup
        .call(&pic, core, caller, "get_order_by_request", &request)
        .expect("call")
        .expect("lookup");
    assert_eq!(status.order.expect("found").order_id, accepted.order_id);
    let absent: api_types::envelope::OrderRequestStatus = lookup
        .call(
            &pic,
            core,
            caller,
            "get_order_by_request",
            &api_types::envelope::OrderRequestQuery {
                client_request_id: blob(&[72u8; 32]),
                ..request.clone()
            },
        )
        .expect("call")
        .expect("lookup absent");
    assert!(absent.order.is_none());
    let malformed: Result<api_types::envelope::OrderRequestStatus, ErrorCode> = lookup
        .call(
            &pic,
            core,
            caller,
            "get_order_by_request",
            &api_types::envelope::OrderRequestQuery {
                client_request_id: blob(&[1]),
                ..request.clone()
            },
        )
        .expect("call");
    assert!(matches!(malformed, Err(ErrorCode::BadRequest { .. })));
    let caller_b = principal(199);
    let session_b = open_session(&pic, vault, caller_b, &secret(199));
    let foreign: api_types::envelope::OrderRequestStatus = lookup
        .call(
            &pic,
            core,
            caller_b,
            "get_order_by_request",
            &api_types::envelope::OrderRequestQuery {
                session: session_b,
                ..request.clone()
            },
        )
        .expect("call")
        .expect("foreign absent");
    assert!(foreign.order.is_none());
    let spoofed: Result<api_types::envelope::OrderRequestStatus, ErrorCode> = lookup
        .call(&pic, core, caller_b, "get_order_by_request", &request)
        .expect("call");
    assert!(matches!(spoofed, Err(ErrorCode::Unauthenticated { .. })));
    for (id, tamper_method) in [(201u8, true), (202, false)] {
        let (mut sealed, aad) = lookup
            .build_request(
                &pic,
                core,
                caller,
                "get_order_by_request",
                &request,
                [id; 32],
                envelope::now_ms(&pic) + 60_000,
            )
            .expect("build");
        if tamper_method {
            sealed.method = "list_orders".to_string();
        } else {
            sealed.aad = blob(&[0; 32]);
        }
        let wrong: Result<api_types::envelope::OrderRequestStatus, ErrorCode> = lookup
            .call_request(&pic, core, caller, "get_order_by_request", &sealed, &aad)
            .expect("call");
        assert!(wrong.is_err());
    }

    // 一覧は新しい順に返り、別principalのセッションでは取得できない。
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let listed = listed.expect("list");
    assert_eq!(
        listed.items.len(),
        1,
        "受付けた1件のみ（競合・拒否は登録されない）"
    );
    assert_eq!(listed.items[0].order_id, accepted.order_id);
    assert_eq!(listed.items[0].market, "ETH");
    assert_eq!(listed.items[0].state, api_types::order::OrderState::Pending);
    assert_eq!(listed.items[0].asset_index, 1, "metaの添字から解決");
    assert!(listed.next_cursor.is_none(), "上限未満ならカーソルなし");

    let denied_list: Result<api_types::Paged<OrderSummary>, ErrorCode> = envelope::list_orders(
        &pic,
        core,
        principal(113),
        &session.clone(),
        None::<Blob>,
        10u32,
    )
    .expect("call");
    assert!(
        matches!(denied_list, Err(ErrorCode::Unauthenticated { .. })),
        "{denied_list:?}"
    );

    // 取消要求は冪等で、他者の注文や不明なIDは拒否する。
    let cancel: Result<(), ErrorCode> = envelope::cancel_order(
        &pic,
        core,
        caller,
        &session.clone(),
        accepted.order_id.clone(),
    )
    .expect("call");
    cancel.expect("cancel");

    let after_cancel: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    assert!(
        after_cancel.expect("list").items[0].cancel_requested,
        "取消要求が記録される"
    );

    let again: Result<(), ErrorCode> = envelope::cancel_order(
        &pic,
        core,
        caller,
        &session.clone(),
        accepted.order_id.clone(),
    )
    .expect("call");
    again.expect("cancel is idempotent");

    let unknown: Result<(), ErrorCode> =
        envelope::cancel_order(&pic, core, caller, &session.clone(), blob(&[9u8; 32]))
            .expect("call");
    assert!(unknown.is_err(), "不明な注文は拒否する");

    let other_cancel: Result<(), ErrorCode> = envelope::cancel_order(
        &pic,
        core,
        principal(114),
        &session.clone(),
        accepted.order_id.clone(),
    )
    .expect("call");
    assert!(
        matches!(other_cancel, Err(ErrorCode::Unauthenticated { .. })),
        "{other_cancel:?}"
    );

    // 別principalのセッションでは受付できない。
    let other: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        principal(112),
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"order-3", "BTC", "0.01", "60000"),
        ),
    )
    .expect("call");
    assert!(
        matches!(other, Err(ErrorCode::Unauthenticated { .. })),
        "{other:?}"
    );
    let revoked: Result<(), ErrorCode> =
        update(&pic, vault, caller, "revoke_session", session.clone()).expect("revoke call");
    revoked.expect("revoke");
    let denied: Result<api_types::envelope::OrderRequestStatus, ErrorCode> = lookup
        .call(&pic, core, caller, "get_order_by_request", &request)
        .expect("lookup revoked");
    assert!(matches!(denied, Err(ErrorCode::SessionRevoked)));
}

/// Agent鍵はcoreが導出・保管する（`Implementation.md` 7章）。
#[test]
fn core_derives_agent_keys_for_the_account() {
    let pic = pic();
    let controller = principal(116);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);

    let caller = principal(117);
    let session = open_session(&pic, vault, caller, &secret(183));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"agent-alloc-fund",
        5_000_000_000,
        41,
    );

    let requested: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let requested = requested.expect("generation");
    assert_eq!(requested.generation, 1);
    assert_eq!(requested.agent_address.len(), 20, "coreが導出したアドレス");
    assert_eq!(requested.state, api_types::fund::AgentState::Requested);

    // 再要求は同じ世代（未承認のうちは増やさない）。
    let again: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    assert_eq!(again.expect("generation").generation, 1);

    let status: Result<api_types::fund::AgentStatus, ErrorCode> =
        update_args(&pic, core, caller, "get_agent_status", (session.clone(),)).expect("call");
    let status = status.expect("status");
    assert!(
        status.current.is_none(),
        "vaultがmaster署名で承認するまでは未承認"
    );
    assert_eq!(status.next.expect("next").generation, 1);

    // vaultで承認すると、coreの状態表示も承認済みになる（承認はvaultが永続化する）。
    let approved: Result<api_types::fund::AgentGeneration, ErrorCode> = call_with_mocked_outcall(
        &pic,
        vault,
        caller,
        "approve_agent_generation",
        (
            session.clone(),
            1u64,
            api_types::Blob::from(requested.agent_address.as_ref().to_vec()),
        ),
        Ok((
            200,
            br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
        )),
    )
    .expect("call");
    assert_eq!(
        approved.expect("approved").state,
        api_types::fund::AgentState::Active
    );

    let status: Result<api_types::fund::AgentStatus, ErrorCode> =
        update_args(&pic, core, caller, "get_agent_status", (session.clone(),)).expect("call");
    let status = status.expect("status");
    let current = status.current.expect("vaultの承認が反映される");
    assert_eq!(current.generation, 1);
    assert_eq!(current.state, api_types::fund::AgentState::Active);

    // 別principalは世代を要求できない。
    let denied: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        principal(118),
        "request_agent_generation",
        session,
    )
    .expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

/// 注文actionはcoreのAgent鍵で署名され、そのアドレスへ復元できる。
#[test]
fn core_signs_orders_with_the_agent_key() {
    let pic = pic();
    let controller = principal(119);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(120);
    let session = open_session(&pic, vault, caller, &secret(184));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"sign-alloc-fund",
        5_000_000_000,
        51,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);

    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    // 未承認の世代では署名しないため、vaultで承認しておく。
    let approved = approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approved");
    assert_eq!(approved.state, api_types::fund::AgentState::Active);

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"sign-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    let submitted = submitted.expect("accepted order");

    let signed: Result<(Vec<u8>, Vec<u8>), ErrorCode> = update(
        &pic,
        core,
        caller,
        "test_sign_order_action",
        submitted.order_id.clone(),
    )
    .expect("call");
    let (digest, signature) = signed.expect("signature");
    let digest: [u8; 32] = digest.try_into().expect("digest");
    let signature =
        hl_sign::Signature::from_bytes65(&signature.try_into().expect("65-byte signature"))
            .expect("signature");

    let recovered =
        hl_sign::signature::recover_address(&digest, &signature, None).expect("recover");
    assert_eq!(
        recovered.to_vec(),
        agent_address.as_ref().to_vec(),
        "Agent鍵で署名されている"
    );
}

/// 受付けた注文はAgent鍵で署名されて送信され、取引所のoidを記録する。
#[test]
fn orders_are_dispatched_and_record_the_venue_oid() {
    let pic = pic();
    let controller = principal(121);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(122);
    let session = open_session(&pic, vault, caller, &secret(185));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"dispatch-alloc-fund",
        5_000_000_000,
        61,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"dispatch-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let venue_body = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":12345}}]}}}"#.to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, venue_body.clone()).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let listed = listed.expect("list");
    assert_eq!(listed.items[0].state, api_types::order::OrderState::Open);
    assert_eq!(listed.items[0].hl_oid, Some(12345));
    let configured: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_send_journal", ())
            .expect("journal configured");
    let journal = configured.expect("journal principal").expect("configured");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, core, "recovery_events", (0u64, 10u32))
            .expect("recovery events");
    let records = records.expect("private recovery events");
    assert!(records.iter().any(|record| {
        matches!(
            &record.event.payload,
            RecoveryPayload::OrderActionResult {
                kind,
                accepted: true,
                hl_oid: Some(12345),
                ..
            } if kind == "order"
        )
    }));
    assert!(records.iter().any(|record| {
        matches!(
            &record.event.payload,
            RecoveryPayload::IdentityAccount {
                account_id: recorded_account,
                owner,
                address,
                ..
            } if recorded_account == &account_id && *owner == caller && address.len() == 20
        )
    }));
    // 取引所データの鮮度：観測が新しければ新規リスクを受け付ける。
    let state = r#"{"marginSummary":{"totalMarginUsed":"0"},"assetPositions":[{"position":{"coin":"ETH","szi":"0.05","entryPx":"2500","unrealizedPnl":"0","leverage":{"value":3}}}]}"#;
    let ingested: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_positions",
        (session.clone(), state.to_string()),
    )
    .expect("call");
    ingested.expect("ingested");
    let fresh: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"stale-fresh", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    fresh.expect("fresh data allows new risk");

    // 11秒経過すると新規リスクを拒否する。
    pic.advance_time(std::time::Duration::from_secs(11));
    pic.tick();
    let stale: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"stale-old", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    assert_eq!(
        stale.expect_err("古い観測では新規注文を拒否"),
        ErrorCode::NotAllowed {
            code: NotAllowedCode::OperationNotAvailable,
        }
    );

    // 再取り込みで新しくなれば再び受け付ける。
    let refreshed: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_positions",
        (session.clone(), state.to_string()),
    )
    .expect("call");
    refreshed.expect("refreshed");
    let again: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"stale-again", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    again.expect("fresh data allows new risk again");
}

/// 取引所の拒否と応答喪失を正しく分類し、不明な注文は再送しない。
#[test]
fn rejected_and_uncertain_orders_are_classified_without_resending() {
    let pic = pic();
    let controller = principal(123);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(124);
    let session = open_session(&pic, vault, caller, &secret(186));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"outcome-alloc-fund",
        5_000_000_000,
        71,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");

    let submit = |request_id: &[u8]| -> Result<SubmitOrderResult, ErrorCode> {
        update_args(
            &pic,
            core,
            caller,
            "submit_order",
            (
                session.clone(),
                order_args(&account_id, &session, request_id, "ETH", "0.05", "2500"),
            ),
        )
        .expect("call")
    };

    // 取引所の拒否 → rejected（再送しない）。
    submit(b"outcome-reject").expect("accepted order");
    let rejected = br#"{"status":"err","response":"insufficient margin"}"#.to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, rejected).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    assert_eq!(
        listed.expect("list").items[0].state,
        api_types::order::OrderState::Rejected
    );

    // 応答喪失 → unknown（再送しない）。
    submit(b"outcome-unknown").expect("accepted order");
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        pocket_ic_tests::sweep_with_failed_send(&pic, core, caller).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let listed = listed.expect("list");
    assert_eq!(listed.items[0].state, api_types::order::OrderState::Unknown);

    // どちらも再送しない。
    let accepted =
        br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":1}}]}}}"#
            .to_vec();
    let swept_again: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, accepted).expect("call");
    let again = swept_again.expect("sweep");
    assert_eq!((again.dispatched, again.cancels), (0, 0), "自動再送しない");
}

/// 口座snapshotはvaultの残高とcoreの注文を統合して返す。
#[test]
fn the_snapshot_merges_vault_balances_and_core_orders() {
    let pic = pic();
    let controller = principal(125);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(126);
    let session = open_session(&pic, vault, caller, &secret(187));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"snapshot-alloc-fund",
        5_000_000_000,
        81,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"snapshot-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let snapshot: Result<api_types::order::AccountSnapshot, ErrorCode> =
        envelope::get_account_snapshot(&pic, core, caller, &session).expect("call");
    let snapshot = snapshot.expect("snapshot");
    assert_eq!(snapshot.account_id.len(), 32);
    assert_eq!(
        snapshot.withdrawable, 5_000_000_000,
        "vaultの出金可能額（入金1,000,000 − 予約300,000）"
    );
    assert_eq!(
        snapshot.equity, 5_000_000_000,
        "取引口座へ着金済みのequityが返る"
    );
    assert!(snapshot.open_orders.is_empty());
    assert_eq!(
        snapshot.pending_orders.len(),
        1,
        "受付済み注文はpendingとして出る"
    );
    assert!(snapshot.positions.is_empty());
}

/// 約定一覧は現状空で、認可を要する。
#[test]
fn fills_are_listed_only_for_the_authorized_caller() {
    let pic = pic();
    let controller = principal(127);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    // 封筒はnetworkをaadへ束縛するため、market context（network・dex）も必要とする。
    let context: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");
    rotate_hpke_key(&pic, core, controller);

    let caller = principal(128);
    let session = open_session(&pic, vault, caller, &secret(188));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"fills-alloc-fund",
        5_000_000_000,
        91,
    );

    let fills: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let fills = fills.expect("fills");
    assert!(fills.items.is_empty(), "約定の取り込みは次段階（現状は空）");

    // 別principalは取得できない。
    let denied: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(
            &pic,
            core,
            principal(129),
            &session.clone(),
            None::<Blob>,
            10u32,
        )
        .expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

/// `/info`照合の取り込みは冪等で、約定一覧と注文状態に反映される。
#[test]
fn fills_are_ingested_idempotently() {
    let pic = pic();
    let controller = principal(132);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(133);
    let session = open_session(&pic, vault, caller, &secret(190));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"fills-ingest-alloc-fund",
        5_000_000_000,
        111,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");
    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(
                &account_id,
                &session,
                b"fills-ingest",
                "ETH",
                "0.05",
                "2500",
            ),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let venue_body = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":777}}]}}}"#.to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, venue_body).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    let fills = r#"[{"tid":1,"oid":777,"coin":"ETH","px":"2500","sz":"0.02","fee":"0.000012","time":1758000000000}]"#;
    let journal: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    pic.stop_canister(journal, Some(controller)).unwrap();
    let blocked: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (session.clone(), fills.to_string()),
    )
    .expect("call");
    assert!(
        blocked.is_err(),
        "journal outage must block fill and risk release"
    );
    let before: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(&pic, core, caller, &session, None::<Blob>, 10u32).unwrap();
    assert!(before.unwrap().items.is_empty());
    pic.start_canister(journal, Some(controller)).unwrap();
    let wrong_market: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (
            session.clone(),
            r#"[{"tid":2,"oid":777,"coin":"BTC","px":"2500","sz":"0.05","fee":"0.000012","time":1758000000000}]"#
                .to_string(),
        ),
    )
    .expect("call");
    assert_eq!(wrong_market.unwrap(), 0, "market must match the order");
    let bad_size: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (
            session.clone(),
            r#"[{"tid":4,"oid":777,"coin":"ETH","px":"2500","sz":"-0.01","fee":"0.000001","time":1758000000000}]"#
                .to_string(),
        ),
    )
    .expect("call");
    assert!(bad_size.is_err(), "invalid fill cannot reach the journal");
    let ingested: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (session.clone(), fills.to_string()),
    )
    .expect("call");
    assert_eq!(ingested.expect("ingest"), 1);

    let again: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (session.clone(), fills.to_string()),
    )
    .expect("call");
    assert_eq!(again.expect("ingest"), 0, "同じtidは二重計上しない");
    let partial: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session, None::<Blob>, 10u32).unwrap();
    let partial = partial.unwrap();
    assert_eq!(
        partial.items[0].state,
        api_types::order::OrderState::PartiallyFilled
    );
    assert_eq!(partial.items[0].filled_quantity, "0.02");
    let final_fill: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (
            session.clone(),
            r#"[{"tid":3,"oid":777,"coin":"ETH","px":"2500","sz":"0.03","fee":"0.000018","time":1758000000001}]"#
                .to_string(),
        ),
    )
    .expect("call");
    assert_eq!(final_fill.expect("second partial fill"), 1);
    let recovery: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, core, "recovery_events", (0u64, 100u32)).unwrap();
    assert_eq!(
        recovery
            .unwrap()
            .into_iter()
            .filter(|record| matches!(record.event.payload, RecoveryPayload::FillObserved { .. }))
            .count(),
        2,
        "duplicate fill must not append twice"
    );

    let listed: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let listed = listed.expect("fills");
    assert_eq!(listed.items.len(), 2);
    assert_eq!(listed.items[0].market, "ETH");
    assert!(
        listed
            .items
            .iter()
            .any(|item| item.quantity == "0.02" && item.fee == 12)
    );
    assert!(
        listed
            .items
            .iter()
            .any(|item| item.quantity == "0.03" && item.fee == 18)
    );

    let orders: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let orders = orders.expect("orders");
    assert_eq!(orders.items[0].state, api_types::order::OrderState::Filled);
    assert_eq!(orders.items[0].filled_quantity, "0.05");
}

/// `orderStatus`照合の結果（取消など）が注文状態へ反映される。
#[test]
fn order_status_updates_are_reflected() {
    let pic = pic();
    let controller = principal(134);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(135);
    let session = open_session(&pic, vault, caller, &secret(191));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"status-alloc-fund",
        5_000_000_000,
        121,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");
    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"status-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let venue_body = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":888}}]}}}"#.to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, venue_body).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    let guard: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_journal_guard", ()).unwrap();
    let guard = guard.unwrap().expect("configured recovery guard");
    let hidden: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, caller, "get_journal_guard", ()).unwrap();
    assert!(matches!(hidden, Err(ErrorCode::Unauthenticated { .. })));
    let before_status_snapshot = pic
        .take_canister_snapshot(core, Some(controller), None)
        .expect("snapshot before status reconciliation");

    let journal: Result<Option<Principal>, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "get_send_journal", ()).unwrap();
    let journal = journal.unwrap().unwrap();
    pic.stop_canister(journal, Some(controller)).unwrap();
    let blocked: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_apply_order_status",
        (
            session.clone(),
            r#"{"status":"canceled","order":{"oid":888}}"#.to_string(),
        ),
    )
    .expect("call");
    assert!(
        blocked.is_err(),
        "journal outage must not release order risk"
    );
    let before: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session, None::<Blob>, 10u32).unwrap();
    assert_eq!(
        before.unwrap().items[0].state,
        api_types::order::OrderState::Open
    );
    pic.start_canister(journal, Some(controller)).unwrap();
    let applied: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_apply_order_status",
        (
            session.clone(),
            r#"{"status":"canceled","order":{"oid":888}}"#.to_string(),
        ),
    )
    .expect("call");
    assert!(applied.expect("applied"), "既知のoidへ反映される");
    let records: Result<Vec<RecoveryRecord>, ErrorCode> =
        update_args(&pic, journal, core, "recovery_events", (0u64, 100u32)).unwrap();
    assert_eq!(
        records
            .unwrap()
            .into_iter()
            .filter(|record| {
                matches!(
                    record.event.payload,
                    RecoveryPayload::OrderStatusObserved { .. }
                )
            })
            .count(),
        1
    );

    let orders: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    assert_eq!(
        orders.expect("orders").items[0].state,
        api_types::order::OrderState::Cancelled
    );

    // An older open observation cannot return a terminal order to active risk.
    let stale_open: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_apply_order_status",
        (
            session.clone(),
            r#"{"status":"open","order":{"oid":888}}"#.to_string(),
        ),
    )
    .expect("call");
    assert!(!stale_open.expect("stale open ignored"));
    let conflicting_terminal: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_apply_order_status",
        (
            session.clone(),
            r#"{"status":"filled","order":{"oid":888}}"#.to_string(),
        ),
    )
    .expect("call");
    assert!(!conflicting_terminal.expect("terminal order remains terminal"));

    // A fill arriving after the cancellation is still recorded, but must not
    // turn the cancelled order back into a live partial order.
    let late_fill: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (
            session.clone(),
            r#"[{"tid":1801,"oid":888,"coin":"ETH","px":"2500","sz":"0.01","fee":"0.000001","time":1758000000000}]"#
                .to_string(),
        ),
    )
    .expect("call");
    assert_eq!(late_fill.expect("late fill recorded"), 1);
    let another_fill: Result<u32, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_ingest_fills",
        (
            session.clone(),
            r#"[{"tid":1802,"oid":888,"coin":"ETH","px":"2500","sz":"0.02","fee":"0.000001","time":1758000000001}]"#
                .to_string(),
        ),
    )
    .expect("call");
    assert_eq!(another_fill.expect("second fill recorded"), 1);
    let after_fill: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session, None::<Blob>, 10u32).unwrap();
    let after_fill = after_fill.unwrap();
    assert_eq!(
        after_fill.items[0].state,
        api_types::order::OrderState::Cancelled
    );
    assert_eq!(after_fill.items[0].filled_quantity, "0.03");

    // 未知のoidは何も変えない。
    let unknown: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "test_apply_order_status",
        (
            session.clone(),
            r#"{"status":"canceled","order":{"oid":999999}}"#.to_string(),
        ),
    )
    .expect("call");
    assert!(!unknown.expect("applied"));

    pic.load_canister_snapshot(core, Some(controller), before_status_snapshot.id)
        .expect("restore older core snapshot");
    let resume: Result<(), ErrorCode> =
        update(&pic, guard, controller, "resume_journal", core).unwrap();
    assert!(
        matches!(resume, Err(ErrorCode::PolicyUnavailable)),
        "replayed status still needs external validation"
    );
    let restored: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session, None::<Blob>, 10u32).unwrap();
    let restored = restored.unwrap();
    assert_eq!(
        restored.items[0].state,
        api_types::order::OrderState::Cancelled,
        "journal delta replays the terminal order state"
    );
    assert_eq!(restored.items[0].filled_quantity, "0.03");
    let replayed_fills: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> =
        envelope::list_fills(&pic, core, caller, &session, None::<Blob>, 10u32).unwrap();
    assert_eq!(replayed_fills.unwrap().items.len(), 2);
    let replay_pending: Result<bool, ErrorCode> =
        pocket_ic_tests::query(&pic, core, controller, "recovery_replay_pending", ()).unwrap();
    assert!(replay_pending.unwrap());
}

/// 政策Canisterのprincipalはcontrollerだけが設定できる。
#[test]
fn the_policy_principal_is_settable_by_controllers() {
    let pic = pic();
    let controller = principal(138);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let policy = deploy(
        &pic,
        POLICY_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );

    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_policy_principal", policy).expect("call");
    set.expect("set_policy_principal");
    let stored =
        query_args::<_, Option<Principal>>(&pic, core, principal(139), "get_policy_principal", ())
            .expect("call");
    assert_eq!(stored, Some(policy));

    let denied: Result<(), ErrorCode> =
        update(&pic, core, principal(140), "set_policy_principal", policy).expect("call");
    assert!(
        matches!(denied, Err(ErrorCode::Unauthenticated { .. })),
        "{denied:?}"
    );
}

/// 取消はAgent鍵で署名されて取引所へ送られ、受理で`cancelled`になる。
#[test]
fn a_cancellation_is_dispatched_to_the_venue() {
    let pic = pic();
    let controller = principal(143);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    // 停止状態とallowlistはpolicyへ照会する（未設定はfail-closed）ため、用意する。
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let caller = principal(144);
    let session = open_session(&pic, vault, caller, &secret(195));
    // 取引口座へ着金させてequityを作る（注文はequityに対してリスク上限を検査する）。
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller,
        &session,
        b"cancel-alloc-fund",
        5_000_000_000,
        211,
    );
    let account_id = trading_account_id(&pic, vault, caller, &session);
    observe_empty_account(&pic, core, caller, &session);
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller, &session, 1, agent_address.as_ref())
        .expect("approve agent");

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"cancel-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    let submitted = submitted.expect("accepted order");
    let venue_body: Vec<u8> = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":999}}]}}}"#.to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, venue_body.clone()).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    // 取消を要求し、sweepで取引所へ送る。
    let cancelled: Result<(), ErrorCode> = envelope::cancel_order(
        &pic,
        core,
        caller,
        &session.clone(),
        submitted.order_id.clone(),
    )
    .expect("call");
    cancelled.expect("cancel requested");
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> = sweep_with_venue_outcalls(
        &pic,
        core,
        caller,
        br#"{"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}}"#.to_vec(),
    )
    .expect("call");
    assert_eq!(swept.expect("cancel sweep").cancels, 1, "取消が送信される");

    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    assert_eq!(
        listed.expect("orders").items[0].state,
        api_types::order::OrderState::Cancelled
    );

    // 2件目を出してから一括取消（Cancel All）を要求する。
    let second: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&account_id, &session, b"cancel-2", "ETH", "0.02", "2400"),
        ),
    )
    .expect("call");
    second.expect("accepted order");
    // 取引所のoidは注文ごとに異なる（同じoidを使い回すと照合が取り違える）。
    let second_body: Vec<u8> = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":1000}}]}}}"#.to_vec();
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller, second_body).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    let marked: Result<u64, ErrorCode> =
        update(&pic, core, caller, "cancel_all", session.clone()).expect("call");
    assert_eq!(
        marked.expect("cancel all"),
        1,
        "未終端の注文に取消要求が付く"
    );
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> = sweep_with_venue_outcalls(
        &pic,
        core,
        caller,
        br#"{"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}}"#.to_vec(),
    )
    .expect("call");
    assert_eq!(swept.expect("cancel sweep").cancels, 1);

    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller, &session.clone(), None::<Blob>, 10u32)
            .expect("call");
    let listed = listed.expect("orders");
    assert!(
        listed
            .items
            .iter()
            .all(|order| order.state == api_types::order::OrderState::Cancelled),
        "一括取消で全て取消済みになる: {:?}",
        listed
            .items
            .iter()
            .map(|order| (
                order.state,
                order.cancel_requested,
                order.hl_oid,
                order.dispatch_state
            ))
            .collect::<Vec<_>>()
    );
}

/// 取引所のoidが同じでも、他人の注文の状態は変わらない。
///
/// 取引所のoidは口座ごとに採番されるため、口座で絞らずにoidだけで更新すると
/// 他人の注文を書き換え得る（`apply_order_status` の所有者スコープの回帰試験）。
#[test]
fn order_status_is_scoped_to_the_account() {
    let pic = pic();
    let controller = principal(130);
    let vault = deploy(
        &pic,
        FUNDS_VAULT_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
    configure_policy(&pic, core, controller, &["BTC", "ETH"]);
    rotate_hpke_key(&pic, core, controller);
    let meta: Result<(), ErrorCode> = update_args(
        &pic,
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
        &pic,
        core,
        controller,
        "set_market_context",
        ("local".to_string(), "hyperliquid".to_string()),
    )
    .expect("call");
    context.expect("set_market_context");

    let venue_body =
        br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":777}}]}}}"#
            .to_vec();

    // 利用者A: 入金→発注→送信（oid 777で受理）。
    let caller_a = principal(131);
    let session_a = open_session(&pic, vault, caller_a, &secret(201));
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller_a,
        &session_a,
        b"scope-a",
        5_000_000_000,
        202,
    );
    let account_id_a = trading_account_id(&pic, vault, caller_a, &session_a);
    observe_empty_account(&pic, core, caller_a, &session_a);
    let agent_a: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller_a,
        "request_agent_generation",
        session_a.clone(),
    )
    .expect("call");
    let agent_a = agent_a.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller_a, &session_a, 1, agent_a.as_ref())
        .expect("approved");

    let submitted_a: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller_a,
        "submit_order",
        (
            session_a.clone(),
            order_args(
                &account_id_a,
                &session_a,
                b"scope-order-a",
                "ETH",
                "0.05",
                "2500",
            ),
        ),
    )
    .expect("call");
    submitted_a.expect("accepted order");
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller_a, venue_body.clone()).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    // 利用者B: 同じoidを返すmockで受理させる。
    let caller_b = principal(132);
    let session_b = open_session(&pic, vault, caller_b, &secret(203));
    fund_trading_account(
        &pic,
        vault,
        controller,
        caller_b,
        &session_b,
        b"scope-b",
        5_000_000_000,
        204,
    );
    let account_id_b = trading_account_id(&pic, vault, caller_b, &session_b);
    observe_empty_account(&pic, core, caller_b, &session_b);
    let agent_b: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller_b,
        "request_agent_generation",
        session_b.clone(),
    )
    .expect("call");
    let agent_b = agent_b.expect("agent").agent_address;
    approve_agent_at_vault(&pic, vault, caller_b, &session_b, 1, agent_b.as_ref())
        .expect("approved");

    let submitted_b: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller_b,
        "submit_order",
        (
            session_b.clone(),
            order_args(
                &account_id_b,
                &session_b,
                b"scope-order-b",
                "ETH",
                "0.05",
                "2500",
            ),
        ),
    )
    .expect("call");
    submitted_b.expect("accepted order");
    let swept: Result<api_types::order::SweepOutcome, ErrorCode> =
        sweep_with_venue_outcalls(&pic, core, caller_b, venue_body).expect("call");
    assert_eq!(swept.expect("sweep").dispatched, 1);

    // Bが同じoid(777)のorderStatusを適用する。
    let applied: Result<bool, ErrorCode> = update_args(
        &pic,
        core,
        caller_b,
        "test_apply_order_status",
        (
            session_b.clone(),
            r#"{"status":"filled","order":{"oid":777}}"#.to_string(),
        ),
    )
    .expect("call");
    applied.expect("applied");

    // Aの注文はopenのまま、Bの注文だけがfilledになる。
    let listed_a: Result<api_types::Paged<OrderSummary>, ErrorCode> = envelope::list_orders(
        &pic,
        core,
        caller_a,
        &session_a.clone(),
        None::<Blob>,
        10u32,
    )
    .expect("call");
    let listed_a = listed_a.expect("orders");
    assert!(
        listed_a
            .items
            .iter()
            .all(|order| order.state == api_types::order::OrderState::Open),
        "他人のoidでAの注文を書き換えない: {:?}",
        listed_a.items.iter().map(|o| o.state).collect::<Vec<_>>()
    );

    let listed_b: Result<api_types::Paged<OrderSummary>, ErrorCode> =
        envelope::list_orders(&pic, core, caller_b, &session_b, None::<Blob>, 10u32).expect("call");
    let listed_b = listed_b.expect("orders");
    assert!(
        listed_b
            .items
            .iter()
            .any(|order| order.state == api_types::order::OrderState::Filled),
        "本人の注文は更新される"
    );
}

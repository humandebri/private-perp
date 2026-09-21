//! `trading_core` の注文受付（認可・allowlist・精度・冪等性）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{ErrorCode, NotAllowedCode};
use api_types::order::{OrderKind, OrderSummary, Side, SubmitOrderArgs, SubmitOrderResult};
use api_types::{Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, POLICY_WASM, TRADING_CORE_WASM, approve_agent_at_vault,
    call_with_mocked_outcall, configure_policy, deploy, fund_trading_account, pic, principal,
    query_args, update, update_args,
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

fn order_args(
    session: &SessionHandle,
    request_id: &[u8],
    market: &str,
    quantity: &str,
    price: &str,
) -> SubmitOrderArgs {
    SubmitOrderArgs {
        session: session.clone(),
        client_request_id: blob(request_id),
        account_id: blob(&[0u8; 32]),
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

    // 銘柄解決の設定が無い状態ではfail-closedで拒否する（固定値を使わない）。
    let no_context: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"order-ctx", "ETH", "0.05", "2500"),
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

    // 受付できる（ETHはmetaの添字1）。
    let accepted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"order-1", "ETH", "0.05", "2500"),
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
            order_args(&session, b"order-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    let duplicate = duplicate.expect("duplicate");
    assert_eq!(duplicate.order_id, accepted.order_id);
    assert_eq!(duplicate.cloid, accepted.cloid);

    // 同一ID・異なる本文は拒否する。
    let conflict: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"order-1", "ETH", "0.06", "2500"),
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
            order_args(&session, b"order-2", "SOL", "1", "100"),
        ),
    )
    .expect("call");
    assert_eq!(
        denied.expect_err("denied"),
        ErrorCode::NotAllowed {
            code: NotAllowedCode::AssetNotAllowed
        }
    );

    // 一覧は新しい順に返り、別principalのセッションでは取得できない。
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
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

    let denied_list: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        principal(113),
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    assert!(
        matches!(denied_list, Err(ErrorCode::Unauthenticated { .. })),
        "{denied_list:?}"
    );

    // 取消要求は冪等で、他者の注文や不明なIDは拒否する。
    let cancel: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "cancel_order",
        (session.clone(), accepted.order_id.clone()),
    )
    .expect("call");
    cancel.expect("cancel");

    let after_cancel: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    assert!(
        after_cancel.expect("list").items[0].cancel_requested,
        "取消要求が記録される"
    );

    let again: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "cancel_order",
        (session.clone(), accepted.order_id.clone()),
    )
    .expect("call");
    again.expect("cancel is idempotent");

    let unknown: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "cancel_order",
        (session.clone(), blob(&[9u8; 32])),
    )
    .expect("call");
    assert!(unknown.is_err(), "不明な注文は拒否する");

    let other_cancel: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        principal(114),
        "cancel_order",
        (session.clone(), accepted.order_id.clone()),
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
            order_args(&session, b"order-3", "BTC", "0.01", "60000"),
        ),
    )
    .expect("call");
    assert!(
        matches!(other, Err(ErrorCode::Unauthenticated { .. })),
        "{other:?}"
    );
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
            order_args(&session, b"sign-1", "ETH", "0.05", "2500"),
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
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"dispatch-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let venue_body = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":12345}}]}}}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, venue_body.clone())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let listed = listed.expect("list");
    assert_eq!(listed.items[0].state, api_types::order::OrderState::Open);
    assert_eq!(listed.items[0].hl_oid, Some(12345));
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
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");

    let submit = |request_id: &[u8]| -> Result<SubmitOrderResult, ErrorCode> {
        update_args(
            &pic,
            core,
            caller,
            "submit_order",
            (
                session.clone(),
                order_args(&session, request_id, "ETH", "0.05", "2500"),
            ),
        )
        .expect("call")
    };

    // 取引所の拒否 → rejected（再送しない）。
    submit(b"outcome-reject").expect("accepted order");
    let rejected = br#"{"status":"err","response":"insufficient margin"}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, rejected)),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    assert_eq!(
        listed.expect("list").items[0].state,
        api_types::order::OrderState::Rejected
    );

    // 応答喪失 → unknown（再送しない）。
    submit(b"outcome-unknown").expect("accepted order");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Err((3, "outcall failed".to_string())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);
    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let listed = listed.expect("list");
    assert_eq!(listed.items[0].state, api_types::order::OrderState::Unknown);

    // どちらも再送しない。
    let swept_again: Result<u32, ErrorCode> =
        update_args(&pic, core, caller, "test_sweep_now", ()).expect("call");
    assert_eq!(swept_again.expect("sweep"), 0, "自動再送しない");
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

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"snapshot-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let snapshot: Result<api_types::order::AccountSnapshot, ErrorCode> =
        update(&pic, core, caller, "get_account_snapshot", session.clone()).expect("call");
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

    let fills: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_fills",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let fills = fills.expect("fills");
    assert!(fills.items.is_empty(), "約定の取り込みは次段階（現状は空）");

    // 別principalは取得できない。
    let denied: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> = update_args(
        &pic,
        core,
        principal(129),
        "list_fills",
        (session.clone(), None::<Blob>, 10u32),
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
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");
    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"fills-ingest", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let venue_body = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":777}}]}}}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, venue_body)),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let fills = r#"[{"tid":1,"oid":777,"coin":"ETH","px":"2500","sz":"0.05","fee":12,"time":1758000000000}]"#;
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

    let listed: Result<api_types::Paged<api_types::order::FillView>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_fills",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let listed = listed.expect("fills");
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].market, "ETH");
    assert_eq!(listed.items[0].quantity, "0.05");
    assert_eq!(listed.items[0].fee, 12);

    let orders: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
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
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");
    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"status-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted.expect("accepted order");

    let venue_body = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":888}}]}}}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, venue_body)),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

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

    let orders: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    assert_eq!(
        orders.expect("orders").items[0].state,
        api_types::order::OrderState::Cancelled
    );

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
    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    agent.expect("agent");

    let submitted: Result<SubmitOrderResult, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "submit_order",
        (
            session.clone(),
            order_args(&session, b"cancel-1", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    let submitted = submitted.expect("accepted order");
    let venue_body: Vec<u8> = br#"{"status":"ok","response":{"type":"default","data":{"statuses":[{"resting":{"oid":999}}]}}}"#.to_vec();
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, venue_body.clone())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    // 取消を要求し、sweepで取引所へ送る。
    let cancelled: Result<(), ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "cancel_order",
        (session.clone(), submitted.order_id.clone()),
    )
    .expect("call");
    cancelled.expect("cancel requested");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((
            200,
            br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
        )),
    )
    .expect("call");
    assert_eq!(swept.expect("cancel sweep"), 1, "取消が送信される");

    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
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
            order_args(&session, b"cancel-2", "ETH", "0.02", "2400"),
        ),
    )
    .expect("call");
    second.expect("accepted order");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((200, venue_body.clone())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

    let marked: Result<u64, ErrorCode> =
        update(&pic, core, caller, "cancel_all", session.clone()).expect("call");
    assert_eq!(
        marked.expect("cancel all"),
        1,
        "未終端の注文に取消要求が付く"
    );
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller,
        "test_sweep_now",
        (),
        Ok((
            200,
            br#"{"status":"ok","response":{"type":"default"}}"#.to_vec(),
        )),
    )
    .expect("call");
    assert_eq!(swept.expect("cancel sweep"), 1);

    let listed: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller,
        "list_orders",
        (session.clone(), None::<Blob>, 10u32),
    )
    .expect("call");
    let listed = listed.expect("orders");
    assert!(
        listed
            .items
            .iter()
            .all(|order| order.state == api_types::order::OrderState::Cancelled),
        "一括取消で全て取消済みになる"
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
            order_args(&session_a, b"scope-order-a", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted_a.expect("accepted order");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller_a,
        "test_sweep_now",
        (),
        Ok((200, venue_body.clone())),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

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
            order_args(&session_b, b"scope-order-b", "ETH", "0.05", "2500"),
        ),
    )
    .expect("call");
    submitted_b.expect("accepted order");
    let swept: Result<u32, ErrorCode> = call_with_mocked_outcall(
        &pic,
        core,
        caller_b,
        "test_sweep_now",
        (),
        Ok((200, venue_body)),
    )
    .expect("call");
    assert_eq!(swept.expect("sweep"), 1);

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
    let listed_a: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller_a,
        "list_orders",
        (session_a.clone(), None::<Blob>, 10u32),
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

    let listed_b: Result<api_types::Paged<OrderSummary>, ErrorCode> = update_args(
        &pic,
        core,
        caller_b,
        "list_orders",
        (session_b, None::<Blob>, 10u32),
    )
    .expect("call");
    let listed_b = listed_b.expect("orders");
    assert!(
        listed_b
            .items
            .iter()
            .any(|order| order.state == api_types::order::OrderState::Filled),
        "本人の注文は更新される"
    );
}

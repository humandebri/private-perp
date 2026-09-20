//! `trading_core` の注文受付（認可・allowlist・精度・冪等性）の試験。

use api_types::auth::{
    ChallengePurpose, ChallengeRequest, ChallengeResponse, OpenSessionRequest, SessionHandle,
};
use api_types::error::{ErrorCode, NotAllowedCode};
use api_types::fund::AllocationRequest;
use api_types::order::{OrderKind, OrderSummary, Side, SubmitOrderArgs, SubmitOrderResult};
use api_types::{AccountKind, Blob, Network};
use candid::Principal;
use hl_sign::private_perp;
use hl_sign::signature::address_from_secret;
use pocket_ic::PocketIc;
use pocket_ic_tests::{
    FUNDS_VAULT_WASM, TRADING_CORE_WASM, call_with_mocked_outcall, deploy, deploy_default, pic,
    principal, update, update_args,
};

const ORIGIN: &str = "https://app.example.test";
const UNIVERSE: &str = r#"[{"name":"SOL"},{"name":"ETH"},{"name":"BTC"}]"#;

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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(110);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
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
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[31u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"alloc-for-orders"),
            amount: 500_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(116);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");

    let caller = principal(117);
    let session = open_session(&pic, vault, caller, &secret(183));
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[41u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"agent-alloc"),
            amount: 100_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

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
    assert!(status.current.is_none(), "承認はvaultがmaster署名で行う");
    assert_eq!(status.next.expect("next").generation, 1);

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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(119);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
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
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[51u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"sign-alloc"),
            amount: 200_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

    let agent: Result<api_types::fund::AgentGeneration, ErrorCode> = update(
        &pic,
        core,
        caller,
        "request_agent_generation",
        session.clone(),
    )
    .expect("call");
    let agent_address = agent.expect("agent").agent_address;

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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(121);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
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
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[61u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"dispatch-alloc"),
            amount: 300_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");
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
        Ok((200, venue_body)),
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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(123);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
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
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[71u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"outcome-alloc"),
            amount: 400_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");
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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(125);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");
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
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 1_000_000u64, blob(&[81u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"snapshot-alloc"),
            amount: 300_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

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
        snapshot.withdrawable, 700_000,
        "vaultの出金可能額（入金1,000,000 − 予約300,000）"
    );
    assert_eq!(snapshot.equity, 0, "着金の確定前は取引口座に残高が無い");
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
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let controller = principal(127);
    let core = deploy(
        &pic,
        TRADING_CORE_WASM,
        Some(vec![controller]),
        candid::encode_one(()).unwrap(),
    );
    let set: Result<(), ErrorCode> =
        update(&pic, core, controller, "set_vault_principal", vault).expect("call");
    set.expect("set_vault_principal");

    let caller = principal(128);
    let session = open_session(&pic, vault, caller, &secret(188));
    let credit: Result<(), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_credit_deposit",
        (session.clone(), 500_000u64, blob(&[91u8; 32])),
    )
    .expect("call");
    credit.expect("credit");
    let allocated: Result<api_types::fund::FundRequestAccepted, ErrorCode> = update(
        &pic,
        vault,
        caller,
        "request_allocation",
        AllocationRequest {
            session: session.clone(),
            client_request_id: blob(b"fills-alloc"),
            amount: 100_000,
            target: AccountKind::Trading,
            intent_signature: None,
        },
    )
    .expect("call");
    allocated.expect("allocation");

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

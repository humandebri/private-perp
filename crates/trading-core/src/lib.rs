//! `trading_core` Canister。
//!
//! 責務は注文・取消・決済の認可と状態機械、口座別・世代別Agent鍵、署名actionの構築と
//! 送信、Hyperliquid照合、口座snapshot配信である
//! （`docs/phase-0/api-contract.md` 3節、`docs/phase-0/state-machines.md`）。
//!
//! このCanisterはmaster鍵と出金署名を持たない。`funds_vault` へ出金や任意digest署名を
//! 要求する経路を追加しない（`docs/phase-0/authority-matrix.md` 5節）。
//!
//! 認可は `funds_vault` の `session_status` に問い合わせ、返却されたprincipalをこの
//! Canisterが受け取ったcallerと比較する（vaultはcoreのcallerを知らないため）。

use api_types::auth::{SessionHandle, SessionStatus};
use api_types::error::{BadRequestCode, ErrorCode};
use candid::Principal;
use db::error::Error as DbError;
use ic_cdk::call::Call;
use ic_cdk_management_canister::{EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs, ecdsa_public_key};
#[cfg(feature = "test-venue")]
use ic_cdk_management_canister::{
    HttpHeader, HttpMethod, HttpRequest, SignWithEcdsaArgs, sign_with_ecdsa,
};

const MEMORY_ID: u8 = db::memory_id::TRADING_CORE_MAIN;

fn internal(message: String) -> ErrorCode {
    ErrorCode::Internal { code: message }
}

fn map_db(error: DbError) -> ErrorCode {
    match error {
        DbError::Sql(message) => internal(message),
        DbError::NotFound => internal("not configured".to_string()),
        DbError::Conflict => ErrorCode::ReservationConflict,
        DbError::Overflow => internal("integer overflow".to_string()),
        DbError::Invariant(message) => internal(message.to_string()),
        DbError::InsufficientFunds { .. } => internal("unexpected funds error".to_string()),
        DbError::StateConflict { .. } => ErrorCode::ReservationConflict,
    }
}

/// このビルドのバージョン。デプロイ確認用。
#[ic_cdk::query]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// vaultのprincipalを設定する（controllerのみ）。
#[ic_cdk::update]
fn set_vault_principal(vault: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the vault principal".to_string(),
        });
    }
    let bytes = vault.as_slice().to_vec();
    if bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid vault principal".to_string(),
        });
    }
    db::tx::update(|connection| db::repo::core_config::set_vault_principal(connection, &bytes))
        .map_err(map_db)
}

/// 政策Canisterのprincipalを設定する（controllerのみ）。
#[ic_cdk::update]
fn set_policy_principal(policy: Principal) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the policy principal".to_string(),
        });
    }
    let bytes = policy.as_slice().to_vec();
    if bytes.is_empty() || bytes.len() > 29 {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::MalformedPayload,
            detail: "invalid policy principal".to_string(),
        });
    }
    db::tx::update(|connection| db::repo::core_config::set_policy_principal(connection, &bytes))
        .map_err(map_db)
}

/// 政策Canisterのprincipal（診断用）。
#[ic_cdk::query]
fn get_policy_principal() -> Option<Principal> {
    db::tx::query(db::repo::core_config::policy_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// vaultのprincipal（診断用）。
#[ic_cdk::query]
fn get_vault_principal() -> Option<Principal> {
    db::tx::query(db::repo::core_config::vault_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// セッションを検証し、本人のuser_idを返す（認可境界の試験用）。
///
/// vaultに問い合わせ、返却されたprincipalが「このメッセージのcaller」と一致する場合だけ
/// user_idを返す。順序を逆にしない（callerを信用しない）。
#[ic_cdk::update]
async fn whoami(session: SessionHandle) -> Result<api_types::Blob, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let vault_bytes = db::tx::query(db::repo::core_config::vault_principal)
        .map_err(map_db)?
        .ok_or_else(|| internal("vault principal is not configured".to_string()))?;
    let vault = Principal::from_slice(&vault_bytes);

    let response = Call::bounded_wait(vault, "session_status")
        .with_arg(session)
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault session_status: {error}"),
        })?;
    let status: Result<SessionStatus, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let status = status?;

    if status.principal != caller {
        return Err(ErrorCode::Unauthenticated {
            reason: "session does not belong to this caller".to_string(),
        });
    }
    Ok(status.user_id)
}

/// 銘柄解決に使うnetwork・dexを設定する（controllerのみ）。
#[ic_cdk::update]
fn set_market_context(network: String, dex: String) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can set the market context".to_string(),
        });
    }
    db::tx::update(|connection| {
        db::repo::core_config::set_market_context(connection, &network, &dex)
    })
    .map_err(map_db)
}

/// `meta`の`universe`を登録する（ローカルのブートストラップ。本番はHL `/info` から取得する）。
#[ic_cdk::update]
fn set_meta_cache(network: String, dex: String, universe: String) -> Result<(), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can seed the meta cache".to_string(),
        });
    }
    let digest = hl_sign::keccak256(universe.as_bytes());
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::meta::set_universe(connection, &network, &dex, &digest, &universe, now)
    })
    .map_err(map_db)
}

/// 受付を1件処理する（認可・検証・冪等性・pending注文の登録）。
///
/// 署名・送信・照合はパイプライン（次段階）が行う。ここでは受付だけを確定させる。
#[ic_cdk::update]
async fn submit_order(
    session: SessionHandle,
    args: api_types::order::SubmitOrderArgs,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    let user_id = authorize(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;

    // 銘柄は初期allowlistのみ。asset indexはmetaから解決する（固定値を埋め込まない）。
    let market = args.market.to_uppercase();
    if market != "BTC" && market != "ETH" {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::AssetNotAllowed,
        });
    }
    let asset_index = resolve_asset_index(&market)?;

    // 数量・価格は正規化十進で検証し、丸めない。
    let quantity = hl_types::decimal::Decimal::parse(&args.quantity).map_err(bad_decimal)?;
    if quantity.as_str().starts_with('-') || quantity.as_str() == "0" {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "quantity must be positive",
        ));
    }
    match (&args.kind, &args.limit_price) {
        (api_types::order::OrderKind::MarketIoc, None) => {
            return Err(bad(
                BadRequestCode::MissingField,
                "market orders require a slippage-bounded limit price",
            ));
        }
        (_, Some(price)) => {
            let price = hl_types::decimal::Decimal::parse(price).map_err(bad_decimal)?;
            if price.as_str().starts_with('-') || price.as_str() == "0" {
                return Err(bad(
                    BadRequestCode::PriceOutOfRange,
                    "price must be positive",
                ));
            }
        }
        _ => {}
    }
    if args.leverage.unwrap_or(3) > 5 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "leverage exceeds the UI limit",
        ));
    }

    // 本文fingerprint（受付の冪等性）。
    let mut body = Vec::new();
    body.extend_from_slice(market.as_bytes());
    body.extend_from_slice(quantity.as_str().as_bytes());
    if let Some(price) = &args.limit_price {
        body.extend_from_slice(price.as_bytes());
    }
    let fingerprint = body_fingerprint(&body);

    let account_id = trading_account(&session).await?;

    let cloid = ic_cdk_management_canister::raw_rand()
        .await
        .map_err(|error| internal(format!("raw_rand failed: {error}")))?;
    let cloid: [u8; 16] = cloid
        .get(..16)
        .ok_or_else(|| internal("raw_rand too short".to_string()))?
        .try_into()
        .map_err(|_| internal("cloid length".to_string()))?;

    let order_id = {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&cloid);
        bytes.extend_from_slice(&user_id);
        body_fingerprint(&bytes)
    };

    let accepted = db::tx::update(|connection| {
        let accepted = db::repo::core_requests::accept_request(
            connection,
            &user_id,
            args.client_request_id.as_ref(),
            &fingerprint,
            now,
        )?;
        if accepted != db::repo::core_requests::AcceptOutcome::Accepted {
            return Ok(accepted);
        }
        db::repo::orders::insert_pending_order(
            connection,
            &db::repo::orders::NewOrder {
                order_id,
                user_id,
                account_id,
                client_request_id: args.client_request_id.as_ref().to_vec(),
                cloid,
                market: market.clone(),
                asset_index,
                is_buy: matches!(args.side, api_types::order::Side::Buy),
                kind: match args.kind {
                    api_types::order::OrderKind::MarketIoc => "market_ioc",
                    api_types::order::OrderKind::LimitGtc => "limit_gtc",
                }
                .to_string(),
                price: args.limit_price.clone(),
                quantity: quantity.as_str().to_string(),
                reduce_only: args.reduce_only,
            },
            now,
        )?;
        Ok(accepted)
    })
    .map_err(map_db)?;

    let (accepted_order_id, accepted_cloid) = match accepted {
        db::repo::core_requests::AcceptOutcome::Accepted => (order_id, cloid),
        db::repo::core_requests::AcceptOutcome::Duplicate => {
            // 同一ID・同一本文の再送は、最初に受付けた結果をそのまま返す。
            db::tx::query(|connection| {
                db::repo::orders::order_by_request(
                    connection,
                    &user_id,
                    args.client_request_id.as_ref(),
                )
            })
            .map_err(map_db)?
            .ok_or_else(|| internal("duplicate request without an order".to_string()))?
        }
        db::repo::core_requests::AcceptOutcome::Conflict => {
            return Err(ErrorCode::IdempotencyConflict {
                request_id: args.client_request_id,
            });
        }
    };

    Ok(api_types::order::SubmitOrderResult {
        request_id: args.client_request_id,
        order_id: accepted_order_id.to_vec().into(),
        cloid: accepted_cloid.to_vec().into(),
        accepted_at: now,
    })
}

/// Agent世代を要求する（**coreが鍵を導出・保管**し、vaultはmaster署名でアドレスを承認する）。
///
/// 未承認の世代があるうちは同じ世代を返す。注文はこの世代の鍵で署名する。
#[ic_cdk::update]
async fn request_agent_generation(
    session: SessionHandle,
) -> Result<api_types::fund::AgentGeneration, ErrorCode> {
    // 認可（caller束縛）を確認してから口座を解決する。
    authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;

    if let Some(latest) =
        db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
            .map_err(map_db)?
        && latest.state == api_types::fund::AgentState::Requested
    {
        return Ok(latest);
    }

    let generation =
        db::tx::update(|connection| db::repo::agents::latest_generation(connection, &account_id))
            .map_err(map_db)?
            .saturating_add(1);

    let path = agent_derivation_path(&account_id, generation);
    let public_key = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: path.clone(),
        key_id: ecdsa_key_id(),
    })
    .await
    .map_err(|error| internal(format!("ecdsa_public_key failed: {error}")))?
    .public_key;
    let public_key: [u8; 33] = public_key
        .try_into()
        .map_err(|_| internal("unexpected public key length".to_string()))?;
    let address = hl_sign::address_from_public_key(&public_key)
        .map_err(|error| internal(error.to_string()))?;

    db::tx::update(|connection| {
        db::repo::agents::insert_generation(
            connection,
            &account_id,
            generation,
            &address,
            "private-perp/agent",
            now,
        )
    })
    .map_err(map_db)?;

    Ok(api_types::fund::AgentGeneration {
        account_id: account_id.to_vec().into(),
        generation,
        agent_address: address.to_vec().into(),
        approved_at: None,
        expires_at: None,
        state: api_types::fund::AgentState::Requested,
    })
}

/// Agent世代の状態（承認済みは`current`、要求中は`next`）。
///
/// 認可にvaultへのinter-canister呼び出しが必要なためqueryにはできない（updateで提供）。
#[ic_cdk::update]
async fn get_agent_status(
    session: SessionHandle,
) -> Result<api_types::fund::AgentStatus, ErrorCode> {
    authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    let latest = db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
        .map_err(map_db)?;
    let (current, next) = match latest {
        Some(generation) if generation.state == api_types::fund::AgentState::Active => {
            (Some(generation), None)
        }
        other => (None, other),
    };
    Ok(api_types::fund::AgentStatus {
        current,
        next,
        revocation_pending: false,
        observed_at: now,
    })
}

/// Agent鍵の導出経路（口座と世代で分離する）。
fn agent_derivation_path(account_id: &[u8; 32], generation: u64) -> Vec<Vec<u8>> {
    vec![
        b"private-perp".to_vec(),
        b"agent".to_vec(),
        hex::encode(account_id).into_bytes(),
        generation.to_be_bytes().to_vec(),
    ]
}

fn ecdsa_key_id() -> EcdsaKeyId {
    EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: "test_key_1".to_string(),
    }
}

/// 注文の取消を要求する（署名・送信はパイプラインが行う）。
///
/// 受付と同様に冪等で、既に取消要求済み・終端状態なら何もしない。
#[ic_cdk::update]
async fn cancel_order(session: SessionHandle, order_id: api_types::Blob) -> Result<(), ErrorCode> {
    let user_id = authorize(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    let order_id: [u8; 32] = order_id.as_ref().try_into().map_err(|_| {
        bad(
            BadRequestCode::MalformedPayload,
            "order_id must be 32 bytes",
        )
    })?;

    let owner = db::tx::query(|connection| db::repo::orders::order_owner(connection, &order_id))
        .map_err(map_db)?
        .ok_or_else(|| bad(BadRequestCode::MalformedPayload, "unknown order"))?;

    let (order_user, state, cancel_requested) = owner;
    if order_user != user_id {
        return Err(ErrorCode::Unauthenticated {
            reason: "order does not belong to this caller".to_string(),
        });
    }
    if cancel_requested || is_terminal(state) {
        return Ok(());
    }

    db::tx::update(|connection| db::repo::orders::mark_cancel_requested(connection, &order_id, now))
        .map_err(map_db)
}

/// 終端状態（これ以上状態が進まない）。
fn is_terminal(state: api_types::order::OrderState) -> bool {
    use api_types::order::OrderState;
    matches!(
        state,
        OrderState::Filled | OrderState::Cancelled | OrderState::Rejected
    )
}

/// 約定一覧（新しい順）。
///
/// 認可にvaultへの問い合わせが必要なためupdateで提供する。
#[ic_cdk::update]
async fn list_fills(
    session: SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::order::FillView>, ErrorCode> {
    let user_id = authorize(&session).await?;
    let limit = limit.clamp(1, 100);
    let now = ic_cdk::api::time() / 1_000_000;
    let _ = cursor;
    let rows =
        db::tx::query(|connection| db::repo::orders::list_fills(connection, &user_id, limit))
            .map_err(map_db)?;
    Ok(api_types::Paged {
        items: rows.into_iter().map(|(_, fill)| fill).collect(),
        next_cursor: None,
        observed_at: now,
        revision: 1,
    })
}

/// 口座snapshot（残高はvault、注文はcore）。
///
/// 認可にvaultへの問い合わせが必要なためupdateで提供する。
#[ic_cdk::update]
async fn get_account_snapshot(
    session: SessionHandle,
) -> Result<api_types::order::AccountSnapshot, ErrorCode> {
    let user_id = authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;

    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "get_balances")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_balances: {error}"),
        })?;
    let balances: Result<(u64, u64), ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let (trading, withdrawable) = balances?;

    let rows =
        db::tx::query(|connection| db::repo::orders::list_orders(connection, &user_id, None, 50))
            .map_err(map_db)?;

    let mut open_orders = Vec::new();
    let mut pending_orders = Vec::new();
    for (_, order) in rows {
        match order.state {
            api_types::order::OrderState::Open | api_types::order::OrderState::PartiallyFilled => {
                open_orders.push(api_types::order::OrderView {
                    order_id: order.order_id.clone(),
                    cloid: Some(order.cloid.clone()),
                    market: order.market.clone(),
                    side: if order.is_buy {
                        api_types::order::Side::Buy
                    } else {
                        api_types::order::Side::Sell
                    },
                    kind: if order.kind == "market_ioc" {
                        api_types::order::OrderKind::MarketIoc
                    } else {
                        api_types::order::OrderKind::LimitGtc
                    },
                    price: order.price.clone(),
                    quantity: order.quantity.clone(),
                    filled_quantity: order.filled_quantity.clone(),
                    state: order.state,
                    venue_state: None,
                    hl_oid: order.hl_oid,
                    cancel_requested: order.cancel_requested,
                    updated_at: order.updated_at,
                });
            }
            api_types::order::OrderState::Pending | api_types::order::OrderState::Unknown => {
                pending_orders.push(api_types::order::PendingOrderView {
                    request_id: order.order_id.clone(),
                    cloid: Some(order.cloid.clone()),
                    order_id: Some(order.order_id.clone()),
                    action_state: api_types::fund::ActionState::Queued,
                    since: order.created_at,
                    last_error: None,
                });
            }
            _ => {}
        }
    }

    Ok(api_types::order::AccountSnapshot {
        account_id: account_id.to_vec().into(),
        equity: trading,
        margin_used: 0,
        withdrawable,
        unrealized_pnl: 0,
        positions: Vec::new(),
        open_orders,
        pending_orders,
        observed_at: now,
        revision: 1,
        data_age_ms: 0,
    })
}

/// 注文一覧（新しい順）。
///
/// **updateである理由**：認可に `funds_vault` へのinter-canister呼び出しが必要だが、
/// queryでは他Canisterを呼べない。最終設計では、(a) 個人向け読み取りをupdateのまま
/// 提供する、(b) vaultからセッション写像をcoreへ同期してqueryで返す、のいずれかを選ぶ
/// （`docs/phase-1/README.md` の残課題）。
#[ic_cdk::update]
async fn list_orders(
    session: SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::order::OrderSummary>, ErrorCode> {
    let user_id = authorize(&session).await?;
    let limit = limit.clamp(1, 100);
    let now = ic_cdk::api::time() / 1_000_000;

    let before = match cursor.as_ref() {
        Some(cursor) => {
            let bytes: [u8; 8] = cursor
                .as_ref()
                .try_into()
                .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid cursor"))?;
            Some(i64::from_be_bytes(bytes))
        }
        None => None,
    };

    let rows = db::tx::query(|connection| {
        db::repo::orders::list_orders(connection, &user_id, before, limit)
    })
    .map_err(map_db)?;

    let next_cursor = if rows.len() == limit as usize {
        rows.last()
            .map(|(rowid, _)| rowid.to_be_bytes().to_vec().into())
    } else {
        None
    };

    Ok(api_types::Paged {
        items: rows.into_iter().map(|(_, order)| order).collect(),
        next_cursor,
        observed_at: now,
        revision: 1,
    })
}

fn bad(code: BadRequestCode, detail: &str) -> ErrorCode {
    ErrorCode::BadRequest {
        code,
        detail: detail.to_string(),
    }
}

fn bad_decimal(error: hl_types::decimal::DecimalError) -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: error.to_string(),
    }
}

/// 受付fingerprint（本文の同一性判定）。暗号学的ハッシュを使う。
fn body_fingerprint(bytes: &[u8]) -> [u8; 32] {
    hl_sign::keccak256(bytes)
}

/// vaultに問い合わせてセッションを検証する（caller束縛はここで行う）。
async fn authorize(session: &SessionHandle) -> Result<[u8; 32], ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let status = vault_session_status(session).await?;
    if status.principal != caller {
        return Err(ErrorCode::Unauthenticated {
            reason: "session does not belong to this caller".to_string(),
        });
    }
    status
        .user_id
        .as_ref()
        .try_into()
        .map_err(|_| internal("user_id must be 32 bytes".to_string()))
}

/// 本人の取引口座IDをvaultから取得する（所有権の確認を兼ねる）。
async fn trading_account(session: &SessionHandle) -> Result<[u8; 32], ErrorCode> {
    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "get_trading_account")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_trading_account: {error}"),
        })?;
    let account: Result<Option<api_types::Blob>, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    let account = account?.ok_or(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::AccountNotOwned,
    })?;
    account
        .as_ref()
        .try_into()
        .map_err(|_| internal("account_id must be 32 bytes".to_string()))
}

async fn vault_session_status(session: &SessionHandle) -> Result<SessionStatus, ErrorCode> {
    let vault = vault_principal()?;
    let response = Call::bounded_wait(vault, "session_status")
        .with_arg(session.clone())
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault session_status: {error}"),
        })?;
    let status: Result<SessionStatus, ErrorCode> = response
        .candid()
        .map_err(|error| internal(error.to_string()))?;
    status
}

fn vault_principal() -> Result<Principal, ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::vault_principal)
        .map_err(map_db)?
        .ok_or_else(|| internal("vault principal is not configured".to_string()))?;
    Ok(Principal::from_slice(&bytes))
}

/// metaからasset indexを解決する（未取得はfail-closed）。
fn resolve_asset_index(market: &str) -> Result<u32, ErrorCode> {
    // network・dexは設定から解決する（固定値を埋め込まない）。未設定はfail-closed。
    let (network, dex) = db::tx::query(db::repo::core_config::market_context)
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let universe =
        db::tx::query(|connection| db::repo::meta::universe_json(connection, &network, &dex))
            .map_err(map_db)?
            .ok_or(ErrorCode::PolicyUnavailable)?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&universe).map_err(|error| internal(error.to_string()))?;
    for (index, entry) in entries.iter().enumerate() {
        if entry.get("name").and_then(|name| name.as_str()) == Some(market) {
            return u32::try_from(index).map_err(|_| internal("index overflow".to_string()));
        }
    }
    Err(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::AssetNotAllowed,
    })
}

/// テスト専用：注文actionへAgent鍵で署名する（`test-venue` featureでのみ存在）。
///
/// 戻り値は `(署名対象ダイジェスト, 65バイト署名)`。署名経路の検証に使う。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_sign_order_action(
    order_id: api_types::Blob,
) -> Result<(api_types::Blob, api_types::Blob), ErrorCode> {
    let order_id: [u8; 32] = order_id.as_ref().try_into().map_err(|_| {
        bad(
            BadRequestCode::MalformedPayload,
            "order_id must be 32 bytes",
        )
    })?;
    let order = db::tx::query(|connection| db::repo::orders::signable(connection, &order_id))
        .map_err(map_db)?
        .ok_or_else(|| bad(BadRequestCode::MalformedPayload, "unknown order"))?;

    let tif = if order.kind == "market_ioc" {
        hl_types::action::TimeInForce::Ioc
    } else {
        hl_types::action::TimeInForce::Gtc
    };
    let price = order.price.clone().ok_or_else(|| {
        bad(
            BadRequestCode::MissingField,
            "price is required for signing",
        )
    })?;
    let action = hl_types::action::OrderAction {
        orders: vec![hl_types::action::OrderRequest {
            asset_index: order.asset_index,
            is_buy: order.is_buy,
            price: hl_types::decimal::Decimal::parse(&price).map_err(bad_decimal)?,
            size: hl_types::decimal::Decimal::parse(&order.quantity).map_err(bad_decimal)?,
            reduce_only: order.reduce_only,
            order_type: hl_types::action::OrderType::Limit { tif },
            cloid: Some(format!("0x{}", hex::encode(order.cloid))),
        }],
        grouping: hl_types::action::Grouping::Na,
    };
    let msgpack = action.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &msgpack,
        nonce: order.created_at,
        vault_address: None,
        expires_after: None,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);

    // Agent鍵（coreが導出・保管）で署名し、`v`を復元する。
    let generation =
        db::tx::query(|connection| db::repo::agents::latest(connection, &order.account_id))
            .map_err(map_db)?
            .ok_or(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            })?;
    let path = agent_derivation_path(&order.account_id, generation.generation);
    let public_key: [u8; 33] = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: path.clone(),
        key_id: ecdsa_key_id(),
    })
    .await
    .map_err(|error| internal(format!("ecdsa_public_key failed: {error}")))?
    .public_key
    .try_into()
    .map_err(|_| internal("unexpected public key length".to_string()))?;
    let signature = sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: digest.to_vec(),
        derivation_path: path,
        key_id: ecdsa_key_id(),
    })
    .await
    .map_err(|error| internal(format!("sign_with_ecdsa failed: {error}")))?
    .signature;
    let bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| internal("unexpected signature length".to_string()))?;
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&bytes[0..32]);
    s.copy_from_slice(&bytes[32..64]);
    let v = hl_sign::recover_v(&digest, r, s, &public_key)
        .map_err(|error| internal(format!("cannot recover v: {error}")))?;
    let signature = hl_sign::Signature { r, s, v };

    Ok((
        digest.to_vec().into(),
        signature.to_bytes65().to_vec().into(),
    ))
}

/// 取り引所の応答。
#[cfg(feature = "test-venue")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExchangeOutcome {
    Accepted,
    Rejected,
}

/// 注文のaction JSON（HLの`/exchange`はJSON actionを取る）。
#[cfg(feature = "test-venue")]
fn order_action_json(order: &db::repo::orders::SignableOrder, price: &str) -> serde_json::Value {
    let tif = if order.kind == "market_ioc" {
        "Ioc"
    } else {
        "Gtc"
    };
    serde_json::json!({
        "type": "order",
        "orders": [{
            "a": order.asset_index,
            "b": order.is_buy,
            "p": price,
            "s": order.quantity,
            "r": order.reduce_only,
            "t": { "limit": { "tif": tif } },
        }],
        "grouping": "na",
    })
}

/// 署名と送信本文を作る（action msgpackのハッシュへAgent鍵で署名）。
#[cfg(feature = "test-venue")]
async fn sign_and_build(
    order: &db::repo::orders::SignableOrder,
) -> Result<([u8; 32], hl_sign::Signature, Vec<u8>), ErrorCode> {
    let price = order.price.clone().ok_or_else(|| {
        bad(
            BadRequestCode::MissingField,
            "price is required for signing",
        )
    })?;
    let tif = if order.kind == "market_ioc" {
        hl_types::action::TimeInForce::Ioc
    } else {
        hl_types::action::TimeInForce::Gtc
    };
    let action = hl_types::action::OrderAction {
        orders: vec![hl_types::action::OrderRequest {
            asset_index: order.asset_index,
            is_buy: order.is_buy,
            price: hl_types::decimal::Decimal::parse(&price).map_err(bad_decimal)?,
            size: hl_types::decimal::Decimal::parse(&order.quantity).map_err(bad_decimal)?,
            reduce_only: order.reduce_only,
            order_type: hl_types::action::OrderType::Limit { tif },
            cloid: Some(format!("0x{}", hex::encode(order.cloid))),
        }],
        grouping: hl_types::action::Grouping::Na,
    };
    let msgpack = action.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &msgpack,
        nonce: order.created_at,
        vault_address: None,
        expires_after: None,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);

    let generation =
        db::tx::query(|connection| db::repo::agents::latest(connection, &order.account_id))
            .map_err(map_db)?
            .ok_or(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            })?;
    let path = agent_derivation_path(&order.account_id, generation.generation);
    let public_key: [u8; 33] = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: path.clone(),
        key_id: ecdsa_key_id(),
    })
    .await
    .map_err(|error| internal(format!("ecdsa_public_key failed: {error}")))?
    .public_key
    .try_into()
    .map_err(|_| internal("unexpected public key length".to_string()))?;
    let signature = sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: digest.to_vec(),
        derivation_path: path,
        key_id: ecdsa_key_id(),
    })
    .await
    .map_err(|error| internal(format!("sign_with_ecdsa failed: {error}")))?
    .signature;
    let bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| internal("unexpected signature length".to_string()))?;
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&bytes[0..32]);
    s.copy_from_slice(&bytes[32..64]);
    let v = hl_sign::recover_v(&digest, r, s, &public_key)
        .map_err(|error| internal(format!("cannot recover v: {error}")))?;
    let signature = hl_sign::Signature { r, s, v };

    let body = serde_json::json!({
        "action": order_action_json(order, &price),
        "nonce": order.created_at,
        "signature": {
            "r": format!("0x{}", hex::encode(signature.r)),
            "s": format!("0x{}", hex::encode(signature.s)),
            "v": signature.v,
        },
    });
    let body = serde_json::to_vec(&body).map_err(|error| internal(error.to_string()))?;
    Ok((digest, signature, body))
}

/// 注文を送信する（非replicated POST）。
#[cfg(feature = "test-venue")]
async fn post_order(body: &[u8]) -> Result<(ExchangeOutcome, Option<u64>), ErrorCode> {
    let response = HttpRequest::new("https://api.hyperliquid-testnet.xyz/exchange")
        .with_method(HttpMethod::POST)
        .with_headers(vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }])
        .with_body(body.to_vec())
        .with_max_response_bytes(8 * 1024)
        .non_replicated()
        .send()
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: error.to_string(),
        })?;

    let value: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|_| ErrorCode::UpstreamRejected {
            code: "unparseable exchange response".to_string(),
            retryable: false,
        })?;
    match value.get("status").and_then(|status| status.as_str()) {
        Some("ok") => {
            let oid = value
                .get("response")
                .and_then(|response| response.get("data"))
                .and_then(|data| data.get("statuses"))
                .and_then(|statuses| statuses.get(0))
                .and_then(|status| status.get("resting"))
                .and_then(|resting| resting.get("oid"))
                .and_then(|oid| oid.as_u64());
            Ok((ExchangeOutcome::Accepted, oid))
        }
        Some("err") => Ok((ExchangeOutcome::Rejected, None)),
        _ => Err(ErrorCode::UpstreamRejected {
            code: "unexpected exchange response".to_string(),
            retryable: false,
        }),
    }
}

/// テスト専用：待ち注文を送信する（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_sweep_now() -> Result<u32, ErrorCode> {
    let ids = db::tx::query(|connection| db::repo::orders::queued_orders(connection, 4))
        .map_err(map_db)?;
    let mut processed = 0;
    for order_id in ids {
        let claimed = db::tx::update(|connection| {
            db::repo::orders::claim_for_dispatch(connection, &order_id)
        })
        .map_err(map_db)?;
        if !claimed {
            continue;
        }
        let now = ic_cdk::api::time() / 1_000_000;
        let order = db::tx::query(|connection| db::repo::orders::signable(connection, &order_id))
            .map_err(map_db)?
            .ok_or_else(|| internal("missing order".to_string()))?;
        let (_digest, signature, body) = sign_and_build(&order).await?;
        db::tx::update(|connection| {
            db::repo::orders::mark_dispatching(
                connection,
                &order_id,
                &body,
                &signature.to_bytes65(),
                now,
            )
        })
        .map_err(map_db)?;

        match post_order(&body).await {
            Ok((ExchangeOutcome::Accepted, oid)) => {
                db::tx::update(|connection| {
                    db::repo::orders::mark_venue_accepted(connection, &order_id, oid, now)
                })
                .map_err(map_db)?;
            }
            Ok((ExchangeOutcome::Rejected, _)) => {
                db::tx::update(|connection| {
                    db::repo::orders::mark_venue_rejected(connection, &order_id, now)
                })
                .map_err(map_db)?;
            }
            Err(_) => {
                db::tx::update(|connection| {
                    db::repo::orders::mark_unknown(connection, &order_id, now)
                })
                .map_err(map_db)?;
            }
        }
        processed += 1;
    }
    Ok(processed)
}

/// テスト専用：`/info`の`userFills`相当を取り込む（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_ingest_fills(
    session: api_types::auth::SessionHandle,
    fills_json: String,
) -> Result<u32, ErrorCode> {
    let user_id = authorize(&session).await?;
    let fills: Vec<serde_json::Value> =
        serde_json::from_str(&fills_json).map_err(|error| internal(error.to_string()))?;
    let now = ic_cdk::api::time() / 1_000_000;
    let mut ingested = 0;
    for fill in fills {
        let tid = fill
            .get("tid")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let oid = fill
            .get("oid")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let coin = fill
            .get("coin")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let price = fill
            .get("px")
            .and_then(|value| value.as_str())
            .unwrap_or("0")
            .to_string();
        let quantity = fill
            .get("sz")
            .and_then(|value| value.as_str())
            .unwrap_or("0")
            .to_string();
        let fee = fill
            .get("fee")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let at = fill
            .get("time")
            .and_then(|value| value.as_u64())
            .unwrap_or(now);
        let inserted = db::tx::update(|connection| {
            db::repo::orders::ingest_fill(
                connection,
                &user_id,
                &db::repo::orders::NewFill {
                    tid,
                    hl_oid: oid,
                    market: &coin,
                    price: &price,
                    quantity: &quantity,
                    fee,
                    filled_at: at,
                },
            )
        })
        .map_err(map_db)?;
        if inserted {
            ingested += 1;
        }
    }
    Ok(ingested)
}

/// テスト専用：`orderStatus`相当を反映する（`test-venue` featureでのみ存在）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_apply_order_status(
    session: api_types::auth::SessionHandle,
    status_json: String,
) -> Result<bool, ErrorCode> {
    authorize(&session).await?;
    let value: serde_json::Value =
        serde_json::from_str(&status_json).map_err(|error| internal(error.to_string()))?;
    let status = value
        .get("status")
        .and_then(|status| status.as_str())
        .unwrap_or("unknown")
        .to_string();
    let oid = value
        .get("order")
        .and_then(|order| order.get("oid"))
        .and_then(|oid| oid.as_u64())
        .ok_or_else(|| bad(BadRequestCode::MissingField, "order.oid is required"))?;
    // 取引所の語彙をこちらの状態へ写す（未知はunknownとして保持）。
    let state = match status.as_str() {
        "open" => "open",
        "filled" => "filled",
        "canceled" | "cancelled" => "cancelled",
        "rejected" => "rejected",
        _ => "unknown",
    };
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::orders::apply_order_status(connection, oid, state, now))
        .map_err(map_db)
}

fn init_db() {
    if let Err(error) = db::init(MEMORY_ID, db::schema::core::MIGRATIONS) {
        ic_cdk::trap(format!("db init failed: {error}"));
    }
}

#[ic_cdk::init]
fn init() {
    init_db();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    init_db();
}

ic_cdk::export_candid!();

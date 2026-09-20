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
    let universe = db::tx::query(|connection| {
        db::repo::meta::universe_json(connection, "local", "hyperliquid")
    })
    .map_err(map_db)?
    .ok_or_else(|| ErrorCode::PolicyUnavailable)?;
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

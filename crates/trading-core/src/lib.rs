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

/// 取引所データを「古い」とみなす閾値（ミリ秒）。
const STALE_DATA_MS: u64 = 10_000;

/// 1口座あたりの未終端注文（pending・open・partially_filled・unknown）の上限。
const MAX_PENDING_ORDERS: u64 = 50;

/// 決済（反対売買のIOC指値）で価格を導出するときのスリッページ許容幅（bps）。
const DEFAULT_SLIPPAGE_BPS: u32 = 50;

/// HPKE封筒の用途分離ラベル（vaultと同じ封筒実装を使う）。
const ENVELOPE_INFO: &[u8] = b"private-perp/envelope/v1";

/// 封筒の受付期限の上限（`now`から先の許容幅）。時計のずれと使い回しを抑える。
const MAX_ENVELOPE_TTL_MS: u64 = 300_000;

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
        DbError::RiskLimitExceeded { limit } => ErrorCode::RiskLimitExceeded { limit },
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

/// 政策が停止中なら拒否する。**未設定・照会失敗はfail-closed**で拒否する
/// （`authority-matrix.md`: 読み取り失敗時は新規受付・新規リスク増加を停止する）。
async fn require_not_stopped() -> Result<(), ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::policy_principal).map_err(map_db)?;
    let Some(bytes) = bytes else {
        return Err(ErrorCode::PolicyUnavailable);
    };
    let policy = Principal::from_slice(&bytes);
    let response = Call::bounded_wait(policy, "get_stop_status")
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("policy get_stop_status: {error}"),
        })?;
    // `get_stop_status` は`Result`ではなく`StopStatus`を返す。
    let status: api_types::policy::StopStatus = response
        .candid()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    if status.stopped {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    Ok(())
}

/// 取引可能な銘柄のallowlist（policy_registry）。未設定・照会失敗はfail-closed。
async fn policy_markets() -> Result<Vec<String>, ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::policy_principal).map_err(map_db)?;
    let Some(bytes) = bytes else {
        return Err(ErrorCode::PolicyUnavailable);
    };
    let policy = Principal::from_slice(&bytes);
    let response = Call::bounded_wait(policy, "get_policy")
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("policy get_policy: {error}"),
        })?;
    let policy: Result<api_types::policy::Policy, ErrorCode> = response
        .candid()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    Ok(policy?.markets)
}

/// vaultのprincipal（診断用）。
#[ic_cdk::query]
fn get_vault_principal() -> Option<Principal> {
    db::tx::query(db::repo::core_config::vault_principal)
        .ok()
        .flatten()
        .map(|bytes| Principal::from_slice(&bytes))
}

/// HPKEの鍵世代を更新する（controllerのみ）。
///
/// 秘密鍵はcanister内のDBに留め、公開鍵のみを配布する（`Plan.md` 16.5、
/// `docs/phase-0/api-contract.md` 6節）。更新すると以前の世代は退役し、
/// 旧鍵で作られた封筒は復号できない（クライアントは公開鍵を取得し直す）。
#[ic_cdk::update]
async fn rotate_hpke_key() -> Result<api_types::Blob, ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(ErrorCode::Unauthenticated {
            reason: "only a controller can rotate the HPKE key".to_string(),
        });
    }
    let ikm = raw_rand32().await?;
    let (secret, public) = hpke_envelope::derive_keypair(&ikm);
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| internal("unexpected secret length".to_string()))?;
    let public: [u8; 32] = public
        .try_into()
        .map_err(|_| internal("unexpected public length".to_string()))?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| db::repo::hpke::insert_key(connection, &secret, &public, now))
        .map_err(map_db)?;
    Ok(public.to_vec().into())
}

/// 現行のHPKE公開鍵。未生成はエラー（機密性の前提が欠けている）。
#[ic_cdk::query]
fn get_hpke_public_key() -> Result<api_types::Blob, ErrorCode> {
    db::tx::query(db::repo::hpke::active_public)
        .map_err(map_db)?
        .map(|public| public.into())
        .ok_or(ErrorCode::PolicyUnavailable)
}

/// 封筒を開いて平文を取り出し、`request_id`を消費する（`api-contract.md` 6節）。
///
/// 束縛する値：鍵ID（現行世代）・`network`・`canister`・`method`・`caller`・
/// `request_id`・期限。`aad`は再計算した値と比較し、復号にも同じ値を使うため
/// 改竄は復号失敗になる。`request_id`は復号に成功した要求だけ消費する。
async fn open_envelope<T: serde::de::DeserializeOwned + candid::CandidType>(
    envelope: &api_types::envelope::HpkeRequest,
    method: &str,
) -> Result<(T, [u8; 32], Principal), ErrorCode> {
    let caller = ic_cdk::api::msg_caller();
    let now = ic_cdk::api::time() / 1_000_000;
    let malformed = |detail: &str| bad(BadRequestCode::MalformedPayload, detail);

    let public = db::tx::query(db::repo::hpke::active_public)
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if envelope.key_id.as_ref() != public.as_slice() {
        return Err(malformed("unknown hpke key id"));
    }
    if envelope.canister != ic_cdk::api::canister_self() {
        return Err(malformed("envelope canister mismatch"));
    }
    if envelope.method != method {
        return Err(malformed("envelope method mismatch"));
    }
    let network = network_name()?;
    if !network_matches(envelope.network, &network) {
        return Err(malformed("envelope network mismatch"));
    }
    if envelope.expires_at < now {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::ExpiredIntent,
            detail: "the envelope has expired".to_string(),
        });
    }
    if envelope.expires_at > now.saturating_add(MAX_ENVELOPE_TTL_MS) {
        return Err(malformed("the envelope expiry is too far ahead"));
    }
    let request_id: [u8; 32] = envelope
        .request_id
        .as_ref()
        .try_into()
        .map_err(|_| malformed("request_id must be 32 bytes"))?;
    let aad = envelope_aad(method, caller, &request_id, envelope.expires_at, &network)?;
    if envelope.aad.as_ref() != aad.as_slice() {
        return Err(malformed("envelope aad mismatch"));
    }
    let secret = db::tx::query(db::repo::hpke::active_secret)
        .map_err(map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let plaintext = hpke_envelope::open(&secret, ENVELOPE_INFO, &aad, envelope.ciphertext.as_ref())
        .map_err(|_| malformed("cannot open the envelope"))?;
    // 復号できた要求だけを単回使用として記録する（改竄された要求でIDを消費しない）。
    let consumed = db::tx::update(|connection| {
        db::repo::hpke_requests::consume(
            connection,
            &request_id,
            method,
            caller.as_slice(),
            now,
            envelope.expires_at,
        )
    })
    .map_err(map_db)?;
    if !consumed {
        return Err(ErrorCode::BadRequest {
            code: BadRequestCode::NonceReused,
            detail: "request_id was already used".to_string(),
        });
    }
    let payload =
        candid::decode_one(&plaintext).map_err(|_| malformed("cannot decode the payload"))?;
    Ok((payload, request_id, caller))
}

/// 応答を封筒へ入れる（`client_public_key`宛。`aad`は要求と同じ束縛）。
async fn seal_envelope<T: candid::CandidType>(
    envelope: &api_types::envelope::HpkeRequest,
    method: &str,
    request_id: &[u8; 32],
    caller: Principal,
    response: &T,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let network = network_name()?;
    let plaintext = candid::encode_one(response).map_err(|error| internal(error.to_string()))?;
    let seed = raw_rand32().await?;
    let aad = envelope_aad(method, caller, request_id, envelope.expires_at, &network)?;
    let ciphertext = hpke_envelope::seal(
        envelope.client_public_key.as_ref(),
        ENVELOPE_INFO,
        &aad,
        &plaintext,
        &seed,
    )
    .map_err(|_| bad(BadRequestCode::MalformedPayload, "cannot seal the response"))?;
    Ok(api_types::envelope::HpkeResponse {
        request_id: request_id.to_vec().into(),
        key_id: envelope.key_id.clone(),
        observed_at: ic_cdk::api::time() / 1_000_000,
        ciphertext: ciphertext.into(),
    })
}

/// 要求・応答の`aad`（設定されたnetworkと自分のprincipalを束縛する）。
fn envelope_aad(
    method: &str,
    caller: Principal,
    request_id: &[u8; 32],
    expires_at: u64,
    network: &str,
) -> Result<Vec<u8>, ErrorCode> {
    let canister = ic_cdk::api::canister_self();
    let canister = canister.as_slice();
    Ok(hpke_envelope::envelope_aad(
        network,
        canister,
        method,
        caller.as_slice(),
        request_id,
        expires_at,
    ))
}

/// 設定されたnetwork名（封筒の束縛に使う。未設定はfail-closed）。
fn network_name() -> Result<String, ErrorCode> {
    db::tx::query(db::repo::core_config::market_context)
        .map_err(map_db)?
        .map(|(network, _)| network)
        .ok_or(ErrorCode::PolicyUnavailable)
}

/// 封筒の`Network`と設定値の対応。
fn network_matches(network: api_types::Network, name: &str) -> bool {
    matches!(
        (network, name),
        (api_types::Network::Local, "local")
            | (api_types::Network::Testnet, "testnet")
            | (api_types::Network::Mainnet, "mainnet")
    )
}

/// `raw_rand`から32バイトを取る。
async fn raw_rand32() -> Result<[u8; 32], ErrorCode> {
    let bytes = ic_cdk_management_canister::raw_rand()
        .await
        .map_err(|error| internal(format!("raw_rand failed: {error}")))?;
    bytes
        .get(..32)
        .ok_or_else(|| internal("raw_rand too short".to_string()))?
        .try_into()
        .map_err(|_| internal("raw_rand length".to_string()))
}

/// セッションを検証し、本人のuser_idを返す（認可境界の試験用）。///
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
    submit_inner(&session, user_id, args).await
}

/// 認可済みの受付（`submit_order`・決済の共通経路）。
///
/// 検証・冪等性・リスク予約・`pending`注文の登録を行う。`reduce_only`は
/// 新規リスクを増やさないため、リスク予約と鮮度ゲートの対象外とする
/// （建玉があるときに保護・決済を打てなくなる方が危険である）。
async fn submit_inner(
    session: &SessionHandle,
    user_id: [u8; 32],
    args: api_types::order::SubmitOrderArgs,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    let now = ic_cdk::api::time() / 1_000_000;

    // 銘柄は初期allowlistのみ。asset indexはmetaから解決する（固定値を埋め込まない）。
    // 緊急停止中は新規受付を行わない（fail-closed）。
    require_not_stopped().await?;

    let market = args.market.to_uppercase();
    // 銘柄はpolicy_registryのallowlistで判定する（照会失敗・未設定はfail-closed）。
    if !policy_markets()
        .await?
        .iter()
        .any(|allowed| allowed == &market)
    {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::AssetNotAllowed,
        });
    }

    // 数量・価格は正規化十進で検証し、丸めない。
    let quantity = hl_types::decimal::Decimal::parse(&args.quantity).map_err(bad_decimal)?;
    if quantity.as_str().starts_with('-') || quantity.as_str() == "0" {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "quantity must be positive",
        ));
    }
    let price = match (&args.kind, &args.limit_price) {
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
            Some(price)
        }
        _ => {
            // 成行（Market IOC）も指値（スリッページ上限）と同様に価格を必須とする。
            return Err(bad(BadRequestCode::MissingField, "limit price is required"));
        }
    };
    if args.leverage.unwrap_or(3) > 5 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "leverage exceeds the UI limit",
        ));
    }

    // 数量・価格の精度を銘柄の `szDecimals` で検査する（HLはperpsで有効数字5桁まで、
    // 価格は 6-szDecimals 桁まで）。metaに無い場合は設定不備としてfail-closedで拒否する。
    let (asset_index, sz_decimals) = resolve_asset(&market)?;
    // 数量は `szDecimals` まで。reduce-only（決済・保護）は建玉の数量をそのまま送れる
    // 必要があるため桁数のみを検査する（有効数字の制限で決済不能にしない）。
    let quantity_limit = if args.reduce_only { None } else { Some(5) };
    match quantity_limit {
        Some(max_significant) => quantity
            .validate_precision(sz_decimals, max_significant)
            .map_err(|error| bad(BadRequestCode::PrecisionExceeded, &error.to_string()))?,
        None => {
            if quantity.scale() > sz_decimals {
                return Err(bad(
                    BadRequestCode::PrecisionExceeded,
                    "quantity exceeds the market scale",
                ));
            }
        }
    }
    if let Some(price) = &price {
        price
            .validate_precision(6u32.saturating_sub(sz_decimals), 5)
            .map_err(|error| bad(BadRequestCode::PrecisionExceeded, &error.to_string()))?;
    }
    // 本文fingerprint（受付の冪等性）。**本文全体**を含める。side・reduce_only・
    // leverage・kind を除くと、同一IDで反対売買やreduce_onlyを変えた再送が
    // 「同一本文」と誤判定され、黙って捨てられる。
    let mut body = Vec::new();
    body.extend_from_slice(market.as_bytes());
    body.extend_from_slice(quantity.as_str().as_bytes());
    body.extend_from_slice(args.limit_price.as_deref().unwrap_or("").as_bytes());
    body.push(u8::from(matches!(args.side, api_types::order::Side::Buy)));
    body.push(u8::from(args.reduce_only));
    body.push(match args.kind {
        api_types::order::OrderKind::MarketIoc => 1,
        api_types::order::OrderKind::LimitGtc => 2,
    });
    body.extend_from_slice(&args.leverage.unwrap_or(3).to_be_bytes());
    // トリガの有無と内容も本文に含める（SL/TPだけを差し替えた再送を別本文とする）。
    if let Some(trigger) = &args.trigger {
        body.push(u8::from(matches!(
            trigger.kind,
            api_types::order::TriggerKind::StopLoss
        )));
        body.extend_from_slice(trigger.trigger_price.as_bytes());
        body.push(u8::from(trigger.is_market));
    }
    let fingerprint = body_fingerprint(&body);

    let account_id = trading_account(session).await?;
    // 取引所データが古い場合は新規リスクを増やさない（観測が無い口座は対象外）。
    // reduce-onlyはリスクを減らす方向にしか作用しないため、鮮度に関わらず受け付ける
    // （建玉があるときに保護・決済を打てなくなる方が危険である）。
    if !args.reduce_only
        && let Some(observed) = db::tx::query(|connection| {
            db::repo::positions::latest_observed(connection, &account_id)
        })
        .map_err(map_db)?
        && now.saturating_sub(observed) > STALE_DATA_MS
    {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }

    // SL/TPは建玉単位（positionTpsl）のreduce-only注文としてのみ受け付ける。
    if let Some(trigger) = &args.trigger {
        validate_trigger(&account_id, &market, &args, trigger, sz_decimals)?;
    }

    // 想定元本とequityは新規リスクの上限判断にのみ使う（reduce-onlyは予約しない）。
    let (notional, equity) = if args.reduce_only {
        (0, 0)
    } else {
        (
            notional_micros(args.limit_price.as_deref().unwrap_or("0"), &args.quantity)?,
            vault_trading_equity(session).await?,
        )
    };

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
        // 未終端の注文数に上限を設ける（1口座あたりのpending・open・unknown）。
        if db::repo::orders::pending_order_count(connection, &account_id)? >= MAX_PENDING_ORDERS {
            return Err(DbError::Invariant("too many pending orders"));
        }
        // reduce-onlyはエクスポージャを増やさないため、リスク予約を取らない
        // （予約すると建玉を閉じるための資金が無い状態で決済できなくなる）。
        if !args.reduce_only {
            // 予約済みリスクに今回の想定元本を足してもequityを超えないこと。
            db::repo::orders::ensure_risk_within_equity(connection, &account_id, notional, equity)?;
            db::repo::orders::reserve_risk(
                connection,
                &account_id,
                args.client_request_id.as_ref(),
                notional,
                now,
            )?;
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
                trigger: args
                    .trigger
                    .as_ref()
                    .map(|trigger| db::repo::orders::NewTrigger {
                        kind: db::states::trigger_kind_str(trigger.kind).to_string(),
                        price: trigger.trigger_price.clone(),
                        is_market: trigger.is_market,
                    }),
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

/// 建玉を閉じる（全量または比率指定）。反対売買のreduce-only IOC指値として受付ける。
///
/// `limit_price`はスリッページ上限（公開市況から画面が決める）。省略時は観測した
/// 建玉からmark価格を近似して`DEFAULT_SLIPPAGE_BPS`の幅を付ける。
/// `ratio_bps`は建玉に対する比率（10000 = 全量）。
#[ic_cdk::update]
async fn close_position(
    session: SessionHandle,
    client_request_id: api_types::Blob,
    market: String,
    ratio_bps: u32,
    limit_price: Option<String>,
) -> Result<api_types::order::SubmitOrderResult, ErrorCode> {
    let user_id = authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let market = market.to_uppercase();
    let position = open_position(&account_id, &market)?;
    let args = close_order_args(
        &session,
        &account_id,
        client_request_id.as_ref().to_vec(),
        &position,
        ratio_bps,
        limit_price,
    )?;
    submit_inner(&session, user_id, args).await
}

/// 建玉をすべて閉じる（建玉ごとに`close_position`と同じ反対売買を送る）。
///
/// 1件の失敗で全体を止めない（建玉ごとの結果を返す）。受付IDは
/// `client_request_id`と銘柄から導出するため、同じIDの再送は同じ注文として扱われる
/// （建玉が変わっている場合は`IdempotencyConflict`になる）。
#[ic_cdk::update]
async fn close_all(
    session: SessionHandle,
    client_request_id: api_types::Blob,
) -> Result<api_types::order::CloseAllOutcome, ErrorCode> {
    let user_id = authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let positions = db::tx::query(|connection| db::repo::positions::list(connection, &account_id))
        .map_err(map_db)?;

    let mut outcome = api_types::order::CloseAllOutcome {
        submitted: Vec::new(),
        failed: Vec::new(),
    };
    for position in positions {
        if !is_open_size(&position.size) {
            continue;
        }
        // 銘柄ごとに決定的な受付IDを作り、再送を冪等にする。
        let mut seed = client_request_id.as_ref().to_vec();
        seed.extend_from_slice(b"close_all");
        seed.extend_from_slice(position.market.as_bytes());
        let request_id = body_fingerprint(&seed).to_vec();
        let result =
            match close_order_args(&session, &account_id, request_id, &position, 10_000, None) {
                Ok(args) => submit_inner(&session, user_id, args).await,
                Err(error) => Err(error),
            };
        match result {
            Ok(result) => outcome.submitted.push(result),
            Err(error) => outcome.failed.push(api_types::order::CloseFailure {
                market: position.market,
                error,
            }),
        }
    }
    Ok(outcome)
}

/// 建玉の数量が0でない（建玉が開いている）。
fn is_open_size(size: &str) -> bool {
    size.chars()
        .any(|character| character.is_ascii_digit() && character != '0')
}

/// 口座の開いている建玉（無ければ拒否）。
fn open_position(
    account_id: &[u8; 32],
    market: &str,
) -> Result<api_types::order::PositionView, ErrorCode> {
    let position =
        db::tx::query(|connection| db::repo::positions::find(connection, account_id, market))
            .map_err(map_db)?
            .ok_or(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            })?;
    if !is_open_size(&position.size) {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    Ok(position)
}

/// 決済注文（反対売買のreduce-only IOC指値）を組み立てる。
///
/// 数量は建玉数量×比率を銘柄の`szDecimals`で切り捨てる（建玉を超えない）。
/// 価格は呼び出し元の指定（公開市況から決めるスリッページ上限）を優先し、
/// 省略時は観測した建玉からmark価格を近似する。
fn close_order_args(
    session: &SessionHandle,
    account_id: &[u8; 32],
    client_request_id: Vec<u8>,
    position: &api_types::order::PositionView,
    ratio_bps: u32,
    limit_price: Option<String>,
) -> Result<api_types::order::SubmitOrderArgs, ErrorCode> {
    if ratio_bps == 0 || ratio_bps > 10_000 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "ratio must be within 1..=10000 bps",
        ));
    }
    let market = position.market.to_uppercase();
    let (_, sz_decimals) = resolve_asset(&market)?;
    let is_long = !position.size.starts_with('-');
    let size_micros = signed_micros(&position.size)?;
    let magnitude = size_micros.unsigned_abs();
    // `szDecimals`より細かい桁は送れないため、切り捨てる（建玉を超えない方向）。
    let step = if sz_decimals >= 6 {
        1u128
    } else {
        10u128.pow(6 - sz_decimals)
    };
    let quantity_micros = magnitude * u128::from(ratio_bps) / 10_000 / step * step;
    if quantity_micros == 0 {
        return Err(bad(
            BadRequestCode::QuantityOutOfRange,
            "the position is too small to close at this ratio",
        ));
    }
    let quantity = micros_to_decimal(quantity_micros);

    let price = match limit_price {
        Some(price) => price,
        None => slippage_bounded_price(position, is_long, sz_decimals)?,
    };
    Ok(api_types::order::SubmitOrderArgs {
        session: session.clone(),
        client_request_id: client_request_id.into(),
        account_id: account_id.to_vec().into(),
        market,
        side: if is_long {
            api_types::order::Side::Sell
        } else {
            api_types::order::Side::Buy
        },
        kind: api_types::order::OrderKind::MarketIoc,
        quantity,
        limit_price: Some(price),
        slippage_tolerance_bps: Some(DEFAULT_SLIPPAGE_BPS),
        reduce_only: true,
        leverage: None,
        trigger: None,
        expires_after: None,
    })
}

/// 観測した建玉からスリッページ上限つきのIOC指値を作る。
///
/// mark価格は`entry_price + unrealized_pnl / size`で近似する（coreはmark配信を
/// 持たない）。reduce-onlyでは価格はスリッページ上限としてのみ作用し、建玉を
/// 反転できないため、近似でも安全側に働く。価格の桁は`szDecimals`に合わせて切り捨てる。
fn slippage_bounded_price(
    position: &api_types::order::PositionView,
    is_long: bool,
    sz_decimals: u32,
) -> Result<String, ErrorCode> {
    let entry = decimal_micros(&position.entry_price)?;
    let size = signed_micros(&position.size)?;
    if size == 0 {
        return Err(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        });
    }
    let adjustment = i128::from(position.unrealized_pnl)
        .checked_mul(1_000_000)
        .ok_or_else(|| internal("mark price overflow".to_string()))?
        / size;
    let mark = entry
        .checked_add(adjustment)
        .ok_or_else(|| internal("mark price overflow".to_string()))?;
    let bounded = if is_long {
        mark * i128::from(10_000 - DEFAULT_SLIPPAGE_BPS) / 10_000
    } else {
        mark * i128::from(10_000 + DEFAULT_SLIPPAGE_BPS) / 10_000
    };
    if bounded <= 0 {
        return Err(bad(
            BadRequestCode::PriceOutOfRange,
            "cannot derive a positive limit price",
        ));
    }
    let step = if sz_decimals >= 6 {
        1u128
    } else {
        10u128.pow(sz_decimals)
    };
    // 売りの上限は切り捨て（弱気側）、買いの上限は切り上げ（スリッページ幅を狭めない）。
    let micros = bounded.unsigned_abs();
    let floored = micros / step * step;
    let rounded = if is_long || floored == micros {
        floored
    } else {
        floored + step
    };
    Ok(micros_to_decimal(rounded))
}

/// マイクロ単位の整数を正規化した十進文字列へ戻す（末尾ゼロを残さない）。
fn micros_to_decimal(micros: u128) -> String {
    let whole = micros / 1_000_000;
    let fraction = micros % 1_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    let mut text = format!("{fraction:06}");
    while text.ends_with('0') {
        text.pop();
    }
    format!("{whole}.{text}")
}

/// 符号付きの十進文字列をマイクロ（1e-6）単位へ変換する（丸めない）。
///
/// `decimal_micros`は整数部の符号だけを見るため`-0.02`の符号を落とす。
/// 建玉の符号（ロング・ショート）を扱う経路ではこちらを使う。
fn signed_micros(text: &str) -> Result<i128, ErrorCode> {
    match text.strip_prefix('-') {
        Some(magnitude) => Ok(-decimal_micros(magnitude)?),
        None => decimal_micros(text),
    }
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
    let next = db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
        .map_err(map_db)?;
    // 承認はvaultがmaster署名で行い、vaultのDBへ永続化される。coreは自分の行を
    // 「承認済み」と推測せず、vaultの状態を正とする。
    let current = match next.as_ref() {
        Some(generation) if generation.state != api_types::fund::AgentState::Active => {
            agent_approval(&account_id, generation.generation).await?
        }
        Some(generation) => Some(generation.clone()),
        None => None,
    };
    let next = next.filter(|generation| {
        !matches!(
            current.as_ref(),
            Some(approved) if approved.generation == generation.generation
        )
    });
    Ok(api_types::fund::AgentStatus {
        current,
        next,
        revocation_pending: false,
        observed_at: now,
    })
}

/// vaultに永続化された承認状態（未承認・照会失敗は `None`）。
///
/// 署名可否の判断にも使うため、照会できない場合は「承認済み」と扱わない。
async fn agent_approval(
    account_id: &[u8; 32],
    generation: u64,
) -> Result<Option<api_types::fund::AgentGeneration>, ErrorCode> {
    let vault = vault_principal()?;
    // 2引数は `with_args`（`with_arg` は1引数としてエンコードする）。
    let response = Call::bounded_wait(vault, "get_agent_approval")
        .with_args(&(api_types::Blob::from(account_id.to_vec()), generation))
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: format!("vault get_agent_approval: {error}"),
        })?;
    let approval: Result<Option<api_types::fund::AgentGeneration>, ErrorCode> =
        response
            .candid()
            .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let approval = approval?;
    Ok(approval.filter(|row| row.state == api_types::fund::AgentState::Active))
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

/// 注文の取消を要求する（**封筒必須**。署名・送信はパイプラインが行う）。
///
/// 認証は封筒の`aad`と本文のセッションで行う（`api-contract.md` 6節）。
#[ic_cdk::update]
async fn cancel_order(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::CancelOrderQuery>(&envelope, "cancel_order").await?;
    cancel_order_inner(&query.session, query.order_id).await?;
    seal_envelope(&envelope, "cancel_order", &request_id, caller, &()).await
}

/// 取消の本体（受付と同様に冪等で、既に取消要求済み・終端状態なら何もしない）。
async fn cancel_order_inner(
    session: &SessionHandle,
    order_id: api_types::Blob,
) -> Result<(), ErrorCode> {
    let user_id = authorize(session).await?;
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

/// 未終端の注文すべてに取消要求を付ける（送信はsweepが行う）。
#[ic_cdk::update]
async fn cancel_all(session: SessionHandle) -> Result<u64, ErrorCode> {
    let user_id = authorize(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        db::repo::orders::mark_all_cancel_requested(connection, &user_id, now)
    })
    .map_err(map_db)
}

/// 約定一覧（新しい順。**封筒必須**。認可にvaultへの問い合わせが必要なためupdate）。
#[ic_cdk::update]
async fn list_fills(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::ListQuery>(&envelope, "list_fills").await?;
    let page = fills_page(&query.session, query.cursor, query.limit).await?;
    seal_envelope(&envelope, "list_fills", &request_id, caller, &page).await
}

async fn fills_page(
    session: &SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::order::FillView>, ErrorCode> {
    let user_id = authorize(session).await?;
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
        db::repo::orders::list_fills(connection, &user_id, before, limit)
    })
    .map_err(map_db)?;
    let next_cursor = if rows.len() == limit as usize {
        rows.last()
            .map(|(rowid, _)| rowid.to_be_bytes().to_vec().into())
    } else {
        None
    };
    Ok(api_types::Paged {
        items: rows.into_iter().map(|(_, fill)| fill).collect(),
        next_cursor,
        observed_at: now,
        revision: 1,
    })
}

/// 口座snapshot（残高はvault、注文はcore。**封筒必須**）。
#[ic_cdk::update]
async fn get_account_snapshot(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::SnapshotQuery>(&envelope, "get_account_snapshot")
            .await?;
    let snapshot = account_snapshot(&query.session).await?;
    seal_envelope(
        &envelope,
        "get_account_snapshot",
        &request_id,
        caller,
        &snapshot,
    )
    .await
}

async fn account_snapshot(
    session: &SessionHandle,
) -> Result<api_types::order::AccountSnapshot, ErrorCode> {
    let user_id = authorize(session).await?;
    let account_id = trading_account(session).await?;
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
    let revision =
        db::tx::query(|connection| db::repo::orders::account_revision(connection, &user_id))
            .map_err(map_db)?;
    let latest_fill =
        db::tx::query(|connection| db::repo::orders::latest_fill_at(connection, &user_id))
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
                    trigger: order.trigger.clone(),
                    updated_at: order.updated_at,
                });
            }
            api_types::order::OrderState::Pending | api_types::order::OrderState::Unknown => {
                pending_orders.push(api_types::order::PendingOrderView {
                    request_id: order.order_id.clone(),
                    cloid: Some(order.cloid.clone()),
                    order_id: Some(order.order_id.clone()),
                    action_state: order.dispatch_state,
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
        margin_used: db::tx::query(|connection| {
            db::repo::orders::held_risk(connection, &account_id)
        })
        .map_err(map_db)?,
        withdrawable,
        unrealized_pnl: 0,
        positions: db::tx::query(|connection| db::repo::positions::list(connection, &account_id))
            .map_err(map_db)?,
        open_orders,
        pending_orders,
        observed_at: now,
        revision,
        // 取引所由来のデータ（約定）の最終観測からの経過。約定が無い間は0とする。
        data_age_ms: latest_fill.map_or(0, |filled_at| now.saturating_sub(filled_at)),
    })
}

/// 注文一覧（新しい順。**封筒必須**）。
///
/// **updateである理由**：認可に `funds_vault` へのinter-canister呼び出しが必要だが、
/// queryでは他Canisterを呼べない。最終設計では、(a) 個人向け読み取りをupdateのまま
/// 提供する、(b) vaultからセッション写像をcoreへ同期してqueryで返す、のいずれかを選ぶ
/// （`docs/phase-1/README.md` の残課題）。
#[ic_cdk::update]
async fn list_orders(
    envelope: api_types::envelope::HpkeRequest,
) -> Result<api_types::envelope::HpkeResponse, ErrorCode> {
    let (query, request_id, caller) =
        open_envelope::<api_types::envelope::ListQuery>(&envelope, "list_orders").await?;
    let page = orders_page(&query.session, query.cursor, query.limit).await?;
    seal_envelope(&envelope, "list_orders", &request_id, caller, &page).await
}

async fn orders_page(
    session: &SessionHandle,
    cursor: Option<api_types::Blob>,
    limit: u32,
) -> Result<api_types::Paged<api_types::order::OrderSummary>, ErrorCode> {
    let user_id = authorize(session).await?;
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

/// 十進文字列をマイクロ（1e-6）単位の整数へ変換する（丸めない）。
fn decimal_micros(text: &str) -> Result<i128, ErrorCode> {
    let (integer, fraction) = match text.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (text, ""),
    };
    if integer.is_empty() && fraction.is_empty() {
        return Err(bad(BadRequestCode::MalformedPayload, "empty decimal"));
    }
    if fraction.len() > 6 {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "more than 6 decimals",
        ));
    }
    let integer: i128 = integer
        .parse()
        .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid decimal"))?;
    let mut padded = fraction.to_string();
    while padded.len() < 6 {
        padded.push('0');
    }
    let fraction: i128 = padded
        .parse()
        .map_err(|_| bad(BadRequestCode::MalformedPayload, "invalid decimal"))?;
    Ok(integer * 1_000_000 + fraction)
}

/// 価格と数量から想定元本（マイクロUSDC）を求める。
fn notional_micros(price: &str, quantity: &str) -> Result<u64, ErrorCode> {
    let notional = decimal_micros(price)?
        .checked_mul(decimal_micros(quantity)?)
        .ok_or_else(|| internal("notional overflow".to_string()))?
        / 1_000_000;
    u64::try_from(notional).map_err(|_| internal("notional out of range".to_string()))
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

/// vaultから本人の取引口座equityを取得する（リスク上限の判断に使う）。
async fn vault_trading_equity(session: &SessionHandle) -> Result<u64, ErrorCode> {
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
    Ok(balances?.0)
}

fn vault_principal() -> Result<Principal, ErrorCode> {
    let bytes = db::tx::query(db::repo::core_config::vault_principal)
        .map_err(map_db)?
        .ok_or_else(|| internal("vault principal is not configured".to_string()))?;
    Ok(Principal::from_slice(&bytes))
}

/// metaからasset indexと `szDecimals` を解決する（未取得・欠落はfail-closed）。
fn resolve_asset(market: &str) -> Result<(u32, u32), ErrorCode> {
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
            let index = u32::try_from(index).map_err(|_| internal("index overflow".to_string()))?;
            let sz_decimals = entry
                .get("szDecimals")
                .and_then(|value| value.as_u64())
                .ok_or(ErrorCode::PolicyUnavailable)?;
            let sz_decimals = u32::try_from(sz_decimals)
                .map_err(|_| internal("szDecimals overflow".to_string()))?;
            return Ok((index, sz_decimals));
        }
    }
    Err(ErrorCode::NotAllowed {
        code: api_types::error::NotAllowedCode::AssetNotAllowed,
    })
}

/// SL/TPトリガの受付検証（`docs/phase-0/api-contract.md` 3.1）。
///
/// 建玉単位（`positionTpsl`）の保護注文としてのみ受け付ける。
/// - `reduce_only`必須（トリガで建玉を増やさない）。
/// - トリガ価格は正で、価格と同じ精度条件に収まること。
/// - 建玉が存在し、`side`がその建玉の反対売買であること（建玉の向きとの整合）。
///
/// 市場価格に対する上下（longのSLは下・TPは上）は検証しない。coreはmark価格を
/// 持たず、entry価格で代用すると含み益のある建玉の逆指値を誤って拒否する。
/// 上下の妥当性は取引所が最終的に判定する。
fn validate_trigger(
    account_id: &[u8; 32],
    market: &str,
    args: &api_types::order::SubmitOrderArgs,
    trigger: &api_types::order::Trigger,
    sz_decimals: u32,
) -> Result<(), ErrorCode> {
    if !args.reduce_only {
        return Err(bad(
            BadRequestCode::MissingField,
            "trigger orders must be reduce-only",
        ));
    }
    let trigger_price =
        hl_types::decimal::Decimal::parse(&trigger.trigger_price).map_err(bad_decimal)?;
    if trigger_price.as_str().starts_with('-') || trigger_price.as_str() == "0" {
        return Err(bad(
            BadRequestCode::PriceOutOfRange,
            "trigger price must be positive",
        ));
    }
    trigger_price
        .validate_precision(6u32.saturating_sub(sz_decimals), 5)
        .map_err(|error| bad(BadRequestCode::PrecisionExceeded, &error.to_string()))?;

    let position = open_position(account_id, market)?;
    let is_long = !position.size.starts_with('-');
    let is_buy = matches!(args.side, api_types::order::Side::Buy);
    if is_buy == is_long {
        return Err(bad(
            BadRequestCode::MalformedPayload,
            "trigger side must reduce the position",
        ));
    }
    Ok(())
}

/// 署名対象の`order` actionを組み立てる（通常注文・トリガ注文の共通経路）。
///
/// 実装は`test-venue`ビルドでのみ使う（本番の署名パイプラインは未実装）。
/// トリガは建玉単位（`positionTpsl`）として送る（`docs/phase-0/api-contract.md` 3.1）。
#[cfg(feature = "test-venue")]
fn order_action(
    order: &db::repo::orders::SignableOrder,
    price: &str,
) -> Result<hl_types::action::OrderAction, ErrorCode> {
    let cloid = format!("0x{}", hex::encode(order.cloid));
    let (order_type, grouping) = match &order.trigger {
        Some(trigger) => {
            let tpsl = match trigger.kind.as_str() {
                "stop_loss" => hl_types::action::Tpsl::StopLoss,
                "take_profit" => hl_types::action::Tpsl::TakeProfit,
                other => return Err(internal(format!("unknown trigger kind: {other}"))),
            };
            (
                hl_types::action::OrderType::Trigger(hl_types::action::TriggerOrder {
                    is_market: trigger.is_market,
                    trigger_price: hl_types::decimal::Decimal::parse(&trigger.price)
                        .map_err(bad_decimal)?,
                    tpsl,
                }),
                hl_types::action::Grouping::PositionTpsl,
            )
        }
        None => (
            hl_types::action::OrderType::Limit {
                tif: if order.kind == "market_ioc" {
                    hl_types::action::TimeInForce::Ioc
                } else {
                    hl_types::action::TimeInForce::Gtc
                },
            },
            hl_types::action::Grouping::Na,
        ),
    };
    Ok(hl_types::action::OrderAction {
        orders: vec![hl_types::action::OrderRequest {
            asset_index: order.asset_index,
            is_buy: order.is_buy,
            price: hl_types::decimal::Decimal::parse(price).map_err(bad_decimal)?,
            size: hl_types::decimal::Decimal::parse(&order.quantity).map_err(bad_decimal)?,
            reduce_only: order.reduce_only,
            order_type,
            cloid: Some(cloid),
        }],
        grouping,
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

    let price = order.price.clone().ok_or_else(|| {
        bad(
            BadRequestCode::MissingField,
            "price is required for signing",
        )
    })?;
    let action = order_action(&order, &price)?;
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
    // 未承認の世代では署名しない。承認はvaultがmaster署名で行い永続化するため、
    // vaultの状態を確認してから鍵を使う（取引所に拒否される署名を送らない）。
    let approved = agent_approval(&order.account_id, generation.generation).await?;
    match approved {
        Some(approved)
            if approved.agent_address.as_ref() == generation.agent_address.as_slice() => {}
        _ => {
            return Err(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            });
        }
    }
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
    let order_type = match &order.trigger {
        Some(trigger) => serde_json::json!({
            "trigger": {
                "isMarket": trigger.is_market,
                "triggerPx": trigger.price,
                "tpsl": if trigger.kind == "stop_loss" { "sl" } else { "tp" },
            }
        }),
        None => serde_json::json!({
            "limit": {
                "tif": if order.kind == "market_ioc" { "Ioc" } else { "Gtc" },
            }
        }),
    };
    serde_json::json!({
        "type": "order",
        "orders": [{
            "a": order.asset_index,
            "b": order.is_buy,
            "p": price,
            "s": order.quantity,
            "r": order.reduce_only,
            "t": order_type,
        }],
        "grouping": if order.trigger.is_some() { "positionTpsl" } else { "na" },
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
    let action = order_action(order, &price)?;
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
                    db::repo::orders::mark_venue_rejected(connection, &order_id, now)?;
                    db::repo::orders::release_risk(
                        connection,
                        &order.account_id,
                        &order.client_request_id,
                    )?;
                    Ok(())
                })
                .map_err(map_db)?;
            }
            Err(_) => {
                // 送信した可能性がある。再送せず、**リスク予約も解放しない**
                // （解放すると同一資金で追加の注文ができ、二重エクスポージャになる。
                // 解消は照合の結果に従う）。
                db::tx::update(|connection| {
                    db::repo::orders::mark_unknown(connection, &order_id, now)?;
                    Ok(())
                })
                .map_err(map_db)?;
            }
        }
        processed += 1;
    }

    // 取消の送信（取消要求済みで未送信の注文）。
    let cancels = db::tx::query(|connection| db::repo::orders::cancel_candidates(connection, 4))
        .map_err(map_db)?;
    for order_id in cancels {
        if dispatch_cancel(&order_id, ic_cdk::api::time() / 1_000_000).await? {
            processed += 1;
        }
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
    // 本人の取引口座の注文だけを更新対象にする（oidは口座ごとに採番される）。
    let account_id = trading_account(&session).await?;
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
    db::tx::update(|connection| {
        db::repo::orders::apply_order_status(connection, &account_id, oid, state, now)
    })
    .map_err(map_db)
}

/// 取消actionをAgent鍵で署名して送信する（受理で`cancelled`へ）。
#[cfg(feature = "test-venue")]
async fn dispatch_cancel(order_id: &[u8; 32], now: u64) -> Result<bool, ErrorCode> {
    let claimed = db::tx::update(|connection| db::repo::orders::claim_cancel(connection, order_id))
        .map_err(map_db)?;
    if !claimed {
        return Ok(false);
    }
    let (account_id, asset_index, oid) =
        db::tx::query(|connection| db::repo::orders::cancel_target(connection, order_id))
            .map_err(map_db)?
            .ok_or_else(|| bad(BadRequestCode::MalformedPayload, "unknown order"))?;

    let action = hl_types::action::CancelAction {
        cancels: vec![(asset_index, oid)],
    };
    let msgpack = action.to_value().encode();
    let action_hash = hl_sign::hash::action_hash(&hl_sign::hash::ActionHashInput {
        action_msgpack: &msgpack,
        nonce: now,
        vault_address: None,
        expires_after: None,
    });
    let digest = hl_sign::hash::signing_digest(action_hash, false);

    let generation = db::tx::query(|connection| db::repo::agents::latest(connection, &account_id))
        .map_err(map_db)?
        .ok_or(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        })?;
    let path = agent_derivation_path(&account_id, generation.generation);
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
        "action": {
            "type": "cancel",
            "cancels": [{ "a": asset_index, "o": oid }],
        },
        "nonce": now,
        "signature": {
            "r": format!("0x{}", hex::encode(signature.r)),
            "s": format!("0x{}", hex::encode(signature.s)),
            "v": signature.v,
        },
    });
    let body = serde_json::to_vec(&body).map_err(|error| internal(error.to_string()))?;

    match post_order(&body).await {
        Ok((ExchangeOutcome::Accepted, _)) => {
            db::tx::update(|connection| {
                db::repo::orders::mark_cancel_sent(connection, order_id, &body, now)
            })
            .map_err(map_db)?;
        }
        Ok((ExchangeOutcome::Rejected, _)) | Err(_) => {
            // 拒否・不明のいずれも「送ったか不明」として保持する（再送しない）。
            db::tx::update(|connection| {
                db::repo::orders::mark_cancel_unknown(connection, order_id, now)
            })
            .map_err(map_db)?;
        }
    }
    Ok(true)
}

/// テスト専用：`clearinghouseState`相当の建玉を取り込む（`test-venue`限定）。
#[cfg(feature = "test-venue")]
#[ic_cdk::update]
async fn test_ingest_positions(
    session: api_types::auth::SessionHandle,
    positions_json: String,
) -> Result<u32, ErrorCode> {
    authorize(&session).await?;
    let account_id = trading_account(&session).await?;
    let now = ic_cdk::api::time() / 1_000_000;
    let value: serde_json::Value =
        serde_json::from_str(&positions_json).map_err(|error| internal(error.to_string()))?;
    let entries = value
        .get("assetPositions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut count = 0;
    let mut observed = Vec::new();
    for entry in entries {
        let Some(position) = entry.get("position") else {
            continue;
        };
        let Some(coin) = position.get("coin").and_then(|value| value.as_str()) else {
            continue;
        };
        // 未実現損益はUSD建ての十進文字列。ローカルではf64で近似する（厳密な桁は照合段階の課題）。
        let unrealized_pnl = position
            .get("unrealizedPnl")
            .and_then(|value| value.as_str())
            .and_then(|text| text.parse::<f64>().ok())
            .map(|value| (value * 1_000_000.0).round() as i64)
            .unwrap_or(0);
        let view = api_types::order::PositionView {
            market: coin.to_string(),
            size: position
                .get("szi")
                .and_then(|value| value.as_str())
                .unwrap_or("0")
                .to_string(),
            entry_price: position
                .get("entryPx")
                .and_then(|value| value.as_str())
                .unwrap_or("0")
                .to_string(),
            liquidation_price: position
                .get("liquidationPx")
                .and_then(|value| value.as_str())
                .map(|text| text.to_string()),
            unrealized_pnl,
            leverage: position
                .get("leverage")
                .and_then(|value| value.get("value"))
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
            margin_mode: position
                .get("marginMode")
                .and_then(|value| value.as_str())
                .unwrap_or("cross")
                .to_string(),
            stop_loss: None,
            take_profit: None,
        };
        observed.push(view);
        count += 1;
    }
    // 観測は建玉の全量であるため、消えた建玉（決済済み）を残さない。
    db::tx::update(|connection| {
        db::repo::positions::replace_all(connection, &account_id, &observed, now)
    })
    .map_err(map_db)?;
    Ok(count)
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

//! 注文・取消の送信と取引所状態の照合（sweep）。
//!
//! `docs/phase-0/state-machines.md` 4節のとおり、受付（`submit_order`）は
//! `pending`を書くだけで、署名・送信・照合はここが担う。`heartbeat`（本番）と
//! 明示的なsweep（`sweep`・`test_sweep_now`）が同じ経路を通る。
//!
//! 送信結果の分類を守る：
//! - 受理 → `open`＋oid（oidが無い即時約定は`filled`の照合に委ねる）
//! - 拒否 → `rejected`＋リスク予約の解放
//! - 不明 → `unknown`（**再送しない**。リスク予約も解放しない。解消は照合で行う）

use crate::venue::{self, ExchangeOutcome};
use crate::{
    agent_approval, agent_derivation_path, bad, bad_decimal, ecdsa_key_id, internal, map_db,
};
use api_types::error::{BadRequestCode, ErrorCode};
use ic_cdk_management_canister::{
    EcdsaPublicKeyArgs, SignWithEcdsaArgs, ecdsa_public_key, sign_with_ecdsa,
};

/// 1回のsweepで送る注文・取消の上限（outcallの回数を抑える）。
const MAX_DISPATCH_PER_SWEEP: u32 = 4;
const MAX_CANCEL_PER_SWEEP: u32 = 4;
/// 1回のsweepで照合する口座の上限。
pub const MAX_RECONCILE_PER_SWEEP: u32 = 2;
/// 1口座あたり1回のsweepで問い合わせる注文状態の上限。
const MAX_STATUS_CHECKS_PER_ACCOUNT: u32 = 4;
/// 自動sweep（heartbeat）の間隔（ミリ秒）。建玉の鮮度（10秒）より短くし、
/// 新規リスクの受付を止めない。試験ビルドは自動sweepを行わないため定数も持たない。
#[cfg(not(feature = "test-venue"))]
pub const SWEEP_INTERVAL_MS: u64 = 5_000;

/// 1回のsweep（送信・取消・照合）。件数の内訳を返す。
pub async fn sweep_once(now: u64) -> Result<api_types::order::SweepOutcome, ErrorCode> {
    Ok(api_types::order::SweepOutcome {
        dispatched: dispatch_queued(now).await?,
        cancels: dispatch_cancels(now).await?,
        reconciled: reconcile_accounts(now).await?,
    })
}

/// 受付済み（`queued`）の注文へ署名して送信する。
async fn dispatch_queued(now: u64) -> Result<u32, ErrorCode> {
    let ids = db::tx::query(|connection| {
        db::repo::orders::queued_orders(connection, MAX_DISPATCH_PER_SWEEP)
    })
    .map_err(map_db)?;
    let mut processed = 0;
    for order_id in ids {
        // 送信権を先に取る（同時実行でも二重送信しない）。
        let claimed = db::tx::update(|connection| {
            db::repo::orders::claim_for_dispatch(connection, &order_id)
        })
        .map_err(map_db)?;
        if !claimed {
            continue;
        }
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

        match venue::post_exchange(&body).await {
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
                // （解放すると同一資金で追加の注文ができ、二重エクスポージャになる）。
                db::tx::update(|connection| {
                    db::repo::orders::mark_unknown(connection, &order_id, now)?;
                    Ok(())
                })
                .map_err(map_db)?;
            }
        }
        processed += 1;
    }
    Ok(processed)
}

/// 取消要求済みの注文へ署名して送信する（受理で`cancelled`へ）。
async fn dispatch_cancels(now: u64) -> Result<u32, ErrorCode> {
    let ids = db::tx::query(|connection| {
        db::repo::orders::cancel_candidates(connection, MAX_CANCEL_PER_SWEEP)
    })
    .map_err(map_db)?;
    let mut processed = 0;
    for order_id in ids {
        if dispatch_cancel(&order_id, now).await? {
            processed += 1;
        }
    }
    Ok(processed)
}

/// 取消actionをAgent鍵で署名して送信する。
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
    let signature = sign_with_agent_key(&account_id, digest).await?;

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

    match venue::post_exchange(&body).await {
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

/// 有効な口座を巡回し、建玉・約定・注文状態を取り込む。
async fn reconcile_accounts(now: u64) -> Result<u32, ErrorCode> {
    let cursor = db::tx::query(db::repo::accounts::reconcile_cursor).map_err(map_db)?;
    let mut candidates = db::tx::query(|connection| {
        db::repo::accounts::reconcile_candidates(
            connection,
            cursor.as_ref(),
            MAX_RECONCILE_PER_SWEEP,
        )
    })
    .map_err(map_db)?;
    if candidates.is_empty() && cursor.is_some() {
        // 末尾まで来たら先頭から巡回し直す（最後の口座が永久に対象外にならない）。
        candidates = db::tx::query(|connection| {
            db::repo::accounts::reconcile_candidates(connection, None, MAX_RECONCILE_PER_SWEEP)
        })
        .map_err(map_db)?;
    }
    let mut processed = 0;
    for account in candidates {
        // 1口座の失敗で巡回全体を止めない（次のsweepで再試行する）。
        if reconcile_account(&account, now).await.is_ok() {
            processed += 1;
        }
        db::tx::update(|connection| {
            db::repo::accounts::set_reconcile_cursor(connection, &account.account_id, now)
        })
        .map_err(map_db)?;
    }
    Ok(processed)
}

/// 1口座分の照合（建玉の全量・約定・未終端注文の状態）。
async fn reconcile_account(
    account: &db::repo::accounts::AccountRow,
    now: u64,
) -> Result<(), ErrorCode> {
    let address = format!("0x{}", hex::encode(account.master_address));

    // 建玉は観測の全量で置き換える（消えた建玉を残さない）。
    let state = venue::clearinghouse_state(&address).await?;
    db::tx::update(|connection| {
        ingest_positions_json(connection, &account.account_id, &state, now)
    })
    .map_err(map_db)?;

    // 約定は`tid`で冪等に取り込む。
    let fills = venue::user_fills(&address).await?;
    db::tx::update(|connection| ingest_fills_json(connection, &account.user_id, &fills, now))
        .map_err(map_db)?;

    // 未終端でoidが分かっている注文の状態を問い合わせる。
    let oids = db::tx::query(|connection| {
        db::repo::orders::oids_awaiting_status(
            connection,
            &account.account_id,
            MAX_STATUS_CHECKS_PER_ACCOUNT,
        )
    })
    .map_err(map_db)?;
    for oid in oids {
        let status = venue::order_status(&address, oid).await?;
        db::tx::update(|connection| {
            apply_order_status_json(connection, &account.account_id, &status, now)
        })
        .map_err(map_db)?;
    }
    Ok(())
}

/// 建玉の全量を取り込む（`clearinghouseState`の本文。テスト専用フックと共用）。
pub fn ingest_positions_json(
    connection: &mut ic_sqlite_vfs::db::UpdateConnection<'_>,
    account_id: &[u8; 32],
    body: &str,
    now: u64,
) -> Result<u32, db::error::Error> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| db::error::Error::Invariant("invalid json"))?;
    let entries = value
        .get("assetPositions")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
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
        observed.push(api_types::order::PositionView {
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
        });
    }
    let count = u32::try_from(observed.len()).unwrap_or(u32::MAX);
    db::repo::positions::replace_all(connection, account_id, &observed, now)?;
    Ok(count)
}

/// 約定の一覧を取り込む（`userFills`の本文。テスト専用フックと共用）。
pub fn ingest_fills_json(
    connection: &mut ic_sqlite_vfs::db::UpdateConnection<'_>,
    user_id: &[u8; 32],
    body: &str,
    now: u64,
) -> Result<u32, db::error::Error> {
    let fills: Vec<serde_json::Value> =
        serde_json::from_str(body).map_err(|_| db::error::Error::Invariant("invalid json"))?;
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
        let inserted = db::repo::orders::ingest_fill(
            connection,
            user_id,
            &db::repo::orders::NewFill {
                tid,
                hl_oid: oid,
                market: &coin,
                price: &price,
                quantity: &quantity,
                fee,
                filled_at: at,
            },
        )?;
        if inserted {
            ingested += 1;
        }
    }
    Ok(ingested)
}

/// `orderStatus`の本文を注文へ反映する（テスト専用フックと共用）。
pub fn apply_order_status_json(
    connection: &mut ic_sqlite_vfs::db::UpdateConnection<'_>,
    account_id: &[u8; 32],
    body: &str,
    now: u64,
) -> Result<bool, db::error::Error> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| db::error::Error::Invariant("invalid json"))?;
    let status = value
        .get("status")
        .and_then(|status| status.as_str())
        .unwrap_or("unknown")
        .to_string();
    let oid = value
        .get("order")
        .and_then(|order| order.get("oid"))
        .and_then(|oid| oid.as_u64())
        .ok_or(db::error::Error::Invariant("orderStatus without oid"))?;
    // 取引所の語彙をこちらの状態へ写す（未知はunknownとして保持）。
    let state = match status.as_str() {
        "open" => "open",
        "filled" => "filled",
        "canceled" | "cancelled" => "cancelled",
        "rejected" => "rejected",
        _ => "unknown",
    };
    db::repo::orders::apply_order_status(connection, account_id, oid, state, now)
}

/// 注文の署名actionを組み立てる（通常注文・トリガ注文の共通経路）。
///
/// トリガは建玉単位（`positionTpsl`）として送る（`docs/phase-0/api-contract.md` 3.1）。
pub fn order_action(
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

/// 注文のaction JSON（HLの`/exchange`はJSON actionを取る）。
pub fn order_action_json(
    order: &db::repo::orders::SignableOrder,
    price: &str,
) -> serde_json::Value {
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

/// 署名と送信本文を作る（action msgpackのハッシュへAgent鍵で署名する）。
pub async fn sign_and_build(
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
    let signature = sign_with_agent_key(&order.account_id, digest).await?;

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

/// 口座の現在世代のAgent鍵でdigestへ署名する（`v`は公開鍵から復元する）。
///
/// 未承認の世代では署名しない（取引所に拒否される署名を送らない）。
pub(crate) async fn sign_with_agent_key(
    account_id: &[u8; 32],
    digest: [u8; 32],
) -> Result<hl_sign::Signature, ErrorCode> {
    let generation = db::tx::query(|connection| db::repo::agents::latest(connection, account_id))
        .map_err(map_db)?
        .ok_or(ErrorCode::NotAllowed {
            code: api_types::error::NotAllowedCode::OperationNotAvailable,
        })?;
    let approved = agent_approval(account_id, generation.generation).await?;
    match approved {
        Some(approved)
            if approved.agent_address.as_ref() == generation.agent_address.as_slice() => {}
        _ => {
            return Err(ErrorCode::NotAllowed {
                code: api_types::error::NotAllowedCode::OperationNotAvailable,
            });
        }
    }
    let path = agent_derivation_path(account_id, generation.generation);
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
    Ok(hl_sign::Signature { r, s, v })
}

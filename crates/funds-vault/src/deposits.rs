//! 取引所の入金の取得（replicatedな`/info`）と取り込み。
//!
//! 取得はreplicated outcall＋決定論的な変換関数で行い、取り込みは正規化した
//! イベントID（`keccak256("deposit" ‖ tx_hash)`）で二重計上を防ぐ。宛先が導出口座
//! （`custody_accounts.master_address`）と一致すれば本人へ計上し、未知の宛先は記録のみ。

use crate::clock;
use api_types::error::ErrorCode;
use db::error::Error as DbError;
use ic_cdk_management_canister::{HttpMethod, HttpRequest, transform_context_from_query};
use ic_sqlite_vfs::db::UpdateConnection;

/// 変換関数：必要な要素だけを決定論的に残す（順序・付随フィールドの揺れを除く）。
#[ic_cdk::query]
fn transform_info(
    args: ic_cdk_management_canister::TransformArgs,
) -> ic_cdk_management_canister::HttpRequestResult {
    let canonical = serde_json::from_slice::<serde_json::Value>(&args.response.body)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .map(|entries| {
            let trimmed: Vec<serde_json::Value> = entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "hash": entry.get("hash").cloned().unwrap_or(serde_json::Value::Null),
                        "time": entry.get("time").cloned().unwrap_or(serde_json::Value::Null),
                        "usdc": entry
                            .get("delta")
                            .and_then(|delta| delta.get("usdc"))
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    })
                })
                .collect();
            serde_json::Value::Array(trimmed)
        });
    let body = canonical
        .map(|value| value.to_string().into_bytes())
        .unwrap_or_default();
    ic_cdk_management_canister::HttpRequestResult {
        status: args.response.status,
        headers: Vec::new(),
        body,
    }
}

/// 入金（non-funding ledger updates）をreplicated outcallで取得する。
pub async fn fetch_ledger_updates(user: &str) -> Result<Vec<u8>, ErrorCode> {
    let info_url = crate::environment::resolved()?.info_url;
    let body = serde_json::json!({
        "type": "userNonFundingLedgerUpdates",
        "user": user,
    })
    .to_string();
    let response = HttpRequest::new(&info_url)
        .with_method(HttpMethod::POST)
        .with_header("Content-Type", "application/json")
        .with_body(body.into_bytes())
        .with_max_response_bytes(16 * 1024)
        .with_transform(transform_context_from_query(
            "transform_info".to_string(),
            Vec::new(),
        ))
        .send()
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: error.to_string(),
        })?;
    Ok(response.body)
}

/// 十進文字列をマイクロUSDCへ（丸めない）。
pub fn decimal_micros(text: &str) -> Result<u64, ErrorCode> {
    let (integer, fraction) = match text.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (text, ""),
    };
    let integer: u128 = integer.parse().map_err(|_| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: "invalid amount".to_string(),
    })?;
    if fraction.len() > 6 {
        return Err(ErrorCode::BadRequest {
            code: api_types::error::BadRequestCode::MalformedPayload,
            detail: "more than 6 decimals".to_string(),
        });
    }
    let mut padded = fraction.to_string();
    while padded.len() < 6 {
        padded.push('0');
    }
    let fraction: u128 = padded.parse().map_err(|_| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: "invalid amount".to_string(),
    })?;
    u64::try_from(integer * 1_000_000 + fraction).map_err(|_| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: "amount out of range".to_string(),
    })
}

/// 入金として計上できる額（マイクロUSDC）だけを返す。
///
/// `userNonFundingLedgerUpdates` には送金・出金などの**負の**deltaやゼロ・非十進が混ざる。
/// これらは入金ではないため `None` とし、照合側は読み飛ばす（1件の負値で巡回全体を
/// 止めない）。
pub fn deposit_amount_micros(text: &str) -> Option<u64> {
    if text.starts_with('-') || text.starts_with('+') {
        return None;
    }
    match decimal_micros(text) {
        Ok(amount) if amount > 0 => Some(amount),
        _ => None,
    }
}

/// 入金を取り込む。既知の`tx_hash`なら`false`。
///
/// 宛先が未知の入金も資金は既に動いているため、suspense勘定へ計上して記録に残す
/// （イベント行を先に入れるため、後から `credit` を呼び直して計上することはできない。
/// 写像が判明した時点で controller が `claim_unmatched_deposit` で本人へ振り替える）。
pub fn credit(
    connection: &mut UpdateConnection<'_>,
    network: &str,
    tx_hash: &[u8],
    amount: u64,
    address: &[u8; 20],
    asset: &str,
    now: u64,
) -> Result<bool, DbError> {
    let mut input = b"deposit".to_vec();
    input.extend_from_slice(tx_hash);
    let event_id = hl_sign::keccak256(&input);

    let event = db::repo::events::ExternalEvent {
        event_id,
        network: network.to_string(),
        account_address: *address,
        counterparty: [0u8; 20],
        asset: asset.to_string(),
        amount,
        kind: "deposit".to_string(),
        at: now,
        evidence_ref: Some(hex::encode(tx_hash)),
    };
    if !db::repo::events::ingest_external_event(connection, &event, now)? {
        return Ok(false);
    }
    match db::repo::ledger::custody_account_by_address(connection, address)? {
        Some(owner) if owner.kind == "trading" => {
            // 取引口座への着金＝配分の確定（移動中→取引）。移動中の額を超える分は
            // 配分として説明できないため、取引口座への直接入金として与信する
            // （そうしないと `user_in_transit` が負債超過になり残高参照が壊れる）。
            let in_transit = db::repo::ledger::user_in_transit_balance(connection, &owner.user_id)?;
            let confirmed = amount.min(in_transit);
            if confirmed > 0 {
                db::repo::ledger::allocation_confirm(
                    connection,
                    &owner.user_id,
                    &owner.account_id,
                    confirmed,
                    now,
                    &event_id,
                )?;
            }
            let excess = amount - confirmed;
            if excess > 0 {
                db::repo::ledger::trading_deposit_confirmed(
                    connection,
                    &owner.account_id,
                    excess,
                    now,
                )?;
                db::repo::events::insert_audit(
                    connection,
                    "system",
                    "trading_deposit_direct",
                    None,
                    Some(&hex::encode(address)),
                    now,
                )?;
            }
        }
        Some(owner) => {
            // 準備口座への着金＝利用者への与信。
            db::repo::ledger::deposit_confirmed(
                connection,
                &owner.user_id,
                amount,
                now,
                &event_id,
            )?;
        }
        None => {
            db::repo::ledger::unmatched_deposit(connection, amount, now, &event_id)?;
            db::repo::events::insert_audit(
                connection,
                "system",
                "unmatched_deposit",
                None,
                Some(&hex::encode(address)),
                now,
            )?;
        }
    }
    Ok(true)
}

/// 現在時刻（ミリ秒）。
pub fn now_ms() -> u64 {
    clock::now_ms()
}

/// 有界な定期照合。カーソルの位置から `limit` 件の入金先を確認して計上する。
///
/// 先頭N件固定では3人目以降が永久に対象外になるため、`(created_at, master_address)` の
/// キーセットで巡回し、末尾まで到達したら次回は先頭から始める。1件の取得失敗や
/// 解釈できないイベントで巡回全体を止めない（次のheartbeatで再試行される）。
/// 有効な入金先を巡回して入金を取り込む（自動sweep専用。試験は`reconcile_deposits`を使う）。
#[cfg(not(feature = "test-venue"))]
pub async fn reconcile_all(limit: u32) -> Result<u32, ErrorCode> {
    let internal = |error: DbError| ErrorCode::Internal {
        code: format!("{error:?}"),
    };
    let cursor = db::tx::query(db::repo::ledger::reconcile_cursor).map_err(internal)?;
    let addresses = db::tx::query(|connection| {
        db::repo::ledger::reserve_addresses_after(connection, limit, cursor)
    })
    .map_err(internal)?;

    let network = crate::environment::network_name()?;
    let mut credited = 0;
    let mut last = None;
    for (address, created_at) in addresses {
        last = Some((created_at, address));
        let body = match fetch_ledger_updates(&format!("0x{}", hex::encode(address))).await {
            Ok(body) => body,
            Err(error) => {
                let now = now_ms();
                db::tx::update(|connection| {
                    db::repo::events::insert_audit(
                        connection,
                        "system",
                        "reconcile_fetch_failed",
                        None,
                        Some(&format!("{error:?}")),
                        now,
                    )
                })
                .map_err(internal)?;
                continue;
            }
        };
        let Ok(entries) = serde_json::from_slice::<Vec<serde_json::Value>>(&body) else {
            continue;
        };
        let now = now_ms();
        for entry in entries {
            let Some(hash) = entry.get("hash").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(usdc) = entry.get("usdc").and_then(|value| value.as_str()) else {
                continue;
            };
            // 負値・ゼロ・非十進は入金ではない（送金・出金など）。
            let Some(amount) = deposit_amount_micros(usdc) else {
                continue;
            };
            let hash = hash.strip_prefix("0x").unwrap_or(hash);
            let Ok(tx_hash) = hex::decode(hash) else {
                continue;
            };
            let at = entry
                .get("time")
                .and_then(|value| value.as_u64())
                .unwrap_or(now);
            let inserted = db::tx::update(|connection| {
                credit(connection, &network, &tx_hash, amount, &address, "usdc", at)
            })
            .map_err(internal)?;
            if inserted {
                credited += 1;
            }
        }
    }

    let now = now_ms();
    match last {
        Some((created_at, address)) => {
            db::tx::update(|connection| {
                db::repo::ledger::set_reconcile_cursor(connection, created_at, &address, now)
            })
            .map_err(internal)?;
        }
        None => {
            // 末尾まで到達した（または対象が無い）。次回は先頭から巡回する。
            db::tx::update(|connection| db::repo::ledger::reset_reconcile_cursor(connection, now))
                .map_err(internal)?;
        }
    }
    Ok(credited)
}

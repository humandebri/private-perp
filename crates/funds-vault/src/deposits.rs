//! 取引所の入金の取得（replicatedな`/info`）と取り込み。
//!
//! 取得はreplicated outcall＋変換関数で行い、取り込みは正規化した
//! イベントIDで二重計上を防ぐ。共通保管口座への入金はHLの送金元と認証EOAを
//! 照合して本人へ計上する。送金元の証跡がない入金は未帰属勘定に保持する。

use crate::clock;
use api_types::error::ErrorCode;
use api_types::journal::{RecoveryEvent, RecoveryPayload};
use api_types::operations::BudgetClass;
use db::error::Error as DbError;
use ic_cdk_management_canister::{HttpMethod, HttpRequest, transform_context_from_query};
use ic_sqlite_vfs::db::UpdateConnection;

/// 変換関数：必要な要素だけを決定論的に残す（順序・付随フィールドの揺れを除く）。
#[cfg_attr(not(feature = "embedded"), ic_cdk::query)]
#[cfg_attr(feature = "embedded", ic_cdk::query(name = "vault_transform_info"))]
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
                        "delta": {
                            "type": entry.get("delta").and_then(|delta| delta.get("type")).cloned().unwrap_or(serde_json::Value::Null),
                            "user": entry.get("delta").and_then(|delta| delta.get("user")).cloned().unwrap_or(serde_json::Value::Null),
                            "destination": entry.get("delta").and_then(|delta| delta.get("destination")).cloned().unwrap_or(serde_json::Value::Null),
                            "usdc": entry.get("delta").and_then(|delta| delta.get("usdc")).cloned().unwrap_or(serde_json::Value::Null),
                        },
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
pub async fn fetch_ledger_updates_range(
    user: &str,
    start_time: u64,
    end_time: Option<u64>,
) -> Result<Vec<u8>, ErrorCode> {
    // This info endpoint has a base weight of 20 and a return-size surcharge.
    // Reserve the documented maximum of 2000 rows before starting the outcall.
    let permit = crate::rest_budget::acquire(BudgetClass::Reconcile, 120).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let info_url = crate::environment::resolved()?.info_url;
    let mut body = serde_json::json!({
        "type": "userNonFundingLedgerUpdates",
        "user": user,
        "startTime": start_time,
    });
    if let Some(end_time) = end_time {
        body["endTime"] = serde_json::json!(end_time);
    }
    let body = body.to_string();
    let response = HttpRequest::new(&info_url)
        .with_method(HttpMethod::POST)
        .with_header("Content-Type", "application/json")
        .with_body(body.into_bytes())
        .with_max_response_bytes(512 * 1024)
        .with_transform(transform_context_from_query(
            if cfg!(feature = "embedded") {
                "vault_transform_info".to_string()
            } else {
                "transform_info".to_string()
            },
            Vec::new(),
        ))
        .send()
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: error.to_string(),
        })?;
    if response.status.to_string() != "200" {
        return Err(ErrorCode::UpstreamUnavailable {
            venue: format!("ledger info HTTP {}", response.status),
        });
    }
    Ok(response.body)
}

/// 入金として計上できる額（マイクロUSDC）だけを返す。
///
/// `userNonFundingLedgerUpdates` には送金・出金などの**負の**deltaやゼロ・非十進が混ざる。
/// これらは入金ではないため `None` とし、照合側は読み飛ばす（1件の負値で巡回全体を
/// 止めない）。
pub fn deposit_amount_micros(value: &serde_json::Value) -> Option<u64> {
    let amount = crate::amount::parse(value)?;
    (!amount.negative && amount.micros > 0).then_some(amount.micros)
}

/// Only credit genuine deposits or inbound transfers. A managed trading to
/// reserve recovery is settled by its own action and must not be credited twice.
pub fn creditable_entry(entry: &serde_json::Value, address: &[u8; 20]) -> Result<bool, DbError> {
    let delta = entry.get("delta");
    match delta
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
    {
        Some("deposit") => Ok(true),
        Some("internalTransfer") => {
            let expected = format!("0x{}", hex::encode(address));
            if delta
                .and_then(|value| value.get("destination"))
                .and_then(|value| value.as_str())
                .is_none_or(|destination| !destination.eq_ignore_ascii_case(&expected))
            {
                return Ok(false);
            }
            let Some(sender) = delta
                .and_then(|value| value.get("user"))
                .and_then(|value| value.as_str())
            else {
                return Ok(false);
            };
            let Ok(sender_bytes) = hex::decode(sender.strip_prefix("0x").unwrap_or(sender)) else {
                return Ok(false);
            };
            let Ok(sender_address) = <[u8; 20]>::try_from(sender_bytes) else {
                return Ok(false);
            };
            let managed_recovery = db::tx::query(|connection| {
                let from =
                    db::repo::ledger::custody_account_by_address(connection, &sender_address)?;
                let to = db::repo::ledger::custody_account_by_address(connection, address)?;
                Ok::<bool, DbError>(matches!((from, to), (Some(from), Some(to))
                    if from.kind == "trading" && to.kind == "reserve"))
            })?;
            Ok(!managed_recovery)
        }
        _ => Ok(false),
    }
}

/// Only an actual inbound internalTransfer supplies ownership evidence. A bridge
/// deposit's `user` field is not proof of the depositor's EOA.
pub fn transfer_sender(entry: &serde_json::Value) -> Option<[u8; 20]> {
    let delta = entry.get("delta")?;
    if delta.get("type")?.as_str()? != "internalTransfer" {
        return None;
    }
    let value = delta.get("user")?.as_str()?;
    hex::decode(value.strip_prefix("0x").unwrap_or(value))
        .ok()?
        .try_into()
        .ok()
}

/// 入金を取り込む。既知の`tx_hash`なら`false`。
///
/// 宛先が未知の入金も資金は既に動いているため、suspense勘定へ計上して記録に残す
/// （イベント行を先に入れるため、後から `credit` を呼び直して計上することはできない。
/// 写像が判明した時点で controller が `claim_unmatched_deposit` で本人へ振り替える）。
#[allow(clippy::too_many_arguments)]
pub fn credit(
    connection: &mut UpdateConnection<'_>,
    network: &str,
    tx_hash: &[u8],
    amount: u64,
    address: &[u8; 20],
    asset: &str,
    now: u64,
    sender: Option<&[u8; 20]>,
) -> Result<bool, DbError> {
    let mut input = b"deposit".to_vec();
    input.extend_from_slice(tx_hash);
    let event_id = hl_sign::keccak256(&input);

    db::repo::deposits::credit_external_deposit(
        connection, &event_id, network, tx_hash, amount, address, asset, now, sender,
    )
}

/// Persist the venue evidence before changing any balance. The receipt and
/// ledger mutation commit together; an ambiguous journal response locks sends.
pub async fn credit_journaled(
    network: &str,
    tx_hash: &[u8],
    amount: u64,
    address: &[u8; 20],
    at: u64,
    sender: Option<[u8; 20]>,
) -> Result<bool, ErrorCode> {
    let internal = |error: DbError| ErrorCode::Internal {
        code: format!("{error:?}"),
    };
    if tx_hash.is_empty() || tx_hash.len() > 64 || amount == 0 || amount > i64::MAX as u64 {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let mut input = b"deposit".to_vec();
    input.extend_from_slice(tx_hash);
    let event_id = hl_sign::keccak256(&input);
    if db::tx::query(|c| db::repo::events::find_external_event(c, network, &event_id))
        .map_err(&internal)?
        .is_some()
    {
        return Ok(false);
    }
    db::tx::query(|c| db::repo::deposits::ensure_allocation_ready(c, address, sender.as_ref()))
        .map_err(&internal)?;
    let mut logical = b"deposit_credit".to_vec();
    logical.extend_from_slice(network.as_bytes());
    logical.extend_from_slice(&event_id);
    let event = RecoveryEvent {
        version: 1,
        logical_id: hl_sign::keccak256(&logical).to_vec().into(),
        payload: RecoveryPayload::DepositCredit {
            sender: sender.map(|address| address.to_vec().into()),
            tx_hash: tx_hash.to_vec().into(),
            network: network.to_string(),
            address: address.to_vec().into(),
            amount_micros: amount,
            observed_at_ms: at,
        },
    };
    let ack = journal_client::append_recovery_event_if("vault", event.clone(), |connection| {
        Ok(db::repo::events::find_external_event(connection, network, &event_id)?.is_none())
    })
    .await?;
    let Some(ack) = ack else {
        return Ok(false);
    };
    let result = db::tx::update(|c| {
        journal_client::record_recovery_event(c, &event, &ack)?;
        if !credit(
            c,
            network,
            tx_hash,
            amount,
            address,
            "usdc",
            at,
            sender.as_ref(),
        )? {
            return Err(DbError::Conflict);
        }
        Ok(true)
    });
    match result {
        Ok(inserted) => Ok(inserted),
        Err(error) => {
            journal_client::lock()?;
            Err(internal(error))
        }
    }
}

/// 現在時刻（ミリ秒）。
pub fn now_ms() -> u64 {
    clock::now_ms()
}

/// 有界な定期照合。カーソルの位置から `limit` 件のcustody口座を確認して計上する。
///
/// 先頭N件固定では3人目以降が永久に対象外になるため、`(created_at, master_address)` の
/// キーセットで巡回し、末尾まで到達したら次回は先頭から始める。1件の取得失敗や
/// 解釈できないイベントで巡回全体を止めない（次のheartbeatで再試行される）。
/// reserveへの入金とtradingへのallocation着金を巡回して取り込む
/// （自動sweep専用。試験は`reconcile_deposits`を使う）。
#[cfg(not(feature = "test-venue"))]
pub async fn reconcile_all(limit: u32) -> Result<u32, ErrorCode> {
    let internal = |error: DbError| ErrorCode::Internal {
        code: format!("{error:?}"),
    };
    let cursor = db::tx::query(db::repo::ledger::reconcile_cursor).map_err(internal)?;
    let addresses = db::tx::query(|connection| {
        db::repo::ledger::custody_addresses_after(connection, limit, cursor)
    })
    .map_err(internal)?;

    let mut credited = 0;
    let mut last = None;
    for (address, created_at) in addresses {
        last = Some((created_at, address));
        match reconcile_address(&address).await {
            Ok(count) => {
                credited += count;
                if let Some(owner) =
                    db::tx::query(|c| db::repo::ledger::custody_account_by_address(c, &address))
                        .map_err(internal)?
                    && owner.kind == "trading"
                    && let Some(user) = owner.user_id
                {
                    // Pending transfers intentionally defer observations; keep rotating accounts.
                    let _ = crate::balance::refresh(&user).await;
                }
            }
            Err(error) => {
                ic_cdk::println!("deposit page reconciliation failed: {error:?}");
                // Continue rotating addresses; this address retains its page cursor.
                db::tx::update(|connection| {
                    db::repo::events::insert_audit(
                        connection,
                        "system",
                        "reconcile_fetch_failed",
                        None,
                        Some("deposit_page_incomplete"),
                        now_ms(),
                    )
                })
                .map_err(internal)?;
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

/// Fetch one bounded page per address/turn. The inclusive boundary is intentional:
/// advancing by one millisecond can drop transfers sharing the last timestamp.
pub async fn reconcile_address(address: &[u8; 20]) -> Result<u32, ErrorCode> {
    let internal = |error: DbError| ErrorCode::Internal {
        code: format!("{error:?}"),
    };
    let network = crate::environment::network_name()?;
    let start = db::tx::query(|c| db::repo::ledger::deposit_history_start(c, &network, address))
        .map_err(&internal)?;
    let body =
        fetch_ledger_updates_range(&format!("0x{}", hex::encode(address)), start, None).await?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_slice(&body).map_err(|_| ErrorCode::UpstreamRejected {
            code: "unexpected deposit history response".into(),
            retryable: true,
        })?;
    let mut last = start;
    for entry in &entries {
        let time = entry
            .get("time")
            .and_then(|v| v.as_u64())
            .filter(|time| *time >= start && *time <= i64::MAX as u64 && *time > 0)
            .ok_or_else(|| ErrorCode::UpstreamRejected {
                code: "invalid deposit history timestamp".into(),
                retryable: true,
            })?;
        last = last.max(time);
    }
    // The API has no offset within a timestamp. Never silently skip a saturated
    // boundary: keep it pending for a complete evidence source instead.
    if entries.len() >= 500 && last == start {
        return Err(ErrorCode::UpstreamRejected {
            code: "deposit history timestamp saturated; complete evidence required".into(),
            retryable: false,
        });
    }
    let mut credited = 0;
    for entry in entries {
        if !creditable_entry(&entry, address).map_err(&internal)? {
            continue;
        }
        let Some(amount) = entry.get("usdc").and_then(deposit_amount_micros) else {
            continue;
        };
        let tx_hash = entry
            .get("hash")
            .and_then(|v| v.as_str())
            .and_then(|hash| hex::decode(hash.strip_prefix("0x").unwrap_or(hash)).ok())
            .filter(|hash| !hash.is_empty() && hash.len() <= 64)
            .ok_or_else(|| ErrorCode::UpstreamRejected {
                code: "invalid deposit history hash".into(),
                retryable: true,
            })?;
        let at = entry["time"].as_u64().expect("validated timestamp");
        if credit_journaled(
            &network,
            &tx_hash,
            amount,
            address,
            at,
            transfer_sender(&entry),
        )
        .await?
        {
            credited += 1;
        }
    }
    db::tx::update(|c| db::repo::ledger::advance_deposit_history(c, &network, address, last))
        .map_err(internal)?;
    Ok(credited)
}

//! Hyperliquid REST（`/exchange`の送信と`/info`の取得）。
//!
//! `docs/phase-0/api-contract.md` 6節・`Implementation.md` 5.4に従い、状態変更POSTは
//! **非replicated** outcallで送る（応答の揺れを合意の対象にしない。解釈できない応答は
//! 「未実行」と扱わず、呼び出し側が`unknown`へ進める）。読み取り（`/info`）は
//! replicated outcallと変換関数を使い、単一ノードの改変を資金・リスク判断へ取り込まない。
//!
//! 送信先URLは現状testnet固定である。Phase 2の環境設定一般化でnetwork設定から
//! 解決する（`docs/phase-2/README.md`、`docs/phase-0/environments.md`のE-1/E-2）。

use api_types::error::ErrorCode;
use api_types::operations::BudgetClass;
use ic_cdk_management_canister::{
    HttpHeader, HttpMethod, HttpRequest, HttpRequestResult, transform_context_from_query,
};

/// `/exchange`の応答本文の上限（生の本文+ヘッダを見込む）。
const MAX_EXCHANGE_RESPONSE_BYTES: u64 = 8 * 1024;
/// `userFills`の応答上限。
const MAX_FILLS_RESPONSE_BYTES: u64 = 32 * 1024;
/// `clearinghouseState`の応答上限。
const MAX_STATE_RESPONSE_BYTES: u64 = 16 * 1024;
/// `orderStatus`の応答上限。
const MAX_STATUS_RESPONSE_BYTES: u64 = 8 * 1024;

/// `/exchange`の応答の分類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeOutcome {
    /// 受理された。即時約定では`filled`、restingでは`oid`が入る。
    Accepted { oid: Option<u64>, filled: bool },
    /// outer/innerいずれかで明示的に拒否された。
    Rejected { message: String },
}

/// `/exchange`へ署名済みactionを送る（非replicated）。受理時は取引所のoidを返す。
pub async fn post_exchange(
    body: &[u8],
    permit: &crate::rest_budget::Permit,
) -> Result<ExchangeOutcome, ErrorCode> {
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let request: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ErrorCode::UpstreamRejected {
            code: "invalid exchange request".to_string(),
            retryable: false,
        })?;
    let action_type = request
        .get("action")
        .and_then(|action| action.get("type"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| ErrorCode::UpstreamRejected {
            code: "exchange request without action type".to_string(),
            retryable: false,
        })?;
    let exchange_url = crate::environment::resolved()?.exchange_url;
    let response = HttpRequest::new(&exchange_url)
        .with_method(HttpMethod::POST)
        .with_headers(vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }])
        .with_body(body.to_vec())
        .with_max_response_bytes(MAX_EXCHANGE_RESPONSE_BYTES)
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
    classify_exchange_response(action_type, &value).ok_or_else(|| ErrorCode::UpstreamRejected {
        code: "unexpected exchange response".to_string(),
        retryable: false,
    })
}

fn classify_exchange_response(
    action_type: &str,
    value: &serde_json::Value,
) -> Option<ExchangeOutcome> {
    match value.get("status")?.as_str()? {
        "err" => Some(ExchangeOutcome::Rejected {
            message: value
                .get("response")
                .and_then(|response| response.as_str())
                .unwrap_or("exchange rejected the action")
                .to_string(),
        }),
        "ok" => classify_ok_response(action_type, value),
        _ => None,
    }
}

fn classify_ok_response(action_type: &str, value: &serde_json::Value) -> Option<ExchangeOutcome> {
    if action_type == "updateLeverage" {
        return (value.get("response")?.get("type")?.as_str()? == "default").then_some(
            ExchangeOutcome::Accepted {
                oid: None,
                filled: false,
            },
        );
    }
    let status = value
        .get("response")
        .and_then(|response| response.get("data"))
        .and_then(|data| data.get("statuses"))
        .and_then(|statuses| statuses.get(0))?;
    if let Some(message) = status.get("error").and_then(|error| error.as_str()) {
        return Some(ExchangeOutcome::Rejected {
            message: message.to_string(),
        });
    }
    if matches!(action_type, "cancel" | "cancelByCloid") {
        return status
            .get("success")
            .is_some()
            .then_some(ExchangeOutcome::Accepted {
                oid: None,
                filled: false,
            });
    }
    if action_type != "order" {
        return None;
    }
    if let Some(resting) = status.get("resting") {
        return Some(ExchangeOutcome::Accepted {
            oid: resting.get("oid").and_then(|oid| oid.as_u64()),
            filled: false,
        });
    }
    status
        .get("filled")
        .map(|filled| ExchangeOutcome::Accepted {
            oid: filled.get("oid").and_then(|oid| oid.as_u64()),
            filled: true,
        })
}

/// 本人の約定を取得する（replicated＋変換）。
pub async fn user_fills(user: &str) -> Result<String, ErrorCode> {
    fetch_info(
        serde_json::json!({ "type": "userFills", "user": user }),
        MAX_FILLS_RESPONSE_BYTES,
    )
    .await
}

/// 本人の建玉（clearinghouseState）を取得する（replicated＋変換）。
pub async fn clearinghouse_state(user: &str) -> Result<String, ErrorCode> {
    fetch_info(
        serde_json::json!({ "type": "clearinghouseState", "user": user }),
        MAX_STATE_RESPONSE_BYTES,
    )
    .await
}

/// 回収直前にcore未登録の注文も含めて確認する。
pub async fn open_orders(user: &str) -> Result<String, ErrorCode> {
    fetch_info(
        serde_json::json!({ "type": "openOrders", "user": user }),
        MAX_STATE_RESPONSE_BYTES,
    )
    .await
}

/// 注文の状態（orderStatus）を取得する（replicated＋変換）。
pub async fn order_status(user: &str, oid: impl serde::Serialize) -> Result<String, ErrorCode> {
    fetch_info(
        serde_json::json!({ "type": "orderStatus", "user": user, "oid": oid }),
        MAX_STATUS_RESPONSE_BYTES,
    )
    .await
}

pub async fn meta_and_asset_ctxs() -> Result<String, ErrorCode> {
    fetch_info(
        serde_json::json!({ "type": "metaAndAssetCtxs" }),
        128 * 1024,
    )
    .await
}

pub async fn l2_book(coin: &str) -> Result<String, ErrorCode> {
    fetch_info(
        serde_json::json!({ "type": "l2Book", "coin": coin }),
        32 * 1024,
    )
    .await
}

/// `/info`へPOSTし、変換後の本文を返す。
async fn fetch_info(
    query: serde_json::Value,
    max_response_bytes: u64,
) -> Result<String, ErrorCode> {
    // userFills can return 2000 rows: 20 base + at most 100 row units.
    // There is no refund when fewer rows are returned.
    let weight = match query.get("type").and_then(|value| value.as_str()) {
        Some("clearinghouseState" | "orderStatus") => 2,
        Some("userFills") => 120,
        Some("openOrders") => 20,
        Some("metaAndAssetCtxs") => 20,
        Some("l2Book") => 2,
        _ => return Err(ErrorCode::PolicyUnavailable),
    };
    let transform = match query.get("type").and_then(|value| value.as_str()) {
        Some("openOrders") => "transform_open_orders",
        Some("metaAndAssetCtxs" | "l2Book") => "transform_market_info",
        _ => "transform_info",
    };
    let permit = crate::rest_budget::acquire(BudgetClass::Reconcile, weight).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let info_url = crate::environment::resolved()?.info_url;
    let response = HttpRequest::new(&info_url)
        .with_method(HttpMethod::POST)
        .with_headers(vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }])
        .with_body(query.to_string().into_bytes())
        .with_max_response_bytes(max_response_bytes)
        .with_transform(transform_context_from_query(
            transform.to_string(),
            Vec::new(),
        ))
        .send()
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: error.to_string(),
        })?;
    if response.status.to_string() != "200" {
        return Err(ErrorCode::UpstreamUnavailable {
            venue: format!("info HTTP {}", response.status),
        });
    }
    String::from_utf8(response.body).map_err(|_| ErrorCode::UpstreamRejected {
        code: "non-utf8 info response".to_string(),
        retryable: false,
    })
}

#[ic_cdk::query]
fn transform_market_info(
    args: ic_cdk_management_canister::TransformArgs,
) -> ic_cdk_management_canister::HttpRequestResult {
    let body = serde_json::from_slice::<serde_json::Value>(&args.response.body)
        .map(|value| value.to_string().into_bytes())
        .unwrap_or_default();
    HttpRequestResult {
        status: args.response.status,
        headers: Vec::new(),
        body,
    }
}

#[ic_cdk::query]
fn transform_open_orders(
    args: ic_cdk_management_canister::TransformArgs,
) -> ic_cdk_management_canister::HttpRequestResult {
    let body = serde_json::from_slice::<serde_json::Value>(&args.response.body)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .map(|orders| {
            serde_json::Value::Array(
                orders.iter().map(|order| {
                    serde_json::json!({"oid": order.get("oid").cloned().unwrap_or(serde_json::Value::Null)})
                }).collect()
            ).to_string().into_bytes()
        })
        .unwrap_or_default();
    HttpRequestResult {
        status: args.response.status,
        headers: Vec::new(),
        body,
    }
}

/// 変換関数：`/info`の応答から照合に使う要素だけを決定論的に残す。
///
/// 全ノードで同じ本文にするため、付随フィールドと並びの揺れを落とす。解釈できない
/// 応答は空本文にし、呼び出し側は「未観測」として扱う（誤って建玉0にしない）。
#[ic_cdk::query]
fn transform_info(
    args: ic_cdk_management_canister::TransformArgs,
) -> ic_cdk_management_canister::HttpRequestResult {
    HttpRequestResult {
        status: args.response.status,
        headers: Vec::new(),
        body: canonical_info(&args.response.body),
    }
}

/// `/info`の応答を用途別に正規化する。
fn canonical_info(raw: &[u8]) -> Vec<u8> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let canonical = match &value {
        serde_json::Value::Array(entries) => {
            serde_json::Value::Array(entries.iter().map(canonical_fill).collect())
        }
        serde_json::Value::Object(fields) if fields.contains_key("assetPositions") => {
            canonical_state(fields)
        }
        serde_json::Value::Object(fields)
            if fields.contains_key("order") || fields.contains_key("status") =>
        {
            canonical_order_status(fields)
        }
        _ => return Vec::new(),
    };
    canonical.to_string().into_bytes()
}

/// 約定1件から使う要素だけを残す。
fn canonical_fill(entry: &serde_json::Value) -> serde_json::Value {
    let field = |name: &str| entry.get(name).cloned().unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "tid": field("tid"),
        "oid": field("oid"),
        "coin": field("coin"),
        "px": field("px"),
        "sz": field("sz"),
        "fee": field("fee"),
        "time": field("time"),
    })
}

/// `clearinghouseState`から建玉の要素だけを残す。
fn canonical_state(fields: &serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    if !fields
        .get("assetPositions")
        .is_some_and(serde_json::Value::is_array)
    {
        return serde_json::Value::Null;
    }
    let positions: Vec<serde_json::Value> = fields
        .get("assetPositions")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let position = entry.get("position").cloned().unwrap_or(serde_json::Value::Null);
                    let field = |name: &str| {
                        position.get(name).cloned().unwrap_or(serde_json::Value::Null)
                    };
                    serde_json::json!({
                        "position": {
                            "coin": field("coin"),
                            "szi": field("szi"),
                            "entryPx": field("entryPx"),
                            "liquidationPx": field("liquidationPx"),
                            "unrealizedPnl": field("unrealizedPnl"),
                            "leverage": { "value": position.get("leverage").and_then(|l| l.get("value")).cloned().unwrap_or(serde_json::Value::Null) },
                            "marginMode": field("marginMode"),
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let margin_summary = fields
        .get("marginSummary")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    serde_json::json!({
        "assetPositions": positions,
        "marginSummary": {
            "totalMarginUsed": margin_summary.get("totalMarginUsed").cloned().unwrap_or(serde_json::Value::Null),
            "totalNtlPos": margin_summary.get("totalNtlPos").cloned().unwrap_or(serde_json::Value::Null),
        }
    })
}

/// `orderStatus`から状態とoidだけを残す。
fn canonical_order_status(
    fields: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    // Real HL wraps the order and its state in {status:"order", order:{order:...,status:...}}.
    let wrapper = fields.get("order");
    let nested = wrapper.and_then(|v| v.get("order"));
    let order = nested.or(wrapper);
    let status = if nested.is_some() {
        wrapper.and_then(|v| v.get("status"))
    } else {
        fields.get("status")
    };
    serde_json::json!({ "status": status,
        "order": {"oid": order.and_then(|v| v.get("oid")), "cloid": order.and_then(|v| v.get("cloid"))}
    })
}

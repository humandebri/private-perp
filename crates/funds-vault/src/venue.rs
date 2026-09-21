//! Hyperliquid REST（`/exchange`）への払出し送信。
//!
//! `docs/phase-0/api-contract.md` 6節・`Implementation.md` 5.4に従い、状態変更POSTは
//! **非replicated** outcallで送る。`max_response_bytes`は生のヘッダ+本文から見積もる
//! （変換後本文だけで見積もらない）。応答は照合の手掛かりとして保存し、成功応答の
//! 解釈に失敗した場合は「未実行」と扱わない（呼び出し側が`unknown`へ進める）。
//!
//! outboxへ接続するまでの間は未使用の警告を抑止する。
#![allow(dead_code)]

use crate::crypto;
use api_types::error::ErrorCode;
use hl_sign::user_signed;
use hl_types::msgpack::Value;
use ic_cdk_management_canister::{HttpHeader, HttpMethod, HttpRequest};

/// `/exchange`の応答本文の上限（生の本文+ヘッダを見込む）。
const MAX_EXCHANGE_RESPONSE_BYTES: u64 = 8 * 1024;

/// EIP-712へ入れるchain値（起動時の環境設定から解決する）。
struct ChainValues {
    chain_name: String,
    signature_chain_id: String,
    user_signed_chain_id: u64,
}

fn chain_values() -> Result<ChainValues, ErrorCode> {
    let network: hl_types::Network = crate::environment::resolved()?.network.into();
    let map =
        |error: hl_types::environment::EnvironmentError| crate::environment::map_environment(error);
    Ok(ChainValues {
        chain_name: hl_types::environment::chain_name(network)
            .map_err(map)?
            .to_string(),
        signature_chain_id: hl_types::environment::signature_chain_id(network)
            .map_err(map)?
            .to_string(),
        user_signed_chain_id: hl_types::environment::user_signed_chain_id(network).map_err(map)?,
    })
}

/// 払出し（`usdSend`）のaction本体（msgpack用の値とJSON）。
pub struct UsdSend {
    pub destination: String,
    pub amount_micros: u64,
    pub time: u64,
}

impl UsdSend {
    /// EIP-712の署名対象ダイジェスト（master鍵で署名する）。
    pub fn digest(&self) -> Result<[u8; 32], ErrorCode> {
        let chain = chain_values()?;
        let values = vec![
            user_signed::TypedValue::String(chain.chain_name.clone()),
            user_signed::TypedValue::String(self.destination.clone()),
            user_signed::TypedValue::String(amount_text(self.amount_micros)),
            user_signed::TypedValue::Uint64(self.time),
        ];
        user_signed::digest(
            chain.user_signed_chain_id,
            user_signed::USD_SEND_PRIMARY_TYPE,
            user_signed::USD_SEND_FIELDS,
            &values,
        )
        .map_err(|error| ErrorCode::Internal {
            code: format!("usdSend digest: {error}"),
        })
    }

    /// 送信するJSON本文（action・signature・nonce）。
    pub fn body(&self, signature: &hl_sign::Signature) -> Result<Vec<u8>, ErrorCode> {
        let chain = chain_values()?;
        let body = serde_json::json!({
            "action": {
                "type": "usdSend",
                "signatureChainId": chain.signature_chain_id,
                "hyperliquidChain": chain.chain_name,
                "destination": self.destination,
                "amount": amount_text(self.amount_micros),
                "time": self.time,
            },
            "nonce": self.time,
            "signature": {
                "r": format!("0x{}", hex::encode(signature.r)),
                "s": format!("0x{}", hex::encode(signature.s)),
                "v": signature.v,
            },
        });
        serde_json::to_vec(&body).map_err(|error| ErrorCode::Internal {
            code: format!("usdSend body: {error}"),
        })
    }
}

/// Agent承認（`approveAgent`）のaction。
pub struct ApproveAgent {
    /// Agentのアドレス（EIP-712では`address`型、JSONでは0x文字列）。
    pub agent: [u8; 20],
    pub name: String,
    pub time: u64,
}

impl ApproveAgent {
    /// EIP-712の署名対象ダイジェスト（master鍵＝口座所有者で署名する）。
    pub fn digest(&self) -> Result<[u8; 32], ErrorCode> {
        let chain = chain_values()?;
        let values = vec![
            user_signed::TypedValue::String(chain.chain_name.clone()),
            user_signed::TypedValue::Address(self.agent),
            user_signed::TypedValue::String(self.name.clone()),
            user_signed::TypedValue::Uint64(self.time),
        ];
        user_signed::digest(
            chain.user_signed_chain_id,
            user_signed::APPROVE_AGENT_PRIMARY_TYPE,
            user_signed::APPROVE_AGENT_FIELDS,
            &values,
        )
        .map_err(|error| ErrorCode::Internal {
            code: format!("approveAgent digest: {error}"),
        })
    }

    /// 送信するJSON本文（action・signature・nonce）。
    pub fn body(&self, signature: &hl_sign::Signature) -> Result<Vec<u8>, ErrorCode> {
        let chain = chain_values()?;
        let body = serde_json::json!({
            "action": {
                "type": "approveAgent",
                "signatureChainId": chain.signature_chain_id,
                "hyperliquidChain": chain.chain_name,
                "agentAddress": format!("0x{}", hex::encode(self.agent)),
                "agentName": self.name,
                "nonce": self.time,
            },
            "nonce": self.time,
            "signature": {
                "r": format!("0x{}", hex::encode(signature.r)),
                "s": format!("0x{}", hex::encode(signature.s)),
                "v": signature.v,
            },
        });
        serde_json::to_vec(&body).map_err(|error| ErrorCode::Internal {
            code: format!("approveAgent body: {error}"),
        })
    }
}

/// `approveAgent`を送信する（非replicated POST）。
pub async fn post_approve_agent(
    payload: &ApproveAgent,
    signature: &hl_sign::Signature,
) -> Result<(ExchangeOutcome, Vec<u8>), ErrorCode> {
    let body = payload.body(signature)?;
    let exchange_url = crate::environment::resolved()?.exchange_url;
    let response = HttpRequest::new(&exchange_url)
        .with_method(HttpMethod::POST)
        .with_headers(vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }])
        .with_body(body)
        .with_max_response_bytes(MAX_EXCHANGE_RESPONSE_BYTES)
        .non_replicated()
        .send()
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: error.to_string(),
        })?;

    match parse_exchange_response(&response.body) {
        Some(outcome) => Ok((outcome, response.body)),
        None => Err(ErrorCode::UpstreamRejected {
            code: "unparseable exchange response".to_string(),
            retryable: false,
        }),
    }
}

/// マイクロUSDCをHLへ渡す十進文字列にする（指数表記を使わない）。
pub fn amount_text(micros: u64) -> String {
    hl_types::UsdcMicros::from_micros(micros).to_decimal_string()
}

/// 応答の分類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeOutcome {
    /// 受理された（`status: ok`）。
    Accepted,
    /// 取引所が拒否した（`status: err`）。本文は照合の手掛かりとして保持する。
    Rejected { message: String },
}

/// `/exchange`の応答を解釈する。解釈できない場合は`None`（=結果不明として扱う）。
pub fn parse_exchange_response(body: &[u8]) -> Option<ExchangeOutcome> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    match value.get("status")?.as_str()? {
        "ok" => Some(ExchangeOutcome::Accepted),
        "err" => Some(ExchangeOutcome::Rejected {
            message: value
                .get("response")
                .and_then(|response| response.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        }),
        _ => None,
    }
}

/// `usdSend`を送信する（非replicated POST）。
pub async fn post_usd_send(
    payload: &UsdSend,
    signature: &hl_sign::Signature,
) -> Result<(ExchangeOutcome, Vec<u8>), ErrorCode> {
    let body = payload.body(signature)?;
    let exchange_url = crate::environment::resolved()?.exchange_url;
    let response = HttpRequest::new(&exchange_url)
        .with_method(HttpMethod::POST)
        .with_headers(vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }])
        .with_body(body)
        .with_max_response_bytes(MAX_EXCHANGE_RESPONSE_BYTES)
        .non_replicated()
        .send()
        .await
        .map_err(|error| ErrorCode::UpstreamUnavailable {
            venue: error.to_string(),
        })?;

    let outcome = parse_exchange_response(&response.body);
    match outcome {
        Some(outcome) => Ok((outcome, response.body)),
        None => Err(ErrorCode::UpstreamRejected {
            code: "unparseable exchange response".to_string(),
            retryable: false,
        }),
    }
}

/// 署名器（master鍵）を呼び出して`usdSend`へ署名する。
pub async fn sign_usd_send(
    payload: &UsdSend,
    derivation_path: Vec<Vec<u8>>,
    expected_public_key: &[u8; 33],
) -> Result<hl_sign::Signature, ErrorCode> {
    let digest = payload.digest()?;
    crypto::sign_with_key(&digest, derivation_path, expected_public_key).await
}

/// msgpack用の値（テスト・照合補助）。
pub fn usd_send_values(payload: &UsdSend) -> Value {
    Value::map(vec![
        ("type", Value::str("usdSend")),
        ("destination", Value::owned(payload.destination.clone())),
        ("amount", Value::owned(amount_text(payload.amount_micros))),
        ("time", Value::UInt(payload.time)),
    ])
}

#[cfg(test)]
mod tests {
    use super::{ExchangeOutcome, amount_text, parse_exchange_response};

    #[test]
    fn amounts_are_decimal_strings_without_exponents() {
        assert_eq!(amount_text(1_000_000), "1");
        assert_eq!(amount_text(1_500_000), "1.5");
        assert_eq!(amount_text(1), "0.000001");
    }

    #[test]
    fn exchange_responses_are_classified() {
        assert_eq!(
            parse_exchange_response(br#"{"status":"ok","response":{"type":"default"}}"#),
            Some(ExchangeOutcome::Accepted)
        );
        assert_eq!(
            parse_exchange_response(br#"{"status":"err","response":"insufficient margin"}"#),
            Some(ExchangeOutcome::Rejected {
                message: "insufficient margin".to_string()
            })
        );
        // 解釈できない応答は「未実行」と扱わない（呼び出し側がunknownへ進める）。
        assert_eq!(parse_exchange_response(b"not json"), None);
        assert_eq!(parse_exchange_response(br#"{"unexpected":true}"#), None);
    }
}

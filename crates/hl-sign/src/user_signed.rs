//! user-signed EIP-712（`approveAgent`・`usdSend` などの資金・アカウント操作）。
//!
//! 取引action（order/cancel等）はphantom agent方式だが、資金・アカウント操作は
//! `HyperliquidSignTransaction` domainの通常のEIP-712で署名する。
//! 公式SDKの `signUserSignedAction` と同じ型・domainを使う。
//!
//! - domain: `name = "HyperliquidSignTransaction"`, `version = "1"`,
//!   `chainId = action.signatureChainId`, `verifyingContract = 0x0`
//! - primaryType: 型定義のキー（例 `HyperliquidTransaction:ApproveAgent`）

use crate::eip712::{Domain, domain_separator};
use crate::error::SignError;
use crate::keccak::{keccak256, keccak256_concat};
use crate::signature::{Signature, sign_digest};

/// EIP-712のフィールド型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKind {
    String,
    Address,
    Uint64,
}

impl TypedKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Address => "address",
            Self::Uint64 => "uint64",
        }
    }
}

/// EIP-712のフィールド定義。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedField {
    pub name: &'static str,
    pub kind: TypedKind,
}

/// フィールドの値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedValue {
    String(String),
    Address([u8; 20]),
    Uint64(u64),
}

const fn field(name: &'static str, kind: TypedKind) -> TypedField {
    TypedField { name, kind }
}

/// `approveAgent` の型定義（SDKの `ApproveAgentTypes` と同一）。
pub const APPROVE_AGENT_PRIMARY_TYPE: &str = "HyperliquidTransaction:ApproveAgent";
pub const APPROVE_AGENT_FIELDS: &[TypedField] = &[
    field("hyperliquidChain", TypedKind::String),
    field("agentAddress", TypedKind::Address),
    field("agentName", TypedKind::String),
    field("nonce", TypedKind::Uint64),
];

/// `usdSend` の型定義（SDKの `UsdSendTypes` と同一）。
pub const USD_SEND_PRIMARY_TYPE: &str = "HyperliquidTransaction:UsdSend";
pub const USD_SEND_FIELDS: &[TypedField] = &[
    field("hyperliquidChain", TypedKind::String),
    field("destination", TypedKind::String),
    field("amount", TypedKind::String),
    field("time", TypedKind::Uint64),
];

/// `HyperliquidSignTransaction` domain。
pub const fn transaction_domain(chain_id: u64) -> Domain {
    Domain {
        name: "HyperliquidSignTransaction",
        version: "1",
        chain_id,
        verifying_contract: [0u8; 20],
    }
}

/// EIP-712の型文字列（`Primary(type1 name1,type2 name2)`）。
pub fn type_string(primary_type: &str, fields: &[TypedField]) -> String {
    let inner = fields
        .iter()
        .map(|field| format!("{} {}", field.kind.as_str(), field.name))
        .collect::<Vec<_>>()
        .join(",");
    format!("{primary_type}({inner})")
}

/// 型ハッシュ。
pub fn type_hash(primary_type: &str, fields: &[TypedField]) -> [u8; 32] {
    keccak256(type_string(primary_type, fields).as_bytes())
}

/// 構造体ハッシュ。フィールド数と値の数が一致しない場合は拒否する。
pub fn struct_hash(
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
) -> Result<[u8; 32], SignError> {
    if fields.len() != values.len() {
        return Err(SignError::TypedDataMismatch);
    }
    let mut parts: Vec<[u8; 32]> = Vec::with_capacity(fields.len() + 1);
    parts.push(type_hash(primary_type, fields));
    for (field, value) in fields.iter().zip(values) {
        let encoded = match (field.kind, value) {
            (TypedKind::String, TypedValue::String(text)) => keccak256(text.as_bytes()),
            (TypedKind::Address, TypedValue::Address(address)) => {
                crate::eip712::address_word(*address)
            }
            (TypedKind::Uint64, TypedValue::Uint64(value)) => crate::eip712::u64_word(*value),
            _ => return Err(SignError::TypedDataMismatch),
        };
        parts.push(encoded);
    }
    let refs: Vec<&[u8]> = parts.iter().map(|part| part.as_slice()).collect();
    Ok(keccak256_concat(&refs))
}

/// 署名対象ダイジェスト。
pub fn digest(
    chain_id: u64,
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
) -> Result<[u8; 32], SignError> {
    let domain = transaction_domain(chain_id);
    let struct_hash = struct_hash(primary_type, fields, values)?;
    let mut input = Vec::with_capacity(66);
    input.extend_from_slice(&[0x19, 0x01]);
    input.extend_from_slice(&domain_separator(&domain));
    input.extend_from_slice(&struct_hash);
    Ok(keccak256(&input))
}

/// user-signed actionへ署名する。
pub fn sign(
    chain_id: u64,
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
    secret_key: &[u8; 32],
) -> Result<Signature, SignError> {
    let digest = digest(chain_id, primary_type, fields, values)?;
    sign_digest(&digest, secret_key)
}

#[cfg(test)]
mod tests {
    use super::{
        APPROVE_AGENT_FIELDS, APPROVE_AGENT_PRIMARY_TYPE, TypedKind, TypedValue, USD_SEND_FIELDS,
        USD_SEND_PRIMARY_TYPE, digest, sign, struct_hash, transaction_domain, type_string,
    };
    use crate::eip712::domain_separator;
    use crate::signature::{address_from_secret, recover_address};

    fn hex(bytes: [u8; 32]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn secret(seed: u8) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[31] = seed;
        bytes
    }

    #[test]
    fn type_strings_match_the_sdk_definitions() {
        assert_eq!(
            type_string(APPROVE_AGENT_PRIMARY_TYPE, APPROVE_AGENT_FIELDS),
            "HyperliquidTransaction:ApproveAgent(string hyperliquidChain,address agentAddress,string agentName,uint64 nonce)"
        );
        assert_eq!(
            type_string(USD_SEND_PRIMARY_TYPE, USD_SEND_FIELDS),
            "HyperliquidTransaction:UsdSend(string hyperliquidChain,string destination,string amount,uint64 time)"
        );
    }

    #[test]
    fn transaction_domain_is_not_the_exchange_domain() {
        let transaction = transaction_domain(421_614);
        assert_eq!(transaction.name, "HyperliquidSignTransaction");
        assert_ne!(
            domain_separator(&transaction),
            domain_separator(&crate::Domain::exchange())
        );
    }

    #[test]
    fn field_count_must_match_value_count() {
        assert_eq!(
            struct_hash(
                APPROVE_AGENT_PRIMARY_TYPE,
                APPROVE_AGENT_FIELDS,
                &[TypedValue::String("Testnet".to_string())],
            ),
            Err(crate::SignError::TypedDataMismatch)
        );
    }

    #[test]
    fn value_kind_must_match_field_kind() {
        let values = vec![
            TypedValue::Uint64(1),
            TypedValue::Address([0u8; 20]),
            TypedValue::String(String::new()),
            TypedValue::Uint64(1),
        ];
        assert_eq!(
            struct_hash(APPROVE_AGENT_PRIMARY_TYPE, APPROVE_AGENT_FIELDS, &values),
            Err(crate::SignError::TypedDataMismatch)
        );
        assert_eq!(APPROVE_AGENT_FIELDS[0].kind, TypedKind::String);
    }

    #[test]
    fn signing_recovers_to_the_signer() {
        let secret_key = secret(31);
        let values = vec![
            TypedValue::String("Testnet".to_string()),
            TypedValue::Address([3u8; 20]),
            TypedValue::String("agent".to_string()),
            TypedValue::Uint64(1_758_000_000_000),
        ];
        let digest = digest(
            421_614,
            APPROVE_AGENT_PRIMARY_TYPE,
            APPROVE_AGENT_FIELDS,
            &values,
        )
        .expect("digest");
        let signature = sign(
            421_614,
            APPROVE_AGENT_PRIMARY_TYPE,
            APPROVE_AGENT_FIELDS,
            &values,
            &secret_key,
        )
        .expect("sign");
        assert_eq!(
            recover_address(&digest, &signature, None).expect("recover"),
            address_from_secret(&secret_key).expect("address")
        );
        assert_eq!(hex(digest).len(), 64);
    }

    #[test]
    fn chain_id_changes_the_digest() {
        let values = vec![
            TypedValue::String("Testnet".to_string()),
            TypedValue::Address([3u8; 20]),
            TypedValue::String("agent".to_string()),
            TypedValue::Uint64(1),
        ];
        assert_ne!(
            digest(
                421_614,
                APPROVE_AGENT_PRIMARY_TYPE,
                APPROVE_AGENT_FIELDS,
                &values
            )
            .expect("digest"),
            digest(
                42_161,
                APPROVE_AGENT_PRIMARY_TYPE,
                APPROVE_AGENT_FIELDS,
                &values
            )
            .expect("digest")
        );
    }
}

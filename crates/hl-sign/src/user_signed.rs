//! Hyperliquidのuser-signed EIP-712（資金・アカウント操作）。
//!
//! 取引action（order/cancel等）はphantom agent方式だが、`approveAgent`・`usdSend`
//! などは `HyperliquidSignTransaction` domainの通常のEIP-712で署名する。
//! 型定義は公式SDKの `ApproveAgentTypes`／`UsdSendTypes` と同一で、
//! `crates/hl-sign/tests/fixtures/` のテストベクトルで一致を確認している。
//!
//! 符号化そのものは `crate::typed_data` の汎用実装を使う。

pub use crate::typed_data::{
    TypedField, TypedKind, TypedValue, struct_hash, type_hash, type_string,
};

use crate::eip712::Domain;
use crate::error::SignError;
use crate::signature::Signature;
use crate::typed_data::{digest_with_domain, sign_with_domain};

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

/// 署名対象ダイジェスト。
pub fn digest(
    chain_id: u64,
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
) -> Result<[u8; 32], SignError> {
    digest_with_domain(&transaction_domain(chain_id), primary_type, fields, values)
}

/// user-signed actionへ署名する（**テスト専用**。本番は管理CanisterのtECDSAを使う）。
pub fn sign(
    chain_id: u64,
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
    secret_key: &[u8; 32],
) -> Result<Signature, SignError> {
    sign_with_domain(
        &transaction_domain(chain_id),
        primary_type,
        fields,
        values,
        secret_key,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        APPROVE_AGENT_FIELDS, APPROVE_AGENT_PRIMARY_TYPE, TypedKind, TypedValue, USD_SEND_FIELDS,
        USD_SEND_PRIMARY_TYPE, digest, sign, struct_hash, transaction_domain, type_string,
    };
    use crate::eip712::domain_separator;
    use crate::signature::{address_from_secret, recover_address};

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

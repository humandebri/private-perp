//! 汎用のEIP-712符号化（型定義・構造体ハッシュ・ダイジェスト）。
//!
//! Hyperliquidのuser-signed方式と、本サービス自身の型（`crate::private_perp`）の
//! 双方がこの実装を使う。符号化は公式SDKのfixtureで検証済みの規則に従う。
//!
//! - 型文字列: `Primary(type1 name1,type2 name2)`（`,` の後に空白を入れない）
//! - `string` は値のKeccak-256、`address` は左詰めゼロ埋め32バイト、`uint64` は32バイトBE
//! - 構造体ハッシュ: `keccak256(typeHash || enc(field1) || …)`
//! - ダイジェスト: `keccak256(0x1901 || domainSeparator || structHash)`

use crate::eip712::{Domain, address_word, domain_separator, u64_word};
use crate::error::SignError;
use crate::keccak::{keccak256, keccak256_concat};
use crate::signature::{Signature, sign_digest_for_tests};

/// EIP-712のフィールド型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKind {
    String,
    Address,
    Uint64,
    /// 可変長バイト列（ICのPrincipalなど）。
    Bytes,
    /// 32バイト固定値（user_id・nonceなど）。
    Bytes32,
}

impl TypedKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Address => "address",
            Self::Uint64 => "uint64",
            Self::Bytes => "bytes",
            Self::Bytes32 => "bytes32",
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
    Bytes(Vec<u8>),
    Bytes32([u8; 32]),
}

/// EIP-712の型文字列。
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

/// 構造体ハッシュ。フィールド数・型と値が一致しない場合は拒否する。
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
            (TypedKind::Address, TypedValue::Address(address)) => address_word(*address),
            (TypedKind::Uint64, TypedValue::Uint64(value)) => u64_word(*value),
            (TypedKind::Bytes, TypedValue::Bytes(bytes)) => keccak256(bytes),
            (TypedKind::Bytes32, TypedValue::Bytes32(bytes)) => *bytes,
            _ => return Err(SignError::TypedDataMismatch),
        };
        parts.push(encoded);
    }
    let refs: Vec<&[u8]> = parts.iter().map(|part| part.as_slice()).collect();
    Ok(keccak256_concat(&refs))
}

/// 任意のdomainに対する署名対象ダイジェスト。
pub fn digest_with_domain(
    domain: &Domain,
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
) -> Result<[u8; 32], SignError> {
    let struct_hash = struct_hash(primary_type, fields, values)?;
    let mut input = Vec::with_capacity(66);
    input.extend_from_slice(&[0x19, 0x01]);
    input.extend_from_slice(&domain_separator(domain));
    input.extend_from_slice(&struct_hash);
    Ok(keccak256(&input))
}

/// 任意のdomainに対する署名（**テスト専用**。本番は管理CanisterのtECDSAを使う）。
pub fn sign_with_domain(
    domain: &Domain,
    primary_type: &str,
    fields: &[TypedField],
    values: &[TypedValue],
    secret_key: &[u8; 32],
) -> Result<Signature, SignError> {
    let digest = digest_with_domain(domain, primary_type, fields, values)?;
    sign_digest_for_tests(&digest, secret_key)
}

#[cfg(test)]
mod tests {
    use super::{TypedField, TypedKind, TypedValue, digest_with_domain, struct_hash, type_string};
    use crate::eip712::Domain;

    const FIELDS: &[TypedField] = &[
        TypedField {
            name: "owner",
            kind: TypedKind::Address,
        },
        TypedField {
            name: "amount",
            kind: TypedKind::Uint64,
        },
        TypedField {
            name: "memo",
            kind: TypedKind::String,
        },
        TypedField {
            name: "nonce",
            kind: TypedKind::Bytes32,
        },
    ];

    fn values() -> Vec<TypedValue> {
        vec![
            TypedValue::Address([1u8; 20]),
            TypedValue::Uint64(7),
            TypedValue::String("memo".to_string()),
            TypedValue::Bytes32([2u8; 32]),
        ]
    }

    fn domain() -> Domain {
        Domain {
            name: "test",
            version: "1",
            chain_id: 1,
            verifying_contract: [0u8; 20],
        }
    }

    #[test]
    fn type_string_has_no_space_after_commas() {
        assert_eq!(
            type_string("Test", FIELDS),
            "Test(address owner,uint64 amount,string memo,bytes32 nonce)"
        );
    }

    #[test]
    fn kind_and_value_must_match() {
        let mismatched = vec![
            TypedValue::Uint64(1),
            TypedValue::Uint64(7),
            TypedValue::String("memo".to_string()),
            TypedValue::Bytes32([2u8; 32]),
        ];
        assert_eq!(
            struct_hash("Test", FIELDS, &mismatched),
            Err(crate::SignError::TypedDataMismatch)
        );
    }

    #[test]
    fn every_field_changes_the_digest() {
        let base = digest_with_domain(&domain(), "Test", FIELDS, &values()).expect("digest");

        let mut changed_address = values();
        changed_address[0] = TypedValue::Address([9u8; 20]);
        assert_ne!(
            base,
            digest_with_domain(&domain(), "Test", FIELDS, &changed_address).expect("digest")
        );

        let mut changed_amount = values();
        changed_amount[1] = TypedValue::Uint64(8);
        assert_ne!(
            base,
            digest_with_domain(&domain(), "Test", FIELDS, &changed_amount).expect("digest")
        );

        let mut changed_memo = values();
        changed_memo[2] = TypedValue::String("other".to_string());
        assert_ne!(
            base,
            digest_with_domain(&domain(), "Test", FIELDS, &changed_memo).expect("digest")
        );

        let mut changed_nonce = values();
        changed_nonce[3] = TypedValue::Bytes32([3u8; 32]);
        assert_ne!(
            base,
            digest_with_domain(&domain(), "Test", FIELDS, &changed_nonce).expect("digest")
        );
    }

    #[test]
    fn domain_changes_the_digest() {
        let other = Domain {
            chain_id: 2,
            ..domain()
        };
        assert_ne!(
            digest_with_domain(&domain(), "Test", FIELDS, &values()).expect("digest"),
            digest_with_domain(&other, "Test", FIELDS, &values()).expect("digest")
        );
    }

    #[test]
    fn bytes_and_string_are_encoded_as_different_types() {
        let bytes_fields = &[TypedField {
            name: "value",
            kind: TypedKind::Bytes,
        }];
        let string_fields = &[TypedField {
            name: "value",
            kind: TypedKind::String,
        }];

        let with_bytes = digest_with_domain(
            &domain(),
            "T",
            bytes_fields,
            &[TypedValue::Bytes(b"abc".to_vec())],
        )
        .expect("digest");
        let with_string = digest_with_domain(
            &domain(),
            "T",
            string_fields,
            &[TypedValue::String("abc".to_string())],
        )
        .expect("digest");
        assert_ne!(with_bytes, with_string);

        // 型が一致しない値は拒否する。
        assert_eq!(
            digest_with_domain(
                &domain(),
                "T",
                bytes_fields,
                &[TypedValue::String("abc".to_string())],
            ),
            Err(crate::SignError::TypedDataMismatch)
        );
    }
}

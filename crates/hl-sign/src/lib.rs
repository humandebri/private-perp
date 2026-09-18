//! Hyperliquidのaction構築・署名対象ハッシュ。
//!
//! `Implementation.md` 3.2 により**非async・純粋関数のみ**とする。`await`・
//! `ic0.call_perform`・inter-canister callをこのクレートへ持ち込まない
//! （`scripts/check-no-await.sh` でCI検査する）。
//!
//! Phase 0 は骨格のみである。actionの構築、msgpackエンコード、EIP-712ハッシュ、
//! `v` の復元は Phase 1（`Implementation.md` 1-1、9.1）で実装し、公式SDKと
//! 同一入力のテストベクトルを固定する。署名実装は公式SDKへ依存しない。
#![forbid(unsafe_code)]

use sha3::{Digest, Keccak256};

/// Keccak-256。EIP-712およびmsgpack由来のハッシュ計算の基礎。
pub fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// secp256k1の秘密鍵から圧縮公開鍵（33バイト）を導出する。
///
/// tECDSAの導出結果の検証と、Agent公開鍵の保存（`Implementation.md` 7章）で使う。
/// 任意のダイジェストへの署名APIはこのクレートへ追加しない
/// （`docs/phase-0/authority-matrix.md` 4節）。
pub fn public_key_compressed(secret_key: &[u8; 32]) -> Result<[u8; 33], SignError> {
    use k256::elliptic_curve::sec1::ToSec1Point;

    let secret =
        k256::SecretKey::from_bytes(secret_key.into()).map_err(|_| SignError::InvalidSecretKey)?;
    let point = secret.public_key().to_sec1_point(true);
    let encoded = point.as_bytes();

    let mut compressed = [0u8; 33];
    compressed.copy_from_slice(encoded);
    Ok(compressed)
}

/// EIP-712 domainの値。chain idとverifying contractは Phase 1 で確定する。
///
/// type hash・domain separatorの計算は Phase 1（1-1）で実装する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Domain<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub chain_id: u64,
    pub verifying_contract: [u8; 20],
}

/// 署名関連のエラー。Phase 1 でaction構築・正規化の失敗を追加する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignError {
    #[error("invalid secp256k1 secret key")]
    InvalidSecretKey,
}

#[cfg(test)]
mod tests {
    use super::{SignError, keccak256, public_key_compressed};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn keccak256_matches_known_vectors() {
        assert_eq!(
            hex(&keccak256(b"")),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
        assert_eq!(
            hex(&keccak256(b"abc")),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
    }

    #[test]
    fn secp256k1_generator_public_key_is_stable() {
        // 秘密鍵 1 は生成点 G に対応する（SEC圧縮形式の既知値）。
        let mut secret = [0u8; 32];
        secret[31] = 1;
        assert_eq!(
            hex(&public_key_compressed(&secret).expect("secret key 1 is valid")),
            "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        );
    }

    #[test]
    fn zero_secret_key_is_rejected() {
        assert_eq!(
            public_key_compressed(&[0u8; 32]),
            Err(SignError::InvalidSecretKey)
        );
    }
}

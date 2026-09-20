//! 閾値ECDSA（管理Canister）による公開鍵導出と署名。
//!
//! 署名は`Implementation.md` 9.1の4に従い、閾値署名の応答に`v`が含まれないため
//! 期待公開鍵と照合して`v`を決める。任意のユーザー入力ダイジェストをそのまま
//! 署名する経路は作らない（呼び出し側が用途と宛先を決める）。
//!
//! 公開鍵は口座ごとに`custody_accounts`へ保存して再利用する（毎回の照会は避ける）。

use crate::config;
use api_types::error::ErrorCode;
use ic_cdk_management_canister::{
    EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs, SignWithEcdsaArgs, ecdsa_public_key,
    sign_with_ecdsa,
};

fn internal(message: &str) -> ErrorCode {
    ErrorCode::Internal {
        code: message.to_string(),
    }
}

fn key_id() -> EcdsaKeyId {
    EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: config::ECDSA_KEY_ID.to_string(),
    }
}

/// derivation pathを組み立てる（`codec`は`vec blob`）。
pub fn derivation_path(parts: &[&[u8]]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.to_vec()).collect()
}

/// テスト用の決定的鍵（`test-venue` featureでのみ有効）。
///
/// PocketIC 16.0.0の既定トポロジには閾値ECDSAの鍵が無く（`existing keys: []`）、
/// ローカルでは実tECDSAを検証できない。outboxの状態機械をローカルで検証するための
/// 代替であり、**本番ビルド（feature無し）では使われない**。
#[cfg(feature = "test-venue")]
fn test_signing_key(path: &[Vec<u8>]) -> [u8; 32] {
    let parts: Vec<&[u8]> = path.iter().map(|part| part.as_slice()).collect();
    hl_sign::keccak256_concat(&parts)
}

#[cfg(feature = "test-venue")]
fn test_public_key(path: &[Vec<u8>]) -> Result<[u8; 33], ErrorCode> {
    hl_sign::public_key_compressed(&test_signing_key(path))
        .map_err(|error| internal(&error.to_string()))
}

/// 圧縮公開鍵（33バイト）を導出する。
pub async fn public_key(path: Vec<Vec<u8>>) -> Result<[u8; 33], ErrorCode> {
    match ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: path.clone(),
        key_id: key_id(),
    })
    .await
    {
        Ok(result) => result
            .public_key
            .try_into()
            .map_err(|_| internal("unexpected public key length")),
        Err(error) => {
            #[cfg(feature = "test-venue")]
            {
                let _ = error;
                test_public_key(&path)
            }
            #[cfg(not(feature = "test-venue"))]
            {
                Err(internal(&error.to_string()))
            }
        }
    }
}

/// ダイジェストへ署名し、`v`を復元した65バイト署名を返す（公開鍵を照会する）。
/// 払出しの署名（署名段階）で使う。
#[allow(dead_code)]
pub async fn sign(digest: &[u8; 32], path: Vec<Vec<u8>>) -> Result<hl_sign::Signature, ErrorCode> {
    let expected = public_key(path.clone()).await?;
    sign_with_key(digest, path, &expected).await
}

/// 既知の公開鍵を使って署名する（`v`の復元に公開鍵照会を挟まない）。
pub async fn sign_with_key(
    digest: &[u8; 32],
    path: Vec<Vec<u8>>,
    expected_public_key: &[u8; 33],
) -> Result<hl_sign::Signature, ErrorCode> {
    let result = match sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: digest.to_vec(),
        derivation_path: path.clone(),
        key_id: key_id(),
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            #[cfg(feature = "test-venue")]
            {
                let _ = error;
                return hl_sign::sign_digest_for_tests(digest, &test_signing_key(&path))
                    .map_err(|error| internal(&error.to_string()));
            }
            #[cfg(not(feature = "test-venue"))]
            {
                return Err(internal(&error.to_string()));
            }
        }
    };

    let bytes: [u8; 64] = result
        .signature
        .try_into()
        .map_err(|_| internal("unexpected signature length"))?;
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&bytes[0..32]);
    s.copy_from_slice(&bytes[32..64]);

    let v = hl_sign::recover_v(digest, r, s, expected_public_key).map_err(|error| {
        ErrorCode::Internal {
            code: format!("cannot recover v: {error}"),
        }
    })?;
    Ok(hl_sign::Signature { r, s, v })
}

#[cfg(test)]
mod tests {
    use super::derivation_path;

    #[test]
    fn derivation_paths_are_byte_vectors() {
        let path = derivation_path(&[b"private-perp", b"vault"]);
        assert_eq!(path, vec![b"private-perp".to_vec(), b"vault".to_vec()]);
    }
}

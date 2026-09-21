//! 閾値ECDSA（管理Canister）による公開鍵導出と署名。
//!
//! 署名は`Implementation.md` 9.1の4に従い、閾値署名の応答に`v`が含まれないため
//! 期待公開鍵と照合して`v`を決める。任意のユーザー入力ダイジェストをそのまま
//! 署名する経路は作らない（呼び出し側が用途と宛先を決める）。
//!
//! 公開鍵は口座ごとに`custody_accounts`へ保存して再利用する（毎回の照会は避ける）。

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

/// 閾値ECDSAのkey ID（起動時の環境設定から解決する）。
fn key_id() -> Result<EcdsaKeyId, ErrorCode> {
    Ok(EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: crate::environment::resolved()?.ecdsa_key_id,
    })
}

/// derivation pathを組み立てる（`codec`は`vec blob`）。
/// 口座の導出（署名段階）とテストで使う。
#[allow(dead_code)]
pub fn derivation_path(parts: &[&[u8]]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.to_vec()).collect()
}

/// 圧縮公開鍵（33バイト）を導出する。
pub async fn public_key(path: Vec<Vec<u8>>) -> Result<[u8; 33], ErrorCode> {
    let result = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: path,
        key_id: key_id()?,
    })
    .await
    .map_err(|error| internal(&error.to_string()))?;

    result
        .public_key
        .try_into()
        .map_err(|_| internal("unexpected public key length"))
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
    let result = sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: digest.to_vec(),
        derivation_path: path,
        key_id: key_id()?,
    })
    .await
    .map_err(|error| internal(&error.to_string()))?;

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
    // 閾値署名は high-s を返し得る。Ethereum系の検証は high-s を拒否するため、
    // 送信する署名は low-s 形へ正規化する（v も反転させる）。
    hl_sign::Signature { r, s, v }
        .normalized_low_s()
        .map_err(|error| internal(&format!("cannot normalize s: {error}")))
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

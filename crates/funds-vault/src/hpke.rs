//! RFC 9180 HPKE（X25519 / HKDF-SHA256 / ChaCha20-Poly1305）の鍵導出。
//!
//! 監査実績のある実装（`hpke` crate）を使い、暗号プリミティブを自作しない
//! （`Plan.md` 16.5）。鍵はcanisterではOS乱数が使えないため `raw_rand` から
//! 入力鍵材料（IKM）として渡す。暗号化秘密鍵は公開queryへ出さない。
#![allow(dead_code)]

use hpke::{Kem, Serializable, kem::X25519HkdfSha256};

/// IKMからX25519鍵対を導出し、公開鍵（32バイト）を返す。
pub fn derive_public_key(ikm: &[u8]) -> Vec<u8> {
    derive_keypair(ikm).1
}

/// IKMからX25519鍵対を導出する（秘密鍵はcanister内に留める）。
pub fn derive_keypair(ikm: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (secret, public) = X25519HkdfSha256::derive_keypair(ikm);
    (secret.to_bytes().to_vec(), public.to_bytes().to_vec())
}

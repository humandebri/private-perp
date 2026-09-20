//! RFC 9180 HPKE（X25519 / HKDF-SHA256 / ChaCha20-Poly1305）の鍵導出。
//!
//! 監査実績のある実装（`hpke` crate）を使い、暗号プリミティブを自作しない
//! （`Plan.md` 16.5）。鍵はcanisterではOS乱数が使えないため `raw_rand` から
//! 入力鍵材料（IKM）として渡す。暗号化秘密鍵は公開queryへ出さない。
#![allow(dead_code)]

use core::convert::Infallible;
use hpke::rand_core::{TryCryptoRng, TryRng};
use hpke::{
    Deserializable, Kem, OpModeR, OpModeS, Serializable, aead::ChaCha20Poly1305, kdf::HkdfSha256,
    kem::X25519HkdfSha256, single_shot_open, single_shot_seal_with_rng,
};

/// `raw_rand`の値を種にした決定的RNG（canisterにはOS乱数が無い）。
struct SeededRng {
    seed: [u8; 32],
    counter: u64,
}

impl TryRng for SeededRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut bytes = [0u8; 4];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut bytes = [0u8; 8];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        let mut offset = 0;
        while offset < dst.len() {
            let mut input = Vec::with_capacity(40);
            input.extend_from_slice(&self.seed);
            input.extend_from_slice(&self.counter.to_le_bytes());
            let block = hl_sign::keccak256(&input);
            let take = (dst.len() - offset).min(block.len());
            dst[offset..offset + take].copy_from_slice(&block[..take]);
            offset += take;
            self.counter += 1;
        }
        Ok(())
    }
}

impl TryCryptoRng for SeededRng {}

/// 平文を封筒へ入れる（`enc || ciphertext`）。
pub fn seal(
    public: &[u8],
    info: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    rng_seed: &[u8; 32],
) -> Result<Vec<u8>, String> {
    let public = <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(public)
        .map_err(|error| format!("invalid public key: {error}"))?;
    let mut rng = SeededRng {
        seed: *rng_seed,
        counter: 0,
    };
    let (enc, ciphertext) = single_shot_seal_with_rng::<
        ChaCha20Poly1305,
        HkdfSha256,
        X25519HkdfSha256,
    >(&OpModeS::Base, &public, info, plaintext, aad, &mut rng)
    .map_err(|error| format!("seal failed: {error}"))?;

    let mut envelope = enc.to_bytes().to_vec();
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

/// 封筒（`enc || ciphertext`）を開ける。
pub fn open(secret: &[u8], info: &[u8], aad: &[u8], envelope: &[u8]) -> Result<Vec<u8>, String> {
    if envelope.len() <= 32 {
        return Err("envelope is too short".to_string());
    }
    let (enc, ciphertext) = envelope.split_at(32);
    let secret = <X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(secret)
        .map_err(|error| format!("invalid secret key: {error}"))?;
    let enc = <X25519HkdfSha256 as Kem>::EncappedKey::from_bytes(enc)
        .map_err(|error| format!("invalid encapsulated key: {error}"))?;
    single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
        &OpModeR::Base,
        &secret,
        &enc,
        info,
        ciphertext,
        aad,
    )
    .map_err(|error| format!("open failed: {error}"))
}

/// 要求・応答の`aad`（network・canister・method・caller・request_id・期限を束縛する）。
pub fn envelope_aad(
    network: &str,
    canister: &[u8],
    method: &str,
    caller: &[u8],
    request_id: &[u8],
    expires_at: u64,
) -> Vec<u8> {
    let mut aad = Vec::new();
    for part in [
        network.as_bytes(),
        canister,
        method.as_bytes(),
        caller,
        request_id,
    ] {
        aad.extend_from_slice(&(part.len() as u32).to_be_bytes());
        aad.extend_from_slice(part);
    }
    aad.extend_from_slice(&expires_at.to_be_bytes());
    aad
}

/// IKMからX25519鍵対を導出し、公開鍵（32バイト）を返す。
pub fn derive_public_key(ikm: &[u8]) -> Vec<u8> {
    derive_keypair(ikm).1
}

/// IKMからX25519鍵対を導出する（秘密鍵はcanister内に留める）。
pub fn derive_keypair(ikm: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (secret, public) = X25519HkdfSha256::derive_keypair(ikm);
    (secret.to_bytes().to_vec(), public.to_bytes().to_vec())
}

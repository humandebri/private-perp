//! HPKE（RFC 9180, X25519 / HKDF-SHA256 / ChaCha20-Poly1305）の往復試験（2D）。
//!
//! canisterではOS乱数が使えないため、決定的RNGを渡す設計を検証する（本番は`raw_rand`を種にする）。
//! `aad`の束縛が効くこと（改竄で復号できないこと）も確認する。

use hpke::{
    Kem, OpModeR, OpModeS, Serializable, aead::ChaCha20Poly1305, kdf::HkdfSha256,
    kem::X25519HkdfSha256, single_shot_open, single_shot_seal_with_rng,
};
use pocket_ic_tests::fixed_rng::FixedRng;

#[test]
fn hpke_seals_and_opens_with_a_deterministic_rng() {
    let ikm = [7u8; 32];
    let (secret, public) = X25519HkdfSha256::derive_keypair(&ikm);
    assert_eq!(public.to_bytes().len(), 32);

    let info = b"private-perp/envelope/v1";
    let aad = b"local|canister|method|caller|request_id|expires";
    let plaintext = b"{\"market\":\"BTC\"}";

    let mut rng = FixedRng::new([9u8; 32]);
    let (enc, ciphertext) = single_shot_seal_with_rng::<
        ChaCha20Poly1305,
        HkdfSha256,
        X25519HkdfSha256,
    >(&OpModeS::Base, &public, info, plaintext, aad, &mut rng)
    .expect("seal");

    let opened = single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
        &OpModeR::Base,
        &secret,
        &enc,
        info,
        &ciphertext,
        aad,
    )
    .expect("open");
    assert_eq!(opened, plaintext);

    // aadが一致しない場合は復号できない（束縛が効いている）。
    let tampered = single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
        &OpModeR::Base,
        &secret,
        &enc,
        info,
        &ciphertext,
        b"other aad",
    );
    assert!(tampered.is_err(), "aad不一致は失敗する");

    // 同じIKMから同じ鍵対が決定的に導出できる（canisterでraw_randから作る前提）。
    let (secret2, public2) = X25519HkdfSha256::derive_keypair(&ikm);
    assert_eq!(secret.to_bytes(), secret2.to_bytes());
    assert_eq!(public.to_bytes(), public2.to_bytes());
}

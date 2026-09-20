//! ローカルECDSAスパイク（`Implementation.md` 1-2、9.1の4）。
//!
//! PocketICの閾値ECDSAで公開鍵導出と署名ができ、`v`を復元できることを確認する。
//! 実測した所要時間は `docs/phase-1/evidence/` に記録する（testnet実測ではない）。

use api_types::error::ErrorCode;
use hl_sign::signature::{Signature, address_from_public_key, recover_address};
use pocket_ic_tests::{FUNDS_VAULT_WASM, deploy_default, pic, principal, update_args};
use std::time::Instant;

#[test]
fn threshold_ecdsa_signs_and_recovers_locally() {
    let pic = pic();
    let vault = deploy_default(&pic, FUNDS_VAULT_WASM);
    let caller = principal(30);

    let digest = hl_sign::keccak256(b"private-perp ecdsa spike");
    let started = Instant::now();
    let outcome: Result<(Vec<u8>, Vec<u8>), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_ecdsa_roundtrip",
        (7u32, digest.to_vec()),
    )
    .expect("test_ecdsa_roundtrip call");
    let elapsed = started.elapsed();
    let (public_key, signature_bytes) = outcome.expect("ecdsa roundtrip");
    println!("threshold ecdsa roundtrip: {:?}", elapsed);

    let public_key: [u8; 33] = public_key.try_into().expect("33-byte public key");
    assert!(
        public_key[0] == 2 || public_key[0] == 3,
        "compressed SEC1 point"
    );
    let signature_bytes: [u8; 65] = signature_bytes.try_into().expect("65-byte signature");
    let signature = Signature::from_bytes65(&signature_bytes).expect("signature");

    // 署名は公開鍵のアドレスへ復元できる（=閾値署名と v 復元が正しい）。
    let address = address_from_public_key(&public_key).expect("address");
    assert_eq!(
        recover_address(&digest, &signature, Some(&public_key)).expect("recover"),
        address
    );

    // 同じseedは同じ公開鍵（導出が決定的）。
    let again: Result<(Vec<u8>, Vec<u8>), ErrorCode> = update_args(
        &pic,
        vault,
        caller,
        "test_ecdsa_roundtrip",
        (7u32, hl_sign::keccak256(b"another digest").to_vec()),
    )
    .expect("second call");
    let (public_key_2, _) = again.expect("second roundtrip");
    assert_eq!(public_key.to_vec(), public_key_2, "same seed, same key");
}

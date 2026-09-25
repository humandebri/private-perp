//! Hyperliquidのactionハッシュ・EIP-712・署名。
//!
//! `Implementation.md` 3.2 により**非async・純粋関数のみ**とする。`await`・
//! `ic0.call_perform`・inter-canister callをこのクレートへ持ち込まない
//! （`scripts/check-no-await.sh` でCI検査する）。
//!
//! 署名実装は公式SDKへ依存しない。公式SDKは
//! `crates/hl-sign/tests/fixtures/` のテストベクトルの生成元としてのみ使う
//! （`Implementation.md` 9.1）。actionの組み立ては `hl-types` が担う。
#![forbid(unsafe_code)]

pub mod eip712;
pub mod error;
pub mod hash;
pub mod keccak;
pub mod private_perp;
pub mod signature;
pub mod typed_data;
pub mod usd_send;
pub mod user_signed;

pub use eip712::Domain;
pub use error::SignError;
pub use hash::{ActionHashInput, action_hash, sign_action_for_tests, signing_digest};
pub use keccak::{keccak256, keccak256_concat};
pub use signature::{
    Signature, address_from_public_key, address_from_secret, public_key_compressed,
    recover_address, recover_v, sign_digest_for_tests,
};
pub use typed_data::{TypedField, TypedKind, TypedValue, digest_with_domain, sign_with_domain};

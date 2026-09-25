//! Canister APIのCandid型。
//!
//! `docs/phase-0/api-contract.md` の契約をRustの型として固定し、Canisterと
//! PocketIC試験（`crates/pocket-ic-tests`）の双方から同じ定義を使う。
//!
//! このクレートは純粋であり、`ic-cdk`・Canister API・非同期処理へ依存しない。
//! ロジックは置かず、型と定数だけを置く。

pub mod auth;
pub mod builder_fee;
pub mod eligibility;
pub mod envelope;
pub mod environment;
pub mod error;
pub mod fund;
pub mod guard;
pub mod journal;
pub mod operations;
pub mod operations_status;
pub mod order;
pub mod policy;
pub mod recovery;

use candid::CandidType;
use serde::{Deserialize, Serialize};

/// IC network。`docs/phase-0/environments.md` の環境分離の単位。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Network {
    Local,
    Testnet,
    Mainnet,
}

/// 口座の用途。共通保管（取引しない）とユーザー別取引口座を区別する。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Reserve,
    Trading,
}

/// 資産・銘柄。初期対応はHyperCore USDCとBTC・ETH perpsのみ。
#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetId {
    Usdc,
    BtcPerp,
    EthPerp,
}

/// バイナリ列（Candid `blob`）。
pub type Blob = serde_bytes::ByteBuf;

/// ミリ秒の時刻（Canister時刻）。
pub type Timestamp = u64;

/// USDC最小単位（1e-6 USDC）。`docs/phase-0/money-and-units.md` 2節。
pub type Micros = u64;

/// カーソルページング（`docs/phase-0/api-contract.md` 1.2）。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Blob>,
    pub observed_at: Timestamp,
    pub revision: u64,
}

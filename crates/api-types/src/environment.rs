//! 環境設定の公開型（`docs/phase-0/environments.md`）。
//!
//! 環境は「network」「鍵」「endpoint」で分離する。値は秘密ではなく、運用時の確認
//! （E-2・E-5）のため`get_environment`で公開する。

use crate::Network;
use candid::CandidType;
use serde::{Deserialize, Serialize};

impl From<Network> for hl_types::Network {
    fn from(network: Network) -> Self {
        match network {
            Network::Local => Self::Local,
            Network::Testnet => Self::Testnet,
            Network::Mainnet => Self::Mainnet,
        }
    }
}

impl From<hl_types::Network> for Network {
    fn from(network: hl_types::Network) -> Self {
        match network {
            hl_types::Network::Local => Self::Local,
            hl_types::Network::Testnet => Self::Testnet,
            hl_types::Network::Mainnet => Self::Mainnet,
        }
    }
}

/// 解決済みの環境設定（既定値の適用と検証を通したもの）。
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentView {
    pub network: Network,
    /// Hyperliquidの`/exchange` endpoint。
    pub exchange_url: String,
    /// Hyperliquidの`/info` endpoint。
    pub info_url: String,
    /// 閾値ECDSAのkey ID。
    pub ecdsa_key_id: String,
}

//! private-perp の共有型。
//!
//! `Implementation.md` 3.2 により純粋クレートとする。非同期処理とCanister APIへ依存しない。
//! ここに置く型は Phase 0 の契約（`docs/phase-0/money-and-units.md`、
//! `docs/phase-0/environments.md`）に対応する。
#![forbid(unsafe_code)]

/// IC network。`docs/phase-0/environments.md` の環境分離の単位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Network {
    /// ローカルレプリカ。Hyperliquidはmockを使うため実endpointを持たない。
    Local,
    Testnet,
    Mainnet,
}

impl Network {
    /// Hyperliquid REST endpoint。mockを使うローカルでは `None`。
    pub const fn hl_rest_endpoint(self) -> Option<&'static str> {
        match self {
            Self::Local => None,
            Self::Testnet => Some("https://api.hyperliquid-testnet.xyz"),
            Self::Mainnet => Some("https://api.hyperliquid.xyz"),
        }
    }

    /// ブラウザが公開市況のためだけに直結するHyperliquid WS endpoint。
    /// `Implementation.md` 6.1（D11）。本人データをこの経路へ流さない。
    pub const fn hl_ws_endpoint(self) -> Option<&'static str> {
        match self {
            Self::Local => None,
            Self::Testnet => Some("wss://api.hyperliquid-testnet.xyz/ws"),
            Self::Mainnet => Some("wss://api.hyperliquid.xyz/ws"),
        }
    }

    /// 本番環境か。mock issuer・mock tokenを本番設定で通さないための判定に使う
    /// （`docs/phase-0/environments.md` 4節 E-1、E-2）。
    pub const fn is_production(self) -> bool {
        matches!(self, Self::Mainnet)
    }
}

/// 口座の用途。共通保管（取引しない）とユーザー別取引口座を区別する
/// （`docs/phase-0/authority-matrix.md` 2節）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountKind {
    /// 共通保管口座。Agentを承認しない。
    Reserve,
    /// ユーザー別取引口座。masterは独立で、sub-accountではない。
    Trading,
}

/// USDCの最小単位（1e-6 USDC）。
///
/// `docs/phase-0/money-and-units.md` 2節に従い、浮動小数点を使わない。
/// 上限額のポリシー値は同文書9節のとおり未確定であり、ここでは表現範囲のみを固定する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UsdcMicros(u64);

impl UsdcMicros {
    /// 1 USDC の最小単位数。
    pub const SCALE: u64 = 1_000_000;

    pub const ZERO: Self = Self(0);

    /// 表現可能な最大値。実装上の上限額は別途確定する。
    pub const MAX: Self = Self(u64::MAX);

    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    pub const fn micros(self) -> u64 {
        self.0
    }

    /// 小数6桁の正規化表記。末尾ゼロを削除し、指数表記を使わない。
    pub fn to_decimal_string(self) -> String {
        let whole = self.0 / Self::SCALE;
        let fraction = self.0 % Self::SCALE;
        if fraction == 0 {
            return whole.to_string();
        }
        let digits = format!("{fraction:06}");
        let trimmed = digits.trim_end_matches('0');
        format!("{whole}.{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountKind, Network, UsdcMicros};

    #[test]
    fn network_endpoints_are_separated() {
        assert_eq!(Network::Local.hl_rest_endpoint(), None);
        assert!(!Network::Testnet.is_production());
        assert!(Network::Mainnet.is_production());
        assert_ne!(
            Network::Testnet.hl_ws_endpoint(),
            Network::Mainnet.hl_ws_endpoint()
        );
    }

    #[test]
    fn account_kinds_are_distinct() {
        assert_ne!(AccountKind::Reserve, AccountKind::Trading);
    }

    #[test]
    fn usdc_strings_are_normalized() {
        assert_eq!(UsdcMicros::ZERO.to_decimal_string(), "0");
        assert_eq!(UsdcMicros::from_micros(1).to_decimal_string(), "0.000001");
        assert_eq!(
            UsdcMicros::from_micros(1_500_000).to_decimal_string(),
            "1.5"
        );
        assert_eq!(
            UsdcMicros::from_micros(10_000_000).to_decimal_string(),
            "10"
        );
        assert_eq!(UsdcMicros::MAX.to_decimal_string(), "18446744073709.551615");
    }

    #[test]
    fn usdc_arithmetic_stays_integer() {
        let total = UsdcMicros::from_micros(2_500_000);
        assert_eq!(total.micros(), 2_500_000);
        assert!(UsdcMicros::from_micros(1) < UsdcMicros::from_micros(2));
    }
}

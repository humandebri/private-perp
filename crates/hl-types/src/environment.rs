//! 環境分離（network・endpoint・鍵ID）。`docs/phase-0/environments.md`。
//!
//! 環境は「network」「鍵」「endpoint」で分離し、1つの設定ミスでmockや testnet の
//! 設定が本番（mainnet）へ通らないようにする。判定は**ビルド定数ではなく起動時に
//! 検証できる値**で行う（同4節）。Phase 2ではmainnetを有効化しない（E-2で拒否を試験）。
#![forbid(unsafe_code)]

use crate::Network;

/// testnetのHyperliquid host。
pub const TESTNET_HOST: &str = "api.hyperliquid-testnet.xyz";
/// mainnetのHyperliquid host。Phase 2では接続しない。
pub const MAINNET_HOST: &str = "api.hyperliquid.xyz";
/// ローカルのmock HLを使うときのプレースホルダhost。
///
/// ローカルはmockを前提とするため、未設定のまま実endpointへ出ないようループバックを使う。
/// 実際のmockのURLは起動時に設定する（`set_venue_endpoints`）。
pub const LOCAL_HOST: &str = "localhost";
/// ローカルの既定ポート（mock HLを立てる場合の例）。
pub const LOCAL_PORT: u16 = 8080;

/// 環境設定の解決エラー。理由コードは`docs/phase-0/environments.md` 4節の試験に対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentError {
    /// mainnetはPhase 2では拒否する（E-2）。
    MainnetNotEnabled,
    /// network名が既知でない。
    UnknownNetwork,
    /// URLの形式が不正（schemeはhttp/https、hostが必要）。
    InvalidUrl,
    /// networkとendpointのhostが一致しない（E-2）。
    HostMismatch,
    /// key IDが不正（空・長すぎ・使用不可の文字）。
    InvalidKeyId,
}

impl EnvironmentError {
    /// 診断用の短い理由コード。
    pub const fn code(self) -> &'static str {
        match self {
            Self::MainnetNotEnabled => "mainnet_not_enabled",
            Self::UnknownNetwork => "unknown_network",
            Self::InvalidUrl => "invalid_url",
            Self::HostMismatch => "host_mismatch",
            Self::InvalidKeyId => "invalid_key_id",
        }
    }
}

/// network名（`local`・`testnet`・`mainnet`）を解釈する。
///
/// mainnetは「未対応」ではなく**明示的に拒否**する（Phase 2で実資金を扱わない）。
pub fn parse_network(name: &str) -> Result<Network, EnvironmentError> {
    match name {
        "local" => Ok(Network::Local),
        "testnet" => Ok(Network::Testnet),
        "mainnet" => Err(EnvironmentError::MainnetNotEnabled),
        _ => Err(EnvironmentError::UnknownNetwork),
    }
}

/// network名（設定の保存形式）。
pub const fn network_name(network: Network) -> &'static str {
    match network {
        Network::Local => "local",
        Network::Testnet => "testnet",
        Network::Mainnet => "mainnet",
    }
}

/// URLからhostを取り出す（schemeはhttp/httpsのみ、hostは空でないこと）。
pub fn url_host(url: &str) -> Result<&str, EnvironmentError> {
    let rest = match url.strip_prefix("https://") {
        Some(rest) => rest,
        None => match url.strip_prefix("http://") {
            Some(rest) => rest,
            None => return Err(EnvironmentError::InvalidUrl),
        },
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // userinfo・portを落としてhostだけを比較する。
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err(EnvironmentError::InvalidUrl);
    }
    Ok(host)
}

/// networkに対して許されるhostか。
///
/// - testnet: testnet hostのみ
/// - local: HLの実host（testnet・mainnet）を**拒否**する（mock前提。取り違えを防ぐ）
/// - mainnet: 常に拒否（Phase 2）
pub fn host_allowed(network: Network, host: &str) -> bool {
    match network {
        Network::Testnet => host == TESTNET_HOST,
        Network::Local => host != TESTNET_HOST && host != MAINNET_HOST,
        Network::Mainnet => false,
    }
}

/// `/exchange`と`/info`のendpointを検証する（E-2）。
pub fn validate_endpoints(
    network: Network,
    exchange_url: &str,
    info_url: &str,
) -> Result<(), EnvironmentError> {
    if network.is_production() {
        return Err(EnvironmentError::MainnetNotEnabled);
    }
    for url in [exchange_url, info_url] {
        let host = url_host(url)?;
        if !host_allowed(network, host) {
            return Err(EnvironmentError::HostMismatch);
        }
    }
    Ok(())
}

/// 既定のendpoint（`(exchange, info)`）。
///
/// ローカルはmockを立てる前提のループバックを使う（未設定のまま実endpointへ出ない）。
pub fn default_endpoints(
    network: Network,
) -> Result<(&'static str, &'static str), EnvironmentError> {
    match network {
        Network::Local => Ok((
            "http://localhost:8080/exchange",
            "http://localhost:8080/info",
        )),
        Network::Testnet => Ok((
            "https://api.hyperliquid-testnet.xyz/exchange",
            "https://api.hyperliquid-testnet.xyz/info",
        )),
        Network::Mainnet => Err(EnvironmentError::MainnetNotEnabled),
    }
}

/// 既定のtECDSA key ID。
///
/// local・testnetはテスト鍵（`test_key_1`。testnetの実名はデプロイ後に確定し、
/// `set_ecdsa_key_id`で上書きする）。mainnetはPhase 2では拒否する。
pub fn default_ecdsa_key_id(network: Network) -> Result<&'static str, EnvironmentError> {
    match network {
        Network::Local | Network::Testnet => Ok("test_key_1"),
        Network::Mainnet => Err(EnvironmentError::MainnetNotEnabled),
    }
}

/// key IDの妥当性（空・長すぎ・空白を拒否。名前は英数と`_`・`-`に限る）。
pub fn validate_key_id(key_id: &str) -> Result<(), EnvironmentError> {
    if key_id.is_empty() || key_id.len() > 64 {
        return Err(EnvironmentError::InvalidKeyId);
    }
    let valid = key_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-');
    if !valid {
        return Err(EnvironmentError::InvalidKeyId);
    }
    Ok(())
}

/// EIP-712の`hyperliquidChain`値（ローカル・testnetは`Testnet`）。
pub fn chain_name(network: Network) -> Result<&'static str, EnvironmentError> {
    match network {
        Network::Local | Network::Testnet => Ok("Testnet"),
        Network::Mainnet => Err(EnvironmentError::MainnetNotEnabled),
    }
}

/// EIP-712の`signatureChainId`（16進。testnetは0x66eee）。
pub fn signature_chain_id(network: Network) -> Result<&'static str, EnvironmentError> {
    match network {
        Network::Local | Network::Testnet => Ok("0x66eee"),
        Network::Mainnet => Err(EnvironmentError::MainnetNotEnabled),
    }
}

/// user-signed EIP-712のchain id（testnet・ローカルは421614）。
pub fn user_signed_chain_id(network: Network) -> Result<u64, EnvironmentError> {
    match network {
        Network::Local | Network::Testnet => Ok(421_614),
        Network::Mainnet => Err(EnvironmentError::MainnetNotEnabled),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EnvironmentError, MAINNET_HOST, TESTNET_HOST, chain_name, default_ecdsa_key_id,
        default_endpoints, network_name, parse_network, signature_chain_id, url_host,
        user_signed_chain_id, validate_endpoints, validate_key_id,
    };
    use crate::Network;

    #[test]
    fn networks_are_parsed_and_mainnet_is_refused() {
        assert_eq!(parse_network("local"), Ok(Network::Local));
        assert_eq!(parse_network("testnet"), Ok(Network::Testnet));
        // E-2: Phase 2ではmainnetを明示的に拒否する。
        assert_eq!(
            parse_network("mainnet"),
            Err(EnvironmentError::MainnetNotEnabled)
        );
        assert_eq!(
            parse_network("devnet"),
            Err(EnvironmentError::UnknownNetwork)
        );
        assert_eq!(network_name(Network::Local), "local");
        assert_eq!(network_name(Network::Testnet), "testnet");
        assert_eq!(network_name(Network::Mainnet), "mainnet");
    }

    #[test]
    fn hosts_are_extracted_without_scheme_or_port() {
        assert_eq!(
            url_host("https://api.hyperliquid-testnet.xyz/info"),
            Ok(TESTNET_HOST)
        );
        assert_eq!(url_host("http://localhost:8080/exchange"), Ok("localhost"));
        assert_eq!(url_host("https://user@example.com/x"), Ok("example.com"));
        assert_eq!(
            url_host("api.hyperliquid.xyz/info"),
            Err(EnvironmentError::InvalidUrl)
        );
        assert_eq!(url_host("https://"), Err(EnvironmentError::InvalidUrl));
    }

    #[test]
    fn testnet_requires_the_testnet_host() {
        assert_eq!(
            validate_endpoints(
                Network::Testnet,
                "https://api.hyperliquid-testnet.xyz/exchange",
                "https://api.hyperliquid-testnet.xyz/info"
            ),
            Ok(())
        );
        // E-2: testnet設定にmainnet endpointを混ぜると拒否する。
        assert_eq!(
            validate_endpoints(
                Network::Testnet,
                "https://api.hyperliquid.xyz/exchange",
                "https://api.hyperliquid-testnet.xyz/info"
            ),
            Err(EnvironmentError::HostMismatch)
        );
        // 別環境のendpoint（別ドメイン）も拒否する。
        assert_eq!(
            validate_endpoints(
                Network::Testnet,
                "https://api.hyperliquid.xyz.evil.test/exchange",
                "https://api.hyperliquid-testnet.xyz/info"
            ),
            Err(EnvironmentError::HostMismatch)
        );
        assert_eq!(
            validate_endpoints(Network::Testnet, "not-a-url", "not-a-url"),
            Err(EnvironmentError::InvalidUrl)
        );
    }

    #[test]
    fn local_refuses_the_real_venue_hosts() {
        let (exchange, info) = default_endpoints(Network::Local).expect("local defaults");
        assert_eq!(url_host(exchange), Ok(super::LOCAL_HOST));
        assert_eq!(url_host(info), Ok(super::LOCAL_HOST));
        assert_eq!(validate_endpoints(Network::Local, exchange, info), Ok(()));
        // ローカルの設定で実testnetへ出さない（mock前提の取り違えを防ぐ）。
        assert_eq!(
            validate_endpoints(
                Network::Local,
                "https://api.hyperliquid-testnet.xyz/exchange",
                "http://localhost:8080/info"
            ),
            Err(EnvironmentError::HostMismatch)
        );
        // mockのホストは任意に設定できる。
        assert_eq!(
            validate_endpoints(
                Network::Local,
                "http://127.0.0.1:9000/exchange",
                "http://127.0.0.1:9000/info"
            ),
            Ok(())
        );
    }

    #[test]
    fn mainnet_is_refused_everywhere() {
        assert_eq!(
            default_endpoints(Network::Mainnet),
            Err(EnvironmentError::MainnetNotEnabled)
        );
        assert_eq!(
            default_ecdsa_key_id(Network::Mainnet),
            Err(EnvironmentError::MainnetNotEnabled)
        );
        assert_eq!(
            chain_name(Network::Mainnet),
            Err(EnvironmentError::MainnetNotEnabled)
        );
        assert_eq!(
            signature_chain_id(Network::Mainnet),
            Err(EnvironmentError::MainnetNotEnabled)
        );
        assert_eq!(
            user_signed_chain_id(Network::Mainnet),
            Err(EnvironmentError::MainnetNotEnabled)
        );
        assert_eq!(
            validate_endpoints(
                Network::Mainnet,
                "https://api.hyperliquid.xyz/exchange",
                "https://api.hyperliquid.xyz/info"
            ),
            Err(EnvironmentError::MainnetNotEnabled)
        );
    }

    #[test]
    fn testnet_values_are_the_verified_ones() {
        let (exchange, info) = default_endpoints(Network::Testnet).expect("testnet defaults");
        assert_eq!(url_host(exchange), Ok(TESTNET_HOST));
        assert_eq!(url_host(info), Ok(TESTNET_HOST));
        assert_eq!(default_ecdsa_key_id(Network::Testnet), Ok("test_key_1"));
        assert_eq!(chain_name(Network::Testnet), Ok("Testnet"));
        assert_eq!(signature_chain_id(Network::Testnet), Ok("0x66eee"));
        assert_eq!(user_signed_chain_id(Network::Testnet), Ok(421_614));
        assert!(!host_allowed(Network::Testnet, MAINNET_HOST));
    }

    #[test]
    fn key_ids_are_validated() {
        assert_eq!(validate_key_id("test_key_1"), Ok(()));
        assert_eq!(validate_key_id("key_1"), Ok(()));
        assert_eq!(validate_key_id(""), Err(EnvironmentError::InvalidKeyId));
        assert_eq!(
            validate_key_id("key 1"),
            Err(EnvironmentError::InvalidKeyId)
        );
        assert_eq!(
            validate_key_id(&"k".repeat(65)),
            Err(EnvironmentError::InvalidKeyId)
        );
    }

    use super::host_allowed;
}

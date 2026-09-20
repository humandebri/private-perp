//! 実行時設定。`docs/phase-0/environments.md` の分離に対応する。
//!
//! 値は今のところビルド定数である。デプロイ時の上書き（canister環境変数）は
//! アイデンティティ・鍵・endpointを扱う段階で導入する。ここへ秘密を置かない。

use api_types::Network;

/// ログインchallengeの有効期間（5分）。
pub const CHALLENGE_TTL_MS: u64 = 5 * 60 * 1000;

/// セッションの有効期間（30分）。
pub const SESSION_TTL_MS: u64 = 30 * 60 * 1000;

/// 同一EOAが発行できるchallengeの件数上限（ウィンドウあたり）。
pub const CHALLENGE_RATE_LIMIT: u64 = 5;

/// challenge発行レートの観測ウィンドウ（10分）。
pub const CHALLENGE_RATE_WINDOW_MS: u64 = 10 * 60 * 1000;

/// 受理前payloadの上限（`docs/phase-0/api-contract.md` 1.2）。
/// 資金APIで使う（後続の段階）。
#[allow(dead_code)]
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

/// 一覧の1ページ上限。
#[allow(dead_code)]
pub const MAX_PAGE_SIZE: u32 = 100;

/// 閾値ECDSAのkey id。PocketICでの実測で確定する（候補: `test_key_1`／`dfx_test_key`）。
/// 本番は `key_1`（`pzp6e`）を使う。値は `docs/phase-0/environments.md` に記録する。
pub const ECDSA_KEY_ID: &str = "test_key_1";

/// Hyperliquidのuser-signed EIP-712に使うchain id（testnet/localは421614）。
/// 払出しのwire payload構築（署名段階）で使う。
#[allow(dead_code)]
pub const HL_USER_SIGNED_CHAIN_ID: u64 = 421_614;

/// 現在のnetwork。ローカル・testnet・mainnetを混同しないための識別子。
#[allow(dead_code)]
pub const NETWORK: Network = Network::Local;

/// networkの識別子（EIP-712のメッセージへ入れる）。
pub const fn network_name(network: Network) -> &'static str {
    match network {
        Network::Local => "local",
        Network::Testnet => "testnet",
        Network::Mainnet => "mainnet",
    }
}

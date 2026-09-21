//! 実行時設定（TTL・上限）。`docs/phase-0/environments.md` の分離に対応する。
//!
//! network・HL endpoint・tECDSA key IDは**起動時の設定**として
//! `crate::environment` が解決する（ビルド定数にしない。同4節）。ここへ秘密を置かない。

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

/// networkの識別子（EIP-712のメッセージへ入れる）。
///
/// 環境モデルの実装は `hl_types::environment` に置く（この関数は既存の呼び出し元の
/// 便宜のための委譲）。
pub const fn network_name(network: Network) -> &'static str {
    hl_types::environment::network_name(convert_network(network))
}

const fn convert_network(network: Network) -> hl_types::Network {
    match network {
        Network::Local => hl_types::Network::Local,
        Network::Testnet => hl_types::Network::Testnet,
        Network::Mainnet => hl_types::Network::Mainnet,
    }
}

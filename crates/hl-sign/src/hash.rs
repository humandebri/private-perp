//! actionハッシュと署名対象ダイジェスト。
//!
//! 署名対象は
//! `keccak256( msgpack(action) || nonce_be_u64 || vault_flag[|| vault_addr] || [0x00 || expires_after_be_u64] )`
//! を `connectionId` とする phantom agent のEIP-712ダイジェストである。
//! 連結の順序と `vault_flag` の扱いは公式SDKの実装に合わせ、
//! `crates/hl-sign/tests/fixtures/` のテストベクトルで確認する。

use crate::eip712::{Domain, agent_digest};
use crate::error::SignError;
use crate::keccak::keccak256;
use crate::signature::{Signature, sign_digest};

/// actionハッシュの入力。
#[derive(Debug, Clone, Copy)]
pub struct ActionHashInput<'a> {
    /// canonical msgpackで符号化したaction本体。
    pub action_msgpack: &'a [u8],
    pub nonce: u64,
    /// vault（sub-account）経由の場合のみ指定する。20バイト。
    pub vault_address: Option<[u8; 20]>,
    pub expires_after: Option<u64>,
}

/// actionハッシュ。
pub fn action_hash(input: &ActionHashInput<'_>) -> [u8; 32] {
    let mut data = Vec::with_capacity(
        input.action_msgpack.len() + 8 + 1 + 20 + if input.expires_after.is_some() { 8 } else { 0 },
    );
    data.extend_from_slice(input.action_msgpack);
    data.extend_from_slice(&input.nonce.to_be_bytes());
    match input.vault_address {
        Some(address) => {
            data.push(1);
            data.extend_from_slice(&address);
        }
        None => data.push(0),
    }
    if let Some(expires_after) = input.expires_after {
        // SDKは `expiresAfter` が指定された場合に 0x00 のマーカーを1バイト足してから
        // 8バイトbig-endianで連結する（公式SDK `createL1ActionHash` の実装）。
        data.push(0);
        data.extend_from_slice(&expires_after.to_be_bytes());
    }
    keccak256(&data)
}

/// phantom agentの署名対象ダイジェスト。`mainnet` でsourceが `a`／`b` に変わる。
pub fn signing_digest(action_hash: [u8; 32], mainnet: bool) -> [u8; 32] {
    let source = if mainnet { "a" } else { "b" };
    agent_digest(&Domain::exchange(), source, action_hash)
}

/// actionをハッシュし、そのダイジェストへ署名する。
///
/// 本番の署名はtECDSA（管理Canister）で行う。この関数はテストベクトル比較と
/// 決定的なローカル検証のために秘密鍵を直接受け取る。
pub fn sign_action(
    input: &ActionHashInput<'_>,
    secret_key: &[u8; 32],
    mainnet: bool,
) -> Result<Signature, SignError> {
    let digest = signing_digest(action_hash(input), mainnet);
    sign_digest(&digest, secret_key)
}

#[cfg(test)]
mod tests {
    use super::{ActionHashInput, action_hash, sign_action, signing_digest};
    use crate::signature::{address_from_secret, recover_address, sign_digest};

    fn secret(seed: u8) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[31] = seed;
        bytes
    }

    #[test]
    fn action_hash_changes_with_every_bound_field() {
        let base = ActionHashInput {
            action_msgpack: b"action",
            nonce: 1_758_000_000_000,
            vault_address: None,
            expires_after: None,
        };
        let hash = action_hash(&base);

        let other_nonce = ActionHashInput {
            nonce: base.nonce + 1,
            ..base
        };
        assert_ne!(hash, action_hash(&other_nonce));

        let with_vault = ActionHashInput {
            vault_address: Some([1u8; 20]),
            ..base
        };
        assert_ne!(hash, action_hash(&with_vault));

        let with_expiry = ActionHashInput {
            expires_after: Some(1_758_000_060_000),
            ..base
        };
        assert_ne!(hash, action_hash(&with_expiry));
    }

    #[test]
    fn action_hash_is_deterministic() {
        let input = ActionHashInput {
            action_msgpack: b"deterministic",
            nonce: 42,
            vault_address: None,
            expires_after: None,
        };
        assert_eq!(action_hash(&input), action_hash(&input));
    }

    #[test]
    fn signing_digest_differs_between_networks() {
        let hash = action_hash(&ActionHashInput {
            action_msgpack: b"action",
            nonce: 7,
            vault_address: None,
            expires_after: None,
        });
        assert_ne!(signing_digest(hash, true), signing_digest(hash, false));
    }

    #[test]
    fn signed_action_recovers_to_the_signer() {
        let secret_key = secret(21);
        let input = ActionHashInput {
            action_msgpack: b"action",
            nonce: 1_758_000_000_000,
            vault_address: None,
            expires_after: None,
        };
        let signature = sign_action(&input, &secret_key, false).expect("sign action");
        let digest = signing_digest(action_hash(&input), false);
        assert_eq!(
            recover_address(&digest, &signature, None).expect("recover"),
            address_from_secret(&secret_key).expect("address")
        );
        // 同じ入力なら同じ署名（決定的）。
        assert_eq!(
            sign_digest(&digest, &secret_key).expect("sign digest"),
            signature
        );
    }
}

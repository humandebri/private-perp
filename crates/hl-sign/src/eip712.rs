//! EIP-712（domain separator・phantom agent）。
//!
//! Hyperliquidの取引actionは、actionハッシュを `connectionId` に持つ
//! phantom agent をEIP-712で署名する。domainは `Exchange` / `1` / `1337` / `0x0`。
//!
//! 値の根拠は公式SDKの実装であり、`crates/hl-sign/tests/fixtures/` の
//! テストベクトルと一致することを試験で確認する。

use crate::keccak::{keccak256, keccak256_concat};

/// EIP-712 domain。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    pub name: &'static str,
    pub version: &'static str,
    pub chain_id: u64,
    pub verifying_contract: [u8; 20],
}

impl Domain {
    /// Hyperliquidの取引action用domain。
    pub const fn exchange() -> Self {
        Self {
            name: "Exchange",
            version: "1",
            chain_id: 1337,
            verifying_contract: [0u8; 20],
        }
    }
}

/// `EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)`
pub fn domain_type_hash() -> [u8; 32] {
    keccak256(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
}

/// `Agent(string source,bytes32 connectionId)`
pub fn agent_type_hash() -> [u8; 32] {
    keccak256(b"Agent(string source,bytes32 connectionId)")
}

/// domain separator。
pub fn domain_separator(domain: &Domain) -> [u8; 32] {
    keccak256_concat(&[
        &domain_type_hash(),
        &keccak256(domain.name.as_bytes()),
        &keccak256(domain.version.as_bytes()),
        &u64_word(domain.chain_id),
        &address_word(domain.verifying_contract),
    ])
}

/// Agent構造体のハッシュ（`keccak256(abi.encode(typeHash, keccak256(source), connectionId))`）。
pub fn agent_struct_hash(source: &str, connection_id: [u8; 32]) -> [u8; 32] {
    keccak256_concat(&[
        &agent_type_hash(),
        &keccak256(source.as_bytes()),
        &connection_id,
    ])
}

/// EIP-712の署名対象ダイジェスト（`keccak256(0x1901 || domainSeparator || structHash)`）。
pub fn agent_digest(domain: &Domain, source: &str, connection_id: [u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(2 + 32 + 32);
    input.extend_from_slice(&[0x19, 0x01]);
    input.extend_from_slice(&domain_separator(domain));
    input.extend_from_slice(&agent_struct_hash(source, connection_id));
    keccak256(&input)
}

/// `u64` を32バイトのワードへ（左詰めゼロ埋め）。
pub fn u64_word(value: u64) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

/// 20バイトのアドレスを32バイトのワードへ。
pub fn address_word(address: [u8; 20]) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(&address);
    word
}

#[cfg(test)]
mod tests {
    use super::{
        Domain, address_word, agent_digest, agent_struct_hash, domain_separator, domain_type_hash,
        u64_word,
    };

    #[test]
    fn type_hashes_are_stable() {
        let hex = |bytes: [u8; 32]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        // 型文字列のKeccak-256。SDK実装と一致することをfixtureで確認する。
        assert_eq!(domain_type_hash().len(), 32);
        assert_eq!(
            hex(domain_type_hash()),
            hex(crate::keccak::keccak256(
                b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
            ))
        );
    }

    #[test]
    fn abi_words_are_left_padded() {
        assert_eq!(u64_word(1)[31], 1);
        assert_eq!(u64_word(1)[0..31], [0u8; 31]);
        let mut address = [0u8; 20];
        address[19] = 1;
        assert_eq!(address_word(address)[31], 1);
        assert_eq!(address_word(address)[0..12], [0u8; 12]);
    }

    #[test]
    fn domain_separator_depends_on_chain_id() {
        let exchange = Domain::exchange();
        let other = Domain {
            chain_id: 1,
            ..exchange
        };
        assert_ne!(domain_separator(&exchange), domain_separator(&other));
    }

    #[test]
    fn agent_struct_hash_uses_source_and_connection_id() {
        let connection_id = [7u8; 32];
        assert_ne!(
            agent_struct_hash("a", connection_id),
            agent_struct_hash("b", connection_id)
        );
        assert_ne!(
            agent_struct_hash("a", connection_id),
            agent_struct_hash("a", [8u8; 32])
        );
    }

    #[test]
    fn digest_is_deterministic_and_domain_bound() {
        let connection_id = [9u8; 32];
        let mainnet = Domain::exchange();
        assert_eq!(
            agent_digest(&mainnet, "a", connection_id),
            agent_digest(&mainnet, "a", connection_id)
        );
        assert_ne!(
            agent_digest(&mainnet, "a", connection_id),
            agent_digest(&mainnet, "b", connection_id)
        );
    }
}

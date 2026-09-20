//! 本サービス自身のEIP-712型（ログインchallengeと出金intent）。
//!
//! Hyperliquidの署名方式（phantom agent／user-signed）とは別の、独自の型である。
//! domainは `private-perp` / `1` / chainId `1` / verifyingContract `0x0` に固定し、
//! networkとcanister（vault principal）をメッセージ側で束縛する。
//!
//! 確定した型は `docs/phase-0/api-contract.md` 2節の契約を具体化したものである。

use crate::eip712::Domain;
use crate::error::SignError;
use crate::signature::Signature;
use crate::typed_data::{TypedField, TypedKind, TypedValue, digest_with_domain, sign_with_domain};

pub const DOMAIN_NAME: &str = "private-perp";
pub const DOMAIN_VERSION: &str = "1";
pub const DOMAIN_CHAIN_ID: u64 = 1;

const fn field(name: &'static str, kind: TypedKind) -> TypedField {
    TypedField { name, kind }
}

/// challengeの型（`purpose` は `"login"` または `"withdrawal"`）。
pub const CHALLENGE_PRIMARY_TYPE: &str = "PrivatePerpChallenge";
pub const CHALLENGE_FIELDS: &[TypedField] = &[
    field("purpose", TypedKind::String),
    field("eoa", TypedKind::Address),
    field("principal", TypedKind::Bytes),
    field("canister", TypedKind::Bytes),
    field("network", TypedKind::String),
    field("origin", TypedKind::String),
    field("nonce", TypedKind::Bytes32),
    field("expiresAt", TypedKind::Uint64),
];

/// 出金intentの型。
pub const WITHDRAWAL_PRIMARY_TYPE: &str = "PrivatePerpWithdrawal";
pub const WITHDRAWAL_FIELDS: &[TypedField] = &[
    field("userId", TypedKind::Bytes32),
    field("accountId", TypedKind::Bytes32),
    field("amount", TypedKind::Uint64),
    field("asset", TypedKind::String),
    field("destination", TypedKind::String),
    field("network", TypedKind::String),
    field("nonce", TypedKind::Uint64),
    field("expiresAt", TypedKind::Uint64),
    field("canister", TypedKind::Bytes),
];

/// 独自型のdomain。
pub const fn domain() -> Domain {
    Domain {
        name: DOMAIN_NAME,
        version: DOMAIN_VERSION,
        chain_id: DOMAIN_CHAIN_ID,
        verifying_contract: [0u8; 20],
    }
}

/// ログインchallenge。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub purpose: String,
    pub eoa: [u8; 20],
    pub principal: Vec<u8>,
    pub canister: Vec<u8>,
    pub network: String,
    pub origin: String,
    pub nonce: [u8; 32],
    pub expires_at: u64,
}

impl Challenge {
    fn values(&self) -> Vec<TypedValue> {
        vec![
            TypedValue::String(self.purpose.clone()),
            TypedValue::Address(self.eoa),
            TypedValue::Bytes(self.principal.clone()),
            TypedValue::Bytes(self.canister.clone()),
            TypedValue::String(self.network.clone()),
            TypedValue::String(self.origin.clone()),
            TypedValue::Bytes32(self.nonce),
            TypedValue::Uint64(self.expires_at),
        ]
    }

    /// 署名対象ダイジェスト。
    pub fn digest(&self) -> Result<[u8; 32], SignError> {
        digest_with_domain(
            &domain(),
            CHALLENGE_PRIMARY_TYPE,
            CHALLENGE_FIELDS,
            &self.values(),
        )
    }

    /// テスト用の署名（本番の署名はクライアント側のEOAが行う）。
    pub fn sign_for_tests(&self, secret_key: &[u8; 32]) -> Result<Signature, SignError> {
        sign_with_domain(
            &domain(),
            CHALLENGE_PRIMARY_TYPE,
            CHALLENGE_FIELDS,
            &self.values(),
            secret_key,
        )
    }
}

/// 出金intent。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withdrawal {
    pub user_id: [u8; 32],
    pub account_id: [u8; 32],
    pub amount: u64,
    pub asset: String,
    pub destination: String,
    pub network: String,
    pub nonce: u64,
    pub expires_at: u64,
    pub canister: Vec<u8>,
}

impl Withdrawal {
    fn values(&self) -> Vec<TypedValue> {
        vec![
            TypedValue::Bytes32(self.user_id),
            TypedValue::Bytes32(self.account_id),
            TypedValue::Uint64(self.amount),
            TypedValue::String(self.asset.clone()),
            TypedValue::String(self.destination.clone()),
            TypedValue::String(self.network.clone()),
            TypedValue::Uint64(self.nonce),
            TypedValue::Uint64(self.expires_at),
            TypedValue::Bytes(self.canister.clone()),
        ]
    }

    /// 署名対象ダイジェスト。
    pub fn digest(&self) -> Result<[u8; 32], SignError> {
        digest_with_domain(
            &domain(),
            WITHDRAWAL_PRIMARY_TYPE,
            WITHDRAWAL_FIELDS,
            &self.values(),
        )
    }

    /// テスト用の署名（本番の署名は本人のEOAが行う）。
    pub fn sign_for_tests(&self, secret_key: &[u8; 32]) -> Result<Signature, SignError> {
        sign_with_domain(
            &domain(),
            WITHDRAWAL_PRIMARY_TYPE,
            WITHDRAWAL_FIELDS,
            &self.values(),
            secret_key,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Challenge, WITHDRAWAL_FIELDS, Withdrawal, domain};
    use crate::keccak::keccak256;
    use crate::signature::{address_from_secret, recover_address};
    use crate::typed_data::type_string;

    fn secret(seed: u8) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[31] = seed;
        bytes
    }

    fn challenge() -> Challenge {
        Challenge {
            purpose: "login".to_string(),
            eoa: [1u8; 20],
            principal: vec![2u8; 29],
            canister: vec![3u8; 10],
            network: "testnet".to_string(),
            origin: "https://example.test".to_string(),
            nonce: [4u8; 32],
            expires_at: 1_758_000_000_000,
        }
    }

    fn withdrawal() -> Withdrawal {
        Withdrawal {
            user_id: [5u8; 32],
            account_id: [6u8; 32],
            amount: 1_000_000,
            asset: "usdc".to_string(),
            destination: "0x11".to_string(),
            network: "testnet".to_string(),
            nonce: 42,
            expires_at: 1_758_000_600_000,
            canister: vec![7u8; 10],
        }
    }

    #[test]
    fn type_strings_are_fixed() {
        assert_eq!(
            type_string(super::CHALLENGE_PRIMARY_TYPE, super::CHALLENGE_FIELDS),
            "PrivatePerpChallenge(string purpose,address eoa,bytes principal,bytes canister,string network,string origin,bytes32 nonce,uint64 expiresAt)"
        );
        assert_eq!(
            type_string(super::WITHDRAWAL_PRIMARY_TYPE, WITHDRAWAL_FIELDS),
            "PrivatePerpWithdrawal(bytes32 userId,bytes32 accountId,uint64 amount,string asset,string destination,string network,uint64 nonce,uint64 expiresAt,bytes canister)"
        );
    }

    #[test]
    fn domain_is_fixed() {
        let domain = domain();
        assert_eq!(domain.name, "private-perp");
        assert_eq!(domain.version, "1");
        assert_eq!(domain.chain_id, 1);
        assert_eq!(domain.verifying_contract, [0u8; 20]);
    }

    #[test]
    fn challenge_binds_every_field() {
        let base = challenge().digest().expect("digest");

        let mut other = challenge();
        other.purpose = "withdrawal".to_string();
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = challenge();
        other.network = "mainnet".to_string();
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = challenge();
        other.origin = "https://evil.test".to_string();
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = challenge();
        other.expires_at += 1;
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = challenge();
        other.principal = vec![9u8; 29];
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = challenge();
        other.canister = vec![9u8; 10];
        assert_ne!(base, other.digest().expect("digest"));
    }

    #[test]
    fn withdrawal_binds_amount_and_destination() {
        let base = withdrawal().digest().expect("digest");

        let mut other = withdrawal();
        other.amount += 1;
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = withdrawal();
        other.destination = "0x22".to_string();
        assert_ne!(base, other.digest().expect("digest"));

        let mut other = withdrawal();
        other.nonce += 1;
        assert_ne!(base, other.digest().expect("digest"));
    }

    #[test]
    fn signature_recovers_to_the_signer() {
        let secret_key = secret(41);
        let challenge = challenge();
        let digest = challenge.digest().expect("digest");
        let signature = challenge.sign_for_tests(&secret_key).expect("sign");
        assert_eq!(
            recover_address(&digest, &signature, None).expect("recover"),
            address_from_secret(&secret_key).expect("address")
        );

        let withdrawal = withdrawal();
        let digest = withdrawal.digest().expect("digest");
        let signature = withdrawal.sign_for_tests(&secret_key).expect("sign");
        assert_eq!(
            recover_address(&digest, &signature, None).expect("recover"),
            address_from_secret(&secret_key).expect("address")
        );
        assert_ne!(keccak256(b"x"), digest);
    }
}

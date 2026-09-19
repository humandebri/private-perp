//! 署名・ハッシュのエラー。

/// 署名・復元・正規化の失敗。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignError {
    #[error("invalid secp256k1 secret key")]
    InvalidSecretKey,
    #[error("invalid secp256k1 public key")]
    InvalidPublicKey,
    #[error("invalid signature encoding")]
    InvalidSignature,
    #[error("invalid recovery id")]
    InvalidRecoveryId,
    #[error("failed to recover the public key")]
    RecoveryFailed,
    #[error("recovered public key does not match the expected key")]
    PublicKeyMismatch,
    #[error("EIP-712 fields and values do not match")]
    TypedDataMismatch,
}

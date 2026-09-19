//! 署名と復元（secp256k1）。
//!
//! tECDSAの閾値署名応答には `v`（recovery id）が含まれないため、候補を試して
//! 公開鍵・アドレスが一致するものを選ぶ（`Implementation.md` 9.1の4）。
//! r/sのバイト一致は要求しない。

use crate::error::SignError;
use crate::keccak::keccak256;
use k256::ecdsa::{RecoveryId, Signature as K256Signature, SigningKey, VerifyingKey};

/// 65バイト形式（r || s || v）の署名。`v` は27/28。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub v: u8,
}

impl Signature {
    /// `r || s || v` の65バイト。
    pub fn to_bytes65(&self) -> [u8; 65] {
        let mut out = [0u8; 65];
        out[0..32].copy_from_slice(&self.r);
        out[32..64].copy_from_slice(&self.s);
        out[64] = self.v;
        out
    }

    /// 65バイトから復元する（`v` は27/28または0/1）。
    pub fn from_bytes65(bytes: &[u8; 65]) -> Result<Self, SignError> {
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&bytes[0..32]);
        s.copy_from_slice(&bytes[32..64]);
        let v = match bytes[64] {
            0 | 27 => 27,
            1 | 28 => 28,
            _ => return Err(SignError::InvalidRecoveryId),
        };
        Ok(Self { r, s, v })
    }

    fn recovery_id(&self) -> Result<RecoveryId, SignError> {
        let id = match self.v {
            27 => 0,
            28 => 1,
            _ => return Err(SignError::InvalidRecoveryId),
        };
        RecoveryId::try_from(id).map_err(|_| SignError::InvalidRecoveryId)
    }

    fn k256_signature(&self) -> Result<K256Signature, SignError> {
        let mut bytes = [0u8; 64];
        bytes[0..32].copy_from_slice(&self.r);
        bytes[32..64].copy_from_slice(&self.s);
        K256Signature::from_slice(&bytes).map_err(|_| SignError::InvalidSignature)
    }
}

/// 秘密鍵から圧縮公開鍵（33バイト）を導出する。
pub fn public_key_compressed(secret_key: &[u8; 32]) -> Result<[u8; 33], SignError> {
    use k256::elliptic_curve::sec1::ToSec1Point;

    let secret = secret_key_from_bytes(secret_key)?;
    let point = secret.public_key().to_sec1_point(true);
    let mut compressed = [0u8; 33];
    compressed.copy_from_slice(point.as_bytes());
    Ok(compressed)
}

/// 公開鍵（SEC1形式）からEthereumアドレス（20バイト）を導出する。
pub fn address_from_public_key(public_key: &[u8]) -> Result<[u8; 20], SignError> {
    let key = VerifyingKey::from_sec1_bytes(public_key).map_err(|_| SignError::InvalidPublicKey)?;
    let uncompressed = key.to_sec1_point(false);
    let bytes = uncompressed.as_bytes();
    if bytes.len() != 65 {
        return Err(SignError::InvalidPublicKey);
    }
    let hash = keccak256(&bytes[1..]);
    let mut address = [0u8; 20];
    address.copy_from_slice(&hash[12..]);
    Ok(address)
}

/// 秘密鍵からアドレスを導出する。
pub fn address_from_secret(secret_key: &[u8; 32]) -> Result<[u8; 20], SignError> {
    let compressed = public_key_compressed(secret_key)?;
    address_from_public_key(&compressed)
}

/// ダイジェストへ決定的に署名する（RFC 6979。テストとfixture比較に使う）。
pub fn sign_digest(digest: &[u8; 32], secret_key: &[u8; 32]) -> Result<Signature, SignError> {
    let signing_key = signing_key_from_bytes(secret_key)?;
    let (signature, recovery_id) = signing_key.sign_prehash_recoverable(digest);
    let bytes = signature.to_bytes();
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&bytes[0..32]);
    s.copy_from_slice(&bytes[32..64]);
    Ok(Signature {
        r,
        s,
        v: 27 + recovery_id.to_byte(),
    })
}

/// 署名からアドレスを復元する。`expected_public_key` を渡すと一致を検証する
/// （閾値署名で `v` が無い場合の候補選択に使う）。
pub fn recover_address(
    digest: &[u8; 32],
    signature: &Signature,
    expected_public_key: Option<&[u8; 33]>,
) -> Result<[u8; 20], SignError> {
    let recovered = recover_public_key(digest, signature)?;
    if let Some(expected) = expected_public_key
        && recovered != *expected
    {
        return Err(SignError::PublicKeyMismatch);
    }
    address_from_public_key(&recovered)
}

/// `r`・`s` と期待公開鍵から `v` を決定する（`Implementation.md` 9.1の4）。
///
/// 候補（27/28）を試し、公開鍵が一致するものを返す。一致が無ければ拒否する。
pub fn recover_v(
    digest: &[u8; 32],
    r: [u8; 32],
    s: [u8; 32],
    expected_public_key: &[u8; 33],
) -> Result<u8, SignError> {
    for v in [27u8, 28u8] {
        let candidate = Signature { r, s, v };
        if let Ok(public_key) = recover_public_key(digest, &candidate)
            && public_key == *expected_public_key
        {
            return Ok(v);
        }
    }
    Err(SignError::PublicKeyMismatch)
}

fn recover_public_key(digest: &[u8; 32], signature: &Signature) -> Result<[u8; 33], SignError> {
    let recovered = VerifyingKey::recover_from_prehash(
        digest,
        &signature.k256_signature()?,
        signature.recovery_id()?,
    )
    .map_err(|_| SignError::RecoveryFailed)?;
    let point = recovered.to_sec1_point(true);
    let mut compressed = [0u8; 33];
    compressed.copy_from_slice(point.as_bytes());
    Ok(compressed)
}

fn signing_key_from_bytes(secret_key: &[u8; 32]) -> Result<SigningKey, SignError> {
    SigningKey::from_bytes(secret_key.into()).map_err(|_| SignError::InvalidSecretKey)
}

fn secret_key_from_bytes(secret_key: &[u8; 32]) -> Result<k256::SecretKey, SignError> {
    k256::SecretKey::from_bytes(secret_key.into()).map_err(|_| SignError::InvalidSecretKey)
}

#[cfg(test)]
mod tests {
    use super::{
        address_from_public_key, address_from_secret, public_key_compressed, recover_address,
        recover_v, sign_digest,
    };
    use crate::error::SignError;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn secret(seed: u8) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[31] = seed;
        bytes
    }

    #[test]
    fn secp256k1_generator_public_key_is_stable() {
        assert_eq!(
            hex(&public_key_compressed(&secret(1)).expect("secret key 1")),
            "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        );
    }

    #[test]
    fn zero_secret_key_is_rejected() {
        assert_eq!(
            public_key_compressed(&[0u8; 32]),
            Err(SignError::InvalidSecretKey)
        );
    }

    #[test]
    fn signature_recovers_to_the_signer_address() {
        let secret_key = secret(7);
        let digest = crate::keccak::keccak256(b"private-perp");
        let signature = sign_digest(&digest, &secret_key).expect("sign");
        assert!(signature.v == 27 || signature.v == 28);

        let expected = address_from_secret(&secret_key).expect("address");
        let recovered = recover_address(&digest, &signature, None).expect("recover");
        assert_eq!(recovered, expected);

        let public_key = public_key_compressed(&secret_key).expect("public key");
        let with_expected =
            recover_address(&digest, &signature, Some(&public_key)).expect("recover with key");
        assert_eq!(with_expected, expected);
    }

    #[test]
    fn signature_is_deterministic() {
        let secret_key = secret(9);
        let digest = crate::keccak::keccak256(b"deterministic");
        assert_eq!(
            sign_digest(&digest, &secret_key).expect("sign"),
            sign_digest(&digest, &secret_key).expect("sign")
        );
    }

    #[test]
    fn recover_v_finds_the_recovery_id_without_v() {
        let secret_key = secret(11);
        let digest = crate::keccak::keccak256(b"threshold");
        let signature = sign_digest(&digest, &secret_key).expect("sign");
        let public_key = public_key_compressed(&secret_key).expect("public key");

        let v = recover_v(&digest, signature.r, signature.s, &public_key).expect("recover v");
        assert_eq!(v, signature.v);
    }

    #[test]
    fn recover_v_rejects_a_wrong_public_key() {
        let digest = crate::keccak::keccak256(b"mismatch");
        let signature = sign_digest(&digest, &secret(13)).expect("sign");
        let other_public_key = public_key_compressed(&secret(14)).expect("public key");
        assert_eq!(
            recover_v(&digest, signature.r, signature.s, &other_public_key),
            Err(SignError::PublicKeyMismatch)
        );
    }

    #[test]
    fn wrong_public_key_is_rejected_on_recovery() {
        let digest = crate::keccak::keccak256(b"guard");
        let signature = sign_digest(&digest, &secret(15)).expect("sign");
        let other_public_key = public_key_compressed(&secret(16)).expect("public key");
        assert_eq!(
            recover_address(&digest, &signature, Some(&other_public_key)),
            Err(SignError::PublicKeyMismatch)
        );
        // 公開鍵からアドレスを導く経路も一致する。
        assert!(address_from_public_key(&other_public_key).is_ok());
    }
}

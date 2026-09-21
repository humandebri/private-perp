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

    /// `s` を low-s 形（EIP-2）へ正規化する。反転した場合は `v` も反転させる。
    ///
    /// 閾値ECDSAの応答は `s > n/2` を取り得る。`recover_v` は候補を総当たりするため
    /// high-s でも公開鍵を復元できるが、Ethereum系の検証は high-s を拒否するため、
    /// **送信する署名**は正規形へ揃える。
    pub fn normalized_low_s(&self) -> Result<Self, SignError> {
        if self.v != 27 && self.v != 28 {
            return Err(SignError::InvalidRecoveryId);
        }
        let normalized = self.k256_signature()?.normalize_s();
        let bytes = normalized.to_bytes();
        if bytes[32..64] == self.s[..] {
            return Ok(*self);
        }
        let mut s = [0u8; 32];
        s.copy_from_slice(&bytes[32..64]);
        Ok(Self {
            r: self.r,
            s,
            v: if self.v == 27 { 28 } else { 27 },
        })
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

/// ダイジェストへ決定的に署名する（RFC 6979）。
///
/// **テスト専用。** 本番の署名は管理CanisterのtECDSA（`sign_with_ecdsa`）で行い、
/// canisterクレートからこの関数を呼ばない（`docs/phase-0/authority-matrix.md` 4節の
/// 「任意digest署名を設けない」不変条件）。`scripts/check-signing-boundary.sh` で検査する。
pub fn sign_digest_for_tests(
    digest: &[u8; 32],
    secret_key: &[u8; 32],
) -> Result<Signature, SignError> {
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
        Signature, address_from_public_key, address_from_secret, public_key_compressed,
        recover_address, recover_v, sign_digest_for_tests,
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

    /// secp256k1の位数 n。
    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xfe, 0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36,
        0x41, 0x41,
    ];

    /// `s` を `n - s` にする（high-s 版を作る）。
    fn negate_s(s: &[u8; 32]) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut borrow = 0i16;
        for index in (0..32).rev() {
            let diff = ORDER[index] as i16 - s[index] as i16 - borrow;
            if diff < 0 {
                out[index] = (diff + 256) as u8;
                borrow = 1;
            } else {
                out[index] = diff as u8;
                borrow = 0;
            }
        }
        out
    }

    /// high-s の署名を low-s へ正規化しても、同じアドレスが復元できる。
    ///
    /// 閾値ECDSAは high-s を返し得るが、Ethereum系の検証は high-s を拒否する。
    #[test]
    fn high_s_signatures_are_normalized_to_low_s() {
        let digest = [7u8; 32];
        let key = secret(21);
        let low = sign_digest_for_tests(&digest, &key).expect("sign");
        let public_key = public_key_compressed(&key).expect("public key");
        let address = address_from_secret(&key).expect("address");

        // low-s はそのまま（vも変わらない）。
        assert_eq!(low.normalized_low_s().expect("normalize"), low);

        // 同じ署名の high-s 版（s → n-s、v 反転）を作る。
        let high = Signature {
            r: low.r,
            s: negate_s(&low.s),
            v: if low.v == 27 { 28 } else { 27 },
        };
        assert_ne!(high, low);

        let normalized = high.normalized_low_s().expect("normalize");
        assert_eq!(normalized, low, "正規化でlow-s形へ戻る");
        assert_eq!(
            recover_address(&digest, &normalized, Some(&public_key)).expect("recover"),
            address,
            "正規化後も同じアドレスを復元する"
        );
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
        let signature = sign_digest_for_tests(&digest, &secret_key).expect("sign");
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
            sign_digest_for_tests(&digest, &secret_key).expect("sign"),
            sign_digest_for_tests(&digest, &secret_key).expect("sign")
        );
    }

    #[test]
    fn recover_v_finds_the_recovery_id_without_v() {
        let secret_key = secret(11);
        let digest = crate::keccak::keccak256(b"threshold");
        let signature = sign_digest_for_tests(&digest, &secret_key).expect("sign");
        let public_key = public_key_compressed(&secret_key).expect("public key");

        let v = recover_v(&digest, signature.r, signature.s, &public_key).expect("recover v");
        assert_eq!(v, signature.v);
    }

    #[test]
    fn recover_v_rejects_a_wrong_public_key() {
        let digest = crate::keccak::keccak256(b"mismatch");
        let signature = sign_digest_for_tests(&digest, &secret(13)).expect("sign");
        let other_public_key = public_key_compressed(&secret(14)).expect("public key");
        assert_eq!(
            recover_v(&digest, signature.r, signature.s, &other_public_key),
            Err(SignError::PublicKeyMismatch)
        );
    }

    #[test]
    fn wrong_public_key_is_rejected_on_recovery() {
        let digest = crate::keccak::keccak256(b"guard");
        let signature = sign_digest_for_tests(&digest, &secret(15)).expect("sign");
        let other_public_key = public_key_compressed(&secret(16)).expect("public key");
        assert_eq!(
            recover_address(&digest, &signature, Some(&other_public_key)),
            Err(SignError::PublicKeyMismatch)
        );
        // 公開鍵からアドレスを導く経路も一致する。
        assert!(address_from_public_key(&other_public_key).is_ok());
    }
}

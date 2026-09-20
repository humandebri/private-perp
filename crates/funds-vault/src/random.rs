//! 乱数。`SQLite` の `random()` は決定的なので使わず、`raw_rand` から取る。

use api_types::error::ErrorCode;
use ic_cdk_management_canister::raw_rand;

/// `raw_rand` から `length` バイトを得る。
pub async fn random_bytes(length: usize) -> Result<Vec<u8>, ErrorCode> {
    let bytes = raw_rand().await.map_err(|error| ErrorCode::Internal {
        code: format!("raw_rand failed: {error}"),
    })?;
    if bytes.len() < length {
        return Err(ErrorCode::Internal {
            code: "raw_rand returned fewer bytes than requested".to_string(),
        });
    }
    Ok(bytes[..length].to_vec())
}

/// 32バイトの乱数（IDやnonceに使う）。
pub async fn random32() -> Result<[u8; 32], ErrorCode> {
    let bytes = random_bytes(32).await?;
    bytes.try_into().map_err(|_| ErrorCode::Internal {
        code: "raw_rand length mismatch".to_string(),
    })
}

/// 16バイトの乱数（cloidに使う。後続の段階）。
#[allow(dead_code)]
pub async fn random16() -> Result<[u8; 16], ErrorCode> {
    let bytes = random_bytes(16).await?;
    bytes.try_into().map_err(|_| ErrorCode::Internal {
        code: "raw_rand length mismatch".to_string(),
    })
}

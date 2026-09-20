//! `trading_core` の受付の冪等性（`Implementation.md` 5.2）。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::params;

/// 受付の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptOutcome {
    Accepted,
    /// 同一ID・同一本文の再送。
    Duplicate,
    /// 同一ID・異なる本文。
    Conflict,
}

/// 受付を記録する（同一IDは本文hashで判定する）。
pub fn accept_request(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    client_request_id: &[u8],
    body_hash: &[u8; 32],
    now: u64,
) -> Result<AcceptOutcome, Error> {
    let existing = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT body_hash FROM requests WHERE user_id = ?1 AND client_request_id = ?2",
            params![user_id.as_slice(), client_request_id],
        )
        .map_err(sql)?;
    if let Some(existing) = existing {
        return Ok(if existing == body_hash.as_slice() {
            AcceptOutcome::Duplicate
        } else {
            AcceptOutcome::Conflict
        });
    }
    connection
        .execute(
            "INSERT INTO requests (user_id, client_request_id, body_hash, accepted_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                user_id.as_slice(),
                client_request_id,
                body_hash.as_slice(),
                now as i64
            ],
        )
        .map_err(sql)?;
    Ok(AcceptOutcome::Accepted)
}

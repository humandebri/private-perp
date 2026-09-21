//! 封筒の`request_id`の単回使用（`docs/phase-0/api-contract.md` 6節）。
//!
//! HPKEは再送防止の代わりではない。同じ`request_id`の2回目以降を拒否するため、
//! 消費したIDを記録する（期限切れの行は同じトランザクションで掃除する）。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::params;

/// `request_id`を消費する。既に使われていれば`false`（再送）。
pub fn consume(
    connection: &mut UpdateConnection<'_>,
    request_id: &[u8; 32],
    method: &str,
    caller: &[u8],
    received_at: u64,
    expires_at: u64,
) -> Result<bool, Error> {
    // 期限切れの記録は掃除する（期限切れの要求は別途期限で拒否される）。
    connection
        .execute(
            "DELETE FROM hpke_requests WHERE expires_at < ?1",
            params![received_at as i64],
        )
        .map_err(sql)?;
    let existing = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT request_id FROM hpke_requests WHERE request_id = ?1",
            params![request_id.as_slice()],
        )
        .map_err(sql)?;
    if existing.is_some() {
        return Ok(false);
    }
    connection
        .execute(
            "INSERT INTO hpke_requests (request_id, method, caller, received_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                request_id.as_slice(),
                method,
                caller,
                received_at as i64,
                expires_at as i64
            ],
        )
        .map_err(sql)?;
    Ok(true)
}

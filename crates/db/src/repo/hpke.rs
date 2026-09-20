//! HPKE鍵世代の永続化。秘密鍵は公開APIへ出さない。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 新しい世代を登録し、以前の世代を退役させる。
pub fn insert_key(
    connection: &mut UpdateConnection<'_>,
    secret: &[u8; 32],
    public: &[u8; 32],
    now: u64,
) -> Result<u64, Error> {
    connection
        .execute(
            "UPDATE hpke_keys SET retired_at = ?1 WHERE retired_at IS NULL",
            params![now as i64],
        )
        .map_err(sql)?;
    connection
        .execute(
            "INSERT INTO hpke_keys (generation, secret, public, created_at, retired_at)
             VALUES ((SELECT COALESCE(MAX(generation), 0) + 1 FROM hpke_keys), ?1, ?2, ?3, NULL)",
            params![secret.as_slice(), public.as_slice(), now as i64],
        )
        .map_err(sql)?;
    connection
        .query_scalar::<i64>("SELECT MAX(generation) FROM hpke_keys", params![])
        .map_err(sql)
        .and_then(|value| u64::try_from(value).map_err(|_| Error::Invariant("negative generation")))
}

/// 現行世代の公開鍵。
pub fn active_public(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT public FROM hpke_keys WHERE retired_at IS NULL ORDER BY generation DESC LIMIT 1",
            params![],
        )
        .map_err(sql)
}

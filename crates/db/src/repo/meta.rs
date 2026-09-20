//! `meta_cache`（銘柄の添字解決）。`docs/phase-0/money-and-units.md` 3節。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// `meta` の `universe` を保存する（network/dex単位で置き換える）。
pub fn set_universe(
    connection: &mut UpdateConnection<'_>,
    network: &str,
    dex: &str,
    digest: &[u8; 32],
    universe_json: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO meta_cache (network, dex, fetched_at, digest, universe)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(network, dex) DO UPDATE SET fetched_at = excluded.fetched_at, digest = excluded.digest, universe = excluded.universe",
            params![network, dex, now as i64, digest.as_slice(), universe_json],
        )
        .map_err(sql)
}

/// 保存済みの `universe` JSON。
pub fn universe_json(
    connection: &Connection,
    network: &str,
    dex: &str,
) -> Result<Option<String>, Error> {
    connection
        .query_optional_scalar::<String>(
            "SELECT universe FROM meta_cache WHERE network = ?1 AND dex = ?2",
            params![network, dex],
        )
        .map_err(sql)
}

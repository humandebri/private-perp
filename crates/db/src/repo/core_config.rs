//! `trading_core` のブートストラップ設定。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// vaultのprincipalを設定する（controllerのみ、初期化時）。
pub fn set_vault_principal(
    connection: &mut UpdateConnection<'_>,
    principal: &[u8],
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO core_config (singleton, vault_principal) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET vault_principal = excluded.vault_principal",
            params![principal],
        )
        .map_err(sql)
}

/// vaultのprincipal（未設定はNone。未設定では認可できない）。
pub fn vault_principal(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT vault_principal FROM core_config WHERE singleton = 1",
            params![],
        )
        .map_err(sql)
}

/// 銘柄解決に使うnetwork・dexを設定する（controllerのみ）。
pub fn set_market_context(
    connection: &mut UpdateConnection<'_>,
    network: &str,
    dex: &str,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO core_config (singleton, network, dex) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET network = excluded.network, dex = excluded.dex",
            params![network, dex],
        )
        .map_err(sql)
}

/// 銘柄解決に使うnetwork・dex（未設定はNone。未設定では銘柄を解決しない）。
pub fn market_context(connection: &Connection) -> Result<Option<(String, String)>, Error> {
    let row = connection
        .query_optional(
            "SELECT network, dex FROM core_config WHERE singleton = 1",
            params![],
            |row| Ok((row.get::<Option<String>>(0)?, row.get::<Option<String>>(1)?)),
        )
        .map_err(sql)?;
    Ok(row.and_then(|(network, dex)| match (network, dex) {
        (Some(network), Some(dex)) => Some((network, dex)),
        _ => None,
    }))
}

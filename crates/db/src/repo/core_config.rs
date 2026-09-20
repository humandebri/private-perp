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

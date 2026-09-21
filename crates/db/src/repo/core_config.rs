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

/// 環境設定（network・endpoint・key ID）の生の値。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentRow {
    pub network: Option<String>,
    pub exchange_url: Option<String>,
    pub info_url: Option<String>,
    pub ecdsa_key_id: Option<String>,
}

/// 環境設定を読む（networkは`core_config`、endpointとkey IDは`core_environment`）。
pub fn environment(connection: &Connection) -> Result<EnvironmentRow, Error> {
    // NULLを取り得る列は`Option<String>`で読む（固定型だとNULLで型エラーになる）。
    let network = connection
        .query_optional(
            "SELECT network FROM core_config WHERE singleton = 1",
            params![],
            |row| row.get::<Option<String>>(0),
        )
        .map_err(sql)?
        .flatten();
    let row = connection
        .query_optional(
            "SELECT exchange_url, info_url, ecdsa_key_id
               FROM core_environment WHERE singleton = 1",
            params![],
            |row| {
                Ok(EnvironmentRow {
                    network: None,
                    exchange_url: row.get::<Option<String>>(0)?,
                    info_url: row.get::<Option<String>>(1)?,
                    ecdsa_key_id: row.get::<Option<String>>(2)?,
                })
            },
        )
        .map_err(sql)?;
    Ok(EnvironmentRow {
        network,
        ..row.unwrap_or_default()
    })
}

/// HLのendpointを設定する（controllerのみ。検証は呼び出し側で行う）。
pub fn set_venue_endpoints(
    connection: &mut UpdateConnection<'_>,
    exchange_url: &str,
    info_url: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO core_environment (singleton, exchange_url, info_url, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
               exchange_url = excluded.exchange_url, info_url = excluded.info_url,
               updated_at = excluded.updated_at",
            params![exchange_url, info_url, now as i64],
        )
        .map_err(sql)
}

/// tECDSAのkey IDを設定する（controllerのみ。検証は呼び出し側で行う）。
pub fn set_ecdsa_key_id(
    connection: &mut UpdateConnection<'_>,
    key_id: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO core_environment (singleton, ecdsa_key_id, updated_at) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               ecdsa_key_id = excluded.ecdsa_key_id, updated_at = excluded.updated_at",
            params![key_id, now as i64],
        )
        .map_err(sql)
}

/// 政策Canisterのprincipalを設定する（controllerのみが呼ぶ）。
pub fn set_policy_principal(
    connection: &mut UpdateConnection<'_>,
    principal: &[u8],
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO core_config (singleton, policy_principal) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET policy_principal = excluded.policy_principal",
            params![principal],
        )
        .map_err(sql)
}

/// 政策Canisterのprincipal（未設定はNone）。
pub fn policy_principal(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    // NULLを取り得る列は`Option<T>`で読む（`Vec<u8>`固定だとNULLで型エラーになる）。
    let row = connection
        .query_optional(
            "SELECT policy_principal FROM core_config WHERE singleton = 1",
            params![],
            |row| row.get::<Option<Vec<u8>>>(0),
        )
        .map_err(sql)?;
    Ok(row.flatten())
}

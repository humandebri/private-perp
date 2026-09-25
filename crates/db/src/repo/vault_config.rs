//! `funds_vault` の環境設定（network・HL endpoint・tECDSA key ID）。
//!
//! `docs/phase-0/environments.md` 4節に従い、環境は起動時に検証可能な値で判定する。
//! 未設定はnetwork既定（local）として扱い、既定値の解決は呼び出し側（canister）が行う。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 環境設定の生の値。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentRow {
    pub network: Option<String>,
    pub exchange_url: Option<String>,
    pub info_url: Option<String>,
    pub ecdsa_key_id: Option<String>,
}

/// 環境設定を読む（未設定はすべて`None`）。
pub fn environment(connection: &Connection) -> Result<EnvironmentRow, Error> {
    let row = connection
        .query_optional(
            "SELECT network, exchange_url, info_url, ecdsa_key_id
               FROM vault_config WHERE singleton = 1",
            params![],
            |row| {
                Ok(EnvironmentRow {
                    network: row.get::<Option<String>>(0)?,
                    exchange_url: row.get::<Option<String>>(1)?,
                    info_url: row.get::<Option<String>>(2)?,
                    ecdsa_key_id: row.get::<Option<String>>(3)?,
                })
            },
        )
        .map_err(sql)?;
    Ok(row.unwrap_or_default())
}

/// networkを設定する（controllerのみ。検証は呼び出し側で行う）。
pub fn set_network(
    connection: &mut UpdateConnection<'_>,
    name: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO vault_config (singleton, network, updated_at) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               network = excluded.network, recovery_history_verified = 0,
               updated_at = excluded.updated_at",
            params![name, now as i64],
        )
        .map_err(sql)
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
            "INSERT INTO vault_config (singleton, exchange_url, info_url, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
               exchange_url = excluded.exchange_url, info_url = excluded.info_url,
               recovery_history_verified = 0, updated_at = excluded.updated_at",
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
            "INSERT INTO vault_config (singleton, ecdsa_key_id, updated_at) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               ecdsa_key_id = excluded.ecdsa_key_id, updated_at = excluded.updated_at",
            params![key_id, now as i64],
        )
        .map_err(sql)
}

/// 共有REST予算のpolicy。未設定ではHLへの送信を許可しない。
pub fn policy_principal(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    connection
        .query_optional(
            "SELECT policy_principal FROM vault_config WHERE singleton = 1",
            params![],
            |row| row.get::<Option<Vec<u8>>>(0),
        )
        .map(|value| value.flatten())
        .map_err(sql)
}

pub fn set_policy_principal(
    connection: &mut UpdateConnection<'_>,
    principal: &[u8],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO vault_config (singleton, policy_principal, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               policy_principal = excluded.policy_principal,
               updated_at = excluded.updated_at",
            params![principal, now as i64],
        )
        .map_err(sql)
}

pub fn core_principal(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    connection
        .query_optional(
            "SELECT core_principal FROM vault_config WHERE singleton = 1",
            params![],
            |row| row.get::<Option<Vec<u8>>>(0),
        )
        .map(|value| value.flatten())
        .map_err(sql)
}

pub fn set_core_principal(
    connection: &mut UpdateConnection<'_>,
    principal: &[u8],
    now: u64,
) -> Result<(), Error> {
    connection.execute(
        "INSERT INTO vault_config (singleton, core_principal, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET core_principal = excluded.core_principal, updated_at = excluded.updated_at",
        params![principal, now as i64],
    ).map_err(sql)?;
    Ok(())
}

pub fn recovery_history_verified(connection: &Connection) -> Result<bool, Error> {
    Ok(connection
        .query_optional_scalar::<i64>(
            "SELECT recovery_history_verified FROM vault_config WHERE singleton = 1",
            params![],
        )
        .map_err(sql)?
        .unwrap_or(0)
        != 0)
}

pub fn set_recovery_history_verified(
    connection: &mut UpdateConnection<'_>,
    verified: bool,
    now: u64,
) -> Result<(), Error> {
    connection.execute(
        "INSERT INTO vault_config (singleton, recovery_history_verified, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET recovery_history_verified = excluded.recovery_history_verified,
           updated_at = excluded.updated_at",
        params![if verified { 1 } else { 0 }, now as i64],
    ).map_err(sql)?;
    Ok(())
}

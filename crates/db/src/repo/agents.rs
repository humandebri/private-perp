//! Agent世代の永続化（`Implementation.md` 7章）。

use crate::error::Error;
use crate::repo::sql;
use api_types::fund::{AgentGeneration, AgentState};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

fn state_name(state: AgentState) -> &'static str {
    match state {
        AgentState::Requested => "requested",
        AgentState::Approving => "approving",
        AgentState::Active => "active",
        AgentState::Expiring => "expiring",
        AgentState::Revoked => "revoked",
        AgentState::Failed => "failed",
    }
}

fn state_from_name(value: &str) -> Option<AgentState> {
    Some(match value {
        "requested" => AgentState::Requested,
        "approving" => AgentState::Approving,
        "active" => AgentState::Active,
        "expiring" => AgentState::Expiring,
        "revoked" => AgentState::Revoked,
        "failed" => AgentState::Failed,
        _ => return None,
    })
}

type RawGeneration = (Vec<u8>, i64, Vec<u8>, Option<i64>, Option<i64>, String);

fn convert(raw: RawGeneration) -> Result<AgentGeneration, Error> {
    Ok(AgentGeneration {
        account_id: raw
            .0
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
        generation: u64::try_from(raw.1).map_err(|_| Error::Invariant("negative generation"))?,
        agent_address: raw
            .2
            .try_into()
            .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
        approved_at: raw
            .3
            .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("negative time")))
            .transpose()?,
        expires_at: raw
            .4
            .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("negative time")))
            .transpose()?,
        state: state_from_name(&raw.5).ok_or(Error::Invariant("unknown agent state"))?,
    })
}

fn read(row: &ic_sqlite_vfs::db::Row<'_>) -> Result<RawGeneration, ic_sqlite_vfs::DbError> {
    Ok((
        row.get::<Vec<u8>>(0)?,
        row.get::<i64>(1)?,
        row.get::<Vec<u8>>(2)?,
        row.get::<Option<i64>>(3)?,
        row.get::<Option<i64>>(4)?,
        row.get::<String>(5)?,
    ))
}

const COLUMNS: &str = "account_id, generation, agent_address, approved_at, expires_at, state";

/// 新しい世代を`requested`で登録する。
pub fn insert_generation(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    generation: u64,
    agent_address: &[u8; 20],
    derivation_path: &str,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO agent_generations
               (account_id, generation, agent_address, derivation_path, approved_at, expires_at, revoked_at, state, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5, ?6)",
            params![
                account_id.as_slice(),
                generation as i64,
                agent_address.as_slice(),
                derivation_path,
                state_name(AgentState::Requested),
                now as i64
            ],
        )
        .map_err(sql)
}

/// 最大の世代番号（未登録は0）。
pub fn latest_generation(connection: &Connection, account_id: &[u8; 32]) -> Result<u64, Error> {
    let value = connection
        .query_scalar::<i64>(
            "SELECT COALESCE(MAX(generation), 0) FROM agent_generations WHERE account_id = ?1",
            params![account_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(value).map_err(|_| Error::Invariant("negative generation"))
}

/// 世代を1件返す。
pub fn generation(
    connection: &Connection,
    account_id: &[u8; 32],
    generation: u64,
) -> Result<Option<AgentGeneration>, Error> {
    let raw = connection
        .query_optional(
            &format!(
                "SELECT {COLUMNS} FROM agent_generations WHERE account_id = ?1 AND generation = ?2"
            ),
            params![account_id.as_slice(), generation as i64],
            read,
        )
        .map_err(sql)?;
    raw.map(convert).transpose()
}

/// 最新世代を返す。
pub fn latest(
    connection: &Connection,
    account_id: &[u8; 32],
) -> Result<Option<AgentGeneration>, Error> {
    let raw = connection
        .query_optional(
            &format!("SELECT {COLUMNS} FROM agent_generations WHERE account_id = ?1 ORDER BY generation DESC LIMIT 1"),
            params![account_id.as_slice()],
            read,
        )
        .map_err(sql)?;
    raw.map(convert).transpose()
}

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
        // 長さはテーブルのCHECK制約で保証されている。
        account_id: raw.0.into(),
        generation: u64::try_from(raw.1).map_err(|_| Error::Invariant("negative generation"))?,
        agent_address: raw.2.into(),
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

/// 世代を`active`へ遷移させる（承認の送信が受理された後）。
pub fn mark_approving(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    generation: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE agent_generations SET state = 'approving'
         WHERE account_id = ?1 AND generation = ?2 AND state IN ('requested', 'failed')",
            params![account_id.as_slice(), generation as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "requested or definitively rejected agent",
        "agent changed",
    )
}

pub fn mark_active(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    generation: u64,
    approved_at: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE agent_generations SET state = 'active', approved_at = ?3
              WHERE account_id = ?1 AND generation = ?2 AND state = 'approving'",
            params![account_id.as_slice(), generation as i64, approved_at as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        return Err(Error::StateConflict {
            expected: "approving".to_string(),
            actual: "missing or already active".to_string(),
        });
    }
    Ok(())
}

/// 世代を`failed`へ遷移させる（取引所が承認を拒否した）。
///
/// 不明（送信した可能性がある）は `failed` にしない。同じアドレスでの再承認は
/// 取引所側で冪等なため、`requested` のまま再試行できる。
pub fn mark_failed(
    connection: &mut UpdateConnection<'_>,
    account_id: &[u8; 32],
    generation: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE agent_generations SET state = 'failed'
              WHERE account_id = ?1 AND generation = ?2 AND state IN ('requested', 'approving')",
            params![account_id.as_slice(), generation as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        return Err(Error::StateConflict {
            expected: "requested or approving".to_string(),
            actual: "missing or terminal".to_string(),
        });
    }
    Ok(())
}

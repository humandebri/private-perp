//! 資金actionの永続outbox（`Implementation.md` 14.2、`docs/phase-0/state-machines.md` 2節）。
//!
//! 署名前に `queued`、署名後に `signed`、送信直前に `dispatching` を永続化する。
//! すべての遷移は `worker_epoch` と現在状態のCASで行い、更新0件なら競合として拒否する。

use crate::error::Error;
use crate::repo::sql;
use crate::states::{action_state_from_str, action_state_str};
use api_types::fund::ActionState;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// outboxの1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundActionRow {
    pub action_id: [u8; 32],
    pub user_id: [u8; 32],
    pub client_request_id: Option<Vec<u8>>,
    pub kind: String,
    pub signer_id: String,
    pub canonical_action: Vec<u8>,
    pub digest: [u8; 32],
    pub nonce: u64,
    pub signature: Option<Vec<u8>>,
    pub wire_payload: Option<Vec<u8>>,
    pub dispatch_state: ActionState,
    pub worker_epoch: u64,
    pub attempt: u32,
}

/// 新規actionの内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFundAction {
    pub action_id: [u8; 32],
    pub user_id: [u8; 32],
    pub client_request_id: Option<Vec<u8>>,
    pub kind: String,
    pub signer_id: String,
    pub canonical_action: Vec<u8>,
    pub digest: [u8; 32],
    pub nonce: u64,
}

type RawAction = (
    Vec<u8>,
    Vec<u8>,
    Option<Vec<u8>>,
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    String,
    i64,
    i64,
);

const ACTION_COLUMNS: &str =
    "action_id, user_id, client_request_id, kind, signer_id, canonical_action,
     digest, nonce, signature, wire_payload, dispatch_state, worker_epoch, attempt";

fn convert_action(raw: RawAction) -> Result<FundActionRow, Error> {
    Ok(FundActionRow {
        action_id: raw
            .0
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte action id"))?,
        user_id: raw
            .1
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
        client_request_id: raw.2,
        kind: raw.3,
        signer_id: raw.4,
        canonical_action: raw.5,
        digest: raw
            .6
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte digest"))?,
        nonce: u64::try_from(raw.7).map_err(|_| Error::Invariant("negative nonce"))?,
        signature: raw.8,
        wire_payload: raw.9,
        dispatch_state: action_state_from_str(&raw.10)
            .ok_or(Error::Invariant("unknown dispatch state"))?,
        worker_epoch: u64::try_from(raw.11).map_err(|_| Error::Invariant("negative epoch"))?,
        attempt: u32::try_from(raw.12).map_err(|_| Error::Invariant("negative attempt"))?,
    })
}

fn read_action(row: &ic_sqlite_vfs::db::Row<'_>) -> Result<RawAction, ic_sqlite_vfs::DbError> {
    Ok((
        row.get::<Vec<u8>>(0)?,
        row.get::<Vec<u8>>(1)?,
        row.get::<Option<Vec<u8>>>(2)?,
        row.get::<String>(3)?,
        row.get::<String>(4)?,
        row.get::<Vec<u8>>(5)?,
        row.get::<Vec<u8>>(6)?,
        row.get::<i64>(7)?,
        row.get::<Option<Vec<u8>>>(8)?,
        row.get::<Option<Vec<u8>>>(9)?,
        row.get::<String>(10)?,
        row.get::<i64>(11)?,
        row.get::<i64>(12)?,
    ))
}

/// actionを `queued` で登録する。
pub fn insert_fund_action(
    connection: &mut UpdateConnection<'_>,
    action: &NewFundAction,
    now: u64,
) -> Result<(), Error> {
    let request_value = match action.client_request_id.as_deref() {
        Some(value) => ic_sqlite_vfs::db::Value::Blob(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let state = action_state_str(ActionState::Queued);
    connection
        .execute(
            "INSERT INTO fund_actions
               (action_id, user_id, client_request_id, kind, signer_id, canonical_action, digest, nonce,
                signature, wire_payload, dispatch_state, worker_epoch, lease_until, attempt, reason_code,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, ?9, 0, NULL, 0, NULL, ?10, ?10)",
            params![
                action.action_id.as_slice(),
                action.user_id.as_slice(),
                request_value,
                action.kind.as_str(),
                action.signer_id.as_str(),
                action.canonical_action.as_slice(),
                action.digest.as_slice(),
                action.nonce as i64,
                state,
                now as i64
            ],
        )
        .map_err(sql)?;
    record_action_event(
        connection,
        &action.action_id,
        None,
        ActionState::Queued,
        None,
        now,
    )
}

/// 署名者ごとのnonceを `max(now_ms, last + 1)` で確保する（同一トランザクション）。
pub fn allocate_master_nonce(
    connection: &mut UpdateConnection<'_>,
    signer_id: &str,
    now_ms: u64,
) -> Result<u64, Error> {
    let last = connection
        .query_optional_scalar::<i64>(
            "SELECT last_nonce FROM master_nonces WHERE signer_id = ?1",
            params![signer_id],
        )
        .map_err(sql)?;
    let now = i64::try_from(now_ms).map_err(|_| Error::Overflow)?;
    let next = match last {
        Some(previous) => previous.saturating_add(1).max(now),
        None => now,
    };
    connection
        .execute(
            "INSERT INTO master_nonces (signer_id, last_nonce) VALUES (?1, ?2)
             ON CONFLICT(signer_id) DO UPDATE SET last_nonce = excluded.last_nonce",
            params![signer_id, next],
        )
        .map_err(sql)?;
    u64::try_from(next).map_err(|_| Error::Overflow)
}

/// 未署名actionを1件確保する（epochを増やしてリースを取る）。
pub fn claim_action(
    connection: &mut UpdateConnection<'_>,
    now: u64,
    lease_ms: u64,
) -> Result<Option<FundActionRow>, Error> {
    let raw = connection
        .query_optional(
            &format!(
                "SELECT {ACTION_COLUMNS} FROM fund_actions
                  WHERE dispatch_state IN ('queued', 'signing')
                    AND (lease_until IS NULL OR lease_until < ?1)
                  ORDER BY updated_at, action_id LIMIT 1"
            ),
            params![now as i64],
            read_action,
        )
        .map_err(sql)?;

    let Some(raw) = raw else {
        return Ok(None);
    };
    let action = convert_action(raw)?;
    let lease_until = i64::try_from(now.saturating_add(lease_ms)).map_err(|_| Error::Overflow)?;

    connection
        .execute(
            "UPDATE fund_actions
                SET dispatch_state = 'signing', worker_epoch = worker_epoch + 1,
                    lease_until = ?2, attempt = attempt + 1, updated_at = ?3
              WHERE action_id = ?1 AND worker_epoch = ?4 AND dispatch_state = ?5",
            params![
                action.action_id.as_slice(),
                lease_until,
                now as i64,
                action.worker_epoch as i64,
                action_state_str(action.dispatch_state)
            ],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(
        changed,
        action_state_str(action.dispatch_state),
        "changed by another worker",
    )?;

    action_row(connection, &action.action_id)?
        .ok_or(Error::NotFound)
        .map(Some)
}

/// actionを読む。
pub fn action_row(
    connection: &Connection,
    action_id: &[u8; 32],
) -> Result<Option<FundActionRow>, Error> {
    let raw = connection
        .query_optional(
            &format!("SELECT {ACTION_COLUMNS} FROM fund_actions WHERE action_id = ?1"),
            params![action_id.as_slice()],
            read_action,
        )
        .map_err(sql)?;
    raw.map(convert_action).transpose()
}

/// 現在状態。
pub fn action_state(
    connection: &Connection,
    action_id: &[u8; 32],
) -> Result<Option<ActionState>, Error> {
    let state = connection
        .query_optional_scalar::<String>(
            "SELECT dispatch_state FROM fund_actions WHERE action_id = ?1",
            params![action_id.as_slice()],
        )
        .map_err(sql)?;
    state
        .map(|value| {
            action_state_from_str(&value).ok_or(Error::Invariant("unknown dispatch state"))
        })
        .transpose()
}

/// 署名済みとして保存する（payloadと署名はこの時点で必須）。
pub fn mark_signed(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    signature: &[u8],
    wire_payload: &[u8],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE fund_actions
                SET signature = ?4, wire_payload = ?5
              WHERE action_id = ?1 AND worker_epoch = ?2 AND dispatch_state = ?3",
            params![
                action_id.as_slice(),
                epoch as i64,
                action_state_str(ActionState::Signing),
                signature,
                wire_payload
            ],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "signing with matching epoch", "state changed")?;
    cas_transition(
        connection,
        action_id,
        epoch,
        ActionState::Signing,
        ActionState::Signed,
        None,
        now,
    )
}

/// 送信権を取得する（POST発行**前**に呼ぶ）。
pub fn mark_dispatching(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    cas_transition(
        connection,
        action_id,
        epoch,
        ActionState::Signed,
        ActionState::Dispatching,
        None,
        now,
    )
}

/// 照合済みにする。
pub fn mark_reconciled(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    cas_transition(
        connection,
        action_id,
        epoch,
        ActionState::Dispatching,
        ActionState::Reconciled,
        None,
        now,
    )
}

/// 結果不明にする（自動再送しない）。
pub fn mark_unknown(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    reason: &str,
    now: u64,
) -> Result<(), Error> {
    cas_transition(
        connection,
        action_id,
        epoch,
        ActionState::Dispatching,
        ActionState::Unknown,
        Some(reason),
        now,
    )
}

/// 未送信を保証して中止する。
pub fn abort_unsent(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    from: ActionState,
    reason: &str,
    now: u64,
) -> Result<(), Error> {
    if !from.is_pre_dispatch() {
        return Err(Error::Invariant("only unsent actions can be aborted"));
    }
    cas_transition(
        connection,
        action_id,
        epoch,
        from,
        ActionState::Aborted,
        Some(reason),
        now,
    )
}

fn cas_transition(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    from: ActionState,
    to: ActionState,
    reason: Option<&str>,
    now: u64,
) -> Result<(), Error> {
    let reason_value = match reason {
        Some(value) => ic_sqlite_vfs::db::Value::Text(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "UPDATE fund_actions
                SET dispatch_state = ?4, reason_code = ?5, updated_at = ?6, lease_until = NULL
              WHERE action_id = ?1 AND worker_epoch = ?2 AND dispatch_state = ?3",
            params![
                action_id.as_slice(),
                epoch as i64,
                action_state_str(from),
                action_state_str(to),
                reason_value,
                now as i64
            ],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        let actual = action_state(connection, action_id)?
            .map(|state| action_state_str(state).to_string())
            .unwrap_or_else(|| "missing".to_string());
        return Err(Error::StateConflict {
            expected: action_state_str(from).to_string(),
            actual,
        });
    }
    record_action_event(connection, action_id, Some(from), to, reason, now)
}

/// 状態遷移を記録する（平文のpayloadは入れない）。
pub fn record_action_event(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    from: Option<ActionState>,
    to: ActionState,
    reason: Option<&str>,
    now: u64,
) -> Result<(), Error> {
    let from_value = match from {
        Some(state) => ic_sqlite_vfs::db::Value::Text(action_state_str(state)),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let reason_value = match reason {
        Some(value) => ic_sqlite_vfs::db::Value::Text(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO action_events (action_id, from_state, to_state, reason_code, at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                action_id.as_slice(),
                from_value,
                action_state_str(to),
                reason_value,
                now as i64
            ],
        )
        .map_err(sql)
}

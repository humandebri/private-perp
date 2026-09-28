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
pub fn next_master_nonce(
    connection: &Connection,
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
        Some(previous) => previous.checked_add(1).ok_or(Error::Overflow)?.max(now),
        None => now,
    };
    u64::try_from(next).map_err(|_| Error::Overflow)
}

/// 署名者ごとのnonceを確保する。先行ジャーナルが予定値を記録した場合は、
/// 同じトランザクションで返値を照合してからactionを作る。
pub fn allocate_master_nonce(
    connection: &mut UpdateConnection<'_>,
    signer_id: &str,
    now_ms: u64,
) -> Result<u64, Error> {
    let next = next_master_nonce(connection, signer_id, now_ms)?;
    connection
        .execute(
            "INSERT INTO master_nonces (signer_id, last_nonce) VALUES (?1, ?2)
             ON CONFLICT(signer_id) DO UPDATE SET last_nonce = excluded.last_nonce",
            params![signer_id, i64::try_from(next).map_err(|_| Error::Overflow)?],
        )
        .map_err(sql)?;
    Ok(next)
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

pub fn recovery_lease_valid(
    connection: &Connection,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<bool, Error> {
    let valid = connection
        .query_optional_scalar::<i64>(
            "SELECT 1 FROM fund_actions WHERE action_id = ?1 AND worker_epoch = ?2
          AND dispatch_state = 'signing' AND kind = 'recovery' AND lease_until >= ?3",
            params![action_id.as_slice(), epoch as i64, now as i64],
        )
        .map_err(sql)?;
    Ok(valid.is_some())
}

/// 署名済みとして保存する（payloadと署名はこの時点で必須）。
///
/// 状態と署名を**同じUPDATE**で書く。`fund_actions` のCHECKは「`signing`では署名がNULL」
/// 「`signed`・`dispatching`以降は署名とpayloadが必須」を要求するため、2段階に分けると
/// 中間状態が制約違反になる。
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
                SET signature = ?4, wire_payload = ?5, dispatch_state = 'signed',
                    updated_at = ?6, lease_until = NULL
              WHERE action_id = ?1 AND worker_epoch = ?2 AND dispatch_state = ?3",
            params![
                action_id.as_slice(),
                epoch as i64,
                action_state_str(ActionState::Signing),
                signature,
                wire_payload,
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
            expected: action_state_str(ActionState::Signing).to_string(),
            actual,
        });
    }
    record_action_event(
        connection,
        action_id,
        Some(ActionState::Signing),
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

/// HL履歴によってunknownが確定した場合だけ遷移する。
pub fn reconcile_unknown(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    cas_transition(
        connection,
        action_id,
        epoch,
        ActionState::Unknown,
        ActionState::Reconciled,
        None,
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

/// 未解決（送信中・結果不明）のaction。出金可能額へ算入しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedActionRow {
    pub action_id: [u8; 32],
    pub kind: api_types::fund::FundActionKind,
    pub state: ActionState,
    pub since: u64,
}

/// 本人の未解決actionを古い順に返す。
pub fn unresolved_actions(
    connection: &Connection,
    user_id: &[u8; 32],
) -> Result<Vec<UnresolvedActionRow>, Error> {
    let raw = connection
        .query_all(
            "SELECT action_id, kind, dispatch_state, created_at
               FROM fund_actions
              WHERE user_id = ?1 AND dispatch_state IN ('dispatching', 'unknown')
              ORDER BY created_at",
            params![user_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<String>(1)?,
                    row.get::<String>(2)?,
                    row.get::<i64>(3)?,
                ))
            },
        )
        .map_err(sql)?;

    raw.into_iter()
        .map(|raw| {
            Ok(UnresolvedActionRow {
                action_id: raw
                    .0
                    .try_into()
                    .map_err(|_| Error::Invariant("expected a 32-byte action id"))?,
                kind: match raw.1.as_str() {
                    "recovery" => api_types::fund::FundActionKind::Recovery,
                    "withdrawal" => api_types::fund::FundActionKind::Withdrawal,
                    "agent_approval" => api_types::fund::FundActionKind::AgentApproval,
                    "agent_revocation" => api_types::fund::FundActionKind::AgentRevocation,
                    _ => api_types::fund::FundActionKind::Allocation,
                },
                state: action_state_from_str(&raw.2)
                    .ok_or(Error::Invariant("unknown dispatch state"))?,
                since: u64::try_from(raw.3).map_err(|_| Error::Invariant("negative timestamp"))?,
            })
        })
        .collect()
}

/// テスト専用：queuedなactionのダイジェストを上書きする（署名前の照合を検証するため）。
pub fn overwrite_queued_digest(
    connection: &mut UpdateConnection<'_>,
    digest: &[u8; 32],
) -> Result<u64, Error> {
    connection
        .execute(
            "UPDATE fund_actions SET digest = ?1 WHERE dispatch_state = 'queued'",
            params![digest.as_slice()],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    u64::try_from(changed).map_err(|_| Error::Invariant("negative change count"))
}

/// `action_id`から引く所有者情報（user_id・要求ID・epoch・種別）。
pub type ActionOwner = ([u8; 32], Option<Vec<u8>>, u64, String);

/// `action_id`から所有者・要求ID・epoch・種別を引く（不明actionの解消に使う）。
pub fn action_owner(
    connection: &Connection,
    action_id: &[u8; 32],
) -> Result<Option<ActionOwner>, Error> {
    let row = connection
        .query_optional(
            "SELECT user_id, client_request_id, worker_epoch, kind FROM fund_actions WHERE action_id = ?1",
            params![action_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Option<Vec<u8>>>(1)?,
                    row.get::<i64>(2)?,
                    row.get::<String>(3)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(user_id, request_id, epoch, kind)| {
        Ok((
            user_id
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
            request_id,
            u64::try_from(epoch).map_err(|_| Error::Invariant("negative epoch"))?,
            kind,
        ))
    })
    .transpose()
}

/// `unknown`のactionを解消済み（`reconciled`）へ遷移させる。
pub fn mark_resolved(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    cas_transition(
        connection,
        action_id,
        epoch,
        ActionState::Unknown,
        ActionState::Reconciled,
        None,
        now,
    )
}

/// 回収のcoreフェンス世代を送信前に保存する。
pub fn record_recovery_fence(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    worker_epoch: u64,
    fence_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE fund_actions SET recovery_fence_epoch = ?3, updated_at = ?4
         WHERE action_id = ?1 AND worker_epoch = ?2 AND dispatch_state = 'signing'
           AND kind = 'recovery' AND (recovery_fence_epoch IS NULL OR recovery_fence_epoch = ?3)",
            params![
                action_id.as_slice(),
                worker_epoch as i64,
                fence_epoch as i64,
                now as i64
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "signing recovery",
        "claim lost",
    )
}

pub fn recovery_fence_epoch(
    connection: &Connection,
    action_id: &[u8; 32],
) -> Result<Option<u64>, Error> {
    connection
        .query_optional(
            "SELECT recovery_fence_epoch FROM fund_actions WHERE action_id = ?1",
            params![action_id.as_slice()],
            |row| row.get::<Option<i64>>(0),
        )
        .map_err(sql)?
        .flatten()
        .map(|value| u64::try_from(value).map_err(|_| Error::Invariant("bad fence epoch")))
        .transpose()
}

#[derive(Debug, Clone)]
pub struct RecoveryRelease {
    pub action_id: [u8; 32],
    pub account_id: [u8; 32],
    pub user_id: [u8; 32],
    pub request_id: Vec<u8>,
    pub fence_epoch: u64,
    pub state: String,
}

pub fn recovery_release_candidates(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<RecoveryRelease>, Error> {
    let rows = connection
        .query_all(
            "SELECT a.action_id, r.account_id, a.user_id, a.client_request_id,
                a.recovery_fence_epoch, a.dispatch_state
           FROM fund_actions a JOIN fund_requests r
             ON r.user_id = a.user_id AND r.client_request_id = a.client_request_id
          WHERE a.kind = 'recovery' AND a.recovery_fence_epoch IS NOT NULL
            AND a.recovery_fence_released_at IS NULL
            AND ((a.dispatch_state = 'reconciled' AND r.state IN ('settled', 'rejected'))
              OR a.dispatch_state = 'aborted')
          ORDER BY a.updated_at, a.action_id LIMIT ?1",
            params![limit as i64],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<Vec<u8>>(3)?,
                    row.get::<i64>(4)?,
                    row.get::<String>(5)?,
                ))
            },
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|(action, account, user, request, epoch, state)| {
            Ok(RecoveryRelease {
                action_id: action
                    .try_into()
                    .map_err(|_| Error::Invariant("bad action id"))?,
                account_id: account
                    .try_into()
                    .map_err(|_| Error::Invariant("bad account id"))?,
                user_id: user
                    .try_into()
                    .map_err(|_| Error::Invariant("bad user id"))?,
                request_id: request,
                fence_epoch: u64::try_from(epoch)
                    .map_err(|_| Error::Invariant("bad fence epoch"))?,
                state,
            })
        })
        .collect()
}

pub fn mark_recovery_fence_released(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection.execute(
        "UPDATE fund_actions SET recovery_fence_released_at = ?3 WHERE action_id = ?1 AND recovery_fence_epoch = ?2 AND recovery_fence_released_at IS NULL",
        params![action_id.as_slice(), epoch as i64, now as i64],
    ).map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "fence release pending",
        "already released",
    )
}

#[derive(Debug, Clone)]
pub struct RecoveryCheck {
    pub kind: String,
    pub action_id: [u8; 32],
    pub account_id: [u8; 32],
    pub user_id: [u8; 32],
    pub request_id: Vec<u8>,
    pub amount: u64,
    pub destination: String,
    pub nonce: u64,
    pub worker_epoch: u64,
    pub fence_epoch: u64,
    pub checked_until: Option<u64>,
    pub window_ms: u64,
    pub action_state: String,
    pub match_hash: Option<[u8; 32]>,
    pub ambiguous: bool,
}

pub fn recovery_checks(connection: &Connection, limit: u32) -> Result<Vec<RecoveryCheck>, Error> {
    let rows = connection
        .query_all(
            "SELECT a.action_id, COALESCE(r.account_id, zeroblob(32)), a.user_id, a.client_request_id, r.amount,
                r.destination, a.nonce, a.worker_epoch, COALESCE(a.recovery_fence_epoch, 0),
                a.recovery_checked_until, a.recovery_window_ms, a.dispatch_state,
                a.recovery_match_hash, a.recovery_ambiguous, a.kind
           FROM fund_actions a JOIN fund_requests r
             ON r.user_id = a.user_id AND r.client_request_id = a.client_request_id
          WHERE ((a.kind = 'recovery' AND a.recovery_fence_epoch IS NOT NULL) OR (a.kind IN ('allocation','withdrawal') AND a.dispatch_state = 'unknown'))
            AND a.recovery_ambiguous = 0
            AND r.state IN ('unknown', 'executing')
            AND a.dispatch_state IN ('unknown', 'reconciled')
          ORDER BY a.updated_at, a.action_id LIMIT ?1",
            params![limit as i64],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<Vec<u8>>(3)?,
                    row.get::<i64>(4)?,
                    row.get::<String>(5)?,
                    row.get::<i64>(6)?,
                    row.get::<i64>(7)?,
                    row.get::<i64>(8)?,
                    row.get::<Option<i64>>(9)?,
                    row.get::<i64>(10)?,
                    row.get::<String>(11)?,
                    row.get::<Option<Vec<u8>>>(12)?,
                    row.get::<i64>(13)?,
                    row.get::<String>(14)?,
                ))
            },
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|row| {
            Ok(RecoveryCheck {
                kind: row.14,
                action_id: row
                    .0
                    .try_into()
                    .map_err(|_| Error::Invariant("bad action id"))?,
                account_id: row
                    .1
                    .try_into()
                    .map_err(|_| Error::Invariant("bad account id"))?,
                user_id: row
                    .2
                    .try_into()
                    .map_err(|_| Error::Invariant("bad user id"))?,
                request_id: row.3,
                amount: u64::try_from(row.4).map_err(|_| Error::Invariant("bad amount"))?,
                destination: row.5,
                nonce: u64::try_from(row.6).map_err(|_| Error::Invariant("bad nonce"))?,
                worker_epoch: u64::try_from(row.7)
                    .map_err(|_| Error::Invariant("bad worker epoch"))?,
                fence_epoch: u64::try_from(row.8)
                    .map_err(|_| Error::Invariant("bad fence epoch"))?,
                checked_until: row
                    .9
                    .map(|v| u64::try_from(v).map_err(|_| Error::Invariant("bad cursor")))
                    .transpose()?,
                window_ms: u64::try_from(row.10).map_err(|_| Error::Invariant("bad window"))?,
                action_state: row.11,
                match_hash: row
                    .12
                    .map(|hash| {
                        hash.try_into()
                            .map_err(|_| Error::Invariant("bad recovery hash"))
                    })
                    .transpose()?,
                ambiguous: row.13 != 0,
            })
        })
        .collect()
}

pub fn mark_recovery_ambiguous(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection.execute(
        "UPDATE fund_actions SET recovery_ambiguous = 1, updated_at = ?3
          WHERE action_id = ?1 AND worker_epoch = ?2 AND dispatch_state IN ('unknown', 'reconciled')",
        params![action_id.as_slice(), epoch as i64, now as i64],
    ).map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "recovery check",
        "stale callback",
    )
}

pub fn defer_recovery_check(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE fund_actions SET updated_at = MAX(updated_at + 1, ?3)
              WHERE action_id = ?1 AND worker_epoch = ?2
          AND dispatch_state IN ('unknown', 'reconciled')",
            params![action_id.as_slice(), epoch as i64, now as i64],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "recovery check",
        "stale callback",
    )
}

pub fn advance_recovery_check(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    checked_until: u64,
    window_ms: u64,
    now: u64,
) -> Result<(), Error> {
    connection.execute(
        "UPDATE fund_actions SET recovery_checked_until = ?3, recovery_window_ms = ?4, updated_at = ?5
          WHERE action_id = ?1 AND worker_epoch = ?2 AND kind IN ('recovery','allocation','withdrawal')
            AND dispatch_state IN ('unknown', 'reconciled')
            AND (recovery_checked_until IS NULL OR recovery_checked_until <= ?3)",
        params![action_id.as_slice(), epoch as i64, checked_until as i64, window_ms as i64, now as i64],
    ).map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "recovery check",
        "stale callback",
    )
}

pub fn set_recovery_match_hash(
    connection: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    epoch: u64,
    hash: &[u8; 32],
) -> Result<(), Error> {
    connection.execute(
        "UPDATE fund_actions SET recovery_match_hash = ?3 WHERE action_id = ?1 AND worker_epoch = ?2
          AND recovery_match_hash IS NULL AND dispatch_state IN ('unknown', 'reconciled')",
        params![action_id.as_slice(), epoch as i64, hash.as_slice()],
    ).map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "unmatched recovery",
        "already matched",
    )
}

/// 本人の回収フェンス表示。未送信の準備と、送信後の照合を区別する。
pub fn recovery_fence_status(
    connection: &Connection,
    user_id: &[u8; 32],
) -> Result<Option<String>, Error> {
    connection.query_optional_scalar::<String>(
        "SELECT CASE WHEN r.state IN ('unknown', 'executing') THEN 'reconciling' ELSE 'preparing' END
           FROM fund_actions a JOIN fund_requests r
             ON r.user_id = a.user_id AND r.client_request_id = a.client_request_id
          WHERE a.user_id = ?1 AND a.kind = 'recovery'
            AND a.recovery_fence_released_at IS NULL
            AND (r.state IN ('reserved', 'unknown', 'executing') OR a.recovery_fence_epoch IS NOT NULL)
          ORDER BY CASE WHEN r.state IN ('unknown', 'executing') THEN 0 ELSE 1 END,
                   a.created_at DESC LIMIT 1",
        params![user_id.as_slice()],
    ).map_err(sql)
}

pub fn active_recovery_exists(connection: &Connection) -> Result<bool, Error> {
    Ok(connection
        .query_scalar::<i64>(
            "SELECT EXISTS(
            SELECT 1 FROM fund_actions a JOIN fund_requests r
              ON r.user_id = a.user_id AND r.client_request_id = a.client_request_id
            WHERE a.kind = 'recovery' AND (
              r.state IN ('reserved', 'unknown', 'executing') OR
              (a.recovery_fence_epoch IS NOT NULL AND a.recovery_fence_released_at IS NULL)
            ))",
            &[],
        )
        .map_err(sql)?
        != 0)
}

#[derive(Debug, Clone)]
pub struct RecoveryProof {
    pub worker_epoch: u64,
    pub checked_until: Option<u64>,
    pub match_hash: Option<[u8; 32]>,
    pub ambiguous: bool,
}

pub fn recovery_proof(
    connection: &Connection,
    action_id: &[u8; 32],
) -> Result<Option<RecoveryProof>, Error> {
    let row = connection
        .query_optional(
            "SELECT worker_epoch, recovery_checked_until, recovery_match_hash, recovery_ambiguous
           FROM fund_actions WHERE action_id = ?1 AND kind IN ('recovery','allocation','withdrawal')
             AND dispatch_state IN ('unknown', 'reconciled')",
            params![action_id.as_slice()],
            |row| {
                Ok((
                    row.get::<i64>(0)?,
                    row.get::<Option<i64>>(1)?,
                    row.get::<Option<Vec<u8>>>(2)?,
                    row.get::<i64>(3)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(epoch, cursor, hash, ambiguous)| {
        Ok(RecoveryProof {
            worker_epoch: u64::try_from(epoch)
                .map_err(|_| Error::Invariant("bad recovery epoch"))?,
            checked_until: cursor
                .map(|value| {
                    u64::try_from(value).map_err(|_| Error::Invariant("bad recovery cursor"))
                })
                .transpose()?,
            match_hash: hash
                .map(|value| {
                    value
                        .try_into()
                        .map_err(|_| Error::Invariant("bad recovery hash"))
                })
                .transpose()?,
            ambiguous: ambiguous != 0,
        })
    })
    .transpose()
}

#[derive(Debug, Clone)]
pub struct LegacyRecovery {
    pub action_id: [u8; 32],
    pub account_id: [u8; 32],
    pub user_id: [u8; 32],
    pub request_id: Vec<u8>,
    pub worker_epoch: u64,
    pub dispatch_state: String,
}

pub fn legacy_recoveries(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<LegacyRecovery>, Error> {
    let rows = connection.query_all(
        "SELECT a.action_id, r.account_id, a.user_id, a.client_request_id, a.worker_epoch, a.dispatch_state
           FROM fund_actions a JOIN fund_requests r
             ON r.user_id = a.user_id AND r.client_request_id = a.client_request_id
          WHERE a.kind = 'recovery' AND a.recovery_fence_epoch IS NULL
            AND a.dispatch_state IN ('dispatching', 'unknown', 'reconciled')
            AND r.state IN ('reserved', 'unknown', 'executing')
          ORDER BY a.updated_at, a.action_id LIMIT ?1",
        params![limit as i64],
        |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Vec<u8>>(2)?, row.get::<Vec<u8>>(3)?, row.get::<i64>(4)?, row.get::<String>(5)?)),
    ).map_err(sql)?;
    rows.into_iter()
        .map(|row| {
            Ok(LegacyRecovery {
                action_id: row
                    .0
                    .try_into()
                    .map_err(|_| Error::Invariant("bad legacy action"))?,
                account_id: row
                    .1
                    .try_into()
                    .map_err(|_| Error::Invariant("bad legacy account"))?,
                user_id: row
                    .2
                    .try_into()
                    .map_err(|_| Error::Invariant("bad legacy user"))?,
                request_id: row.3,
                worker_epoch: u64::try_from(row.4)
                    .map_err(|_| Error::Invariant("bad legacy epoch"))?,
                dispatch_state: row.5,
            })
        })
        .collect()
}

pub fn record_legacy_recovery_fence(
    connection: &mut UpdateConnection<'_>,
    action: &LegacyRecovery,
    fence_epoch: u64,
    now: u64,
) -> Result<(), Error> {
    if action.dispatch_state == "dispatching" {
        mark_unknown(
            connection,
            &action.action_id,
            action.worker_epoch,
            "upgrade_result_unknown",
            now,
        )?;
        crate::repo::funds::set_request_state(
            connection,
            &action.user_id,
            &action.request_id,
            api_types::fund::FundRequestState::Unknown,
            now,
        )?;
    }
    connection
        .execute(
            "UPDATE fund_actions SET recovery_fence_epoch = ?3, updated_at = ?4
          WHERE action_id = ?1 AND worker_epoch = ?2 AND kind = 'recovery'
            AND recovery_fence_epoch IS NULL AND dispatch_state IN ('unknown', 'reconciled')",
            params![
                action.action_id.as_slice(),
                action.worker_epoch as i64,
                fence_epoch as i64,
                now as i64
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "legacy recovery",
        "stale legacy recovery",
    )
}

pub fn save_transfer_result(
    c: &mut UpdateConnection<'_>,
    action_id: &[u8; 32],
    event: &[u8],
) -> Result<(), Error> {
    c.execute("INSERT INTO pending_transfer_results(action_id,event) VALUES (?1,?2) ON CONFLICT(action_id) DO NOTHING", params![action_id.as_slice(),event]).map_err(sql)?;
    Ok(())
}
pub fn pending_transfer_results(c: &Connection) -> Result<Vec<Vec<u8>>, Error> {
    c.query_all(
        "SELECT event FROM pending_transfer_results ORDER BY rowid LIMIT 4",
        params![],
        |r| r.get(0),
    )
    .map_err(sql)
}
pub fn delete_transfer_result(c: &mut UpdateConnection<'_>, action_id: &[u8]) -> Result<(), Error> {
    c.execute(
        "DELETE FROM pending_transfer_results WHERE action_id=?1",
        params![action_id],
    )
    .map_err(sql)?;
    Ok(())
}

pub fn transfer_has_competitor(
    c: &Connection,
    row: &RecoveryCheck,
    window: u64,
) -> Result<bool, Error> {
    Ok(c.query_optional_scalar::<i64>("SELECT 1 FROM fund_actions a JOIN fund_requests r ON r.user_id=a.user_id AND r.client_request_id=a.client_request_id WHERE a.action_id != ?1 AND a.signer_id = (SELECT signer_id FROM fund_actions WHERE action_id=?1) AND (a.kind != 'recovery' OR r.account_id=?6) AND r.state != 'rejected' AND r.destination=?2 AND r.amount=?3 AND a.nonce BETWEEN ?4 AND ?5 AND a.dispatch_state IN ('dispatching','unknown','reconciled') LIMIT 1",params![row.action_id.as_slice(),row.destination.as_str(),row.amount as i64,row.nonce.saturating_sub(window) as i64,row.nonce.saturating_add(window) as i64,row.account_id.as_slice()]).map_err(sql)?.is_some())
}

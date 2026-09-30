use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

pub fn set_principal(c: &mut UpdateConnection<'_>, principal: &[u8]) -> Result<(), Error> {
    let prior = c
        .query_optional_scalar::<Vec<u8>>(
            "SELECT principal FROM send_journal_client WHERE singleton = 1",
            params![],
        )
        .map_err(sql)?;
    match prior {
        Some(prior) if prior == principal => Ok(()),
        Some(_) => Err(Error::Conflict),
        None => c
            .execute(
                "INSERT INTO send_journal_client(singleton, principal, locked) VALUES(1, ?1, 0)",
                params![principal],
            )
            .map_err(sql),
    }
}

pub fn principal(c: &Connection) -> Result<Option<Vec<u8>>, Error> {
    c.query_optional_scalar::<Vec<u8>>(
        "SELECT principal FROM send_journal_client WHERE singleton = 1",
        params![],
    )
    .map_err(sql)
}

pub fn set_guard(c: &mut UpdateConnection<'_>, guard: &[u8]) -> Result<(), Error> {
    let previous = guard_principal(c)?;
    if previous.is_some() && previous.as_deref() != Some(guard) {
        return Err(Error::Conflict);
    }
    c.execute(
        "UPDATE send_journal_client SET guard = ?1 WHERE singleton = 1",
        params![guard],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(c)?,
        "journal configured",
        "journal unconfigured",
    )
}

pub fn guard_principal(c: &Connection) -> Result<Option<Vec<u8>>, Error> {
    c.query_optional_scalar::<Vec<u8>>(
        "SELECT guard FROM send_journal_client WHERE singleton = 1 AND guard IS NOT NULL",
        params![],
    )
    .map_err(sql)
}

pub fn locked(c: &Connection) -> Result<bool, Error> {
    Ok(c.query_optional_scalar::<i64>(
        "SELECT locked FROM send_journal_client WHERE singleton = 1",
        params![],
    )
    .map_err(sql)?
    .unwrap_or(1)
        != 0)
}

pub fn replay_pending_validation(c: &Connection) -> Result<bool, Error> {
    Ok(c.query_optional_scalar::<i64>(
        "SELECT replay_pending_validation FROM send_journal_client WHERE singleton = 1",
        params![],
    )
    .map_err(sql)?
    .unwrap_or(1)
        != 0)
}

pub fn mark_replay_pending_validation(c: &mut UpdateConnection<'_>) -> Result<(), Error> {
    c.execute(
        "UPDATE send_journal_client SET replay_pending_validation = 1, locked = 1 WHERE singleton = 1",
        params![],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(c)?,
        "journal configured",
        "journal unconfigured",
    )
}

pub fn set_locked(c: &mut UpdateConnection<'_>, locked: bool) -> Result<(), Error> {
    c.execute(
        "UPDATE send_journal_client SET locked = ?1 WHERE singleton = 1",
        params![i64::from(locked)],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(c)?,
        "journal configured",
        "journal unconfigured",
    )
}

pub fn claim_writer(
    c: &mut UpdateConnection<'_>,
    kind: &str,
    request_id: &[u8; 32],
) -> Result<u64, Error> {
    if kind.is_empty() || kind.len() > 32 || !kind.is_ascii() {
        return Err(Error::Invariant("invalid journal writer kind"));
    }
    c.execute(
        "UPDATE send_journal_client
         SET writer_epoch = writer_epoch + 1, writer_kind = ?1, writer_request_id = ?2
         WHERE singleton = 1 AND locked = 0 AND writer_request_id IS NULL
           AND writer_epoch < 9223372036854775807",
        params![kind, request_id.as_slice()],
    )
    .map_err(sql)?;
    if crate::cas::changes(c)? != 1 {
        // Classify contention in the same transaction as the failed CAS.
        // A locked or unconfigured journal remains PolicyUnavailable.
        let active = c
            .query_optional_scalar::<i64>(
                "SELECT 1 FROM send_journal_client WHERE singleton = 1
                 AND locked = 0 AND writer_request_id IS NOT NULL
                 AND writer_epoch < 9223372036854775807",
                params![],
            )
            .map_err(sql)?;
        if active.is_some() {
            return Err(Error::WriterBusy);
        }
        return Err(Error::Conflict);
    }
    let epoch = c
        .query_optional_scalar::<i64>(
            "SELECT writer_epoch FROM send_journal_client WHERE singleton = 1",
            params![],
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    u64::try_from(epoch).map_err(|_| Error::Overflow)
}

pub fn writer_matches(
    c: &Connection,
    epoch: u64,
    kind: &str,
    request_id: &[u8; 32],
) -> Result<bool, Error> {
    let epoch = i64::try_from(epoch).map_err(|_| Error::Overflow)?;
    Ok(c.query_optional_scalar::<i64>(
        "SELECT 1 FROM send_journal_client WHERE singleton = 1
         AND writer_epoch = ?1 AND writer_kind = ?2 AND writer_request_id = ?3",
        params![epoch, kind, request_id.as_slice()],
    )
    .map_err(sql)?
    .is_some())
}

pub fn release_writer(
    c: &mut UpdateConnection<'_>,
    epoch: u64,
    kind: &str,
    request_id: &[u8; 32],
) -> Result<(), Error> {
    c.execute(
        "UPDATE send_journal_client SET writer_kind = NULL, writer_request_id = NULL
         WHERE singleton = 1 AND writer_epoch = ?1
           AND writer_kind = ?2 AND writer_request_id = ?3",
        params![
            i64::try_from(epoch).map_err(|_| Error::Overflow)?,
            kind,
            request_id.as_slice()
        ],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(c)?,
        "journal writer owned",
        "journal writer stale",
    )
}

/// Called only after the guard verified both remote heads and staged state.
/// A later callback cannot use its old epoch once another append claims it.
pub fn begin_resume(c: &mut UpdateConnection<'_>) -> Result<u64, Error> {
    c.execute(
        "UPDATE send_journal_client SET locked = 1, writer_epoch = writer_epoch + 1
        WHERE singleton = 1 AND writer_epoch < 9223372036854775807",
        params![],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(crate::cas::changes(c)?, "resume epoch", "unavailable")?;
    let epoch: i64 = c
        .query_scalar(
            "SELECT writer_epoch FROM send_journal_client WHERE singleton = 1",
            &[],
        )
        .map_err(sql)?;
    u64::try_from(epoch).map_err(|_| Error::Overflow)
}

pub fn unlock_after_resume(c: &mut UpdateConnection<'_>, epoch: u64) -> Result<(), Error> {
    c.execute(
        "UPDATE send_journal_client
         SET locked = 0, writer_kind = NULL, writer_request_id = NULL
         WHERE singleton = 1 AND replay_pending_validation = 0 AND locked = 1 AND writer_epoch = ?1",
        params![i64::try_from(epoch).map_err(|_| Error::Overflow)?],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(c)?,
        "journal configured",
        "journal unconfigured",
    )
}

pub fn local_head(c: &Connection) -> Result<(u64, [u8; 32], bool), Error> {
    let (count, max_seq) = c
        .query_optional(
            "SELECT COUNT(*), COALESCE(MAX(sequence), 0) FROM send_journal_receipts",
            params![],
            |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?)),
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let hash = if max_seq == 0 {
        [0; 32]
    } else {
        let value = c
            .query_optional_scalar::<Vec<u8>>(
                "SELECT hash FROM send_journal_receipts WHERE sequence = ?1",
                params![max_seq],
            )
            .map_err(sql)?
            .ok_or(Error::NotFound)?;
        value
            .try_into()
            .map_err(|_| Error::Invariant("bad receipt hash"))?
    };
    Ok((
        u64::try_from(max_seq).map_err(|_| Error::Overflow)?,
        hash,
        count == max_seq,
    ))
}

/// 永続ステージの末尾。空ならローカル受領済み末尾を基準点にする。
pub fn stage_head(c: &Connection) -> Result<(u64, [u8; 32]), Error> {
    let (receipt_seq, receipt_hash, contiguous) = local_head(c)?;
    if !contiguous {
        return Err(Error::Invariant("receipt gap"));
    }
    let count = c
        .query_optional_scalar::<i64>("SELECT COUNT(*) FROM send_journal_stage", params![])
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let row = c
        .query_optional(
            "SELECT sequence, hash FROM send_journal_stage ORDER BY sequence DESC LIMIT 1",
            params![],
            |r| Ok((r.get::<i64>(0)?, r.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    match row {
        Some((sequence, hash)) => {
            let sequence = u64::try_from(sequence).map_err(|_| Error::Overflow)?;
            if sequence.checked_sub(receipt_seq)
                != Some(u64::try_from(count).map_err(|_| Error::Overflow)?)
            {
                return Err(Error::Invariant("journal stage gap"));
            }
            let first = receipt_seq.checked_add(1).ok_or(Error::Overflow)?;
            let first_previous = c
                .query_optional_scalar::<Vec<u8>>(
                    "SELECT previous_hash FROM send_journal_stage WHERE sequence = ?1",
                    params![i64::try_from(first).map_err(|_| Error::Overflow)?],
                )
                .map_err(sql)?
                .ok_or(Error::Invariant("journal stage gap"))?;
            if first_previous.as_slice() != receipt_hash {
                return Err(Error::Invariant("journal stage predecessor mismatch"));
            }
            Ok((
                sequence,
                hash.try_into()
                    .map_err(|_| Error::Invariant("bad staged hash"))?,
            ))
        }
        None if count == 0 => Ok((receipt_seq, receipt_hash)),
        None => Err(Error::Invariant("journal stage gap")),
    }
}

pub fn has_staged(c: &Connection) -> Result<bool, Error> {
    Ok(
        c.query_optional_scalar::<i64>("SELECT 1 FROM send_journal_stage LIMIT 1", params![])
            .map_err(sql)?
            .is_some(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsentCandidate {
    pub writer_epoch: u64,
    pub sequence: u64,
    pub kind: String,
    pub action_id: [u8; 32],
    pub account_id: [u8; 32],
    pub nonce: u64,
    pub digest: [u8; 32],
    pub hash: [u8; 32],
}

/// Only the outstanding writer's one staged record can be resolved here.
/// Cancellation evidence must still be obtained from the independent journal.
pub fn unsent_candidate(c: &Connection) -> Result<Option<UnsentCandidate>, Error> {
    let row = c.query_optional(
        "SELECT w.writer_epoch, s.sequence, s.kind, s.request_id, s.account_id, s.nonce, s.digest, s.hash
         FROM send_journal_client w JOIN send_journal_stage s
           ON s.kind = w.writer_kind AND s.request_id = w.writer_request_id
         JOIN fund_actions a ON a.action_id = s.request_id
         JOIN fund_requests f ON f.user_id = a.user_id AND f.client_request_id = a.client_request_id
         WHERE w.singleton = 1 AND w.locked = 1 AND w.replay_pending_validation = 0
           AND s.kind IN ('allocation', 'withdrawal', 'recovery')
           AND a.kind = s.kind AND a.nonce = s.nonce AND a.digest = s.digest
           AND a.dispatch_state IN ('queued','signing') AND a.signature IS NULL AND a.wire_payload IS NULL
           AND f.kind = a.kind AND f.state = 'reserved'
           AND (SELECT COUNT(*) FROM send_journal_stage) = 1",
        params![], |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?, r.get::<String>(2)?,
            r.get::<Vec<u8>>(3)?, r.get::<Vec<u8>>(4)?, r.get::<i64>(5)?,
            r.get::<Vec<u8>>(6)?, r.get::<Vec<u8>>(7)?)),
    ).map_err(sql)?;
    row.map(
        |(epoch, sequence, kind, id, account, nonce, digest, hash)| {
            Ok(UnsentCandidate {
                writer_epoch: u64::try_from(epoch).map_err(|_| Error::Overflow)?,
                sequence: u64::try_from(sequence).map_err(|_| Error::Overflow)?,
                kind,
                action_id: id
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged action"))?,
                account_id: account
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged account"))?,
                nonce: u64::try_from(nonce).map_err(|_| Error::Overflow)?,
                digest: digest
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged digest"))?,
                hash: hash
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged hash"))?,
            })
        },
    )
    .transpose()
}

/// Called only with durable cancellation evidence, inside one local transaction.
pub fn finish_cancelled_send(
    c: &mut UpdateConnection<'_>,
    expected: &UnsentCandidate,
    now: u64,
) -> Result<(), Error> {
    if unsent_candidate(c)?.as_ref() != Some(expected) {
        return Err(Error::Conflict);
    }
    let (local, _, contiguous) = local_head(c)?;
    if !contiguous || local.checked_add(1) != Some(expected.sequence) {
        return Err(Error::Conflict);
    }
    let action =
        crate::repo::actions::action_row(c, &expected.action_id)?.ok_or(Error::NotFound)?;
    let request_id = action.client_request_id.as_deref().ok_or(Error::NotFound)?;
    let request =
        crate::repo::funds::fund_request(c, &action.user_id, request_id)?.ok_or(Error::NotFound)?;
    crate::repo::actions::abort_unsent(
        c,
        &action.action_id,
        action.worker_epoch,
        action.dispatch_state,
        "journal_send_cancelled",
        now,
    )?;
    if expected.kind == "withdrawal" {
        crate::repo::ledger::withdrawal_release(
            c,
            &action.user_id,
            request.amount,
            now,
            request_id,
        )?;
    }
    crate::repo::funds::release_reservation(c, &action.user_id, request_id, now)?;
    crate::repo::funds::set_request_state(
        c,
        &action.user_id,
        request_id,
        api_types::fund::FundRequestState::Rejected,
        now,
    )?;
    // Receipt acknowledges the cancelled intent, not a successful exchange POST.
    record(
        c,
        expected.sequence,
        &expected.kind,
        &expected.action_id,
        &expected.account_id,
        expected.nonce,
        &expected.digest,
        &expected.hash,
    )?;
    c.execute(
        "DELETE FROM send_journal_stage WHERE sequence = ?1",
        params![i64::try_from(expected.sequence).map_err(|_| Error::Overflow)?],
    )
    .map_err(sql)?;
    release_writer(
        c,
        expected.writer_epoch,
        &expected.kind,
        &expected.action_id,
    )
}

pub fn recovery_local_head(c: &Connection) -> Result<(u64, [u8; 32], bool), Error> {
    let (count, max_seq) = c
        .query_optional(
            "SELECT COUNT(*), COALESCE(MAX(sequence), 0) FROM recovery_event_receipts",
            params![],
            |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?)),
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let hash = if max_seq == 0 {
        [0; 32]
    } else {
        c.query_optional_scalar::<Vec<u8>>(
            "SELECT hash FROM recovery_event_receipts WHERE sequence = ?1",
            params![max_seq],
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?
        .try_into()
        .map_err(|_| Error::Invariant("bad recovery receipt hash"))?
    };
    Ok((
        u64::try_from(max_seq).map_err(|_| Error::Overflow)?,
        hash,
        count == max_seq,
    ))
}

pub fn recovery_stage_head(c: &Connection) -> Result<(u64, [u8; 32]), Error> {
    let (receipt_seq, receipt_hash, contiguous) = recovery_local_head(c)?;
    if !contiguous {
        return Err(Error::Invariant("recovery receipt gap"));
    }
    let count = c
        .query_optional_scalar::<i64>("SELECT COUNT(*) FROM recovery_event_stage", params![])
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let row = c
        .query_optional(
            "SELECT sequence, hash FROM recovery_event_stage ORDER BY sequence DESC LIMIT 1",
            params![],
            |r| Ok((r.get::<i64>(0)?, r.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    match row {
        Some((sequence, hash)) => {
            let sequence = u64::try_from(sequence).map_err(|_| Error::Overflow)?;
            if sequence.checked_sub(receipt_seq)
                != Some(u64::try_from(count).map_err(|_| Error::Overflow)?)
            {
                return Err(Error::Invariant("recovery stage gap"));
            }
            let first_previous = c
                .query_optional_scalar::<Vec<u8>>(
                    "SELECT previous_hash FROM recovery_event_stage WHERE sequence = ?1",
                    params![
                        i64::try_from(receipt_seq.checked_add(1).ok_or(Error::Overflow)?)
                            .map_err(|_| Error::Overflow)?
                    ],
                )
                .map_err(sql)?
                .ok_or(Error::Invariant("recovery stage gap"))?;
            if first_previous.as_slice() != receipt_hash {
                return Err(Error::Invariant("recovery stage predecessor mismatch"));
            }
            Ok((
                sequence,
                hash.try_into()
                    .map_err(|_| Error::Invariant("bad recovery stage hash"))?,
            ))
        }
        None if count == 0 => Ok((receipt_seq, receipt_hash)),
        None => Err(Error::Invariant("recovery stage gap")),
    }
}

pub fn has_recovery_staged(c: &Connection) -> Result<bool, Error> {
    Ok(
        c.query_optional_scalar::<i64>("SELECT 1 FROM recovery_event_stage LIMIT 1", params![])
            .map_err(sql)?
            .is_some(),
    )
}

pub struct StagedRecoveryEvent {
    pub sequence: u64,
    pub logical_id: [u8; 32],
    pub version: u16,
    pub payload: Vec<u8>,
    pub previous_hash: [u8; 32],
    pub hash: [u8; 32],
}

/// Return only the next unapplied event. Replaying a later record would hide a gap.
pub fn next_recovery_event(c: &Connection) -> Result<Option<StagedRecoveryEvent>, Error> {
    let (head, _, contiguous) = recovery_local_head(c)?;
    if !contiguous {
        return Err(Error::Invariant("recovery receipt gap"));
    }
    let next = head.checked_add(1).ok_or(Error::Overflow)?;
    let raw = c
        .query_optional(
            "SELECT sequence, logical_id, version, payload, previous_hash, hash
         FROM recovery_event_stage WHERE sequence = ?1",
            params![i64::try_from(next).map_err(|_| Error::Overflow)?],
            |r| {
                Ok((
                    r.get::<i64>(0)?,
                    r.get::<Vec<u8>>(1)?,
                    r.get::<i64>(2)?,
                    r.get::<Vec<u8>>(3)?,
                    r.get::<Vec<u8>>(4)?,
                    r.get::<Vec<u8>>(5)?,
                ))
            },
        )
        .map_err(sql)?;
    raw.map(
        |(sequence, logical_id, version, payload, previous_hash, hash)| {
            Ok(StagedRecoveryEvent {
                sequence: u64::try_from(sequence).map_err(|_| Error::Overflow)?,
                logical_id: logical_id
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged logical id"))?,
                version: u16::try_from(version).map_err(|_| Error::Overflow)?,
                payload,
                previous_hash: previous_hash
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged predecessor"))?,
                hash: hash
                    .try_into()
                    .map_err(|_| Error::Invariant("bad staged hash"))?,
            })
        },
    )
    .transpose()
}

pub fn delete_replayed_event(
    c: &mut UpdateConnection<'_>,
    event: &StagedRecoveryEvent,
) -> Result<(), Error> {
    c.execute(
        "DELETE FROM recovery_event_stage WHERE sequence = ?1 AND logical_id = ?2 AND hash = ?3",
        params![
            i64::try_from(event.sequence).map_err(|_| Error::Overflow)?,
            event.logical_id.as_slice(),
            event.hash.as_slice()
        ],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(c)?,
        "staged event",
        "staged event missing",
    )
}

pub fn record_recovery_event(
    c: &mut UpdateConnection<'_>,
    sequence: u64,
    logical_id: &[u8; 32],
    version: u16,
    payload: &[u8],
    hash: &[u8; 32],
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO recovery_event_receipts(sequence, logical_id, version, payload, hash)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            i64::try_from(sequence).map_err(|_| Error::Overflow)?,
            logical_id.as_slice(),
            i64::from(version),
            payload,
            hash.as_slice()
        ],
    )
    .map_err(sql)
}

#[allow(clippy::too_many_arguments)]
pub fn stage_recovery_event(
    c: &mut UpdateConnection<'_>,
    sequence: u64,
    logical_id: &[u8; 32],
    version: u16,
    payload: &[u8],
    previous_hash: &[u8; 32],
    hash: &[u8; 32],
) -> Result<(), Error> {
    let (last, prior) = recovery_stage_head(c)?;
    if last.checked_add(1) != Some(sequence) || prior != *previous_hash {
        return Err(Error::Conflict);
    }
    c.execute(
        "INSERT INTO recovery_event_stage(sequence, logical_id, version, payload, previous_hash, hash)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![i64::try_from(sequence).map_err(|_| Error::Overflow)?, logical_id.as_slice(),
            i64::from(version), payload, previous_hash.as_slice(), hash.as_slice()],
    ).map_err(sql)
}

#[allow(clippy::too_many_arguments)]
pub fn stage_record(
    c: &mut UpdateConnection<'_>,
    sequence: u64,
    kind: &str,
    request_id: &[u8; 32],
    account_id: &[u8; 32],
    nonce: u64,
    digest: &[u8; 32],
    previous_hash: &[u8; 32],
    hash: &[u8; 32],
) -> Result<(), Error> {
    let (last, prior) = stage_head(c)?;
    if last.checked_add(1) != Some(sequence) || prior != *previous_hash {
        return Err(Error::Conflict);
    }
    c.execute(
        "INSERT INTO send_journal_stage(sequence, kind, request_id, account_id, nonce, digest, previous_hash, hash)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![i64::try_from(sequence).map_err(|_| Error::Overflow)?, kind,
            request_id.as_slice(), account_id.as_slice(), i64::try_from(nonce).map_err(|_| Error::Overflow)?,
            digest.as_slice(), previous_hash.as_slice(), hash.as_slice()],
    ).map_err(sql)
}

/// A restored pre-journal snapshot may contain an unresolved POST even when both
/// journal heads are zero. Never interpret that matching zero as permission to send.
pub fn unresolved_without_receipt(c: &Connection, role: &str) -> Result<bool, Error> {
    let count: i64 = match role {
        "vault" => c.query_optional_scalar(
            "SELECT COUNT(*) FROM fund_actions a
             WHERE a.dispatch_state IN ('dispatching', 'unknown')
               AND NOT EXISTS (SELECT 1 FROM send_journal_receipts j
                 WHERE j.request_id = a.action_id AND j.kind = a.kind)", params![]
        ).map_err(sql)?.unwrap_or(0),
        "core" => c.query_optional_scalar(
            "SELECT COUNT(*) FROM orders o WHERE
               (o.dispatch_state IN ('dispatching', 'unknown') AND NOT EXISTS
                 (SELECT 1 FROM send_journal_receipts j WHERE j.request_id = o.order_id AND j.kind = 'order'))
               OR (o.preflight_state IN ('dispatching', 'unknown') AND NOT EXISTS
                 (SELECT 1 FROM send_journal_receipts j WHERE j.request_id = o.order_id AND j.kind = 'leverage'))
               OR (o.cancel_dispatch_state IN ('dispatching', 'unknown', 'sent') AND NOT EXISTS
                 (SELECT 1 FROM send_journal_receipts j WHERE j.request_id = o.order_id AND j.kind = 'cancel'))",
            params![]
        ).map_err(sql)?.unwrap_or(0),
        _ => return Err(Error::Invariant("invalid journal worker role")),
    };
    Ok(count != 0)
}

pub fn receipt_matches(
    c: &Connection,
    kind: &str,
    request_id: &[u8; 32],
    account_id: &[u8; 32],
    nonce: u64,
    digest: &[u8; 32],
) -> Result<bool, Error> {
    let nonce = i64::try_from(nonce).map_err(|_| Error::Overflow)?;
    Ok(c.query_optional_scalar::<i64>(
        "SELECT 1 FROM send_journal_receipts
         WHERE kind = ?1 AND request_id = ?2 AND account_id = ?3
           AND nonce = ?4 AND digest = ?5 LIMIT 1",
        params![
            kind,
            request_id.as_slice(),
            account_id.as_slice(),
            nonce,
            digest.as_slice()
        ],
    )
    .map_err(sql)?
    .is_some())
}

#[allow(clippy::too_many_arguments)]
pub fn record(
    c: &mut UpdateConnection<'_>,
    sequence: u64,
    kind: &str,
    request_id: &[u8; 32],
    account_id: &[u8; 32],
    nonce: u64,
    digest: &[u8; 32],
    hash: &[u8; 32],
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO send_journal_receipts(sequence, kind, request_id, account_id, nonce, digest, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![i64::try_from(sequence).map_err(|_| Error::Overflow)?, kind,
            request_id.as_slice(), account_id.as_slice(), i64::try_from(nonce).map_err(|_| Error::Overflow)?,
            digest.as_slice(), hash.as_slice()],
    ).map_err(sql)
}

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

pub struct StoredRecord {
    pub sequence: u64,
    pub kind: String,
    pub request_id: [u8; 32],
    pub account_id: [u8; 32],
    pub nonce: u64,
    pub digest: [u8; 32],
    pub previous_hash: [u8; 32],
    pub hash: [u8; 32],
}

pub struct ExistingRecord {
    pub sequence: u64,
    pub hash: [u8; 32],
    pub account_id: [u8; 32],
    pub nonce: u64,
    pub digest: [u8; 32],
}

/// No legacy record is backfilled: absence cannot establish that a send was prevented.
pub fn prepare_send(
    c: &mut UpdateConnection<'_>,
    worker: &[u8],
    kind: &str,
    id: &[u8; 32],
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO send_permits(worker, kind, request_id, state) VALUES (?1, ?2, ?3, 'prepared')",
        params![worker, kind, id.as_slice()],
    )
    .map_err(sql)
}

pub fn send_state(
    c: &Connection,
    worker: &[u8],
    kind: &str,
    id: &[u8; 32],
) -> Result<Option<String>, Error> {
    c.query_optional_scalar(
        "SELECT state FROM send_permits WHERE worker=?1 AND kind=?2 AND request_id=?3",
        params![worker, kind, id.as_slice()],
    )
    .map_err(sql)
}

/// Authorization is single-use; a lost reply never produces a second permission.
pub fn authorize_send(
    c: &mut UpdateConnection<'_>,
    worker: &[u8],
    kind: &str,
    id: &[u8; 32],
) -> Result<bool, Error> {
    c.execute("UPDATE send_permits SET state='authorized' WHERE worker=?1 AND kind=?2 AND request_id=?3 AND state='prepared'",
        params![worker, kind, id.as_slice()]).map_err(sql)?;
    Ok(crate::cas::changes(c)? == 1)
}

/// True is durable evidence that authorization was never granted and is now impossible.
pub fn cancel_send(
    c: &mut UpdateConnection<'_>,
    worker: &[u8],
    kind: &str,
    id: &[u8; 32],
) -> Result<bool, Error> {
    c.execute("UPDATE send_permits SET state='cancelled' WHERE worker=?1 AND kind=?2 AND request_id=?3 AND state='prepared'",
        params![worker, kind, id.as_slice()]).map_err(sql)?;
    Ok(send_state(c, worker, kind, id)?.as_deref() == Some("cancelled"))
}

pub fn records(
    c: &Connection,
    worker: &[u8],
    after: u64,
    limit: u32,
) -> Result<Vec<StoredRecord>, Error> {
    let rows = c
        .query_all(
            "SELECT sequence, kind, request_id, account_id, nonce, digest, previous_hash, hash
         FROM journal_records WHERE worker = ?1 AND sequence > ?2
         ORDER BY sequence LIMIT ?3",
            params![
                worker,
                i64::try_from(after).map_err(|_| Error::Overflow)?,
                i64::from(limit)
            ],
            |r| {
                Ok((
                    r.get::<i64>(0)?,
                    r.get::<String>(1)?,
                    r.get::<Vec<u8>>(2)?,
                    r.get::<Vec<u8>>(3)?,
                    r.get::<i64>(4)?,
                    r.get::<Vec<u8>>(5)?,
                    r.get::<Vec<u8>>(6)?,
                    r.get::<Vec<u8>>(7)?,
                ))
            },
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|row| {
            let (sequence, kind, request_id, account_id, nonce, digest, previous_hash, hash) = row;
            Ok(StoredRecord {
                sequence: u64::try_from(sequence).map_err(|_| Error::Overflow)?,
                kind,
                request_id: request_id
                    .try_into()
                    .map_err(|_| Error::Invariant("bad request id"))?,
                account_id: account_id
                    .try_into()
                    .map_err(|_| Error::Invariant("bad account id"))?,
                nonce: u64::try_from(nonce).map_err(|_| Error::Overflow)?,
                digest: digest
                    .try_into()
                    .map_err(|_| Error::Invariant("bad digest"))?,
                previous_hash: previous_hash
                    .try_into()
                    .map_err(|_| Error::Invariant("bad previous hash"))?,
                hash: hash.try_into().map_err(|_| Error::Invariant("bad hash"))?,
            })
        })
        .collect()
}

pub fn register_worker(
    c: &mut UpdateConnection<'_>,
    role: &str,
    principal: &[u8],
) -> Result<(), Error> {
    let old = c
        .query_optional_scalar::<Vec<u8>>(
            "SELECT principal FROM journal_workers WHERE role = ?1",
            params![role],
        )
        .map_err(sql)?;
    match old {
        Some(old) if old == principal => Ok(()),
        Some(_) => Err(Error::Conflict),
        None => c
            .execute(
                "INSERT INTO journal_workers(role, principal) VALUES (?1, ?2)",
                params![role, principal],
            )
            .map_err(sql),
    }
}

pub fn authorized(c: &Connection, worker: &[u8]) -> Result<bool, Error> {
    Ok(c.query_optional_scalar::<String>(
        "SELECT role FROM journal_workers WHERE principal = ?1",
        params![worker],
    )
    .map_err(sql)?
    .is_some())
}

pub fn head(c: &Connection, worker: &[u8]) -> Result<(u64, [u8; 32]), Error> {
    let row = c
        .query_optional(
            "SELECT sequence, hash FROM journal_heads WHERE worker = ?1",
            params![worker],
            |r| Ok((r.get::<i64>(0)?, r.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    match row {
        Some((sequence, hash)) => Ok((
            u64::try_from(sequence).map_err(|_| Error::Overflow)?,
            hash.try_into()
                .map_err(|_| Error::Invariant("bad journal hash"))?,
        )),
        None => Ok((0, [0; 32])),
    }
}

pub fn existing(
    c: &Connection,
    worker: &[u8],
    kind: &str,
    request_id: &[u8; 32],
) -> Result<Option<ExistingRecord>, Error> {
    let row = c.query_optional(
        "SELECT sequence, hash, account_id, nonce, digest FROM journal_records WHERE worker = ?1 AND kind = ?2 AND request_id = ?3",
        params![worker, kind, request_id.as_slice()],
        |r| Ok((r.get::<i64>(0)?, r.get::<Vec<u8>>(1)?, r.get::<Vec<u8>>(2)?, r.get::<i64>(3)?, r.get::<Vec<u8>>(4)?)),
    ).map_err(sql)?;
    row.map(|(sequence, hash, account, nonce, digest)| {
        Ok(ExistingRecord {
            sequence: u64::try_from(sequence).map_err(|_| Error::Overflow)?,
            hash: hash
                .try_into()
                .map_err(|_| Error::Invariant("bad journal hash"))?,
            account_id: account
                .try_into()
                .map_err(|_| Error::Invariant("bad journal account"))?,
            nonce: u64::try_from(nonce).map_err(|_| Error::Overflow)?,
            digest: digest
                .try_into()
                .map_err(|_| Error::Invariant("bad journal digest"))?,
        })
    })
    .transpose()
}

#[allow(clippy::too_many_arguments)]
pub fn append(
    c: &mut UpdateConnection<'_>,
    worker: &[u8],
    sequence: u64,
    kind: &str,
    request_id: &[u8; 32],
    account_id: &[u8; 32],
    nonce: u64,
    digest: &[u8; 32],
    previous_hash: &[u8; 32],
    hash: &[u8; 32],
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO journal_records(worker, sequence, kind, request_id, account_id, nonce, digest, previous_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![worker, i64::try_from(sequence).map_err(|_| Error::Overflow)?, kind,
            request_id.as_slice(), account_id.as_slice(), i64::try_from(nonce).map_err(|_| Error::Overflow)?,
            digest.as_slice(), previous_hash.as_slice(), hash.as_slice()],
    ).map_err(sql)?;
    c.execute(
        "INSERT INTO journal_heads(worker, sequence, hash) VALUES (?1, ?2, ?3)
         ON CONFLICT(worker) DO UPDATE SET sequence = excluded.sequence, hash = excluded.hash",
        params![
            worker,
            i64::try_from(sequence).map_err(|_| Error::Overflow)?,
            hash.as_slice()
        ],
    )
    .map_err(sql)
}

pub struct StoredRecoveryEvent {
    pub sequence: u64,
    pub logical_id: [u8; 32],
    pub version: u16,
    pub payload: Vec<u8>,
    pub previous_hash: [u8; 32],
    pub hash: [u8; 32],
}

pub fn recovery_head(c: &Connection, worker: &[u8]) -> Result<(u64, [u8; 32]), Error> {
    let row = c
        .query_optional(
            "SELECT sequence, hash FROM recovery_event_heads WHERE worker = ?1",
            params![worker],
            |r| Ok((r.get::<i64>(0)?, r.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    match row {
        Some((sequence, hash)) => Ok((
            u64::try_from(sequence).map_err(|_| Error::Overflow)?,
            hash.try_into()
                .map_err(|_| Error::Invariant("bad recovery head"))?,
        )),
        None => Ok((0, [0; 32])),
    }
}

pub fn recovery_event(
    c: &Connection,
    worker: &[u8],
    logical_id: &[u8; 32],
) -> Result<Option<StoredRecoveryEvent>, Error> {
    let row = c
        .query_optional(
            "SELECT sequence, logical_id, version, payload, previous_hash, hash
         FROM recovery_event_records WHERE worker = ?1 AND logical_id = ?2",
            params![worker, logical_id.as_slice()],
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
    row.map(decode_recovery_row).transpose()
}

fn decode_recovery_row(
    row: (i64, Vec<u8>, i64, Vec<u8>, Vec<u8>, Vec<u8>),
) -> Result<StoredRecoveryEvent, Error> {
    Ok(StoredRecoveryEvent {
        sequence: u64::try_from(row.0).map_err(|_| Error::Overflow)?,
        logical_id: row
            .1
            .try_into()
            .map_err(|_| Error::Invariant("bad logical id"))?,
        version: u16::try_from(row.2).map_err(|_| Error::Overflow)?,
        payload: row.3,
        previous_hash: row
            .4
            .try_into()
            .map_err(|_| Error::Invariant("bad previous hash"))?,
        hash: row
            .5
            .try_into()
            .map_err(|_| Error::Invariant("bad recovery hash"))?,
    })
}

pub fn recovery_events(
    c: &Connection,
    worker: &[u8],
    after: u64,
    limit: u32,
) -> Result<Vec<StoredRecoveryEvent>, Error> {
    let rows = c
        .query_all(
            "SELECT sequence, logical_id, version, payload, previous_hash, hash
         FROM recovery_event_records WHERE worker = ?1 AND sequence > ?2
         ORDER BY sequence LIMIT ?3",
            params![
                worker,
                i64::try_from(after).map_err(|_| Error::Overflow)?,
                i64::from(limit)
            ],
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
    rows.into_iter().map(decode_recovery_row).collect()
}

pub fn append_recovery_event(
    c: &mut UpdateConnection<'_>,
    worker: &[u8],
    record: &StoredRecoveryEvent,
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO recovery_event_records(worker, sequence, logical_id, version, payload, previous_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![worker, i64::try_from(record.sequence).map_err(|_| Error::Overflow)?,
            record.logical_id.as_slice(), i64::from(record.version), record.payload.as_slice(),
            record.previous_hash.as_slice(), record.hash.as_slice()],
    ).map_err(sql)?;
    c.execute(
        "INSERT INTO recovery_event_heads(worker, sequence, hash) VALUES (?1, ?2, ?3)
         ON CONFLICT(worker) DO UPDATE SET sequence = excluded.sequence, hash = excluded.hash",
        params![
            worker,
            i64::try_from(record.sequence).map_err(|_| Error::Overflow)?,
            record.hash.as_slice()
        ],
    )
    .map_err(sql)
}

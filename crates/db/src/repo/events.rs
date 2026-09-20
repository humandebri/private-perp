//! 外部イベントの取り込みと資金履歴。`Implementation.md` 14.1・14.2。

use crate::error::Error;
use crate::repo::sql;
use api_types::fund::FundRequestState;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 検証済みの外部イベント。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEvent {
    pub event_id: [u8; 32],
    pub network: String,
    pub account_address: [u8; 20],
    pub counterparty: [u8; 20],
    pub asset: String,
    pub amount: u64,
    pub kind: String,
    pub at: u64,
    pub evidence_ref: Option<String>,
}

/// 資金履歴の1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundEventRow {
    pub request_id: Vec<u8>,
    pub kind: String,
    pub state: FundRequestState,
    pub amount: u64,
    pub at: u64,
}

/// 外部イベントを取り込む。同じ安定IDの二重取り込みは `false`（計上しない）。
pub fn ingest_external_event(
    connection: &mut UpdateConnection<'_>,
    event: &ExternalEvent,
    now: u64,
) -> Result<bool, Error> {
    let evidence_value = match event.evidence_ref.as_deref() {
        Some(value) => ic_sqlite_vfs::db::Value::Text(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let amount = i64::try_from(event.amount).map_err(|_| Error::Overflow)?;
    let result = connection.execute(
        "INSERT INTO external_events
           (event_id, network, account_address, counterparty, asset, amount, kind, at, evidence_ref, ingested_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event.event_id.as_slice(),
            event.network.as_str(),
            event.account_address.as_slice(),
            event.counterparty.as_slice(),
            event.asset.as_str(),
            amount,
            event.kind.as_str(),
            event.at as i64,
            evidence_value,
            now as i64
        ],
    );
    match result {
        Ok(()) => Ok(true),
        Err(error) => {
            let classified = crate::error::classify_sql(error.to_string());
            match classified {
                Error::Conflict => Ok(false),
                other => Err(other),
            }
        }
    }
}

/// 外部イベントを引く。
pub fn find_external_event(
    connection: &Connection,
    network: &str,
    event_id: &[u8; 32],
) -> Result<Option<ExternalEvent>, Error> {
    let raw = connection
        .query_optional(
            "SELECT event_id, network, account_address, counterparty, asset, amount, kind, at, evidence_ref
               FROM external_events WHERE network = ?1 AND event_id = ?2",
            params![network, event_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<String>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<Vec<u8>>(3)?,
                    row.get::<String>(4)?,
                    row.get::<i64>(5)?,
                    row.get::<String>(6)?,
                    row.get::<i64>(7)?,
                    row.get::<Option<String>>(8)?,
                ))
            },
        )
        .map_err(sql)?;

    raw.map(|raw| {
        Ok(ExternalEvent {
            event_id: raw
                .0
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte event id"))?,
            network: raw.1,
            account_address: raw
                .2
                .try_into()
                .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
            counterparty: raw
                .3
                .try_into()
                .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
            asset: raw.4,
            amount: u64::try_from(raw.5).map_err(|_| Error::Invariant("negative amount"))?,
            kind: raw.6,
            at: u64::try_from(raw.7).map_err(|_| Error::Invariant("negative timestamp"))?,
            evidence_ref: raw.8,
        })
    })
    .transpose()
}

/// 資金履歴（LIMIT付き。カーソルは直前の `at`）。
pub fn list_fund_events(
    connection: &Connection,
    user_id: &[u8; 32],
    limit: u32,
    cursor: Option<u64>,
) -> Result<Vec<FundEventRow>, Error> {
    let limit = i64::from(limit.clamp(1, 100));
    let cursor_value = match cursor {
        Some(value) => {
            ic_sqlite_vfs::db::Value::Integer(i64::try_from(value).map_err(|_| Error::Overflow)?)
        }
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let raw = connection
        .query_all(
            "SELECT client_request_id, kind, state, amount, updated_at
               FROM fund_requests
              WHERE user_id = ?1 AND (?2 IS NULL OR updated_at < ?2)
              ORDER BY updated_at DESC, client_request_id DESC
              LIMIT ?3",
            params![user_id.as_slice(), cursor_value, limit],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<String>(1)?,
                    row.get::<String>(2)?,
                    row.get::<i64>(3)?,
                    row.get::<i64>(4)?,
                ))
            },
        )
        .map_err(sql)?;

    raw.into_iter()
        .map(|raw| {
            Ok(FundEventRow {
                request_id: raw.0,
                kind: raw.1,
                state: crate::states::fund_request_state_from_str(&raw.2)
                    .ok_or(Error::Invariant("unknown request state"))?,
                amount: u64::try_from(raw.3).map_err(|_| Error::Invariant("negative amount"))?,
                at: u64::try_from(raw.4).map_err(|_| Error::Invariant("negative timestamp"))?,
            })
        })
        .collect()
}

/// 監査記録（平文のintent・署名は入れない）。
pub fn insert_audit(
    connection: &mut UpdateConnection<'_>,
    actor: &str,
    action: &str,
    subject: Option<&str>,
    reason: Option<&str>,
    now: u64,
) -> Result<(), Error> {
    let subject_value = match subject {
        Some(value) => ic_sqlite_vfs::db::Value::Text(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let reason_value = match reason {
        Some(value) => ic_sqlite_vfs::db::Value::Text(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO audit (at, actor, action, subject, reason_code) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now as i64, actor, action, subject_value, reason_value],
        )
        .map_err(sql)
}

//! Durable one-shot conversion of an evidenced Spot receipt inside the shared reserve.
use crate::{error::Error, repo::sql};
use ic_sqlite_vfs::{
    db::{UpdateConnection, connection::Connection},
    params,
};

#[derive(Clone)]
pub struct Conversion {
    pub nonce: u64,
    pub state: String,
    pub amount: u64,
    pub sender: Vec<u8>,
    pub account: Vec<u8>,
}

pub fn get(c: &Connection, id: &[u8; 32]) -> Result<Option<Conversion>, Error> {
    c.query_optional(
        "SELECT nonce,state,amount,sender,account_id FROM spot_deposit_conversions WHERE event_id=?1",
        params![id.as_slice()],
        |row| {
            Ok(Conversion {
                nonce: row.get::<i64>(0)? as u64,
                state: row.get(1)?,
                amount: row.get::<i64>(2)? as u64,
                sender: row.get(3)?,
                account: row.get(4)?,
            })
        },
    )
    .map_err(sql)
}
pub fn begin(
    c: &mut UpdateConnection<'_>,
    id: &[u8; 32],
    account: &[u8; 32],
    amount: u64,
    sender: &[u8; 20],
    now: u64,
) -> Result<u64, Error> {
    if c.query_optional_scalar::<i64>(
        "SELECT 1 FROM spot_deposit_conversions WHERE account_id=?1 AND state <> 'settled' LIMIT 1",
        params![account.as_slice()],
    )
    .map_err(sql)?
    .is_some()
    {
        return Err(Error::Conflict);
    }
    let nonce = super::actions::allocate_master_nonce(c, "reserve", now)?;
    c.execute("INSERT INTO spot_deposit_conversions(event_id,account_id,nonce,amount,sender,created_at,state) VALUES(?1,?2,?3,?4,?5,?6,'prepared')",params![id.as_slice(),account.as_slice(),nonce as i64,i64::try_from(amount).map_err(|_|Error::Overflow)?,sender.as_slice(),now as i64]).map_err(sql)?;
    Ok(nonce)
}
pub fn transition(
    c: &mut UpdateConnection<'_>,
    id: &[u8; 32],
    from: &str,
    to: &str,
) -> Result<(), Error> {
    c.execute(
        "UPDATE spot_deposit_conversions SET state=?1 WHERE event_id=?2 AND state=?3",
        params![to, id.as_slice(), from],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(crate::cas::changes(c)?, from, to)
}
pub fn pending_claims(c: &Connection, eoa: &[u8; 20]) -> Result<Vec<([u8; 32], u64)>, Error> {
    c.query_all("SELECT e.event_id,e.amount FROM external_events e JOIN journals j ON j.external_event_id=e.event_id WHERE e.counterparty=?1 AND j.kind='deposit_unmatched' AND NOT EXISTS (SELECT 1 FROM journal_requests claimed WHERE claimed.kind='deposit_claimed' AND claimed.request_id=e.event_id) ORDER BY e.at,e.event_id LIMIT 50",params![eoa.as_slice()],|r|Ok((r.get::<Vec<u8>>(0)?,r.get::<i64>(1)?))).map_err(sql)?.into_iter().map(|(id,amount)|Ok((id.try_into().map_err(|_|Error::Invariant("invalid claim id"))?,u64::try_from(amount).map_err(|_|Error::Overflow)?))).collect()
}

pub fn unresolved(
    c: &Connection,
    eoa: &[u8; 20],
) -> Result<Vec<super::actions::UnresolvedActionRow>, Error> {
    c.query_all("SELECT event_id,created_at FROM spot_deposit_conversions WHERE sender=?1 AND state <> 'settled' ORDER BY created_at LIMIT 100",params![eoa.as_slice()],|r|Ok((r.get::<Vec<u8>>(0)?,r.get::<i64>(1)?))).map_err(sql)?.into_iter().map(|(id,at)|Ok(super::actions::UnresolvedActionRow {action_id:id.try_into().map_err(|_|Error::Invariant("bad conversion id"))?,kind:api_types::fund::FundActionKind::SpotDeposit,state:api_types::fund::ActionState::Unknown,since:at as u64})).collect()
}

/// Already evidenced class transfers cannot satisfy another deposit conversion.
pub fn consumed_receipts(
    c: &Connection,
    id: &[u8; 32],
    account: &[u8; 32],
    nonce: u64,
) -> Result<Vec<Vec<u8>>, Error> {
    c.query_all("SELECT receipt_hash FROM spot_deposit_conversions WHERE event_id<>?1 AND account_id=?2 AND receipt_hash IS NOT NULL AND nonce BETWEEN ?3 AND ?4 LIMIT 500", params![id.as_slice(),account.as_slice(),nonce.saturating_sub(120_000) as i64,nonce.saturating_add(120_000) as i64], |r| r.get(0)).map_err(sql)
}

pub fn bind_receipt(c: &mut UpdateConnection<'_>, id: &[u8; 32], hash: &[u8]) -> Result<(), Error> {
    c.execute("UPDATE spot_deposit_conversions SET receipt_hash=?1 WHERE event_id=?2 AND state IN ('dispatching','accepted') AND (receipt_hash IS NULL OR receipt_hash=?1)", params![hash,id.as_slice()]).map_err(sql)?;
    crate::cas::ensure_changed(crate::cas::changes(c)?, "pending", "evidenced")
}

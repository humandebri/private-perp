//! Shared deposit posting for live ingestion and journal replay.

use crate::error::Error;
use crate::repo::{auth, events, funds, ledger};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::params;

#[allow(clippy::too_many_arguments)]
pub fn credit_external_deposit(
    connection: &mut UpdateConnection<'_>,
    event_id: &[u8; 32],
    network: &str,
    tx_hash: &[u8],
    amount: u64,
    address: &[u8; 20],
    asset: &str,
    now: u64,
    sender: Option<&[u8; 20]>,
) -> Result<bool, Error> {
    credit_external_deposit_with_fee(
        connection, event_id, network, tx_hash, amount, 0, address, asset, now, sender,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn credit_external_deposit_with_fee(
    connection: &mut UpdateConnection<'_>,
    event_id: &[u8; 32],
    network: &str,
    tx_hash: &[u8],
    amount: u64,
    fee: u64,
    address: &[u8; 20],
    asset: &str,
    now: u64,
    sender: Option<&[u8; 20]>,
) -> Result<bool, Error> {
    if tx_hash.is_empty()
        || tx_hash.len() > 64
        || fee >= amount
        || amount == 0
        || amount > i64::MAX as u64
        || now == 0
        || now > i64::MAX as u64
        || asset != "usdc"
    {
        return Err(Error::Invariant("invalid deposit evidence"));
    }
    // Managed recoveries are settled by their action, never as fresh deposits.
    if let Some(sender) = sender
        && ledger::custody_account_by_address(connection, address)?
            .is_some_and(|a| a.kind == "reserve")
        && ledger::custody_account_by_address(connection, sender)?.is_some()
    {
        return Ok(false);
    }
    ensure_allocation_ready(connection, address, sender)?;
    let event = events::ExternalEvent {
        event_id: *event_id,
        network: network.to_string(),
        account_address: *address,
        counterparty: sender.copied().unwrap_or([0u8; 20]),
        asset: asset.to_string(),
        amount: amount - fee,
        kind: "deposit".to_string(),
        at: now,
        evidence_ref: Some(hex::encode(tx_hash)),
    };
    if !events::ingest_external_event(connection, &event, now)? {
        return Ok(false);
    }
    match ledger::custody_account_by_address(connection, address)? {
        Some(owner) if owner.kind == "trading" => {
            let user_id = owner
                .user_id
                .ok_or(Error::Invariant("trading account requires an owner"))?;
            let in_transit = ledger::user_in_transit_balance(connection, &user_id)?;
            let confirmable = amount.min(in_transit);
            if fee > 0 && confirmable > 0 && confirmable != amount {
                return Err(Error::Invariant(
                    "fee-bearing allocation requires complete settlement",
                ));
            }
            let confirmed = funds::confirm_executing_allocations(
                connection,
                &user_id,
                &owner.account_id,
                event_id,
                confirmable,
                now,
            )?;
            if fee > 0 && confirmed > 0 && confirmed != amount {
                return Err(Error::Invariant(
                    "fee-bearing allocation is not fully matched",
                ));
            }
            if confirmed > 0 {
                ledger::allocation_confirm_with_fee(
                    connection,
                    &user_id,
                    &owner.account_id,
                    confirmed,
                    fee,
                    now,
                    event_id,
                )?;
            }
            let excess = if confirmed == 0 {
                amount - fee
            } else {
                amount - confirmed
            };
            // Once observations begin, direct arrivals are included in the next
            // absolute equity observation. Never add them again: the response
            // may already contain an arrival whose history was ingested later.
            if excess > 0 && ledger::trading_observed_at(connection, &owner.account_id)? == 0 {
                ledger::trading_deposit_confirmed(connection, &owner.account_id, excess, now)?;
                events::insert_audit(
                    connection,
                    "system",
                    "trading_deposit_direct",
                    None,
                    Some("trading_account"),
                    now,
                )?;
            }
        }
        Some(owner) if owner.kind == "reserve" => {
            let identity = sender
                .map(|sender| auth::find_identity_by_eoa(connection, sender))
                .transpose()?
                .flatten();
            if let Some(identity) = identity {
                ledger::deposit_confirmed(
                    connection,
                    &identity.user_id,
                    amount - fee,
                    now,
                    event_id,
                )?;
            } else {
                ledger::unmatched_deposit(connection, amount - fee, now, event_id)?;
            }
        }
        _ => {
            ledger::unmatched_deposit(connection, amount - fee, now, event_id)?;
            events::insert_audit(
                connection,
                "system",
                "unmatched_deposit",
                None,
                Some("unmatched_address"),
                now,
            )?;
        }
    }
    Ok(true)
}

pub fn ensure_allocation_ready(
    c: &ic_sqlite_vfs::db::connection::Connection,
    address: &[u8; 20],
    sender: Option<&[u8; 20]>,
) -> Result<(), Error> {
    let Some(sender) = sender else {
        return Ok(());
    };
    if !ledger::custody_account_by_address(c, sender)?.is_some_and(|a| a.kind == "reserve") {
        return Ok(());
    }
    let Some(owner) = ledger::custody_account_by_address(c, address)? else {
        return Ok(());
    };
    if owner.kind == "trading" && c.query_optional_scalar::<i64>("SELECT 1 FROM fund_requests WHERE account_id=?1 AND kind='allocation' AND state IN ('reserved','unknown') LIMIT 1", params![owner.account_id.as_slice()]).map_err(super::sql)?.is_some() {
        return Err(Error::Conflict);
    }
    Ok(())
}

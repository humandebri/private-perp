//! Receive Spot USDC without exposing a signing or arbitrary-transfer endpoint.
use api_types::{
    auth::SessionHandle,
    error::ErrorCode,
    journal::{RecoveryEvent, RecoveryPayload},
    operations::BudgetClass,
};
use hl_sign::user_signed::{self, TypedValue};
use ic_sqlite_vfs::params;

fn internal(e: db::error::Error) -> ErrorCode {
    crate::auth::map_db(e, None)
}
fn invalid() -> ErrorCode {
    ErrorCode::UpstreamRejected {
        code: "incomplete Spot conversion evidence".into(),
        retryable: false,
    }
}
fn address(value: &serde_json::Value) -> Option<[u8; 20]> {
    let text = value.as_str()?;
    hex::decode(text.strip_prefix("0x").unwrap_or(text))
        .ok()?
        .try_into()
        .ok()
}
/// Only the venue's inbound USDC `send` to the shared reserve is eligible.
/// Its fee is charged separately to the sender; `amount` is the received amount.
pub(crate) fn receipt(
    entry: &serde_json::Value,
    reserve: &[u8; 20],
) -> Result<Option<([u8; 20], u64, bool)>, ErrorCode> {
    let d = &entry["delta"];
    if d["type"] != "send" || d["token"] != "USDC" {
        return Ok(None);
    }
    let destination = address(&d["destination"]).ok_or_else(invalid)?;
    if destination != *reserve {
        return Ok(None);
    }
    let sender = address(&d["user"])
        .filter(|sender| sender != reserve)
        .ok_or_else(invalid)?;
    let amount = crate::deposits::deposit_amount_micros(&d["amount"])
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(invalid)?;
    let spot = match d["destinationDex"].as_str().ok_or_else(invalid)? {
        "spot" => true,
        "" => false,
        _ => return Ok(None),
    };
    Ok(Some((sender, amount, spot)))
}

pub(crate) fn digest(
    amount: u64,
    nonce: u64,
    network: hl_types::Network,
) -> Result<[u8; 32], ErrorCode> {
    let chain =
        hl_types::environment::chain_name(network).map_err(crate::environment::map_environment)?;
    let id = hl_types::environment::user_signed_chain_id(network)
        .map_err(crate::environment::map_environment)?;
    user_signed::digest(
        id,
        user_signed::USD_CLASS_TRANSFER_PRIMARY_TYPE,
        user_signed::USD_CLASS_TRANSFER_FIELDS,
        &[
            TypedValue::String(chain.into()),
            TypedValue::String(crate::venue::amount_text(amount)),
            TypedValue::Bool(true),
            TypedValue::Uint64(nonce),
        ],
    )
    .map_err(|_| ErrorCode::PolicyUnavailable)
}

/// A matched conversion is evidence of movement, never an additional deposit.
fn conversion_evidence(
    entries: &[serde_json::Value],
    amount: u64,
    nonce: u64,
) -> Result<Option<Vec<u8>>, ErrorCode> {
    let matches: Vec<_> = entries
        .iter()
        .filter(|e| {
            let d = &e["delta"];
            e["time"].as_u64().is_some_and(|time| {
                time >= nonce.saturating_sub(60_000) && time <= nonce.saturating_add(60_000)
            }) && d["type"] == "accountClassTransfer"
                && d["toPerp"] == true
                && crate::deposits::deposit_amount_micros(&d["usdc"]) == Some(amount)
                && e["hash"]
                    .as_str()
                    .and_then(|h| hex::decode(h.strip_prefix("0x").unwrap_or(h)).ok())
                    .is_some_and(|h| h.len() == 32)
        })
        .collect();
    if entries.len() >= 500 || matches.len() > 1 {
        Err(invalid())
    } else {
        matches
            .first()
            .map(|entry| {
                hex::decode(entry["hash"].as_str().unwrap().trim_start_matches("0x"))
                    .map_err(|_| invalid())
            })
            .transpose()
    }
}

pub(crate) async fn convert(
    entry: &serde_json::Value,
    reserve: &[u8; 20],
    amount: u64,
) -> Result<bool, ErrorCode> {
    let network = crate::environment::network_name()?;
    let hash = entry["hash"]
        .as_str()
        .and_then(|h| hex::decode(h.strip_prefix("0x").unwrap_or(h)).ok())
        .filter(|h| h.len() == 32)
        .ok_or_else(invalid)?;
    let mut material = b"spot_conversion".to_vec();
    material.extend_from_slice(network.as_bytes());
    material.extend_from_slice(&hash);
    let id = hl_sign::keccak256(&material);
    let owner = db::tx::query(|c| db::repo::ledger::custody_account_by_address(c, reserve))
        .map_err(internal)?
        .ok_or_else(invalid)?;
    if owner.kind != "reserve" {
        return Err(invalid());
    }
    let account = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, &[0; 32], api_types::AccountKind::Reserve)
    })
    .map_err(internal)?
    .ok_or_else(invalid)?;
    if account.master_address != *reserve || account.network != network || account.state != "active"
    {
        return Err(invalid());
    }
    let previous = db::tx::query(|c| db::repo::spot_deposits::get(c, &id)).map_err(internal)?;
    let sender = receipt(entry, reserve)?.ok_or_else(invalid)?.0;
    if let Some(previous) = &previous
        && (previous.amount != amount
            || previous.sender != sender
            || previous.account != account.account_id)
    {
        return Err(invalid());
    }
    let conversion = if let Some(previous) = previous.as_ref().filter(|r| r.state != "prepared") {
        previous.clone()
    } else {
        journal_client::ensure_ready("vault").await?;
        if crate::deposits::spot_available(reserve).await? < amount {
            return Err(invalid());
        }
        let permit = crate::rest_budget::acquire(BudgetClass::Reconcile, 1).await?;
        let now = crate::clock::now_ms();
        // A prepared row has never reached POST authorization. Resume signing
        // with its original nonce; the prepared -> dispatching CAS grants one sender.
        let nonce = if let Some(previous) = &previous {
            previous.nonce
        } else {
            db::tx::update(|c| {
                db::repo::spot_deposits::begin(c, &id, &account.account_id, amount, &sender, now)
            })
            .map_err(internal)?
        };
        let path = crate::crypto::derivation_path(&[
            b"private-perp",
            b"reserve",
            hex::encode(account.account_id).as_bytes(),
        ]);
        let key = crate::crypto::public_key(path.clone()).await?;
        if hl_sign::address_from_public_key(&key).map_err(|_| invalid())? != *reserve {
            return Err(invalid());
        }
        let hl_network: hl_types::Network = crate::environment::resolved()?.network.into();
        let digest = digest(amount, nonce, hl_network)?;
        let signature = crate::crypto::sign_with_key(&digest, path, &key).await?;
        let intent =
            journal_client::intent("spot_conversion", &id, &account.account_id, nonce, &digest);
        let ack = journal_client::prepare_send("vault", intent.clone()).await?;
        if let Err(error) = db::tx::update(|c| {
            journal_client::record(c, &intent, &ack)?;
            db::repo::spot_deposits::transition(c, &id, "prepared", "dispatching")
        }) {
            journal_client::lock()?;
            return Err(internal(error));
        }
        journal_client::authorize_send(&intent).await?;
        if db::tx::query(|c| db::repo::spot_deposits::get(c, &id))
            .map_err(internal)?
            .is_none_or(|r| r.state != "dispatching")
        {
            return Err(invalid());
        }
        let chain = hl_types::environment::chain_name(hl_network)
            .map_err(crate::environment::map_environment)?;
        let chain_id = hl_types::environment::signature_chain_id(hl_network)
            .map_err(crate::environment::map_environment)?;
        let body=serde_json::to_vec(&serde_json::json!({"action":{"type":"usdClassTransfer","hyperliquidChain":chain,"signatureChainId":chain_id,"amount":crate::venue::amount_text(amount),"toPerp":true,"nonce":nonce},"nonce":nonce,"signature":{"r":format!("0x{}",hex::encode(signature.r)),"s":format!("0x{}",hex::encode(signature.s)),"v":signature.v}})).map_err(|_|invalid())?;
        // This POST is attempted once. A timeout leaves dispatching and is only reconciled.
        let state = match crate::venue::post_signed_body(body, &permit).await {
            Ok((crate::venue::ExchangeOutcome::Accepted, _)) => "accepted",
            Ok((crate::venue::ExchangeOutcome::Rejected { .. }, _)) => "rejected",
            Err(_) => "dispatching",
        };
        if state != "dispatching" {
            db::tx::update(|c| db::repo::spot_deposits::transition(c, &id, "dispatching", state))
                .map_err(internal)?;
        }
        db::repo::spot_deposits::Conversion {
            nonce,
            state: state.into(),
            amount,
            sender: sender.to_vec(),
            account: account.account_id.to_vec(),
        }
    };
    match conversion.state.as_str() {
        "settled" => return Ok(false),
        "prepared" | "rejected" => return Err(invalid()),
        "dispatching" | "accepted" => {}
        _ => return Err(invalid()),
    }
    let body = crate::deposits::fetch_ledger_updates_range(
        &format!("0x{}", hex::encode(reserve)),
        conversion.nonce.saturating_sub(60_000),
        Some(conversion.nonce.saturating_add(60_000)),
    )
    .await?;
    let mut entries: Vec<serde_json::Value> =
        serde_json::from_slice(&body).map_err(|_| invalid())?;
    if entries.len() >= 500 {
        return Err(invalid());
    }
    let consumed = db::tx::query(|c| {
        db::repo::spot_deposits::consumed_receipts(c, &id, &account.account_id, conversion.nonce)
    })
    .map_err(internal)?;
    if consumed.len() >= 500 {
        return Err(invalid());
    }
    entries.retain(|entry| {
        !entry["hash"]
            .as_str()
            .and_then(|h| hex::decode(h.trim_start_matches("0x")).ok())
            .is_some_and(|h| consumed.contains(&h))
    });
    let hash = conversion_evidence(&entries, amount, conversion.nonce)?.ok_or_else(invalid)?;
    db::tx::update(|c| db::repo::spot_deposits::bind_receipt(c, &id, &hash)).map_err(internal)?;
    // Leave settlement pending until the caller's journaled credit succeeds.
    Ok(true)
}

pub(crate) async fn claim_for_session(session: &SessionHandle) -> Result<(), ErrorCode> {
    let verified = crate::auth::verify_session(session, ic_cdk::api::msg_caller())?;
    let identity = db::tx::query(|c| db::repo::auth::identity_by_user(c, &verified.user_id))
        .map_err(internal)?
        .ok_or_else(invalid)?;
    let claims =
        db::tx::query(|c| db::repo::spot_deposits::pending_claims(c, &identity.eoa_address))
            .map_err(internal)?;
    for (event_id, amount) in claims {
        let now = crate::clock::now_ms();
        let mut logical = b"deposit_claim".to_vec();
        logical.extend_from_slice(&event_id);
        let event = RecoveryEvent {
            version: 1,
            logical_id: hl_sign::keccak256(&logical).to_vec().into(),
            payload: RecoveryPayload::DepositClaim {
                event_id: event_id.to_vec().into(),
                user_id: verified.user_id.to_vec().into(),
                amount_micros: amount,
                claimed_at_ms: now,
            },
        };
        let ack = journal_client::append_recovery_event_if("vault", event.clone(), |c| {
            Ok(!db::repo::ledger::unmatched_deposit_claimed(c, &event_id)?)
        })
        .await?;
        if let Some(ack) = ack {
            crate::auth::verify_session(session, ic_cdk::api::msg_caller())?;
            let result = db::tx::update(|c| {
                journal_client::record_recovery_event(c, &event, &ack)?;
                db::repo::ledger::claim_unmatched_deposit(
                    c,
                    &verified.user_id,
                    amount,
                    now,
                    &event_id,
                )
            });
            if let Err(error) = result {
                journal_client::lock()?;
                return Err(internal(error));
            }
        }
    }
    Ok(())
}

pub(crate) fn settle(entry: &serde_json::Value) -> Result<(), ErrorCode> {
    let hash = hex::decode(
        entry["hash"]
            .as_str()
            .ok_or_else(invalid)?
            .trim_start_matches("0x"),
    )
    .map_err(|_| invalid())?;
    let mut material = b"spot_conversion".to_vec();
    material.extend_from_slice(crate::environment::network_name()?.as_bytes());
    material.extend_from_slice(&hash);
    let id = hl_sign::keccak256(&material);
    db::tx::update(|c| {
        let row = db::repo::spot_deposits::get(c, &id)?.ok_or(db::error::Error::NotFound)?;
        if row.state == "settled" {
            return Ok(());
        }
        db::repo::spot_deposits::transition(c, &id, &row.state, "settled")
    })
    .map_err(internal)
}

pub(crate) fn history_start(network: &str, address: &[u8; 20]) -> Result<u64, ErrorCode> {
    db::tx::query(|c| {
        c.query_optional_scalar::<i64>(
            "SELECT start_time FROM spot_deposit_history_cursors WHERE network=?1 AND address=?2",
            params![network, address.as_slice()],
        )
        .map_err(|e| db::error::Error::Sql(e.to_string()))
    })
    .map_err(internal)
    .map(|n| n.unwrap_or(0) as u64)
}
pub(crate) fn advance(network: &str, address: &[u8; 20], time: u64) -> Result<(), ErrorCode> {
    db::tx::update(|c|c.execute("INSERT INTO spot_deposit_history_cursors(network,address,start_time) VALUES(?1,?2,?3) ON CONFLICT(network,address) DO UPDATE SET start_time=MAX(start_time,excluded.start_time)",params![network,address.as_slice(),time as i64]).map_err(|e|db::error::Error::Sql(e.to_string()))).map_err(internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_requires_usdc_destination_and_valid_sender_amount() {
        let reserve = [1; 20];
        let mut e = serde_json::json!({"delta":{"type":"send","token":"USDC","user":format!("0x{}",hex::encode([2;20])),"destination":format!("0x{}",hex::encode(reserve)),"destinationDex":"spot","amount":"10","fee":"1"}});
        assert_eq!(
            receipt(&e, &reserve).unwrap(),
            Some(([2; 20], 10_000_000, true))
        );
        e["delta"]["amount"] = "-10".into();
        assert!(receipt(&e, &reserve).is_err());
        e["delta"]["amount"] = "10".into();
        e["delta"]["token"] = "HYPE".into();
        assert_eq!(receipt(&e, &reserve).unwrap(), None);
    }
    #[test]
    fn class_transfer_evidence_rejects_ambiguous_wrong_direction_and_bad_hash() {
        let event = serde_json::json!({"time":100000,"hash":format!("0x{}",hex::encode([3;32])),"delta":{"type":"accountClassTransfer","usdc":"10","toPerp":true}});
        assert!(
            conversion_evidence(std::slice::from_ref(&event), 10_000_000, 100000)
                .unwrap()
                .is_some()
        );
        assert!(conversion_evidence(&[event.clone(), event.clone()], 10_000_000, 100000).is_err());
        assert!(
            conversion_evidence(std::slice::from_ref(&event), 9_000_000, 100000)
                .unwrap()
                .is_none()
        );
        let mut bad = event;
        bad["delta"]["toPerp"] = false.into();
        assert!(
            conversion_evidence(&[bad.clone()], 10_000_000, 100000)
                .unwrap()
                .is_none()
        );
        bad["delta"]["toPerp"] = true.into();
        bad["hash"] = "0x00".into();
        assert!(
            conversion_evidence(&[bad], 10_000_000, 100000)
                .unwrap()
                .is_none()
        );
    }
}

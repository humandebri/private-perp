//! vaultが保持する取引口座からの回収をcoreの注文境界で直列化する。

use api_types::error::{BadRequestCode, ErrorCode};
use api_types::recovery::{PrepareRecovery, RecoveryFenceToken};

fn malformed() -> ErrorCode {
    ErrorCode::BadRequest {
        code: BadRequestCode::MalformedPayload,
        detail: "invalid recovery fence token".into(),
    }
}

fn bytes<const N: usize>(value: &[u8]) -> Result<[u8; N], ErrorCode> {
    value.try_into().map_err(|_| malformed())
}

fn require_vault() -> Result<(), ErrorCode> {
    if crate::vault_principal()? != ic_cdk::api::msg_caller() {
        return Err(ErrorCode::Unauthenticated {
            reason: "only the configured vault can change recovery fences".into(),
        });
    }
    Ok(())
}

fn token_parts(token: &RecoveryFenceToken) -> Result<([u8; 32], [u8; 32]), ErrorCode> {
    if token.request_id.is_empty() || token.request_id.len() > 128 || token.epoch == 0 {
        return Err(malformed());
    }
    Ok((
        bytes(token.account_id.as_ref())?,
        bytes(token.user_id.as_ref())?,
    ))
}

async fn venue_is_flat(address: &[u8; 20]) -> Result<bool, ErrorCode> {
    let address = format!("0x{}", hex::encode(address));
    let orders = crate::venue::open_orders(&address).await?;
    let orders: serde_json::Value =
        serde_json::from_str(&orders).map_err(|_| ErrorCode::PolicyUnavailable)?;
    let orders = orders.as_array().ok_or(ErrorCode::PolicyUnavailable)?;
    if orders
        .iter()
        .any(|order| !order.get("oid").is_some_and(|oid| oid.is_u64()))
    {
        return Err(ErrorCode::PolicyUnavailable);
    }
    if !orders.is_empty() {
        return Ok(false);
    }
    let state = crate::venue::clearinghouse_state(&address).await?;
    let state: serde_json::Value =
        serde_json::from_str(&state).map_err(|_| ErrorCode::PolicyUnavailable)?;
    let positions = state
        .get("assetPositions")
        .and_then(|value| value.as_array())
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if positions.iter().any(|entry| {
        entry
            .get("position")
            .and_then(|value| value.get("szi"))
            .and_then(|value| value.as_str())
            .is_none()
    }) {
        return Err(ErrorCode::PolicyUnavailable);
    }
    Ok(positions.iter().all(|entry| {
        let size = entry["position"]["szi"].as_str().unwrap_or("");
        matches!(hl_types::decimal::Decimal::parse(size), Ok(value) if value.as_str() == "0")
    }))
}

pub async fn prepare(args: PrepareRecovery) -> Result<RecoveryFenceToken, ErrorCode> {
    require_vault()?;
    let account_id: [u8; 32] = bytes(args.account_id.as_ref())?;
    let user_id: [u8; 32] = bytes(args.user_id.as_ref())?;
    let address: [u8; 20] = bytes(args.master_address.as_ref())?;
    let request_id = args.request_id.as_ref();
    if request_id.is_empty() || request_id.len() > 128 {
        return Err(malformed());
    }
    let now = ic_cdk::api::time() / 1_000_000;
    let epoch = db::tx::update(|connection| {
        if let Some((owner, saved_address)) = db::repo::accounts::identity(connection, &account_id)?
            && (owner != user_id || saved_address != address)
        {
            return Err(db::error::Error::Conflict);
        }
        db::repo::accounts::upsert(connection, &account_id, &user_id, &address, now)?;
        if db::repo::orders::pending_order_count(connection, &account_id)? != 0 {
            return Err(db::error::Error::Conflict);
        }
        db::repo::recovery_fences::begin(connection, &account_id, &user_id, request_id, now)
    })
    .map_err(crate::map_db)?;
    let token = RecoveryFenceToken {
        account_id: account_id.to_vec().into(),
        user_id: user_id.to_vec().into(),
        request_id: request_id.to_vec().into(),
        epoch,
    };
    // 途中で応答を失えばpreparingのまま保持し、同一要求だけが再確認できる。
    if !venue_is_flat(&address).await? {
        abort(token.clone())?;
        return Err(ErrorCode::ReservationConflict);
    }
    let checked_at = ic_cdk::api::time() / 1_000_000;
    let ready = db::tx::update(|connection| {
        let row = db::repo::recovery_fences::matches(
            connection,
            &account_id,
            &user_id,
            request_id,
            epoch,
        )?
        .ok_or(db::error::Error::Conflict)?;
        if db::repo::orders::pending_order_count(connection, &account_id)? != 0 {
            if row.state == "preparing" {
                db::repo::recovery_fences::transition(
                    connection,
                    &account_id,
                    request_id,
                    epoch,
                    "preparing",
                    "released",
                    checked_at,
                )?;
            }
            return Ok(false);
        }
        if row.state == "ready" || row.state == "committed" {
            return Ok(true);
        }
        db::repo::recovery_fences::transition(
            connection,
            &account_id,
            request_id,
            epoch,
            "preparing",
            "ready",
            checked_at,
        )?;
        Ok(true)
    })
    .map_err(crate::map_db)?;
    if !ready {
        return Err(ErrorCode::ReservationConflict);
    }
    Ok(token)
}

pub async fn commit(token: RecoveryFenceToken) -> Result<(), ErrorCode> {
    require_vault()?;
    let (account_id, user_id) = token_parts(&token)?;
    let row = db::tx::query(|connection| {
        db::repo::recovery_fences::matches(
            connection,
            &account_id,
            &user_id,
            token.request_id.as_ref(),
            token.epoch,
        )
    })
    .map_err(crate::map_db)?
    .ok_or(ErrorCode::ReservationConflict)?;
    if row.state != "ready" && row.state != "committed" {
        return Err(ErrorCode::ReservationConflict);
    }
    let address =
        db::tx::query(|connection| db::repo::accounts::master_address(connection, &account_id))
            .map_err(crate::map_db)?
            .ok_or(ErrorCode::ReservationConflict)?;
    if !venue_is_flat(&address).await? {
        return Err(ErrorCode::ReservationConflict);
    }
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|connection| {
        let current = db::repo::recovery_fences::matches(
            connection,
            &account_id,
            &user_id,
            token.request_id.as_ref(),
            token.epoch,
        )?
        .ok_or(db::error::Error::Conflict)?;
        if db::repo::orders::pending_order_count(connection, &account_id)? != 0 {
            return Err(db::error::Error::Conflict);
        }
        if current.state == "committed" {
            return Ok(());
        }
        db::repo::recovery_fences::transition(
            connection,
            &account_id,
            token.request_id.as_ref(),
            token.epoch,
            "ready",
            "committed",
            now,
        )
    })
    .map_err(crate::map_db)
}

pub fn mark_unknown(token: RecoveryFenceToken) -> Result<(), ErrorCode> {
    require_vault()?;
    let (account_id, user_id) = token_parts(&token)?;
    db::tx::update(|connection| {
        let row = db::repo::recovery_fences::matches(
            connection,
            &account_id,
            &user_id,
            token.request_id.as_ref(),
            token.epoch,
        )?
        .ok_or(db::error::Error::Conflict)?;
        if row.state == "unknown" {
            return Ok(());
        }
        db::repo::recovery_fences::transition(
            connection,
            &account_id,
            token.request_id.as_ref(),
            token.epoch,
            "committed",
            "unknown",
            ic_cdk::api::time() / 1_000_000,
        )
    })
    .map_err(crate::map_db)
}

pub fn abort(token: RecoveryFenceToken) -> Result<(), ErrorCode> {
    require_vault()?;
    let (account_id, user_id) = token_parts(&token)?;
    db::tx::update(|connection| {
        let row = db::repo::recovery_fences::matches(
            connection,
            &account_id,
            &user_id,
            token.request_id.as_ref(),
            token.epoch,
        )?
        .ok_or(db::error::Error::Conflict)?;
        if row.state == "released" {
            return Ok(());
        }
        if row.state != "preparing" && row.state != "ready" {
            return Err(db::error::Error::Conflict);
        }
        db::repo::recovery_fences::transition(
            connection,
            &account_id,
            token.request_id.as_ref(),
            token.epoch,
            &row.state,
            "released",
            ic_cdk::api::time() / 1_000_000,
        )
    })
    .map_err(crate::map_db)
}

pub fn finish(token: RecoveryFenceToken) -> Result<(), ErrorCode> {
    require_vault()?;
    let (account_id, user_id) = token_parts(&token)?;
    db::tx::update(|connection| {
        let row = db::repo::recovery_fences::matches(
            connection,
            &account_id,
            &user_id,
            token.request_id.as_ref(),
            token.epoch,
        )?
        .ok_or(db::error::Error::Conflict)?;
        if row.state == "released" {
            return Ok(());
        }
        if row.state != "committed" && row.state != "unknown" {
            return Err(db::error::Error::Conflict);
        }
        db::repo::recovery_fences::transition(
            connection,
            &account_id,
            token.request_id.as_ref(),
            token.epoch,
            &row.state,
            "released",
            ic_cdk::api::time() / 1_000_000,
        )
    })
    .map_err(crate::map_db)
}

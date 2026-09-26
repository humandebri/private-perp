//! HLで確認できたレバレッジだけを再利用する。送信結果不明は後続の変更を塞ぐ。

use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::params;

/// 外部のAgentによる変更を永久には見逃さないため、確認結果には期限を設ける。
const CONFIRMED_FRESH_MS: u64 = 10 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Send,
    Skip,
    Wait,
}

pub fn prepare(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    account_id: &[u8; 32],
    asset_index: u32,
    desired: u32,
    now: u64,
) -> Result<Decision, Error> {
    // 先行注文の送信が終わるまで別倍率へ切り替えない。HLでの適用順序を守る。
    let conflicting = connection
        .query_scalar::<i64>(
            "SELECT COUNT(*) FROM orders
             WHERE account_id = ?1 AND asset_index = ?2 AND effective_leverage != ?3
               AND order_id != ?4 AND state IN ('pending', 'unknown')
               AND dispatch_state IN ('queued', 'signing', 'dispatching', 'unknown')
               AND preflight_state IN ('reconciled', 'dispatching', 'unknown')",
            params![
                account_id.as_slice(),
                asset_index as i64,
                desired as i64,
                order_id.as_slice()
            ],
        )
        .map_err(sql)?;
    if conflicting > 0 {
        return Ok(Decision::Wait);
    }
    let row = connection
        .query_optional(
            "SELECT confirmed_leverage, confirmed_at, pending_order_id, pending_state
               FROM leverage_cache WHERE account_id = ?1 AND asset_index = ?2",
            params![account_id.as_slice(), asset_index as i64],
            |row| {
                Ok((
                    row.get::<Option<i64>>(0)?,
                    row.get::<Option<i64>>(1)?,
                    row.get::<Option<Vec<u8>>>(2)?,
                    row.get::<Option<String>>(3)?,
                ))
            },
        )
        .map_err(sql)?;
    if let Some((confirmed, confirmed_at, owner, pending_state)) = row {
        if let Some(owner) = owner {
            if owner.as_slice() == order_id && pending_state.as_deref() == Some("reserved") {
                return Ok(Decision::Send);
            }
            if pending_state.as_deref() == Some("unknown") {
                return Ok(Decision::Wait);
            }
            // 予約者が署名前に停止した場合だけ、次の注文が引き継ぐ。
            let old = connection
                .query_optional(
                    "SELECT preflight_state, dispatch_state, lease_until FROM orders WHERE order_id = ?1",
                    params![owner.as_slice()],
                    |row| Ok((row.get::<String>(0)?, row.get::<String>(1)?, row.get::<Option<i64>>(2)?)),
                )
                .map_err(sql)?;
            match old {
                Some((preflight, _, _)) if preflight == "dispatching" || preflight == "unknown" => {
                    return Ok(Decision::Wait);
                }
                Some((_, dispatch, lease))
                    if dispatch == "signing" && lease.is_some_and(|until| until >= now as i64) =>
                {
                    return Ok(Decision::Wait);
                }
                _ => {}
            }
        } else if confirmed == Some(desired as i64)
            && confirmed_at
                .is_some_and(|at| at >= 0 && now.saturating_sub(at as u64) <= CONFIRMED_FRESH_MS)
        {
            return Ok(Decision::Skip);
        }
    }
    connection
        .execute(
            "INSERT INTO leverage_cache(account_id, asset_index, confirmed_leverage, confirmed_at,
                 pending_order_id, pending_leverage, pending_state)
             VALUES (?1, ?2, NULL, NULL, ?3, ?4, 'reserved')
             ON CONFLICT(account_id, asset_index) DO UPDATE SET
                 confirmed_leverage = NULL, confirmed_at = NULL,
                 pending_order_id = excluded.pending_order_id,
                 pending_leverage = excluded.pending_leverage, pending_state = 'reserved'",
            params![
                account_id.as_slice(),
                asset_index as i64,
                order_id.as_slice(),
                desired as i64
            ],
        )
        .map_err(sql)?;
    Ok(Decision::Send)
}

pub fn owns_reservation(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    account_id: &[u8; 32],
    asset_index: u32,
) -> Result<bool, Error> {
    let owner = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT pending_order_id FROM leverage_cache
             WHERE account_id = ?1 AND asset_index = ?2 AND pending_state = 'reserved'",
            params![account_id.as_slice(), asset_index as i64],
        )
        .map_err(sql)?;
    Ok(owner.as_deref() == Some(order_id.as_slice()))
}

pub fn confirmed(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    account_id: &[u8; 32],
    asset_index: u32,
    leverage: u32,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE leverage_cache SET confirmed_leverage = ?4, confirmed_at = ?5,
                pending_order_id = NULL, pending_leverage = NULL, pending_state = NULL
             WHERE account_id = ?1 AND asset_index = ?2 AND pending_order_id = ?3
               AND pending_state IN ('reserved', 'unknown') AND pending_leverage = ?4",
            params![
                account_id.as_slice(),
                asset_index as i64,
                order_id.as_slice(),
                leverage as i64,
                now as i64
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "owned leverage change",
        "cache owner changed",
    )
}

pub fn unresolved(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    account_id: &[u8; 32],
    asset_index: u32,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE leverage_cache SET pending_state = 'unknown'
             WHERE account_id = ?1 AND asset_index = ?2 AND pending_order_id = ?3 AND pending_state = 'reserved'",
            params![account_id.as_slice(), asset_index as i64, order_id.as_slice()],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "owned leverage change",
        "cache owner changed",
    )
}

pub fn rejected(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    account_id: &[u8; 32],
    asset_index: u32,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE leverage_cache SET confirmed_leverage = NULL, confirmed_at = NULL,
                pending_order_id = NULL, pending_leverage = NULL, pending_state = NULL
             WHERE account_id = ?1 AND asset_index = ?2 AND pending_order_id = ?3",
            params![
                account_id.as_slice(),
                asset_index as i64,
                order_id.as_slice()
            ],
        )
        .map_err(sql)?;
    crate::cas::ensure_changed(
        crate::cas::changes(connection)?,
        "owned leverage change",
        "cache owner changed",
    )
}

/// v27より前の未解決preflightにもcontrollerの確認結果を適用できるようにする。
/// 他の注文が同じ口座・銘柄の予約を持つ場合は推測で上書きしない。
pub fn resolve_legacy_or_pending(
    connection: &mut UpdateConnection<'_>,
    order_id: &[u8; 32],
    account_id: &[u8; 32],
    asset_index: u32,
    leverage: u32,
    applied: bool,
    now: u64,
) -> Result<(), Error> {
    let owner = connection
        .query_optional_scalar::<Option<Vec<u8>>>(
            "SELECT pending_order_id FROM leverage_cache
             WHERE account_id = ?1 AND asset_index = ?2",
            params![account_id.as_slice(), asset_index as i64],
        )
        .map_err(sql)?
        .flatten();
    match owner {
        Some(owner) if owner.as_slice() == order_id => {
            if applied {
                confirmed(connection, order_id, account_id, asset_index, leverage, now)
            } else {
                rejected(connection, order_id, account_id, asset_index)
            }
        }
        Some(_) => Err(Error::Conflict),
        None => {
            if applied {
                connection
                    .execute(
                        "INSERT INTO leverage_cache(account_id, asset_index, confirmed_leverage,
                             confirmed_at, pending_order_id, pending_leverage, pending_state)
                         VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL)
                         ON CONFLICT(account_id, asset_index) DO UPDATE SET
                             confirmed_leverage = excluded.confirmed_leverage,
                             confirmed_at = excluded.confirmed_at",
                        params![
                            account_id.as_slice(),
                            asset_index as i64,
                            leverage as i64,
                            now as i64
                        ],
                    )
                    .map_err(sql)?;
            }
            Ok(())
        }
    }
}

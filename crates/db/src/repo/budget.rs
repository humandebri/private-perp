//! 単一policy DB上の共有rolling-window予算。返金・タイマーによるリセットは行わない。

use crate::error::Error;
use crate::repo::{amount_i64, sql};
use api_types::operations::{BudgetClass, RestBudgetConfig, RestBudgetRequest, RestBudgetStatus};
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

pub const WINDOW_MS: u64 = api_types::operations::REST_BUDGET_WINDOW_MS;

pub fn register_worker(
    c: &mut UpdateConnection<'_>,
    role: &str,
    principal: &[u8],
) -> Result<(), Error> {
    let old = c
        .query_optional_scalar::<Vec<u8>>(
            "SELECT principal FROM budget_workers WHERE role = ?1",
            params![role],
        )
        .map_err(sql)?;
    if let Some(old) = old {
        return if old == principal {
            Ok(())
        } else {
            Err(Error::Conflict)
        };
    }
    if is_worker(c, principal)? {
        return Err(Error::Conflict);
    }
    c.execute(
        "INSERT INTO budget_workers(role, principal) VALUES (?1, ?2)",
        params![role, principal],
    )
    .map_err(sql)
}

pub fn is_worker(c: &Connection, caller: &[u8]) -> Result<bool, Error> {
    Ok(c.query_optional_scalar::<String>(
        "SELECT role FROM budget_workers WHERE principal = ?1",
        params![caller],
    )
    .map_err(sql)?
    .is_some())
}

pub fn configure(c: &mut UpdateConnection<'_>, config: &RestBudgetConfig) -> Result<(), Error> {
    c.execute(
        "INSERT INTO rest_budget_config(singleton, capacity, exit_reserve) VALUES (1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET capacity = excluded.capacity, exit_reserve = excluded.exit_reserve",
        params![i64::from(config.capacity), i64::from(config.exit_reserve)],
    ).map_err(sql)
}

pub fn pause(c: &mut UpdateConnection<'_>, paused: bool) -> Result<(), Error> {
    c.execute(
        "UPDATE rest_budget_config SET recovery_paused = ?1 WHERE singleton = 1",
        params![i64::from(paused)],
    )
    .map_err(sql)?;
    crate::cas::ensure_changed(crate::cas::changes(c)?, "configured budget", "unconfigured")
}

pub fn status(c: &Connection, now: u64) -> Result<RestBudgetStatus, Error> {
    let config = c.query_optional(
        "SELECT capacity, exit_reserve, recovery_paused FROM rest_budget_config WHERE singleton = 1", params![],
        |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?, r.get::<i64>(2)?)),
    ).map_err(sql)?;
    // A grant may be dispatched any time BEFORE expires_at. Charge until that
    // deadline + one full window, not until consumed_at + one window.
    // expires_at <= consumed_at + WINDOW_MS bounds the indexed candidate range.
    let floor = amount_i64(now, "time")?.saturating_sub(WINDOW_MS as i64);
    let usage = c.query_optional(
        "SELECT COALESCE(SUM(weight), 0), COALESCE(SUM(CASE WHEN class = 'risk' THEN weight ELSE 0 END), 0)
         FROM rest_budget_usage WHERE consumed_at > ?1 AND expires_at > ?2",
        params![floor.saturating_sub(WINDOW_MS as i64), floor],
        |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?)),
    ).map_err(sql)?.ok_or(Error::NotFound)?;
    Ok(RestBudgetStatus {
        config: config
            .map(|(capacity, exit_reserve, _)| {
                Ok(RestBudgetConfig {
                    capacity: u32::try_from(capacity).map_err(|_| Error::Overflow)?,
                    exit_reserve: u32::try_from(exit_reserve).map_err(|_| Error::Overflow)?,
                })
            })
            .transpose()?,
        used: u32::try_from(usage.0).map_err(|_| Error::Overflow)?,
        new_risk_used: u32::try_from(usage.1).map_err(|_| Error::Overflow)?,
        recovery_paused: config.is_none_or(|(_, _, paused)| paused != 0),
    })
}

/// Called inside one Db::update: validation, aggregate check and insertion are atomic.
/// False means insufficient budget. A duplicate remains an error, never a second permit.
pub fn consume(
    c: &mut UpdateConnection<'_>,
    caller: &[u8],
    req: &RestBudgetRequest,
    now: u64,
) -> Result<bool, Error> {
    if !req.valid_at(now) {
        return Err(Error::Invariant("invalid budget request"));
    }
    let state = status(c, now)?;
    let config = state.config.ok_or(Error::NotFound)?;
    if state.recovery_paused && req.class != BudgetClass::Reconcile {
        return Err(Error::StateConflict {
            expected: "running".into(),
            actual: "recovery paused".into(),
        });
    }
    let duplicate = c
        .query_optional_scalar::<i64>(
            "SELECT 1 FROM rest_budget_usage WHERE caller = ?1 AND request_id = ?2",
            params![caller, req.request_id.as_ref()],
        )
        .map_err(sql)?
        .is_some();
    if duplicate {
        return Err(Error::Conflict);
    }
    if u64::from(state.used) + u64::from(req.weight) > u64::from(config.capacity)
        || (req.class == BudgetClass::NewRisk
            && u64::from(state.new_risk_used) + u64::from(req.weight)
                > u64::from(config.capacity - config.exit_reserve))
    {
        return Ok(false);
    }
    // Retain the charge for a full window AFTER the last possible dispatch.
    // The expiry encoded into the ID prevents reuse after bounded garbage collection.
    c.execute(
        "DELETE FROM rest_budget_usage WHERE rowid IN (
          SELECT rowid FROM rest_budget_usage WHERE consumed_at <= ?1 AND expires_at <= ?1 ORDER BY consumed_at, rowid LIMIT 100)",
        params![amount_i64(now, "time")?.saturating_sub(WINDOW_MS as i64)],
    ).map_err(sql)?;
    let class = match req.class {
        BudgetClass::NewRisk => "risk",
        BudgetClass::Exit => "exit",
        BudgetClass::Reconcile => "reconcile",
    };
    c.execute(
        "INSERT INTO rest_budget_usage(caller, request_id, class, weight, consumed_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![caller, req.request_id.as_ref(), class, i64::from(req.weight), amount_i64(now, "time")?, amount_i64(req.expires_at, "expiry")?],
    ).map_err(sql)?;
    Ok(true)
}

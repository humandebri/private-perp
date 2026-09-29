//! One-shot execution permission; never rewrites a send, signature or reservation.
use crate::repo::sql;
use crate::{DbScope, error::Error};
use ic_sqlite_vfs::{
    db::{UpdateConnection, connection::Connection},
    params,
};
use std::{cell::RefCell, collections::BTreeSet};
type Key = (Option<DbScope>, String, [u8; 32]);
thread_local! { static ACTIVE: RefCell<BTreeSet<Key>> = const { RefCell::new(BTreeSet::new()) }; }

pub fn allowed(c: &Connection, kind: &str, id: &[u8; 32]) -> Result<bool, Error> {
    Ok(c.query_optional_scalar::<i64>(
        "SELECT allowed FROM worker_permissions WHERE kind=?1 AND work_id=?2",
        params![kind, id.as_slice()],
    )
    .map_err(sql)?
    .unwrap_or(1)
        == 1)
}
pub fn stop(
    c: &mut UpdateConnection<'_>,
    kind: &str,
    id: &[u8; 32],
    user: &[u8; 32],
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO worker_permissions(kind,work_id,user_id,allowed) VALUES(?1,?2,?3,0)
      ON CONFLICT(kind,work_id) DO UPDATE SET allowed=0",
        params![kind, id.as_slice(), user.as_slice()],
    )
    .map_err(sql)?;
    Ok(())
}
/// Persist consumption before any await. A trap/upgrade leaves permission stopped.
pub fn begin(kind: &str, id: &[u8; 32], user: &[u8; 32]) -> Result<Option<Attempt>, Error> {
    let key = (crate::tx::active_scope(), kind.to_string(), *id);
    if ACTIVE.with(|a| a.borrow().contains(&key)) {
        return Ok(None);
    }
    let generation = crate::tx::update(|c| {
        if !allowed(c, kind, id)? {
            return Ok(None);
        }
        stop(c, kind, id, user)?;
        c.execute("UPDATE worker_permissions SET generation=generation+1 WHERE kind=?1 AND work_id=?2 AND user_id=?3",params![kind,id.as_slice(),user.as_slice()]).map_err(sql)?;
        crate::cas::ensure_changed(crate::cas::changes(c)?, "owner", "changed")?;
        c.query_scalar::<i64>(
            "SELECT generation FROM worker_permissions WHERE kind=?1 AND work_id=?2",
            params![kind, id.as_slice()],
        )
        .map(Some)
        .map_err(sql)
    })?;
    let Some(generation) = generation else {
        return Ok(None);
    };
    ACTIVE.with(|a| a.borrow_mut().insert(key.clone()));
    Ok(Some(Attempt { key, generation }))
}
pub struct Attempt {
    key: Key,
    generation: i64,
}
impl Attempt {
    /// Only normal successful continuation earns another automatic observation.
    pub fn completed(self) -> Result<(), Error> {
        crate::tx::update(|c| {
            c.execute("UPDATE worker_permissions SET allowed=1 WHERE kind=?1 AND work_id=?2 AND generation=?3",params![self.key.1,self.key.2.as_slice(),self.generation]).map_err(sql)?;
            crate::cas::ensure_changed(crate::cas::changes(c)?, "attempt", "changed")
        })
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.borrow_mut().remove(&self.key));
    }
}
#[derive(Debug)]
pub enum ResumeError {
    Database(Error),
    Blocked(&'static str),
}
impl From<Error> for ResumeError {
    fn from(error: Error) -> Self {
        Self::Database(error)
    }
}

/// Check ownership and business eligibility in the same transaction as the grant.
/// Blocked work retains its stopped permission and generation.
pub fn resume(
    kind: &str,
    id: &[u8; 32],
    user: &[u8; 32],
    generation: u64,
) -> Result<(), ResumeError> {
    let key = (crate::tx::active_scope(), kind.to_string(), *id);
    if ACTIVE.with(|a| a.borrow().contains(&key)) {
        return Err(Error::Conflict.into());
    }
    let generation = i64::try_from(generation).map_err(|_| Error::Overflow)?;
    let blocked = crate::tx::update(|c| {
        let owned = c.query_optional_scalar::<i64>(
            "SELECT 1 FROM worker_permissions WHERE kind=?1 AND work_id=?2 AND user_id=?3 AND generation=?4 AND allowed=0",
            params![kind, id.as_slice(), user.as_slice(), generation],
        ).map_err(sql)?.is_some();
        if !owned {
            return Err(Error::Conflict);
        }
        let blocker = match kind {
            "order" => c.query_optional_scalar::<i64>(
                "SELECT 1 FROM orders WHERE order_id=?1 AND preflight_state='unknown'",
                params![id.as_slice()],
            ).map_err(sql)?.map(|_| "レバレッジ設定の結果が不明です。管理者による結果確認・解決が必要です。"),
            "fund" => c.query_optional_scalar::<i64>(
                "SELECT 1 FROM fund_actions WHERE action_id=?1 AND recovery_ambiguous=1",
                params![id.as_slice()],
            ).map_err(sql)?.map(|_| "送金の証跡が曖昧なため再開できません。証跡の調査が必要です。予約と回収フェンスは維持されます。"),
            _ => None,
        };
        if blocker.is_some() {
            return Ok(blocker);
        }
        c.execute("UPDATE worker_permissions SET allowed=1 WHERE kind=?1 AND work_id=?2 AND user_id=?3 AND generation=?4 AND allowed=0",params![kind,id.as_slice(),user.as_slice(),generation]).map_err(sql)?;
        crate::cas::ensure_changed(
            crate::cas::changes(c)?,
            "stopped owner attempt",
            "stale or unauthorized",
        )?;
        Ok(None)
    })?;
    match blocked {
        Some(reason) => Err(ResumeError::Blocked(reason)),
        None => Ok(()),
    }
}
/// Bounded private status, excluding finished business operations.
pub type StoppedWork = (String, Vec<u8>, u64);
pub fn list(c: &Connection, user: &[u8; 32], vault: bool) -> Result<Vec<StoppedWork>, Error> {
    let pending = if vault {
        "((w.kind='fund' AND EXISTS(SELECT 1 FROM fund_actions a JOIN fund_requests r ON r.user_id=a.user_id AND r.client_request_id=a.client_request_id WHERE a.action_id=w.work_id AND r.state NOT IN ('settled','rejected')))
        OR (w.kind='result' AND EXISTS(SELECT 1 FROM pending_transfer_results p WHERE p.action_id=w.work_id))
        OR (w.kind='release' AND EXISTS(SELECT 1 FROM fund_actions a WHERE a.action_id=w.work_id AND a.recovery_fence_epoch IS NOT NULL AND a.recovery_fence_released_at IS NULL)))"
    } else {
        "(w.kind='monitor' OR EXISTS(SELECT 1 FROM orders o WHERE o.order_id=w.work_id AND o.state IN ('pending','open','partially_filled','unknown') AND (w.kind='order' OR (w.kind='cancel' AND o.cancel_requested=1))))"
    };
    let rows=c.query_all(&format!("SELECT w.kind,w.work_id,w.generation FROM worker_permissions w WHERE w.user_id=?1 AND w.allowed=0 AND {pending} ORDER BY w.kind,w.work_id LIMIT 100"),params![user.as_slice()],|r|Ok((r.get::<String>(0)?,r.get::<Vec<u8>>(1)?,r.get::<i64>(2)?))).map_err(sql)?;
    let mut result = Vec::new();
    for (kind, id, generation) in rows {
        let key = (
            crate::tx::active_scope(),
            kind.clone(),
            id.as_slice()
                .try_into()
                .map_err(|_| Error::Invariant("bad work id"))?,
        );
        if !ACTIVE.with(|a| a.borrow().contains(&key)) {
            result.push((
                kind,
                id,
                u64::try_from(generation).map_err(|_| Error::Overflow)?,
            ));
        }
    }
    Ok(result)
}

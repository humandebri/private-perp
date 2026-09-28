//! Canisterコード向けのトランザクション境界（`Db::update`／`Db::query` のラッパ）。
//!
//! `ic-sqlite-vfs` のクロージャは `DbError` を返す必要があるが、Canister側は
//! ドメインの `Error` を扱いたい。同期メッセージ内でだけ使うことを前提に、
//! ドメインエラーを一時保管して往復させる（awaitを跨がないので競合しない）。
//!
//! **1つの論理操作を1つの `update` で完結させる。** クロージャが `Err` を返すと
//! SQLiteトランザクションはロールバックする（`transaction::run_immediate`）。

use crate::{DbScope, error::Error};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

thread_local! {
    static STASHED: RefCell<Option<Error>> = const { RefCell::new(None) };
    static ACTIVE_SCOPE: RefCell<Option<DbScope>> = const { RefCell::new(None) };
}

struct ScopeGuard(Option<DbScope>);

impl ScopeGuard {
    fn enter(scope: DbScope) -> Self {
        let previous = ACTIVE_SCOPE.with(|active| active.replace(Some(scope)));
        Self(previous)
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        ACTIVE_SCOPE.with(|active| {
            active.replace(self.0);
        });
    }
}

/// Select a database for one synchronous entry point. The scope is restored
/// before another canister message can execute.
pub fn with_scope<T>(scope: DbScope, f: impl FnOnce() -> T) -> T {
    let _scope = ScopeGuard::enter(scope);
    f()
}

/// The scope of the currently executing entrypoint, if one is selected.
pub fn active_scope() -> Option<DbScope> {
    ACTIVE_SCOPE.with(|active| *active.borrow())
}

/// The explicit application administrator is stored once during installation.
/// Reading it uses the policy DB even when called from the vault or core.
pub fn is_application_admin(caller: &[u8]) -> Result<bool, Error> {
    with_scope(DbScope::Policy, || {
        query(crate::repo::policy::administrator)
            .map(|principal| principal.as_deref() == Some(caller))
    })
}

/// Select the right database on each poll of an asynchronous entry point.
/// A global scope must not be held across `await`, since another message may
/// run while the future is suspended.
pub fn with_scope_future<F: Future>(scope: DbScope, future: F) -> ScopedFuture<F> {
    with_optional_scope_future(Some(scope), future)
}

/// Standalone workers use their default DB; combined workers select a scope.
pub fn with_optional_scope_future<F: Future>(scope: Option<DbScope>, future: F) -> ScopedFuture<F> {
    ScopedFuture {
        scope,
        future: Box::pin(future),
    }
}

pub struct ScopedFuture<F: Future> {
    scope: Option<DbScope>,
    future: Pin<Box<F>>,
}

impl<F: Future> Future for ScopedFuture<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let _scope = self.scope.map(ScopeGuard::enter);
        self.future.as_mut().poll(cx)
    }
}

fn stash(error: Error) -> ic_sqlite_vfs::DbError {
    STASHED.with(|slot| {
        let mut slot = slot.borrow_mut();
        debug_assert!(slot.is_none(), "nested transaction in one message");
        *slot = Some(error);
    });
    ic_sqlite_vfs::DbError::Constraint("domain error".to_string())
}

fn take_stashed() -> Error {
    STASHED
        .with(|slot| slot.borrow_mut().take())
        .unwrap_or_else(|| Error::Sql("transaction failed without a domain error".to_string()))
}

/// 書き込みトランザクション。
pub fn update<T>(
    f: impl FnOnce(&mut UpdateConnection<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let scope = ACTIVE_SCOPE.with(|active| *active.borrow());
    let run = |connection: &mut UpdateConnection<'_>| match f(connection) {
        Ok(value) => Ok(value),
        Err(error) => Err(stash(error)),
    };
    let result = match scope {
        Some(scope) => crate::scoped_handle(scope)
            .ok_or(Error::Invariant("scoped database is not initialized"))?
            .update(run),
        None => ic_sqlite_vfs::Db::update(run),
    };
    result.map_err(|_| take_stashed())
}

/// 読み取りクエリ。
pub fn query<T>(f: impl FnOnce(&Connection) -> Result<T, Error>) -> Result<T, Error> {
    let scope = ACTIVE_SCOPE.with(|active| *active.borrow());
    let run = |connection: &Connection| match f(connection) {
        Ok(value) => Ok(value),
        Err(error) => Err(stash(error)),
    };
    let result = match scope {
        Some(scope) => crate::scoped_handle(scope)
            .ok_or(Error::Invariant("scoped database is not initialized"))?
            .query(run),
        None => ic_sqlite_vfs::Db::query(run),
    };
    result.map_err(|_| take_stashed())
}

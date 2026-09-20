//! Canisterコード向けのトランザクション境界（`Db::update`／`Db::query` のラッパ）。
//!
//! `ic-sqlite-vfs` のクロージャは `DbError` を返す必要があるが、Canister側は
//! ドメインの `Error` を扱いたい。同期メッセージ内でだけ使うことを前提に、
//! ドメインエラーを一時保管して往復させる（awaitを跨がないので競合しない）。
//!
//! **1つの論理操作を1つの `update` で完結させる。** クロージャが `Err` を返すと
//! SQLiteトランザクションはロールバックする（`transaction::run_immediate`）。

use crate::error::Error;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use std::cell::RefCell;

thread_local! {
    static STASHED: RefCell<Option<Error>> = const { RefCell::new(None) };
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
    let result = ic_sqlite_vfs::Db::update(|connection| match f(connection) {
        Ok(value) => Ok(value),
        Err(error) => Err(stash(error)),
    });
    result.map_err(|_| take_stashed())
}

/// 読み取りクエリ。
pub fn query<T>(f: impl FnOnce(&Connection) -> Result<T, Error>) -> Result<T, Error> {
    let result = ic_sqlite_vfs::Db::query(|connection| match f(connection) {
        Ok(value) => Ok(value),
        Err(error) => Err(stash(error)),
    });
    result.map_err(|_| take_stashed())
}

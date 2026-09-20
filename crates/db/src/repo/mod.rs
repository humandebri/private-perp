//! 永続化操作。呼び出し側が `Db::update`／`Db::query` のトランザクション内で使う。
//!
//! 1つの論理操作（受付＋予約＋action作成など）は1つの同期トランザクションで
//! 完結させる。`repo` の関数はトランザクションを開かない。

pub mod actions;
pub mod agents;
pub mod auth;
pub mod core_config;
pub mod events;
pub mod funds;
pub mod guard;
pub mod ledger;
pub mod orders;
pub mod policy;

use crate::error::Error;

/// `DbError` をドメインのエラーへ分類する。
pub(crate) fn sql(error: ic_sqlite_vfs::DbError) -> Error {
    crate::error::classify_sql(error.to_string())
}

/// `i64` を非負の `u64` として読む（負値は不変条件違反）。
pub(crate) fn amount_u64(value: i64, what: &'static str) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Invariant(what))
}

/// `u64` を `i64` として保存する（範囲外は拒否）。
pub(crate) fn amount_i64(value: u64, _what: &'static str) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::Overflow)
}

//! 状態CASと変更行数の検査（`Implementation.md` 4.6）。
//!
//! すべての `await` 後の書き込みを「期待する状態のときだけ更新する」形にし、
//! 更新0件なら結果を破棄する。SQLiteの `changes()` で判定する。

use crate::error::Error;
use ic_sqlite_vfs::db::connection::Connection;

/// 直前のUPDATE/DELETEで変更された行数。
pub fn changes(connection: &Connection) -> Result<i64, Error> {
    let no_values: [&dyn ic_sqlite_vfs::db::ToSql; 0] = [];
    connection
        .query_scalar::<i64>("SELECT changes()", &no_values)
        .map_err(|error| Error::Sql(error.to_string()))
}

/// CASが成立したことを確認する。0件なら現在状態を添えて拒否する。
pub fn ensure_changed(changed: i64, expected: &str, actual: &str) -> Result<(), Error> {
    if changed == 0 {
        return Err(Error::StateConflict {
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

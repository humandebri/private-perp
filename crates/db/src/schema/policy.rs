//! `policy_registry` のスキーマ。`docs/phase-0/api-contract.md` 5節。

use crate::Migration;

/// 政策と停止状態。
const POLICY: &str = "
CREATE TABLE policy (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL CHECK (version >= 0),
    markets TEXT NOT NULL,
    operator BLOB
);

CREATE TABLE stop_status (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    stopped INTEGER NOT NULL CHECK (stopped IN (0, 1)),
    reason TEXT,
    since INTEGER
);
";

/// `policy_registry` のMigration一覧。
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: POLICY,
}];

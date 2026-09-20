//! `control_guard` のスキーマ。`docs/phase-0/api-contract.md` 4節。

use crate::Migration;

/// 変更予約と実行記録。
const UPGRADES: &str = "
CREATE TABLE guard_config (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    sns_principal BLOB
);

CREATE TABLE upgrades (
    upgrade_id INTEGER PRIMARY KEY AUTOINCREMENT,
    target BLOB NOT NULL CHECK (length(target) BETWEEN 1 AND 29),
    wasm_hash BLOB NOT NULL CHECK (length(wasm_hash) = 32),
    arg_hash BLOB NOT NULL CHECK (length(arg_hash) = 32),
    scheduled_at INTEGER NOT NULL,
    executable_at INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'executable', 'executing', 'executed', 'cancelled')),
    executed_at INTEGER,
    cancelled_at INTEGER
);

CREATE UNIQUE INDEX upgrades_active_by_target ON upgrades (target)
    WHERE state IN ('pending', 'executable');
";

/// `control_guard` のMigration一覧。
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: UPGRADES,
}];

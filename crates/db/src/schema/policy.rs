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

/// 役割別のprincipal（運営・SNS・guard）。
///
/// `policy` 行の `operator` 列は使わない。`set_operator` が政策行を version 0・
/// markets '' で作ってしまい、「政策行が無ければエラー」というfail-closedの判定を
/// 壊していたため、役割は専用表で持つ。
const POLICY_ROLES: &str = "
CREATE TABLE policy_roles (
    role TEXT PRIMARY KEY NOT NULL CHECK (role IN ('operator', 'sns', 'guard')),
    principal BLOB NOT NULL CHECK (length(principal) BETWEEN 1 AND 29),
    updated_at INTEGER NOT NULL
);
";

/// `policy_registry` のMigration一覧。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: POLICY,
    },
    Migration {
        version: 2,
        sql: POLICY_ROLES,
    },
];

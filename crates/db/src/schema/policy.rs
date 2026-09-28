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

/// 共有REST予算。既存policy/roleデータは変更しない。
const SHARED_BUDGET: &str = "
CREATE TABLE budget_workers (
    role TEXT PRIMARY KEY NOT NULL CHECK(role IN ('vault', 'core')),
    principal BLOB NOT NULL UNIQUE CHECK(length(principal) BETWEEN 1 AND 29)
);
CREATE TABLE rest_budget_config (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    capacity INTEGER NOT NULL CHECK(capacity BETWEEN 4 AND 10000),
    exit_reserve INTEGER NOT NULL CHECK(exit_reserve > 0 AND exit_reserve < capacity),
    recovery_paused INTEGER NOT NULL DEFAULT 0 CHECK(recovery_paused IN (0, 1))
);
CREATE TABLE rest_budget_usage (
    caller BLOB NOT NULL,
    request_id BLOB NOT NULL CHECK(length(request_id) = 32),
    class TEXT NOT NULL CHECK(class IN ('risk', 'exit', 'reconcile')),
    weight INTEGER NOT NULL CHECK(weight > 0),
    consumed_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY(caller, request_id)
);
CREATE INDEX rest_budget_usage_time ON rest_budget_usage(consumed_at);
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
    Migration {
        version: 3,
        sql: SHARED_BUDGET,
    },
    Migration {
        version: 4,
        sql: "CREATE TABLE application_admin (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), principal BLOB NOT NULL CHECK(length(principal) BETWEEN 1 AND 29));",
    },
];

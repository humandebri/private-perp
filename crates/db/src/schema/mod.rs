//! スキーマ（バージョン付きMigration）。
//!
//! `Implementation.md` 4.3・14.1 の表を、Canisterごとの静的なSQLとして定義する。
//! 版は厳密に増加させ、`IF NOT EXISTS` による冪等初期化として書かない
//! （`ic-sqlite-vfs` の `Db::migrate` が適用済み版を記録する）。

pub mod core;
pub mod guard;
pub mod policy;
pub mod send_journal;
pub mod vault;

/// Execution permissions are separate from financial state and send evidence.
pub const MANUAL_WORK: &str = "CREATE TABLE worker_permissions (
 kind TEXT NOT NULL, work_id BLOB NOT NULL CHECK(length(work_id)=32),
 user_id BLOB NOT NULL CHECK(length(user_id)=32),
 generation INTEGER NOT NULL DEFAULT 0,
 allowed INTEGER NOT NULL CHECK(allowed IN (0,1)),
 PRIMARY KEY(kind,work_id));";

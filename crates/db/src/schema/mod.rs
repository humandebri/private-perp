//! スキーマ（バージョン付きMigration）。
//!
//! `Implementation.md` 4.3・14.1 の表を、Canisterごとの静的なSQLとして定義する。
//! 版は厳密に増加させ、`IF NOT EXISTS` による冪等初期化として書かない
//! （`ic-sqlite-vfs` の `Db::migrate` が適用済み版を記録する）。

pub mod core;
pub mod guard;
pub mod vault;

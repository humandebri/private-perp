//! 永続化ラッパ。`ic-sqlite-vfs` だけを使う。
//!
//! `Implementation.md` 3.2・4章に従い、次を守る。
//!
//! - **非async。** トランザクション内で `await`・inter-canister call・
//!   `ic0.call_perform` を跨がない（`scripts/check-no-await.sh` でCI検査する）。
//! - 任意のSQLを外部入力から組み立てない。値はbindする。
//! - `SQLite` の `random()`・`randomblob()` を使わない（このVFSでは決定的）。
//! - Migrationは静的なSQLで、版を厳密に増加させる（`IF NOT EXISTS` の
//!   冪等初期化として書かない）。
//!
//! Canisterごとにスキーマが異なるため、`init` はそのCanisterのMigration一覧を
//! 受け取る。`repo` と `cas` は `ic-sqlite-vfs` の型に依存するためwasm専用である。
#![forbid(unsafe_code)]

pub mod error;
pub mod schema;
pub mod states;

#[cfg(target_family = "wasm")]
pub mod cas;
#[cfg(target_family = "wasm")]
pub mod repo;
#[cfg(target_family = "wasm")]
pub mod tx;

/// CanisterごとのMemoryId割当（`Implementation.md` 4.2）。
///
/// デプロイ済みCanisterの寿命の間、変更しない。255は同梱MemoryManager互換
/// レイアウトが予約しているため、アプリは `0..=254` のみを使う。
pub mod memory_id {
    /// `trading_core` のメインDB（users, agents, orders, order_events, nonces, audit）。
    pub const TRADING_CORE_MAIN: u8 = 0;
    /// `trading_core` の予約スロット（将来の独立イメージ。slot catalogに記録する）。
    pub const TRADING_CORE_ARCHIVE: u8 = 1;
    /// `policy_registry` のpolicy DB。
    pub const POLICY_REGISTRY: u8 = 120;
    /// `funds_vault` の認証・資金台帳・資金outbox（別Canisterなので0から始まる）。
    pub const FUNDS_VAULT_MAIN: u8 = 0;
    /// `control_guard` の変更予約・実行記録（別Canister）。
    pub const CONTROL_GUARD_MAIN: u8 = 0;
    /// アプリが使えるMemoryIdの上限。255は予約済み。
    pub const MAX_APP_MEMORY_ID: u8 = 254;
}

/// バージョン付きのMigrationステップ（`Implementation.md` 4.7）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Migration {
    pub version: u64,
    pub sql: &'static str,
}

/// DBとMigrationを初期化する。`#[ic_cdk::init]` と `#[ic_cdk::post_upgrade]` の
/// 両方で、MigrationやDBアクセスの前に呼ぶ（`Implementation.md` 4.2）。
///
/// ホストビルドでは `ic-sqlite-vfs` を依存させないため、MemoryIdの検査だけを行う。
pub fn init(id: u8, migrations: &[Migration]) -> Result<(), String> {
    if id > memory_id::MAX_APP_MEMORY_ID {
        return Err(format!(
            "memory id {id} is reserved (max {})",
            memory_id::MAX_APP_MEMORY_ID
        ));
    }

    #[cfg(target_family = "wasm")]
    {
        wasm::init(id, migrations)?;
    }

    #[cfg(not(target_family = "wasm"))]
    {
        let _ = migrations;
    }

    Ok(())
}

#[cfg(target_family = "wasm")]
mod wasm {
    use super::Migration;
    use ic_sqlite_vfs::db::migrate::Migration as VfsMigration;
    use ic_sqlite_vfs::{Db, DefaultMemoryImpl, MemoryId, MemoryManager};
    use std::cell::RefCell;

    thread_local! {
        static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
            RefCell::new(
                MemoryManager::init_strict(DefaultMemoryImpl::default())
                    .expect("stable memory must either be empty or use the MemoryManager layout"),
            );
    }

    pub(super) fn init(id: u8, migrations: &[Migration]) -> Result<(), String> {
        MEMORY_MANAGER.with(|manager| {
            Db::init(manager.borrow().get(MemoryId::new(id))).map_err(|error| error.to_string())?;
            Db::migrate(&convert_all(migrations)).map_err(|error| error.to_string())
        })
    }

    fn convert_all(migrations: &[Migration]) -> Vec<VfsMigration> {
        migrations.iter().copied().map(convert).collect()
    }

    fn convert(migration: Migration) -> VfsMigration {
        VfsMigration {
            version: migration.version,
            sql: migration.sql,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Migration, init, memory_id, schema};

    fn all_migrations() -> Vec<(&'static str, &'static [Migration])> {
        vec![
            ("vault", schema::vault::MIGRATIONS),
            ("core", schema::core::MIGRATIONS),
        ]
    }

    #[test]
    fn migration_versions_increase_strictly() {
        for (name, migrations) in all_migrations() {
            for pair in migrations.windows(2) {
                assert!(
                    pair[0].version < pair[1].version,
                    "{name}: migration versions must increase strictly: {} then {}",
                    pair[0].version,
                    pair[1].version
                );
            }
            assert_eq!(
                migrations.first().map(|migration| migration.version),
                Some(1),
                "{name}: 最初のmigrationは版1にする"
            );
        }
    }

    #[test]
    fn migration_sql_is_static_and_not_idempotent_init() {
        for (name, migrations) in all_migrations() {
            for migration in migrations {
                assert!(
                    !migration.sql.trim().is_empty(),
                    "{name}: migration {} has empty SQL",
                    migration.version
                );
                assert!(
                    migration.sql.trim_start().starts_with("CREATE TABLE"),
                    "{name}: migration {} must start with CREATE TABLE",
                    migration.version
                );
                assert!(
                    !migration.sql.to_uppercase().contains("IF NOT EXISTS"),
                    "{name}: migration {} must not use IF NOT EXISTS",
                    migration.version
                );
            }
        }
    }

    #[test]
    fn schema_covers_the_security_relevant_tables() {
        let vault = schema::vault::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        for table in [
            "challenges",
            "sessions",
            "journals",
            "postings",
            "fund_requests",
            "reservations",
            "fund_actions",
            "master_nonces",
            "external_events",
            "key_registry",
        ] {
            assert!(vault.contains(table), "vault schema lacks {table}");
        }

        let core = schema::core::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        for table in [
            "agents",
            "requests",
            "actions",
            "orders",
            "nonces",
            "meta_cache",
        ] {
            assert!(core.contains(table), "core schema lacks {table}");
        }
    }

    #[test]
    fn action_state_constraint_requires_signature_before_dispatch() {
        let vault = schema::vault::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        assert!(
            vault.contains("dispatch_state IN ('dispatching', 'reconciled', 'unknown') AND signature IS NOT NULL"),
            "fund_actions must require a signature before dispatch"
        );
    }

    #[test]
    fn reserved_memory_id_is_rejected() {
        assert!(init(memory_id::MAX_APP_MEMORY_ID, &[]).is_ok());
        assert!(init(255, &[]).is_err());
    }

    #[test]
    fn memory_ids_are_within_the_application_range() {
        for id in [
            memory_id::TRADING_CORE_MAIN,
            memory_id::TRADING_CORE_ARCHIVE,
            memory_id::POLICY_REGISTRY,
            memory_id::FUNDS_VAULT_MAIN,
            memory_id::CONTROL_GUARD_MAIN,
        ] {
            assert!(id <= memory_id::MAX_APP_MEMORY_ID);
        }
    }
}

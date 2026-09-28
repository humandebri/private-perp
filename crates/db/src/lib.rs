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
    /// `trading_core` のメインDB（users, accounts, orders, nonces）。
    pub const TRADING_CORE_MAIN: u8 = 0;
    /// `trading_core` の予約スロット（将来の独立イメージ。slot catalogに記録する）。
    pub const TRADING_CORE_ARCHIVE: u8 = 1;
    /// `policy_registry` のpolicy DB。
    pub const POLICY_REGISTRY: u8 = 120;
    /// `funds_vault` の認証・資金台帳・資金outbox（別Canisterなので0から始まる）。
    pub const FUNDS_VAULT_MAIN: u8 = 0;
    /// `control_guard` の変更予約・実行記録（別Canister）。
    pub const CONTROL_GUARD_MAIN: u8 = 0;
    /// 独立した送信意図の追記専用ジャーナル。vault/coreのbackup対象外。
    pub const SEND_JOURNAL_MAIN: u8 = 0;
    /// アプリが使えるMemoryIdの上限。255は予約済み。
    pub const MAX_APP_MEMORY_ID: u8 = 254;
}

/// Separate databases owned by one application Canister. Keeping the existing
/// schemas in distinct stable-memory slots avoids table-name collisions during
/// the single-Canister transition.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DbScope {
    Vault,
    Core,
    Policy,
    Guard,
    Journal,
}

impl DbScope {
    pub const fn memory_id(self) -> u8 {
        match self {
            Self::Vault => 0,
            Self::Core => 1,
            Self::Policy => 2,
            Self::Guard => 3,
            Self::Journal => 4,
        }
    }
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

/// Initialize one database in a combined Canister. Each scope has a permanent
/// MemoryId; the existing single-Canister `init` API remains available while
/// callers are migrated to scoped transactions.
pub fn init_scoped(scope: DbScope, migrations: &[Migration]) -> Result<(), String> {
    #[cfg(target_family = "wasm")]
    {
        wasm::init_scoped(scope, migrations)?;
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = (scope, migrations);
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
mod wasm {
    use super::{DbScope, Migration};
    use ic_sqlite_vfs::db::migrate::Migration as VfsMigration;
    use ic_sqlite_vfs::{Db, DbHandle, DefaultMemoryImpl, MemoryId, MemoryManager};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
            RefCell::new(
                MemoryManager::init_strict(DefaultMemoryImpl::default())
                    .expect("stable memory must either be empty or use the MemoryManager layout"),
            );
        static SCOPED: RefCell<BTreeMap<DbScope, DbHandle>> = const { RefCell::new(BTreeMap::new()) };
    }

    pub(super) fn init_scoped(scope: DbScope, migrations: &[Migration]) -> Result<(), String> {
        let handle = MEMORY_MANAGER.with(|manager| {
            DbHandle::init(manager.borrow().get(MemoryId::new(scope.memory_id())))
                .map_err(|error| error.to_string())
        })?;
        handle
            .migrate(&convert_all(migrations))
            .map_err(|error| error.to_string())?;
        SCOPED.with(|handles| {
            handles.borrow_mut().insert(scope, handle);
        });
        Ok(())
    }

    pub(super) fn scoped_handle(scope: DbScope) -> Option<DbHandle> {
        SCOPED.with(|handles| handles.borrow().get(&scope).copied())
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

#[cfg(target_family = "wasm")]
pub(crate) fn scoped_handle(scope: DbScope) -> Option<ic_sqlite_vfs::DbHandle> {
    wasm::scoped_handle(scope)
}

#[cfg(test)]
mod tests {
    use super::{DbScope, Migration, init, memory_id, schema};

    #[test]
    fn combined_canister_scopes_have_distinct_memory_ids() {
        let ids = [
            DbScope::Vault,
            DbScope::Core,
            DbScope::Policy,
            DbScope::Guard,
            DbScope::Journal,
        ]
        .map(DbScope::memory_id);
        let distinct = ids.into_iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(distinct.len(), ids.len());
        assert!(ids.iter().all(|id| *id <= memory_id::MAX_APP_MEMORY_ID));
    }

    fn all_migrations() -> Vec<(&'static str, &'static [Migration])> {
        vec![
            ("vault", schema::vault::MIGRATIONS),
            ("core", schema::core::MIGRATIONS),
            ("guard", schema::guard::MIGRATIONS),
            ("policy", schema::policy::MIGRATIONS),
            ("send_journal", schema::send_journal::MIGRATIONS),
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
                // 初期schemaは表を作る。後続migrationはALTER TABLEなどで
                // 既存schemaを更新するため、CREATE TABLEでの開始を要求しない。
                if migration.version == 1 {
                    assert!(
                        migration.sql.trim_start().starts_with("CREATE TABLE"),
                        "{name}: initial migration must start with CREATE TABLE"
                    );
                }
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
        // 表名は `CREATE TABLE <name> (` の形で探す。部分文字列一致だと
        // "agents" が "agent_generations" に、といった誤検出で実表の欠落を見逃す。
        fn assert_tables(schema: &str, name: &str, tables: &[&str]) {
            for table in tables {
                let needle = format!("CREATE TABLE {table} (");
                assert!(schema.contains(&needle), "{name} schema lacks {needle}");
            }
        }

        let vault = schema::vault::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        assert_tables(
            &vault,
            "vault",
            &[
                "identities",
                "challenges",
                "sessions",
                "custody_accounts",
                "accounts",
                "journals",
                "postings",
                "fund_requests",
                "reservations",
                "fund_actions",
                "action_events",
                "master_nonces",
                "external_events",
                "key_registry",
                "audit",
                "agent_generations",
                "hpke_keys",
                "reconcile_cursor",
                "used_intent_nonces",
                "journal_requests",
            ],
        );

        let core = schema::core::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        assert_tables(
            &core,
            "core",
            &[
                "users",
                "accounts",
                "agents",
                "requests",
                "actions",
                "orders",
                "action_orders",
                "order_events",
                "risk_reservations",
                "meta_cache",
                "core_config",
                "agent_generations",
                "fills",
                "account_observations",
            ],
        );

        let guard = schema::guard::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        assert_tables(&guard, "guard", &["guard_config", "upgrades"]);

        let policy = schema::policy::MIGRATIONS
            .iter()
            .map(|migration| migration.sql)
            .collect::<String>();
        assert_tables(
            &policy,
            "policy",
            &[
                "policy",
                "stop_status",
                "policy_roles",
                "budget_workers",
                "rest_budget_config",
                "rest_budget_usage",
            ],
        );
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

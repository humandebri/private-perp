//! 永続化ラッパ。`ic-sqlite-vfs` だけを使う。
//!
//! `Implementation.md` 3.2・4章に従い、次を守る。
//!
//! - **非async。** トランザクション内で `await`・inter-canister call・
//!   `ic0.call_perform` を跨がない（`scripts/check-no-await.sh` でCI検査する）。
//! - 任意のSQLを外部入力から組み立てない。値はbindする。
//! - `SQLite` の `random()`・`randomblob()` を使わない（このVFSでは決定的）。
//!
//! Phase 0 は骨格のみである。実テーブルとMigrationは Phase 2-1、疎通試験は
//! Phase 1（`Implementation.md` 1-7）で実装する。
#![forbid(unsafe_code)]

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
///
/// `IF NOT EXISTS` による冪等初期化として書かず、厳密に増加する版として扱う。
/// migration SQLは静的に保ち、実行時に組み立てない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Migration {
    pub version: u64,
    pub sql: &'static str,
}

/// 適用するMigration。Phase 0 時点では実テーブルがないため空である。
///
/// 実テーブルは Phase 2-1（`Implementation.md` 2-1）で追加する。
pub const MIGRATIONS: &[Migration] = &[];

/// DBとMigrationを初期化する。`#[ic_cdk::init]` と `#[ic_cdk::post_upgrade]` の
/// 両方で、MigrationやDBアクセスの前に呼ぶ（`Implementation.md` 4.2）。
///
/// ホストビルドでは `ic-sqlite-vfs` を依存させないため、何もせず `Ok(())` を返す。
/// 実際の初期化は wasm32 ビルドでのみ行う。
pub fn init(id: u8) -> Result<(), String> {
    if id > memory_id::MAX_APP_MEMORY_ID {
        return Err(format!(
            "memory id {id} is reserved (max {})",
            memory_id::MAX_APP_MEMORY_ID
        ));
    }

    #[cfg(target_family = "wasm")]
    {
        wasm::init(id)?;
    }

    Ok(())
}

#[cfg(target_family = "wasm")]
mod wasm {
    use super::{MIGRATIONS, Migration};
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

    pub(super) fn init(id: u8) -> Result<(), String> {
        MEMORY_MANAGER.with(|manager| {
            Db::init(manager.borrow().get(MemoryId::new(id))).map_err(|error| error.to_string())?;
            Db::migrate(&migrations()).map_err(|error| error.to_string())
        })
    }

    fn migrations() -> Vec<VfsMigration> {
        MIGRATIONS.iter().map(convert).collect()
    }

    fn convert(migration: &Migration) -> VfsMigration {
        VfsMigration {
            version: migration.version,
            sql: migration.sql,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MIGRATIONS, Migration, init, memory_id};

    #[test]
    fn migration_versions_increase_strictly() {
        for pair in MIGRATIONS.windows(2) {
            assert!(
                pair[0].version < pair[1].version,
                "migration versions must increase strictly: {} then {}",
                pair[0].version,
                pair[1].version
            );
        }
    }

    #[test]
    fn migration_sql_is_not_empty() {
        for migration in MIGRATIONS {
            assert!(
                !migration.sql.trim().is_empty(),
                "migration {} has empty SQL",
                migration.version
            );
        }
    }

    #[test]
    fn reserved_memory_id_is_rejected() {
        assert!(init(memory_id::MAX_APP_MEMORY_ID).is_ok());
        assert!(init(255).is_err());
    }

    #[test]
    fn memory_ids_are_within_the_application_range() {
        let ids = [
            memory_id::TRADING_CORE_MAIN,
            memory_id::TRADING_CORE_ARCHIVE,
            memory_id::POLICY_REGISTRY,
            memory_id::FUNDS_VAULT_MAIN,
            memory_id::CONTROL_GUARD_MAIN,
        ];
        for id in ids {
            assert!(id <= memory_id::MAX_APP_MEMORY_ID);
        }
    }

    #[test]
    fn migration_type_is_constructible() {
        let migration = Migration {
            version: 1,
            sql: "CREATE TABLE example (id INTEGER PRIMARY KEY)",
        };
        assert_eq!(migration.version, 1);
    }
}

//! 変更予約の永続化。`Implementation.md` 14.4、`docs/phase-0/api-contract.md` 4節。

use crate::error::Error;
use crate::repo::sql;
use api_types::Timestamp;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 予約されたupgrade。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeRow {
    pub upgrade_id: i64,
    pub target: Vec<u8>,
    pub wasm_hash: [u8; 32],
    pub arg_hash: [u8; 32],
    pub scheduled_at: Timestamp,
    pub executable_at: Timestamp,
    pub state: String,
}

type RawUpgrade = (i64, Vec<u8>, Vec<u8>, Vec<u8>, i64, i64, String);

fn convert(raw: RawUpgrade) -> Result<UpgradeRow, Error> {
    Ok(UpgradeRow {
        upgrade_id: raw.0,
        target: raw.1,
        wasm_hash: raw
            .2
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte wasm hash"))?,
        arg_hash: raw
            .3
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte arg hash"))?,
        scheduled_at: u64::try_from(raw.4).map_err(|_| Error::Invariant("negative timestamp"))?,
        executable_at: u64::try_from(raw.5).map_err(|_| Error::Invariant("negative timestamp"))?,
        state: raw.6,
    })
}

fn read_upgrade(row: &ic_sqlite_vfs::db::Row<'_>) -> Result<RawUpgrade, ic_sqlite_vfs::DbError> {
    Ok((
        row.get::<i64>(0)?,
        row.get::<Vec<u8>>(1)?,
        row.get::<Vec<u8>>(2)?,
        row.get::<Vec<u8>>(3)?,
        row.get::<i64>(4)?,
        row.get::<i64>(5)?,
        row.get::<String>(6)?,
    ))
}

const COLUMNS: &str = "upgrade_id, target, wasm_hash, arg_hash, scheduled_at, executable_at, state";

/// SNS governanceのPrincipalを設定する（初期化時にcontrollerが1度だけ行う）。
pub fn set_sns_principal(
    connection: &mut UpdateConnection<'_>,
    principal: &[u8],
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO guard_config (singleton, sns_principal) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET sns_principal = excluded.sns_principal",
            params![principal],
        )
        .map_err(sql)
}

/// SNS governanceのPrincipal。
pub fn sns_principal(connection: &Connection) -> Result<Option<Vec<u8>>, Error> {
    let value = connection
        .query_optional_scalar::<Vec<u8>>(
            "SELECT sns_principal FROM guard_config WHERE singleton = 1",
            params![],
        )
        .map_err(sql)?;
    Ok(value)
}

/// 予約を追加する（対象ごとに1件のみ）。
pub fn insert_upgrade(
    connection: &mut UpdateConnection<'_>,
    target: &[u8],
    wasm_hash: &[u8; 32],
    arg_hash: &[u8; 32],
    scheduled_at: Timestamp,
    executable_at: Timestamp,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO upgrades (target, wasm_hash, arg_hash, scheduled_at, executable_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            params![
                target,
                wasm_hash.as_slice(),
                arg_hash.as_slice(),
                scheduled_at as i64,
                executable_at as i64
            ],
        )
        .map_err(sql)
}

/// 対象の有効な予約（pending／executable）を返す。
///
/// 対象を指定せずに最古の1件を実行すると、複数対象の予約が競合したときに
/// 実行対象が不定になる（`upgrades_active_by_target` は対象ごとに1件を許す）。
pub fn active_upgrade_for_target(
    connection: &Connection,
    target: &[u8],
) -> Result<Option<UpgradeRow>, Error> {
    let raw = connection
        .query_optional(
            &format!("SELECT {COLUMNS} FROM upgrades WHERE target = ?1 AND state IN ('pending', 'executable') ORDER BY upgrade_id LIMIT 1"),
            params![target],
            read_upgrade,
        )
        .map_err(sql)?;
    raw.map(convert).transpose()
}

/// 直近の予約（状態を問わない）。実行済み・取消済みも状態表示に含める。
pub fn latest_upgrade(connection: &Connection) -> Result<Option<UpgradeRow>, Error> {
    let raw = connection
        .query_optional(
            &format!("SELECT {COLUMNS} FROM upgrades ORDER BY upgrade_id DESC LIMIT 1"),
            params![],
            read_upgrade,
        )
        .map_err(sql)?;
    raw.map(convert).transpose()
}

/// 対象の有効な予約を取消す。
pub fn cancel_active_for_target(
    connection: &mut UpdateConnection<'_>,
    target: &[u8],
    now: Timestamp,
) -> Result<bool, Error> {
    connection
        .execute(
            "UPDATE upgrades SET state = 'cancelled', cancelled_at = ?2
              WHERE target = ?1 AND state IN ('pending', 'executable')",
            params![target, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    Ok(changed > 0)
}

/// 実行権を予約へ戻す（`install_code`が失敗し、対象が実際には置き換わっていない場合）。
///
/// 猶予は既に経過しているため、新しい7日を待たずに再実行できる。
pub fn revert_executing(
    connection: &mut UpdateConnection<'_>,
    upgrade_id: i64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE upgrades SET state = 'executable' WHERE upgrade_id = ?1 AND state = 'executing'",
            params![upgrade_id],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "executing", "not executing")
}

/// 実行済みにする。
pub fn mark_executed(
    connection: &mut UpdateConnection<'_>,
    upgrade_id: i64,
    now: Timestamp,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE upgrades SET state = 'executed', executed_at = ?2 WHERE upgrade_id = ?1 AND state = 'executing'",
            params![upgrade_id, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    if changed == 0 {
        return Err(Error::StateConflict {
            expected: "pending or executable".to_string(),
            actual: "executed or cancelled".to_string(),
        });
    }
    Ok(())
}

/// 実行権を取得する（`install_code`の**前**に呼ぶ。単一の実行者のみtrue）。
pub fn claim_upgrade(
    connection: &mut UpdateConnection<'_>,
    upgrade_id: i64,
) -> Result<bool, Error> {
    connection
        .execute(
            "UPDATE upgrades SET state = 'executing'
              WHERE upgrade_id = ?1 AND state IN ('pending', 'executable')",
            params![upgrade_id],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    Ok(changed > 0)
}

//! 資金要求の受付・冪等性・予約。`docs/phase-0/api-contract.md` 2.2、7節。

use crate::error::Error;
use crate::repo::{amount_i64, sql};
use crate::states::fund_request_state_str;
use api_types::fund::FundRequestState;
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// 資金要求の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Allocation,
    Recovery,
    Withdrawal,
}

impl RequestKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allocation => "allocation",
            Self::Recovery => "recovery",
            Self::Withdrawal => "withdrawal",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        Some(match value {
            "allocation" => Self::Allocation,
            "recovery" => Self::Recovery,
            "withdrawal" => Self::Withdrawal,
            _ => return None,
        })
    }
}

/// 受付の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptOutcome {
    /// 新規に受付けた。
    Accepted,
    /// 同一ID・同一本文の再送。既存の結果を返す。
    Duplicate,
    /// 同一ID・異なる本文。拒否する。
    Conflict,
}

/// 受付済みの資金要求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundRequestRow {
    pub kind: RequestKind,
    pub state: FundRequestState,
    pub amount: u64,
    pub account_id: Option<[u8; 32]>,
    pub destination: Option<String>,
    pub body_hash: [u8; 32],
}

/// 受付済みの要求を引く。
pub fn fund_request(
    connection: &Connection,
    user_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<Option<FundRequestRow>, Error> {
    let raw = connection
        .query_optional(
            "SELECT kind, state, amount, account_id, destination, body_hash
               FROM fund_requests WHERE user_id = ?1 AND client_request_id = ?2",
            params![user_id.as_slice(), client_request_id],
            |row| {
                Ok((
                    row.get::<String>(0)?,
                    row.get::<String>(1)?,
                    row.get::<i64>(2)?,
                    row.get::<Option<Vec<u8>>>(3)?,
                    row.get::<Option<String>>(4)?,
                    row.get::<Vec<u8>>(5)?,
                ))
            },
        )
        .map_err(sql)?;

    raw.map(
        |(kind, state, amount, account_id, destination, body_hash)| {
            Ok(FundRequestRow {
                kind: RequestKind::from_db_str(&kind)
                    .ok_or(Error::Invariant("unknown request kind"))?,
                state: crate::states::fund_request_state_from_str(&state)
                    .ok_or(Error::Invariant("unknown request state"))?,
                amount: u64::try_from(amount).map_err(|_| Error::Invariant("negative amount"))?,
                account_id: match account_id {
                    Some(bytes) => Some(
                        bytes
                            .try_into()
                            .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
                    ),
                    None => None,
                },
                destination,
                body_hash: body_hash
                    .try_into()
                    .map_err(|_| Error::Invariant("expected a 32-byte body hash"))?,
            })
        },
    )
    .transpose()
}

/// 要求の現在状態。
pub fn request_state(
    connection: &Connection,
    user_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<Option<FundRequestState>, Error> {
    let state = connection
        .query_optional_scalar::<String>(
            "SELECT state FROM fund_requests WHERE user_id = ?1 AND client_request_id = ?2",
            params![user_id.as_slice(), client_request_id],
        )
        .map_err(sql)?;
    state
        .map(|value| {
            crate::states::fund_request_state_from_str(&value)
                .ok_or(Error::Invariant("unknown request state"))
        })
        .transpose()
}

/// 新規の資金要求。
#[derive(Debug, Clone, Copy)]
pub struct NewFundRequest<'a> {
    pub user_id: &'a [u8; 32],
    pub client_request_id: &'a [u8],
    pub body_hash: &'a [u8; 32],
    pub kind: RequestKind,
    pub account_id: Option<&'a [u8; 32]>,
    pub amount: u64,
    pub destination: Option<&'a str>,
}

/// 要求を受付ける。同一ID・同一本文は再送として扱い、異なる本文は拒否する。
pub fn accept_fund_request(
    connection: &mut UpdateConnection<'_>,
    request: &NewFundRequest<'_>,
    now: u64,
) -> Result<AcceptOutcome, Error> {
    let NewFundRequest {
        user_id,
        client_request_id,
        body_hash,
        kind,
        account_id,
        amount,
        destination,
    } = *request;
    if let Some(existing) = fund_request(connection, user_id, client_request_id)? {
        return Ok(if existing.body_hash == *body_hash {
            AcceptOutcome::Duplicate
        } else {
            AcceptOutcome::Conflict
        });
    }

    let amount = amount_i64(amount, "amount out of range")?;
    let state = fund_request_state_str(FundRequestState::Accepted);
    let account_value = match account_id {
        Some(value) => ic_sqlite_vfs::db::Value::Blob(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let destination_value = match destination {
        Some(value) => ic_sqlite_vfs::db::Value::Text(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO fund_requests
               (user_id, client_request_id, body_hash, kind, account_id, amount, destination, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                user_id.as_slice(),
                client_request_id,
                body_hash.as_slice(),
                kind.as_str(),
                account_value,
                amount,
                destination_value,
                state,
                now as i64
            ],
        )
        .map_err(sql)?;
    Ok(AcceptOutcome::Accepted)
}

/// 要求の状態を更新する。
pub fn set_request_state(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    client_request_id: &[u8],
    state: FundRequestState,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE fund_requests SET state = ?3, updated_at = ?4
              WHERE user_id = ?1 AND client_request_id = ?2",
            params![
                user_id.as_slice(),
                client_request_id,
                fund_request_state_str(state),
                now as i64
            ],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "existing request", "missing request")
}

/// 保持中の予約合計（未解放）。
pub fn held_reservation_total(connection: &Connection, user_id: &[u8; 32]) -> Result<u64, Error> {
    let total = connection
        .query_scalar::<i64>(
            "SELECT COALESCE(SUM(amount), 0) FROM reservations
              WHERE user_id = ?1 AND state = 'held'",
            params![user_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(total).map_err(|_| Error::Invariant("negative reservation total"))
}

/// 配分（allocation）で拘束中の合計。
///
/// 出金の拘束は台帳側（`user_reserved_for_withdrawal`）にあるため含めない。
/// 同じ額を二度引かないための境界である。
pub fn held_allocation_total(connection: &Connection, user_id: &[u8; 32]) -> Result<u64, Error> {
    let total = connection
        .query_scalar::<i64>(
            "SELECT COALESCE(SUM(r.amount), 0)
               FROM reservations r
               JOIN fund_requests f
                 ON f.user_id = r.user_id AND f.client_request_id = r.client_request_id
              WHERE r.user_id = ?1 AND r.state = 'held' AND f.kind = 'allocation'",
            params![user_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(total).map_err(|_| Error::Invariant("negative reservation total"))
}

/// 回収（recovery）で拘束中の合計（取引口座のequityに対する拘束）。
pub fn held_recovery_total(connection: &Connection, user_id: &[u8; 32]) -> Result<u64, Error> {
    let total = connection
        .query_scalar::<i64>(
            "SELECT COALESCE(SUM(r.amount), 0)
               FROM reservations r
               JOIN fund_requests f
                 ON f.user_id = r.user_id AND f.client_request_id = r.client_request_id
              WHERE r.user_id = ?1 AND r.state = 'held' AND f.kind = 'recovery'",
            params![user_id.as_slice()],
        )
        .map_err(sql)?;
    u64::try_from(total).map_err(|_| Error::Invariant("negative reservation total"))
}

/// 取引口座のequityに対して残高を拘束する（回収の受付）。
///
/// 回収は取引口座から出るため、準備口座の残高ではなく取引口座のequityと比較する。
/// 拘束しないと、同じequityに対して複数の回収が同時に送信され得る。
pub fn reserve_trading_funds(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    client_request_id: &[u8],
    account_name: &str,
    amount: u64,
    now: u64,
) -> Result<(), Error> {
    let balances = crate::repo::ledger::user_balances(connection, user_id)?;
    let available = balances
        .trading_equity
        .checked_sub(held_recovery_total(connection, user_id)?)
        .ok_or(Error::Invariant("recovery holds exceed the trading equity"))?;
    if available < amount {
        return Err(Error::InsufficientFunds {
            available: i64::try_from(available).unwrap_or(i64::MAX),
            requested: i64::try_from(amount).unwrap_or(i64::MAX),
        });
    }

    let amount = amount_i64(amount, "amount out of range")?;
    connection
        .execute(
            "INSERT INTO reservations (user_id, client_request_id, account_name, amount, state, created_at, released_at)
             VALUES (?1, ?2, ?3, ?4, 'held', ?5, NULL)",
            params![
                user_id.as_slice(),
                client_request_id,
                account_name,
                amount,
                now as i64
            ],
        )
        .map_err(sql)
}
/// 残高を拘束する。未配分残高が不足する場合は拒否する。
///
/// 出金の拘束は `withdrawal_reserve` が台帳で行うため、ここで引くのは配分の拘束だけ。
pub fn reserve_funds(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    client_request_id: &[u8],
    account_name: &str,
    amount: u64,
    now: u64,
) -> Result<(), Error> {
    let balances = crate::repo::ledger::user_balances(connection, user_id)?;
    let available = balances
        .reserve_unallocated
        .checked_sub(held_allocation_total(connection, user_id)?)
        .ok_or(Error::Invariant("allocation holds exceed the balance"))?;
    if available < amount {
        return Err(Error::InsufficientFunds {
            available: i64::try_from(available).unwrap_or(i64::MAX),
            requested: i64::try_from(amount).unwrap_or(i64::MAX),
        });
    }

    let amount = amount_i64(amount, "amount out of range")?;
    connection
        .execute(
            "INSERT INTO reservations (user_id, client_request_id, account_name, amount, state, created_at, released_at)
             VALUES (?1, ?2, ?3, ?4, 'held', ?5, NULL)",
            params![
                user_id.as_slice(),
                client_request_id,
                account_name,
                amount,
                now as i64
            ],
        )
        .map_err(sql)
}

/// 予約を解放する（未送信が保証できる場合のみ）。
pub fn release_reservation(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    client_request_id: &[u8],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE reservations SET state = 'released', released_at = ?3
              WHERE user_id = ?1 AND client_request_id = ?2 AND state = 'held'",
            params![user_id.as_slice(), client_request_id, now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(
        changed,
        "held reservation",
        "missing or released reservation",
    )
}

/// 予約を消費する（外部効果が確定した場合）。
pub fn consume_reservation(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    client_request_id: &[u8],
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE reservations SET state = 'consumed'
              WHERE user_id = ?1 AND client_request_id = ?2 AND state = 'held'",
            params![user_id.as_slice(), client_request_id],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(
        changed,
        "held reservation",
        "missing or released reservation",
    )
}

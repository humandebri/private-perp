//! 複式台帳。`docs/phase-0/money-and-units.md` 4節、`state-machines.md` 3節。
//!
//! - 仕訳（journal）は借方と貸方の合計が0になる符号付きpostingsで表す
//!   （正=借方、負=貸方）。
//! - 残高はpostingsから導出し、キャッシュ残高を持たない。
//! - 同じjournal内で同じ勘定を2回使わない（`PRIMARY KEY (journal_id, account_id)`）。
//! - 金額は `i64` マイクロUSDCで、checked演算によりオーバーフローを拒否する。

use crate::error::Error;
use crate::repo::{amount_u64, sql};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// サービスが保有する資産の勘定。
pub const CASH_RESERVE: &str = "cash_reserve";
pub const CASH_TRADING: &str = "cash_trading";
pub const CASH_IN_TRANSIT: &str = "cash_in_transit";
/// 外部（入金元・出金先）を表すsuspense勘定。
pub const EXTERNAL: &str = "external";
/// 手数料収益。
pub const FEE_INCOME: &str = "fee_income";

/// ユーザーの未配分残高（負債）。
pub fn user_reserve(user_id: &[u8; 32]) -> String {
    format!("user_reserve:{}", hex::encode(user_id))
}

/// ユーザーの移動中の額（負債）。
pub fn user_in_transit(user_id: &[u8; 32]) -> String {
    format!("user_in_transit:{}", hex::encode(user_id))
}

/// ユーザーの出金予約額（負債）。
pub fn user_reserved_for_withdrawal(user_id: &[u8; 32]) -> String {
    format!("user_reserved_for_withdrawal:{}", hex::encode(user_id))
}

/// ユーザー別取引口座のequity（負債）。
pub fn user_trading(account_id: &[u8; 32]) -> String {
    format!("user_trading:{}", hex::encode(account_id))
}

/// 勘定の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    Asset,
    Liability,
    Suspense,
    Income,
}

impl AccountKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Liability => "liability",
            Self::Suspense => "suspense",
            Self::Income => "income",
        }
    }
}

/// 仕訳の1行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posting {
    pub account: String,
    pub kind: AccountKind,
    pub amount: i64,
}

/// 勘定を取得（無ければ作成）する。
fn account_id(connection: &Connection, name: &str, kind: AccountKind) -> Result<i64, Error> {
    if let Some(existing) = connection
        .query_optional_scalar::<i64>(
            "SELECT account_id FROM accounts WHERE name = ?1",
            params![name],
        )
        .map_err(sql)?
    {
        return Ok(existing);
    }
    connection
        .execute(
            "INSERT INTO accounts (name, kind, asset) VALUES (?1, ?2, 'usdc')",
            params![name, kind.as_str()],
        )
        .map_err(sql)?;
    connection
        .query_optional_scalar::<i64>(
            "SELECT account_id FROM accounts WHERE name = ?1",
            params![name],
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)
}

/// 仕訳を1件投稿する。合計が0でなければ拒否する。
///
/// `external_event_id` は外部イベントの安定IDで、同じIDの二重計上を
/// UNIQUE制約で拒否する（重複時は `Conflict`）。
pub fn post_journal(
    connection: &mut UpdateConnection<'_>,
    kind: &str,
    at: u64,
    external_event_id: Option<&[u8; 32]>,
    request_id: Option<&[u8]>,
    postings: &[Posting],
) -> Result<i64, Error> {
    if postings.is_empty() {
        return Err(Error::Invariant("empty journal"));
    }

    let mut total: i64 = 0;
    for posting in postings {
        if posting.amount == 0 {
            return Err(Error::Invariant("zero posting"));
        }
        total = total.checked_add(posting.amount).ok_or(Error::Overflow)?;
    }
    if total != 0 {
        return Err(Error::Invariant("journal is not balanced"));
    }

    let external_event_value = match external_event_id {
        Some(value) => ic_sqlite_vfs::db::Value::Blob(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    let request_value = match request_id {
        Some(value) => ic_sqlite_vfs::db::Value::Blob(value),
        None => ic_sqlite_vfs::db::Value::Null,
    };
    connection
        .execute(
            "INSERT INTO journals (kind, external_event_id, request_id, memo, at)
             VALUES (?1, ?2, ?3, NULL, ?4)",
            params![kind, external_event_value, request_value, at as i64],
        )
        .map_err(sql)?;

    let journal_id = connection
        .query_scalar::<i64>("SELECT last_insert_rowid()", &[])
        .map_err(sql)?;

    for posting in postings {
        let account = account_id(connection, &posting.account, posting.kind)?;
        connection
            .execute(
                "INSERT INTO postings (journal_id, account_id, amount) VALUES (?1, ?2, ?3)",
                params![journal_id, account, posting.amount],
            )
            .map_err(sql)?;
    }

    // 同一 `request_id`・同一種別の仕訳二重計上を拒否する（`journals.request_id` は
    // 一意制約を持てないため別表の主キーで担保する。予約と解放のように1つの要求へ
    // 複数種別の仕訳が対応するため、要求IDだけでは一意にできない）。
    if let Some(request_id) = request_id {
        connection
            .execute(
                "INSERT INTO journal_requests (request_id, kind, journal_id, at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![request_id, kind, journal_id, at as i64],
            )
            .map_err(sql)?;
    }

    Ok(journal_id)
}

/// 勘定の符号付き残高（正=借方）。
pub fn signed_balance(connection: &Connection, account: &str) -> Result<i64, Error> {
    connection
        .query_scalar::<i64>(
            "SELECT COALESCE(SUM(p.amount), 0)
               FROM postings p
               JOIN accounts a ON a.account_id = p.account_id
              WHERE a.name = ?1",
            params![account],
        )
        .map_err(sql)
}

/// 負債勘定の残高（正=負っている額）。
fn liability_balance(connection: &Connection, account: &str) -> Result<u64, Error> {
    let signed = signed_balance(connection, account)?;
    amount_u64(
        signed.checked_neg().ok_or(Error::Overflow)?,
        "liability balance is negative",
    )
}

/// 本人の残高区分（`docs/phase-0/api-contract.md` 2.2の `FundStatus`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserBalances {
    pub reserve_unallocated: u64,
    pub in_transit: u64,
    pub reserved_for_withdrawal: u64,
    pub trading_equity: u64,
    pub withdrawable: u64,
}

/// 本人の残高区分を導出する。二重計上しない。
///
/// 拘束の持ち方を種別ごとに1箇所へ寄せる（同じ額を二度引かない）。
/// - 出金の拘束は台帳（`user_reserved_for_withdrawal`）。`user_reserve` から
///   既に移されているため `reserve_unallocated` に含まれない。
/// - 配分の拘束は `reservations` 表のみ（受付〜送信完了まで台帳は動かない）。
///   したがって `reserve_unallocated` から**配分拘束だけ**を引く。
pub fn user_balances(connection: &Connection, user_id: &[u8; 32]) -> Result<UserBalances, Error> {
    let reserve_unallocated = liability_balance(connection, &user_reserve(user_id))?;
    let in_transit = liability_balance(connection, &user_in_transit(user_id))?;
    let reserved_for_withdrawal =
        liability_balance(connection, &user_reserved_for_withdrawal(user_id))?;

    let mut trading_equity: u64 = 0;
    let account_ids = connection
        .query_all(
            "SELECT account_id FROM custody_accounts WHERE user_id = ?1 AND kind = 'trading'",
            params![user_id.as_slice()],
            |row| row.get::<Vec<u8>>(0),
        )
        .map_err(sql)?;
    for account in account_ids {
        let account_id: [u8; 32] = account
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte account id"))?;
        trading_equity = trading_equity
            .checked_add(liability_balance(connection, &user_trading(&account_id))?)
            .ok_or(Error::Overflow)?;
    }

    let allocation_holds = crate::repo::funds::held_allocation_total(connection, user_id)?;
    let withdrawable = reserve_unallocated
        .checked_sub(allocation_holds)
        .ok_or(Error::Invariant("allocation holds exceed the balance"))?;

    Ok(UserBalances {
        reserve_unallocated,
        in_transit,
        reserved_for_withdrawal,
        trading_equity,
        withdrawable,
    })
}

/// 本人の移動中の額（負債の残高）。
///
/// 取引口座への直接入金を「配分の確定」と誤認しないための境界に使う。
pub fn user_in_transit_balance(connection: &Connection, user_id: &[u8; 32]) -> Result<u64, Error> {
    liability_balance(connection, &user_in_transit(user_id))
}

/// 取引口座への直接入金（配分として説明できない着金）の計上。
///
/// 移動中の額を超える着金は配分の確定ではないため、取引口座のequityへ直接与信する
/// （保留中の配分が無いのに `allocation_confirm` を呼ぶと `user_in_transit` が
/// 負債超過になり、以後の残高参照が不変条件違反で失敗する）。
pub fn trading_deposit_confirmed(
    connection: &mut UpdateConnection<'_>,
    trading_account_id: &[u8; 32],
    amount: u64,
    at: u64,
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "trading_deposit",
        at,
        None,
        None,
        &[
            Posting {
                account: CASH_TRADING.to_string(),
                kind: AccountKind::Asset,
                amount,
            },
            Posting {
                account: user_trading(trading_account_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
        ],
    )
}

/// 配分（予約→取引口座）の開始仕訳。
pub fn allocation_start(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    amount: u64,
    at: u64,
    request_id: &[u8],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "allocation_start",
        at,
        None,
        Some(request_id),
        &[
            Posting {
                account: CASH_IN_TRANSIT.to_string(),
                kind: AccountKind::Asset,
                amount,
            },
            Posting {
                account: CASH_RESERVE.to_string(),
                kind: AccountKind::Asset,
                amount: -amount,
            },
            Posting {
                account: user_reserve(user_id),
                kind: AccountKind::Liability,
                amount,
            },
            Posting {
                account: user_in_transit(user_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
        ],
    )
}

/// 配分の確定仕訳（取引口座への着金）。
pub fn allocation_confirm(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    trading_account_id: &[u8; 32],
    amount: u64,
    at: u64,
    external_event_id: &[u8; 32],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "allocation_confirm",
        at,
        Some(external_event_id),
        None,
        &[
            Posting {
                account: CASH_TRADING.to_string(),
                kind: AccountKind::Asset,
                amount,
            },
            Posting {
                account: CASH_IN_TRANSIT.to_string(),
                kind: AccountKind::Asset,
                amount: -amount,
            },
            Posting {
                account: user_in_transit(user_id),
                kind: AccountKind::Liability,
                amount,
            },
            Posting {
                account: user_trading(trading_account_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
        ],
    )
}

/// 回収（取引口座→共通保管）の確定仕訳。
pub fn recovery_confirm(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    trading_account_id: &[u8; 32],
    amount: u64,
    at: u64,
    external_event_id: &[u8; 32],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "recovery_confirm",
        at,
        Some(external_event_id),
        None,
        &[
            Posting {
                account: CASH_RESERVE.to_string(),
                kind: AccountKind::Asset,
                amount,
            },
            Posting {
                account: CASH_TRADING.to_string(),
                kind: AccountKind::Asset,
                amount: -amount,
            },
            Posting {
                account: user_reserve(user_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
            Posting {
                account: user_trading(trading_account_id),
                kind: AccountKind::Liability,
                amount,
            },
        ],
    )
}

/// 入金の計上仕訳（外部→共通保管、本人への与信）。
pub fn deposit_confirmed(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    amount: u64,
    at: u64,
    external_event_id: &[u8; 32],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "deposit_confirmed",
        at,
        Some(external_event_id),
        None,
        &[
            Posting {
                account: CASH_RESERVE.to_string(),
                kind: AccountKind::Asset,
                amount,
            },
            Posting {
                account: user_reserve(user_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
        ],
    )
}

/// 宛先が未解決の入金の計上（共通保管へ着金し、相手方はsuspenseのまま）。
///
/// 所有者が判明した後で `claim_unmatched_deposit` により本人へ振り替える。
pub fn unmatched_deposit(
    connection: &mut UpdateConnection<'_>,
    amount: u64,
    at: u64,
    external_event_id: &[u8; 32],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "deposit_unmatched",
        at,
        Some(external_event_id),
        None,
        &[
            Posting {
                account: CASH_RESERVE.to_string(),
                kind: AccountKind::Asset,
                amount,
            },
            Posting {
                account: EXTERNAL.to_string(),
                kind: AccountKind::Suspense,
                amount: -amount,
            },
        ],
    )
}

/// 未解決入金を本人へ振り替える（controllerの判断）。
///
/// `event_id` を要求IDとして使い、同一イベントの二重請求を `journal_requests` の
/// 主キーで拒否する。
pub fn claim_unmatched_deposit(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    amount: u64,
    at: u64,
    event_id: &[u8; 32],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "deposit_claimed",
        at,
        None,
        Some(event_id),
        &[
            Posting {
                account: EXTERNAL.to_string(),
                kind: AccountKind::Suspense,
                amount,
            },
            Posting {
                account: user_reserve(user_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
        ],
    )
}

/// 外部イベントIDに対応する仕訳の種別（未計上は `None`）。
pub fn journal_kind_by_external_event(
    connection: &Connection,
    external_event_id: &[u8; 32],
) -> Result<Option<String>, Error> {
    connection
        .query_optional_scalar::<String>(
            "SELECT kind FROM journals WHERE external_event_id = ?1",
            params![external_event_id.as_slice()],
        )
        .map_err(sql)
}

/// 出金予約の仕訳（未配分→出金予約）。
pub fn withdrawal_reserve(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    amount: u64,
    at: u64,
    request_id: &[u8],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "withdrawal_reserve",
        at,
        None,
        Some(request_id),
        &[
            Posting {
                account: user_reserve(user_id),
                kind: AccountKind::Liability,
                amount,
            },
            Posting {
                account: user_reserved_for_withdrawal(user_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
        ],
    )
}

/// 出金予約の取消（棄却時に予約を戻す）。
pub fn withdrawal_release(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    amount: u64,
    at: u64,
    request_id: &[u8],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "withdrawal_release",
        at,
        None,
        Some(request_id),
        &[
            Posting {
                account: user_reserve(user_id),
                kind: AccountKind::Liability,
                amount: -amount,
            },
            Posting {
                account: user_reserved_for_withdrawal(user_id),
                kind: AccountKind::Liability,
                amount,
            },
        ],
    )
}

/// 払出しの確定仕訳（出金予約→外部）。
pub fn payout_settled(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    amount: u64,
    at: u64,
    external_event_id: &[u8; 32],
) -> Result<i64, Error> {
    let amount = i64::try_from(amount).map_err(|_| Error::Overflow)?;
    post_journal(
        connection,
        "payout_settled",
        at,
        Some(external_event_id),
        None,
        &[
            Posting {
                account: user_reserved_for_withdrawal(user_id),
                kind: AccountKind::Liability,
                amount,
            },
            Posting {
                account: CASH_RESERVE.to_string(),
                kind: AccountKind::Asset,
                amount: -amount,
            },
        ],
    )
}

/// 保管口座の参照行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodyAccount {
    pub account_id: [u8; 32],
    pub kind: api_types::AccountKind,
    pub derivation_path: String,
    pub master_address: [u8; 20],
    pub network: String,
    pub state: String,
}

/// 本人の保管口座を1件返す。
pub fn custody_account(
    connection: &Connection,
    user_id: &[u8; 32],
    kind: api_types::AccountKind,
) -> Result<Option<CustodyAccount>, Error> {
    let kind_name = match kind {
        api_types::AccountKind::Reserve => "reserve",
        api_types::AccountKind::Trading => "trading",
    };
    let raw = connection
        .query_optional(
            "SELECT account_id, derivation_path, master_address, network, state
               FROM custody_accounts WHERE user_id = ?1 AND kind = ?2 LIMIT 1",
            params![user_id.as_slice(), kind_name],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<String>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<String>(3)?,
                    row.get::<String>(4)?,
                ))
            },
        )
        .map_err(sql)?;

    raw.map(|raw| {
        Ok(CustodyAccount {
            account_id: raw
                .0
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
            kind,
            derivation_path: raw.1,
            master_address: raw
                .2
                .try_into()
                .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
            network: raw.3,
            state: raw.4,
        })
    })
    .transpose()
}

/// 登録する保管口座の内容。
#[derive(Debug, Clone, Copy)]
pub struct NewCustodyAccount<'a> {
    pub account_id: &'a [u8; 32],
    pub user_id: &'a [u8; 32],
    pub kind: api_types::AccountKind,
    pub derivation_path: &'a str,
    pub master_address: &'a [u8; 20],
    pub network: &'a str,
}

/// 保管口座を登録する（既にあればそのまま）。導出鍵の公開アドレスを保存する。
pub fn ensure_custody_account(
    connection: &mut UpdateConnection<'_>,
    account: &NewCustodyAccount<'_>,
    now: u64,
) -> Result<CustodyAccount, Error> {
    let NewCustodyAccount {
        account_id,
        user_id,
        kind,
        derivation_path,
        master_address,
        network,
    } = *account;
    if let Some(existing) = custody_account(connection, user_id, kind)? {
        return Ok(existing);
    }
    let kind_name = match kind {
        api_types::AccountKind::Reserve => "reserve",
        api_types::AccountKind::Trading => "trading",
    };
    connection
        .execute(
            "INSERT INTO custody_accounts
               (account_id, user_id, kind, derivation_path, master_address, network, state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7)",
            params![
                account_id.as_slice(),
                user_id.as_slice(),
                kind_name,
                derivation_path,
                master_address.as_slice(),
                network,
                now as i64
            ],
        )
        .map_err(sql)?;
    custody_account(connection, user_id, kind)?.ok_or(Error::NotFound)
}

/// 導出アドレスから利用者を引く（入金の宛先解決）。
pub fn custody_account_by_address(
    connection: &Connection,
    address: &[u8; 20],
) -> Result<Option<CustodyOwner>, Error> {
    let row = connection
        .query_optional(
            "SELECT user_id, account_id, kind FROM custody_accounts WHERE master_address = ?1 LIMIT 1",
            params![address.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<String>(2)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(user_id, account_id, kind)| {
        Ok(CustodyOwner {
            user_id: user_id
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
            account_id: account_id
                .try_into()
                .map_err(|_| Error::Invariant("expected a 32-byte account id"))?,
            kind,
        })
    })
    .transpose()
}

/// 導出アドレスの所有者（利用者・口座・種別）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodyOwner {
    pub user_id: [u8; 32],
    pub account_id: [u8; 32],
    pub kind: String,
}

/// 定期照合のカーソル（最後に処理した入金先の `(created_at, master_address)`）。
pub fn reconcile_cursor(connection: &Connection) -> Result<Option<(u64, [u8; 20])>, Error> {
    let raw = connection
        .query_optional(
            "SELECT last_created_at, last_address FROM reconcile_cursor WHERE singleton = 1",
            params![],
            |row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?)),
        )
        .map_err(sql)?;
    raw.map(|(created_at, address)| {
        Ok((
            u64::try_from(created_at).map_err(|_| Error::Invariant("negative timestamp"))?,
            address
                .try_into()
                .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
        ))
    })
    .transpose()
}

/// 定期照合のカーソルを更新する。
pub fn set_reconcile_cursor(
    connection: &mut UpdateConnection<'_>,
    created_at: u64,
    address: &[u8; 20],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO reconcile_cursor (singleton, last_created_at, last_address, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton) DO UPDATE SET
               last_created_at = excluded.last_created_at,
               last_address = excluded.last_address,
               updated_at = excluded.updated_at",
            params![created_at as i64, address.as_slice(), now as i64],
        )
        .map_err(sql)
}

/// 定期照合のカーソルを先頭へ戻す（末尾まで到達したとき）。
pub fn reset_reconcile_cursor(
    connection: &mut UpdateConnection<'_>,
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO reconcile_cursor (singleton, last_created_at, last_address, updated_at)
             VALUES (1, 0, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               last_created_at = 0,
               last_address = excluded.last_address,
               updated_at = excluded.updated_at",
            params![[0u8; 20].as_slice(), now as i64],
        )
        .map_err(sql)
}

/// カーソルより後の全custody口座を `(created_at, master_address)` 昇順で返す。
///
/// reserveへの外部入金だけでなく、allocationの送金先であるtrading口座も同じ
/// ledger update経路で確定する必要がある。先頭N件固定では古い口座が永久に
/// 対象外になるため、キーセットで巡回する。
pub fn custody_addresses_after(
    connection: &Connection,
    limit: u32,
    cursor: Option<(u64, [u8; 20])>,
) -> Result<Vec<([u8; 20], u64)>, Error> {
    let cursor_address_bytes: [u8; 20] = match cursor {
        Some((_, address)) => address,
        None => [0u8; 20],
    };
    let (cursor_created, cursor_address) = match cursor {
        Some((created_at, _)) => (
            ic_sqlite_vfs::db::Value::Integer(
                i64::try_from(created_at).map_err(|_| Error::Overflow)?,
            ),
            ic_sqlite_vfs::db::Value::Blob(cursor_address_bytes.as_slice()),
        ),
        None => (
            ic_sqlite_vfs::db::Value::Null,
            ic_sqlite_vfs::db::Value::Null,
        ),
    };
    let rows = connection
        .query_all(
            "SELECT master_address, created_at FROM custody_accounts
              WHERE (?1 IS NULL
                     OR created_at > ?1
                     OR (created_at = ?1 AND master_address > ?2))
              ORDER BY created_at, master_address
              LIMIT ?3",
            params![cursor_created, cursor_address, limit as i64],
            |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<i64>(1)?)),
        )
        .map_err(sql)?;
    rows.into_iter()
        .map(|(address, created_at)| {
            Ok((
                address
                    .try_into()
                    .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
                u64::try_from(created_at).map_err(|_| Error::Invariant("negative timestamp"))?,
            ))
        })
        .collect()
}

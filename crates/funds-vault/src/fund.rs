//! 資金API（参照系）。`docs/phase-0/api-contract.md` 2.2。
//!
//! 送金を伴う操作（配分・回収・払出し）はoutboxの署名送信（2C）と合わせて実装する。
//! ここでは認証済みセッションでの参照（資金状態・履歴・入金案内）を提供する。

use crate::auth::{VerifiedSession, map_db};
use crate::clock;
use crate::config;
use api_types::error::{ErrorCode, NotAllowedCode};
use api_types::fund::{FundEvent, FundStatus, FundingInstructions};
use api_types::{AccountKind, Blob, Paged};

/// 入金案内。共通保管口座が未作成の間は利用できない（鍵導出は2C）。
pub fn funding_instructions(session: &VerifiedSession) -> Result<FundingInstructions, ErrorCode> {
    let account = db::tx::query(|connection| {
        db::repo::ledger::custody_account(connection, &session.user_id, AccountKind::Reserve)
    })
    .map_err(|error| map_db(error, None))?;

    match account {
        Some(account) => Ok(FundingInstructions {
            account_kind: AccountKind::Reserve,
            hl_account_address: account.master_address.to_vec().into(),
            asset: api_types::AssetId::Usdc,
            network: config::NETWORK,
            minimum_amount: None,
            memo_required: false,
        }),
        None => Err(ErrorCode::NotAllowed {
            code: NotAllowedCode::OperationNotAvailable,
        }),
    }
}

/// 資金状態。残高は仕訳から導出し、未確定額を確定残高へ含めない。
pub fn fund_status(session: &VerifiedSession) -> Result<FundStatus, ErrorCode> {
    let now = clock::now_ms();
    let (balances, unknowns) = db::tx::query(|connection| {
        let balances = db::repo::ledger::user_balances(connection, &session.user_id)?;
        let unknowns = db::repo::actions::unresolved_actions(connection, &session.user_id)?;
        Ok((balances, unknowns))
    })
    .map_err(|error| map_db(error, None))?;

    Ok(FundStatus {
        reserve_unallocated: balances.reserve_unallocated,
        in_transit: balances.in_transit,
        reserved_for_withdrawal: balances.reserved_for_withdrawal,
        // ローカルキャッシュ値（モックベニューの照合値）。実HLの照合値ではない。
        trading_equity: balances.trading_equity,
        trading_unrealized_pnl: 0,
        withdrawable: balances.withdrawable,
        observed_at: now,
        revision: 0,
        unknowns: unknowns
            .into_iter()
            .map(|action| api_types::fund::UnresolvedAction {
                action_id: action.action_id.to_vec().into(),
                kind: action.kind,
                state: action.state,
                since: action.since,
            })
            .collect(),
    })
}

/// 資金履歴（カーソル方式、上限 `MAX_PAGE_SIZE`）。
pub fn fund_events(
    session: &VerifiedSession,
    cursor: Option<Blob>,
    limit: u32,
) -> Result<Paged<FundEvent>, ErrorCode> {
    let limit = limit.clamp(1, config::MAX_PAGE_SIZE);
    let cursor_value = match cursor {
        Some(blob) => Some(decode_cursor(&blob)?),
        None => None,
    };
    let now = clock::now_ms();
    let rows = db::tx::query(|connection| {
        db::repo::events::list_fund_events(connection, &session.user_id, limit, cursor_value)
    })
    .map_err(|error| map_db(error, None))?;

    let next_cursor = if rows.len() as u32 == limit {
        rows.last().map(|row| encode_cursor(row.at).to_vec().into())
    } else {
        None
    };

    Ok(Paged {
        items: rows
            .into_iter()
            .map(|row| FundEvent {
                event_id: row.request_id.into(),
                kind: request_kind(row.kind.as_str()),
                amount: row.amount,
                state: row.state,
                at: row.at,
            })
            .collect(),
        next_cursor,
        observed_at: now,
        revision: 0,
    })
}

fn request_kind(value: &str) -> api_types::fund::FundActionKind {
    match value {
        "recovery" => api_types::fund::FundActionKind::Recovery,
        "withdrawal" => api_types::fund::FundActionKind::Withdrawal,
        _ => api_types::fund::FundActionKind::Allocation,
    }
}

fn encode_cursor(at: u64) -> [u8; 8] {
    at.to_be_bytes()
}

fn decode_cursor(blob: &[u8]) -> Result<u64, ErrorCode> {
    let bytes: [u8; 8] = blob.try_into().map_err(|_| ErrorCode::BadRequest {
        code: api_types::error::BadRequestCode::MalformedPayload,
        detail: "cursor must be 8 bytes".to_string(),
    })?;
    Ok(u64::from_be_bytes(bytes))
}

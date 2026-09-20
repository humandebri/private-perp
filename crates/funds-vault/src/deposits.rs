//! 取引所の入金の取り込み（`/info`照合の受信側）。
//!
//! 正規化したイベントID（`keccak256("deposit" ‖ tx_hash)`）で二重計上を防ぐ。
//! 宛先が導出口座（`custody_accounts.master_address`）と一致すれば本人へ計上し、
//! 未知の宛先は記録のみとする（写像が無い入金を誰かへ付けない）。

use crate::clock;
use db::error::Error as DbError;
use ic_sqlite_vfs::db::UpdateConnection;

/// 入金を取り込む。既知の`tx_hash`なら`false`。
pub fn credit(
    connection: &mut UpdateConnection<'_>,
    tx_hash: &[u8],
    amount: u64,
    address: &[u8; 20],
    asset: &str,
    now: u64,
) -> Result<bool, DbError> {
    let mut input = b"deposit".to_vec();
    input.extend_from_slice(tx_hash);
    let event_id = hl_sign::keccak256(&input);

    let event = db::repo::events::ExternalEvent {
        event_id,
        network: crate::config::network_name(crate::config::NETWORK).to_string(),
        account_address: *address,
        counterparty: [0u8; 20],
        asset: asset.to_string(),
        amount,
        kind: "deposit".to_string(),
        at: now,
        evidence_ref: Some(hex::encode(tx_hash)),
    };
    if !db::repo::events::ingest_external_event(connection, &event, now)? {
        return Ok(false);
    }
    if let Some((user_id, _kind)) =
        db::repo::ledger::custody_account_by_address(connection, address)?
    {
        db::repo::ledger::deposit_confirmed(connection, &user_id, amount, now, &event_id)?;
    }
    Ok(true)
}

/// 現在時刻（ミリ秒）。
pub fn now_ms() -> u64 {
    clock::now_ms()
}

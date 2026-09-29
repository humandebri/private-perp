//! Reconcile the user's trading equity, including realized PnL, funding and fees.
use api_types::{
    error::ErrorCode,
    journal::{RecoveryEvent, RecoveryPayload},
};
use ic_cdk_management_canister::{HttpMethod, HttpRequest, transform_context_from_query};

#[ic_cdk::query]
fn transform_balance(
    args: ic_cdk_management_canister::TransformArgs,
) -> ic_cdk_management_canister::HttpRequestResult {
    let body = serde_json::from_slice::<serde_json::Value>(&args.response.body)
        .ok()
        .map(|v| {
            serde_json::json!({"equity":v["marginSummary"]["accountValue"]})
                .to_string()
                .into_bytes()
        })
        .unwrap_or_default();
    ic_cdk_management_canister::HttpRequestResult {
        status: args.response.status,
        headers: vec![],
        body,
    }
}

pub async fn refresh(user: &[u8; 32]) -> Result<(), ErrorCode> {
    let map = |e| match e {
        db::error::Error::Conflict => ErrorCode::ReservationConflict,
        other => crate::auth::map_db(other, None),
    };
    let account = db::tx::query(|c| {
        db::repo::ledger::custody_account(c, user, api_types::AccountKind::Trading)
    })
    .map_err(map)?
    .ok_or(ErrorCode::PolicyUnavailable)?;
    let before =
        db::tx::query(|c| db::repo::ledger::trading_observation_base(c, user, &account.account_id))
            .map_err(map)?;
    let permit =
        crate::rest_budget::acquire(api_types::operations::BudgetClass::Reconcile, 2).await?;
    if !permit.valid_now() {
        return Err(ErrorCode::PolicyUnavailable);
    }
    // Keep the response independent of HL's per-request timestamp.
    // Direct arrivals after the first observation are accounted by balance
    // observations, not additive deposit postings (see repo::deposits).
    let at = crate::clock::now_ms();
    let url = crate::environment::resolved()?.info_url;
    let response = HttpRequest::new(&url).non_replicated().with_method(HttpMethod::POST).with_header("Content-Type","application/json")
        .with_body(serde_json::json!({"type":"clearinghouseState","user":format!("0x{}",hex::encode(account.master_address))}).to_string().into_bytes())
        .with_max_response_bytes(256*1024).with_transform(transform_context_from_query("transform_balance".into(),vec![])).send().await
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    if response.status.to_string() != "200" {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|_| ErrorCode::PolicyUnavailable)?;
    let amount = crate::amount::parse(&value["equity"])
        .filter(|v| !v.negative && v.micros <= i64::MAX as u64)
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if db::tx::query(|c| {
        Ok(
            db::repo::ledger::trading_observation_base(c, user, &account.account_id)? == before
                && at == db::repo::ledger::trading_observed_at(c, &account.account_id)?
                && before.0 == amount.micros,
        )
    })
    .map_err(map)?
    {
        return Ok(());
    }
    let mut identity = b"trading_balance_observed".to_vec();
    identity.extend_from_slice(&account.account_id);
    identity.extend_from_slice(&at.to_be_bytes());
    let id = hl_sign::keccak256(&identity);
    let event = RecoveryEvent {
        version: 1,
        logical_id: id.to_vec().into(),
        payload: RecoveryPayload::TradingBalanceObserved {
            user_id: user.to_vec().into(),
            account_id: account.account_id.to_vec().into(),
            previous_equity: before.0,
            equity: amount.micros,
            observed_at_ms: at,
        },
    };
    let ack = journal_client::append_recovery_event_if("vault", event.clone(), |c| {
        Ok(
            db::repo::ledger::trading_observation_base(c, user, &account.account_id)? == before
                && at > db::repo::ledger::trading_observed_at(c, &account.account_id)?
                && at >= db::repo::ledger::trading_latest_posting_at(c, &account.account_id)?,
        )
    })
    .await?
    .ok_or(ErrorCode::PolicyUnavailable)?;
    let result = db::tx::update(|c| {
        journal_client::record_recovery_event(c, &event, &ack)?;
        db::repo::ledger::observe_trading_balance(
            c,
            user,
            &account.account_id,
            before.0,
            amount.micros,
            at,
            &id,
        )
    });
    if let Err(e) = result {
        journal_client::lock()?;
        return Err(map(e));
    }
    Ok(())
}

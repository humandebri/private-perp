//! Shared policy permits for every vault Hyperliquid REST request.

use api_types::error::ErrorCode;
use api_types::operations::{BudgetClass, RestBudgetRequest};
use candid::Principal;
use ic_cdk::call::Call;
use std::cell::Cell;

thread_local! {
    static NEXT_ATTEMPT: Cell<u64> = const { Cell::new(0) };
}

pub struct Permit {
    request: RestBudgetRequest,
}

impl Permit {
    pub fn valid_now(&self) -> bool {
        self.request.valid_at(ic_cdk::api::time() / 1_000_000)
    }
}

pub async fn acquire(class: BudgetClass, weight: u32) -> Result<Permit, ErrorCode> {
    let policy = db::tx::query(db::repo::vault_config::policy_principal)
        .map_err(|error| crate::auth::map_db(error, None))?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    let now = ic_cdk::api::time() / 1_000_000;
    let expires_at = now.saturating_add(30_000);
    let attempt = NEXT_ATTEMPT.with(|next| {
        let value = next.get();
        next.set(value.wrapping_add(1));
        value
    });
    let request_id = hl_sign::rest_budget::request_id(
        hl_sign::rest_budget::Worker::Vault,
        ic_cdk::api::canister_self().as_slice(),
        ic_cdk::api::time(),
        attempt,
        expires_at,
    );
    let request = RestBudgetRequest {
        request_id: request_id.to_vec().into(),
        class,
        weight,
        expires_at,
    };
    let response = Call::bounded_wait(Principal::from_slice(&policy), "consume_rest_budget")
        .with_arg(request.clone())
        .await
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    let result: Result<(), ErrorCode> = response
        .candid()
        .map_err(|_| ErrorCode::PolicyUnavailable)?;
    result?;
    if !request.valid_at(ic_cdk::api::time() / 1_000_000) {
        return Err(ErrorCode::PolicyUnavailable);
    }
    Ok(Permit { request })
}

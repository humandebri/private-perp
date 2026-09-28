use api_types::error::ErrorCode;
use api_types::operations_status::CyclesStatus;

fn map_db(error: db::error::Error) -> ErrorCode {
    crate::map_db(error)
}

pub fn configure(daily_floor: u128, exit_reserve: u128) -> Result<(), ErrorCode> {
    journal_client::require_management()?;
    if daily_floor == 0 || exit_reserve == 0 {
        return Err(ErrorCode::PolicyUnavailable);
    }
    db::tx::update(|c| db::repo::cycles::configure(c, daily_floor, exit_reserve)).map_err(map_db)
}

pub fn status() -> Result<CyclesStatus, ErrorCode> {
    let now = ic_cdk::api::time() / 1_000_000;
    let balance = ic_cdk::api::canister_cycle_balance();
    db::tx::update(|c| db::repo::cycles::sample(c, balance, now)).map_err(map_db)?;
    current(now, balance)
}

fn current(now: u64, balance: u128) -> Result<CyclesStatus, ErrorCode> {
    let (config, observed) = db::tx::query(|c| {
        Ok((
            db::repo::cycles::config(c)?,
            db::repo::cycles::observed_daily_burn(c, now)?,
        ))
    })
    .map_err(map_db)?;
    Ok(CyclesStatus::calculate(balance, observed, config, now))
}

pub fn require_new() -> Result<(), ErrorCode> {
    if current(
        ic_cdk::api::time() / 1_000_000,
        ic_cdk::api::canister_cycle_balance(),
    )?
    .new_risk_stopped
    {
        Err(ErrorCode::PolicyUnavailable)
    } else {
        Ok(())
    }
}

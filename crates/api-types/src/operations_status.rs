use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CyclesStatus {
    pub balance: u128,
    pub observed_daily_burn: u128,
    pub configured_daily_floor: Option<u128>,
    pub exit_reserve: Option<u128>,
    pub estimated_days: Option<u64>,
    pub refill_target: Option<u128>,
    pub warning: bool,
    pub new_risk_stopped: bool,
    pub observed_at: u64,
}

impl CyclesStatus {
    pub fn calculate(
        balance: u128,
        measured: u128,
        config: Option<(u128, u128)>,
        now: u64,
    ) -> Self {
        let (floor, reserve) = match config {
            Some((floor, reserve)) if floor > 0 && reserve > 0 => (Some(floor), Some(reserve)),
            _ => (None, None),
        };
        let daily = floor.unwrap_or(0).max(measured);
        let days = reserve.and_then(|reserve| {
            balance
                .saturating_sub(reserve)
                .checked_div(daily)
                .map(|days| days.min(u128::from(u64::MAX)) as u64)
        });
        let stopped = match (reserve, days) {
            (Some(reserve), Some(days)) => balance <= reserve || days <= 3,
            _ => true,
        };
        Self {
            balance,
            observed_daily_burn: measured,
            configured_daily_floor: floor,
            exit_reserve: reserve,
            estimated_days: days,
            refill_target: reserve.map(|reserve| reserve.saturating_add(daily.saturating_mul(30))),
            warning: days.is_none_or(|days| days <= 7),
            new_risk_stopped: stopped,
            observed_at: now,
        }
    }
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MarketThreshold {
    pub market: String,
    pub expected_index: u32,
    pub min_day_notional_usdc: u64,
    pub max_spread_bps: u32,
    pub min_each_side_depth_usdc: u64,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MarketStatus {
    pub market: String,
    pub observed_at: Option<u64>,
    pub reason_code: Option<String>,
    pub eligible_for_new_risk: bool,
}

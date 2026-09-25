use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

pub struct Threshold {
    pub expected_index: u32,
    pub min_day_notional_usdc: u64,
    pub max_spread_bps: u32,
    pub min_each_side_depth_usdc: u64,
}

pub struct Observation {
    pub observed_at: Option<u64>,
    pub checked_at: u64,
    pub reason_code: Option<String>,
}

pub fn set_threshold(
    c: &mut UpdateConnection<'_>,
    market: &str,
    t: &Threshold,
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO market_thresholds(market, expected_index, min_day_notional_usdc, max_spread_bps, min_each_side_depth_usdc)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(market) DO UPDATE SET expected_index=excluded.expected_index,
           min_day_notional_usdc=excluded.min_day_notional_usdc, max_spread_bps=excluded.max_spread_bps,
           min_each_side_depth_usdc=excluded.min_each_side_depth_usdc",
        params![market, i64::from(t.expected_index), i64::try_from(t.min_day_notional_usdc).map_err(|_| Error::Overflow)?,
            i64::from(t.max_spread_bps), i64::try_from(t.min_each_side_depth_usdc).map_err(|_| Error::Overflow)?],
    ).map_err(sql)
}

pub fn threshold(c: &Connection, market: &str) -> Result<Option<Threshold>, Error> {
    let row = c
        .query_optional(
            "SELECT expected_index, min_day_notional_usdc, max_spread_bps, min_each_side_depth_usdc
         FROM market_thresholds WHERE market = ?1",
            params![market],
            |r| {
                Ok((
                    r.get::<i64>(0)?,
                    r.get::<i64>(1)?,
                    r.get::<i64>(2)?,
                    r.get::<i64>(3)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(index, volume, spread, depth)| {
        Ok(Threshold {
            expected_index: u32::try_from(index).map_err(|_| Error::Overflow)?,
            min_day_notional_usdc: u64::try_from(volume).map_err(|_| Error::Overflow)?,
            max_spread_bps: u32::try_from(spread).map_err(|_| Error::Overflow)?,
            min_each_side_depth_usdc: u64::try_from(depth).map_err(|_| Error::Overflow)?,
        })
    })
    .transpose()
}

pub fn observation(c: &Connection, market: &str) -> Result<Option<Observation>, Error> {
    let row = c.query_optional(
        "SELECT observed_at, checked_at, reason_code FROM market_observations WHERE market = ?1",
        params![market],
        |r| Ok((r.get::<Option<i64>>(0)?, r.get::<i64>(1)?, r.get::<Option<String>>(2)?)),
    ).map_err(sql)?;
    row.map(|(observed, checked, reason)| {
        Ok(Observation {
            observed_at: observed
                .map(|n| u64::try_from(n).map_err(|_| Error::Overflow))
                .transpose()?,
            checked_at: u64::try_from(checked).map_err(|_| Error::Overflow)?,
            reason_code: reason,
        })
    })
    .transpose()
}

#[allow(clippy::too_many_arguments)]
pub fn record(
    c: &mut UpdateConnection<'_>,
    market: &str,
    now: u64,
    reason: Option<&str>,
    index: Option<u32>,
    volume: Option<u64>,
    spread: Option<u32>,
    bid: Option<u64>,
    ask: Option<u64>,
) -> Result<(), Error> {
    use ic_sqlite_vfs::db::Value;
    let number = |value: Option<u64>| -> Result<Value<'static>, Error> {
        Ok(match value {
            Some(n) => Value::Integer(i64::try_from(n).map_err(|_| Error::Overflow)?),
            None => Value::Null,
        })
    };
    let reason_value = match reason {
        Some(reason) => Value::Text(reason),
        None => Value::Null,
    };
    let old = observation(c, market)?;
    let observed_at = if reason.is_none() {
        Some(now)
    } else {
        old.and_then(|o| o.observed_at)
    };
    c.execute(
        "INSERT INTO market_observations(market, observed_at, checked_at, reason_code, asset_index, day_notional_usdc, spread_bps, bid_depth_usdc, ask_depth_usdc)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(market) DO UPDATE SET observed_at=excluded.observed_at, checked_at=excluded.checked_at,
           reason_code=excluded.reason_code, asset_index=excluded.asset_index,
           day_notional_usdc=excluded.day_notional_usdc, spread_bps=excluded.spread_bps,
           bid_depth_usdc=excluded.bid_depth_usdc, ask_depth_usdc=excluded.ask_depth_usdc",
        params![market, number(observed_at)?, i64::try_from(now).map_err(|_| Error::Overflow)?, reason_value,
            number(index.map(u64::from))?, number(volume)?, number(spread.map(u64::from))?,
            number(bid)?, number(ask)?],
    ).map_err(sql)
}

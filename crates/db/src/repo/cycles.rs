use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

fn parse(value: String) -> Result<u128, Error> {
    value
        .parse()
        .map_err(|_| Error::Invariant("invalid cycles value"))
}

pub fn configure(c: &mut UpdateConnection<'_>, floor: u128, reserve: u128) -> Result<(), Error> {
    c.execute(
        "INSERT INTO cycles_config(singleton, daily_floor, exit_reserve) VALUES(1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET daily_floor=excluded.daily_floor, exit_reserve=excluded.exit_reserve",
        params![floor.to_string(), reserve.to_string()],
    ).map_err(sql)
}

pub fn config(c: &Connection) -> Result<Option<(u128, u128)>, Error> {
    let row = c
        .query_optional(
            "SELECT daily_floor, exit_reserve FROM cycles_config WHERE singleton = 1",
            params![],
            |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?)),
        )
        .map_err(sql)?;
    row.map(|(floor, reserve)| Ok((parse(floor)?, parse(reserve)?)))
        .transpose()
}

pub fn sample(c: &mut UpdateConnection<'_>, balance: u128, now: u64) -> Result<(), Error> {
    let bucket = now / 3_600_000;
    c.execute(
        "INSERT INTO cycles_samples(hour_bucket, balance, observed_at) VALUES(?1, ?2, ?3)
         ON CONFLICT(hour_bucket) DO UPDATE SET balance=excluded.balance, observed_at=excluded.observed_at",
        params![i64::try_from(bucket).map_err(|_| Error::Overflow)?, balance.to_string(),
            i64::try_from(now).map_err(|_| Error::Overflow)?],
    ).map_err(sql)?;
    c.execute(
        "DELETE FROM cycles_samples WHERE hour_bucket < ?1",
        params![i64::try_from(bucket.saturating_sub(24 * 8)).map_err(|_| Error::Overflow)?],
    )
    .map_err(sql)
}

/// Sum only observed decreases; a refill does not count as negative burn.
pub fn observed_daily_burn(c: &Connection, now: u64) -> Result<u128, Error> {
    let start = now.saturating_sub(7 * 24 * 3_600_000) / 3_600_000;
    let rows = c.query_all(
        "SELECT hour_bucket, balance FROM cycles_samples WHERE hour_bucket >= ?1 ORDER BY hour_bucket",
        params![i64::try_from(start).map_err(|_| Error::Overflow)?],
        |r| Ok((r.get::<i64>(0)?, r.get::<String>(1)?)),
    ).map_err(sql)?;
    let Some((first_hour, first_balance)) = rows.first().cloned() else {
        return Ok(0);
    };
    let mut previous = parse(first_balance)?;
    let mut consumed = 0u128;
    let mut last_hour = first_hour;
    for (hour, balance) in rows.into_iter().skip(1) {
        let current = parse(balance)?;
        consumed = consumed.saturating_add(previous.saturating_sub(current));
        previous = current;
        last_hour = hour;
    }
    let hours =
        u128::try_from(last_hour.saturating_sub(first_hour)).map_err(|_| Error::Overflow)?;
    if hours == 0 {
        return Ok(0);
    }
    Ok(consumed.saturating_mul(24).saturating_add(hours - 1) / hours)
}

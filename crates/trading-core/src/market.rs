//! BTC/ETH market admission from bounded, budgeted HL observations.

use api_types::error::ErrorCode;
use api_types::operations_status::{MarketStatus, MarketThreshold};

const POLL_MS: u64 = 5 * 60_000;
const STALE_MS: u64 = 10 * 60_000;

fn threshold(market: &str) -> Result<db::repo::market::Threshold, ErrorCode> {
    db::tx::query(|c| db::repo::market::threshold(c, market))
        .map_err(crate::map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)
}

pub fn configure(input: MarketThreshold) -> Result<(), ErrorCode> {
    let guard = db::tx::query(db::repo::send_journal_client::guard_principal)
        .map_err(crate::map_db)?
        .ok_or(ErrorCode::PolicyUnavailable)?;
    if ic_cdk::api::msg_caller().as_slice() != guard.as_slice() {
        return Err(ErrorCode::Unauthenticated {
            reason: "SNS guard required".into(),
        });
    }
    if !matches!(input.market.as_str(), "BTC" | "ETH")
        || input.min_day_notional_usdc == 0
        || input.min_each_side_depth_usdc == 0
        || !(1..=10_000).contains(&input.max_spread_bps)
    {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let now = ic_cdk::api::time() / 1_000_000;
    db::tx::update(|c| {
        db::repo::market::set_threshold(
            c,
            &input.market,
            &db::repo::market::Threshold {
                expected_index: input.expected_index,
                min_day_notional_usdc: input.min_day_notional_usdc,
                max_spread_bps: input.max_spread_bps,
                min_each_side_depth_usdc: input.min_each_side_depth_usdc,
            },
        )?;
        db::repo::market::record(
            c,
            &input.market,
            now,
            Some("threshold_changed"),
            None,
            None,
            None,
            None,
            None,
        )
    })
    .map_err(crate::map_db)
}

pub fn status(market: &str) -> Result<MarketStatus, ErrorCode> {
    if !matches!(market, "BTC" | "ETH") {
        return Err(ErrorCode::PolicyUnavailable);
    }
    let configured = db::tx::query(|c| db::repo::market::threshold(c, market))
        .map_err(crate::map_db)?
        .is_some();
    let observation =
        db::tx::query(|c| db::repo::market::observation(c, market)).map_err(crate::map_db)?;
    let now = ic_cdk::api::time() / 1_000_000;
    let observed_at = observation.as_ref().and_then(|o| o.observed_at);
    let reason = if !configured {
        Some("threshold_unconfigured".into())
    } else if observed_at.is_none_or(|at| now.saturating_sub(at) > STALE_MS) {
        Some("market_observation_stale".into())
    } else {
        observation.and_then(|o| o.reason_code)
    };
    Ok(MarketStatus {
        market: market.into(),
        observed_at,
        eligible_for_new_risk: reason.is_none(),
        reason_code: reason,
    })
}

pub fn require(market: &str) -> Result<(), ErrorCode> {
    if status(market)?.eligible_for_new_risk {
        Ok(())
    } else {
        Err(ErrorCode::PolicyUnavailable)
    }
}

fn micro(value: &serde_json::Value) -> Option<u128> {
    let owned;
    let input = if let Some(text) = value.as_str() {
        text
    } else {
        owned = value.as_number()?.to_string();
        &owned
    };
    if input.starts_with('-') || input.starts_with('+') {
        return None;
    }
    let (whole, fractional) = input.split_once('.').unwrap_or((input, ""));
    if whole.is_empty()
        || fractional.len() > 6
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fractional.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole: u128 = whole.parse().ok()?;
    let fraction: u128 = if fractional.is_empty() {
        0
    } else {
        fractional.parse().ok()?
    };
    whole
        .checked_mul(1_000_000)?
        .checked_add(fraction.checked_mul(10u128.pow(6 - fractional.len() as u32))?)
}

fn dollars(value: u128) -> Option<u64> {
    u64::try_from(value / 1_000_000).ok()
}

fn book_side(value: &serde_json::Value) -> Option<(u128, u128)> {
    let levels = value.as_array()?;
    if levels.is_empty() || levels.len() > 20 {
        return None;
    }
    let top = micro(levels.first()?.get("px")?)?;
    let mut total = 0u128;
    for level in levels {
        let px = micro(level.get("px")?)?;
        let sz = micro(level.get("sz")?)?;
        total = total.checked_add(px.checked_mul(sz)? / 1_000_000)?;
    }
    Some((top, total))
}

type MarketEvaluation = (u32, u64, u32, u64, u64, Option<&'static str>);

fn evaluate(
    market: &str,
    meta: &serde_json::Value,
    book: &serde_json::Value,
    t: &db::repo::market::Threshold,
) -> Option<MarketEvaluation> {
    let universe = meta.get(0)?.get("universe")?.as_array()?;
    let contexts = meta.get(1)?.as_array()?;
    let index = universe
        .iter()
        .position(|entry| entry.get("name").and_then(|name| name.as_str()) == Some(market))?;
    let entry = universe.get(index)?;
    let actual_index = u32::try_from(index).ok()?;
    let volume = dollars(micro(contexts.get(index)?.get("dayNtlVlm")?)?)?;
    let levels = book.get("levels")?.as_array()?;
    if levels.len() != 2 {
        return None;
    }
    let (bid_px, bid_value) = book_side(&levels[0])?;
    let (ask_px, ask_value) = book_side(&levels[1])?;
    if bid_px == 0 || ask_px <= bid_px {
        return None;
    }
    let midpoint = bid_px.checked_add(ask_px)? / 2;
    let spread = u32::try_from(ask_px.checked_sub(bid_px)?.checked_mul(10_000)? / midpoint).ok()?;
    let bid = dollars(bid_value)?;
    let ask = dollars(ask_value)?;
    let reason = if entry.get("isDelisted").and_then(|v| v.as_bool()) == Some(true) {
        Some("asset_delisted")
    } else if actual_index != t.expected_index {
        Some("asset_index_changed")
    } else if volume < t.min_day_notional_usdc {
        Some("volume_low")
    } else if spread > t.max_spread_bps {
        Some("spread_wide")
    } else if bid < t.min_each_side_depth_usdc || ask < t.min_each_side_depth_usdc {
        Some("book_shallow")
    } else {
        None
    };
    Some((actual_index, volume, spread, bid, ask, reason))
}

async fn observe(now: u64) -> Result<(), ErrorCode> {
    let _ = (threshold("BTC")?, threshold("ETH")?);
    let meta_raw = crate::venue::meta_and_asset_ctxs().await?;
    let meta: serde_json::Value =
        serde_json::from_str(&meta_raw).map_err(|_| ErrorCode::PolicyUnavailable)?;
    for market in ["BTC", "ETH"] {
        let t = threshold(market)?;
        let book_raw = crate::venue::l2_book(market).await?;
        let book: serde_json::Value =
            serde_json::from_str(&book_raw).map_err(|_| ErrorCode::PolicyUnavailable)?;
        let result = evaluate(market, &meta, &book, &t);
        match result {
            Some((index, volume, spread, bid, ask, reason)) => db::tx::update(|c| {
                db::repo::market::record(
                    c,
                    market,
                    now,
                    reason,
                    Some(index),
                    Some(volume),
                    Some(spread),
                    Some(bid),
                    Some(ask),
                )
            })
            .map_err(crate::map_db)?,
            None => db::tx::update(|c| {
                db::repo::market::record(
                    c,
                    market,
                    now,
                    Some("market_data_invalid"),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .map_err(crate::map_db)?,
        }
    }
    Ok(())
}

pub async fn poll_if_due(now: u64) -> Result<(), ErrorCode> {
    let claimed = db::tx::update(|c| {
        let mut due = false;
        for market in ["BTC", "ETH"] {
            let observation = db::repo::market::observation(c, market)?;
            due |= observation.is_none_or(|o| {
                o.reason_code.as_deref() == Some("threshold_changed")
                    || now.saturating_sub(o.checked_at) >= POLL_MS
            });
        }
        if !due {
            return Ok(false);
        }
        for market in ["BTC", "ETH"] {
            db::repo::market::record(
                c,
                market,
                now,
                Some("market_polling"),
                None,
                None,
                None,
                None,
                None,
            )?;
        }
        Ok(true)
    })
    .map_err(crate::map_db)?;
    if !claimed {
        return Ok(());
    }
    if let Err(error) = observe(now).await {
        for market in ["BTC", "ETH"] {
            db::tx::update(|c| {
                db::repo::market::record(
                    c,
                    market,
                    now,
                    Some("market_observation_failed"),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            })
            .map_err(crate::map_db)?;
        }
        return Err(error);
    }
    Ok(())
}

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
    journal_client::require_management()?;
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
    decimal_micro(value, false)
}

// HL reports rolling market volume with more than six decimal places. Floor
// this observation so admission never overstates volume. Money, book prices
// and sizes retain their existing strict precision checks.
fn volume_micro(value: &serde_json::Value) -> Option<u128> {
    decimal_micro(value, true)
}

fn decimal_micro(value: &serde_json::Value, floor_extra_precision: bool) -> Option<u128> {
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
        || (!floor_extra_precision && fractional.len() > 6)
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fractional.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let fractional = &fractional[..fractional.len().min(6)];
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

fn book_side(value: &serde_json::Value, bids: bool) -> Option<(u128, u128)> {
    let levels = value.as_array()?;
    if levels.is_empty() || levels.len() > 20 {
        return None;
    }
    let top = micro(levels.first()?.get("px")?)?;
    let mut total = 0u128;
    let mut previous = None;
    for level in levels {
        let px = micro(level.get("px")?)?;
        let sz = micro(level.get("sz")?)?;
        if px == 0
            || sz == 0
            || previous.is_some_and(|prev| if bids { px >= prev } else { px <= prev })
        {
            return None;
        }
        previous = Some(px);
        total = total.checked_add(px.checked_mul(sz)? / 1_000_000)?;
    }
    Some((top, total))
}

// Only admission verdicts are agreed, not diagnostic price/volume samples.
// Thresholds and time bounds travel in immutable transform context.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct CheckContext {
    pub threshold: MarketThreshold,
    pub book: bool,
    pub requested_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Verdict {
    Eligible,
    AssetDelisted,
    AssetIndexChanged,
    VolumeLow,
    SpreadWide,
    BookShallow,
    Invalid,
}

impl Verdict {
    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Eligible => None,
            Self::AssetDelisted => Some("asset_delisted"),
            Self::AssetIndexChanged => Some("asset_index_changed"),
            Self::VolumeLow => Some("volume_low"),
            Self::SpreadWide => Some("spread_wide"),
            Self::BookShallow => Some("book_shallow"),
            Self::Invalid => Some("market_data_invalid"),
        }
    }
}

const REQUEST_WINDOW_MS: u64 = 60_000;

fn snapshot(market: &str) -> Result<MarketThreshold, ErrorCode> {
    let t = threshold(market)?;
    Ok(MarketThreshold {
        market: market.into(),
        expected_index: t.expected_index,
        min_day_notional_usdc: t.min_day_notional_usdc,
        max_spread_bps: t.max_spread_bps,
        min_each_side_depth_usdc: t.min_each_side_depth_usdc,
    })
}

fn evaluate_meta(meta: &serde_json::Value, t: &MarketThreshold) -> Option<Verdict> {
    let universe = meta.get(0)?.get("universe")?.as_array()?;
    let contexts = meta.get(1)?.as_array()?;
    if universe.len() != contexts.len() {
        return None;
    }
    let mut matches = universe
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.get("name").and_then(|v| v.as_str()) == Some(t.market.as_str()));
    let (index, entry) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let delisted = match entry.get("isDelisted") {
        None => false,
        Some(value) => value.as_bool()?,
    };
    let volume = dollars(volume_micro(contexts.get(index)?.get("dayNtlVlm")?)?)?;
    Some(if delisted {
        Verdict::AssetDelisted
    } else if u32::try_from(index).ok()? != t.expected_index {
        Verdict::AssetIndexChanged
    } else if volume < t.min_day_notional_usdc {
        Verdict::VolumeLow
    } else {
        Verdict::Eligible
    })
}

fn evaluate_book(book: &serde_json::Value, context: &CheckContext) -> Option<Verdict> {
    let t = &context.threshold;
    let time = book.get("time")?.as_u64()?;
    if book.get("coin")?.as_str()? != t.market
        || time < context.requested_at.saturating_sub(REQUEST_WINDOW_MS)
        || time > context.requested_at.saturating_add(REQUEST_WINDOW_MS)
    {
        return None;
    }
    let levels = book.get("levels")?.as_array()?;
    if levels.len() != 2 {
        return None;
    }
    let (bid_px, bid_value) = book_side(&levels[0], true)?;
    let (ask_px, ask_value) = book_side(&levels[1], false)?;
    if bid_px == 0 || ask_px <= bid_px {
        return None;
    }
    // Compare the exact ratio: integer truncation must not admit a spread
    // slightly above the configured bound.
    let spread_numerator = ask_px.checked_sub(bid_px)?.checked_mul(20_000)?;
    let spread_limit = bid_px
        .checked_add(ask_px)?
        .checked_mul(u128::from(t.max_spread_bps))?;
    Some(if spread_numerator > spread_limit {
        Verdict::SpreadWide
    } else if dollars(bid_value)? < t.min_each_side_depth_usdc
        || dollars(ask_value)? < t.min_each_side_depth_usdc
    {
        Verdict::BookShallow
    } else {
        Verdict::Eligible
    })
}

pub(crate) fn canonical_check(body: &[u8], context: &[u8]) -> Vec<u8> {
    let verdict = (|| {
        let context: CheckContext = serde_json::from_slice(context).ok()?;
        let t = &context.threshold;
        if !matches!(t.market.as_str(), "BTC" | "ETH")
            || t.min_day_notional_usdc == 0
            || t.min_each_side_depth_usdc == 0
            || !(1..=10_000).contains(&t.max_spread_bps)
        {
            return None;
        }
        let value = serde_json::from_slice(body).ok()?;
        if context.book {
            evaluate_book(&value, &context)
        } else {
            evaluate_meta(&value, t)
        }
    })()
    .unwrap_or(Verdict::Invalid);
    serde_json::to_vec(&verdict).unwrap_or_default()
}

fn owns_poll(
    c: &ic_sqlite_vfs::db::connection::Connection,
    market: &str,
    now: u64,
) -> Result<bool, db::error::Error> {
    Ok(db::repo::market::observation(c, market)?
        .is_some_and(|o| o.checked_at == now && o.reason_code.as_deref() == Some("market_polling")))
}

async fn observe(now: u64) -> Result<(), ErrorCode> {
    let thresholds = [snapshot("BTC")?, snapshot("ETH")?];
    let mut observations = Vec::new();
    for t in &thresholds {
        let meta = crate::venue::market_check(&CheckContext {
            threshold: t.clone(),
            book: false,
            requested_at: now,
        })
        .await?;
        let book = crate::venue::market_check(&CheckContext {
            threshold: t.clone(),
            book: true,
            requested_at: now,
        })
        .await?;
        observations.push(if meta != Verdict::Eligible {
            meta
        } else {
            book
        });
    }
    let completed_at = ic_cdk::api::time() / 1_000_000;
    if completed_at.saturating_sub(now) > REQUEST_WINDOW_MS {
        return Err(ErrorCode::PolicyUnavailable);
    }
    // There is no await between this check and commit. Reconfiguration or a
    // newer poll must not be overwritten by an older response.
    for t in &thresholds {
        if snapshot(&t.market)? != *t
            || !db::tx::query(|c| owns_poll(c, &t.market, now)).map_err(crate::map_db)?
        {
            return Err(ErrorCode::PolicyUnavailable);
        }
    }
    db::tx::update(|c| {
        for (t, verdict) in thresholds.iter().zip(observations) {
            db::repo::market::record(
                c,
                &t.market,
                now,
                verdict.reason(),
                None,
                None,
                None,
                None,
                None,
            )?;
        }
        Ok(())
    })
    .map_err(crate::map_db)
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
                if !owns_poll(c, market, now)? {
                    return Ok(());
                }
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

#[cfg(test)]
mod tests {
    use super::{dollars, micro, volume_micro};

    #[test]
    fn rolling_volume_is_floored_without_relaxing_money_precision() {
        let observation = serde_json::json!("2945429.7254399993");
        assert_eq!(volume_micro(&observation), Some(2_945_429_725_439));
        assert_eq!(
            dollars(volume_micro(&observation).unwrap()),
            Some(2_945_429)
        );
        assert_eq!(micro(&observation), None);
        assert_eq!(
            dollars(volume_micro(&serde_json::json!("999999.9999999999")).unwrap()),
            Some(999_999)
        );
        for input in ["-1.2", "+1.2", "1e6", "1.000000bad", "NaN"] {
            assert_eq!(volume_micro(&serde_json::json!(input)), None);
        }
    }
}

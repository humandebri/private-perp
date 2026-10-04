/// Round a positive price in USDC micros to the venue's decimal and five-digit limits.
/// Closing sells round down and closing buys round up to preserve executable IOC bounds.
pub(super) fn round_price_micros(micros: u128, round_up: bool, size_decimals: u32) -> u128 {
    let market_step = if size_decimals >= 6 {
        1
    } else {
        10u128.pow(size_decimals)
    };
    let significant_step = 10u128.pow((micros.ilog10() + 1).saturating_sub(5));
    let step = market_step.max(significant_step);
    let floored = micros / step * step;
    if round_up && floored != micros {
        floored + step
    } else {
        floored
    }
}

#[cfg(test)]
mod tests {
    use super::round_price_micros;

    #[test]
    fn fractional_btc_close_prices_fit_five_significant_digits() {
        assert_eq!(round_price_micros(59_998_500_000, false, 5), 59_998_000_000);
        assert_eq!(round_price_micros(60_601_500_000, true, 5), 60_602_000_000);
    }

    #[test]
    fn both_decimal_and_significant_precision_apply() {
        assert_eq!(round_price_micros(2_487_622_385, false, 3), 2_487_600_000);
        assert_eq!(round_price_micros(2_512_623_615, true, 3), 2_512_700_000);
        assert_eq!(round_price_micros(122_838, false, 0), 122_830);
        assert_eq!(round_price_micros(122_838, true, 5), 200_000);
    }

    #[test]
    fn exact_prices_and_rounding_across_digit_boundary_are_valid() {
        assert_eq!(round_price_micros(60_000_000_000, true, 5), 60_000_000_000);
        assert_eq!(round_price_micros(99_999_500_000, true, 5), 100_000_000_000);
    }
}

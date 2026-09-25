//! HL ledger の USDC 額を、JSON 数値・文字列のどちらからも浮動小数点を使わず読む。

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerUsdc {
    pub micros: u64,
    pub negative: bool,
}

/// 6 桁より細かい額、範囲外、丸めを要する指数表記は受け付けない。
pub fn parse(value: &Value) -> Option<LedgerUsdc> {
    let text = match value {
        Value::String(text) => text.as_str(),
        Value::Number(number) => return parse_text(&number.to_string()),
        _ => return None,
    };
    parse_text(text)
}

pub fn parse_text(text: &str) -> Option<LedgerUsdc> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (unsigned, 0),
    };
    // 元の十進表記が 6 桁を超える精度を要求する場合は、丸めず拒否する。
    let has_decimal_point = mantissa.contains('.');
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || (has_decimal_point && fraction.is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 6
    {
        return None;
    }
    let mut digits = 0u128;
    for byte in whole.bytes().chain(fraction.bytes()) {
        digits = digits
            .checked_mul(10)?
            .checked_add(u128::from(byte - b'0'))?;
    }
    let shift = exponent
        .checked_sub(i32::try_from(fraction.len()).ok()?)?
        .checked_add(6)?;
    let micros = if shift >= 0 {
        digits.checked_mul(10u128.checked_pow(u32::try_from(shift).ok()?)?)?
    } else {
        let divisor = 10u128.checked_pow(shift.unsigned_abs())?;
        if !digits.is_multiple_of(divisor) {
            return None;
        }
        digits / divisor
    };
    Some(LedgerUsdc {
        micros: u64::try_from(micros).ok()?,
        negative,
    })
}

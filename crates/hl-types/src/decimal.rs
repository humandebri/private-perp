//! 正規化十進文字列。`docs/phase-0/money-and-units.md` 2〜3節。
//!
//! 金額・数量・価格を浮動小数点で扱わないための最小限の型と検証を提供する。

use std::fmt;

/// 十進表記の解析・検証エラー。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DecimalError {
    #[error("empty value")]
    Empty,
    #[error("invalid character")]
    InvalidCharacter,
    #[error("exponent notation is not allowed")]
    ExponentNotAllowed,
    #[error("sign is not allowed")]
    SignNotAllowed,
    #[error("fraction exceeds the allowed scale")]
    ScaleExceeded,
    #[error("significant figures exceed the allowed limit")]
    SignificantFiguresExceeded,
    #[error("value is out of range")]
    OutOfRange,
}

/// 正規化された十進文字列。
///
/// 先頭の`+`・指数表記・末尾ゼロを許さない。負値は`-`を先頭に1つだけ許す。
/// 内部表現は文字列であり、数値型へ変換しない。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decimal(String);

impl Decimal {
    /// 入力を検証し、正規化する。
    pub fn parse(input: &str) -> Result<Self, DecimalError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(DecimalError::Empty);
        }
        let (negative, digits) = match trimmed.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
        };
        if digits.is_empty() {
            return Err(DecimalError::InvalidCharacter);
        }
        if digits.contains(['e', 'E']) {
            return Err(DecimalError::ExponentNotAllowed);
        }

        let (whole, fraction) = match digits.split_once('.') {
            Some((whole, fraction)) => (whole, fraction),
            None => (digits, ""),
        };
        if whole.is_empty() && fraction.is_empty() {
            return Err(DecimalError::InvalidCharacter);
        }
        if !whole.chars().all(|c| c.is_ascii_digit())
            || !fraction.chars().all(|c| c.is_ascii_digit())
        {
            return Err(DecimalError::InvalidCharacter);
        }

        let whole_normalized = whole.trim_start_matches('0');
        let fraction_trimmed = fraction.trim_end_matches('0');

        let mut normalized = String::new();
        if negative && (!whole_normalized.is_empty() || !fraction_trimmed.is_empty()) {
            normalized.push('-');
        }
        normalized.push_str(if whole_normalized.is_empty() {
            "0"
        } else {
            whole_normalized
        });
        if !fraction_trimmed.is_empty() {
            normalized.push('.');
            normalized.push_str(fraction_trimmed);
        }

        Ok(Self(normalized))
    }

    /// 正規化された文字列。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 小数部の桁数。
    pub fn scale(&self) -> u32 {
        match self.0.split_once('.') {
            Some((_, fraction)) => fraction.len() as u32,
            None => 0,
        }
    }

    /// 有効数字の桁数（先頭のゼロを数えない）。
    pub fn significant_figures(&self) -> u32 {
        let digits: String = self
            .0
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>();
        let leading = digits.len() - digits.trim_start_matches('0').len();
        let significant = &digits[leading..];
        let significant = significant.trim_end_matches('0');
        if significant.is_empty() {
            1
        } else {
            significant.len() as u32
        }
    }

    /// `szDecimals` とHLが受理する有効桁の条件を検査する（`money-and-units.md` 3節）。
    pub fn validate_precision(
        &self,
        max_scale: u32,
        max_significant_figures: u32,
    ) -> Result<(), DecimalError> {
        if self.scale() > max_scale {
            return Err(DecimalError::ScaleExceeded);
        }
        if self.significant_figures() > max_significant_figures {
            return Err(DecimalError::SignificantFiguresExceeded);
        }
        Ok(())
    }

    /// 整数部を`u64`へ変換する（小数部が無い場合のみ）。
    pub fn to_u64_exact(&self) -> Result<u64, DecimalError> {
        if self.0.contains('.') || self.0.starts_with('-') {
            return Err(DecimalError::OutOfRange);
        }
        self.0.parse::<u64>().map_err(|_| DecimalError::OutOfRange)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Decimal, DecimalError};

    #[test]
    fn normalizes_input_without_changing_value() {
        assert_eq!(Decimal::parse("1.500").unwrap().as_str(), "1.5");
        assert_eq!(Decimal::parse("007").unwrap().as_str(), "7");
        assert_eq!(Decimal::parse("0.0").unwrap().as_str(), "0");
        assert_eq!(Decimal::parse(".5").unwrap().as_str(), "0.5");
        assert_eq!(Decimal::parse("1.").unwrap().as_str(), "1");
        assert_eq!(Decimal::parse(" -0.250 ").unwrap().as_str(), "-0.25");
    }

    #[test]
    fn rejects_exponent_and_bad_input() {
        assert_eq!(Decimal::parse("").unwrap_err(), DecimalError::Empty);
        assert_eq!(
            Decimal::parse("1e5").unwrap_err(),
            DecimalError::ExponentNotAllowed
        );
        assert_eq!(
            Decimal::parse("1.2.3").unwrap_err(),
            DecimalError::InvalidCharacter
        );
        assert_eq!(
            Decimal::parse("0x10").unwrap_err(),
            DecimalError::InvalidCharacter
        );
    }

    #[test]
    fn checks_scale_and_significant_figures() {
        let value = Decimal::parse("0.000123").unwrap();
        assert_eq!(value.scale(), 6);
        assert_eq!(value.significant_figures(), 3);
        assert_eq!(
            value.validate_precision(5, 5).unwrap_err(),
            DecimalError::ScaleExceeded
        );
        assert_eq!(
            value.validate_precision(6, 2).unwrap_err(),
            DecimalError::SignificantFiguresExceeded
        );
        assert!(value.validate_precision(6, 5).is_ok());
    }

    #[test]
    fn counts_significant_figures_for_integers() {
        assert_eq!(Decimal::parse("100").unwrap().significant_figures(), 1);
        assert_eq!(Decimal::parse("0").unwrap().significant_figures(), 1);
        assert_eq!(Decimal::parse("12345").unwrap().significant_figures(), 5);
    }

    #[test]
    fn exact_integer_conversion_never_silently_rounds() {
        assert_eq!(Decimal::parse("42").unwrap().to_u64_exact().unwrap(), 42);
        assert!(Decimal::parse("42.5").unwrap().to_u64_exact().is_err());
        assert!(Decimal::parse("-1").unwrap().to_u64_exact().is_err());
    }
}

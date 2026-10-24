//! Amounts: decimal quantities with a commodity and a printing style.
//!
//! All arithmetic uses `rust_decimal` (28 significant digits). The style is kept
//! so that `parse -> print -> parse` is a fixed point (see `tests/roundtrip.rs`).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// How an amount is spelled. Rendered back by [`Amount::format`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmountStyle {
    /// Commodity symbol before the number (`¥4.50`).
    pub prefix: bool,
    /// Space between symbol and number (`4.50 CNY` vs `4.50CNY`).
    pub space: bool,
    pub decimal_mark: char,
    pub group_mark: Option<char>,
    /// Digits after the decimal mark. Always exact for parsed amounts.
    pub precision: u32,
}

impl Default for AmountStyle {
    fn default() -> Self {
        Self {
            prefix: false,
            space: true,
            decimal_mark: '.',
            group_mark: None,
            precision: 2,
        }
    }
}

impl AmountStyle {
    /// Style used for amounts the application generates itself.
    pub fn canonical(commodity: &str, precision: u32) -> Self {
        let letters = commodity
            .chars()
            .next()
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false);
        Self {
            prefix: false,
            space: letters,
            decimal_mark: '.',
            group_mark: None,
            precision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amount {
    pub quantity: Decimal,
    pub commodity: String,
    #[serde(default)]
    pub style: AmountStyle,
}

impl Amount {
    pub fn new(quantity: Decimal, commodity: impl Into<String>, style: AmountStyle) -> Self {
        Self {
            quantity,
            commodity: commodity.into(),
            style,
        }
    }

    /// Amount with the canonical style for `commodity`/`precision`.
    pub fn canonical(quantity: Decimal, commodity: &str, precision: u32) -> Self {
        Self::new(
            quantity,
            commodity,
            AmountStyle::canonical(commodity, precision),
        )
    }

    pub fn zero(commodity: &str, precision: u32) -> Self {
        Self::canonical(Decimal::ZERO, commodity, precision)
    }

    pub fn is_zero(&self) -> bool {
        self.quantity.is_zero()
    }

    pub fn neg(&self) -> Self {
        let mut c = self.clone();
        c.quantity = -c.quantity;
        c
    }

    pub fn abs(&self) -> Self {
        let mut c = self.clone();
        c.quantity = c.quantity.abs();
        c
    }

    /// Re-quantize to `precision`, keeping the style (used for display/totals).
    pub fn with_precision(&self, precision: u32) -> Self {
        let mut c = self.clone();
        c.quantity = c.quantity.round_dp(precision);
        c.style.precision = precision;
        c
    }

    pub fn precision(&self) -> u32 {
        self.style.precision
    }

    pub fn format(&self) -> String {
        let number = format_quantity(self.quantity, &self.style);
        self.assemble(&number)
    }

    fn assemble(&self, number: &str) -> String {
        if self.commodity.is_empty() {
            return number.to_string();
        }
        let gap = if self.style.space { " " } else { "" };
        if self.style.prefix {
            // The sign has to sit in front of the symbol, otherwise `-¥88.00` would
            // print as `¥-88.00` and be re-parsed as commodity `¥-`.
            match number.strip_prefix('-') {
                Some(rest) => format!("-{}{}{}", self.commodity, gap, rest),
                None => format!("{}{}{}", self.commodity, gap, number),
            }
        } else {
            format!("{}{}{}", number, gap, self.commodity)
        }
    }
}

impl std::fmt::Display for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format())
    }
}

/// Render a decimal with explicit precision and optional digit grouping.
pub fn format_quantity(quantity: Decimal, style: &AmountStyle) -> String {
    let precision = style.precision as usize;
    let negative = quantity.is_sign_negative() && !quantity.is_zero();
    let mut s = quantity.abs().round_dp(style.precision).to_string();

    let dot = s.find('.').unwrap_or(s.len());
    if precision == 0 {
        s.truncate(dot);
    } else {
        if dot == s.len() {
            s.push('.');
        }
        let frac = s.len() - dot - 1;
        if frac < precision {
            for _ in frac..precision {
                s.push('0');
            }
        } else if frac > precision {
            s.truncate(dot + 1 + precision);
        }
    }

    let (int_part, frac_part) = match s.find('.') {
        Some(i) => (s[..i].to_string(), Some(s[i + 1..].to_string())),
        None => (s.clone(), None),
    };
    let int_out = match style.group_mark {
        Some(gm) => {
            let mut grouped = String::new();
            for (count, ch) in int_part.chars().rev().enumerate() {
                if count > 0 && count % 3 == 0 {
                    grouped.push(gm);
                }
                grouped.push(ch);
            }
            grouped.chars().rev().collect()
        }
        None => int_part,
    };
    s = match frac_part {
        Some(frac) => format!("{int_out}{}{frac}", style.decimal_mark),
        None => int_out,
    };

    if negative {
        format!("-{s}")
    } else {
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AmountError {
    #[error("amount is empty")]
    Empty,
    #[error("amount has no digits: {0}")]
    NoDigits(String),
    #[error("amount has both a prefix and a suffix commodity: {0}")]
    TwoCommodities(String),
    #[error("unexpected character {1:?} in amount {0}")]
    BadChar(String, char),
    #[error("commodity symbol contains a digit: {0}")]
    DigitInCommodity(String),
    #[error("invalid number: {0}")]
    BadNumber(String),
}

/// Parse an amount such as `4.5`, `4.5cny`, `¥4.5`, `1,000.00 CNY`, `4,50 EUR`.
pub fn parse_amount(text: &str) -> Result<Amount, AmountError> {
    let raw = text.trim();
    if raw.is_empty() {
        return Err(AmountError::Empty);
    }

    let (sign, rest) = match raw.as_bytes()[0] {
        b'-' => (-1i32, &raw[1..]),
        b'+' => (1i32, &raw[1..]),
        _ => (1i32, raw),
    };
    let rest = rest.trim_start();

    let first_digit = rest
        .char_indices()
        .find(|(_, c)| c.is_ascii_digit())
        .map(|(i, _)| i)
        .ok_or_else(|| AmountError::NoDigits(text.to_string()))?;
    let last_digit = rest
        .char_indices()
        .filter(|(_, c)| c.is_ascii_digit())
        .next_back()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap();
    // Reject digits appearing after the numeric core through a symbol (e.g. `USD4X`).
    let before = &rest[..first_digit];
    let after = &rest[last_digit..];
    let digits = &rest[first_digit..last_digit];

    let (commodity, prefix, space) = match (before.trim().is_empty(), after.trim().is_empty()) {
        (true, true) => (String::new(), false, true),
        (false, true) => (
            before.trim().to_string(),
            true,
            before.ends_with(char::is_whitespace),
        ),
        (true, false) => (
            after.trim().to_string(),
            false,
            after.starts_with(char::is_whitespace),
        ),
        (false, false) => return Err(AmountError::TwoCommodities(text.to_string())),
    };
    if commodity.chars().any(|c| c.is_ascii_digit()) {
        return Err(AmountError::DigitInCommodity(text.to_string()));
    }
    if !commodity.chars().all(|c| !c.is_control()) {
        return Err(AmountError::BadChar(text.to_string(), '?'));
    }

    for c in digits.chars() {
        if !c.is_ascii_digit() && c != '.' && c != ',' {
            return Err(AmountError::BadChar(text.to_string(), c));
        }
    }

    let dots = digits.matches('.').count();
    let commas = digits.matches(',').count();

    let mut decimal_mark = '.';
    let mut group_mark: Option<char> = None;

    if dots > 0 && commas > 0 {
        // Whichever appears last is the decimal mark.
        let dot_pos = digits.rfind('.').unwrap();
        let comma_pos = digits.rfind(',').unwrap();
        if comma_pos > dot_pos {
            decimal_mark = ',';
            group_mark = Some('.');
        } else {
            decimal_mark = '.';
            group_mark = Some(',');
        }
    } else if dots + commas > 1 {
        // Repeated separator: digit grouping (`1.234.567`).
        group_mark = Some(if dots > 0 { '.' } else { ',' });
    } else if dots + commas == 1 {
        let sep = if dots == 1 { '.' } else { ',' };
        let pos = digits.find(sep).unwrap();
        let int_part = &digits[..pos];
        let frac = &digits[pos + 1..];
        let looks_like_group = frac.len() == 3
            && !int_part.is_empty()
            && int_part != "0"
            && !int_part.starts_with('0');
        if looks_like_group {
            group_mark = Some(sep);
        } else {
            decimal_mark = sep;
        }
    }

    let precision = match digits.rfind(decimal_mark) {
        Some(pos) => digits[pos + 1..].chars().count() as u32,
        None => 0,
    };

    let mut cleaned: String = digits.chars().filter(|c| Some(*c) != group_mark).collect();
    if decimal_mark != '.' {
        cleaned = cleaned.replace(decimal_mark, ".");
    }

    let mut quantity =
        Decimal::from_str(&cleaned).map_err(|_| AmountError::BadNumber(text.to_string()))?;
    if sign < 0 {
        quantity = -quantity;
    }

    Ok(Amount {
        quantity,
        commodity,
        style: AmountStyle {
            prefix,
            space,
            decimal_mark,
            group_mark,
            precision,
        },
    })
}

/// Parse a rule template percentage such as `70%` or `33.33%`.
pub fn parse_percent(text: &str) -> Result<Decimal, AmountError> {
    let t = text.trim();
    let body = t
        .strip_suffix('%')
        .ok_or_else(|| AmountError::BadNumber(text.to_string()))?
        .trim();
    let amount = parse_amount(body)?;
    Ok(amount.quantity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn q(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn parses_plain() {
        let a = parse_amount("4.5").unwrap();
        assert_eq!(a.quantity, q("4.5"));
        assert_eq!(a.commodity, "");
        assert_eq!(a.style.precision, 1);
    }

    #[test]
    fn parses_suffix_without_space() {
        let a = parse_amount("4.5cny").unwrap();
        assert_eq!(a.quantity, q("4.5"));
        assert_eq!(a.commodity, "cny");
        assert!(!a.style.prefix);
        assert!(!a.style.space);
    }

    #[test]
    fn parses_prefix_symbol() {
        let a = parse_amount("¥4.50").unwrap();
        assert_eq!(a.quantity, q("4.50"));
        assert_eq!(a.commodity, "¥");
        assert!(a.style.prefix);
        assert!(!a.style.space);
    }

    #[test]
    fn parses_suffix_with_space_and_grouping() {
        let a = parse_amount("1,000.00 CNY").unwrap();
        assert_eq!(a.quantity, q("1000.00"));
        assert_eq!(a.commodity, "CNY");
        assert_eq!(a.style.group_mark, Some(','));
        assert_eq!(a.style.precision, 2);
        assert_eq!(a.format(), "1,000.00 CNY");
    }

    #[test]
    fn parses_decimal_comma() {
        let a = parse_amount("4,50 EUR").unwrap();
        assert_eq!(a.quantity, q("4.50"));
        assert_eq!(a.style.decimal_mark, ',');
        assert_eq!(a.format(), "4,50 EUR");
    }

    #[test]
    fn three_digit_comma_is_grouping() {
        let a = parse_amount("1,000").unwrap();
        assert_eq!(a.quantity, q("1000"));
        assert_eq!(a.style.precision, 0);
    }

    #[test]
    fn zero_leading_three_decimals_is_decimal() {
        let a = parse_amount("0.500").unwrap();
        assert_eq!(a.quantity, q("0.500"));
        assert_eq!(a.style.precision, 3);
    }

    #[test]
    fn negative_amount() {
        let a = parse_amount("-4.50 CNY").unwrap();
        assert_eq!(a.quantity, q("-4.50"));
        assert_eq!(a.format(), "-4.50 CNY");
    }

    #[test]
    fn mixed_marks() {
        let a = parse_amount("1.234,56 EUR").unwrap();
        assert_eq!(a.quantity, q("1234.56"));
        assert_eq!(a.format(), "1.234,56 EUR");
    }

    #[test]
    fn precision_is_padded() {
        let a = parse_amount("4 CNY").unwrap();
        assert_eq!(a.quantity, q("4"));
        assert_eq!(a.style.precision, 0);
        let p = a.with_precision(2);
        assert_eq!(p.format(), "4.00 CNY");
    }

    #[test]
    fn percent() {
        assert_eq!(parse_percent("70.00%").unwrap(), q("70.00"));
        assert!(parse_percent("70").is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_amount("").is_err());
        assert!(parse_amount("abc").is_err());
        assert!(parse_amount("USD 4 CNY").is_err());
        assert!(parse_amount("4x5").is_err());
    }

    #[test]
    fn format_is_stable() {
        for s in [
            "4.50 CNY",
            "1,000.00 CNY",
            "4,50 EUR",
            "-4.50 CNY",
            "¥4.50",
            "4.5cny",
            "1.234,56 EUR",
            "0.500",
        ] {
            let a = parse_amount(s).unwrap();
            let printed = a.format();
            let b = parse_amount(&printed).unwrap();
            assert_eq!(a, b, "{s} -> {printed}");
        }
    }
}

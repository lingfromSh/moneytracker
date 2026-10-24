//! The hledger query subset used by MoneyTracker filters, rules and search.
//!
//! ```text
//! expr    := or
//! or      := and ("or" and)*
//! and     := unary ("and" unary)*
//! unary   := "not" unary | "(" expr ")" | term
//! term    := KEY ":" VALUE      (acct: desc: date: tag: amt: status: cur: code:)
//!          | bareword           (description substring)
//! ```

use chrono::NaiveDate;
use regex::Regex;
use rust_decimal::Decimal;
use std::str::FromStr;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message} (at byte {pos})")]
pub struct QueryError {
    pub message: String,
    pub pos: usize,
}

impl QueryError {
    fn new(message: impl Into<String>, pos: usize) -> Self {
        Self {
            message: message.into(),
            pos,
        }
    }
}

/// A transaction flattened for matching.
#[derive(Debug, Clone, Default)]
pub struct TxnInfo<'a> {
    pub date: Option<NaiveDate>,
    pub payee: &'a str,
    pub code: &'a str,
    pub status: &'a str,
    pub accounts: &'a [String],
    pub tags: &'a [(String, Option<String>)],
    /// Signed posting quantities (used by `amt:`).
    pub amounts: &'a [Decimal],
    pub commodities: &'a [String],
}

#[derive(Debug, Clone)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Acct(Regex),
    Desc(Regex),
    Code(Regex),
    Status(String),
    Tag { key: String, value: Option<String> },
    Date(DateSpec),
    Amount(AmountSpec),
    Commodity(String),
}

#[derive(Debug, Clone)]
pub enum DateSpec {
    Prefix(String),
    Ge(String),
    Gt(String),
    Le(String),
    Lt(String),
    Range(String, String),
    Eq(String),
}

#[derive(Debug, Clone)]
pub enum AmountSpec {
    Eq(Decimal),
    Gt(Decimal),
    Ge(Decimal),
    Lt(Decimal),
    Le(Decimal),
    Between(Decimal, Decimal),
}

#[derive(Debug, Clone)]
pub struct Query {
    raw: String,
    expr: Expr,
}

impl Query {
    pub fn parse(raw: &str) -> Result<Self, QueryError> {
        let mut p = QParser::new(raw);
        let expr = p.parse_expr()?;
        p.skip_ws();
        if !p.eof() {
            return Err(QueryError::new(
                format!("unexpected trailing input `{}`", &raw[p.pos..]),
                p.pos,
            ));
        }
        Ok(Self {
            raw: raw.to_string(),
            expr,
        })
    }

    /// Like [`Query::parse`] but falls back to a description substring match so
    /// free text typed in the UI never errors out.
    pub fn parse_lenient(raw: &str) -> Self {
        match Self::parse(raw) {
            Ok(q) => q,
            Err(_) => {
                let escaped = regex::escape(raw.trim());
                Self {
                    raw: raw.to_string(),
                    expr: Expr::Desc(Regex::new(&format!("(?i){}", escaped)).unwrap()),
                }
            }
        }
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn is_empty(&self) -> bool {
        self.raw.trim().is_empty()
    }

    pub fn matches(&self, t: &TxnInfo<'_>) -> bool {
        eval(&self.expr, t)
    }

    /// Stable identity for rule files: lowercased, whitespace collapsed.
    pub fn normalized(&self) -> String {
        self.raw
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
}

fn eval(e: &Expr, t: &TxnInfo<'_>) -> bool {
    match e {
        Expr::And(a, b) => eval(a, t) && eval(b, t),
        Expr::Or(a, b) => eval(a, t) || eval(b, t),
        Expr::Not(a) => !eval(a, t),
        Expr::Acct(re) => t.accounts.iter().any(|a| re.is_match(a)),
        Expr::Desc(re) => re.is_match(t.payee),
        Expr::Code(re) => re.is_match(t.code),
        Expr::Status(s) => match s.as_str() {
            "cleared" | "*" => t.status == "cleared",
            "pending" | "!" => t.status == "pending",
            "uncleared" | "unmarked" => t.status.is_empty(),
            other => t.status == other,
        },
        Expr::Tag { key, value } => t.tags.iter().any(|(k, v)| {
            k == key
                && match value {
                    None => true,
                    Some(want) => v.as_deref() == Some(want.as_str()),
                }
        }),
        Expr::Date(spec) => match &t.date {
            None => false,
            Some(d) => date_matches(spec, *d),
        },
        Expr::Amount(spec) => t.amounts.iter().any(|q| amount_matches(spec, *q)),
        Expr::Commodity(c) => t.commodities.iter().any(|x| x == c),
    }
}

fn date_matches(spec: &DateSpec, d: NaiveDate) -> bool {
    let iso = d.format("%Y-%m-%d").to_string();
    match spec {
        DateSpec::Prefix(p) => iso.starts_with(p.as_str()),
        DateSpec::Eq(p) => iso == *p,
        DateSpec::Ge(p) => iso.as_str() >= p.as_str(),
        DateSpec::Gt(p) => iso.as_str() > p.as_str(),
        DateSpec::Le(p) => iso.as_str() <= p.as_str(),
        DateSpec::Lt(p) => iso.as_str() < p.as_str(),
        DateSpec::Range(a, b) => iso.as_str() >= a.as_str() && iso.as_str() <= b.as_str(),
    }
}

fn amount_matches(spec: &AmountSpec, q: Decimal) -> bool {
    let a = q.abs();
    match spec {
        AmountSpec::Eq(v) => a == v.abs(),
        AmountSpec::Gt(v) => a > v.abs(),
        AmountSpec::Ge(v) => a >= v.abs(),
        AmountSpec::Lt(v) => a < v.abs(),
        AmountSpec::Le(v) => a <= v.abs(),
        AmountSpec::Between(lo, hi) => a >= lo.abs() && a <= hi.abs(),
    }
}

struct QParser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> QParser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.rest().chars().next() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn eat_keyword(&mut self, kw: &str) -> bool {
        self.skip_ws();
        let rest = self.rest();
        if rest.len() >= kw.len() && rest[..kw.len()].eq_ignore_ascii_case(kw) {
            let after = rest[kw.len()..].chars().next();
            if after.map(|c| c.is_whitespace() || c == '(').unwrap_or(true) {
                self.pos += kw.len();
                return true;
            }
        }
        false
    }

    fn parse_expr(&mut self) -> Result<Expr, QueryError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_and()?;
        while self.eat_keyword("or") {
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_unary()?;
        loop {
            let save = self.pos;
            if self.eat_keyword("and") {
                let right = self.parse_unary()?;
                left = Expr::And(Box::new(left), Box::new(right));
            } else {
                self.pos = save;
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, QueryError> {
        self.skip_ws();
        if self.eat_keyword("not") {
            let inner = self.parse_unary()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.skip_ws();
        if self.rest().starts_with('(') {
            self.pos += 1;
            let inner = self.parse_expr()?;
            self.skip_ws();
            if !self.rest().starts_with(')') {
                return Err(QueryError::new("expected `)`", self.pos));
            }
            self.pos += 1;
            return Ok(inner);
        }
        self.parse_term()
    }

    fn parse_term(&mut self) -> Result<Expr, QueryError> {
        self.skip_ws();
        let start = self.pos;
        let word = self.take_word();
        if word.is_empty() {
            return Err(QueryError::new("expected a query term", start));
        }
        let (key, value) = match word.split_once(':') {
            Some((k, v)) => (k.to_ascii_lowercase(), v.to_string()),
            None => {
                let re = Regex::new(&format!("(?i){}", regex::escape(&word)))
                    .map_err(|e| QueryError::new(e.to_string(), start))?;
                return Ok(Expr::Desc(re));
            }
        };
        let value = if value.is_empty() {
            // Allow `acct: foo bar` (space after the colon).
            self.skip_ws();
            self.take_word()
        } else {
            value
        };
        match key.as_str() {
            "acct" | "account" | "a" => Ok(Expr::Acct(compile_regex(&value, start)?)),
            "desc" | "payee" | "d" => Ok(Expr::Desc(compile_regex(&value, start)?)),
            "code" => Ok(Expr::Code(compile_regex(&value, start)?)),
            "status" => Ok(Expr::Status(value)),
            "cur" | "commodity" => Ok(Expr::Commodity(value)),
            "tag" => {
                let (k, v) = match value.split_once('=') {
                    Some((k, v)) => (k.to_string(), Some(v.to_string())),
                    None => (value, None),
                };
                Ok(Expr::Tag { key: k, value: v })
            }
            "date" | "dat" => parse_date_spec(&value).map(Expr::Date),
            "amt" | "amount" => parse_amount_spec(&value).map(Expr::Amount),
            other => Err(QueryError::new(
                format!(
                    "unknown query key `{other}` (expected acct/desc/date/tag/amt/status/cur/code)"
                ),
                start,
            )),
        }
    }

    fn take_word(&mut self) -> String {
        let rest = self.rest();
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if c.is_whitespace() || c == ')' {
                end = i;
                break;
            }
        }
        let word = rest[..end].trim_matches('"').to_string();
        self.pos += end;
        word
    }
}

fn compile_regex(value: &str, pos: usize) -> Result<Regex, QueryError> {
    Regex::new(&format!("(?i){}", value)).map_err(|e| QueryError::new(e.to_string(), pos))
}

fn parse_date_spec(value: &str) -> Result<DateSpec, QueryError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(QueryError::new("date: needs a value", 0));
    }
    let strip_op = |op: &str| v.strip_prefix(op).map(|s| s.trim().to_string());
    if let Some(s) = strip_op(">=") {
        return Ok(DateSpec::Ge(s));
    }
    if let Some(s) = strip_op("<=") {
        return Ok(DateSpec::Le(s));
    }
    if let Some(s) = strip_op(">") {
        return Ok(DateSpec::Gt(s));
    }
    if let Some(s) = strip_op("<") {
        return Ok(DateSpec::Lt(s));
    }
    if let Some((a, b)) = v.split_once("..") {
        return Ok(DateSpec::Range(a.trim().to_string(), b.trim().to_string()));
    }
    if v.len() == 10 {
        return Ok(DateSpec::Eq(v.to_string()));
    }
    if v.len() == 4 || v.len() == 7 {
        return Ok(DateSpec::Prefix(v.to_string()));
    }
    Err(QueryError::new(
        format!("unsupported date expression `{v}`"),
        0,
    ))
}

fn parse_amount_spec(value: &str) -> Result<AmountSpec, QueryError> {
    let v = value.trim();
    let strip = |op: &str| v.strip_prefix(op).map(|s| s.trim());
    if let Some(s) = strip(">=") {
        return dec(s).map(AmountSpec::Ge);
    }
    if let Some(s) = strip("<=") {
        return dec(s).map(AmountSpec::Le);
    }
    if let Some(s) = strip(">") {
        return dec(s).map(AmountSpec::Gt);
    }
    if let Some(s) = strip("<") {
        return dec(s).map(AmountSpec::Lt);
    }
    if let Some(s) = strip("=") {
        return dec(s).map(AmountSpec::Eq);
    }
    if let Some((a, b)) = v.split_once("..") {
        return Ok(AmountSpec::Between(dec(a)?, dec(b)?));
    }
    dec(v).map(AmountSpec::Eq)
}

fn dec(s: &str) -> Result<Decimal, QueryError> {
    Decimal::from_str(s.trim().trim_start_matches('$').trim_start_matches('¥'))
        .map_err(|_| QueryError::new(format!("invalid amount `{s}`"), 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>(
        accounts: &'a [String],
        tags: &'a [(String, Option<String>)],
        amounts: &'a [Decimal],
    ) -> TxnInfo<'a> {
        TxnInfo {
            date: Some(NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()),
            payee: "星巴克 Starbucks",
            code: "42",
            status: "cleared",
            accounts,
            tags,
            amounts,
            commodities: &[],
        }
    }

    fn accts() -> Vec<String> {
        vec!["expenses:food:coffee".into(), "assets:wallet:alipay".into()]
    }

    fn tags() -> Vec<(String, Option<String>)> {
        vec![
            ("coffee".to_string(), None),
            ("rule".to_string(), Some("r-1".to_string())),
        ]
    }

    fn amounts() -> Vec<Decimal> {
        vec![
            Decimal::from_str("4.50").unwrap(),
            Decimal::from_str("-4.50").unwrap(),
        ]
    }

    #[test]
    fn matches_account_regex() {
        let q = Query::parse("acct:coffee").unwrap();
        let a = accts();
        let tg = tags();
        let am = amounts();
        assert!(q.matches(&info(&a, &tg, &am)));
        let q = Query::parse("acct:^assets:").unwrap();
        assert!(q.matches(&info(&a, &tg, &am)));
        assert!(!Query::parse("acct:nope")
            .unwrap()
            .matches(&info(&a, &tg, &am)));
    }

    #[test]
    fn boolean_operators() {
        let a = accts();
        let tg = tags();
        let am = amounts();
        let t = info(&a, &tg, &am);
        assert!(Query::parse("acct:coffee and amt:>4").unwrap().matches(&t));
        assert!(!Query::parse("acct:coffee and amt:>5").unwrap().matches(&t));
        assert!(Query::parse("acct:none or acct:coffee")
            .unwrap()
            .matches(&t));
        assert!(Query::parse("not acct:none").unwrap().matches(&t));
        assert!(Query::parse("(acct:coffee or acct:none) and desc:星巴克")
            .unwrap()
            .matches(&t));
    }

    #[test]
    fn tag_and_status() {
        let a = accts();
        let tg = tags();
        let am = amounts();
        let t = info(&a, &tg, &am);
        assert!(Query::parse("tag:coffee").unwrap().matches(&t));
        assert!(Query::parse("tag:rule=r-1").unwrap().matches(&t));
        assert!(!Query::parse("tag:rule=r-2").unwrap().matches(&t));
        assert!(Query::parse("status:cleared").unwrap().matches(&t));
        assert!(!Query::parse("status:pending").unwrap().matches(&t));
    }

    #[test]
    fn date_expressions() {
        let a = accts();
        let tg = tags();
        let am = amounts();
        let t = info(&a, &tg, &am);
        assert!(Query::parse("date:2026").unwrap().matches(&t));
        assert!(Query::parse("date:2026-10").unwrap().matches(&t));
        assert!(Query::parse("date:2026-10-25").unwrap().matches(&t));
        assert!(Query::parse("date:>=2026-10-01").unwrap().matches(&t));
        assert!(!Query::parse("date:>2026-10-25").unwrap().matches(&t));
        assert!(Query::parse("date:2026-01..2026-12").unwrap().matches(&t));
    }

    #[test]
    fn amount_comparisons() {
        let a = accts();
        let tg = tags();
        let am = amounts();
        let t = info(&a, &tg, &am);
        assert!(Query::parse("amt:4.5").unwrap().matches(&t));
        assert!(Query::parse("amt:<=4.5").unwrap().matches(&t));
        assert!(Query::parse("amt:1..5").unwrap().matches(&t));
        assert!(!Query::parse("amt:>10").unwrap().matches(&t));
    }

    #[test]
    fn bareword_is_description_substring() {
        let a = accts();
        let tg = tags();
        let am = amounts();
        let t = info(&a, &tg, &am);
        assert!(Query::parse("星巴克").unwrap().matches(&t));
        assert!(Query::parse("starbucks").unwrap().matches(&t));
        assert!(!Query::parse("luckin").unwrap().matches(&t));
    }

    #[test]
    fn lenient_falls_back_to_substring() {
        let q = Query::parse_lenient("acct:[unclosed");
        let a = accts();
        let tg = tags();
        let am = amounts();
        assert!(!q.matches(&info(&a, &tg, &am)));
        assert_eq!(q.normalized(), "acct:[unclosed");
    }

    #[test]
    fn normalized_query_is_stable() {
        let q = Query::parse("Desc:星巴克   AND  amt:>0").unwrap();
        assert_eq!(q.normalized(), "desc:星巴克 and amt:>0");
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(Query::parse("wat:1").is_err());
        assert!(Query::parse("(acct:a").is_err());
    }
}

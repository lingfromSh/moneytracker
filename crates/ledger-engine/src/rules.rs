//! Rules: `rules.ledger` (hledger `=` blocks) plus the one-line `=` capture
//! syntax. Rules are expanded **at write time**, so the ledger on disk stays
//! plain hledger data that any external tool can read.

use std::collections::HashMap;

use chrono::NaiveDate;
use ledger_core::{parse_journal, AutoRule, Diagnostic, Item, Posting, Span};
use ledger_query::{Query, TxnInfo};
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;

use crate::types::RulePostingDto;
use crate::writer::FileState;

/// Which way money moves; controls the sign of generated postings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Expense,
    Income,
    Transfer,
}

impl Direction {
    pub fn sign(&self) -> Decimal {
        match self {
            Direction::Expense => Decimal::ONE,
            Direction::Income => -Decimal::ONE,
            Direction::Transfer => Decimal::ONE,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Expense => "expense",
            Direction::Income => "income",
            Direction::Transfer => "transfer",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RulePosting {
    pub account: String,
    pub percent: Option<Decimal>,
    pub amount: Option<ledger_core::Amount>,
}

#[derive(Debug, Clone)]
pub struct RuleDef {
    pub id: String,
    pub query_raw: String,
    pub query: Option<Query>,
    pub enabled: bool,
    pub order: i64,
    pub from_account: Option<String>,
    pub template: Vec<RulePosting>,
    pub span: Span,
    pub raw: String,
    pub notes: Vec<String>,
}

impl RuleDef {
    pub fn is_template(&self) -> bool {
        self.template
            .iter()
            .any(|p| p.percent.is_some() || p.amount.is_some())
    }

    pub fn source_account(&self) -> Option<&str> {
        self.from_account.as_deref().or_else(|| {
            self.template
                .iter()
                .find(|p| p.percent.is_none() && p.amount.is_none())
                .map(|p| p.account.as_str())
        })
    }

    pub fn postings_dto(&self) -> Vec<RulePostingDto> {
        self.template
            .iter()
            .map(|p| RulePostingDto {
                account: p.account.clone(),
                percent: p.percent.map(ledger_core::printer::format_percent),
                amount: p.amount.as_ref().map(crate::amounts::to_dto),
                is_source: p.percent.is_none() && p.amount.is_none(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub rules: Vec<RuleDef>,
    pub diagnostics: Vec<Diagnostic>,
    pub state: Option<FileState>,
}

pub fn rule_id_for_query(query: &str) -> String {
    let norm = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let hash = blake3::hash(norm.as_bytes()).to_hex().to_string();
    format!("r-{}", &hash[..8])
}

fn tag_value<'a>(tags: &'a [ledger_core::Tag], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.key == key)
        .and_then(|t| t.value.as_deref())
}

impl RuleSet {
    pub fn parse(file: &str, src: &str, state: Option<FileState>) -> Self {
        let out = parse_journal(file, src);
        let mut rules = Vec::new();
        let mut diagnostics = out.diagnostics.clone();
        for (idx, item) in out.journal.items.iter().enumerate() {
            let Item::AutoRule(rule) = item else { continue };
            rules.push(rule_from_auto(rule, idx, &mut diagnostics, src, file));
        }
        Self {
            rules,
            diagnostics,
            state,
        }
    }

    pub fn enabled_rules(&self) -> impl Iterator<Item = &RuleDef> {
        self.rules.iter().filter(|r| r.enabled)
    }

    /// First matching rule in `order` sequence wins.
    pub fn first_match(&self, info: &TxnInfo<'_>) -> Option<&RuleDef> {
        let mut candidates: Vec<&RuleDef> = self.enabled_rules().collect();
        candidates.sort_by_key(|r| (r.order, r.id.clone()));
        candidates
            .into_iter()
            .find(|r| r.query.as_ref().map(|q| q.matches(info)).unwrap_or(false))
    }

    pub fn get(&self, id: &str) -> Option<&RuleDef> {
        self.rules.iter().find(|r| r.id == id)
    }

    pub fn render(&self) -> String {
        let mut out = String::from(
            "; MoneyTracker rules — read by the app, never included by index.ledger.\n",
        );
        for (i, r) in self.rules.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&render_rule_block(r));
            out.push('\n');
        }
        out
    }
}

fn rule_from_auto(
    rule: &AutoRule,
    idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
    src: &str,
    file: &str,
) -> RuleDef {
    let mut notes = Vec::new();
    let (query, query_note) = match Query::parse(&rule.query) {
        Ok(q) => (Some(q), None),
        Err(e) => {
            notes.push(format!("query does not parse: {e}"));
            (None, Some(e.to_string()))
        }
    };
    if let Some(msg) = query_note {
        diagnostics.push(
            Diagnostic::warning(
                "rules.bad_query",
                format!("rule `{}`: {msg}", rule.query),
                file,
                rule.span,
            )
            .with_position(src),
        );
    }
    let id = tag_value(&rule.tags, "rule")
        .map(|s| s.to_string())
        .unwrap_or_else(|| rule_id_for_query(&rule.query));
    let order = tag_value(&rule.tags, "order")
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(idx as i64 * 10);
    let enabled = tag_value(&rule.tags, "enabled")
        .map(|s| !matches!(s.trim(), "false" | "0" | "no" | "off"))
        .unwrap_or(true);
    let from_account = tag_value(&rule.tags, "from").map(|s| s.to_string());

    let template = rule
        .postings
        .iter()
        .map(|p: &Posting| RulePosting {
            account: p.account.clone(),
            percent: p.percent,
            amount: p.amount.clone(),
        })
        .collect();

    RuleDef {
        id,
        query_raw: rule.query.clone(),
        query,
        enabled,
        order,
        from_account,
        template,
        span: rule.span,
        raw: src
            .get(rule.span.start..rule.span.end)
            .unwrap_or_default()
            .to_string(),
        notes,
    }
}

pub fn render_rule_block(rule: &RuleDef) -> String {
    let meta = format!(
        "rule:{} order:{} enabled:{}{}",
        rule.id,
        rule.order,
        rule.enabled,
        rule.from_account
            .as_ref()
            .map(|a| format!(" from:{a}"))
            .unwrap_or_default()
    );
    let postings: Vec<Posting> = rule
        .template
        .iter()
        .map(|p| Posting {
            span: Span::default(),
            status: None,
            account: p.account.clone(),
            virtual_kind: ledger_core::VirtualKind::Real,
            amount: p.amount.clone(),
            percent: p.percent,
            price: None,
            balance_assertion: None,
            comment: None,
            tags: Vec::new(),
        })
        .collect();
    format!(
        "= {}  ; {}\n{}",
        rule.query_raw,
        meta,
        ledger_core::printer::print_postings(&postings)
    )
}

/// Split `total` across percentages, giving the remainder to the last entry so
/// the sum is always exact (no ±0.01 drift).
pub fn split_percent(total: Decimal, percents: &[Decimal], precision: u32) -> Vec<Decimal> {
    if percents.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(percents.len());
    let mut allocated = Decimal::ZERO;
    for (i, pct) in percents.iter().enumerate() {
        if i + 1 == percents.len() {
            out.push(total - allocated);
        } else {
            let share = (total * *pct / Decimal::from(100))
                .round_dp_with_strategy(precision, RoundingStrategy::MidpointAwayFromZero);
            allocated += share;
            out.push(share);
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct RuleExpansion {
    /// Classification / extra postings, already signed for the direction.
    pub postings: Vec<(String, ledger_core::Amount)>,
    pub funding: Option<String>,
}

impl RuleDef {
    /// Expand the template for a `total` amount (absolute value).
    pub fn expand(&self, total: &ledger_core::Amount, dir: Direction) -> RuleExpansion {
        let sign = dir.sign();
        let precision = total.precision();
        let mut postings = Vec::new();

        let percent_rows: Vec<&RulePosting> = self
            .template
            .iter()
            .filter(|p| p.percent.is_some())
            .collect();
        let percents: Vec<Decimal> = percent_rows.iter().filter_map(|p| p.percent).collect();
        let shares = split_percent(total.quantity, &percents, precision);
        for (row, share) in percent_rows.iter().zip(shares) {
            let amount = ledger_core::Amount::canonical(share * sign, &total.commodity, precision);
            postings.push((row.account.clone(), amount));
        }

        for row in self.template.iter().filter(|p| p.percent.is_none()) {
            if let Some(amount) = &row.amount {
                let mut a = amount.clone();
                if dir == Direction::Income {
                    a.quantity = -a.quantity;
                }
                postings.push((row.account.clone(), a));
            }
        }

        RuleExpansion {
            postings,
            funding: self.source_account().map(|s| s.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedRuleLine {
    pub query: String,
    pub from_account: Option<String>,
    pub template: Vec<RulePosting>,
}

/// Parse the capture syntax: `= <query> -> <acct>[=70%] , <acct> ... [from <acct>]`
pub fn parse_rule_line(input: &str) -> Result<ParsedRuleLine, String> {
    let body = input.trim().strip_prefix('=').unwrap_or(input).trim();
    if body.is_empty() || body == "-" {
        return Err("rule is empty".to_string());
    }
    let (head, tail) = match body.split_once("->") {
        Some((h, t)) => (h.trim(), t.trim()),
        None => {
            return Err(
                "expected `= <query> -> <account>[=70%] , ...` (use `->` before the template)"
                    .to_string(),
            )
        }
    };
    let mut query = head.to_string();
    let mut from_account = None;
    if let Some((q, from)) = tail.rsplit_once(" from ") {
        let acct = from.trim();
        if acct.is_empty() || acct.contains(',') {
            return Err("`from` must be followed by a single account".to_string());
        }
        from_account = Some(ensure_prefix(acct, "assets"));
        query = head.to_string();
        let _ = q;
    }

    let template_src = if from_account.is_some() {
        let (before, _) = tail.rsplit_once(" from ").unwrap();
        before
    } else {
        tail
    };

    let mut template = Vec::new();
    for part in template_src.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (acct, pct) = match part.rsplit_once('=') {
            Some((a, p)) if p.trim().ends_with('%') => {
                let val = p.trim().trim_end_matches('%').trim();
                let pct: Decimal = val
                    .parse()
                    .map_err(|_| format!("`{}` is not a percentage", p.trim()))?;
                if pct.is_sign_negative() || pct > Decimal::from(100) {
                    return Err(format!("percentage {}% is out of range", pct));
                }
                (a.trim(), Some(pct))
            }
            _ => (part, None),
        };
        if acct.is_empty() {
            return Err("template contains an empty account".to_string());
        }
        template.push(RulePosting {
            account: ensure_prefix(acct, "expenses"),
            percent: pct,
            amount: None,
        });
    }
    if template.is_empty() {
        return Err("template needs at least one account".to_string());
    }
    if template.len() > 1 && template.iter().all(|p| p.percent.is_none()) && from_account.is_none()
    {
        return Err("split templates need percentages, e.g. `餐饮=70%, 咖啡豆=30%`".to_string());
    }
    let percent_sum: Decimal = template.iter().filter_map(|p| p.percent).sum();
    if percent_sum > Decimal::from(100) {
        return Err(format!("percentages add up to {percent_sum}% (> 100%)"));
    }

    Ok(ParsedRuleLine {
        query: query.trim().to_string(),
        from_account,
        template,
    })
}

/// Normalize an account and give it a top-level prefix when the user typed a
/// shorthand (`支付宝` -> `assets:支付宝`, `餐饮` -> `expenses:餐饮`).
fn ensure_prefix(account: &str, prefix: &str) -> String {
    let name = normalize(account);
    if name.contains(':') {
        name
    } else {
        format!("{prefix}:{name}")
    }
}

fn normalize(account: &str) -> String {
    account
        .split(':')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(":")
        .to_lowercase()
}

/// Build a `RuleDef` from capture input, reusing the id of an existing rule
/// with the same query.
pub fn rule_from_line(line: &ParsedRuleLine, existing: &RuleSet) -> RuleDef {
    let id = existing
        .rules
        .iter()
        .find(|r| {
            r.query_raw.trim().eq_ignore_ascii_case(line.query.trim())
                || rule_id_for_query(&r.query_raw) == rule_id_for_query(&line.query)
        })
        .map(|r| r.id.clone())
        .unwrap_or_else(|| rule_id_for_query(&line.query));
    let order = existing
        .rules
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.order)
        .unwrap_or_else(|| existing.rules.iter().map(|r| r.order).max().unwrap_or(0) + 10);
    let query = Query::parse(&line.query).ok();
    RuleDef {
        id,
        query_raw: line.query.clone(),
        query,
        enabled: true,
        order,
        from_account: line.from_account.clone(),
        template: line.template.clone(),
        span: Span::default(),
        raw: String::new(),
        notes: Vec::new(),
    }
}

/// Build `TxnInfo` for a draft so rules can be evaluated before writing.
#[allow(clippy::too_many_arguments)]
pub fn draft_info<'a>(
    date: NaiveDate,
    payee: &'a str,
    accounts: &'a [String],
    tags: &'a [(String, Option<String>)],
    amounts: &'a [Decimal],
    commodities: &'a [String],
    status: &'a str,
) -> TxnInfo<'a> {
    TxnInfo {
        date: Some(date),
        payee,
        code: "",
        status,
        accounts,
        tags,
        amounts,
        commodities,
    }
}

pub fn template_map(rule: &RuleDef) -> HashMap<String, Option<Decimal>> {
    rule.template
        .iter()
        .map(|p| (p.account.clone(), p.percent))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_query::TxnInfo;

    const SRC: &str = "; rules
= desc:星巴克  ; rule:r-abc123 order:10 enabled:true from:assets:wallet:alipay
    expenses:food:coffee    70.00%
    expenses:food:beans     30.00%

= desc:工资 and amt:>100  ; rule:r-sal001 order:20 enabled:true
    income:salary          100.00%
";

    fn ruleset() -> RuleSet {
        RuleSet::parse("rules.ledger", SRC, None)
    }

    #[test]
    fn parses_meta_and_templates() {
        let rs = ruleset();
        assert_eq!(rs.rules.len(), 2);
        let r = &rs.rules[0];
        assert_eq!(r.id, "r-abc123");
        assert_eq!(r.order, 10);
        assert!(r.enabled);
        assert_eq!(r.from_account.as_deref(), Some("assets:wallet:alipay"));
        assert_eq!(r.template.len(), 2);
        assert_eq!(r.template[0].percent, Some(Decimal::new(7000, 2)));
    }

    #[test]
    fn first_match_wins_in_order() {
        let rs = ruleset();
        let accounts = vec![
            "expenses:food:coffee".to_string(),
            "assets:cash".to_string(),
        ];
        let tags: Vec<(String, Option<String>)> = vec![];
        let amounts = vec![Decimal::new(450, 2)];
        let commodities = vec!["CNY".to_string()];
        let info = TxnInfo {
            date: None,
            payee: "星巴克咖啡",
            code: "",
            status: "",
            accounts: &accounts,
            tags: &tags,
            amounts: &amounts,
            commodities: &commodities,
        };
        assert_eq!(rs.first_match(&info).unwrap().id, "r-abc123");
    }

    #[test]
    fn remainder_goes_to_the_last_posting() {
        let parts = split_percent(
            Decimal::new(10000, 2),
            &[
                Decimal::new(3333, 2),
                Decimal::new(3333, 2),
                Decimal::new(3334, 2),
            ],
            2,
        );
        assert_eq!(parts.iter().sum::<Decimal>(), Decimal::new(10000, 2));
        assert_eq!(parts[0], Decimal::new(3333, 2));
        assert_eq!(parts[2], Decimal::new(3334, 2));
    }

    #[test]
    fn remainder_absorbs_rounding() {
        let parts = split_percent(
            Decimal::new(10000, 2),
            &[
                Decimal::new(3333, 2),
                Decimal::new(3333, 2),
                Decimal::new(3333, 2),
            ],
            2,
        );
        assert_eq!(parts.iter().sum::<Decimal>(), Decimal::new(10000, 2));
    }

    #[test]
    fn expands_with_sign_for_income() {
        let rs = ruleset();
        let rule = &rs.rules[1];
        let total = ledger_core::Amount::canonical(Decimal::new(1200000, 2), "CNY", 2);
        let exp = rule.expand(&total, Direction::Income);
        assert_eq!(exp.postings.len(), 1);
        assert_eq!(exp.postings[0].1.quantity, Decimal::new(-1200000, 2));
        assert_eq!(exp.funding, None);
    }

    #[test]
    fn parses_capture_rule_line() {
        let p = parse_rule_line("= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝").unwrap();
        assert_eq!(p.query, "desc:星巴克");
        assert_eq!(p.from_account.as_deref(), Some("assets:支付宝"));
        assert_eq!(p.template.len(), 2);
        assert_eq!(p.template[1].account, "expenses:咖啡豆");
    }

    #[test]
    fn rejects_bad_rule_lines() {
        assert!(parse_rule_line("= desc:x").is_err());
        assert!(parse_rule_line("= desc:x -> 餐饮=120%").is_err());
        assert!(parse_rule_line("= desc:x -> a=70%, b=50%").is_err());
    }

    #[test]
    fn rendering_round_trips_through_the_parser() {
        let rs = ruleset();
        let rendered = rs.render();
        let again = RuleSet::parse("rules.ledger", &rendered, None);
        assert_eq!(again.rules.len(), 2);
        assert_eq!(again.rules[0].id, "r-abc123");
        assert_eq!(again.rules[0].template.len(), 2);
        assert_eq!(again.rules[1].order, 20);
    }
}

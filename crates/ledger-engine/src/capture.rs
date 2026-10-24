//! The one-line capture language: `[#date] [*|!] description amount [@acct]...
//! [+tag]... [; comment]`, plus `=` rule lines, `/` search and `:` commands.
//!
//! All parsing happens here (not in the frontend) so the preview and the
//! committed block are produced by exactly the same code.

use chrono::{Duration, Local, NaiveDate};
use ledger_core::{Amount, Posting, Price, PriceKind, Status, Transaction};
use ledger_query::TxnInfo;
use rust_decimal::Decimal;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::amounts::balance;
use crate::load::{normalize_account, AccountInfo, Vault};
use crate::rules::{
    parse_rule_line, rule_from_line, split_percent, Direction, ParsedRuleLine, RuleDef,
};
use crate::types::{CaptureParseDto, PostingDto, RulePreviewDto};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Empty,
    Transaction,
    Rule,
    Search,
    Command,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Empty => "empty",
            Kind::Transaction => "transaction",
            Kind::Rule => "rule",
            Kind::Search => "search",
            Kind::Command => "command",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedCapture {
    pub kind: Kind,
    pub date: Option<NaiveDate>,
    pub status: Option<Status>,
    pub payee: String,
    pub tags: Vec<ledger_core::Tag>,
    pub comment: Option<String>,
    pub amount: Option<Amount>,
    pub price: Option<(PriceKind, Amount)>,
    pub sign: i8,
    pub accounts: Vec<String>,
    pub transfer: bool,
    pub rule_line: Option<ParsedRuleLine>,
    pub search: Option<String>,
    pub command: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Default for ParsedCapture {
    fn default() -> Self {
        Self {
            kind: Kind::Empty,
            date: None,
            status: None,
            payee: String::new(),
            tags: Vec::new(),
            comment: None,
            amount: None,
            price: None,
            sign: 0,
            accounts: Vec::new(),
            transfer: false,
            rule_line: None,
            search: None,
            command: None,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExpandedCapture {
    pub txn: Transaction,
    pub postings: Vec<PostingDto>,
    pub canonical: String,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub rule_id: Option<String>,
    pub direction: Direction,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct CaptureAnalysis {
    pub parsed: ParsedCapture,
    pub expanded: Option<ExpandedCapture>,
    pub rule_preview: Option<RulePreviewDto>,
    pub rule_id: Option<String>,
    pub canonical: String,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl CaptureAnalysis {
    pub fn empty() -> Self {
        Self {
            parsed: ParsedCapture::default(),
            expanded: None,
            rule_preview: None,
            rule_id: None,
            canonical: String::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn to_dto(&self) -> CaptureParseDto {
        let postings = self
            .expanded
            .as_ref()
            .map(|e| e.postings.clone())
            .unwrap_or_default();
        CaptureParseDto {
            kind: self.parsed.kind.as_str().to_string(),
            canonical_text: self.canonical.clone(),
            postings,
            warnings: self.warnings.clone(),
            errors: self
                .errors
                .iter()
                .map(|msg| crate::types::DiagnosticDto {
                    severity: "error".into(),
                    code: "capture.parse".into(),
                    message: msg.clone(),
                    file: String::new(),
                    line: 0,
                    col: 0,
                })
                .collect(),
            rule_id: self.rule_id.clone(),
            rule_preview: self.rule_preview.clone(),
            summary: self.expanded.as_ref().map(|e| e.summary.clone()),
            date: self.parsed.date.map(|d| d.to_string()),
            payee: Some(self.parsed.payee.clone()).filter(|p| !p.is_empty()),
            amount_display: self.parsed.amount.as_ref().map(|a| a.format()),
        }
    }
}

/// Split a capture line into tokens, honouring `"quoted payees"` and stopping
/// at the first `;` (which starts the free-form comment).
fn tokenize(line: &str) -> (Vec<String>, Option<String>) {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ';' if !in_quotes => {
                let rest: String = chars.collect();
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                let comment = rest.trim().to_string();
                return (
                    tokens,
                    if comment.is_empty() {
                        None
                    } else {
                        Some(comment)
                    },
                );
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            c => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    (tokens, None)
}

fn parse_date_directive(raw: &str) -> Option<NaiveDate> {
    let today = Local::now().date_naive();
    let s = raw.trim().trim_start_matches('#').trim();
    match s.to_lowercase().as_str() {
        "today" | "今天" | "今日" => return Some(today),
        "yesterday" | "昨天" => return Some(today - Duration::days(1)),
        "前天" => return Some(today - Duration::days(2)),
        "明天" => return Some(today + Duration::days(1)),
        _ => {}
    }
    // +3d / -1d / 3天
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None => (1i64, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let suffix = rest[digits.len()..].trim().to_lowercase();
        if matches!(suffix.as_str(), "d" | "day" | "days" | "天") {
            let n: i64 = digits.parse().ok()?;
            return Some(today + Duration::days(sign * n));
        }
    }
    ledger_core::parser::parse_date_str(s)
}

fn normalize_commodity(c: &str) -> String {
    if c.chars().all(|ch| ch.is_ascii_alphabetic()) && c.len() == 3 {
        c.to_ascii_uppercase()
    } else {
        c.to_string()
    }
}

/// Parse an amount-ish token (`4.5`, `+12000`, `¥4.5`, `4.5cny`,
/// `100usd@@720cny`). Returns the amount, an optional inline price and the sign.
/// `(amount, optional inline price, sign)` produced from one capture token.
pub type AmountToken = (Amount, Option<(PriceKind, Amount)>, i8);

pub fn parse_amount_token(token: &str) -> Option<AmountToken> {
    if !token.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if token.len() > 1 && token.chars().next().is_some_and(|c| c.is_alphabetic()) {
        // `iPhone15` is a description, `USD100` is not part of the grammar.
        return None;
    }
    let (sign, body) = match token.as_bytes().first() {
        Some(b'+') => (1i8, &token[1..]),
        Some(b'-') => (-1i8, &token[1..]),
        _ => (0i8, token),
    };
    let (main, price) = if let Some((a, b)) = body.split_once("@@") {
        (a, Some((PriceKind::Total, b)))
    } else if let Some((a, b)) = body.split_once('@') {
        (a, Some((PriceKind::Unit, b)))
    } else {
        (body, None)
    };
    let amount = ledger_core::parse_amount(main).ok()?;
    let amount = Amount::canonical(
        amount.quantity,
        &normalize_commodity(&amount.commodity),
        amount.precision(),
    );
    let price = match price {
        Some((kind, text)) => {
            let p = ledger_core::parse_amount(text).ok()?;
            Some((
                kind,
                Amount::canonical(
                    p.quantity,
                    &normalize_commodity(&p.commodity),
                    p.precision(),
                ),
            ))
        }
        None => None,
    };
    Some((amount, price, sign))
}

fn looks_like_date(token: &str) -> bool {
    let parts: Vec<&str> = token.split(['-', '/', '.']).collect();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].chars().all(|c| c.is_ascii_digit())
}

pub fn parse_capture(line: &str) -> ParsedCapture {
    let trimmed = line.trim();
    let mut parsed = ParsedCapture::default();
    if trimmed.is_empty() {
        return parsed;
    }
    if let Some(rest) = trimmed.strip_prefix('=') {
        parsed.kind = Kind::Rule;
        match parse_rule_line(&format!("={rest}")) {
            Ok(line) => {
                if ledger_query::Query::parse(&line.query).is_err() {
                    parsed.errors.push(format!(
                        "query `{}` does not parse — use keys like desc:, acct:, amt:, tag:, date:",
                        line.query
                    ));
                }
                parsed.rule_line = Some(line);
            }
            Err(e) => parsed.errors.push(e),
        }
        return parsed;
    }
    if let Some(rest) = trimmed.strip_prefix('/') {
        parsed.kind = Kind::Search;
        parsed.search = Some(rest.trim().to_string());
        return parsed;
    }
    if let Some(rest) = trimmed.strip_prefix(':') {
        parsed.kind = Kind::Command;
        parsed.command = Some(rest.trim().to_string());
        return parsed;
    }

    parsed.kind = Kind::Transaction;
    let (tokens, comment) = tokenize(trimmed);
    parsed.comment = comment;
    let mut words: Vec<String> = Vec::new();
    let mut seen_date = false;
    let mut seen_amount = false;

    for (i, token) in tokens.iter().enumerate() {
        if let Some(rest) = token.strip_prefix('#') {
            match parse_date_directive(rest) {
                Some(d) => {
                    if seen_date {
                        parsed
                            .warnings
                            .push("more than one date given; keeping the first".into());
                    } else {
                        parsed.date = Some(d);
                        seen_date = true;
                    }
                }
                None => parsed
                    .errors
                    .push(format!("cannot understand the date `{rest}`")),
            }
            continue;
        }
        if token == "*" || token == "!" {
            parsed.status = if token == "*" {
                Some(Status::Cleared)
            } else {
                Some(Status::Pending)
            };
            continue;
        }
        if token == ">" {
            parsed.transfer = true;
            continue;
        }
        if let Some(rest) = token.strip_prefix('@') {
            if rest.trim().is_empty() {
                parsed.errors.push("@ needs an account name".into());
            } else {
                parsed.accounts.push(normalize_account(rest));
            }
            continue;
        }
        if let Some(rest) = token.strip_prefix('+') {
            if !rest.is_empty() && parse_amount_token(rest).is_none() {
                match rest.split_once(':') {
                    Some((k, v)) => parsed.tags.push(ledger_core::Tag::kv(k.trim(), v.trim())),
                    None => parsed.tags.push(ledger_core::Tag::bare(rest.trim())),
                }
                continue;
            }
        }
        if token.starts_with(':') && token.len() > 2 && token.ends_with(':') {
            for tag in token.split(':').filter(|t| !t.trim().is_empty()) {
                parsed.tags.push(ledger_core::Tag::bare(tag.trim()));
            }
            continue;
        }
        if !seen_amount {
            if let Some((amount, price, sign)) = parse_amount_token(token) {
                parsed.amount = Some(amount);
                parsed.price = price;
                parsed.sign = sign;
                seen_amount = true;
                continue;
            }
        }
        if i == 0 && looks_like_date(token) {
            if let Some(d) = ledger_core::parser::parse_date_str(token) {
                parsed.date = Some(d);
                seen_date = true;
                continue;
            }
        }
        words.push(token.clone());
    }

    parsed.payee = words.join(" ").trim().to_string();
    if parsed.payee.is_empty() && parsed.amount.is_none() {
        parsed
            .warnings
            .push("no description yet — rules fall back to the description/defaults".into());
    }
    if parsed.accounts.len() > 2 {
        parsed
            .warnings
            .push("more than two @accounts given; extra ones are ignored".into());
    }
    parsed
}

fn kind_of_account(vault: &Vault, account: &str) -> char {
    vault
        .accounts
        .get(account)
        .map(|i: &AccountInfo| i.kind)
        .unwrap_or('U')
}

fn detect_direction(parsed: &ParsedCapture, vault: &Vault) -> Direction {
    if parsed.transfer {
        return Direction::Transfer;
    }
    if parsed.sign > 0 {
        return Direction::Income;
    }
    if parsed.sign < 0 {
        return Direction::Expense;
    }
    for acct in &parsed.accounts {
        if kind_of_account(vault, acct) == 'R' {
            return Direction::Income;
        }
    }
    Direction::Expense
}

fn posting(account: &str, amount: Option<Amount>, price: Option<Price>) -> Posting {
    Posting {
        amount,
        price,
        ..Posting::new(account.to_string(), None)
    }
}

/// Expand a parsed capture line into a balanced transaction.
pub fn expand(vault: &Vault, parsed: &ParsedCapture) -> ExpandedCapture {
    let mut warnings = parsed.warnings.clone();
    let mut errors: Vec<String> = parsed.errors.clone();
    let direction = detect_direction(parsed, vault);
    let table = &vault.commodities;

    let raw_amount = parsed
        .amount
        .clone()
        .unwrap_or_else(|| Amount::canonical(Decimal::ZERO, &table.base, table.base_precision));
    let commodity = if raw_amount.commodity.is_empty() {
        table.base.clone()
    } else {
        raw_amount.commodity.clone()
    };
    let precision = table.precision_of(&commodity);
    let total = Amount::canonical(raw_amount.quantity.abs(), &commodity, precision);

    // Resolve shorthand account names such as `@微信` against the vault.
    let mut accounts: Vec<String> = Vec::new();
    for acct in &parsed.accounts {
        let (resolved, notes) = vault.resolve_account(acct);
        warnings.extend(notes);
        accounts.push(resolved);
    }

    let mut postings: Vec<Posting> = Vec::new();
    let mut rule_id: Option<String> = None;
    let is_transfer = direction == Direction::Transfer;
    // Set only when the category side had to fall back to the unclassified account.
    let mut unclassified = false;

    if is_transfer {
        if accounts.len() < 2 {
            errors.push("a transfer needs two accounts: `> @from > @to`".into());
        }
        let src = accounts
            .first()
            .map(|a| prefixed(a, "assets"))
            .unwrap_or_else(|| vault.opts.default_funding.clone());
        let dst = accounts
            .get(1)
            .map(|a| prefixed(a, "assets"))
            .unwrap_or_else(|| vault.opts.default_funding.clone());
        postings.push(posting(&src, Some(total.neg()), None));
        postings.push(posting(&dst, Some(total.clone()), None));
    } else {
        let mut category: Option<String> = None;
        let mut funding: Option<String> = None;
        let tag_pairs: Vec<(String, Option<String>)> = parsed
            .tags
            .iter()
            .map(|t| (t.key.clone(), t.value.clone()))
            .collect();
        let info = TxnInfo {
            date: parsed.date,
            payee: &parsed.payee,
            code: "",
            status: parsed.status.map(|s| s.marker()).unwrap_or(""),
            accounts: &accounts,
            tags: &tag_pairs,
            amounts: &[total.quantity],
            commodities: std::slice::from_ref(&total.commodity),
        };
        let rule_hit = vault.rules.first_match(&info).cloned();
        // `咖啡 4.5 @微信 +餐饮` — the tag names the category, the account is the wallet.
        let tag_category = parsed
            .tags
            .iter()
            .find(|t| t.value.is_none() && !matches!(t.key.as_str(), "inbox" | "txn" | "rule"))
            .map(|t| prefixed(&t.key, "expenses"));
        match accounts.len() {
            0 => {}
            1 => {
                let acct = accounts[0].clone();
                match kind_of_account(vault, &acct) {
                    'A' | 'L' => funding = Some(prefixed(&acct, "assets")),
                    'R' | 'X' | 'E' => category = Some(prefixed(&acct, "expenses")),
                    _ => {
                        // An undeclared name is the funding account when a rule
                        // supplies the categories (or for income), otherwise the
                        // category itself.
                        if rule_hit.is_some()
                            || direction == Direction::Income
                            || tag_category.is_some()
                        {
                            funding = Some(prefixed(&acct, "assets"));
                            if let Some(cat) = tag_category.clone() {
                                category = Some(cat);
                            }
                        } else {
                            warnings.push(format!(
                                "`{acct}` is not declared; treated as the category account"
                            ));
                            category = Some(prefixed(&acct, "expenses"));
                        }
                    }
                }
            }
            _ => {
                category = Some(prefixed(&accounts[0], "expenses"));
                funding = Some(prefixed(&accounts[1], "assets"));
            }
        }

        let both_funding = category
            .as_deref()
            .map(|a| matches!(kind_of_account(vault, a), 'A' | 'L'))
            .unwrap_or(false)
            && funding
                .as_deref()
                .map(|a| matches!(kind_of_account(vault, a), 'A' | 'L'))
                .unwrap_or(false);
        if both_funding {
            // `@招行 @现金` between two assets reads as a transfer.
            let src = category.clone().unwrap();
            let dst = funding.clone().unwrap();
            postings.push(posting(&src, Some(total.neg()), None));
            postings.push(posting(&dst, Some(total.clone()), None));
        } else {
            if let Some(rule) = rule_hit.clone() {
                rule_id = Some(rule.id.clone());
                let expansion = rule.expand(&total, direction);
                for (account, amount) in expansion.postings {
                    postings.push(posting(&account, Some(amount), None));
                }
                if funding.is_none() {
                    funding = expansion
                        .funding
                        .or_else(|| Some(vault.opts.default_funding.clone()));
                }
                if postings.is_empty() {
                    warnings.push(format!(
                        "rule {} has no template postings; falling back to a single category",
                        rule.id
                    ));
                }
            }
            if postings.is_empty() {
                let cat = match category.or_else(|| tag_category.clone()) {
                    Some(c) => c,
                    None => {
                        warnings.push(format!(
                            "no rule and no category account — booked to {} (Inbox)",
                            vault.opts.default_unclassified
                        ));
                        unclassified = true;
                        vault.opts.default_unclassified.clone()
                    }
                };
                let signed = if direction == Direction::Income {
                    total.neg()
                } else {
                    total.clone()
                };
                let price = parsed.price.as_ref().map(|(kind, amount)| Price {
                    kind: *kind,
                    amount: amount.clone(),
                    span: Default::default(),
                });
                postings.push(posting(&cat, Some(signed), price));
            } else if let Some((kind, price_amount)) = &parsed.price {
                if let Some(first) = postings.first_mut() {
                    first.price = Some(Price {
                        kind: *kind,
                        amount: price_amount.clone(),
                        span: Default::default(),
                    });
                }
            }
            let funding = funding.unwrap_or_else(|| vault.opts.default_funding.clone());
            if is_zero_amount(&total) {
                errors.push("the amount is zero".into());
            }
            postings.push(posting(&funding, None, None));
        }
    }

    let table_mut = table.clone();
    if let Err(issues) = balance(&mut postings, &table_mut, &table.base) {
        for issue in issues {
            errors.push(issue.message);
        }
    }
    warnings.extend(errors.iter().cloned());

    let mut tags: Vec<ledger_core::Tag> = parsed.tags.clone();
    if unclassified && !tags.iter().any(|t| t.key == "inbox") {
        tags.push(ledger_core::Tag::bare("inbox"));
    }
    let mut comment_parts: Vec<String> = vec![format!(
        "rule:{}",
        rule_id.clone().unwrap_or_else(|| "none".to_string())
    )];
    for t in &tags {
        let rendered = t.render();
        if !comment_parts.contains(&rendered) {
            comment_parts.push(rendered);
        }
    }
    if let Some(c) = &parsed.comment {
        comment_parts.push(c.clone());
    }

    let txn = Transaction {
        span: Default::default(),
        date: parsed.date.unwrap_or_else(|| Local::now().date_naive()),
        date2: None,
        status: parsed.status,
        code: None,
        payee: parsed.payee.clone(),
        comment: Some(comment_parts.join(" ")),
        tags,
        postings,
    };
    let canonical = ledger_core::print_transaction(&txn);
    let postings_dto: Vec<PostingDto> = txn
        .postings
        .iter()
        .map(|p| crate::load::posting_dto(p, table))
        .collect();
    let summary = postings_dto
        .iter()
        .map(|p| format!("{} {}", p.account, p.display))
        .collect::<Vec<_>>()
        .join("  →  ");

    ExpandedCapture {
        txn,
        postings: postings_dto,
        canonical,
        warnings,
        errors,
        rule_id,
        direction,
        summary,
    }
}

/// Give a shorthand account name its top-level prefix (`微信` -> `assets:微信`).
fn prefixed(name: &str, prefix: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("{prefix}:{name}")
    }
}

fn is_zero_amount(amount: &Amount) -> bool {
    amount.quantity.is_zero()
}

/// Full analysis used by both `capture_parse` and `capture_commit`.
pub fn analyze(vault: &Vault, input: &str) -> CaptureAnalysis {
    let parsed = parse_capture(input);
    let mut analysis = CaptureAnalysis {
        parsed: parsed.clone(),
        expanded: None,
        rule_preview: None,
        rule_id: None,
        canonical: String::new(),
        warnings: parsed.warnings.clone(),
        errors: parsed.errors.clone(),
    };

    match parsed.kind {
        Kind::Empty => {}
        Kind::Rule => {
            if let Some(line) = &parsed.rule_line {
                let rule = rule_from_line(line, &vault.rules);
                if vault.rules.get(&rule.id).is_some() {
                    analysis
                        .warnings
                        .push(format!("this updates the existing rule {}", rule.id));
                }
                analysis.rule_id = Some(rule.id.clone());
                analysis.canonical = crate::rules::render_rule_block(&rule);
                analysis.rule_preview = Some(RulePreviewDto {
                    query: rule.query_raw.clone(),
                    from_account: rule.source_account().map(|s| s.to_string()),
                    template: rule.postings_dto(),
                });
                analysis.warnings.extend(rule.notes.clone());
            }
        }
        Kind::Search => {
            analysis.canonical = parsed.search.clone().unwrap_or_default();
        }
        Kind::Command => {
            analysis.canonical = parsed.command.clone().unwrap_or_default();
        }
        Kind::Transaction => {
            let expanded = expand(vault, &parsed);
            analysis.warnings = expanded.warnings.clone();
            analysis.errors = expanded.errors.clone();
            analysis.canonical = expanded.canonical.clone();
            analysis.rule_id = expanded.rule_id.clone();
            analysis.expanded = Some(expanded);
        }
    }
    analysis
}

/// UUIDv7-shaped identifier: 48-bit millisecond timestamp + hashed randomness.
pub fn new_txn_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ms = now.as_millis() as u64;
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&ms.to_be_bytes());
    hasher.update(&counter.to_be_bytes());
    hasher.update(&std::process::id().to_be_bytes());
    hasher.update(&now.subsec_nanos().to_be_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    bytes[6..].copy_from_slice(&hash.as_bytes()[..10]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Percent-split helper re-exported for tests and previews.
pub fn split(total: Decimal, percents: &[Decimal], precision: u32) -> Vec<Decimal> {
    split_percent(total, percents, precision)
}

/// Used by the rule preview to render a template with percentages.
pub fn preview_rule_postings(rule: &RuleDef) -> Vec<PostingDto> {
    rule.template
        .iter()
        .map(|p| PostingDto {
            account: p.account.clone(),
            amount: p.amount.as_ref().map(crate::amounts::to_dto),
            commodity: p
                .amount
                .as_ref()
                .map(|a| a.commodity.clone())
                .unwrap_or_default(),
            quantity: p
                .amount
                .as_ref()
                .map(|a| a.quantity.to_string())
                .unwrap_or_else(|| p.percent.map(|pct| format!("{pct}%")).unwrap_or_default()),
            display: p.amount.as_ref().map(|a| a.format()).unwrap_or_else(|| {
                p.percent
                    .map(ledger_core::printer::format_percent)
                    .unwrap_or_default()
            }),
            price: None,
            price_kind: None,
            assertion: None,
            comment: None,
            virtual_kind: "real".to_string(),
            tags: Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::Vault;
    use crate::types::VaultCreateOptions;
    use crate::vault::{create_vault, VaultOptions};
    use tempfile::tempdir;

    fn test_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempdir().unwrap();
        create_vault(
            dir.path(),
            &VaultCreateOptions {
                base_commodity: None,
                default_funding: None,
                default_unclassified: None,
                template: Some(true),
            },
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-10-25.ledger"),
            "2026-10-24 * 旧交易\n    expenses:food  1.00 CNY\n    assets:cash\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-10.ledger"),
            "include 2026-10-25.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("rules.ledger"),
            "= desc:星巴克  ; rule:r-star order:10 enabled:true from:assets:wallet\n    expenses:food:coffee    70.00%\n    expenses:food:beans     30.00%\n",
        )
        .unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        (dir, vault)
    }

    #[test]
    fn parses_amount_tokens() {
        let (a, p, s) = parse_amount_token("4.5").unwrap();
        assert_eq!(a.quantity, Decimal::new(45, 1));
        assert!(p.is_none());
        assert_eq!(s, 0);
        let (a, _, s) = parse_amount_token("+12000").unwrap();
        assert_eq!(a.quantity, Decimal::new(12000, 0));
        assert_eq!(s, 1);
        let (a, p, _) = parse_amount_token("100usd@@720cny").unwrap();
        assert_eq!(a.commodity, "USD");
        let (kind, price) = p.unwrap();
        assert_eq!(kind, PriceKind::Total);
        assert_eq!(price.quantity, Decimal::new(720, 0));
        assert!(parse_amount_token("iPhone15").is_none());
        assert!(parse_amount_token("咖啡").is_none());
    }

    #[test]
    fn tokenizes_quotes_and_comments() {
        let (tokens, comment) = tokenize(r#"咖啡 4.5 @微信 +餐饮 ; 备注"#);
        assert_eq!(tokens, vec!["咖啡", "4.5", "@微信", "+餐饮"]);
        assert_eq!(comment.as_deref(), Some("备注"));
        let (tokens, _) = tokenize(r#""iPhone 15" 8999"#);
        assert_eq!(tokens[0], "iPhone 15");
    }

    #[test]
    fn relative_dates_work() {
        let today = Local::now().date_naive();
        assert_eq!(parse_date_directive("-1d"), Some(today - Duration::days(1)));
        assert_eq!(
            parse_date_directive("昨天"),
            Some(today - Duration::days(1))
        );
        assert_eq!(
            parse_date_directive("2026-01-02"),
            Some(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap())
        );
    }

    #[test]
    fn expands_simple_expense_with_rule() {
        let (_dir, vault) = test_vault();
        let parsed = parse_capture("星巴克 45 @微信");
        assert!(parsed.accounts.len() == 1);
        let expanded = expand(&vault, &parsed);
        assert_eq!(expanded.rule_id.as_deref(), Some("r-star"));
        assert_eq!(expanded.txn.postings.len(), 3);
        let accounts: Vec<&str> = expanded
            .txn
            .postings
            .iter()
            .map(|p| p.account.as_str())
            .collect();
        assert_eq!(
            accounts,
            vec!["expenses:food:coffee", "expenses:food:beans", "assets:微信"]
        );
        let sum: Decimal = expanded
            .txn
            .postings
            .iter()
            .filter_map(|p| p.effective_amount().map(|a| a.quantity))
            .sum();
        assert_eq!(sum, Decimal::ZERO);
        assert!(expanded.canonical.contains("rule:r-star"));
    }

    #[test]
    fn falls_back_to_unclassified_with_inbox_tag() {
        let (_dir, vault) = test_vault();
        let expanded = expand(&vault, &parse_capture("神秘支出 12"));
        assert!(expanded
            .txn
            .postings
            .iter()
            .any(|p| p.account == "expenses:unclassified"));
        assert!(expanded.txn.tags.iter().any(|t| t.key == "inbox"));
        assert!(expanded.canonical.contains("rule:none"));
    }

    #[test]
    fn income_sign_flips() {
        let (_dir, vault) = test_vault();
        let expanded = expand(&vault, &parse_capture("工资 +12000 @招行"));
        let p = &expanded.txn.postings;
        let funding = p.iter().find(|p| p.account == "assets:招行").unwrap();
        assert_eq!(
            funding.amount.as_ref().unwrap().quantity,
            Decimal::new(12000, 0)
        );
        let cat = p.iter().find(|p| p.account != "招行").unwrap();
        assert_eq!(cat.account, "expenses:unclassified");
    }

    #[test]
    fn transfer_uses_two_asset_accounts() {
        let (_dir, vault) = test_vault();
        let expanded = expand(&vault, &parse_capture("转 500 > @招行 > @现金"));
        let accounts: Vec<&str> = expanded
            .txn
            .postings
            .iter()
            .map(|p| p.account.as_str())
            .collect();
        assert_eq!(accounts, vec!["assets:招行", "assets:现金"]);
    }

    #[test]
    fn inline_price_is_preserved() {
        let (_dir, vault) = test_vault();
        let expanded = expand(&vault, &parse_capture("酒店 100usd@@720cny @旅行"));
        let first = &expanded.txn.postings[0];
        assert!(first.price.is_some());
        assert_eq!(first.account, "expenses:旅行");
        let funding = &expanded.txn.postings[1];
        assert_eq!(funding.amount.as_ref().unwrap().commodity, "CNY");
    }

    #[test]
    fn rule_lines_are_analysed_without_a_vault_write() {
        let (_dir, vault) = test_vault();
        let a = analyze(&vault, "= desc:咖啡 -> 餐饮=70%, 咖啡豆=30% from 支付宝");
        assert_eq!(a.parsed.kind, Kind::Rule);
        assert!(a.errors.is_empty());
        assert_eq!(a.rule_preview.as_ref().unwrap().template.len(), 2);
        assert!(a.canonical.starts_with("= desc:咖啡"));
    }

    #[test]
    fn txn_ids_are_unique_and_version_7() {
        let a = new_txn_id();
        let b = new_txn_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "7");
    }
}

//! Line oriented recursive-descent parser for the supported hledger subset.
//!
//! The parser never panics on bad input: every problem becomes a [`Diagnostic`]
//! carrying a stable code plus byte span (rendered as line/column). Unsupported
//! hledger features are reported explicitly instead of being silently ignored.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::amount::{parse_amount, parse_percent, AmountError};
use crate::ast::*;
use crate::diagnostic::Diagnostic;
use crate::span::Span;

pub struct ParseOutput {
    pub journal: Journal,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.is_error())
    }
}

pub fn parse_journal(path: &str, src: &str) -> ParseOutput {
    let mut p = Parser::new(path, src);
    p.run();
    ParseOutput {
        journal: p.journal,
        diagnostics: p.diags,
    }
}

/// True when the string starts with a date token (`2026-10-25`, `2026/10/25`).
pub fn starts_with_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 8
        && b[..4].iter().all(u8::is_ascii_digit)
        && matches!(b[4], b'-' | b'/' | b'.')
        && b[5].is_ascii_digit()
}

/// Parse `YYYY-MM-DD`, `YYYY/MM/DD` or `YYYY.MM.DD` (also accepts `YYYY-M-D`).
pub fn parse_date_str(s: &str) -> Option<NaiveDate> {
    let normalized = s.replace(['/', '.'], "-");
    let mut parts = normalized.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    NaiveDate::from_ymd_opt(y, m, d)
}

/// Split trailing `;`-comment from code, ignoring semicolons inside quotes.
pub fn split_comment(s: &str) -> (&str, Option<&str>) {
    let mut in_quotes = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                return (&s[..i], Some(s[i + 1..].trim()));
            }
            _ => {}
        }
    }
    (s, None)
}

/// Parse hledger-style tags out of a comment body.
pub fn parse_tags(comment: &str) -> Vec<Tag> {
    let mut out = Vec::new();
    for token in comment.split_whitespace() {
        if token.len() >= 3 && token.starts_with(':') && token.ends_with(':') {
            let inner = &token[1..token.len() - 1];
            if inner.is_empty() {
                continue;
            }
            let ok = inner
                .split(':')
                .all(|part| !part.is_empty() && is_tag_key(part));
            if ok {
                for part in inner.split(':') {
                    out.push(Tag::bare(part));
                }
                continue;
            }
        }
        if let Some((key, value)) = token.split_once(':') {
            if !key.is_empty() && !value.is_empty() && is_tag_key(key) {
                out.push(Tag::kv(key, value));
            }
        }
    }
    out
}

fn is_tag_key(k: &str) -> bool {
    k.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

struct Line<'a> {
    start: usize,
    text: &'a str,
}

struct Parser<'a> {
    path: &'a str,
    src: &'a str,
    lines: Vec<Line<'a>>,
    idx: usize,
    diags: Vec<Diagnostic>,
    journal: Journal,
}

impl<'a> Parser<'a> {
    fn new(path: &'a str, src: &'a str) -> Self {
        let mut lines = Vec::new();
        let mut cursor = 0usize;
        for raw in src.split('\n') {
            let start = cursor;
            cursor += raw.len() + 1;
            lines.push(Line {
                start,
                text: raw.strip_suffix('\r').unwrap_or(raw),
            });
        }
        Self {
            path,
            src,
            lines,
            idx: 0,
            diags: Vec::new(),
            journal: Journal::default(),
        }
    }

    fn run(&mut self) {
        while self.idx < self.lines.len() {
            let line = &self.lines[self.idx];
            let text = line.text;
            if text.trim().is_empty() {
                self.idx += 1;
                continue;
            }
            if text.starts_with(' ') || text.starts_with('\t') {
                let span = Span::new(line.start, line.start + text.len());
                self.err(
                    "core.parse.orphan_posting",
                    "indented line without a transaction or rule header (stray posting)",
                    span,
                );
                self.idx += 1;
                continue;
            }
            self.parse_top_level_line();
        }
    }

    fn parse_top_level_line(&mut self) {
        let line = &self.lines[self.idx];
        let start = line.start;
        let text = line.text.trim_end().to_string();
        let end = start + text.len();
        let span = Span::new(start, end);

        // Comment-only lines and comment blocks.
        if let Some(kind) = comment_prefix(&text) {
            if kind == CommentPrefix::Line {
                self.take_comment_block();
            } else {
                self.take_bracketed_comment(&text, span);
            }
            return;
        }

        if starts_with_date(&text) {
            self.parse_transaction();
            return;
        }

        let word = text.split_whitespace().next().unwrap_or("").to_string();
        let lower = word.to_ascii_lowercase();

        match lower.as_str() {
            "include" => {
                let rest = &text[word.len()..];
                let (code, comment) = split_comment(rest);
                let path_raw = code.trim();
                let path_str = unquote(path_raw);
                if path_str.is_empty() {
                    self.err("core.parse.empty_include", "include needs a path", span);
                } else {
                    self.journal.items.push(Item::Include(Include {
                        span,
                        path: path_str.to_string(),
                        optional: false,
                        // keep comment out of AST; includes never carry tags
                    }));
                    let _ = comment;
                }
            }
            "!include" => {
                let rest = &text[word.len()..];
                let path_str = unquote(split_comment(rest).0.trim());
                if path_str.is_empty() {
                    self.err("core.parse.empty_include", "!include needs a path", span);
                } else {
                    self.journal.items.push(Item::Include(Include {
                        span,
                        path: path_str.to_string(),
                        optional: true,
                    }));
                }
            }
            "account" => {
                let rest = &text[word.len()..];
                let (code, comment) = split_comment(rest);
                let name = code.trim().to_string();
                if name.is_empty() {
                    self.err(
                        "core.parse.empty_account",
                        "account declaration needs a name",
                        span,
                    );
                } else {
                    let tags = comment.map(parse_tags).unwrap_or_default();
                    self.journal.items.push(Item::Account(AccountDecl {
                        span,
                        name,
                        comment: comment.map(|s| s.trim().to_string()),
                        tags,
                    }));
                }
            }
            "commodity" => {
                let rest = split_comment(&text[word.len()..]);
                let body = rest.0.trim();
                if body.is_empty() {
                    self.err(
                        "core.parse.empty_commodity",
                        "commodity declaration needs a symbol or sample amount",
                        span,
                    );
                } else {
                    let sample = parse_amount(body).ok();
                    let name = sample
                        .as_ref()
                        .map(|a| a.commodity.clone())
                        .filter(|c| !c.is_empty())
                        .unwrap_or_else(|| body.to_string());
                    self.journal.items.push(Item::Commodity(CommodityDecl {
                        span,
                        name,
                        sample,
                        comment: rest.1.map(|s| s.trim().to_string()),
                    }));
                }
            }
            "d" => {
                let rest = split_comment(&text[word.len()..]);
                match parse_amount(rest.0.trim()) {
                    Ok(amount) if !amount.commodity.is_empty() => {
                        self.journal
                            .items
                            .push(Item::DefaultCommodity(CommodityDecl {
                                span,
                                name: amount.commodity.clone(),
                                sample: Some(amount),
                                comment: rest.1.map(|s| s.trim().to_string()),
                            }));
                    }
                    _ => self.err(
                        "core.parse.bad_default_commodity",
                        "D directive expects an amount like `D 1,000.00 CNY`",
                        span,
                    ),
                }
            }
            "y" => {
                let rest = split_comment(&text[word.len()..]).0.trim();
                match rest.parse::<i32>() {
                    Ok(year) => self.journal.items.push(Item::Year(YearDecl { span, year })),
                    Err(_) => self.err(
                        "core.parse.bad_year",
                        "Y directive expects a year number",
                        span,
                    ),
                }
            }
            "p" => self.parse_price(&text, span),
            // These two advance past their posting block themselves.
            "=" => {
                self.parse_auto_rule(&text, span);
                return;
            }
            "~" => {
                self.parse_periodic(&text, span);
                return;
            }
            "c" | "n" => self.err(
                "core.parse.unsupported_conversion",
                format!("`{word}` conversion directives are outside the supported subset"),
                span,
            ),
            _ => {
                if lower.starts_with("csv:") || lower == "csv" {
                    self.err(
                        "core.parse.unsupported_csv_rule",
                        "`csv:` rules are outside the supported subset",
                        span,
                    );
                } else if text.starts_with('(') || text.starts_with('[') {
                    self.err(
                        "core.parse.unsupported_directive",
                        "unsupported directive",
                        span,
                    );
                } else {
                    self.err(
                        "core.parse.unknown_directive",
                        format!(
                            "cannot parse line starting with `{}`",
                            first_words(&text, 3)
                        ),
                        span,
                    );
                }
            }
        }
        self.idx += 1;
    }

    fn parse_price(&mut self, text: &str, span: Span) {
        let body = split_comment(text).0.trim();
        let body = body[1..].trim_start();
        let date_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let date_str = &body[..date_end];
        let rest = body[date_end..].trim_start();
        let commodity_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let commodity = &rest[..commodity_end];
        let amount_text = rest[commodity_end..].trim();
        let date = parse_date_str(date_str);
        let amount = parse_amount(amount_text);
        match (date, amount) {
            (Some(date), Ok(amount)) if !commodity.is_empty() && !amount.commodity.is_empty() => {
                self.journal.items.push(Item::Price(PriceDecl {
                    span,
                    date,
                    commodity: commodity.to_string(),
                    amount,
                }));
            }
            (None, _) => self.err(
                "core.parse.bad_price_date",
                "P directive needs a valid date",
                span,
            ),
            _ => self.err(
                "core.parse.bad_price",
                "P directive expects `P DATE COMMODITY AMOUNT` (e.g. `P 2026-10-25 USD 7.12 CNY`)",
                span,
            ),
        }
    }

    fn parse_auto_rule(&mut self, text: &str, span: Span) {
        let rest = &text[1..];
        let (code, comment) = split_comment(rest);
        let query = code.trim().to_string();
        let tags = comment.map(parse_tags).unwrap_or_default();
        self.idx += 1;
        let postings = self.collect_postings();
        let end = postings
            .last()
            .map(|p| p.span.end)
            .unwrap_or(span.end)
            .max(span.end);
        self.journal.items.push(Item::AutoRule(AutoRule {
            span: Span::new(span.start, end),
            query,
            comment: comment.map(|s| s.trim().to_string()),
            tags,
            postings,
        }));
    }

    fn parse_periodic(&mut self, text: &str, span: Span) {
        let rest = &text[1..];
        let (code, comment) = split_comment(rest);
        let period = code.trim().to_string();
        self.idx += 1;
        let postings = self.collect_postings();
        let end = postings
            .last()
            .map(|p| p.span.end)
            .unwrap_or(span.end)
            .max(span.end);
        self.journal.items.push(Item::Periodic(PeriodicTransaction {
            span: Span::new(span.start, end),
            period,
            comment: comment.map(|s| s.trim().to_string()),
            postings,
        }));
    }

    fn parse_transaction(&mut self) {
        let line = &self.lines[self.idx];
        let start = line.start;
        let text = line.text.trim_end().to_string();
        let header_end = start + text.len();
        let span = Span::new(start, header_end);

        let (date, date2, status, code, payee, comment) = match self.parse_txn_header(&text, span) {
            Some(v) => v,
            None => {
                self.idx += 1;
                return;
            }
        };
        let tags = comment.as_deref().map(parse_tags).unwrap_or_default();
        self.idx += 1;
        let postings = self.collect_postings();
        let end = postings
            .last()
            .map(|p| p.span.end)
            .unwrap_or(header_end)
            .max(header_end);

        self.journal.items.push(Item::Transaction(Transaction {
            span: Span::new(start, end),
            date,
            date2,
            status,
            code,
            payee,
            comment,
            tags,
            postings,
        }));
    }

    #[allow(clippy::type_complexity)]
    fn parse_txn_header(
        &mut self,
        text: &str,
        span: Span,
    ) -> Option<(
        NaiveDate,
        Option<NaiveDate>,
        Option<Status>,
        Option<String>,
        String,
        Option<String>,
    )> {
        let (code_part, comment) = split_comment(text);
        let mut s = code_part;
        let date_len = s
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '-' | '/' | '.'))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        let date = match parse_date_str(&s[..date_len]) {
            Some(d) => d,
            None => {
                self.err(
                    "core.parse.bad_date",
                    format!("invalid transaction date `{}`", &s[..date_len.min(s.len())]),
                    span,
                );
                return None;
            }
        };
        s = &s[date_len..];

        let mut date2 = None;
        let trimmed = s.trim_start();
        if let Some(after_eq) = trimmed.strip_prefix('=') {
            let after = after_eq.trim_start();
            let len = after
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '-' | '/' | '.'))
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            match parse_date_str(&after[..len]) {
                Some(d) => {
                    date2 = Some(d);
                    s = &after[len..];
                }
                None => {
                    self.err(
                        "core.parse.bad_secondary_date",
                        "invalid secondary date",
                        span,
                    );
                    return None;
                }
            }
        } else {
            s = trimmed;
        }

        let mut rest = s.trim_start();
        let mut status = None;
        if let Some(c) = rest.chars().next() {
            if let Some(st) = Status::from_marker(c) {
                let after_marker = &rest[c.len_utf8()..];
                if after_marker.is_empty() || after_marker.starts_with(char::is_whitespace) {
                    status = Some(st);
                    rest = after_marker.trim_start();
                }
            }
        }

        let mut code = None;
        if let Some(after) = rest.strip_prefix('(') {
            if let Some(close) = after.find(')') {
                code = Some(after[..close].to_string());
                rest = after[close + 1..].trim_start();
            }
        }

        let payee = unquote(rest.trim()).to_string();
        Some((
            date,
            date2,
            status,
            code,
            payee,
            comment.map(|c| c.trim().to_string()),
        ))
    }

    /// Consume indented posting lines until a blank or non-indented line.
    fn collect_postings(&mut self) -> Vec<Posting> {
        let mut out = Vec::new();
        while self.idx < self.lines.len() {
            let line = &self.lines[self.idx];
            let text = line.text;
            if text.trim().is_empty() || !(text.starts_with(' ') || text.starts_with('\t')) {
                break;
            }
            let start = line.start;
            let body = text.trim_end().to_string();
            let span = Span::new(start, start + body.len());
            let lead = body.len() - body.trim_start().len();
            if let Some(p) = self.parse_posting(body.trim_start(), span, start + lead) {
                out.push(p);
            }
            self.idx += 1;
        }
        out
    }

    fn parse_posting(&mut self, text: &str, span: Span, text_offset: usize) -> Option<Posting> {
        let mut rest = text;
        let mut status = None;
        if let Some(c) = rest.chars().next() {
            if let Some(st) = Status::from_marker(c) {
                let after = &rest[c.len_utf8()..];
                if after.is_empty() || after.starts_with(char::is_whitespace) {
                    status = Some(st);
                    rest = after.trim_start();
                }
            }
        }

        let (code, comment) = split_comment(rest);
        let code = code.trim_end();

        // Account ends at a tab or at two or more spaces.
        let sep = find_account_separator(code);
        let (account_raw, amount_part) = match sep {
            Some(i) => (&code[..i], code[i..].trim_start()),
            None => (code, ""),
        };
        let account_raw = account_raw.trim_end();
        let mut account = account_raw.to_string();
        let mut virtual_kind = VirtualKind::Real;
        if account.len() >= 2 && account.starts_with('(') && account.ends_with(')') {
            virtual_kind = VirtualKind::Virtual;
            account = account[1..account.len() - 1].trim().to_string();
        } else if account.len() >= 2 && account.starts_with('[') && account.ends_with(']') {
            virtual_kind = VirtualKind::BalancedVirtual;
            account = account[1..account.len() - 1].trim().to_string();
        }
        if account.is_empty() {
            self.err(
                "core.parse.empty_posting_account",
                "posting is missing an account name",
                span,
            );
            return None;
        }
        let account = normalize_account(&account);

        if amount_part.contains('{') || amount_part.contains("{{") {
            self.err(
                "core.parse.unsupported_lot_price",
                "lot/cost prices in braces are outside the supported subset",
                span,
            );
            return None;
        }

        let (amount_text, assertion_text) = split_balance_assertion(amount_part);
        let (amount_text, price_text, price_kind) = split_price(amount_text);

        let mut percent = None;
        let amount = if amount_text.trim().is_empty() {
            None
        } else if amount_text.trim().ends_with('%') {
            match parse_percent(amount_text) {
                Ok(p) => {
                    percent = Some(p);
                    None
                }
                Err(e) => {
                    let aspan = Self::sub_span(text, amount_text, text_offset);
                    self.amount_error(e, amount_text.trim(), aspan);
                    return None;
                }
            }
        } else {
            match parse_amount(amount_text) {
                Ok(a) => Some(a),
                Err(e) => {
                    let aspan = Self::sub_span(text, amount_text, text_offset);
                    self.amount_error(e, amount_text.trim(), aspan);
                    return None;
                }
            }
        };

        let price = match price_text {
            Some(p) if !p.trim().is_empty() => match parse_amount(p) {
                Ok(a) => Some(Price {
                    kind: price_kind.unwrap_or(PriceKind::Unit),
                    amount: a,
                    span,
                }),
                Err(e) => {
                    let pspan = Self::sub_span(text, p, text_offset);
                    self.amount_error(e, p.trim(), pspan);
                    return None;
                }
            },
            _ => None,
        };

        let balance_assertion = match assertion_text {
            Some(a) if !a.trim().is_empty() => match parse_amount(a) {
                Ok(a) => Some(a),
                Err(e) => {
                    let aspan = Self::sub_span(text, a, text_offset);
                    self.amount_error(e, a.trim(), aspan);
                    return None;
                }
            },
            _ => None,
        };

        Some(Posting {
            span,
            status,
            account,
            virtual_kind,
            amount,
            percent,
            price,
            balance_assertion,
            comment: comment.map(|c| c.trim().to_string()),
            tags: comment.map(parse_tags).unwrap_or_default(),
        })
    }

    /// Span of `needle` inside the posting line that starts at `text_offset`.
    fn sub_span(text: &str, needle: &str, text_offset: usize) -> Span {
        let needle = needle.trim();
        let rel = text.find(needle).unwrap_or(0);
        Span::new(text_offset + rel, text_offset + rel + needle.len())
    }

    fn amount_error(&mut self, e: AmountError, text: &str, span: Span) {
        let code = match e {
            AmountError::NoDigits(_) => "core.parse.amount_no_digits",
            AmountError::TwoCommodities(_) => "core.parse.amount_two_commodities",
            AmountError::BadChar(_, '+' | '-' | '*' | '/' | '(' | ')') => {
                "core.parse.unsupported_amount_expression"
            }
            _ => "core.parse.bad_amount",
        };
        self.err(code, format!("cannot parse amount `{text}`: {e}"), span);
    }

    fn take_comment_block(&mut self) {
        let start = self.lines[self.idx].start;
        let mut lines = Vec::new();
        let mut end = start;
        while self.idx < self.lines.len() {
            let line = &self.lines[self.idx];
            if comment_prefix(line.text.trim_end()) == Some(CommentPrefix::Line) {
                lines.push(line.text.trim_end().to_string());
                end = line.start + line.text.trim_end().len();
                self.idx += 1;
            } else {
                break;
            }
        }
        self.journal.items.push(Item::Comment(CommentBlock {
            span: Span::new(start, end),
            lines,
        }));
    }

    fn take_bracketed_comment(&mut self, text: &str, span: Span) {
        let mut lines = vec![text.to_string()];
        let mut end = span.end;
        self.idx += 1;
        while self.idx < self.lines.len() {
            let line = &self.lines[self.idx];
            let t = line.text.trim_end();
            lines.push(t.to_string());
            end = line.start + t.len();
            self.idx += 1;
            if t.trim() == "end comment" {
                break;
            }
        }
        self.journal.items.push(Item::Comment(CommentBlock {
            span: Span::new(span.start, end),
            lines,
        }));
    }

    fn err(&mut self, code: &str, message: impl Into<String>, span: Span) {
        self.diags
            .push(Diagnostic::error(code, message, self.path, span).with_position(self.src));
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum CommentPrefix {
    Line,
    Block,
}

fn comment_prefix(text: &str) -> Option<CommentPrefix> {
    let t = text.trim_start();
    if t.is_empty() {
        return None;
    }
    let first = t.chars().next()?;
    if matches!(first, ';' | '#' | '%' | '|' | '*') {
        return Some(CommentPrefix::Line);
    }
    if t == "comment" || t.starts_with("comment ") {
        return Some(CommentPrefix::Block);
    }
    None
}

fn first_words(s: &str, n: usize) -> String {
    s.split_whitespace().take(n).collect::<Vec<_>>().join(" ")
}

fn unquote(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

pub fn normalize_account(a: &str) -> String {
    a.split(':')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

/// Byte index in `code` where the amount column starts (tab or 2+ spaces).
fn find_account_separator(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\t' {
            return Some(i);
        }
        if bytes[i] == b' ' {
            let mut j = i;
            while j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            if j - i >= 2 {
                return Some(i);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    None
}

/// Split `4.5 CNY = 100 CNY` into (`4.5 CNY`, Some(`100 CNY`)).
fn split_balance_assertion(s: &str) -> (&str, Option<&str>) {
    let mut in_quotes = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '=' if !in_quotes => {
                let prev_space = i == 0 || bytes[i - 1] == b' ';
                let next_eq = bytes.get(i + 1) == Some(&b'=');
                let prev_eq = i > 0 && bytes[i - 1] == b'=';
                if !next_eq && !prev_eq && prev_space {
                    return (s[..i].trim_end(), Some(s[i + 1..].trim()));
                }
            }
            _ => {}
        }
    }
    (s, None)
}

/// Split `4.5 CNY @@ 30 CNY` into (`4.5 CNY`, Some(`30 CNY`), Some(Total)).
fn split_price(s: &str) -> (&str, Option<&str>, Option<PriceKind>) {
    let mut in_quotes = false;
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '@' if !in_quotes => {
                let total = bytes.get(i + 1) == Some(&b'@');
                let skip = if total { 2 } else { 1 };
                return (
                    s[..i].trim_end(),
                    Some(s[i + skip..].trim()),
                    Some(if total {
                        PriceKind::Total
                    } else {
                        PriceKind::Unit
                    }),
                );
            }
            _ => {}
        }
    }
    (s, None, None)
}

/// Format helper used by tests: parse a single transaction block.
pub fn parse_single_transaction(path: &str, src: &str) -> (Option<Transaction>, Vec<Diagnostic>) {
    let out = parse_journal(path, src);
    let mut txns: Vec<Transaction> = out
        .journal
        .items
        .into_iter()
        .filter_map(|i| match i {
            Item::Transaction(t) => Some(t),
            _ => None,
        })
        .collect();
    if txns.is_empty() {
        (None, out.diagnostics)
    } else {
        (Some(txns.remove(0)), out.diagnostics)
    }
}

/// Parse a decimal with an optional leading `+`/`-` (used by capture amounts).
pub fn parse_decimal(s: &str) -> Option<Decimal> {
    Decimal::from_str(s.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> ParseOutput {
        parse_journal("test.ledger", src)
    }

    #[test]
    fn parses_simple_transaction() {
        let src = "2026-10-25 * 星巴克  ; txn:01JA3KQ7 rule:r-9f21c4 query:desc:星巴克\n    expenses:food:coffee      4.50 CNY\n    assets:wallet:alipay     -4.50 CNY\n";
        let out = parse(src);
        assert!(!out.has_errors(), "{:?}", out.diagnostics);
        let txn = out.journal.transactions().next().unwrap();
        assert_eq!(txn.payee, "星巴克");
        assert_eq!(txn.status, Some(Status::Cleared));
        assert_eq!(txn.date, parse_date_str("2026-10-25").unwrap());
        assert_eq!(txn.postings.len(), 2);
        assert_eq!(txn.postings[0].account, "expenses:food:coffee");
        assert_eq!(
            txn.postings[0].amount.as_ref().unwrap().format(),
            "4.50 CNY"
        );
        assert_eq!(txn.tag("txn"), Some("01JA3KQ7"));
        assert_eq!(txn.tag("rule"), Some("r-9f21c4"));
        assert_eq!(txn.tag("query"), Some("desc:星巴克"));
        let p0 = txn.postings[0].span;
        assert!(src[p0.start..p0.end].contains("expenses:food:coffee"));
        assert!(src[p0.start..p0.end]
            .trim_start()
            .starts_with("expenses:food:coffee"));
        assert!(txn.span.slice(src).starts_with("2026-10-25 * 星巴克"));
        assert!(txn.span.slice(src).ends_with("-4.50 CNY"));
    }

    #[test]
    fn parses_directives() {
        let src = "commodity 1,000.00 CNY\naccount assets:cash  ; type:A\ninclude 2026/2026.ledger\n!include optional.ledger\nP 2026-10-25 USD 7.12 CNY\nY 2026\n";
        let out = parse(src);
        assert!(!out.has_errors(), "{:?}", out.diagnostics);
        let kinds: Vec<&str> = out
            .journal
            .items
            .iter()
            .map(|i| match i {
                Item::Commodity(_) => "commodity",
                Item::Account(_) => "account",
                Item::Include(_) => "include",
                Item::Price(_) => "price",
                Item::Year(_) => "year",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "commodity",
                "account",
                "include",
                "include",
                "price",
                "year"
            ]
        );
        let account = out.journal.account_decls().next().unwrap();
        assert_eq!(account.name, "assets:cash");
        assert_eq!(account.tags[0], Tag::kv("type", "A"));
        let includes: Vec<_> = out.journal.includes().collect();
        assert_eq!(includes[0].path, "2026/2026.ledger");
        assert!(!includes[0].optional);
        assert!(includes[1].optional);
    }

    #[test]
    fn parses_elided_amount_and_virtual_postings() {
        let src = "2026-01-02 ! (42) \"quoted payee\"\n    expenses:a    10 CNY\n    (assets:v)    3 CNY\n    [assets:b]    1 CNY\n    assets:c\n";
        let out = parse(src);
        assert!(!out.has_errors(), "{:?}", out.diagnostics);
        let txn = out.journal.transactions().next().unwrap();
        assert_eq!(txn.code.as_deref(), Some("42"));
        assert_eq!(txn.payee, "quoted payee");
        assert_eq!(txn.status, Some(Status::Pending));
        assert_eq!(txn.postings[3].amount, None);
        assert_eq!(txn.postings[1].virtual_kind, VirtualKind::Virtual);
        assert_eq!(txn.postings[2].virtual_kind, VirtualKind::BalancedVirtual);
    }

    #[test]
    fn parses_price_and_assertion() {
        let src =
            "2026-01-01 x\n    expenses:travel   100 USD @@ 720 CNY = 720 CNY\n    assets:cash\n";
        let out = parse(src);
        let txn = out.journal.transactions().next().unwrap();
        let p = &txn.postings[0];
        assert_eq!(p.price.as_ref().unwrap().kind, PriceKind::Total);
        assert_eq!(p.balance_assertion.as_ref().unwrap().format(), "720 CNY");
        assert_eq!(p.effective_amount().unwrap().format(), "720 CNY");
    }

    #[test]
    fn parses_auto_rule_block() {
        let src = "= desc:星巴克 and amt:>0  ; rule:r-1 order:10 enabled:true\n    expenses:food:coffee   70.00%\n    expenses:food:beans    30.00%\n";
        let out = parse(src);
        assert!(!out.has_errors(), "{:?}", out.diagnostics);
        let rule = out.journal.auto_rules().next().unwrap();
        assert_eq!(rule.query, "desc:星巴克 and amt:>0");
        assert_eq!(rule.postings.len(), 2);
        assert!(rule
            .tags
            .iter()
            .any(|t| t.key == "rule" && t.value.as_deref() == Some("r-1")));
    }

    #[test]
    fn reports_unsupported_features() {
        let src = "2026-01-01 x\n    a:b   1 CNY {2 USD}\n    c:d\nC 1 USD = 7 CNY\n";
        let out = parse(src);
        let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(
            codes.contains(&"core.parse.unsupported_lot_price"),
            "{codes:?}"
        );
        assert!(
            codes.contains(&"core.parse.unsupported_conversion"),
            "{codes:?}"
        );
    }

    #[test]
    fn reports_positions() {
        let src = "2026-01-01 good\n    a:b   1 CNY\n    c:d  oops\n";
        let out = parse(src);
        let d = out.diagnostics.iter().find(|d| d.is_error()).unwrap();
        assert_eq!(d.line, 3);
        assert_eq!(d.col, 10);
        assert!(
            d.display().starts_with("test.ledger:3:10: error["),
            "{}",
            d.display()
        );
        assert!(d.message.contains("oops"), "{}", d.message);
    }

    #[test]
    fn tags_roundtrip_from_comment() {
        let tags = parse_tags("txn:abc rule:r-1 :coffee:travel: note:hello-world");
        assert_eq!(tags[0], Tag::kv("txn", "abc"));
        assert_eq!(tags[2], Tag::bare("coffee"));
        assert_eq!(tags[3], Tag::bare("travel"));
        assert_eq!(tags[4], Tag::kv("note", "hello-world"));
    }

    #[test]
    fn comment_blocks() {
        let src = "; hello\n; world\n\ncomment\nmulti\nend comment\n2026-01-01 x\n    a:b  1 CNY\n    c:d\n";
        let out = parse(src);
        assert!(!out.has_errors(), "{:?}", out.diagnostics);
        assert_eq!(out.journal.items.len(), 3);
    }
}

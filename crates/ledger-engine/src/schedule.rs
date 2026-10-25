//! Recurring schedules: *declared* monthly money, in `recurring.ledger`.
//!
//! [`crate::recur`] guesses patterns from the transactions that already exist.
//! This module is the opposite: the user states once what happens every month —
//! salary with its 五险一金 split, the mortgage payment, subscriptions — and the
//! app can then show it as a calendar instead of a surprise.
//!
//! The file is plain hledger periodic syntax (`~ monthly ...` plus postings), so
//! the numbers are hand-editable, but it is **app metadata**: like
//! `rules.ledger` and `goals.ledger` it is not included by `index.ledger`, so
//! hledger never double counts a template and budgets never mistake one for an
//! envelope.
//!
//! Nothing is booked automatically. [`commit`] writes one occurrence when the
//! user asks; the settings switch only controls whether the app *suggests* it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, NaiveDate};
use ledger_core::{
    parse_journal, Diagnostic, Item, Posting, Severity, Span, Status, Tag, Transaction,
};
use rust_decimal::Decimal;

use crate::amounts::{to_dto_prec, CommodityTable};
use crate::load::{diagnostic_dto, Vault};
use crate::rules::Direction;
use crate::types::{
    AmountDto, SalaryTemplateDto, ScheduleCommitDto, ScheduleLegDto, ScheduleMonthDto,
    ScheduleOccurrenceDto, ScheduleOverviewDto, ScheduleTemplateDto,
};
use crate::vault::VaultOptions;
use crate::{EngineError, Result};
use ledger_core::Amount;

/// `recurring.ledger` is included by `index.ledger`, so hledger sees the
/// templates (`--forecast`); the budget and forecast reports skip it explicitly.
pub const SCHEDULE_FILE: &str = "recurring.ledger";

/// True for the declared-schedule file of a vault.
///
/// The schedule is part of the ledger (`index.ledger` includes it, so
/// `hledger --forecast` works), but its `~` legs are plans, not envelopes: the
/// budget and forecast reports have to ignore the file or a declared salary
/// would look like a budget.
pub fn is_schedule_file(rel: &str) -> bool {
    rel == SCHEDULE_FILE
}
/// Comment tag written on booked occurrences: `; sched:salary@2026-11`.
pub const TAG_KEY: &str = "sched";
/// `recur::skip_list` keys are namespaced so detection and schedules never clash.
const SKIP_PREFIX: &str = "sched:";
/// Never render more than three years of calendar.
const MAX_MONTHS: usize = 36;
/// Guard against a `daily` template projected over a decade.
const MAX_DATES: usize = 4000;

// ---------------------------------------------------------------------------
// cadence
// ---------------------------------------------------------------------------

/// How often a template repeats. Mirrors the hledger period words we accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl Cadence {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_lowercase().as_str() {
            "daily" | "day" | "days" | "d" => Some(Self::Daily),
            "weekly" | "week" | "weeks" | "w" => Some(Self::Weekly),
            "monthly" | "month" | "months" | "m" => Some(Self::Monthly),
            "quarterly" | "quarter" | "quarters" | "q" => Some(Self::Quarterly),
            "yearly" | "annually" | "annual" | "year" | "years" | "y" => Some(Self::Yearly),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Yearly => "yearly",
        }
    }

    fn step_months(self) -> i32 {
        match self {
            Self::Monthly => 1,
            Self::Quarterly => 3,
            Self::Yearly => 12,
            _ => 1,
        }
    }

    fn step_days(self) -> i64 {
        match self {
            Self::Daily => 1,
            Self::Weekly => 7,
            _ => 30,
        }
    }
}

// ---------------------------------------------------------------------------
// dates
// ---------------------------------------------------------------------------

pub fn month_key(date: NaiveDate) -> String {
    date.format("%Y-%m").to_string()
}

pub fn month_start(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

pub fn month_end(date: NaiveDate) -> NaiveDate {
    let first = month_start(date);
    first + Duration::days(days_in_month(first.year(), first.month()) as i64 - 1)
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    let next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid month");
    (next - first).num_days() as u32
}

fn clamped(year: i32, month: u32, day: u32) -> NaiveDate {
    let day = day.clamp(1, days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).expect("clamped date")
}

/// Shift by whole months, clamping the day (Jan 31 + 1 month = Feb 28).
pub fn shift_months(date: NaiveDate, months: i32) -> NaiveDate {
    let index = date.year() * 12 + date.month() as i32 - 1 + months;
    let year = index.div_euclid(12);
    let month = index.rem_euclid(12) as u32 + 1;
    clamped(year, month, date.day())
}

/// Accept both `2026-11-05` and the shorter `2026-11`.
fn parse_iso_date(text: &str) -> Option<NaiveDate> {
    let text = text.trim().trim_end_matches([';', ',']);
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(date);
    }
    let short = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok()?;
    Some(month_start(short))
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

/// One `~` block, with the metadata the app reads from its header comment.
#[derive(Debug, Clone)]
pub struct ScheduleTemplate {
    pub id: String,
    pub label: String,
    /// `salary` | `loan` | `income` | `expense` | `other`
    pub kind: String,
    /// The raw hledger period text, shown verbatim in the UI.
    pub period: String,
    pub cadence: Cadence,
    pub day: u32,
    pub start: Option<NaiveDate>,
    pub until: Option<NaiveDate>,
    pub enabled: bool,
    pub postings: Vec<Posting>,
    /// `tax:<account>` — where withheld income tax goes (`expenses:个税`).
    /// Absent means the app books no tax for this template.
    pub tax_account: Option<String>,
    /// `gross:` — 应发工资, without the employer's share.
    pub gross: Option<Decimal>,
    /// `personal:` — 个人代扣的五险一金合计 (the part that lowers the tax base).
    pub personal: Option<Decimal>,
    /// `deduct:` — 专项附加扣除 (房贷利息 1000 / 子女教育 2000 …), per month.
    pub deduct: Decimal,
    pub span: Span,
    pub line: u32,
}

/// What the app needs to estimate income tax for one template.
#[derive(Debug, Clone, Copy)]
pub struct TaxSetup {
    pub gross: Decimal,
    /// 个人代扣 → 税前扣除.
    pub personal: Decimal,
    /// 专项附加扣除 + 其他免税项.
    pub deduct: Decimal,
}

/// A resolved posting: amounts filled in, account type attached.
#[derive(Debug, Clone)]
pub struct Leg {
    pub account: String,
    pub kind: char,
    pub role: String,
    pub amount: Decimal,
}

#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub settle: Decimal,
    pub income: Decimal,
    pub expense: Decimal,
    pub assets: Decimal,
    pub debt: Decimal,
}

/// The parsed contents of `recurring.ledger`.
#[derive(Debug, Clone)]
pub struct ScheduleSet {
    pub src: String,
    pub templates: Vec<ScheduleTemplate>,
    pub diagnostics: Vec<Diagnostic>,
    /// False when the vault has no `recurring.ledger` yet.
    pub exists: bool,
}

impl ScheduleSet {
    pub fn get(&self, id: &str) -> Option<&ScheduleTemplate> {
        self.templates.iter().find(|t| t.id == id)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

fn line_of(src: &str, span: Span) -> u32 {
    src.get(..span.start)
        .map(|head| head.matches('\n').count() as u32 + 1)
        .unwrap_or(0)
}

fn meta_map(comment: Option<&str>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(comment) = comment else { return out };
    for token in comment.split_whitespace() {
        if let Some((key, value)) = token.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            if !key.is_empty() && !value.is_empty() {
                out.insert(key, value.to_string());
            }
        }
    }
    out
}

/// `工资 / 2026` → `salary-2026`; CJK is kept, punctuation is not.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "sched".to_string()
    } else {
        trimmed
    }
}

fn short_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()[..6].to_string()
}

/// Parse `recurring.ledger` source. Pure: no filesystem access, so the tests can
/// hand it strings.
pub fn parse(src: &str) -> ScheduleSet {
    let parsed = parse_journal(SCHEDULE_FILE, src);
    let mut diagnostics = parsed.diagnostics;
    let mut templates = Vec::new();

    for item in &parsed.journal.items {
        let Item::Periodic(pt) = item else { continue };
        let meta = meta_map(pt.comment.as_deref());
        let mut cadence: Option<Cadence> = None;
        let mut day: Option<u32> = None;
        let mut start: Option<NaiveDate> = None;
        let mut until: Option<NaiveDate> = None;

        let tokens: Vec<&str> = pt.period.split_whitespace().collect();
        let mut index = 0;
        while index < tokens.len() {
            let token = tokens[index];
            let lower = token.to_ascii_lowercase();
            if let Some(parsed) = Cadence::parse(token) {
                if cadence.is_none() {
                    cadence = Some(parsed);
                }
            } else if let Some(value) = lower.strip_prefix("day:") {
                day = value.parse().ok();
            } else if let Some(value) = lower.strip_prefix("from:") {
                start = parse_iso_date(value);
            } else if let Some(value) = lower.strip_prefix("until:") {
                until = parse_iso_date(value);
            } else if lower == "from" || lower == "since" {
                if let Some(value) = tokens.get(index + 1) {
                    start = parse_iso_date(value);
                    index += 1;
                }
            } else if lower == "until" || lower == "to" || lower == "through" {
                if let Some(value) = tokens.get(index + 1) {
                    until = parse_iso_date(value);
                    index += 1;
                }
            } else if lower == "day" || lower == "on" {
                if let Some(value) = tokens.get(index + 1) {
                    if let Ok(parsed) = value.trim_end_matches(',').parse::<u32>() {
                        day = Some(parsed);
                        index += 1;
                    }
                }
            }
            index += 1;
        }

        let Some(cadence) = cadence else {
            let mut diagnostic = Diagnostic::error(
                "core.schedule.period",
                format!(
                    "unknown period `{}` — use daily / weekly / monthly / quarterly / yearly",
                    pt.period
                ),
                SCHEDULE_FILE,
                pt.span,
            );
            diagnostic.line = line_of(src, pt.span);
            diagnostics.push(diagnostic);
            continue;
        };

        let label = meta
            .get("label")
            .cloned()
            .or_else(|| meta.get("name").cloned())
            .unwrap_or_else(|| cadence.label().to_string());
        let id = meta
            .get("id")
            .cloned()
            .unwrap_or_else(|| format!("{}-{}", slug(&label), short_hash(&pt.period)));
        let kind = meta
            .get("kind")
            .cloned()
            .unwrap_or_else(|| "other".to_string());
        let enabled = meta
            .get("enabled")
            .map(|v| {
                !matches!(
                    v.to_ascii_lowercase().as_str(),
                    "false" | "no" | "0" | "off"
                )
            })
            .unwrap_or(true);
        if day.is_some() {
            // hledger has no `day:` token, and this file is included by index.ledger,
            // so an app-only token would make `hledger --forecast` fail to parse it.
            let mut diagnostic = Diagnostic::warning(
                "core.schedule.day_token",
                "`day:` 只有 App 认，hledger 不认；把日号写进 from 日期，例如 `monthly from 2026-11-15`"
                    .to_string(),
                SCHEDULE_FILE,
                pt.span,
            );
            diagnostic.line = line_of(src, pt.span);
            diagnostics.push(diagnostic);
        }
        let day = day
            .or_else(|| start.map(|d| d.day()))
            .unwrap_or(1)
            .clamp(1, 31);
        let line = line_of(src, pt.span);

        let tax_account = meta
            .get("tax")
            .map(|value| value.trim().to_string())
            .filter(|value| value.contains(':'));
        templates.push(ScheduleTemplate {
            id,
            label,
            kind,
            period: pt.period.clone(),
            cadence,
            day,
            start,
            until,
            enabled,
            postings: pt.postings.clone(),
            tax_account,
            gross: meta.get("gross").and_then(|value| money(value)),
            personal: meta.get("personal").and_then(|value| money(value)),
            deduct: meta
                .get("deduct")
                .and_then(|value| money(value))
                .unwrap_or(Decimal::ZERO),
            span: pt.span,
            line,
        });
    }

    ScheduleSet {
        src: src.to_string(),
        templates,
        diagnostics,
        exists: true,
    }
}

fn empty_set() -> ScheduleSet {
    ScheduleSet {
        src: String::new(),
        templates: Vec::new(),
        diagnostics: Vec::new(),
        exists: false,
    }
}

pub fn text(root: &Path) -> String {
    std::fs::read_to_string(root.join(SCHEDULE_FILE)).unwrap_or_default()
}

/// Read `recurring.ledger`. A missing file is an empty schedule, not an error.
pub fn load(root: &Path) -> ScheduleSet {
    match std::fs::read_to_string(root.join(SCHEDULE_FILE)) {
        Ok(src) => parse(&src),
        Err(_) => empty_set(),
    }
}

impl ScheduleTemplate {
    /// Occurrence dates in `[from, to]`, clamped to `until` and honouring `day`.
    pub fn dates(&self, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
        let mut out = Vec::new();
        let end = self.until.map_or(to, |until| until.min(to));
        if end < from {
            return out;
        }
        let day = self.day.clamp(1, 31);
        match self.cadence {
            Cadence::Daily | Cadence::Weekly => {
                let step = self.cadence.step_days();
                let mut cursor = self.start.unwrap_or(from);
                let mut guard = 0;
                while cursor < from && guard < MAX_DATES {
                    cursor += Duration::days(step);
                    guard += 1;
                }
                while cursor <= end && guard < MAX_DATES {
                    if self.start.is_none_or(|start| cursor >= start) {
                        out.push(cursor);
                    }
                    cursor += Duration::days(step);
                    guard += 1;
                }
            }
            _ => {
                let step = self.cadence.step_months();
                let anchor = self.start.unwrap_or(month_start(from));
                let anchor_index = anchor.year() * 12 + anchor.month() as i32 - 1;
                let from_index = from.year() * 12 + from.month() as i32 - 1;
                let mut step_no =
                    ((from_index - anchor_index).max(0) as f64 / step as f64).floor() as i32;
                let mut guard = 0;
                while guard < MAX_DATES {
                    let index = anchor_index + step_no * step;
                    let year = index.div_euclid(12);
                    let month = index.rem_euclid(12) as u32 + 1;
                    let date = clamped(year, month, day);
                    if date > end {
                        break;
                    }
                    let after_start = self.start.is_none_or(|start| date >= start);
                    if date >= from && after_start {
                        out.push(date);
                    }
                    step_no += 1;
                    guard += 1;
                }
            }
        }
        out
    }

    /// The figures the tax estimate needs, or `None` when the template does not
    /// declare `tax:` + `gross:`. Hand-written blocks stay tax-free on purpose:
    /// guessing 应发 from the legs would silently book a wrong number.
    pub fn tax_setup(&self) -> Option<TaxSetup> {
        let gross = self.gross?;
        self.tax_account.as_ref()?;
        Some(TaxSetup {
            gross,
            personal: self.personal.unwrap_or(Decimal::ZERO),
            deduct: self.deduct,
        })
    }

    /// Fill elided amounts and attach account types.
    pub fn legs(&self, kind_of: impl Fn(&str) -> char, precision: u32) -> Vec<Leg> {
        let mut total = Decimal::ZERO;
        let mut missing = 0usize;
        for posting in &self.postings {
            match &posting.amount {
                Some(amount) => total += amount.quantity,
                None => missing += 1,
            }
        }
        let residual = -total;
        let last = self.postings.len().saturating_sub(1);
        self.postings
            .iter()
            .enumerate()
            .map(|(index, posting)| {
                let quantity = match &posting.amount {
                    Some(amount) => amount.quantity,
                    None if missing == 1 => residual,
                    None => Decimal::ZERO,
                };
                Leg {
                    account: posting.account.clone(),
                    kind: kind_of(&posting.account),
                    role: if index == last { "settle" } else { "leg" }.to_string(),
                    amount: quantity.round_dp(precision),
                }
            })
            .collect()
    }

    pub fn totals(&self, legs: &[Leg]) -> Totals {
        let mut totals = Totals::default();
        for leg in legs {
            match leg.kind {
                'R' => totals.income -= leg.amount,
                'X' => totals.expense += leg.amount,
                'A' => totals.assets += leg.amount,
                'L' => totals.debt += leg.amount,
                _ => {}
            }
            if leg.role == "settle" {
                totals.settle = leg.amount;
            }
        }
        totals
    }
}

// ---------------------------------------------------------------------------
// rendering the salary skeleton (应发 + 公司缴纳 side)
// ---------------------------------------------------------------------------

struct Rate {
    key: &'static str,
    bps: i64,
    kind: char,
}

/// 个人部分: 养老 8% · 医疗 2% · 失业 0.5% · 公积金 7%.
///
/// 公积金按上海口径 7% + 7%；其他城市常在 5%–12% 之间，改这两个 `bps` 即可。
/// **企业年金不在里面**：它和五险一金不同，只有公司建立了年金计划才有，
/// 有了再手动加两条 `assets:年金` / `expenses:年金` posting。
const PERSONAL_RATES: [Rate; 4] = [
    Rate {
        key: "pension",
        bps: 800,
        kind: 'A',
    },
    Rate {
        key: "medical",
        bps: 200,
        kind: 'X',
    },
    Rate {
        key: "unemployment",
        bps: 50,
        kind: 'X',
    },
    Rate {
        key: "fund",
        bps: 700,
        kind: 'A',
    },
];

/// 公司部分: 养老 16% · 医疗 8% · 失业 0.5% · 工伤 0.5% · 生育 1% · 公积金 7%.
const EMPLOYER_RATES: [Rate; 6] = [
    Rate {
        key: "pension",
        bps: 1600,
        kind: 'A',
    },
    Rate {
        key: "medical",
        bps: 800,
        kind: 'X',
    },
    Rate {
        key: "unemployment",
        bps: 50,
        kind: 'X',
    },
    Rate {
        key: "injury",
        bps: 50,
        kind: 'X',
    },
    Rate {
        key: "maternity",
        bps: 100,
        kind: 'X',
    },
    Rate {
        key: "fund",
        bps: 700,
        kind: 'A',
    },
];

/// 工资薪金预扣率表（居民个人，累计预扣法）：
/// `(累计应纳税所得额上限, 税率万分比, 速算扣除数)`。
const BRACKETS: [(i64, u32, i64); 7] = [
    (36_000, 300, 0),
    (144_000, 1_000, 2_520),
    (300_000, 2_000, 16_920),
    (420_000, 2_500, 31_920),
    (660_000, 3_000, 52_920),
    (960_000, 3_500, 85_920),
    (i64::MAX, 4_500, 181_920),
];

/// 个税起征点（元/月）。
const THRESHOLD: i64 = 5_000;

/// 累计应纳税额：按累计应纳税所得额查表，减速算扣除数。
pub fn income_tax(cumulative_taxable: Decimal) -> Decimal {
    if cumulative_taxable <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    for (cap, bps, quick) in BRACKETS {
        if cap == i64::MAX || cumulative_taxable <= Decimal::from(cap) {
            let tax = cumulative_taxable * Decimal::from(bps) / Decimal::from(10_000)
                - Decimal::from(quick);
            return tax.round_dp(2).max(Decimal::ZERO);
        }
    }
    Decimal::ZERO
}

/// `30,000.00` / `¥30,000` / `30000` → 一个金额。
fn money(text: &str) -> Option<Decimal> {
    let cleaned: String = text
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == '-')
        .collect();
    cleaned.parse().ok()
}

/// `assets:cn` + `fund` → `assets:cn:fund`; `expenses` + `medical` for X kinds.
fn sub_account(prefix: &str, key: &str, kind: char) -> String {
    if kind == 'X' {
        format!("expenses:{prefix}:{key}")
    } else {
        format!("assets:{prefix}:{key}")
    }
}

/// Always render with a thousands separator; hledger accepts both styles.
fn fmt_amount(quantity: Decimal, commodity: &str, precision: u32) -> String {
    let negative = quantity.is_sign_negative();
    let text = quantity.abs().round_dp(precision).to_string();
    let (int_part, mut frac_part) = match text.split_once('.') {
        Some((int_part, frac_part)) => (int_part.to_string(), frac_part.to_string()),
        None => (text.clone(), String::new()),
    };
    while frac_part.len() < precision as usize {
        frac_part.push('0');
    }
    let mut grouped = String::new();
    let chars: Vec<char> = int_part.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0 && (chars.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*ch);
    }
    let body = if frac_part.is_empty() {
        grouped
    } else {
        format!("{grouped}.{frac_part}")
    };
    format!("{}{} {}", if negative { "-" } else { "" }, body, commodity)
}

/// Plain `30,000.00` (no commodity) for the metadata comment.
fn fmt_plain(quantity: Decimal, precision: u32) -> String {
    let negative = quantity.is_sign_negative();
    let text = quantity.abs().round_dp(precision).to_string();
    let (int_part, mut frac_part) = match text.split_once('.') {
        Some((int_part, frac_part)) => (int_part.to_string(), frac_part.to_string()),
        None => (text, String::new()),
    };
    while frac_part.len() < precision as usize {
        frac_part.push('0');
    }
    let mut grouped = String::new();
    let chars: Vec<char> = int_part.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0 && (chars.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*ch);
    }
    let body = if frac_part.is_empty() {
        grouped
    } else {
        format!("{grouped}.{frac_part}")
    };
    format!("{}{}", if negative { "-" } else { "" }, body)
}

fn posting_line(account: &str, amount: Option<(Decimal, &str, u32)>) -> String {
    let body = match amount {
        Some((quantity, commodity, precision)) => fmt_amount(quantity, commodity, precision),
        None => String::new(),
    };
    format!("    {account:<36}{body:>16}")
}

/// Stable id for a generated skeleton, so re-running it replaces rather than
/// duplicates (`id:salary-main`).
pub fn salary_id(label: &str) -> String {
    format!("{}-{}", slug(label), short_hash(label))
}

/// Render a `~` block that books gross pay, both sides of 五险一金, and the
/// take-home cash leg. The cash leg is **derived** so the block always balances
/// to the cent.
pub fn render_salary(
    request: &SalaryTemplateDto,
    base_commodity: &str,
    precision: u32,
) -> Result<String> {
    let gross: Decimal = request
        .gross
        .replace([',', ' ', '¥', '￥'], "")
        .parse()
        .map_err(|_| EngineError::Refused(format!("`{}` 不是有效金额", request.gross)))?;
    if gross <= Decimal::ZERO {
        return Err(EngineError::Refused("应发工资必须大于 0".to_string()));
    }
    let day = request.day.clamp(1, 31);
    let anchor = match request.start.as_deref() {
        Some(text) => parse_iso_date(text)
            .or_else(|| {
                // `2026-10` is also accepted: the day comes from `day`.
                let month =
                    chrono::NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok()?;
                Some(clamped(month.year(), month.month(), day))
            })
            .ok_or_else(|| EngineError::Refused(format!("`{text}` 不是有效日期")))?,
        None => clamped(
            chrono::Local::now().date_naive().year(),
            chrono::Local::now().date_naive().month(),
            day,
        ),
    };
    // hledger reads `monthly from <date>` as "every month on that day", clamping
    // short months — the same rule this module uses — so the day goes into the
    // anchor date instead of an app-only `day:` token.
    let start = clamped(anchor.year(), anchor.month(), day);
    let label = if request.label.trim().is_empty() {
        "工资".to_string()
    } else {
        request.label.trim().to_string()
    };
    let id = salary_id(&label);
    let prefix = request.prefix.trim_matches(':');
    let prefix = if prefix.is_empty() {
        "salary".to_string()
    } else {
        prefix.to_string()
    };

    // account -> (kind, amount), so employer and personal shares of the same
    // fund land on one line instead of two.
    let mut buckets: BTreeMap<String, (char, Decimal)> = BTreeMap::new();
    let mut add = |account: String, kind: char, amount: Decimal| {
        let entry = buckets.entry(account).or_insert((kind, Decimal::ZERO));
        entry.1 += amount;
    };
    let share = |rate: &Rate| -> Decimal {
        (gross * Decimal::from(rate.bps) / Decimal::from(10_000)).round_dp(precision)
    };

    let mut personal_total = Decimal::ZERO;
    for rate in &PERSONAL_RATES {
        let amount = share(rate);
        personal_total += amount;
        add(sub_account(&prefix, rate.key, rate.kind), rate.kind, amount);
    }
    let mut employer_total = Decimal::ZERO;
    if request.employer {
        for rate in &EMPLOYER_RATES {
            let amount = share(rate);
            employer_total += amount;
            add(sub_account(&prefix, rate.key, rate.kind), rate.kind, amount);
        }
    }

    let salary_account = request
        .salary_account
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("income:salary");
    let benefit_account = request
        .benefit_account
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("income:benefit");

    let income_total = gross + employer_total;
    let legs_total: Decimal = buckets.values().map(|(_, amount)| *amount).sum();
    let take_home = income_total - legs_total;

    let deduct = request
        .deduct
        .as_deref()
        .and_then(money)
        .unwrap_or(Decimal::ZERO)
        .max(Decimal::ZERO)
        .round_dp(precision);

    let mut out = String::new();
    out.push_str("; 工资 — 应发 / 五险一金(个人+公司) / 个税 / 到手\n");
    out.push_str("; 个人: 养老8% 医疗2% 失业0.5% 公积金7%\n");
    if request.employer {
        out.push_str("; 公司: 养老16% 医疗8% 失业0.5% 工伤0.5% 生育1% 公积金7%\n");
    }
    out.push_str("; 表头里的 gross(应发) personal(个人代扣) deduct(专项附加扣除) tax(个税科目)\n");
    out.push_str("; 决定记入时的个税推算; 算出来和工资条不一样, 记入时改一下就行。\n");
    out.push_str("; 比例写死在生成器里,直接改下面的数字即可;改完记得保存。\n");
    out.push_str("; 每月几号由 from 后面的日期决定 —— hledger 也是这么解读的。\n");
    out.push_str(&format!(
        "~ monthly from {}  ; id:{} label:{} kind:salary gross:{} personal:{} deduct:{} tax:expenses:个税\n",
        start.format("%Y-%m-%d"),
        id,
        label,
        fmt_plain(gross, precision),
        fmt_plain(personal_total, precision),
        fmt_plain(deduct, precision),
    ));
    out.push_str(&posting_line(
        salary_account,
        Some((-gross, base_commodity, precision)),
    ));
    out.push('\n');
    if request.employer && employer_total > Decimal::ZERO {
        out.push_str(&posting_line(
            benefit_account,
            Some((-employer_total, base_commodity, precision)),
        ));
        out.push('\n');
    }
    for (account, (_, amount)) in &buckets {
        if amount.is_zero() {
            continue;
        }
        out.push_str(&posting_line(
            account,
            Some((*amount, base_commodity, precision)),
        ));
        out.push('\n');
    }
    out.push_str(&posting_line(
        &request.net_account,
        Some((take_home, base_commodity, precision)),
    ));
    out.push('\n');
    Ok(out)
}

// ---------------------------------------------------------------------------
// scaffolding
// ---------------------------------------------------------------------------

/// Header + a commented example, so an empty file still teaches the syntax.
pub fn render_starter(commodity: &str) -> String {
    format!(
        "; MoneyTracker · 定期收支模板 (recurring.ledger)
;
; 这里声明「每月固定发生的事」: 工资、房贷、房租、订阅。语法就是 hledger 的
; `~ period` 块, 所以数字可以手改, `hledger -f index.ledger reg --forecast`
; 也能看到它们。App 的预算和现金流预测不把这里当信封, 所以不会算两遍。
;
; 表头注释:
;   id:<唯一 id>  label:<显示名>  kind:salary|loan|income|expense|other
;   enabled:false 可以暂时停用
; 期间写法:  monthly | weekly | quarterly | yearly, 可加 from 2026-11-01
;            until 2027-12-31; `day:5` 这种写法 App 也认, 但 hledger 不认, 别用
; 最后一条 posting 可以省略金额 (它是「实际付出去/收到的那张卡」),
; App 会用其余各条把它补平 —— 这一条在界面上显示为「到手 / 每月还款」。
;
; 例: 每月 20 号还房贷 8,200 (利息 6,070 + 本金 2,130)
; ~ monthly from 2026-01-20  ; id:loan-home label:房贷 kind:loan
;     liabilities:mortgage:home          2,130.00 {commodity}
;     expenses:home:mortgage_interest    6,070.00 {commodity}
;     assets:bank:icbc
;
; 每月几号由 from 后面的日期决定 (hledger 也是这样读的), 所以 `monthly from
; 2026-01-20` 就是每月 20 号; 短月自动落在月末。
"
    )
}

/// Create an empty `recurring.ledger` if the vault does not have one.
pub fn ensure_file(root: &Path, commodity: &str) -> Result<PathBuf> {
    let path = root.join(SCHEDULE_FILE);
    if !path.exists() {
        crate::writer::atomic_write(&path, &render_starter(commodity))?;
    }
    Ok(path)
}

/// Validate and save the whole file. Refuses text that would not parse, so the
/// user never silently loses a template to a typo.
pub fn save(root: &Path, text: &str, opts: &VaultOptions) -> Result<ScheduleSet> {
    let parsed = parse(text);
    if parsed.has_errors() {
        let message = parsed
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| format!("{}:{} {}", d.line, d.col, d.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(EngineError::Refused(message));
    }
    let rel = Path::new(SCHEDULE_FILE);
    let expected = crate::writer::read_rel(root, rel)
        .ok()
        .map(|(_, state)| state);
    crate::writer::write_rel(root, rel, text, expected.as_ref(), opts)?;
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// overview
// ---------------------------------------------------------------------------

fn quantity_of(amount: &AmountDto) -> Decimal {
    amount.quantity.parse().unwrap_or_default()
}

/// Format a scheduled amount the way the ledger itself prints it: the
/// commodity's observed style (thousands separators included) when the vault has
/// seen one, otherwise plain canonical digits.
fn component_dto(table: &CommodityTable, quantity: Decimal, commodity: &str) -> AmountDto {
    let precision = table.precision_of(commodity);
    let mut amount = Amount::canonical(quantity.round_dp(precision), commodity, precision);
    match table.symbols.get(commodity) {
        Some(style) => {
            amount.style = style.clone();
            amount.style.precision = precision;
        }
        None => amount.style.group_mark = Some(','),
    }
    to_dto_prec(&amount, precision)
}

fn leg_dto(table: &CommodityTable, leg: &Leg, commodity: &str) -> ScheduleLegDto {
    ScheduleLegDto {
        account: leg.account.clone(),
        kind: leg.kind.to_string(),
        role: leg.role.clone(),
        amount: component_dto(table, leg.amount, commodity),
    }
}

/// Recorded occurrences, keyed by `(template, month)`.
fn recorded(vault: &Vault) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for record in vault.transactions() {
        for tag in &record.txn.tags {
            if tag.key != TAG_KEY {
                continue;
            }
            let Some(value) = &tag.value else { continue };
            let (id, month) = match value.split_once('@') {
                Some((id, month)) => (id.to_string(), month.trim().to_string()),
                None => (value.clone(), month_key(record.txn.date)),
            };
            let txn_id = record.txn.tag("txn").unwrap_or_default().to_string();
            out.insert((id, month), txn_id);
        }
    }
    out
}

fn skipped(root: &Path) -> BTreeSet<(String, String)> {
    crate::recur::skip_list(root)
        .into_iter()
        .filter_map(|(id, month)| {
            id.strip_prefix(SKIP_PREFIX)
                .map(|id| (id.to_string(), month))
        })
        .collect()
}

/// Tax already booked per `(template, month)`, read back from the ledger — so a
/// figure the user overrode at commit time keeps counting for the next month.
fn booked_taxes(vault: &Vault, set: &ScheduleSet) -> BTreeMap<(String, String), Decimal> {
    let mut out: BTreeMap<(String, String), Decimal> = BTreeMap::new();
    for template in &set.templates {
        let Some(account) = template.tax_account.as_deref() else {
            continue;
        };
        for record in vault.transactions() {
            let Some(tag) = record.txn.tags.iter().find(|tag| tag.key == TAG_KEY) else {
                continue;
            };
            let Some(value) = &tag.value else { continue };
            let (id, month) = match value.split_once('@') {
                Some((id, month)) => (id.to_string(), month.trim().to_string()),
                None => (value.clone(), month_key(record.txn.date)),
            };
            if id != template.id {
                continue;
            }
            let tax: Decimal = record
                .txn
                .postings
                .iter()
                .filter(|posting| posting.account == account)
                .filter_map(|posting| posting.amount.as_ref().map(|amount| amount.quantity))
                .sum();
            if !tax.is_zero() {
                *out.entry((id, month)).or_default() += tax;
            }
        }
    }
    out
}

/// `(本月应纳税所得额, 本月应预扣个税)` for one occurrence.
///
/// 累计预扣法: 本月应扣 = 累计应纳税额(累计所得) − 已预缴. The cumulative count
/// comes from the ledger, so a year that starts mid-way (or a month you skipped)
/// corrects itself instead of assuming twelve payments.
fn tax_for(
    template: &ScheduleTemplate,
    date: NaiveDate,
    booked: &BTreeMap<(String, String), Decimal>,
    precision: u32,
) -> (Decimal, Decimal) {
    let Some(setup) = template.tax_setup() else {
        return (Decimal::ZERO, Decimal::ZERO);
    };
    let base = (setup.gross - Decimal::from(THRESHOLD) - setup.personal - setup.deduct)
        .max(Decimal::ZERO)
        .round_dp(precision);
    if base.is_zero() {
        return (base, Decimal::ZERO);
    }
    let year = date.format("%Y").to_string();
    let month = month_key(date);
    let mut months = 0u32;
    let mut paid = Decimal::ZERO;
    for ((id, booked_month), tax) in booked {
        if id != &template.id || !booked_month.starts_with(&year) || booked_month >= &month {
            continue;
        }
        months += 1;
        paid += *tax;
    }
    let cumulative = base * Decimal::from(months + 1);
    let due = (income_tax(cumulative) - paid)
        .max(Decimal::ZERO)
        .round_dp(precision);
    (base, due)
}

pub fn overview(vault: &Vault, months: usize, today: NaiveDate) -> ScheduleOverviewDto {
    let base = vault.base_commodity().0.to_string();
    let precision = vault.base_commodity().1;
    let set = load(vault.root());
    let done = recorded(vault);
    let muted = skipped(vault.root());
    let booked = booked_taxes(vault, &set);

    let count = months.clamp(1, MAX_MONTHS);
    let first = month_start(today);
    let mut month_keys = Vec::new();
    for offset in 0..count {
        month_keys.push(month_key(shift_months(first, offset as i32)));
    }
    let horizon_end = month_end(shift_months(first, count as i32 - 1));

    let mut occurrences = Vec::new();
    let mut templates = Vec::new();
    for template in &set.templates {
        let legs = template.legs(|account| vault.kind_for(account), precision);
        let totals = template.totals(&legs);
        let window = if template.enabled {
            template.dates(first, horizon_end)
        } else {
            Vec::new()
        };
        let dates = window
            .into_iter()
            .map(|date| {
                let month = month_key(date);
                let txn_id = done.get(&(template.id.clone(), month.clone())).cloned();
                let is_skipped = muted.contains(&(template.id.clone(), month.clone()));
                let is_recorded = txn_id.is_some();
                let (tax_base, tax_due) = tax_for(template, date, &booked, precision);
                ScheduleOccurrenceDto {
                    id: template.id.clone(),
                    label: template.label.clone(),
                    kind: template.kind.clone(),
                    month,
                    date: date.format("%Y-%m-%d").to_string(),
                    recorded: is_recorded,
                    skipped: is_skipped,
                    due: date <= today && !is_recorded && !is_skipped,
                    settle: component_dto(&vault.commodities, totals.settle, &base),
                    income: component_dto(&vault.commodities, totals.income, &base),
                    expense: component_dto(&vault.commodities, totals.expense, &base),
                    assets: component_dto(&vault.commodities, totals.assets, &base),
                    debt: component_dto(&vault.commodities, totals.debt, &base),
                    tax_base: component_dto(&vault.commodities, tax_base, &base),
                    tax: component_dto(&vault.commodities, tax_due, &base),
                    legs: legs
                        .iter()
                        .map(|leg| leg_dto(&vault.commodities, leg, &base))
                        .collect(),
                    txn_id,
                }
            })
            .collect::<Vec<_>>();

        let next = template
            .dates(today, shift_months(today, 24))
            .into_iter()
            .next();
        let next_date = next.map(|date| date.format("%Y-%m-%d").to_string());
        let tax = next.map_or(Decimal::ZERO, |date| {
            tax_for(template, date, &booked, precision).1
        });
        templates.push(ScheduleTemplateDto {
            id: template.id.clone(),
            label: template.label.clone(),
            kind: template.kind.clone(),
            period: template.period.clone(),
            cadence: template.cadence.label().to_string(),
            day: template.day,
            start: template.start.map(|d| d.format("%Y-%m-%d").to_string()),
            until: template.until.map(|d| d.format("%Y-%m-%d").to_string()),
            enabled: template.enabled,
            settle: component_dto(&vault.commodities, totals.settle, &base),
            income: component_dto(&vault.commodities, totals.income, &base),
            expense: component_dto(&vault.commodities, totals.expense, &base),
            assets: component_dto(&vault.commodities, totals.assets, &base),
            debt: component_dto(&vault.commodities, totals.debt, &base),
            next_date,
            tax_account: template.tax_account.clone(),
            deduct: component_dto(&vault.commodities, template.deduct, &base),
            tax: component_dto(&vault.commodities, tax, &base),
            line: template.line,
            legs: legs
                .iter()
                .map(|leg| leg_dto(&vault.commodities, leg, &base))
                .collect(),
        });
        occurrences.extend(dates);
    }

    let mut month_totals = Vec::new();
    for month in &month_keys {
        let rows: Vec<&ScheduleOccurrenceDto> = occurrences
            .iter()
            .filter(|occurrence| &occurrence.month == month)
            .collect();
        let mut income = Decimal::ZERO;
        let mut expense = Decimal::ZERO;
        let mut assets = Decimal::ZERO;
        let mut debt = Decimal::ZERO;
        for row in &rows {
            income += quantity_of(&row.income);
            expense += quantity_of(&row.expense);
            assets += quantity_of(&row.assets);
            debt += quantity_of(&row.debt);
        }
        month_totals.push(ScheduleMonthDto {
            month: month.clone(),
            income: component_dto(&vault.commodities, income, &base),
            expense: component_dto(&vault.commodities, expense, &base),
            assets: component_dto(&vault.commodities, assets, &base),
            debt: component_dto(&vault.commodities, debt, &base),
            template_count: rows.len(),
            recorded: rows.iter().filter(|row| row.recorded).count(),
            pending: rows
                .iter()
                .filter(|row| !row.recorded && !row.skipped)
                .count(),
        });
    }

    occurrences.sort_by(|a, b| a.date.cmp(&b.date).then(a.label.cmp(&b.label)));

    ScheduleOverviewDto {
        today: today.format("%Y-%m-%d").to_string(),
        has_schedule: set.exists && !set.templates.is_empty(),
        file: SCHEDULE_FILE.to_string(),
        templates,
        months: month_keys,
        occurrences,
        month_totals,
        diagnostics: set.diagnostics.iter().map(diagnostic_dto).collect(),
        src: set.src,
        auto_commit: false,
    }
}

// ---------------------------------------------------------------------------
// booking
// ---------------------------------------------------------------------------

/// Book one occurrence of `id` in `month` (default: this month).
pub fn commit(
    vault: &Vault,
    id: &str,
    month: Option<&str>,
    tax: Option<Decimal>,
    opts: &VaultOptions,
) -> Result<ScheduleCommitDto> {
    let today = chrono::Local::now().date_naive();
    let month = month
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| month_key(today));
    let month_date = parse_iso_date(&month)
        .ok_or_else(|| EngineError::Refused(format!("`{month}` 不是有效月份")))?;

    let precision = vault.base_commodity().1;
    let set = load(vault.root());
    let template = set
        .get(id)
        .ok_or_else(|| EngineError::NotFound(format!("模板 `{id}` 不存在")))?;
    let date = template
        .dates(month_start(month_date), month_end(month_date))
        .first()
        .copied()
        .ok_or_else(|| EngineError::NotFound(format!("`{id}` 在 {month} 没有发生")))?;

    if recorded(vault).contains_key(&(id.to_string(), month.clone())) {
        return Ok(ScheduleCommitDto {
            written: 0,
            already: 1,
            files: Vec::new(),
            months: vec![month],
            errors: Vec::new(),
        });
    }

    let legs = template.legs(|account| vault.kind_for(account), precision);
    let totals = template.totals(&legs);
    let commodity = vault.base_commodity().0.to_string();
    let booked = booked_taxes(vault, &set);
    let tax = tax
        .unwrap_or_else(|| tax_for(template, date, &booked, precision).1)
        .round_dp(precision)
        .max(Decimal::ZERO);
    let direction = if totals.income > Decimal::ZERO {
        Direction::Income
    } else {
        Direction::Expense
    };

    let mut txn = Transaction::new(date, template.label.clone());
    txn.status = Some(Status::Cleared);
    txn.postings = template.postings.clone();
    // 个税夹在最后一条（现金腿）之前，并从现金里扣掉：既保持「最后一条 = 到手」
    // 的约定，也不用手写金额去对平。
    if let Some(account) = template.tax_account.as_ref() {
        if !tax.is_zero() {
            let last = txn.postings.len().saturating_sub(1);
            if let Some(amount) = txn.postings[last].amount.as_mut() {
                amount.quantity -= tax;
            }
            txn.postings.insert(
                last,
                Posting::new(
                    account.clone(),
                    Some(Amount::canonical(tax, &commodity, precision)),
                ),
            );
        }
    }
    txn.tags = vec![Tag::kv(TAG_KEY, format!("{id}@{month}"))];

    let expanded = crate::capture::ExpandedCapture {
        txn,
        postings: Vec::new(),
        canonical: String::new(),
        warnings: Vec::new(),
        errors: Vec::new(),
        rule_id: None,
        direction,
        summary: format!("{} · {month}", template.label),
    };
    let block =
        crate::capture::commit_to_day_file(vault, &expanded, &crate::capture::new_txn_id(), opts)?;

    Ok(ScheduleCommitDto {
        written: 1,
        already: 0,
        files: vec![block.rel],
        months: vec![month],
        errors: Vec::new(),
    })
}

/// Book every occurrence that is due in the current month. Used by the
/// automation pass when the user turns `auto_recur` on.
pub fn commit_due(vault: &Vault, opts: &VaultOptions) -> Result<ScheduleCommitDto> {
    let today = chrono::Local::now().date_naive();
    let month = month_key(today);
    let listed = overview(vault, 1, today);
    let due: Vec<String> = listed
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.due && occurrence.month == month)
        .map(|occurrence| occurrence.id.clone())
        .collect();

    let mut out = ScheduleCommitDto {
        written: 0,
        already: 0,
        files: Vec::new(),
        months: vec![month.clone()],
        errors: Vec::new(),
    };
    for id in due {
        match commit(vault, &id, Some(&month), None, opts) {
            Ok(result) => {
                out.written += result.written;
                out.already += result.already;
                for file in result.files {
                    if !out.files.contains(&file) {
                        out.files.push(file);
                    }
                }
            }
            Err(error) => out.errors.push(format!("{id}: {}", error.message())),
        }
    }
    Ok(out)
}

/// "Not this month" — hides the occurrence without writing anything.
pub fn skip(vault: &Vault, id: &str, month: Option<&str>) -> Result<()> {
    let today = chrono::Local::now().date_naive();
    let month = month
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| month_key(today));
    crate::recur::skip(vault.root(), &format!("{SKIP_PREFIX}{id}"), &month)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VaultCreateOptions;

    const LOAN: &str = "~ monthly from 2026-01-20 day:20  ; id:loan-home label:房贷 kind:loan
    liabilities:mortgage:home          2,130.00 CNY
    expenses:home:mortgage_interest    6,070.00 CNY
    assets:bank:icbc
";

    fn kinds(account: &str) -> char {
        let top = account.split(':').next().unwrap_or_default();
        match top {
            "assets" => 'A',
            "liabilities" => 'L',
            "income" => 'R',
            "expenses" => 'X',
            "equity" => 'E',
            _ => 'X',
        }
    }

    #[test]
    fn parses_header_metadata() {
        let set = parse(LOAN);
        assert_eq!(set.templates.len(), 1);
        assert!(!set.has_errors());
        let template = &set.templates[0];
        assert_eq!(template.id, "loan-home");
        assert_eq!(template.label, "房贷");
        assert_eq!(template.kind, "loan");
        assert_eq!(template.cadence, Cadence::Monthly);
        assert_eq!(template.day, 20);
        assert_eq!(
            template.start,
            Some(NaiveDate::from_ymd_opt(2026, 1, 20).unwrap())
        );
        assert!(template.enabled);
        assert_eq!(template.line, 1);
        assert_eq!(template.postings.len(), 3);
    }

    #[test]
    fn rejects_unknown_period() {
        let set =
            parse("~ every-blue-moon  ; id:x label:x\n    expenses:food  1 CNY\n    assets:cash\n");
        assert!(set.has_errors());
        assert!(set.templates.is_empty());
        assert_eq!(set.diagnostics[0].code, "core.schedule.period");
    }

    #[test]
    fn clamps_day_to_short_months() {
        let set =
            parse("~ monthly day:31  ; id:x label:x\n    expenses:food  1 CNY\n    assets:cash\n");
        let template = &set.templates[0];
        let jan = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let apr = month_end(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
        let dates = template.dates(jan, apr);
        assert_eq!(
            dates,
            vec![
                NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
                NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
                NaiveDate::from_ymd_opt(2026, 4, 30).unwrap(),
            ]
        );
        // `day:` is app-only, so parsing it has to say so (the file is in index.ledger).
        assert_eq!(set.diagnostics.len(), 1);
        assert_eq!(set.diagnostics[0].code, "core.schedule.day_token");
        assert_eq!(set.diagnostics[0].severity, Severity::Warning);
    }

    #[test]
    fn the_from_date_carries_the_day_of_month() {
        let set = parse(
            "~ monthly from 2026-01-31  ; id:x label:x\n    expenses:food  1 CNY\n    assets:cash\n",
        );
        assert!(set.diagnostics.is_empty(), "{:?}", set.diagnostics);
        let template = &set.templates[0];
        assert_eq!(template.day, 31);
        assert_eq!(
            template.dates(
                NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                month_end(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap())
            ),
            vec![
                NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
                NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
                NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            ]
        );
    }

    #[test]
    fn resets_the_grid_when_the_window_starts_later() {
        let set = parse(LOAN);
        let template = &set.templates[0];
        let dates = template.dates(
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            month_end(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
        );
        assert_eq!(
            dates,
            vec![
                NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
                NaiveDate::from_ymd_opt(2026, 12, 20).unwrap(),
            ]
        );
    }

    #[test]
    fn fills_the_elided_cash_leg() {
        let set = parse(LOAN);
        let legs = set.templates[0].legs(kinds, 2);
        assert_eq!(legs.len(), 3);
        assert_eq!(legs[2].role, "settle");
        assert_eq!(legs[2].account, "assets:bank:icbc");
        assert_eq!(legs[2].amount, Decimal::from(-8200));
        let totals = set.templates[0].totals(&legs);
        assert_eq!(totals.settle, Decimal::from(-8200));
        assert_eq!(totals.expense, Decimal::from(6070));
        assert_eq!(totals.debt, Decimal::from(2130));
        assert_eq!(totals.income, Decimal::ZERO);
    }

    #[test]
    fn salary_skeleton_balances() {
        let request = SalaryTemplateDto {
            label: "工资".into(),
            gross: "30000".into(),
            day: 5,
            start: Some("2026-11-01".into()),
            net_account: "assets:bank:icbc".into(),
            salary_account: None,
            benefit_account: None,
            prefix: "cn".into(),
            employer: true,
            deduct: Some("1000".into()),
        };
        let text = render_salary(&request, "CNY", 2).expect("render");
        let set = parse(&text);
        assert!(!set.has_errors(), "{:?}", set.diagnostics);
        let template = &set.templates[0];
        assert_eq!(template.kind, "salary");
        assert_eq!(template.day, 5);
        let legs = template.legs(kinds, 2);
        let sum: Decimal = legs.iter().map(|leg| leg.amount).sum();
        assert_eq!(sum, Decimal::ZERO, "salary block must balance:\n{text}");
        // 应发 30,000 + 公司缴 9,900 = 39,900；个人代扣 5,250.00 (养老8 医疗2 失业0.5
        // 公积金7，没有年金) → 到手 24,750.00.
        let totals = template.totals(&legs);
        assert_eq!(totals.income, Decimal::from(39900));
        assert_eq!(totals.expense, Decimal::from(4462) + Decimal::new(50, 2));
        assert_eq!(totals.assets, Decimal::from(43018) + Decimal::new(50, 2));
        assert_eq!(totals.settle, Decimal::from(29452) + Decimal::new(50, 2));
        assert!(text.contains("assets:cn:fund"), "{text}");
        assert!(text.contains("expenses:cn:medical"), "{text}");
        // 企业年金是自愿的，默认不生成。
        assert!(!text.contains("annuity"), "{text}");
        // 税务元数据: 应发 / 个人代扣 / 专项附加扣除 / 个税科目.
        assert!(text.contains("gross:30,000.00"), "{text}");
        assert!(text.contains("personal:5,250.00"), "{text}");
        assert!(text.contains("deduct:1,000.00"), "{text}");
        assert!(text.contains("tax:expenses:个税"), "{text}");
        assert_eq!(
            template.tax_setup().map(|s| s.gross),
            Some(Decimal::from(30000))
        );
        // hledger ignores an app-specific `day:` token, and the file is included
        // by index.ledger now, so the day has to live in the anchor date.
        assert!(!text.contains("day:"), "{text}");
        assert!(text.contains("~ monthly from 2026-11-05"), "{text}");
    }

    #[test]
    fn starter_template_only_comments_the_example() {
        let set = parse(&render_starter("CNY"));
        assert!(set.templates.is_empty(), "example must stay commented out");
        assert!(!set.has_errors());
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = VaultOptions::default();
        crate::vault::create_vault(dir.path(), &VaultCreateOptions::default()).expect("vault");
        std::fs::write(dir.path().join(SCHEDULE_FILE), LOAN).expect("schedule");
        let vault = Vault::load(dir.path(), &opts).expect("load");
        (dir, vault)
    }

    #[test]
    fn the_vault_index_includes_the_schedule() {
        let dir = tempfile::tempdir().unwrap();
        crate::vault::create_vault(dir.path(), &VaultCreateOptions::default()).unwrap();
        let index = std::fs::read_to_string(dir.path().join("index.ledger")).unwrap();
        assert!(
            index
                .lines()
                .any(|line| line.trim() == format!("include {SCHEDULE_FILE}")),
            "index.ledger must include {SCHEDULE_FILE}:\n{index}"
        );
        assert!(dir.path().join(SCHEDULE_FILE).exists());
    }

    #[test]
    fn the_schedule_is_not_a_budget_envelope() {
        let (dir, mut vault) = vault();
        // budgets.ledger ships with a real envelope; the schedule must not add one.
        let before = crate::report::budgets(&vault).len();
        crate::schedule::save(
            dir.path(),
            "~ monthly\n    expenses:food  500.00 CNY\n    assets:cash\n",
            &VaultOptions::default(),
        )
        .expect("save");
        vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        let budgets = crate::report::budgets(&vault);
        assert_eq!(budgets.len(), before);
        assert!(
            !budgets.iter().any(|b| b.raw.contains("expenses:food")),
            "schedule legs must not become envelopes: {budgets:?}"
        );
    }

    #[test]
    fn commit_books_one_occurrence_and_is_idempotent() {
        let (dir, vault) = vault();
        let opts = VaultOptions::default();
        let first = commit(&vault, "loan-home", Some("2026-11"), None, &opts).expect("commit");
        assert_eq!(first.written, 1);
        assert_eq!(first.months, vec!["2026-11".to_string()]);
        assert!(!first.files.is_empty());

        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        let booked = reloaded
            .transactions()
            .iter()
            .find(|record| record.txn.tag(TAG_KEY).is_some())
            .expect("booked txn");
        assert_eq!(
            booked.txn.date,
            NaiveDate::from_ymd_opt(2026, 11, 20).unwrap()
        );
        assert_eq!(booked.txn.payee, "房贷");
        assert_eq!(booked.txn.tag(TAG_KEY), Some("loan-home@2026-11"));

        let again =
            commit(&reloaded, "loan-home", Some("2026-11"), None, &opts).expect("commit again");
        assert_eq!(again.written, 0);
        assert_eq!(again.already, 1);
        let booked = Vault::load(dir.path(), &opts)
            .expect("reload")
            .transactions()
            .iter()
            .filter(|record| record.txn.tag(TAG_KEY).is_some())
            .count();
        assert_eq!(booked, 1);
    }

    #[test]
    fn overview_marks_recorded_and_totals_the_month() {
        let (dir, vault) = vault();
        let opts = VaultOptions::default();
        commit(&vault, "loan-home", Some("2026-11"), None, &opts).expect("commit");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        let today = NaiveDate::from_ymd_opt(2026, 11, 28).unwrap();
        let over = overview(&reloaded, 3, today);
        assert_eq!(over.months, vec!["2026-11", "2026-12", "2027-01"]);
        let november = &over
            .occurrences
            .iter()
            .find(|occurrence| occurrence.month == "2026-11")
            .expect("november");
        assert!(november.recorded);
        assert!(!november.due);
        assert_eq!(november.date, "2026-11-20");
        assert_eq!(november.settle.value, -8200.0);
        assert!(november.txn_id.is_some());

        let totals = &over.month_totals[0];
        assert_eq!(totals.month, "2026-11");
        assert_eq!(totals.recorded, 1);
        assert_eq!(totals.pending, 0);
        assert_eq!(totals.expense.value, 6070.0);

        // December has not been booked yet, so it is still pending.
        assert_eq!(over.month_totals[1].pending, 1);
    }

    #[test]
    fn skip_hides_an_occurrence() {
        let (dir, vault) = vault();
        let opts = VaultOptions::default();
        skip(&vault, "loan-home", Some("2026-12")).expect("skip");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        let over = overview(&reloaded, 2, NaiveDate::from_ymd_opt(2026, 12, 10).unwrap());
        let december = &over
            .occurrences
            .iter()
            .find(|occurrence| occurrence.month == "2026-12")
            .expect("december");
        assert!(december.skipped);
        assert!(!december.due);
        assert_eq!(over.month_totals[0].month, "2026-12");
        assert_eq!(over.month_totals[0].pending, 0);
    }

    #[test]
    fn save_refuses_text_that_does_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        crate::vault::create_vault(dir.path(), &VaultCreateOptions::default()).unwrap();
        let opts = VaultOptions::default();
        let before = std::fs::read_to_string(dir.path().join(SCHEDULE_FILE)).unwrap();
        let error = save(dir.path(), "~ sometimes  ; id:x\n", &opts).expect_err("must refuse");
        assert!(matches!(error, EngineError::Refused(_)));
        let after = std::fs::read_to_string(dir.path().join(SCHEDULE_FILE)).unwrap();
        assert_eq!(before, after, "a refused save must not touch the file");

        save(dir.path(), LOAN, &opts).expect("save");
        let set = load(dir.path());
        assert_eq!(set.templates.len(), 1);
        assert!(set.exists);
    }

    #[test]
    fn missing_file_is_an_empty_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let set = load(dir.path());
        assert!(!set.exists);
        assert!(set.templates.is_empty());
        assert!(!set.has_errors());
    }

    // -- 个税 -------------------------------------------------------------

    fn salary_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = VaultOptions::default();
        crate::vault::create_vault(dir.path(), &VaultCreateOptions::default()).expect("vault");
        let request = SalaryTemplateDto {
            label: "工资".into(),
            gross: "30000".into(),
            day: 15,
            start: Some("2026-10-01".into()),
            net_account: "assets:bank:cmb".into(),
            salary_account: None,
            benefit_account: None,
            prefix: "cn".into(),
            employer: true,
            deduct: Some("1000".into()),
        };
        let text = render_salary(&request, "CNY", 2).expect("render");
        std::fs::write(dir.path().join(SCHEDULE_FILE), text).expect("write schedule");
        let vault = Vault::load(dir.path(), &opts).expect("load");
        (dir, vault)
    }

    fn tax_paid(vault: &Vault, account: &str) -> Decimal {
        vault
            .transactions()
            .iter()
            .flat_map(|record| record.txn.postings.iter())
            .filter(|posting| posting.account == account)
            .filter_map(|posting| posting.amount.as_ref().map(|amount| amount.quantity))
            .sum()
    }

    #[test]
    fn income_tax_follows_the_cumulative_brackets() {
        assert_eq!(income_tax(Decimal::ZERO), Decimal::ZERO);
        assert_eq!(income_tax(Decimal::new(-5, 0)), Decimal::ZERO);
        // 36,000 × 3%
        assert_eq!(income_tax(Decimal::from(36_000)), Decimal::from(1_080));
        // 37,500 × 10% − 2,520
        assert_eq!(income_tax(Decimal::from(37_500)), Decimal::new(123_000, 2));
        // 144,000 × 10% − 2,520
        assert_eq!(income_tax(Decimal::from(144_000)), Decimal::from(11_880));
    }

    #[test]
    fn the_tax_base_uses_personal_withholdings_and_deductions() {
        let (_dir, vault) = salary_vault();
        let set = load(vault.root());
        let template = set
            .get("工资-6279cb")
            .or_else(|| set.templates.first())
            .expect("template");
        // 30,000 − 5,000 − 5,250.00 − 1,000
        let setup = template.tax_setup().expect("tax setup");
        assert_eq!(setup.gross, Decimal::from(30_000));
        assert_eq!(setup.personal, Decimal::new(525_000, 2));
        assert_eq!(setup.deduct, Decimal::from(1_000));
        let (base, due) = tax_for(
            template,
            NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
            &BTreeMap::new(),
            2,
        );
        assert_eq!(base, Decimal::new(1_875_000, 2));
        // 第一笔：18,750.00 × 3% = 562.50
        assert_eq!(due, Decimal::new(56_250, 2));
    }

    #[test]
    fn committing_books_the_tax_and_shrinks_the_cash_leg() {
        let (dir, vault) = salary_vault();
        let opts = VaultOptions::default();
        commit(&vault, "工资-6279cb", Some("2026-10"), None, &opts).expect("commit");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        let record = reloaded
            .transactions()
            .iter()
            .find(|record| record.txn.tag(TAG_KEY).is_some())
            .expect("booked");
        assert_eq!(record.txn.tag(TAG_KEY), Some("工资-6279cb@2026-10"));
        // 个税夹在现金腿之前，最后一条仍然是到手。
        let accounts: Vec<&str> = record
            .txn
            .postings
            .iter()
            .map(|posting| posting.account.as_str())
            .collect();
        assert_eq!(accounts[accounts.len() - 2], "expenses:个税");
        assert_eq!(accounts[accounts.len() - 1], "assets:bank:cmb");
        assert_eq!(
            tax_paid(&reloaded, "expenses:个税"),
            Decimal::new(56_250, 2)
        );
        let cash = record
            .txn
            .postings
            .last()
            .unwrap()
            .amount
            .as_ref()
            .unwrap()
            .quantity;
        // 24,750.00 − 562.50
        assert_eq!(cash, Decimal::new(2_418_750, 2));
    }

    #[test]
    fn the_second_month_withholds_the_bracket_jump() {
        let (dir, vault) = salary_vault();
        let opts = VaultOptions::default();
        commit(&vault, "工资-6279cb", Some("2026-10"), None, &opts).expect("october");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        commit(&reloaded, "工资-6279cb", Some("2026-11"), None, &opts).expect("november");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        // 累计 37,500 → 10% − 2,520 = 1,230.00；已预缴 562.50 → 本月 667.50
        assert_eq!(
            tax_paid(&reloaded, "expenses:个税"),
            Decimal::new(123_000, 2)
        );
    }

    #[test]
    fn a_manual_tax_override_sticks_and_feeds_the_next_month() {
        let (dir, vault) = salary_vault();
        let opts = VaultOptions::default();
        commit(
            &vault,
            "工资-6279cb",
            Some("2026-10"),
            Some(Decimal::from(640)),
            &opts,
        )
        .expect("commit with override");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        assert_eq!(tax_paid(&reloaded, "expenses:个税"), Decimal::from(640));
        // 下一笔按「累计 37,500 − 已缴 640」算，所以会补回来。
        let over = overview(&reloaded, 2, NaiveDate::from_ymd_opt(2026, 10, 20).unwrap());
        let november = over
            .occurrences
            .iter()
            .find(|occurrence| occurrence.month == "2026-11")
            .expect("november");
        assert_eq!(quantity_of(&november.tax), Decimal::new(59_000, 2));
    }

    #[test]
    fn a_block_without_tax_metadata_books_no_tax() {
        let (dir, vault) = vault();
        let opts = VaultOptions::default();
        commit(&vault, "loan-home", Some("2026-11"), None, &opts).expect("commit");
        let reloaded = Vault::load(dir.path(), &opts).expect("reload");
        assert!(tax_paid(&reloaded, "expenses:个税").is_zero());
    }
}

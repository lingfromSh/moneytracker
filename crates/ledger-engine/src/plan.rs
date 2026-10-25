//! Planning: `goals.ledger`, the FIRE projection, debt progress and allocation.
//!
//! `goals.ledger` is application metadata — like `rules.ledger` it is **not**
//! included by `index.ledger`, so hledger never sees it and the ledger stays a
//! valid plain-text file. Everything in this module is derived: nothing is
//! written back except when the user saves the plan text.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use ledger_core::parser::{parse_date_str, parse_decimal, parse_tags, split_comment};
use ledger_core::{Amount, Diagnostic, Item, Span};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use crate::amounts::{decimal_to_dto, f64_of};
use crate::load::Vault;
use crate::report::{last_month, month_series, monthly_totals, MonthTotals};
use crate::types::{
    AllocationDto, AllocationRowDto, AssumptionsDto, DebtDto, DiagnosticDto, ExtraPaymentDto,
    FireDto, FireTargetDto, GoalDto, PlanOverviewDto, PoolMemberDto, ReserveDto,
};

pub const GOALS_FILE: &str = "goals.ledger";

/// Safety net: never project further than a century of monthly steps.
const MAX_MONTHS: i64 = 1200;
const EXTRA_PAYMENTS: [i64; 3] = [500, 1000, 2000];

// ---------------------------------------------------------------------------
// model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Assumptions {
    /// Nominal annual return used for the projection.
    pub nominal_return: Decimal,
    pub inflation: Decimal,
    pub reserve_months: u32,
}

impl Default for Assumptions {
    fn default() -> Self {
        Self {
            nominal_return: Decimal::new(6, 2),
            inflation: Decimal::new(25, 3),
            reserve_months: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GoalKind {
    Fire,
    Account,
}

impl GoalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalKind::Fire => "fire",
            GoalKind::Account => "account",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Goal {
    pub kind: GoalKind,
    pub label: String,
    pub swr: Option<Decimal>,
    pub spend: Option<Amount>,
    pub by: Option<NaiveDate>,
    pub account: Option<String>,
    pub target: Option<Amount>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DebtSpec {
    pub label: String,
    pub account: String,
    pub rate: Option<Decimal>,
    pub payment: Option<Amount>,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct PlanSet {
    pub assumptions: Assumptions,
    pub pool: Vec<String>,
    pub alloc: Vec<(String, Decimal)>,
    pub goals: Vec<Goal>,
    pub debts: Vec<DebtSpec>,
    pub diagnostics: Vec<Diagnostic>,
    pub src: String,
}

impl PlanSet {
    /// Parse `goals.ledger`. Unknown directives become diagnostics instead of
    /// errors: a broken plan must never stop the ledger from loading.
    pub fn parse(file: &str, src: &str) -> Self {
        let mut set = PlanSet {
            assumptions: Assumptions::default(),
            src: src.to_string(),
            ..PlanSet::default()
        };
        let mut offset = 0usize;
        for raw in src.split('\n') {
            let line_start = offset;
            offset += raw.len() + 1;
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with([';', '#', '%', '|', '*']) {
                continue;
            }
            let (body, comment) = split_comment(line);
            let body = body.trim();
            let lead = line.len() - line.trim_start().len();
            let span = Span {
                start: line_start + lead,
                end: line_start + lead + body.len(),
            };
            let tags = comment.map(parse_tags).unwrap_or_default();
            let tag = |key: &str| {
                tags.iter()
                    .find(|t| t.key == key)
                    .and_then(|t| t.value.as_deref())
            };
            let base = set.assumptions.clone();
            macro_rules! diag {
                ($code:expr, $msg:expr $(,)?) => {
                    set.diagnostics
                        .push(Diagnostic::error($code, $msg.to_string(), file, span))
                };
            }
            let mut words = body.split_whitespace();
            match words.next().unwrap_or("") {
                "assume" => {
                    if let Some(v) = tag("return").and_then(share) {
                        set.assumptions.nominal_return = v;
                    } else if tag("return").is_some() {
                        diag!("plan.bad_percent", "return: 需要一个百分比");
                    }
                    if let Some(v) = tag("inflation").and_then(share) {
                        set.assumptions.inflation = v;
                    }
                    if let Some(v) =
                        tag("reserve").and_then(|v| v.trim_end_matches('月').parse().ok())
                    {
                        set.assumptions.reserve_months = v;
                    }
                }
                "pool" => {
                    let list = tag("accounts").unwrap_or_default();
                    set.pool = list
                        .split(',')
                        .map(|a| a.trim().to_lowercase())
                        .filter(|a| !a.is_empty())
                        .collect();
                    if set.pool.is_empty() {
                        diag!("plan.unknown_directive", "pool 需要 accounts:a,b,c");
                    }
                }
                "alloc" => {
                    for (key, value) in tags.iter().map(|t| (t.key.clone(), t.value.clone())) {
                        if key == "accounts" {
                            continue;
                        }
                        match value.as_deref().and_then(share) {
                            Some(v) => set.alloc.push((key.to_lowercase(), v)),
                            None => diag!("plan.bad_percent", format!("alloc {key}: 需要百分比")),
                        }
                    }
                    if set.alloc.is_empty() {
                        diag!("plan.unknown_directive", "alloc 需要 equity:60% 这样的占比");
                    }
                }
                "goal" => {
                    let kind = match words.next() {
                        Some("fire") => GoalKind::Fire,
                        Some("account") => GoalKind::Account,
                        _ => {
                            diag!("plan.unknown_directive", "goal 后面要写 fire 或 account",);
                            continue;
                        }
                    };
                    let label = words.collect::<Vec<_>>().join(" ");
                    if label.is_empty() {
                        diag!("plan.unknown_directive", "goal 需要一个名字");
                        continue;
                    }
                    let by = match tag("by") {
                        Some(v) => match parse_date_str(v) {
                            Some(d) => Some(d),
                            None => {
                                diag!("plan.bad_date", format!("by:{v} 不是日期"));
                                None
                            }
                        },
                        None => None,
                    };
                    let spend = tag("spend").map(|v| parse_amount(v, &base));
                    if spend.as_ref().is_some_and(|a| a.is_none()) {
                        diag!("plan.bad_amount", "spend: 金额无法解析");
                    }
                    let target = tag("target").map(|v| parse_amount(v, &base));
                    if target.as_ref().is_some_and(|a| a.is_none()) {
                        diag!("plan.bad_amount", "target: 金额无法解析");
                    }
                    let goal = Goal {
                        kind: kind.clone(),
                        label,
                        swr: tag("swr").and_then(share),
                        spend: spend.flatten(),
                        by,
                        account: tag("account").map(|a| a.trim().to_lowercase()),
                        target: target.flatten(),
                        span,
                    };
                    if goal.kind == GoalKind::Account
                        && (goal.account.is_none() || goal.target.is_none())
                    {
                        diag!(
                            "plan.unknown_directive",
                            "goal account 需要 account: 和 target:",
                        );
                    }
                    set.goals.push(goal);
                }
                "debt" => {
                    let label = words.collect::<Vec<_>>().join(" ");
                    let Some(account) = tag("account").map(|a| a.trim().to_lowercase()) else {
                        diag!("plan.unknown_directive", "debt 需要 account:");
                        continue;
                    };
                    let rate = match tag("rate").and_then(share) {
                        Some(r) if r > Decimal::ZERO => Some(r),
                        Some(_) => {
                            diag!("plan.bad_percent", "rate: 需要大于 0 的百分比");
                            None
                        }
                        None => None,
                    };
                    let payment = tag("payment")
                        .map(|v| parse_amount(v, &base))
                        .and_then(|a| a)
                        .map(|a| a.abs());
                    set.debts.push(DebtSpec {
                        label: if label.is_empty() {
                            short_label(&account)
                        } else {
                            label
                        },
                        account,
                        rate,
                        payment,
                        span,
                    });
                }
                other => diag!(
                    "plan.unknown_directive",
                    format!("未知指令 `{other}`（可用：assume / pool / alloc / goal / debt）"),
                ),
            }
        }
        // A second fire goal would shadow the first; keep order stable and warn.
        let fires = set
            .goals
            .iter()
            .filter(|g| g.kind == GoalKind::Fire)
            .count();
        if fires > 1 {
            set.diagnostics.push(Diagnostic::warning(
                "plan.duplicate_goal",
                format!("有 {fires} 条 goal fire，使用第一条"),
                file,
                set.goals
                    .iter()
                    .filter(|g| g.kind == GoalKind::Fire)
                    .nth(1)
                    .map(|g| g.span)
                    .unwrap_or_default(),
            ));
        }
        set
    }

    pub fn is_empty(&self) -> bool {
        self.src.trim().is_empty()
    }

    pub fn fire_goal(&self) -> Option<&Goal> {
        self.goals.iter().find(|g| g.kind == GoalKind::Fire)
    }

    /// Canonical text, used by the editor's “reset” and by tests.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "; MoneyTracker plan — read by the app, never included by index.ledger.\n",
        );
        let a = &self.assumptions;
        out.push_str(&format!(
            "assume  ; return:{} inflation:{} reserve:{}\n",
            percent(a.nominal_return),
            percent(a.inflation),
            a.reserve_months
        ));
        if !self.pool.is_empty() {
            out.push_str(&format!("pool  ; accounts:{}\n", self.pool.join(",")));
        }
        if !self.alloc.is_empty() {
            let alloc = self
                .alloc
                .iter()
                .map(|(k, v)| format!("{k}:{}", percent(*v)))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("alloc  ; {alloc}\n"));
        }
        for goal in &self.goals {
            let mut line = format!("goal {} {}", goal.kind.as_str(), goal.label);
            let mut tags = Vec::new();
            if let Some(swr) = goal.swr {
                tags.push(format!("swr:{}", percent(swr)));
            }
            if let Some(spend) = &goal.spend {
                tags.push(format!("spend:{}", format_amount(spend)));
            }
            if let Some(by) = goal.by {
                tags.push(format!("by:{by}"));
            }
            if let Some(account) = &goal.account {
                tags.push(format!("account:{account}"));
            }
            if let Some(target) = &goal.target {
                tags.push(format!("target:{}", format_amount(target)));
            }
            if !tags.is_empty() {
                line.push_str(&format!("  ; {}", tags.join(" ")));
            }
            out.push_str(&line);
            out.push('\n');
        }
        for debt in &self.debts {
            let mut tags = vec![format!("account:{}", debt.account)];
            if let Some(rate) = debt.rate {
                tags.push(format!("rate:{}", percent(rate)));
            }
            if let Some(payment) = &debt.payment {
                tags.push(format!("payment:{}", format_amount(payment)));
            }
            out.push_str(&format!("debt {}  ; {}\n", debt.label, tags.join(" ")));
        }
        out
    }
}

fn short_label(account: &str) -> String {
    account.rsplit(':').next().unwrap_or(account).to_string()
}

fn parse_amount(token: &str, base: &Assumptions) -> Option<Amount> {
    let _ = base;
    ledger_core::parse_amount(token).ok().map(|mut a| {
        if a.commodity.is_empty() {
            a.commodity = "CNY".into();
        }
        a
    })
}

fn format_amount(amount: &Amount) -> String {
    if amount.commodity.is_empty() {
        amount.quantity.to_string()
    } else {
        format!("{} {}", amount.quantity, amount.commodity)
    }
}

/// `6%` → `0.06`, `0.06` → `0.06`, `6` → `0.06`.
fn share(token: &str) -> Option<Decimal> {
    let token = token.trim();
    if let Some(pct) = token.strip_suffix('%') {
        let v: Decimal = parse_decimal(pct)?;
        return Some(v / Decimal::ONE_HUNDRED);
    }
    let v: Decimal = parse_decimal(token)?;
    if v > Decimal::ONE {
        Some(v / Decimal::ONE_HUNDRED)
    } else if v.is_sign_negative() {
        None
    } else {
        Some(v)
    }
}

fn percent(v: Decimal) -> String {
    format!("{}%", (v * Decimal::ONE_HUNDRED).round_dp(2).normalize())
}

// ---------------------------------------------------------------------------
// FIRE projection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FireTarget {
    pub swr: Decimal,
    pub target: Decimal,
    pub months: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FireProjection {
    pub annual_expense: Decimal,
    pub balance: Decimal,
    pub contribution: Decimal,
    pub real_return: Decimal,
    pub targets: Vec<FireTarget>,
    pub note: Option<String>,
}

/// Pure projection so the maths can be tested without a vault.
///
/// `balance·(1+m)^n + c·((1+m)^n − 1)/m ≥ target` with `m` the monthly real
/// return; `m == 0` degrades to a straight division. Money stays `Decimal`;
/// only the logarithm runs in `f64` (it produces a month count, not an amount).
pub fn fire_projection(
    annual_expense: Decimal,
    balance: Decimal,
    contribution: Decimal,
    nominal_return: Decimal,
    inflation: Decimal,
    primary_swr: Decimal,
) -> FireProjection {
    let real_return = if inflation.is_zero() {
        nominal_return
    } else {
        (Decimal::ONE + nominal_return) / (Decimal::ONE + inflation) - Decimal::ONE
    };
    let mut swrs = vec![
        primary_swr,
        Decimal::new(30, 3),
        Decimal::new(35, 3),
        Decimal::new(40, 3),
    ];
    swrs.dedup();
    let mut seen: Vec<Decimal> = Vec::new();
    let mut targets = Vec::new();
    for swr in swrs {
        if swr <= Decimal::ZERO || seen.contains(&swr) {
            continue;
        }
        seen.push(swr);
        let target = (annual_expense / swr).round_dp(2);
        targets.push(FireTarget {
            swr,
            target,
            months: months_to_target(target, balance, contribution, real_return),
        });
    }
    // Stable, ascending order so the UI can show 3 % / 3.5 % / 4 % consistently.
    targets.sort_by(|a, b| a.swr.cmp(&b.swr));
    let note = if annual_expense <= Decimal::ZERO {
        Some("账本里还没有支出记录，先记几个月再算目标".into())
    } else if contribution <= Decimal::ZERO && real_return <= Decimal::ZERO {
        Some("储蓄为负且实际收益不为正，无法推算达成时间".into())
    } else if targets.iter().all(|t| t.months.is_none()) {
        Some("按当前储蓄速度，100 年内无法达成".into())
    } else {
        None
    };
    FireProjection {
        annual_expense,
        balance,
        contribution,
        real_return,
        targets,
        note,
    }
}

fn months_to_target(
    target: Decimal,
    balance: Decimal,
    contribution: Decimal,
    real_return: Decimal,
) -> Option<i64> {
    if target <= balance {
        return Some(0);
    }
    if target <= Decimal::ZERO {
        return None;
    }
    let m = monthly_rate(real_return);
    if m.abs() < 1e-12 {
        if contribution <= Decimal::ZERO {
            return None;
        }
        let months = ((target - balance) / contribution).to_f64()?;
        return clamp_months(months.ceil() as i64);
    }
    // Solving for n needs logs; keep the money in Decimal and only round at the end.
    let n = if contribution.is_zero() {
        let ratio = (target / balance).to_f64()?;
        ratio.ln() / (1.0 + m).ln()
    } else {
        // Simulate: robust for negative-ish contributions and avoids domain errors.
        let mut value = balance.to_f64()?;
        let c = contribution.to_f64()?;
        let t = target.to_f64()?;
        let mut n = 0i64;
        while value < t && n < MAX_MONTHS {
            value = value * (1.0 + m) + c;
            n += 1;
        }
        if value < t {
            return None;
        }
        return Some(n);
    };
    clamp_months(n.ceil() as i64)
}

fn monthly_rate(real_return: Decimal) -> f64 {
    let r = real_return.to_f64().unwrap_or(0.0);
    (1.0 + r).powf(1.0 / 12.0) - 1.0
}

fn clamp_months(months: i64) -> Option<i64> {
    if !(0..=MAX_MONTHS).contains(&months) {
        None
    } else {
        Some(months)
    }
}

fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
    let total = date.year() as i64 * 12 + (date.month() as i64 - 1) + months;
    let year = total.div_euclid(12) as i32;
    let month = (total.rem_euclid(12) + 1) as u32;
    let last = days_in_month(year, month);
    NaiveDate::from_ymd_opt(year, month, date.day().min(last)).unwrap_or(date)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

// ---------------------------------------------------------------------------
// vault-derived figures
// ---------------------------------------------------------------------------

/// Base-commodity balance per account. Transactions with a missing FX rate are
/// skipped entirely, matching the dashboard: a partial conversion is a lie.
pub fn account_sums(vault: &Vault) -> BTreeMap<String, Decimal> {
    let mut sums: BTreeMap<String, Decimal> = BTreeMap::new();
    for rec in vault.transactions() {
        if rec.fx_missing {
            continue;
        }
        for value in &rec.values {
            let Some(base) = value.base else { continue };
            *sums.entry(value.account.clone()).or_insert(Decimal::ZERO) += base;
        }
    }
    sums
}

fn subtree_sum(sums: &BTreeMap<String, Decimal>, account: &str) -> Decimal {
    let prefix = format!("{account}:");
    sums.iter()
        .filter(|(k, _)| *k == account || k.starts_with(&prefix))
        .map(|(_, v)| *v)
        .sum()
}

/// Debit-positive balance (liabilities are stored negative, so negate them).
fn signed_balance(sums: &BTreeMap<String, Decimal>, account: &str, kind: char) -> Decimal {
    let sum = subtree_sum(sums, account);
    match kind {
        'L' | 'E' | 'R' => -sum,
        _ => sum,
    }
}

fn trailing_expense(vault: &Vault, months: usize) -> Decimal {
    let totals = monthly_totals(vault);
    let end = last_month(vault);
    month_series(&end, months)
        .iter()
        .map(|key| totals.get(key).map(|t| t.expense).unwrap_or_default())
        .sum()
}

fn trailing_contribution(vault: &Vault, months: usize) -> Decimal {
    let totals = monthly_totals(vault);
    let end = last_month(vault);
    let series = month_series(&end, months);
    if series.is_empty() {
        return Decimal::ZERO;
    }
    let net: Decimal = series
        .iter()
        .map(|key| {
            totals
                .get(key)
                .map(|t: &MonthTotals| t.income - t.expense)
                .unwrap_or_default()
        })
        .sum();
    net / Decimal::from(series.len() as u64)
}

/// Pool accounts: explicit `pool ;` wins, otherwise every asset that does not
/// look like a home or a car.
fn pool_accounts(vault: &Vault, plan: &PlanSet) -> Vec<String> {
    if !plan.pool.is_empty() {
        return plan.pool.clone();
    }
    const EXCLUDED: [&str; 6] = ["房", "home", "house", "property", "车", "car"];
    vault
        .accounts()
        .iter()
        .filter(|(name, info)| info.kind == 'A' && !EXCLUDED.iter().any(|word| name.contains(word)))
        .map(|(name, _)| name.clone())
        .collect()
}

fn cash_accounts(vault: &Vault, pool: &[String]) -> Vec<String> {
    const CASH_WORDS: [&str; 8] = [
        "cash", "现金", "wallet", "钱包", "bank", "储蓄", "saving", "checking",
    ];
    let any_tagged_cash = pool.iter().any(|a| vault.class_of(a) == Some("cash"));
    pool.iter()
        .filter(|account| match vault.class_of(account) {
            Some("cash") => true,
            Some(_) => false,
            None => {
                !any_tagged_cash
                    && CASH_WORDS
                        .iter()
                        .any(|word| account.to_lowercase().contains(word))
            }
        })
        .cloned()
        .collect()
}

fn periodic_payment(vault: &Vault, account: &str) -> Option<Decimal> {
    let mut best: Option<Decimal> = None;
    for file in vault.files() {
        // Declared templates carry the gross payment (principal only on the debt
        // leg), so they would skew a payoff estimate; read hand-written budgets.
        if crate::schedule::is_schedule_file(&file.rel) {
            continue;
        }
        for item in &file.journal.items {
            let Item::Periodic(p) = item else { continue };
            let factor = match p.period.split_whitespace().next().unwrap_or("monthly") {
                "weekly" => Decimal::from(52) / Decimal::from(12),
                "quarterly" => Decimal::ONE / Decimal::from(3),
                "yearly" | "annually" => Decimal::ONE / Decimal::from(12),
                _ => Decimal::ONE,
            };
            for posting in &p.postings {
                let name = posting.account.to_lowercase();
                if name != account && !name.starts_with(&format!("{account}:")) {
                    continue;
                }
                if let Some(amount) = posting.amount.as_ref() {
                    let monthly = amount.quantity.abs() * factor;
                    best = Some(best.map(|b: Decimal| b.max(monthly)).unwrap_or(monthly));
                }
            }
        }
    }
    best
}

fn interest_paid(vault: &Vault, account: &str) -> Decimal {
    let mut total = Decimal::ZERO;
    for rec in vault.transactions() {
        if rec.fx_missing {
            continue;
        }
        let touches_debt = rec
            .values
            .iter()
            .any(|v| v.account == account || v.account.starts_with(&format!("{account}:")));
        if !touches_debt {
            continue;
        }
        for value in &rec.values {
            if value.kind == 'X' {
                if let Some(base) = value.base {
                    total += base;
                }
            }
        }
    }
    total
}

fn payoff_months(balance: Decimal, monthly_payment: Decimal, monthly_rate: Decimal) -> Option<i64> {
    if balance <= Decimal::ZERO || monthly_payment <= Decimal::ZERO {
        return None;
    }
    if monthly_payment <= balance * monthly_rate {
        return None;
    }
    if monthly_rate.is_zero() {
        let n = (balance / monthly_payment).to_f64()?.ceil() as i64;
        return clamp_months(n);
    }
    let b = balance.to_f64()?;
    let p = monthly_payment.to_f64()?;
    let r = monthly_rate.to_f64()?;
    let n = (p / (p - b * r)).ln() / (1.0 + r).ln();
    clamp_months(n.ceil() as i64)
}

fn interest_over(
    balance: Decimal,
    payment: Decimal,
    monthly_rate: Decimal,
    months: i64,
) -> Decimal {
    let mut remaining = balance;
    let mut interest = Decimal::ZERO;
    for _ in 0..months {
        let i = remaining * monthly_rate;
        interest += i;
        let principal = payment - i;
        if principal <= Decimal::ZERO {
            break;
        }
        remaining -= principal;
        if remaining <= Decimal::ZERO {
            break;
        }
    }
    interest
}

// ---------------------------------------------------------------------------
// assembly
// ---------------------------------------------------------------------------

pub fn plan_text(vault: &Vault) -> String {
    std::fs::read_to_string(vault.root().join(GOALS_FILE)).unwrap_or_default()
}

fn fmt_money(vault: &Vault, quantity: Decimal) -> crate::types::AmountDto {
    let (commodity, precision) = vault.base_commodity();
    decimal_to_dto(quantity, commodity, precision)
}

pub fn overview(vault: &Vault) -> PlanOverviewDto {
    let text = plan_text(vault);
    let plan = PlanSet::parse(GOALS_FILE, &text);
    let sums = account_sums(vault);
    let pool = pool_accounts(vault, &plan);

    let pool_members: Vec<PoolMemberDto> = pool
        .iter()
        .map(|account| {
            let amount = signed_balance(&sums, account, 'A');
            PoolMemberDto {
                account: account.clone(),
                class: vault.class_of(account).map(|c| c.to_string()),
                amount: fmt_money(vault, amount),
            }
        })
        .collect();
    let balance: Decimal = pool.iter().map(|a| signed_balance(&sums, a, 'A')).sum();

    let fire = plan.fire_goal();
    let annual_expense = match fire.and_then(|g| g.spend.as_ref()) {
        Some(amount) => amount.quantity,
        None => trailing_expense(vault, 12),
    };
    let contribution = trailing_contribution(vault, 6).max(Decimal::ZERO);
    let primary_swr = fire
        .and_then(|g| g.swr)
        .filter(|s| *s > Decimal::ZERO)
        .unwrap_or(Decimal::new(40, 3));
    let projection = fire_projection(
        annual_expense,
        balance,
        contribution,
        plan.assumptions.nominal_return,
        plan.assumptions.inflation,
        primary_swr,
    );
    let today = vault
        .last_date
        .unwrap_or_else(|| chrono::Local::now().date_naive());

    let primary_target = projection
        .targets
        .iter()
        .find(|t| t.swr == primary_swr)
        .or_else(|| projection.targets.first());
    let fire_dto = FireDto {
        annual_expense: fmt_money(vault, annual_expense),
        balance: fmt_money(vault, balance),
        contribution: fmt_money(vault, contribution),
        real_return: f64_of(projection.real_return * Decimal::ONE_HUNDRED),
        primary_swr: f64_of(primary_swr * Decimal::ONE_HUNDRED),
        progress: primary_target
            .filter(|t| t.target > Decimal::ZERO)
            .map(|t| f64_of((balance / t.target * Decimal::ONE_HUNDRED).min(Decimal::ONE_HUNDRED)))
            .unwrap_or(0.0),
        targets: projection
            .targets
            .iter()
            .map(|t| FireTargetDto {
                swr: f64_of(t.swr * Decimal::ONE_HUNDRED),
                target: fmt_money(vault, t.target),
                eta_months: t.months.map(|m| m as f64),
                eta_date: t.months.map(|m| add_months(today, m).to_string()),
                eta_label: match t.months {
                    Some(0) => "已达成".to_string(),
                    Some(m) => format!("{m} 个月后 · {}", add_months(today, m)),
                    None => "> 100 年".to_string(),
                },
                achieved: t.months == Some(0),
            })
            .collect(),
        note: projection.note,
    };

    let cash: Decimal = cash_accounts(vault, &pool)
        .iter()
        .map(|a| signed_balance(&sums, a, 'A'))
        .sum();
    let monthly_expense = annual_expense / Decimal::from(12);
    let reserve = ReserveDto {
        cash: fmt_money(vault, cash),
        monthly_expense: fmt_money(vault, monthly_expense),
        months: if monthly_expense > Decimal::ZERO {
            f64_of(cash / monthly_expense)
        } else {
            0.0
        },
        target_months: plan.assumptions.reserve_months,
    };

    let goals: Vec<GoalDto> = plan
        .goals
        .iter()
        .filter(|g| g.kind == GoalKind::Account)
        .map(|g| {
            let account = g.account.clone().unwrap_or_default();
            let target = g.target.as_ref().map(|a| a.quantity).unwrap_or_default();
            let current = signed_balance(&sums, &account, vault.kind_for(&account));
            GoalDto {
                label: g.label.clone(),
                account: Some(account),
                target: fmt_money(vault, target),
                current: fmt_money(vault, current),
                progress: if target > Decimal::ZERO {
                    f64_of(
                        (current / target * Decimal::ONE_HUNDRED)
                            .clamp(Decimal::ZERO, Decimal::ONE_HUNDRED),
                    )
                } else {
                    0.0
                },
                by: g.by.map(|d| d.to_string()),
                months_left: g.by.map(|d| months_between(today, d) as f64),
            }
        })
        .collect();

    let debts = debt_rows(vault, &plan, &sums, today);
    let allocation = allocation_rows(vault, &plan, &sums, &pool);

    PlanOverviewDto {
        has_goals: !plan.is_empty(),
        goals_text: text,
        assumptions: AssumptionsDto {
            nominal_return: f64_of(plan.assumptions.nominal_return * Decimal::ONE_HUNDRED),
            inflation: f64_of(plan.assumptions.inflation * Decimal::ONE_HUNDRED),
            reserve_months: plan.assumptions.reserve_months,
        },
        fire: fire_dto,
        reserve,
        pool: pool_members,
        goals,
        debts,
        allocation,
        diagnostics: plan
            .diagnostics
            .iter()
            .map(crate::load::diagnostic_dto)
            .collect::<Vec<DiagnosticDto>>(),
    }
}

fn months_between(from: NaiveDate, to: NaiveDate) -> i64 {
    (to.year() as i64 * 12 + to.month() as i64) - (from.year() as i64 * 12 + from.month() as i64)
}

fn debt_rows(
    vault: &Vault,
    plan: &PlanSet,
    sums: &BTreeMap<String, Decimal>,
    today: NaiveDate,
) -> Vec<DebtDto> {
    let mut specs = plan.debts.clone();
    if specs.is_empty() {
        specs = vault
            .accounts()
            .iter()
            .filter(|(_, info)| info.kind == 'L')
            .filter(|(name, _)| subtree_sum(sums, name) != Decimal::ZERO)
            .map(|(name, _)| DebtSpec {
                label: short_label(name),
                account: name.clone(),
                rate: None,
                payment: None,
                span: Span::default(),
            })
            .collect();
    }
    specs
        .iter()
        .map(|spec| {
            let balance = signed_balance(sums, &spec.account, 'L');
            let payment = spec
                .payment
                .as_ref()
                .map(|a| a.quantity)
                .or_else(|| periodic_payment(vault, &spec.account));
            let monthly_interest = spec.rate.map(|r| balance * r / Decimal::from(12));
            let months = match (spec.rate, payment) {
                (Some(rate), Some(payment)) => {
                    payoff_months(balance, payment, rate / Decimal::from(12))
                }
                _ => None,
            };
            let extra = match (spec.rate, payment, months) {
                (Some(rate), Some(payment), Some(base_months)) => {
                    let monthly = rate / Decimal::from(12);
                    let base_interest = interest_over(balance, payment, monthly, base_months);
                    EXTRA_PAYMENTS
                        .iter()
                        .filter_map(|extra| {
                            let bumped = payment + Decimal::from(*extra);
                            let faster = payoff_months(balance, bumped, monthly)?;
                            let interest = interest_over(balance, bumped, monthly, faster);
                            Some(ExtraPaymentDto {
                                extra: fmt_money(vault, Decimal::from(*extra)),
                                months_saved: (base_months - faster) as f64,
                                interest_saved: fmt_money(
                                    vault,
                                    (base_interest - interest).max(Decimal::ZERO),
                                ),
                            })
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            let note = if spec.rate.is_none() {
                Some("在 goals.ledger 里补 rate: 才能推算还清时间".to_string())
            } else if let (Some(rate), Some(payment)) = (spec.rate, payment) {
                if payment <= balance * rate / Decimal::from(12) {
                    Some("月供不足以覆盖利息，贷款永不到期".to_string())
                } else {
                    None
                }
            } else {
                Some(
                    "没有月供信息：可以在 debt 行写 payment:，或加一条 ~ monthly 定期交易"
                        .to_string(),
                )
            };
            DebtDto {
                label: spec.label.clone(),
                account: spec.account.clone(),
                balance: fmt_money(vault, balance),
                rate: spec.rate.map(|r| f64_of(r * Decimal::ONE_HUNDRED)),
                payment: payment.map(|p| fmt_money(vault, p)),
                monthly_interest: monthly_interest.map(|i| fmt_money(vault, i)),
                interest_paid: fmt_money(vault, interest_paid(vault, &spec.account)),
                payoff_months: months.map(|m| m as f64),
                payoff_date: months.map(|m| add_months(today, m).to_string()),
                note,
                extra,
            }
        })
        .collect()
}

fn allocation_rows(
    vault: &Vault,
    plan: &PlanSet,
    sums: &BTreeMap<String, Decimal>,
    pool: &[String],
) -> AllocationDto {
    let mut by_class: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut untagged = Vec::new();
    for account in pool {
        let class = vault.class_of(account);
        if class.is_none() {
            untagged.push(account.clone());
        }
        let key = class.unwrap_or("other").to_string();
        *by_class.entry(key).or_insert(Decimal::ZERO) += signed_balance(sums, account, 'A');
    }
    let total: Decimal = by_class.values().sum();
    let rows = by_class
        .into_iter()
        .map(|(class, amount)| {
            let current = if total.is_zero() {
                Decimal::ZERO
            } else {
                amount / total
            };
            let target = plan
                .alloc
                .iter()
                .find(|(k, _)| *k == class)
                .map(|(_, v)| *v);
            AllocationRowDto {
                class: class.clone(),
                current: fmt_money(vault, amount),
                current_share: f64_of(current * Decimal::ONE_HUNDRED),
                target_share: target.map(|t| f64_of(t * Decimal::ONE_HUNDRED)),
                drift: target.map(|t| f64_of((current - t) * Decimal::ONE_HUNDRED)),
            }
        })
        .collect();
    AllocationDto { rows, untagged }
}

// ---------------------------------------------------------------------------
// markdown report
// ---------------------------------------------------------------------------

pub fn month_report(vault: &Vault, month: &str) -> String {
    let totals = monthly_totals(vault);
    let entry = totals.get(month).copied().unwrap_or_default();
    let overview = crate::report::dashboard(vault, 12);
    let mut out = format!("# MoneyTracker 月报 {month}\n\n");
    out.push_str(&format!(
        "- 收入：{}\n- 支出：{}\n- 结余：{}\n- 净资产：{}\n",
        fmt_money(vault, entry.income).display,
        fmt_money(vault, entry.expense).display,
        fmt_money(vault, entry.income - entry.expense).display,
        fmt_money(vault, entry.net_worth).display,
    ));
    if !overview.categories.is_empty() {
        out.push_str("\n## 支出分类\n\n| 分类 | 金额 | 占比 |\n| --- | --- | --- |\n");
        for row in overview.categories.iter().take(10) {
            out.push_str(&format!(
                "| {} | {} | {:.1}% |\n",
                row.label, row.display, row.share
            ));
        }
    }
    if !overview.budget.is_empty() {
        out.push_str("\n## 预算执行\n\n| 信封 | 预算 | 实际 | 剩余 |\n| --- | --- | --- | --- |\n");
        for row in &overview.budget {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                row.label, row.budget_display, row.actual_display, row.remaining_display
            ));
        }
    }
    if overview.kpis.missing_fx_count > 0 {
        out.push_str(&format!(
            "\n> {} 笔交易缺少汇率，未计入合计。\n",
            overview.kpis.missing_fx_count
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_core::Severity;

    fn plan(src: &str) -> PlanSet {
        PlanSet::parse(GOALS_FILE, src)
    }

    #[test]
    fn parses_the_documented_grammar() {
        let set = plan(
            "; comment\n\
             assume  ; return:7% inflation:3% reserve:9\n\
             pool    ; accounts:assets:invest,assets:cash\n\
             alloc   ; equity:60% bond:20% cash:20%\n\
             goal fire 财富自由 ; swr:3.5% spend:12000.00 CNY by:2045-01-01\n\
             goal account 退休金 ; account:assets:invest:retirement target:1000000.00 CNY\n\
             debt 房贷 ; account:liabilities:mortgage rate:3.15% payment:8200.00 CNY\n",
        );
        assert!(set.diagnostics.is_empty(), "{:?}", set.diagnostics);
        assert_eq!(set.assumptions.nominal_return, Decimal::new(7, 2));
        assert_eq!(set.assumptions.inflation, Decimal::new(3, 2));
        assert_eq!(set.assumptions.reserve_months, 9);
        assert_eq!(set.pool, vec!["assets:invest", "assets:cash"]);
        assert_eq!(set.alloc[0], ("equity".to_string(), Decimal::new(60, 2)));
        let fire = set.fire_goal().unwrap();
        assert_eq!(fire.swr, Some(Decimal::new(35, 3)));
        assert_eq!(
            fire.spend.as_ref().unwrap().quantity,
            Decimal::new(12000, 0)
        );
        assert_eq!(fire.by.unwrap().to_string(), "2045-01-01");
        assert_eq!(set.goals.len(), 2);
        assert_eq!(set.debts[0].rate, Some(Decimal::new(315, 4)));
        assert_eq!(
            set.debts[0].payment.as_ref().unwrap().quantity,
            Decimal::new(8200, 0)
        );
    }

    #[test]
    fn bad_lines_become_diagnostics_not_failures() {
        let set = plan("nonsense here\nassume ; return:abc\ndebt 房贷 ; rate:3%\n");
        let codes: Vec<&str> = set.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"plan.unknown_directive"));
        assert!(codes.contains(&"plan.bad_percent"));
        assert!(set
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn duplicate_fire_goals_warn() {
        let set = plan("goal fire a\ngoal fire b\n");
        assert!(set
            .diagnostics
            .iter()
            .any(|d| d.code == "plan.duplicate_goal" && d.severity == Severity::Warning));
    }

    #[test]
    fn render_round_trips() {
        let src = "assume  ; return:6% inflation:2.5% reserve:6\n\
                   pool  ; accounts:assets:invest,assets:cash\n\
                   alloc  ; equity:60% cash:40%\n\
                   goal fire 财富自由 ; swr:4% by:2045-01-01\n\
                   goal account 退休金 ; account:assets:invest:retirement target:1000000.00 CNY\n\
                   debt 房贷 ; account:liabilities:mortgage rate:3.15% payment:8200.00 CNY\n";
        let first = plan(src);
        let again = plan(&first.render());
        assert!(again.diagnostics.is_empty(), "{:?}", again.diagnostics);
        assert_eq!(first.assumptions, again.assumptions);
        assert_eq!(first.pool, again.pool);
        let debts = |set: &PlanSet| {
            set.debts
                .iter()
                .map(|d| {
                    (
                        d.label.clone(),
                        d.account.clone(),
                        d.rate,
                        d.payment.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(debts(&first), debts(&again));
        // Spans legitimately move when the text is re-rendered; compare the model.
        let strip = |set: &PlanSet| {
            set.goals
                .iter()
                .map(|g| {
                    (
                        g.kind.clone(),
                        g.label.clone(),
                        g.swr,
                        g.spend.clone(),
                        g.by,
                        g.account.clone(),
                        g.target.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(&first), strip(&again));
        assert_eq!(first.render(), again.render());
    }

    #[test]
    fn fire_targets_are_monotonic_in_swr() {
        let p = fire_projection(
            Decimal::new(120_000, 0),
            Decimal::new(500_000, 0),
            Decimal::new(8_000, 0),
            Decimal::new(6, 2),
            Decimal::new(25, 3),
            Decimal::new(40, 3),
        );
        assert_eq!(p.targets.len(), 3);
        // A lower withdrawal rate needs more money and therefore more time.
        assert!(p.targets[0].target > p.targets[1].target);
        assert!(p.targets[1].target > p.targets[2].target);
        let months: Vec<i64> = p.targets.iter().map(|t| t.months.unwrap()).collect();
        assert!(months[0] >= months[1] && months[1] >= months[2]);
    }

    #[test]
    fn fire_is_achieved_when_the_pool_already_covers_the_target() {
        let p = fire_projection(
            Decimal::new(60_000, 0),
            Decimal::new(5_000_000, 0),
            Decimal::ZERO,
            Decimal::new(6, 2),
            Decimal::new(25, 3),
            Decimal::new(40, 3),
        );
        assert!(p.targets.iter().all(|t| t.months == Some(0)));
        assert!(p.note.is_none());
    }

    #[test]
    fn zero_return_degenerates_to_a_plain_division() {
        let p = fire_projection(
            Decimal::new(120_000, 0),
            Decimal::ZERO,
            Decimal::new(10_000, 0),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::new(40, 3),
        );
        // 4 % of 120,000 = 3,000,000; 3,000,000 / 10,000 = 300 months
        let four = p
            .targets
            .iter()
            .find(|t| t.swr == Decimal::new(40, 3))
            .expect("4% tier present");
        assert_eq!(four.months, Some(300));
        assert_eq!(p.real_return, Decimal::ZERO);
    }

    #[test]
    fn negative_savings_without_returns_has_no_eta() {
        let p = fire_projection(
            Decimal::new(120_000, 0),
            Decimal::new(10_000, 0),
            Decimal::new(-500, 0),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::new(40, 3),
        );
        assert!(p.targets.iter().all(|t| t.months.is_none()));
        assert!(p.note.is_some());
    }

    #[test]
    fn no_expenses_means_no_target() {
        let p = fire_projection(
            Decimal::ZERO,
            Decimal::new(1_000, 0),
            Decimal::new(1_000, 0),
            Decimal::new(6, 2),
            Decimal::new(25, 3),
            Decimal::new(40, 3),
        );
        assert!(p.note.unwrap().contains("还没有支出"));
    }

    #[test]
    fn payoff_and_extra_payments() {
        // 1,000,000 at 4.2%/yr paying 8,200/month.
        let rate = Decimal::new(42, 4) / Decimal::from(12);
        let months =
            payoff_months(Decimal::new(1_000_000, 0), Decimal::new(8_200, 0), rate).unwrap();
        assert!(months > 100 && months < 260, "{months}");
        let faster =
            payoff_months(Decimal::new(1_000_000, 0), Decimal::new(9_200, 0), rate).unwrap();
        assert!(faster < months);
        let interest = interest_over(
            Decimal::new(1_000_000, 0),
            Decimal::new(8_200, 0),
            rate,
            months,
        );
        assert!(interest > Decimal::ZERO);
        // Payment below the interest can never amortise.
        assert_eq!(
            payoff_months(Decimal::new(1_000_000, 0), Decimal::new(1_000, 0), rate),
            None
        );
    }

    #[test]
    fn month_arithmetic_clamps_to_month_end() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(add_months(d, 1).to_string(), "2026-02-28");
        assert_eq!(add_months(d, 13).to_string(), "2027-02-28");
        assert_eq!(add_months(d, 0).to_string(), "2026-01-31");
    }

    #[test]
    fn shares_accept_percent_decimals_and_fractions() {
        assert_eq!(share("6%"), Some(Decimal::new(6, 2)));
        assert_eq!(share("0.035"), Some(Decimal::new(35, 3)));
        assert_eq!(share("3.15"), Some(Decimal::new(315, 4)));
        assert_eq!(share("abc"), None);
    }
}

//! Structured, deterministic analysis: insight bullets for the dashboard and the
//! fact table that answers a natural-language question.
//!
//! Nothing here talks to a model. The model only turns a sentence into a
//! [`QueryPlan`]; this module validates that plan against the real ledger and
//! computes the numbers, so an answer can never be invented.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::load::Vault;
use crate::report::{budget_rows, last_month, month_key, month_shift, monthly_totals};
use crate::types::{AmountDto, FactDto, InsightDto, PlanQueryDto, PlanResultDto};

fn money(vault: &Vault, quantity: Decimal) -> AmountDto {
    let (commodity, precision) = vault.base_commodity();
    crate::amounts::decimal_to_dto(quantity, commodity, precision)
}

fn f64_of(value: Decimal) -> f64 {
    crate::amounts::f64_of(value)
}

fn top_level(account: &str) -> String {
    account.split(':').take(2).collect::<Vec<_>>().join(":")
}

/// Dated, FX-complete expense totals per month for the last `months` months.
fn expense_months(vault: &Vault, months: usize) -> Vec<(String, Decimal)> {
    let totals = monthly_totals(vault);
    let end = last_month(vault);
    let series = crate::report::month_series(&end, months);
    series
        .into_iter()
        .map(|month| {
            let expense = totals.get(&month).map(|t| t.expense).unwrap_or_default();
            (month, expense)
        })
        .collect()
}

/// Category spending for one month, keyed by the two-segment label.
fn category_spend(vault: &Vault, month: &str) -> BTreeMap<String, Decimal> {
    let mut map: BTreeMap<String, Decimal> = BTreeMap::new();
    for rec in vault.transactions() {
        if month_key(rec.txn.date) != month || rec.fx_missing {
            continue;
        }
        for value in &rec.values {
            if value.kind != 'X' {
                continue;
            }
            let Some(base) = value.base else { continue };
            *map.entry(top_level(&value.account)).or_default() += base;
        }
    }
    map
}

/// The dashboard's "what changed" list. Ordered most-urgent first.
pub fn insights(vault: &Vault) -> Vec<InsightDto> {
    let mut out: Vec<InsightDto> = Vec::new();
    let month = last_month(vault);
    let previous = month_shift(&month, -1);

    // 1. budgets that are over or nearly over
    for row in budget_rows(vault, &month) {
        if row.ratio >= 1.0 {
            out.push(InsightDto {
                kind: "budget_over".into(),
                tone: "warn".into(),
                title: format!("{} 超预算", row.label),
                detail: format!(
                    "{} / {}（{:.0}%）",
                    row.actual_display,
                    row.budget_display,
                    row.ratio * 100.0
                ),
                amount: None,
            });
        } else if row.ratio >= 0.9 {
            out.push(InsightDto {
                kind: "budget_near".into(),
                tone: "warn".into(),
                title: format!("{} 预算快用完", row.label),
                detail: format!(
                    "{} / {}（{:.0}%）",
                    row.actual_display,
                    row.budget_display,
                    row.ratio * 100.0
                ),
                amount: None,
            });
        }
    }

    // 2. the two biggest category moves against last month
    let now = category_spend(vault, &month);
    let before = category_spend(vault, &previous);
    let mut moves: Vec<(String, Decimal, Decimal)> = Vec::new();
    for (label, current) in &now {
        let previous_value = before.get(label).copied().unwrap_or_default();
        if previous_value.is_zero() || current.is_zero() {
            continue;
        }
        let change = (*current - previous_value) / previous_value;
        if change.abs() >= Decimal::new(15, 2) {
            moves.push((label.clone(), *current, change));
        }
    }
    moves.sort_by(|a, b| b.1.cmp(&a.1));
    for (label, current, change) in moves.into_iter().take(2) {
        let up = change.is_sign_positive();
        out.push(InsightDto {
            kind: if up {
                "spend_up".into()
            } else {
                "spend_down".into()
            },
            tone: if up { "warn".into() } else { "good".into() },
            title: format!(
                "{} 比上月{}了 {:.0}%",
                label,
                if up { "多" } else { "少" },
                change.abs() * Decimal::from(100)
            ),
            detail: format!(
                "本月 {}，上月 {}",
                money(vault, current).display,
                money(vault, current / (Decimal::ONE + change)).display
            ),
            amount: Some(money(vault, current)),
        });
    }

    // 3. one unusually large expense in the current month
    let history: Vec<(String, Decimal)> = expense_months(vault, 7);
    let average = if history.len() > 1 {
        let sum: Decimal = history[..history.len() - 1].iter().map(|(_, v)| *v).sum();
        sum / Decimal::from(history.len() - 1)
    } else {
        Decimal::ZERO
    };
    let mut biggest: Option<(String, Decimal)> = None;
    for rec in vault.transactions() {
        if month_key(rec.txn.date) != month || rec.fx_missing {
            continue;
        }
        let spend: Decimal = rec
            .values
            .iter()
            .filter(|v| v.kind == 'X')
            .filter_map(|v| v.base)
            .sum();
        if spend.is_zero() {
            continue;
        }
        if biggest
            .as_ref()
            .map(|(_, best)| spend > *best)
            .unwrap_or(true)
        {
            biggest = Some((rec.txn.payee.clone(), spend));
        }
    }
    if let Some((payee, spend)) = biggest {
        if !average.is_zero() && spend >= average * Decimal::from(3) && spend >= Decimal::from(500)
        {
            out.push(InsightDto {
                kind: "unusual_expense".into(),
                tone: "info".into(),
                title: format!("本月有一笔较大的支出：{payee}"),
                detail: format!(
                    "{}，约为此前六个月月均支出的 {:.1} 倍",
                    money(vault, spend).display,
                    f64_of(spend / average)
                ),
                amount: Some(money(vault, spend)),
            });
        }
    }

    // 4. savings rate
    let totals = monthly_totals(vault);
    if let Some(entry) = totals.get(&month) {
        if !entry.income.is_zero() {
            let rate = (entry.income - entry.expense) / entry.income;
            let percent = f64_of(rate * Decimal::from(100));
            if rate < Decimal::new(20, 2) {
                out.push(InsightDto {
                    kind: "savings_low".into(),
                    tone: "warn".into(),
                    title: format!("本月储蓄率 {percent:.0}%"),
                    detail: "低于 20% 的常见目标，可以检查预算里超支的分类".into(),
                    amount: None,
                });
            } else if rate >= Decimal::new(40, 2) {
                out.push(InsightDto {
                    kind: "savings_good".into(),
                    tone: "good".into(),
                    title: format!("本月储蓄率 {percent:.0}%"),
                    detail: "继续保持，按这个节奏 FIRE 计划会提前".into(),
                    amount: None,
                });
            }
        }
    }

    // 5. net worth at a new high
    if let Some(entry) = totals.get(&month) {
        let peak = totals
            .values()
            .map(|t| t.net_worth)
            .max()
            .unwrap_or_default();
        if entry.net_worth >= peak && entry.net_worth > Decimal::ZERO {
            out.push(InsightDto {
                kind: "net_worth_high".into(),
                tone: "good".into(),
                title: "净资产创新高".into(),
                detail: format!("当前 {}", money(vault, entry.net_worth).display),
                amount: Some(money(vault, entry.net_worth)),
            });
        }
    }

    // 6. missing FX rates
    let missing = vault.missing_fx_count();
    if missing > 0 {
        out.push(InsightDto {
            kind: "fx_missing".into(),
            tone: "warn".into(),
            title: format!("{missing} 笔交易缺少汇率"),
            detail: "它们没有计入合计，补齐 prices.ledger 后数字会变".into(),
            amount: None,
        });
    }

    out.truncate(8);
    out
}

// ---------------------------------------------------------------------------
// Question answering
// ---------------------------------------------------------------------------

/// A validated, machine-checked interpretation of a question.
#[derive(Debug, Clone, Default)]
pub struct QueryPlan {
    /// `spend` | `income` | `net` | `balance` | `count`
    pub metric: String,
    pub accounts: Vec<String>,
    pub tags: Vec<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    /// `month` | `account` | `tag`
    pub group_by: Option<String>,
    pub limit: usize,
    pub min_amount: Option<Decimal>,
}

impl QueryPlan {
    pub fn default_metric(&self) -> &str {
        if self.metric.is_empty() {
            "spend"
        } else {
            &self.metric
        }
    }
}

fn parse_bound(raw: &str, end: bool) -> Option<NaiveDate> {
    let trimmed = raw.trim();
    if trimmed.len() == 7 {
        let month = NaiveDate::parse_from_str(&format!("{trimmed}-01"), "%Y-%m-%d").ok()?;
        if end {
            let next = month_shift(trimmed, 1);
            return NaiveDate::parse_from_str(&format!("{next}-01"), "%Y-%m-%d").ok();
        }
        return Some(month);
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").ok()
}

/// Run a validated plan against the ledger. This is the only place an answer's
/// numbers come from.
pub fn run_plan(vault: &Vault, plan: &QueryPlan) -> PlanResultDto {
    let metric = plan.default_metric().to_string();
    let from = plan
        .date_from
        .as_deref()
        .and_then(|d| parse_bound(d, false));
    let to = plan.date_to.as_deref().and_then(|d| parse_bound(d, true));
    let accounts = &plan.accounts;
    let tags = &plan.tags;
    let limit = plan.limit.clamp(1, 50);

    let mut buckets: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut matched = 0usize;
    let mut total = Decimal::ZERO;

    for rec in vault.transactions() {
        if let Some(start) = from {
            if rec.txn.date < start {
                continue;
            }
        }
        if let Some(end) = to {
            if rec.txn.date >= end {
                continue;
            }
        }
        if !accounts.is_empty()
            && !rec.values.iter().any(|v| {
                accounts.iter().any(|wanted| {
                    v.account == *wanted || v.account.starts_with(&format!("{wanted}:"))
                })
            })
        {
            continue;
        }
        if !tags.is_empty()
            && !rec.txn.tags.iter().any(|tag| {
                tags.iter().any(|wanted| {
                    tag.key == *wanted || tag.value.as_deref() == Some(wanted.as_str())
                })
            })
        {
            continue;
        }

        let mut amount = Decimal::ZERO;
        for value in &rec.values {
            let Some(base) = value.base else { continue };
            if rec.fx_missing {
                continue;
            }
            match metric.as_str() {
                "income" if value.kind == 'R' => amount += -base,
                "spend" if value.kind == 'X' => amount += base,
                "net" if value.kind == 'R' => amount += -base,
                "net" if value.kind == 'X' => amount -= base,
                "balance" if matches!(value.kind, 'A' | 'L') => amount += base,
                _ => {}
            }
        }
        if metric == "count" {
            amount = Decimal::ONE;
        }
        if let Some(floor) = plan.min_amount {
            if amount.abs() < floor {
                continue;
            }
        }
        if amount.is_zero() && metric != "count" {
            continue;
        }

        matched += 1;
        total += amount;
        let key = match plan.group_by.as_deref() {
            Some("account") => rec
                .values
                .iter()
                .filter(|v| match metric.as_str() {
                    "income" => v.kind == 'R',
                    "balance" => matches!(v.kind, 'A' | 'L'),
                    _ => v.kind == 'X',
                })
                .map(|v| top_level(&v.account))
                .next()
                .unwrap_or_else(|| "其他".to_string()),
            Some("tag") => rec
                .txn
                .tags
                .first()
                .map(|t| t.key.clone())
                .unwrap_or_else(|| "无标签".to_string()),
            _ => month_key(rec.txn.date),
        };
        *buckets.entry(key).or_default() += amount;
    }

    let mut rows: Vec<(String, Decimal)> = buckets.into_iter().collect();
    rows.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()));
    rows.truncate(limit);

    let facts: Vec<FactDto> = rows
        .iter()
        .map(|(label, value)| {
            let money = money(vault, *value);
            FactDto {
                label: label.clone(),
                value: f64_of(*value),
                display: money.display,
            }
        })
        .collect();

    let label = match metric.as_str() {
        "income" => "收入",
        "net" => "净结余",
        "balance" => "账户余额",
        "count" => "交易笔数",
        _ => "支出",
    };
    let total_amount = if metric == "count" {
        AmountDto {
            quantity: total.to_string(),
            value: f64_of(total),
            commodity: String::new(),
            display: format!("{total} 笔"),
        }
    } else {
        money(vault, total)
    };

    let summary = if facts.is_empty() {
        format!("没有找到符合条件的{label}记录")
    } else if plan.group_by.is_some() {
        format!(
            "{label}合计 {}，按{}分成 {} 组，最高的是 {} {}",
            total_amount.display,
            match plan.group_by.as_deref() {
                Some("account") => "账户",
                Some("tag") => "标签",
                _ => "月份",
            },
            facts.len(),
            facts[0].label,
            facts[0].display
        )
    } else {
        format!("{label}合计 {}，共 {} 笔", total_amount.display, matched)
    };

    PlanResultDto {
        metric,
        group_by: plan.group_by.clone(),
        from: plan.date_from.clone(),
        to: plan.date_to.clone(),
        accounts: plan.accounts.clone(),
        tags: plan.tags.clone(),
        facts,
        total: total_amount,
        matched: f64_of(Decimal::from(matched)),
        summary,
    }
}

/// The Markdown block a report embeds for an answered question.
pub fn plan_markdown(question: &str, result: &PlanResultDto) -> String {
    let mut out = String::new();
    out.push_str(&format!("### {question}\n\n{}\n\n", result.summary));
    if !result.facts.is_empty() {
        out.push_str("| 分组 | 金额 |\n| --- | --- |\n");
        for fact in &result.facts {
            out.push_str(&format!("| {} | {} |\n", fact.label, fact.display));
        }
        out.push('\n');
    }
    out
}

/// Serialised plan, for showing the user exactly what was executed.
pub fn plan_dto(plan: &QueryPlan, result: &PlanResultDto) -> PlanQueryDto {
    PlanQueryDto {
        metric: result.metric.clone(),
        accounts: plan.accounts.clone(),
        tags: plan.tags.clone(),
        date_from: plan.date_from.clone(),
        date_to: plan.date_to.clone(),
        group_by: plan.group_by.clone(),
        limit: f64_of(Decimal::from(plan.limit)),
        min_amount: plan.min_amount.map(f64_of),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VaultCreateOptions;
    use crate::vault::{create_vault, VaultOptions};
    use crate::writer::ensure_day_file;

    fn vault_with(entries: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        create_vault(dir.path(), &VaultCreateOptions::default()).unwrap();
        let opts = VaultOptions::default();
        for (date, block) in entries {
            let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
            let file = ensure_day_file(dir.path(), day, &opts).unwrap();
            let path = dir.path().join(&file.rel);
            let mut src = std::fs::read_to_string(&path).unwrap();
            src.push_str(block);
            src.push('\n');
            std::fs::write(&path, src).unwrap();
        }
        let vault = Vault::load(dir.path(), &opts).unwrap();
        (dir, vault)
    }

    #[test]
    fn flags_a_category_that_jumped_and_a_new_net_worth_high() {
        let (_dir, vault) = vault_with(&[
            (
                "2026-08-10",
                "2026-08-10 * 工资\n    assets:cash         10000.00 CNY\n    income:salary\n\n2026-08-11 * 买菜\n    expenses:food          100.00 CNY\n    assets:cash\n",
            ),
            (
                "2026-09-10",
                "2026-09-10 * 工资\n    assets:cash         10000.00 CNY\n    income:salary\n\n2026-09-11 * 买菜\n    expenses:food         1000.00 CNY\n    assets:cash\n",
            ),
        ]);
        let list = insights(&vault);
        assert!(
            list.iter()
                .any(|i| i.kind == "spend_up" && i.title.contains("expenses:food")),
            "{list:?}"
        );
        assert!(list.iter().any(|i| i.kind == "net_worth_high"));
    }

    #[test]
    fn plan_sums_by_account_and_by_month() {
        let (_dir, vault) = vault_with(&[(
            "2026-09-10",
            "2026-09-10 * 买菜\n    expenses:food:grocery    120.00 CNY\n    assets:cash\n\n2026-09-11 * 咖啡\n    expenses:food:coffee      30.00 CNY\n    assets:cash\n\n2026-09-12 * 打车\n    expenses:transport         50.00 CNY\n    assets:cash\n",
        )]);

        let plan = QueryPlan {
            metric: "spend".into(),
            accounts: vec!["expenses:food".into()],
            group_by: Some("account".into()),
            limit: 5,
            ..Default::default()
        };
        let result = run_plan(&vault, &plan);
        assert_eq!(result.matched, 2.0);
        assert_eq!(result.total.display, "150.00 CNY");
        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.facts[0].label, "expenses:food");

        let monthly = run_plan(
            &vault,
            &QueryPlan {
                metric: "spend".into(),
                group_by: Some("month".into()),
                ..Default::default()
            },
        );
        assert_eq!(monthly.facts[0].label, "2026-09");
        assert_eq!(monthly.total.display, "200.00 CNY");
    }

    #[test]
    fn plan_respects_dates_tags_and_bounds() {
        let (_dir, vault) = vault_with(&[(
            "2026-09-10",
            "2026-09-10 * 旅行机票  ; :trip:\n    expenses:travel       800.00 CNY\n    assets:cash\n\n2026-09-11 * 咖啡\n    expenses:food          30.00 CNY\n    assets:cash\n",
        )]);
        let result = run_plan(
            &vault,
            &QueryPlan {
                metric: "spend".into(),
                tags: vec!["trip".into()],
                date_from: Some("2026-09".into()),
                date_to: Some("2026-10".into()),
                ..Default::default()
            },
        );
        assert_eq!(result.matched, 1.0);
        assert_eq!(result.total.display, "800.00 CNY");

        let capped = run_plan(
            &vault,
            &QueryPlan {
                metric: "spend".into(),
                min_amount: Some(Decimal::from(100)),
                ..Default::default()
            },
        );
        assert_eq!(capped.matched, 1.0);
    }

    #[test]
    fn counting_returns_a_plain_number() {
        let (_dir, vault) = vault_with(&[(
            "2026-09-10",
            "2026-09-10 * 咖啡\n    expenses:food          30.00 CNY\n    assets:cash\n\n2026-09-11 * 打车\n    expenses:transport      50.00 CNY\n    assets:cash\n",
        )]);
        let result = run_plan(
            &vault,
            &QueryPlan {
                metric: "count".into(),
                ..Default::default()
            },
        );
        assert_eq!(result.matched, 2.0);
        assert_eq!(result.total.display, "2 笔");
    }
}

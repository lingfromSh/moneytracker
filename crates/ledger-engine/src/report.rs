//! Reporting: transaction queries, dashboard aggregates, budgets and forecasts.
//! Everything here is derived data — nothing is written back to the vault.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use ledger_core::Item;
use ledger_query::Query;
use rust_decimal::Decimal;

use crate::amounts::{decimal_to_dto, f64_of};
use crate::load::Vault;
use crate::types::*;
use crate::Result;

pub fn month_key(date: NaiveDate) -> String {
    format!("{:04}-{:02}", date.year(), date.month())
}

fn parse_month(key: &str) -> Option<(i32, u32)> {
    let (y, m) = key.split_once('-')?;
    Some((y.parse().ok()?, m.parse().ok()?))
}

pub fn month_shift(key: &str, delta: i32) -> String {
    let (y, m) = parse_month(key).unwrap_or((2000, 1));
    let total = y * 12 + (m as i32 - 1) + delta;
    format!(
        "{:04}-{:02}",
        total.div_euclid(12),
        total.rem_euclid(12) + 1
    )
}

pub fn month_series(end: &str, count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    (0..count)
        .rev()
        .map(|i| month_shift(end, -(i as i32)))
        .collect()
}

pub fn last_month(vault: &Vault) -> String {
    vault
        .last_date
        .map(month_key)
        .unwrap_or_else(|| month_key(chrono::Local::now().date_naive()))
}

pub fn query_txns(vault: &Vault, req: &QueryRequest) -> Result<TxnPageDto> {
    let mut query = match req.query.as_deref().map(str::trim) {
        Some(q) if !q.is_empty() => Some(Query::parse_lenient(q)),
        _ => None,
    };
    if let Some(q) = &query {
        if q.is_empty() {
            query = None;
        }
    }
    let from = req
        .date_from
        .as_deref()
        .and_then(ledger_core::parser::parse_date_str);
    let to = req
        .date_to
        .as_deref()
        .and_then(ledger_core::parser::parse_date_str);

    let mut hits: Vec<usize> = Vec::new();
    for (idx, rec) in vault.txns.iter().enumerate() {
        if let Some(f) = from {
            if rec.txn.date < f {
                continue;
            }
        }
        if let Some(t) = to {
            if rec.txn.date > t {
                continue;
            }
        }
        if let Some(q) = &query {
            let info = vault.txn_info(idx);
            if !q.matches(&info) {
                continue;
            }
        }
        hits.push(idx);
    }

    let order = req.order.as_deref().unwrap_or("date_desc");
    match order {
        "date" | "date_asc" | "oldest" => hits.sort_by_key(|i| (vault.txns[*i].txn.date, *i)),
        "amount" | "amount_desc" => hits.sort_by(|a, b| {
            let av = vault.txns[*a].base_value.unwrap_or_default();
            let bv = vault.txns[*b].base_value.unwrap_or_default();
            bv.cmp(&av)
        }),
        "amount_asc" => hits.sort_by(|a, b| {
            let av = vault.txns[*a].base_value.unwrap_or_default();
            let bv = vault.txns[*b].base_value.unwrap_or_default();
            av.cmp(&bv)
        }),
        _ => hits.sort_by_key(|i| (std::cmp::Reverse(vault.txns[*i].txn.date), *i)),
    }

    let total = hits.len();
    let offset = req.offset.min(total);
    let limit = if req.limit == 0 { 100 } else { req.limit };
    let items: Vec<TxnDto> = hits
        .iter()
        .skip(offset)
        .take(limit)
        .map(|i| vault.txn_dto(*i))
        .collect();
    Ok(TxnPageDto {
        items,
        total,
        offset,
        limit,
    })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MonthTotals {
    pub income: Decimal,
    pub expense: Decimal,
    pub net_worth: Decimal,
}

pub fn monthly_totals(vault: &Vault) -> BTreeMap<String, MonthTotals> {
    let mut map: BTreeMap<String, MonthTotals> = BTreeMap::new();
    let mut running = Decimal::ZERO;
    let mut sorted: Vec<usize> = (0..vault.txns.len()).collect();
    sorted.sort_by_key(|i| vault.txns[*i].txn.date);

    let mut by_month: BTreeMap<String, Decimal> = BTreeMap::new();
    for idx in sorted {
        let rec = &vault.txns[idx];
        let key = month_key(rec.txn.date);
        let entry = map.entry(key.clone()).or_default();
        let mut net = Decimal::ZERO;
        for v in &rec.values {
            let Some(base) = v.base else { continue };
            match v.kind {
                'R' => entry.income += -base,
                'X' => entry.expense += base,
                'A' | 'L' => net += base,
                _ => {}
            }
        }
        running += net;
        let month_entry = by_month.entry(key).or_insert(Decimal::ZERO);
        *month_entry += net;
    }

    // net worth at each month end
    let mut cumulative = Decimal::ZERO;
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        cumulative += by_month.get(&key).copied().unwrap_or_default();
        if let Some(entry) = map.get_mut(&key) {
            entry.net_worth = cumulative;
        }
    }
    map
}

fn pct_change(current: Decimal, previous: Decimal) -> f64 {
    if previous.is_zero() {
        return 0.0;
    }
    f64_of((current - previous) / previous * Decimal::from(100))
}

pub fn dashboard(vault: &Vault, months: usize) -> DashboardDto {
    let months = months.clamp(1, 60);
    let totals = monthly_totals(vault);
    let end = last_month(vault);
    let series = month_series(&end, months);
    let base = &vault.commodities;

    let month_points: Vec<MonthPointDto> = series
        .iter()
        .map(|m| {
            let t = totals.get(m).copied().unwrap_or_default();
            MonthPointDto {
                month: m.clone(),
                income: f64_of(t.income),
                expense: f64_of(t.expense),
                net: f64_of(t.income - t.expense),
                income_display: base.format(&ledger_core::Amount::canonical(
                    t.income,
                    &base.base,
                    base.base_precision,
                )),
                expense_display: base.format(&ledger_core::Amount::canonical(
                    t.expense,
                    &base.base,
                    base.base_precision,
                )),
                net_display: base.format(&ledger_core::Amount::canonical(
                    t.income - t.expense,
                    &base.base,
                    base.base_precision,
                )),
            }
        })
        .collect();

    let current_totals = totals.get(&end).copied().unwrap_or_default();
    let previous_totals = totals
        .get(&month_shift(&end, -1))
        .copied()
        .unwrap_or_default();
    let net_worth = totals
        .values()
        .last()
        .map(|t| t.net_worth)
        .unwrap_or_default();

    let savings_rate = if current_totals.income.is_zero() {
        0.0
    } else {
        f64_of((current_totals.income - current_totals.expense) / current_totals.income)
    };

    let kpis = KpiDto {
        net_worth: decimal_to_dto(net_worth, &base.base, base.base_precision),
        month_income: decimal_to_dto(current_totals.income, &base.base, base.base_precision),
        month_expense: decimal_to_dto(current_totals.expense, &base.base, base.base_precision),
        savings_rate,
        income_change: pct_change(current_totals.income, previous_totals.income),
        expense_change: pct_change(current_totals.expense, previous_totals.expense),
        month: end.clone(),
        missing_fx_count: vault.missing_fx_count(),
    };

    // categories for the current month (full account path, drillable)
    let mut cat_totals: BTreeMap<String, (Decimal, usize)> = BTreeMap::new();
    for rec in &vault.txns {
        if month_key(rec.txn.date) != end {
            continue;
        }
        for v in &rec.values {
            if v.kind == 'X' {
                if let Some(base_value) = v.base {
                    let entry = cat_totals
                        .entry(v.account.clone())
                        .or_insert((Decimal::ZERO, 0));
                    entry.0 += base_value;
                    entry.1 += 1;
                }
            }
        }
    }
    let grand_total: Decimal = cat_totals.values().map(|(v, _)| *v).sum();
    let mut categories: Vec<CategoryTotalDto> = cat_totals
        .into_iter()
        .map(|(account, (value, count))| CategoryTotalDto {
            label: account.split(':').take(2).collect::<Vec<_>>().join(":"),
            account,
            value: f64_of(value),
            display: base.format(&ledger_core::Amount::canonical(
                value,
                &base.base,
                base.base_precision,
            )),
            share: if grand_total.is_zero() {
                0.0
            } else {
                f64_of(value / grand_total)
            },
            txn_count: count,
        })
        .collect();
    categories.sort_by(|a, b| b.value.total_cmp(&a.value));

    // stacked area: top-level categories per month
    let mut stacked_map: BTreeMap<String, BTreeMap<String, Decimal>> = BTreeMap::new();
    let mut cat_totals_all: BTreeMap<String, Decimal> = BTreeMap::new();
    for rec in &vault.txns {
        let m = month_key(rec.txn.date);
        if !series.contains(&m) {
            continue;
        }
        for v in &rec.values {
            if v.kind != 'X' {
                continue;
            }
            let Some(base_value) = v.base else { continue };
            let label = v.account.split(':').nth(1).unwrap_or("other").to_string();
            *stacked_map
                .entry(m.clone())
                .or_default()
                .entry(label.clone())
                .or_insert(Decimal::ZERO) += base_value;
            *cat_totals_all.entry(label).or_insert(Decimal::ZERO) += base_value;
        }
    }
    let mut top_labels: Vec<(String, Decimal)> = cat_totals_all.into_iter().collect();
    top_labels.sort_by(|a, b| b.1.cmp(&a.1));
    let top: Vec<String> = top_labels.iter().take(6).map(|(l, _)| l.clone()).collect();
    let stacked: Vec<StackedPointDto> = series
        .iter()
        .map(|m| {
            let empty = BTreeMap::new();
            let row = stacked_map.get(m).unwrap_or(&empty);
            let mut values: Vec<PointDto> = Vec::new();
            let mut other = Decimal::ZERO;
            for (label, value) in row {
                if top.contains(label) {
                    values.push(PointDto {
                        label: label.clone(),
                        value: f64_of(*value),
                        display: base.format(&ledger_core::Amount::canonical(
                            *value,
                            &base.base,
                            base.base_precision,
                        )),
                    });
                } else {
                    other += *value;
                }
            }
            values.sort_by(|a, b| b.value.total_cmp(&a.value));
            if !other.is_zero() {
                values.push(PointDto {
                    label: "其他".into(),
                    value: f64_of(other),
                    display: base.format(&ledger_core::Amount::canonical(
                        other,
                        &base.base,
                        base.base_precision,
                    )),
                });
            }
            StackedPointDto {
                month: m.clone(),
                values,
            }
        })
        .collect();

    // net worth series
    let net_worth_series: Vec<PointDto> = series
        .iter()
        .map(|m| {
            let v = totals.get(m).map(|t| t.net_worth).unwrap_or_default();
            PointDto {
                label: m.clone(),
                value: f64_of(v),
                display: base.format(&ledger_core::Amount::canonical(
                    v,
                    &base.base,
                    base.base_precision,
                )),
            }
        })
        .collect();

    // calendar heatmap of expenses
    let mut heat: BTreeMap<NaiveDate, Decimal> = BTreeMap::new();
    for rec in &vault.txns {
        for v in &rec.values {
            if v.kind == 'X' {
                if let Some(base_value) = v.base {
                    *heat.entry(rec.txn.date).or_insert(Decimal::ZERO) += base_value;
                }
            }
        }
    }
    let heatmap: Vec<HeatPointDto> = heat
        .into_iter()
        .map(|(date, value)| HeatPointDto {
            date: date.to_string(),
            value: f64_of(value),
            display: base.format(&ledger_core::Amount::canonical(
                value,
                &base.base,
                base.base_precision,
            )),
        })
        .collect();

    let budget = budget_rows(vault, &end);
    let forecast = forecast_points(vault, 6);
    let sankey = sankey_for(vault, &series);
    let fx_missing: Vec<TxnDto> = vault
        .txns
        .iter()
        .enumerate()
        .filter(|(_, t)| t.fx_missing)
        .map(|(i, _)| vault.txn_dto(i))
        .collect();

    DashboardDto {
        kpis,
        months: month_points,
        net_worth_series,
        categories,
        stacked,
        heatmap,
        budget,
        forecast,
        sankey,
        fx_missing,
    }
}

pub fn budgets(vault: &Vault) -> Vec<BudgetDto> {
    let mut out = Vec::new();
    for file in &vault.files {
        for item in &file.journal.items {
            let Item::Periodic(pt) = item else { continue };
            for p in &pt.postings {
                let Some(amount) = &p.amount else { continue };
                // Envelopes are the positive legs; the negative leg is the funding
                // account and only exists to make the periodic block balance.
                if amount.quantity.is_sign_negative() || amount.quantity.is_zero() {
                    continue;
                }
                let kind = vault.kind_for(&p.account);
                // X = spend envelope, A/L = savings envelope (money set aside).
                if !matches!(kind, 'X' | 'A' | 'L') {
                    continue;
                }
                out.push(BudgetDto {
                    period: pt.period.clone(),
                    account: p.account.clone(),
                    kind: kind.to_string(),
                    amount: crate::amounts::to_dto(amount),
                    raw: format!("~ {} {}", pt.period, p.account),
                });
            }
        }
    }
    out
}

fn monthly_budget_amount(amount: &ledger_core::Amount, period: &str) -> Decimal {
    let period = period.to_lowercase();
    if period.contains("year") || period.contains("yearly") {
        amount.quantity / Decimal::from(12)
    } else if period.contains("week") {
        amount.quantity * Decimal::from(52) / Decimal::from(12)
    } else if period.contains("quarter") {
        amount.quantity / Decimal::from(3)
    } else {
        amount.quantity
    }
}

fn budget_rows(vault: &Vault, month: &str) -> Vec<BudgetRowDto> {
    let budgets = budgets(vault);
    let mut actual: BTreeMap<String, Decimal> = BTreeMap::new();
    for rec in &vault.txns {
        if month_key(rec.txn.date) != month {
            continue;
        }
        for v in &rec.values {
            if matches!(v.kind, 'X' | 'A' | 'L') {
                if let Some(base) = v.base {
                    *actual.entry(v.account.clone()).or_insert(Decimal::ZERO) += base;
                }
            }
        }
    }
    let base = &vault.commodities;
    let mut rows: Vec<BudgetRowDto> = budgets
        .iter()
        .map(|b| {
            let budget_qty = if b.amount.commodity == base.base {
                monthly_budget_amount(
                    &ledger_core::Amount::canonical(
                        b.amount.quantity.parse::<Decimal>().unwrap_or_default(),
                        &base.base,
                        base.base_precision,
                    ),
                    &b.period,
                )
            } else {
                Decimal::ZERO
            };
            // An envelope on `expenses:food` also covers its sub-accounts, which is
            // how people actually budget ("餐饮" means all of 餐饮:*).
            let prefix = format!("{}:", b.account);
            let spent: Decimal = actual
                .iter()
                .filter(|(account, _)| {
                    account.as_str() == b.account || account.starts_with(&prefix)
                })
                .map(|(_, value)| *value)
                .sum();
            BudgetRowDto {
                account: b.account.clone(),
                kind: b.kind.clone(),
                label: b
                    .account
                    .split(':')
                    .skip(1)
                    .take(1)
                    .collect::<Vec<_>>()
                    .join(":"),
                budget: f64_of(budget_qty),
                actual: f64_of(spent),
                budget_display: base.format(&ledger_core::Amount::canonical(
                    budget_qty,
                    &base.base,
                    base.base_precision,
                )),
                actual_display: base.format(&ledger_core::Amount::canonical(
                    spent,
                    &base.base,
                    base.base_precision,
                )),
                remaining_display: base.format(&ledger_core::Amount::canonical(
                    budget_qty - spent,
                    &base.base,
                    base.base_precision,
                )),
                ratio: if budget_qty.is_zero() {
                    0.0
                } else {
                    f64_of(spent / budget_qty)
                },
            }
        })
        .collect();
    rows.sort_by(|a, b| b.ratio.total_cmp(&a.ratio));
    rows
}

pub fn forecast_points(vault: &Vault, months: usize) -> Vec<ForecastPointDto> {
    let months = months.clamp(1, 36);
    let end = last_month(vault);
    let base = &vault.commodities;
    let budgets = budgets(vault);
    let budget_total: Decimal = budgets
        .iter()
        .map(|b| {
            monthly_budget_amount(
                &ledger_core::Amount::canonical(
                    b.amount.quantity.parse::<Decimal>().unwrap_or_default(),
                    &base.base,
                    base.base_precision,
                ),
                &b.period,
            )
        })
        .sum();

    let totals = monthly_totals(vault);
    let history = month_series(&end, 6);
    let expenses: Vec<Decimal> = history
        .iter()
        .map(|m| totals.get(m).map(|t| t.expense).unwrap_or_default())
        .collect();
    let moving_average = if expenses.is_empty() {
        Decimal::ZERO
    } else {
        expenses.iter().sum::<Decimal>() / Decimal::from(expenses.len())
    };
    let forecast_amount = if budget_total.is_zero() {
        moving_average
    } else if moving_average.is_zero() {
        budget_total
    } else {
        budget_total * Decimal::new(6, 1) + moving_average * Decimal::new(4, 1)
    };

    let mut out = Vec::new();
    for m in month_series(&end, 3) {
        let t = totals.get(&m).copied().unwrap_or_default();
        out.push(ForecastPointDto {
            month: m,
            actual: Some(f64_of(t.expense)),
            forecast: None,
            label: "实际".into(),
        });
    }
    for i in 1..=months {
        let m = month_shift(&end, i as i32);
        out.push(ForecastPointDto {
            month: m,
            actual: None,
            forecast: Some(f64_of(forecast_amount)),
            label: "预测".into(),
        });
    }
    out
}

fn sankey_for(vault: &Vault, months: &[String]) -> SankeyDto {
    let mut links: BTreeMap<(String, String), Decimal> = BTreeMap::new();
    let mut nodes: BTreeMap<String, SankeyNodeDto> = BTreeMap::new();

    let add_node =
        |id: &str, label: &str, layer: &str, nodes: &mut BTreeMap<String, SankeyNodeDto>| {
            nodes
                .entry(id.to_string())
                .or_insert_with(|| SankeyNodeDto {
                    id: id.to_string(),
                    label: label.to_string(),
                    layer: layer.to_string(),
                });
        };

    for rec in &vault.txns {
        if !months.contains(&month_key(rec.txn.date)) {
            continue;
        }
        let incomes: Vec<(&str, Decimal)> = rec
            .values
            .iter()
            .filter(|v| v.kind == 'R')
            .filter_map(|v| v.base.map(|b| (v.account.as_str(), -b)))
            .collect();
        let expenses: Vec<(&str, Decimal)> = rec
            .values
            .iter()
            .filter(|v| v.kind == 'X')
            .filter_map(|v| v.base.map(|b| (v.account.as_str(), b)))
            .collect();
        let assets: Vec<(&str, Decimal)> = rec
            .values
            .iter()
            .filter(|v| matches!(v.kind, 'A' | 'L'))
            .filter_map(|v| v.base.map(|b| (v.account.as_str(), b)))
            .collect();
        let hub = assets
            .iter()
            .max_by_key(|(_, v)| v.abs())
            .map(|(a, _)| *a)
            .unwrap_or("assets:unclassified");

        for (acct, amount) in incomes {
            add_node(acct, acct, "income", &mut nodes);
            add_node(hub, hub, "account", &mut nodes);
            *links
                .entry((acct.to_string(), hub.to_string()))
                .or_insert(Decimal::ZERO) += amount;
        }
        for (acct, amount) in expenses {
            add_node(hub, hub, "account", &mut nodes);
            add_node(acct, acct, "expense", &mut nodes);
            *links
                .entry((hub.to_string(), acct.to_string()))
                .or_insert(Decimal::ZERO) += amount;
        }
    }

    SankeyDto {
        nodes: nodes.into_values().collect(),
        links: links
            .into_iter()
            .map(|((source, target), value)| SankeyLinkDto {
                source,
                target,
                value: f64_of(value),
            })
            .collect(),
        period: months.first().cloned().unwrap_or_default()
            + " → "
            + months.last().map(|s| s.as_str()).unwrap_or(""),
    }
}

pub fn fx_missing(vault: &Vault) -> Vec<FxMissingDto> {
    let base = &vault.commodities;
    vault
        .txns
        .iter()
        .filter(|t| t.fx_missing)
        .flat_map(|t| {
            t.values
                .iter()
                .filter(|v| v.base.is_none() && !v.amount.commodity.is_empty())
                .map(move |v| FxMissingDto {
                    txn_id: t.id.clone(),
                    date: t.txn.date.to_string(),
                    payee: t.txn.payee.clone(),
                    commodity: v.amount.commodity.clone(),
                    amount_display: base.format(&v.amount),
                })
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

    fn vault() -> (tempfile::TempDir, Vault) {
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
            dir.path().join("2026/2026-09.ledger"),
            "include 2026-09-30.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-10.ledger"),
            "include 2026-10-05.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026.ledger"),
            "include 2026-09.ledger\ninclude 2026-10.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-09-30.ledger"),
            "2026-09-30 salary\n    assets:cash   1000.00 CNY\n    income:salary  -1000.00 CNY\n\n2026-09-30 rent\n    expenses:rent  300.00 CNY\n    assets:cash   -300.00 CNY\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-10-05.ledger"),
            "2026-10-05 coffee\n    expenses:food  20.00 CNY\n    assets:cash   -20.00 CNY\n\n2026-10-05 salary\n    assets:cash   1000.00 CNY\n    income:salary  -1000.00 CNY\n",
        )
        .unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        (dir, vault)
    }

    fn vault_with_budgets(budgets: &str) -> (tempfile::TempDir, Vault) {
        let (dir, _) = vault();
        std::fs::write(dir.path().join("budgets.ledger"), budgets).unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        (dir, vault)
    }

    #[test]
    fn spend_and_savings_envelopes_both_show_up() {
        let (_dir, vault) = vault_with_budgets(
            "~ monthly from 2026-01-01\n    expenses:food   500.00 CNY\n    assets:cash\n\n~ monthly from 2026-01-01\n    assets:invest   200.00 CNY\n    assets:cash\n",
        );
        let rows = budget_rows(&vault, "2026-10");
        let food = rows
            .iter()
            .find(|r| r.account == "expenses:food")
            .expect("spending envelope present");
        assert_eq!(food.kind, "X");
        assert_eq!(food.budget, 500.0);
        assert_eq!(food.actual, 20.0);
        let saving = rows
            .iter()
            .find(|r| r.account == "assets:invest")
            .expect("savings envelope present");
        assert_eq!(saving.kind, "A");
        assert_eq!(saving.budget, 200.0);
        assert_eq!(saving.actual, 0.0);
    }

    #[test]
    fn savings_envelopes_measure_the_money_set_aside() {
        let (dir, _) = vault();
        std::fs::write(
            dir.path().join("2026/2026-10-06.ledger"),
            "2026-10-06 transfer to invest\n    assets:invest   150.00 CNY\n    assets:cash   -150.00 CNY\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026/2026-10.ledger"),
            "include 2026-10-05.ledger\ninclude 2026-10-06.ledger\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("budgets.ledger"),
            "~ monthly from 2026-01-01\n    assets:invest   200.00 CNY\n    assets:cash\n",
        )
        .unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        let rows = budget_rows(&vault, "2026-10");
        let saving = rows.iter().find(|r| r.account == "assets:invest").unwrap();
        assert_eq!(
            saving.actual, 150.0,
            "money moved into the envelope counts as progress"
        );
        assert_eq!(saving.remaining_display, "50.00 CNY");
        assert!((saving.ratio - 0.75).abs() < 1e-9);
    }

    #[test]
    fn months_and_net_worth_add_up() {
        let (_dir, vault) = vault();
        let d = dashboard(&vault, 3);
        let oct = d.months.last().unwrap();
        assert_eq!(oct.month, "2026-10");
        assert_eq!(oct.income, 1000.0);
        assert_eq!(oct.expense, 20.0);
        assert_eq!(oct.net, 980.0);
        assert_eq!(d.kpis.net_worth.value, 1680.0);
        assert!((d.kpis.savings_rate - 0.98).abs() < 1e-9);
    }

    #[test]
    fn categories_and_sankey_are_populated() {
        let (_dir, vault) = vault();
        let d = dashboard(&vault, 3);
        assert!(d.categories.iter().any(|c| c.account == "expenses:food"));
        assert!(d.sankey.links.iter().any(|l| l.target == "expenses:food"));
        assert!(d.sankey.links.iter().any(|l| l.source == "income:salary"));
    }

    #[test]
    fn query_filters_and_pages() {
        let (_dir, vault) = vault();
        let page = query_txns(
            &vault,
            &QueryRequest {
                query: Some("acct:food".into()),
                offset: 0,
                limit: 10,
                order: None,
                date_from: None,
                date_to: None,
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].payee, "coffee");
    }

    #[test]
    fn query_orders_newest_first() {
        let (_dir, vault) = vault();
        let page = query_txns(
            &vault,
            &QueryRequest {
                query: None,
                offset: 0,
                limit: 2,
                order: None,
                date_from: None,
                date_to: None,
            },
        )
        .unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(page.items[0].date, "2026-10-05");
    }

    #[test]
    fn forecast_marks_future_months() {
        let (_dir, vault) = vault();
        let f = forecast_points(&vault, 6);
        assert!(f.iter().any(|p| p.label == "实际"));
        assert!(f.iter().any(|p| p.label == "预测" && p.forecast.is_some()));
        assert_eq!(f.iter().filter(|p| p.label == "预测").count(), 6);
    }

    #[test]
    fn heatmap_has_expense_days() {
        let (_dir, vault) = vault();
        let d = dashboard(&vault, 3);
        assert!(d.heatmap.iter().any(|h| h.date == "2026-10-05"));
    }
}

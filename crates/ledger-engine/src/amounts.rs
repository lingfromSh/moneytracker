//! Commodity metadata, double-entry balancing and DTO conversion helpers.

use std::collections::BTreeMap;

use ledger_core::amount::AmountStyle;
use ledger_core::{Amount, Posting, VirtualKind};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::types::AmountDto;

#[derive(Debug, Clone)]
pub struct CommodityTable {
    pub base: String,
    pub base_precision: u32,
    pub precision: BTreeMap<String, u32>,
    pub symbols: BTreeMap<String, AmountStyle>,
    /// Usage counts (for the settings/commodities list).
    pub used: BTreeMap<String, usize>,
}

impl CommodityTable {
    pub fn new(base: impl Into<String>, base_precision: u32) -> Self {
        Self {
            base: base.into(),
            base_precision,
            precision: BTreeMap::new(),
            symbols: BTreeMap::new(),
            used: BTreeMap::new(),
        }
    }

    pub fn precision_of(&self, commodity: &str) -> u32 {
        if commodity == self.base {
            return self.base_precision;
        }
        self.precision
            .get(commodity)
            .copied()
            .unwrap_or(self.base_precision)
    }

    pub fn observe(&mut self, amount: &Amount) {
        if amount.commodity.is_empty() {
            return;
        }
        *self.used.entry(amount.commodity.clone()).or_insert(0) += 1;
        let p = amount.precision();
        self.precision
            .entry(amount.commodity.clone())
            .and_modify(|e| *e = (*e).max(p))
            .or_insert(p);
        self.symbols
            .entry(amount.commodity.clone())
            .or_insert_with(|| amount.style.clone());
    }

    /// Canonical amount for a commodity at the commodity's display precision.
    pub fn canonical(&self, quantity: Decimal, commodity: &str) -> Amount {
        Amount::canonical(quantity, commodity, self.precision_of(commodity))
    }

    /// Amount rendered at this commodity's display precision.
    ///
    /// Amounts built with `Amount::canonical` carry no grouping, so the style
    /// recorded from the ledger's `commodity`/posting declarations is used to
    /// fill in the group mark (and the prefix/space layout) when it is missing.
    pub fn format(&self, amount: &Amount) -> String {
        let mut rendered = amount.with_precision(self.precision_of(&amount.commodity));
        if rendered.style.group_mark.is_none() {
            if let Some(style) = self.symbols.get(&rendered.commodity) {
                rendered.style.group_mark = style.group_mark;
                rendered.style.prefix = style.prefix;
                rendered.style.space = style.space;
                rendered.style.decimal_mark = style.decimal_mark;
            }
        }
        rendered.format()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceIssue {
    pub code: String,
    pub message: String,
}

fn issue(code: &str, message: impl Into<String>) -> BalanceIssue {
    BalanceIssue {
        code: code.to_string(),
        message: message.into(),
    }
}

/// Postings that participate in the double-entry check.
pub fn balanced_postings(postings: &[Posting]) -> impl Iterator<Item = &Posting> {
    postings.iter().filter(|p| p.is_balanced())
}

/// Per-commodity sums of the balanced postings (using effective amounts).
pub fn commodity_sums(postings: &[Posting]) -> BTreeMap<String, Decimal> {
    let mut sums: BTreeMap<String, Decimal> = BTreeMap::new();
    for p in balanced_postings(postings) {
        if let Some(eff) = p.effective_amount() {
            *sums.entry(eff.commodity).or_insert(Decimal::ZERO) += eff.quantity;
        }
    }
    sums
}

/// Fill the single elided amount (and apply the default commodity) so that the
/// transaction balances. Returns issues instead of panicking on bad input.
pub fn balance(
    postings: &mut [Posting],
    table: &CommodityTable,
    default_commodity: &str,
) -> Result<(), Vec<BalanceIssue>> {
    let mut issues = Vec::new();

    for p in postings.iter_mut() {
        if let Some(a) = p.amount.as_mut() {
            if a.commodity.is_empty() && !default_commodity.is_empty() {
                a.commodity = default_commodity.to_string();
                a.style = AmountStyle::canonical(
                    default_commodity,
                    table.precision_of(default_commodity),
                );
            }
        }
        if let Some(price) = p.price.as_mut() {
            if price.amount.commodity.is_empty() && !default_commodity.is_empty() {
                price.amount.commodity = default_commodity.to_string();
            }
        }
    }

    let elided: Vec<usize> = postings
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is_balanced() && p.amount.is_none())
        .map(|(i, _)| i)
        .collect();

    let sums = commodity_sums(postings);

    match elided.len() {
        0 => {}
        1 => {
            let idx = elided[0];
            let non_zero: Vec<(&String, &Decimal)> =
                sums.iter().filter(|(_, v)| !v.is_zero()).collect();
            match non_zero.len() {
                0 => {
                    issues.push(issue(
                        "balance.no_commodity",
                        "cannot infer the elided amount: no other postings in this transaction",
                    ));
                }
                1 => {
                    let (commodity, total) = non_zero[0];
                    // Do not balance virtual postings against real ones.
                    postings[idx].amount = Some(table.canonical(-*total, commodity));
                }
                _ => {
                    issues.push(issue(
                        "balance.multi_commodity",
                        format!(
                            "cannot infer the elided amount: multiple commodities are out of balance ({})",
                            sums.iter()
                                .map(|(c, v)| format!("{v} {c}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }
            }
        }
        n => {
            issues.push(issue(
                "balance.multiple_elided",
                format!("{n} postings have elided amounts; at most one is allowed"),
            ));
        }
    }

    if issues.is_empty() {
        for (commodity, total) in commodity_sums(postings) {
            if !total.is_zero() {
                issues.push(issue(
                    "balance.not_zero",
                    format!(
                        "transaction does not balance: {} remaining",
                        table.format(&tally(total, &commodity))
                    ),
                ));
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn tally(quantity: Decimal, commodity: &str) -> Amount {
    Amount::canonical(quantity, commodity, 2)
}

/// True when the postings balance exactly (ignores virtual postings).
pub fn is_balanced(postings: &[Posting]) -> bool {
    commodity_sums(postings).values().all(|v| v.is_zero())
}

/// Signed sum of a transaction's real posting values converted to base currency.
/// Positive means money left the funding accounts (an expense).
pub fn net_cash_flow(postings: &[Posting]) -> Option<Amount> {
    let mut total: Option<Amount> = None;
    for p in balanced_postings(postings) {
        if p.virtual_kind == VirtualKind::Virtual {
            continue;
        }
        let eff = p.effective_amount()?;
        match &mut total {
            None => total = Some(eff),
            Some(t) => {
                if t.commodity == eff.commodity {
                    t.quantity += eff.quantity;
                }
            }
        }
    }
    total
}

pub fn to_dto(amount: &Amount) -> AmountDto {
    AmountDto {
        quantity: amount.quantity.to_string(),
        value: amount.quantity.to_f64().unwrap_or(0.0),
        commodity: amount.commodity.clone(),
        display: amount.format(),
    }
}

pub fn to_dto_prec(amount: &Amount, precision: u32) -> AmountDto {
    let a = amount.with_precision(precision);
    AmountDto {
        quantity: a.quantity.to_string(),
        value: a.quantity.to_f64().unwrap_or(0.0),
        commodity: a.commodity.clone(),
        display: a.format(),
    }
}

pub fn decimal_to_dto(quantity: Decimal, commodity: &str, precision: u32) -> AmountDto {
    to_dto_prec(
        &Amount::canonical(quantity, commodity, precision),
        precision,
    )
}

pub fn f64_of(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_core::{parse_journal, Item};

    fn txns(src: &str) -> Vec<ledger_core::Transaction> {
        parse_journal("t.ledger", src)
            .journal
            .items
            .into_iter()
            .filter_map(|i| match i {
                Item::Transaction(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    fn table() -> CommodityTable {
        let mut t = CommodityTable::new("CNY", 2);
        t.precision.insert("USD".into(), 2);
        t
    }

    #[test]
    fn fills_single_elided_amount() {
        let mut t = txns("2026-01-01 x\n    expenses:a   10 CNY\n    assets:b\n");
        let txn = &mut t[0];
        balance(&mut txn.postings, &table(), "CNY").unwrap();
        assert_eq!(
            txn.postings[1].amount.as_ref().unwrap().format(),
            "-10.00 CNY"
        );
        assert!(is_balanced(&txn.postings));
    }

    #[test]
    fn applies_default_commodity() {
        let mut t = txns("2026-01-01 x\n    expenses:a   10\n    assets:b   -10\n");
        let txn = &mut t[0];
        balance(&mut txn.postings, &table(), "CNY").unwrap();
        assert_eq!(
            txn.postings[0].amount.as_ref().unwrap().format(),
            "10.00 CNY"
        );
    }

    #[test]
    fn detects_imbalance() {
        let mut t = txns("2026-01-01 x\n    expenses:a   10 CNY\n    assets:b   -9 CNY\n");
        let txn = &mut t[0];
        let errs = balance(&mut txn.postings, &table(), "CNY").unwrap_err();
        assert_eq!(errs[0].code, "balance.not_zero");
    }

    #[test]
    fn prices_balance_the_transaction() {
        let mut t = txns("2026-01-01 x\n    expenses:a   100 USD @@ 720 CNY\n    assets:b\n");
        let txn = &mut t[0];
        balance(&mut txn.postings, &table(), "CNY").unwrap();
        assert_eq!(
            txn.postings[1].amount.as_ref().unwrap().format(),
            "-720.00 CNY"
        );
    }

    #[test]
    fn two_elided_postings_is_an_error() {
        let mut t = txns("2026-01-01 x\n    expenses:a   10 CNY\n    assets:b\n    assets:c\n");
        let txn = &mut t[0];
        let errs = balance(&mut txn.postings, &table(), "CNY").unwrap_err();
        assert_eq!(errs[0].code, "balance.multiple_elided");
    }
}

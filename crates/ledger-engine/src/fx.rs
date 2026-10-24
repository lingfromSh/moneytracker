//! Foreign exchange: `P` directives are the single source of truth.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use ledger_core::{Amount, PriceDecl};

#[derive(Debug, Clone)]
pub struct PriceRecord {
    pub date: NaiveDate,
    pub commodity: String,
    pub quote: String,
    pub rate: Decimal,
    pub quote_precision: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PriceTable {
    records: Vec<PriceRecord>,
}

impl PriceTable {
    pub fn from_decls<'a>(decls: impl Iterator<Item = &'a PriceDecl>) -> Self {
        let mut records: Vec<PriceRecord> = decls
            .filter(|p| !p.amount.commodity.is_empty() && !p.commodity.is_empty())
            .map(|p| PriceRecord {
                date: p.date,
                commodity: p.commodity.clone(),
                quote: p.amount.commodity.clone(),
                rate: p.amount.quantity,
                quote_precision: p.amount.precision(),
            })
            .collect();
        records.sort_by(|a, b| {
            a.date
                .cmp(&b.date)
                .then(a.commodity.cmp(&b.commodity))
                .then(a.quote.cmp(&b.quote))
        });
        Self { records }
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn all(&self) -> &[PriceRecord] {
        &self.records
    }

    /// Most recent rate `from -> to` on or before `date` (direct or inverse).
    pub fn rate_on(&self, from: &str, to: &str, date: NaiveDate) -> Option<Decimal> {
        if from == to {
            return Some(Decimal::ONE);
        }
        let mut direct: Option<&PriceRecord> = None;
        let mut inverse: Option<&PriceRecord> = None;
        for r in &self.records {
            if r.date > date {
                break;
            }
            if r.commodity == from && r.quote == to {
                direct = Some(r);
            }
            if r.commodity == to && r.quote == from {
                inverse = Some(r);
            }
        }
        if let Some(r) = direct {
            return Some(r.rate);
        }
        if let Some(r) = inverse {
            if !r.rate.is_zero() {
                return Some(Decimal::ONE / r.rate);
            }
        }
        None
    }

    /// Convert an amount into `target` as of `date`.
    pub fn convert(&self, amount: &Amount, target: &str, date: NaiveDate) -> Option<Amount> {
        if amount.commodity == target || amount.commodity.is_empty() {
            return Some(amount.clone());
        }
        let rate = self.rate_on(&amount.commodity, target, date)?;
        let quote_precision = self
            .records
            .iter()
            .rev()
            .find(|r| {
                (r.commodity == amount.commodity && r.quote == target)
                    || (r.commodity == target && r.quote == amount.commodity)
            })
            .map(|r| r.quote_precision)
            .unwrap_or(4);
        let _ = quote_precision;
        Some(Amount::canonical(
            amount.quantity * rate,
            target,
            4, // keep 4 dp internally; display rounds to the commodity precision
        ))
    }

    /// Latest known rate per pair, used by the settings/prices view.
    pub fn latest_by_pair(&self) -> BTreeMap<(String, String), Decimal> {
        let mut out = BTreeMap::new();
        for r in &self.records {
            out.insert((r.commodity.clone(), r.quote.clone()), r.rate);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_core::{parse_journal, Item};

    fn price_table(src: &str) -> PriceTable {
        let out = parse_journal("prices.ledger", src);
        let decls: Vec<PriceDecl> = out
            .journal
            .items
            .into_iter()
            .filter_map(|i| match i {
                Item::Price(p) => Some(p),
                _ => None,
            })
            .collect();
        PriceTable::from_decls(decls.iter())
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn uses_latest_rate_on_or_before() {
        let t = price_table(
            "P 2026-01-01 USD 7.00 CNY\nP 2026-06-01 USD 7.20 CNY\nP 2026-12-01 USD 7.40 CNY\n",
        );
        assert_eq!(
            t.rate_on("USD", "CNY", d("2026-07-01")).unwrap(),
            Decimal::new(720, 2)
        );
        assert_eq!(
            t.rate_on("USD", "CNY", d("2026-01-01")).unwrap(),
            Decimal::new(700, 2)
        );
        assert!(t.rate_on("USD", "CNY", d("2025-12-31")).is_none());
        assert_eq!(
            t.rate_on("CNY", "USD", d("2026-07-01")).unwrap(),
            Decimal::ONE / Decimal::new(720, 2)
        );
        assert_eq!(
            t.rate_on("CNY", "CNY", d("2026-07-01")).unwrap(),
            Decimal::ONE
        );
    }

    #[test]
    fn converts_amounts() {
        let t = price_table("P 2026-01-01 USD 7.00 CNY\n");
        let usd = Amount::canonical(Decimal::new(1000, 2), "USD", 2);
        let cny = t.convert(&usd, "CNY", d("2026-02-01")).unwrap();
        assert_eq!(cny.quantity, Decimal::new(7000, 2));
    }

    #[test]
    fn missing_rate_returns_none() {
        let t = price_table("P 2026-01-01 EUR 7.60 CNY\n");
        let usd = Amount::canonical(Decimal::new(1000, 2), "USD", 2);
        assert!(t.convert(&usd, "CNY", d("2026-02-01")).is_none());
    }
}

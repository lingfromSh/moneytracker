//! Account reconciliation: what is cleared, what is not, and whether the
//! ledger agrees with an assertion you wrote into the file.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use ledger_core::ast::Status;

use crate::amounts::decimal_to_dto;
use crate::types::{ReconcilePreviewDto, ReconcileRowDto};
use crate::Vault;

/// Everything touching `account` (or its subaccounts) up to `as_of`.
pub fn preview(vault: &Vault, account: &str, as_of: Option<NaiveDate>) -> ReconcilePreviewDto {
    let (resolved, warnings) = vault.resolve_account(account);
    let (base, base_precision) = vault.base_commodity();
    let prefix = format!("{resolved}:");
    let mut cleared = Decimal::ZERO;
    let mut uncleared = Decimal::ZERO;
    let mut cleared_count = 0usize;
    let mut uncleared_count = 0usize;
    let mut rows: Vec<ReconcileRowDto> = Vec::new();
    let mut assertion: Option<Decimal> = None;
    let mut missing_fx = 0usize;

    let mut records: Vec<_> = vault.transactions().iter().collect();
    records.sort_by(|a, b| b.txn.date.cmp(&a.txn.date));

    for record in records {
        if let Some(limit) = as_of {
            if record.txn.date > limit {
                continue;
            }
        }
        for (posting, value) in record.txn.postings.iter().zip(record.values.iter()) {
            if value.account != resolved && !value.account.starts_with(&prefix) {
                continue;
            }
            let amount = match value.base {
                Some(base) => base,
                None => {
                    missing_fx += 1;
                    Decimal::ZERO
                }
            };
            let is_cleared = record.txn.status == Some(Status::Cleared);
            if is_cleared {
                cleared += amount;
                cleared_count += 1;
            } else {
                uncleared += amount;
                uncleared_count += 1;
            }
            rows.push(ReconcileRowDto {
                id: record.id.clone(),
                date: record.txn.date.to_string(),
                payee: record.txn.payee.clone(),
                amount: decimal_to_dto(
                    value.amount.quantity,
                    &value.amount.commodity,
                    vault.commodities.precision_of(&value.amount.commodity),
                ),
                cleared: is_cleared,
            });
            if let Some(asserted) = posting.balance_assertion.as_ref() {
                if assertion.is_none() {
                    // Foreign-commodity assertions are compared as written: the
                    // panel reports the raw number rather than inventing a rate.
                    assertion = Some(asserted.quantity);
                }
            }
        }
    }

    let balance = cleared + uncleared;
    let difference = assertion.map(|asserted| balance - asserted);
    let mut notes: Vec<String> = Vec::new();
    if missing_fx > 0 {
        notes.push(format!("{missing_fx} 笔缺少汇率，按 0 计入"));
    }
    if let Some(asserted) = assertion {
        match difference {
            Some(diff) if diff.is_zero() => notes.push("与账本里的余额断言一致".into()),
            Some(diff) => notes.push(format!(
                "与余额断言相差 {}",
                vault
                    .commodities
                    .format(&ledger_core::amount::Amount::canonical(
                        diff,
                        base,
                        base_precision,
                    ))
            )),
            None => {}
        }
        let _ = asserted;
    }
    notes.extend(warnings);

    ReconcilePreviewDto {
        account: resolved.clone(),
        as_of: as_of.map(|date| date.to_string()),
        cleared: decimal_to_dto(cleared, base, base_precision),
        uncleared: decimal_to_dto(uncleared, base, base_precision),
        balance: decimal_to_dto(balance, base, base_precision),
        assertion: assertion.map(|value| decimal_to_dto(value, base, base_precision)),
        difference: difference.map(|value| decimal_to_dto(value, base, base_precision)),
        cleared_count,
        uncleared_count,
        rows,
        note: if notes.is_empty() {
            None
        } else {
            Some(notes.join(" · "))
        },
    }
}

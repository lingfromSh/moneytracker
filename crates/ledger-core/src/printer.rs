//! Canonical printer. `parse -> print -> parse` is a fixed point (see tests).

use rust_decimal::Decimal;

use crate::amount::{format_quantity, Amount, AmountStyle};
use crate::ast::*;

pub fn print_journal(journal: &Journal) -> String {
    let mut out = String::new();
    for (i, item) in journal.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&print_item(item));
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn print_item(item: &Item) -> String {
    match item {
        Item::Comment(c) => c.lines.join("\n"),
        Item::Include(i) => {
            if i.optional {
                format!("!include {}", i.path)
            } else {
                format!("include {}", i.path)
            }
        }
        Item::Account(a) => {
            let mut s = format!("account {}", a.name);
            if let Some(c) = &a.comment {
                s.push_str(&format!("  ; {}", c));
            }
            s
        }
        Item::Commodity(c) => {
            let body = c
                .sample
                .as_ref()
                .map(|a| a.format())
                .unwrap_or_else(|| c.name.clone());
            let mut s = format!("commodity {}", body);
            if let Some(comment) = &c.comment {
                s.push_str(&format!("  ; {}", comment));
            }
            s
        }
        Item::DefaultCommodity(c) => match &c.sample {
            Some(a) => format!("D {}", a.format()),
            None => format!("D {}", c.name),
        },
        Item::Year(y) => format!("Y {}", y.year),
        Item::Price(p) => format!("P {} {} {}", p.date, p.commodity, p.amount.format()),
        Item::Transaction(t) => print_transaction(t),
        Item::Periodic(p) => {
            let mut out = format!("~ {}", p.period);
            if let Some(c) = &p.comment {
                out.push_str(&format!("  ; {}", c));
            }
            out.push('\n');
            out.push_str(&print_postings(&p.postings));
            out
        }
        Item::AutoRule(r) => {
            let mut out = format!("= {}", r.query);
            if let Some(c) = &r.comment {
                out.push_str(&format!("  ; {}", c));
            }
            out.push('\n');
            out.push_str(&print_postings(&r.postings));
            out
        }
    }
}

/// Canonical transaction block, no trailing newline.
pub fn print_transaction(t: &Transaction) -> String {
    let mut header = format!("{}", t.date);
    if let Some(d2) = t.date2 {
        header.push_str(&format!("={}", d2));
    }
    if let Some(st) = t.status {
        header.push(' ');
        header.push_str(st.marker());
    }
    if let Some(code) = &t.code {
        header.push_str(&format!(" ({})", code));
    }
    if !t.payee.is_empty() {
        header.push(' ');
        header.push_str(&t.payee);
    }
    if let Some(c) = &t.comment {
        header.push_str(&format!("  ; {}", c));
    }
    if t.postings.is_empty() {
        return header;
    }
    format!("{}\n{}", header, print_postings(&t.postings))
}

pub fn print_postings(postings: &[Posting]) -> String {
    let rendered: Vec<(String, &Posting)> =
        postings.iter().map(|p| (render_account(p), p)).collect();
    let width = rendered
        .iter()
        .map(|(account, _)| display_width(account))
        .max()
        .unwrap_or(0)
        + 2;
    let mut lines = Vec::with_capacity(postings.len());
    for (account, p) in rendered {
        let mut amount = String::new();
        if let Some(a) = &p.amount {
            amount.push_str(&a.format());
        }
        if let Some(pct) = p.percent {
            if !amount.is_empty() {
                amount.push(' ');
            }
            amount.push_str(&format_percent(pct));
        }
        if let Some(price) = &p.price {
            amount.push_str(match price.kind {
                PriceKind::Unit => " @ ",
                PriceKind::Total => " @@ ",
            });
            amount.push_str(&price.amount.format());
        }
        if let Some(bal) = &p.balance_assertion {
            amount.push_str(" = ");
            amount.push_str(&bal.format());
        }

        if amount.is_empty() {
            match &p.comment {
                Some(c) => lines.push(format!("    {}  ; {}", account, c)),
                None => lines.push(format!("    {}", account)),
            }
            continue;
        }

        // Two spaces is the minimum account/amount separator in the dialect.
        let pad = width.saturating_sub(display_width(&account)).max(2);
        let mut line = format!("    {}{}{}", account, " ".repeat(pad), amount);
        if let Some(c) = &p.comment {
            line.push_str(&format!("  ; {}", c));
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

/// The account column as printed: status marker, virtual brackets, account name.
fn render_account(p: &Posting) -> String {
    let mut account = match p.virtual_kind {
        VirtualKind::Real => p.account.clone(),
        VirtualKind::Virtual => format!("({})", p.account),
        VirtualKind::BalancedVirtual => format!("[{}]", p.account),
    };
    if let Some(st) = p.status {
        account = format!("{} {}", st.marker(), account);
    }
    account
}

/// Terminals render CJK as double width, so pad by column width, not char count.
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF |
        0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F |
        0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

pub fn format_percent(p: Decimal) -> String {
    let style = AmountStyle {
        precision: 2,
        decimal_mark: '.',
        group_mark: None,
        prefix: false,
        space: false,
    };
    format!("{}%", format_quantity(p, &style))
}

/// Format an amount the way the application writes generated amounts.
pub fn canonical_amount(quantity: Decimal, commodity: &str, precision: u32) -> String {
    Amount::canonical(quantity, commodity, precision).format()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_journal;

    fn reprint(src: &str) -> String {
        let out = parse_journal("t.ledger", src);
        assert!(!out.has_errors(), "{:?}", out.diagnostics);
        print_journal(&out.journal)
    }

    /// Prints, reparses and reprints: the printer must be a fixed point.
    fn assert_fixed_point(src: &str) -> String {
        let first = reprint(src);
        let second = reprint(&first);
        assert_eq!(first, second, "printer is not idempotent for:\n{src}");
        let a = parse_journal("t.ledger", &first);
        let b = parse_journal("t.ledger", &second);
        assert_eq!(a.journal, b.journal, "AST changed on second round trip");
        first
    }

    #[test]
    fn prints_transaction_block() {
        let out = assert_fixed_point(
            "2026-10-25 * 星巴克  ; txn:01JA3KQ7\n    expenses:food:coffee      4.50 CNY\n    assets:wallet:alipay     -4.50 CNY\n",
        );
        assert!(out.contains("expenses:food:coffee"), "{out}");
        assert!(out.contains("txn:01JA3KQ7"), "{out}");
    }

    #[test]
    fn prints_directives() {
        let src = "commodity 1,000.00 CNY\naccount assets:cash  ; type:A\ninclude 2026/2026.ledger\n!include opt.ledger\nP 2026-10-25 USD 7.12 CNY\nY 2026\n";
        assert_eq!(reprint(src), src);
    }

    #[test]
    fn prints_rules_with_percent() {
        assert_fixed_point(
            "= desc:星巴克 and amt:>0  ; rule:r-1\n    expenses:food:coffee   70.00%\n    expenses:food:beans    30.00%\n    assets:wallet:alipay\n",
        );
    }

    #[test]
    fn prints_price_and_assertion() {
        assert_fixed_point(
            "2026-01-01 x\n    expenses:travel   100 USD @@ 720 CNY = 720 CNY\n    assets:cash\n",
        );
    }

    #[test]
    fn percent_formatting() {
        assert_eq!(
            format_percent(rust_decimal::Decimal::new(7000, 2)),
            "70.00%"
        );
        assert_eq!(
            format_percent(rust_decimal::Decimal::new(3333, 2)),
            "33.33%"
        );
    }
}

//! Journal AST. Every node keeps its byte span so the writer can rewrite blocks.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::amount::Amount;
use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Cleared,
    Pending,
}

impl Status {
    pub fn marker(&self) -> &'static str {
        match self {
            Status::Cleared => "*",
            Status::Pending => "!",
        }
    }

    pub fn from_marker(c: char) -> Option<Self> {
        match c {
            '*' => Some(Status::Cleared),
            '!' => Some(Status::Pending),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualKind {
    #[default]
    Real,
    /// `(account)` — ignored by balance reports.
    Virtual,
    /// `[account]` — included in balance reports, must balance separately.
    BalancedVirtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceKind {
    /// `@` — price per unit.
    Unit,
    /// `@@` — total price for the whole posting amount.
    Total,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Price {
    pub kind: PriceKind,
    pub amount: Amount,
    pub span: Span,
}

/// `:tag:` (value `None`) or `key:value` (value `Some`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub key: String,
    pub value: Option<String>,
}

impl Tag {
    pub fn bare(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: None,
        }
    }

    pub fn kv(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
        }
    }

    /// Render back into hledger comment syntax (`:a:b:` and `k:v`).
    pub fn render(&self) -> String {
        match &self.value {
            None => format!(":{}:", self.key),
            Some(v) => format!("{}:{}", self.key, v),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    pub span: Span,
    pub status: Option<Status>,
    pub account: String,
    pub virtual_kind: VirtualKind,
    /// `None` when the amount is elided (inferred by the balancing engine).
    pub amount: Option<Amount>,
    /// Rule template percentage (`70.00%`), only used inside `rules.ledger`.
    pub percent: Option<Decimal>,
    pub price: Option<Price>,
    pub balance_assertion: Option<Amount>,
    /// Raw comment text following `;`, tags included.
    pub comment: Option<String>,
    pub tags: Vec<Tag>,
}

impl Posting {
    pub fn new(account: impl Into<String>, amount: Option<Amount>) -> Self {
        Self {
            span: Span::default(),
            status: None,
            account: account.into(),
            virtual_kind: VirtualKind::Real,
            amount,
            percent: None,
            price: None,
            balance_assertion: None,
            comment: None,
            tags: Vec::new(),
        }
    }

    /// Amount converted to the posting's total value in its own commodity
    /// (honours `@@` total prices and `@` unit prices).
    pub fn effective_amount(&self) -> Option<Amount> {
        let amount = self.amount.as_ref()?;
        match &self.price {
            None => Some(amount.clone()),
            Some(p) => match p.kind {
                PriceKind::Total => Some(Amount::new(
                    if amount.quantity.is_sign_negative() {
                        -p.amount.quantity
                    } else {
                        p.amount.quantity
                    },
                    p.amount.commodity.clone(),
                    p.amount.style.clone(),
                )),
                PriceKind::Unit => Some(Amount::new(
                    amount.quantity * p.amount.quantity,
                    p.amount.commodity.clone(),
                    p.amount.style.clone(),
                )),
            },
        }
    }

    /// True when the posting takes part in the double-entry balance check.
    pub fn is_balanced(&self) -> bool {
        self.virtual_kind != VirtualKind::Virtual
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub span: Span,
    pub date: NaiveDate,
    pub date2: Option<NaiveDate>,
    pub status: Option<Status>,
    pub code: Option<String>,
    pub payee: String,
    /// Raw header comment (everything after `;`).
    pub comment: Option<String>,
    pub tags: Vec<Tag>,
    pub postings: Vec<Posting>,
}

impl Transaction {
    pub fn new(date: NaiveDate, payee: impl Into<String>) -> Self {
        Self {
            span: Span::default(),
            date,
            date2: None,
            status: None,
            code: None,
            payee: payee.into(),
            comment: None,
            tags: Vec::new(),
            postings: Vec::new(),
        }
    }

    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.key == key)
            .and_then(|t| t.value.as_deref())
    }

    pub fn has_tag(&self, key: &str) -> bool {
        self.tags.iter().any(|t| t.key == key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountDecl {
    pub span: Span,
    pub name: String,
    pub comment: Option<String>,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommodityDecl {
    pub span: Span,
    pub name: String,
    pub sample: Option<Amount>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceDecl {
    pub span: Span,
    pub date: NaiveDate,
    pub commodity: String,
    pub amount: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Include {
    pub span: Span,
    pub path: String,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodicTransaction {
    pub span: Span,
    pub period: String,
    pub comment: Option<String>,
    pub postings: Vec<Posting>,
}

/// `= query` block: either an application rule (percent templates) or a plain
/// hledger auto posting. Metadata lives in the header comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoRule {
    pub span: Span,
    pub query: String,
    pub comment: Option<String>,
    pub tags: Vec<Tag>,
    pub postings: Vec<Posting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentBlock {
    pub span: Span,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearDecl {
    pub span: Span,
    pub year: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Item {
    Comment(CommentBlock),
    Include(Include),
    Account(AccountDecl),
    Commodity(CommodityDecl),
    DefaultCommodity(CommodityDecl),
    Year(YearDecl),
    Price(PriceDecl),
    Transaction(Transaction),
    Periodic(PeriodicTransaction),
    AutoRule(AutoRule),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Comment(x) => x.span,
            Item::Include(x) => x.span,
            Item::Account(x) => x.span,
            Item::Commodity(x) => x.span,
            Item::DefaultCommodity(x) => x.span,
            Item::Year(x) => x.span,
            Item::Price(x) => x.span,
            Item::Transaction(x) => x.span,
            Item::Periodic(x) => x.span,
            Item::AutoRule(x) => x.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Journal {
    pub items: Vec<Item>,
}

impl Journal {
    pub fn transactions(&self) -> impl Iterator<Item = &Transaction> {
        self.items.iter().filter_map(|i| match i {
            Item::Transaction(t) => Some(t),
            _ => None,
        })
    }

    pub fn includes(&self) -> impl Iterator<Item = &Include> {
        self.items.iter().filter_map(|i| match i {
            Item::Include(x) => Some(x),
            _ => None,
        })
    }

    pub fn account_decls(&self) -> impl Iterator<Item = &AccountDecl> {
        self.items.iter().filter_map(|i| match i {
            Item::Account(a) => Some(a),
            _ => None,
        })
    }

    pub fn auto_rules(&self) -> impl Iterator<Item = &AutoRule> {
        self.items.iter().filter_map(|i| match i {
            Item::AutoRule(r) => Some(r),
            _ => None,
        })
    }
}

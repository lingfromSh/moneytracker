//! `ledger-core` — AST, parser, printer and diagnostics for the hledger subset
//! understood by MoneyTracker.
//!
//! Design notes:
//! * every AST node carries a byte [`Span`], which is what makes span-level
//!   rewriting in `ledger-engine` byte-exact and safe;
//! * the parser is hand written (line oriented recursive descent) because we
//!   need precise spans, high quality error recovery and line/column mapping;
//! * the printer is canonical and idempotent: `parse -> print -> parse` is a
//!   fixed point, so rewriting a block never cascades unrelated changes.

pub mod amount;
pub mod ast;
pub mod diagnostic;
pub mod parser;
pub mod printer;
pub mod span;

pub use amount::{parse_amount, parse_percent, Amount, AmountStyle};
pub use ast::*;
pub use diagnostic::{Diagnostic, Severity};
pub use parser::{parse_journal, ParseOutput};
pub use printer::{print_item, print_journal, print_transaction};
pub use span::{LineIndex, Span};

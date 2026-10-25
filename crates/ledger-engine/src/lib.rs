//! `ledger-engine` — the vault: include graph, incremental index, balances,
//! capture expansion, rules, FX, budgets and reporting.
//!
//! The engine never mutates a file without (a) parsing it first, (b) taking a
//! snapshot into `.bak/`, (c) refusing when the file changed underneath us.

pub mod amounts;
pub mod capture;
pub mod fx;
pub mod load;
pub mod plan;
pub mod report;
pub mod rules;
pub mod statements;
pub mod types;
pub mod vault;
pub mod writer;

pub use capture::{
    analyze, expand, new_txn_id, parse_capture, CaptureAnalysis, ExpandedCapture, Kind,
    ParsedCapture,
};
pub use load::{
    diagnostic_dto, is_system_tag, posting_dto, split_comment, strip_system_tokens, txn_dto,
    AccountInfo, TxnRecord, Vault,
};
pub use plan::{fire_projection, overview as plan_overview, PlanSet, GOALS_FILE};
pub use rules::{Direction, RuleDef, RuleSet};
pub use statements::{balance_sheet, income_statement, net_worth_bridge};
pub use vault::{create_vault, validate_vault, VaultOptions};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("i/o error: {0}")]
    Io(String),
    #[error("vault error: {0}")]
    Vault(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("not found: {0}")]
    NotFound(String),
}

impl EngineError {
    /// Stable machine readable code surfaced to the frontend.
    pub fn code(&self) -> &'static str {
        match self {
            EngineError::Io(_) => "engine.io",
            EngineError::Vault(_) => "engine.vault",
            EngineError::Conflict(_) => "engine.conflict",
            EngineError::Refused(_) => "engine.refused",
            EngineError::NotFound(_) => "engine.not_found",
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

//! Diagnostics: parse errors and warnings with stable codes and source positions.

use serde::{Deserialize, Serialize};

use crate::span::{LineIndex, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// A single diagnostic tied to a byte range of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable machine readable code, e.g. `core.parse.unsupported_lot_price`.
    pub code: String,
    pub message: String,
    pub file: String,
    pub span: Span,
    pub line: u32,
    pub col: u32,
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>, file: &str, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            code: code.to_string(),
            message: message.into(),
            file: file.to_string(),
            span,
            line: 0,
            col: 0,
        }
    }

    pub fn warning(code: &str, message: impl Into<String>, file: &str, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.to_string(),
            message: message.into(),
            file: file.to_string(),
            span,
            line: 0,
            col: 0,
        }
    }

    /// Fill in `line`/`col` from the source text this diagnostic refers to.
    pub fn with_position(mut self, src: &str) -> Self {
        let index = LineIndex::new(src);
        let (line, col) = index.line_col(src, self.span.start);
        self.line = line;
        self.col = col;
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// `file:line:col: error[code]: message`
    pub fn display(&self) -> String {
        format!(
            "{}:{}:{}: {}[{}]: {}",
            self.file,
            self.line,
            self.col,
            self.severity.as_str(),
            self.code,
            self.message
        )
    }
}

/// Convenience for the many `Ok(...) | Err(diag)` sites inside the parser.
pub type PResult<T> = Result<T, Diagnostic>;

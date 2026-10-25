//! Serialisable DTOs shared with the TypeScript frontend (via `tauri-specta`).
//!
//! Money is transferred as strings plus a pre-formatted `display`; the `value`
//! field is a float that exists **only** so chart libraries can compute pixel
//! geometry. Every authoritative sum is computed with `rust_decimal` here.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AmountDto {
    pub quantity: String,
    /// Display-only float; never use for arithmetic in the frontend.
    pub value: f64,
    pub commodity: String,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DiagnosticDto {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VaultInfoDto {
    pub path: String,
    pub base_commodity: String,
    pub base_precision: u32,
    pub default_funding: String,
    pub default_unclassified: String,
    pub fx_enabled: bool,
    #[specta(type = f64)]
    pub transaction_count: usize,
    #[specta(type = f64)]
    pub file_count: usize,
    pub first_date: Option<String>,
    pub last_date: Option<String>,
    pub parse_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TagDto {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PostingDto {
    pub account: String,
    pub amount: Option<AmountDto>,
    pub commodity: String,
    pub quantity: String,
    pub display: String,
    pub price: Option<AmountDto>,
    pub price_kind: Option<String>,
    pub assertion: Option<AmountDto>,
    pub comment: Option<String>,
    pub virtual_kind: String,
    pub tags: Vec<TagDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TxnDto {
    pub id: String,
    pub date: String,
    pub date2: Option<String>,
    pub status: Option<String>,
    pub code: Option<String>,
    pub payee: String,
    pub comment: Option<String>,
    pub tags: Vec<TagDto>,
    pub postings: Vec<PostingDto>,
    pub file: String,
    #[specta(type = f64)]
    pub span_start: usize,
    #[specta(type = f64)]
    pub span_end: usize,
    /// Signed total in the base commodity (positive = money out of assets).
    pub base_value: Option<f64>,
    pub base_value_display: Option<String>,
    pub balanced: bool,
    pub fx_missing: bool,
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TxnPageDto {
    pub items: Vec<TxnDto>,
    #[specta(type = f64)]
    pub total: usize,
    #[specta(type = f64)]
    pub offset: usize,
    #[specta(type = f64)]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BalanceRowDto {
    pub account: String,
    pub commodity: String,
    pub quantity: String,
    pub value: f64,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AccountNodeDto {
    pub name: String,
    pub full: String,
    pub kind: String,
    pub depth: u32,
    pub declared: bool,
    /// Direct postings on this account.
    pub own: Vec<AmountDto>,
    /// Own postings plus all descendants.
    pub total: Vec<AmountDto>,
    #[specta(type = f64)]
    pub txn_count: usize,
    pub children: Vec<AccountNodeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CommodityDto {
    pub symbol: String,
    pub precision: u32,
    pub is_base: bool,
    #[specta(type = f64)]
    pub used: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SnapshotDto {
    pub vault: VaultInfoDto,
    pub accounts: Vec<AccountNodeDto>,
    pub commodities: Vec<CommodityDto>,
    pub balances: Vec<BalanceRowDto>,
    pub net_worth: Vec<AmountDto>,
    pub diagnostics: Vec<DiagnosticDto>,
    pub loaded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PointDto {
    pub label: String,
    pub value: f64,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MonthPointDto {
    pub month: String,
    pub income: f64,
    pub expense: f64,
    pub net: f64,
    pub income_display: String,
    pub expense_display: String,
    pub net_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CategoryTotalDto {
    pub account: String,
    pub label: String,
    pub value: f64,
    pub display: String,
    pub share: f64,
    #[specta(type = f64)]
    pub txn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StackedPointDto {
    pub month: String,
    /// category label -> value (base commodity)
    pub values: Vec<PointDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HeatPointDto {
    pub date: String,
    pub value: f64,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct KpiDto {
    pub net_worth: AmountDto,
    pub month_income: AmountDto,
    pub month_expense: AmountDto,
    pub savings_rate: f64,
    pub income_change: f64,
    pub expense_change: f64,
    pub month: String,
    #[specta(type = f64)]
    pub missing_fx_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BudgetRowDto {
    pub account: String,
    pub label: String,
    /// `X` for a spending envelope, `A`/`L` for a savings envelope.
    pub kind: String,
    pub budget: f64,
    pub actual: f64,
    pub budget_display: String,
    pub actual_display: String,
    pub remaining_display: String,
    pub ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ForecastPointDto {
    pub month: String,
    pub actual: Option<f64>,
    pub forecast: Option<f64>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SankeyNodeDto {
    pub id: String,
    pub label: String,
    pub layer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SankeyLinkDto {
    pub source: String,
    pub target: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SankeyDto {
    pub nodes: Vec<SankeyNodeDto>,
    pub links: Vec<SankeyLinkDto>,
    pub period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DashboardDto {
    pub kpis: KpiDto,
    pub months: Vec<MonthPointDto>,
    pub net_worth_series: Vec<PointDto>,
    pub categories: Vec<CategoryTotalDto>,
    pub stacked: Vec<StackedPointDto>,
    pub heatmap: Vec<HeatPointDto>,
    pub budget: Vec<BudgetRowDto>,
    pub forecast: Vec<ForecastPointDto>,
    pub sankey: SankeyDto,
    pub fx_missing: Vec<TxnDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CaptureParseDto {
    /// `transaction` | `rule` | `search` | `command` | `empty`
    pub kind: String,
    pub canonical_text: String,
    pub postings: Vec<PostingDto>,
    pub warnings: Vec<String>,
    pub errors: Vec<DiagnosticDto>,
    pub rule_id: Option<String>,
    pub rule_preview: Option<RulePreviewDto>,
    pub summary: Option<String>,
    pub date: Option<String>,
    pub payee: Option<String>,
    pub amount_display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RulePreviewDto {
    pub query: String,
    pub from_account: Option<String>,
    pub template: Vec<RulePostingDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CaptureCommitDto {
    pub file: String,
    #[specta(type = f64)]
    pub span_start: usize,
    #[specta(type = f64)]
    pub span_end: usize,
    pub txn_id: String,
    pub canonical_text: String,
    pub created_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RulePostingDto {
    pub account: String,
    pub percent: Option<String>,
    pub amount: Option<AmountDto>,
    pub is_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RuleDto {
    pub id: String,
    pub query: String,
    pub enabled: bool,
    #[specta(type = f64)]
    pub order: i64,
    pub from_account: Option<String>,
    pub postings: Vec<RulePostingDto>,
    pub raw: String,
    #[specta(type = f64)]
    pub span_start: usize,
    #[specta(type = f64)]
    pub span_end: usize,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RuleTestDto {
    pub rule_id: String,
    pub matches: Vec<TxnDto>,
    #[specta(type = f64)]
    pub matched_count: usize,
    pub expanded: Vec<PostingDto>,
    pub expanded_for: Option<TxnDto>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RuleRecomputeDto {
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BudgetDto {
    pub period: String,
    pub account: String,
    /// `X` for a spending envelope, `A`/`L` for a savings envelope.
    pub kind: String,
    pub amount: AmountDto,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxMissingDto {
    pub txn_id: String,
    pub date: String,
    pub payee: String,
    pub commodity: String,
    pub amount_display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxSyncResultDto {
    #[specta(type = f64)]
    pub requested: usize,
    pub written: Vec<String>,
    pub failed: Vec<String>,
    pub skipped_reason: Option<String>,
}

/// Whether the configured global capture shortcut is actually live.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ShortcutStatusDto {
    pub shortcut: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuditEntryDto {
    pub ts: String,
    pub op: String,
    pub file: String,
    #[specta(type = f64)]
    pub span_start: usize,
    #[specta(type = f64)]
    pub span_end: usize,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct VaultCreateOptions {
    pub base_commodity: Option<String>,
    pub default_funding: Option<String>,
    pub default_unclassified: Option<String>,
    pub template: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VaultOptionsDto {
    pub base_commodity: String,
    pub base_precision: u32,
    pub default_funding: String,
    pub default_unclassified: String,
    pub fx_enabled: bool,
    pub account_type_map: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct SaveTxnPatch {
    pub date: Option<String>,
    pub payee: Option<String>,
    pub comment: Option<String>,
    pub status: Option<String>,
    pub postings: Option<Vec<PostingPatch>>,
    pub tags: Option<Vec<TagDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PostingPatch {
    pub account: String,
    pub amount: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BulkPatch {
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
    pub shift_account: Option<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct QueryRequest {
    pub query: Option<String>,
    #[specta(type = f64)]
    pub offset: usize,
    #[specta(type = f64)]
    pub limit: usize,
    pub order: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LedgerChangedEvent {
    pub paths: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SavedResult<T: Type + Serialize> {
    pub data: T,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OpenVaultResultDto {
    pub info: VaultInfoDto,
    pub diagnostics: Vec<DiagnosticDto>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// planning (goals.ledger), statements and reports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AssumptionsDto {
    /// Percent, e.g. `6.0` for 6 %.
    pub nominal_return: f64,
    pub inflation: f64,
    pub reserve_months: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FireTargetDto {
    /// Percent, e.g. `4.0`.
    pub swr: f64,
    pub target: AmountDto,
    /// Months until the pool covers the target; `null` when unreachable.
    pub eta_months: Option<f64>,
    pub eta_date: Option<String>,
    pub eta_label: String,
    pub achieved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FireDto {
    pub annual_expense: AmountDto,
    pub balance: AmountDto,
    pub contribution: AmountDto,
    /// Real (after-inflation) return, in percent.
    pub real_return: f64,
    pub primary_swr: f64,
    pub progress: f64,
    pub targets: Vec<FireTargetDto>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ReserveDto {
    pub cash: AmountDto,
    pub monthly_expense: AmountDto,
    pub months: f64,
    pub target_months: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PoolMemberDto {
    pub account: String,
    pub class: Option<String>,
    pub amount: AmountDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GoalDto {
    pub label: String,
    pub account: Option<String>,
    pub target: AmountDto,
    pub current: AmountDto,
    pub progress: f64,
    pub by: Option<String>,
    pub months_left: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExtraPaymentDto {
    pub extra: AmountDto,
    pub months_saved: f64,
    pub interest_saved: AmountDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DebtDto {
    pub label: String,
    pub account: String,
    pub balance: AmountDto,
    /// Percent, e.g. `3.15`.
    pub rate: Option<f64>,
    pub payment: Option<AmountDto>,
    pub monthly_interest: Option<AmountDto>,
    pub interest_paid: AmountDto,
    pub payoff_months: Option<f64>,
    pub payoff_date: Option<String>,
    pub note: Option<String>,
    pub extra: Vec<ExtraPaymentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AllocationRowDto {
    pub class: String,
    pub current: AmountDto,
    pub current_share: f64,
    pub target_share: Option<f64>,
    /// Percentage points, current minus target.
    pub drift: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AllocationDto {
    pub rows: Vec<AllocationRowDto>,
    /// Pool accounts without a `class:` tag.
    pub untagged: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PlanOverviewDto {
    pub has_goals: bool,
    pub goals_text: String,
    pub assumptions: AssumptionsDto,
    pub fire: FireDto,
    pub reserve: ReserveDto,
    pub pool: Vec<PoolMemberDto>,
    pub goals: Vec<GoalDto>,
    pub debts: Vec<DebtDto>,
    pub allocation: AllocationDto,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StatementRowDto {
    pub account: String,
    pub label: String,
    pub unit: AmountDto,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SectionDto {
    pub title: String,
    pub rows: Vec<StatementRowDto>,
    pub total: AmountDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BalanceSheetDto {
    pub as_of: String,
    pub assets: SectionDto,
    pub liabilities: SectionDto,
    pub equity: SectionDto,
    pub income: SectionDto,
    pub expense: SectionDto,
    /// Income minus expense since the ledger began.
    pub cumulative_net: AmountDto,
    pub net_worth: AmountDto,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IncomeStatementDto {
    pub from: String,
    pub to: String,
    pub income: SectionDto,
    pub expenses: SectionDto,
    pub net: AmountDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BridgeDto {
    pub from: String,
    pub to: String,
    pub opening: AmountDto,
    pub income: AmountDto,
    pub expense: AmountDto,
    pub valuation: AmountDto,
    pub closing: AmountDto,
    pub reconciled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PeriodDto {
    /// `2026-10`, `2026-Q3`, `2026`, `ytd` or `from..to`.
    pub key: String,
    pub from: String,
    pub to: String,
    pub label: String,
}

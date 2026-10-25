//! Optional LLM assistance for the capture palette.
//!
//! The model never writes ledger text: it only fills a small JSON draft which we
//! turn into one canonical capture line, so every LLM entry still goes through
//! the same parser, rule expansion, balance check and write path as a typed one.
//!
//! Two protocols are supported — OpenAI-compatible chat completions (OpenAI,
//! DeepSeek, Moonshot, Ollama, LM Studio, any self-hosted gateway) and the
//! Anthropic Messages API.
//!
//! Everything in this module except [`complete`] is a pure function so it can be
//! unit tested without a network or an API key.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";
pub const DEFAULT_ANTHROPIC_BASE: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAi,
    Anthropic,
}

impl Protocol {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Protocol::Anthropic,
            _ => Protocol::OpenAi,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::OpenAi => "openai",
            Protocol::Anthropic => "anthropic",
        }
    }
}

/// A provider preset: what we fill in when the user picks it in 设置.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub provider: &'static str,
    pub protocol: Protocol,
    pub base_url: &'static str,
    pub model: &'static str,
    /// Self-hosted endpoints (Ollama, LM Studio) work without an API key.
    pub key_required: bool,
}

pub const PRESETS: [Preset; 5] = [
    Preset {
        provider: "openai",
        protocol: Protocol::OpenAi,
        base_url: DEFAULT_OPENAI_BASE,
        model: "gpt-4o-mini",
        key_required: true,
    },
    Preset {
        provider: "anthropic",
        protocol: Protocol::Anthropic,
        base_url: DEFAULT_ANTHROPIC_BASE,
        model: "claude-3-5-haiku-latest",
        key_required: true,
    },
    Preset {
        provider: "deepseek",
        protocol: Protocol::OpenAi,
        base_url: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
        key_required: true,
    },
    Preset {
        provider: "ollama",
        protocol: Protocol::OpenAi,
        base_url: "http://localhost:11434/v1",
        model: "qwen2.5:7b",
        key_required: false,
    },
    Preset {
        provider: "custom",
        protocol: Protocol::OpenAi,
        base_url: "",
        model: "",
        key_required: false,
    },
];

pub fn preset(provider: &str) -> Preset {
    let key = provider.trim().to_ascii_lowercase();
    PRESETS
        .iter()
        .copied()
        .find(|p| p.provider == key)
        .unwrap_or(PRESETS[0])
}

/// A fully resolved client configuration (settings + preset defaults + secret).
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub timeout: Duration,
    pub max_tokens: u32,
}

impl LlmConfig {
    /// Settings win over the preset; a blank field means “use the default”.
    pub fn resolve(
        provider: &str,
        protocol: &str,
        base_url: &str,
        model: &str,
        api_key: &str,
    ) -> Self {
        let p = preset(provider);
        let base = if base_url.trim().is_empty() {
            p.base_url.to_string()
        } else {
            base_url.trim().to_string()
        };
        let model = if model.trim().is_empty() {
            p.model.to_string()
        } else {
            model.trim().to_string()
        };
        Self {
            provider: p.provider.to_string(),
            protocol: if protocol.trim().is_empty() {
                p.protocol
            } else {
                Protocol::parse(protocol)
            },
            base_url: base,
            model,
            api_key: api_key.trim().to_string(),
            timeout: Duration::from_secs(30),
            max_tokens: 1200,
        }
    }

    pub fn needs_key(&self) -> bool {
        preset(&self.provider).key_required
    }

    pub fn is_ready(&self) -> bool {
        !self.base_url.is_empty()
            && !self.model.is_empty()
            && (!self.needs_key() || !self.api_key.is_empty())
    }

    /// Why the client cannot be used yet, phrased for the UI.
    pub fn block_reason(&self) -> Option<String> {
        if self.base_url.is_empty() {
            return Some("还没有填写 Base URL".into());
        }
        if self.model.is_empty() {
            return Some("还没有填写模型名".into());
        }
        if self.needs_key() && self.api_key.is_empty() {
            return Some("还没有填写 API Key".into());
        }
        None
    }

    /// The configured endpoint minus any trailing slash and minus a full
    /// `…/chat/completions` or `…/messages` suffix (people paste those), with `/v1`
    /// added when only a host was given — `http://box:1234` is virtually always
    /// `http://box:1234/v1` for the OpenAI-compatible servers.
    fn base(&self) -> String {
        let mut base = self.base_url.trim().trim_end_matches('/').to_string();
        for suffix in ["/chat/completions", "/messages"] {
            if let Some(stripped) = base.strip_suffix(suffix) {
                base = stripped.trim_end_matches('/').to_string();
                break;
            }
        }
        let without_scheme = base
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&base);
        if !without_scheme.contains('/') {
            base.push_str("/v1");
        }
        base
    }

    pub fn endpoint(&self) -> String {
        match self.protocol {
            Protocol::OpenAi => format!("{}/chat/completions", self.base()),
            Protocol::Anthropic => format!("{}/messages", self.base()),
        }
    }
}

/// The accounts the open ledger actually has, grouped so the prompt can describe
/// them the way a bookkeeper would: assets, liabilities, income, expenses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountCatalog {
    pub assets: Vec<String>,
    pub liabilities: Vec<String>,
    pub income: Vec<String>,
    pub expenses: Vec<String>,
    pub equity: Vec<String>,
    pub other: Vec<String>,
}

/// Names that appear in notes for payment methods, and the account segments they
/// stand for (specific first, then a generic fallback such as "wallet").
const PAYMENT_ALIASES: &[(&str, &[&str], &[&str])] = &[
    ("微信", &["wechat", "weixin"], &["wallet", "钱包"]),
    ("支付宝", &["alipay", "zhifubao"], &["wallet", "钱包"]),
    ("花呗", &["huabei", "花呗"], &["credit", "wallet"]),
    ("京东", &["jd", "jingdong"], &["wallet"]),
    ("现金", &["cash"], &[]),
    ("钱包", &["wallet", "钱包"], &["cash"]),
    ("信用卡", &["creditcard", "credit"], &[]),
    ("储蓄卡", &["saving", "debit"], &[]),
    ("招行", &["cmb", "cmbchina"], &[]),
    ("工行", &["icbc"], &[]),
    ("建行", &["ccb"], &[]),
    ("农行", &["abc"], &[]),
    ("中行", &["boc"], &[]),
    ("交行", &["bocom"], &[]),
];

/// Top-level names that only exist so the tree can nest under them.
const BARE_ROOTS: [&str; 8] = [
    "assets",
    "asset",
    "liabilities",
    "liability",
    "income",
    "expenses",
    "expense",
    "equity",
];

/// Lowercase, colon-joined, no stray whitespace or leading `@`.
fn canonical_account(name: &str) -> String {
    name.trim()
        .trim_start_matches('@')
        .split(':')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(":")
        .to_lowercase()
}

impl AccountCatalog {
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut catalog = AccountCatalog::default();
        for raw in names {
            let name = canonical_account(raw.as_ref());
            // `assets`/`expenses` alone are tree scaffolding, not postable accounts:
            // offering them invites the model to dump everything on the root.
            if name.is_empty() || BARE_ROOTS.contains(&name.as_str()) {
                continue;
            }
            if catalog.all().any(|existing| existing == &name) {
                continue;
            }
            let group = match name.split(':').next().unwrap_or_default() {
                "assets" | "asset" => &mut catalog.assets,
                "liabilities" | "liability" => &mut catalog.liabilities,
                "income" | "revenues" | "revenue" => &mut catalog.income,
                "expenses" | "expense" => &mut catalog.expenses,
                "equity" => &mut catalog.equity,
                _ => &mut catalog.other,
            };
            group.push(name);
        }
        catalog
    }

    pub fn is_empty(&self) -> bool {
        self.all().next().is_none()
    }

    pub fn all(&self) -> impl Iterator<Item = &String> {
        self.assets
            .iter()
            .chain(self.liabilities.iter())
            .chain(self.income.iter())
            .chain(self.expenses.iter())
            .chain(self.equity.iter())
            .chain(self.other.iter())
    }

    fn groups(&self) -> [(&'static str, &Vec<String>); 6] {
        [
            ("Assets", &self.assets),
            ("Liabilities", &self.liabilities),
            ("Income", &self.income),
            ("Expenses", &self.expenses),
            ("Equity", &self.equity),
            ("Other", &self.other),
        ]
    }

    /// The account segment tokens worth matching for `wanted` (its last segment).
    fn tokens_of(wanted: &str) -> Vec<String> {
        let last = wanted.rsplit(':').next().unwrap_or(wanted);
        let mut tokens = vec![last.to_string()];
        if let Some((_, specific, _)) = PAYMENT_ALIASES.iter().find(|(word, _, _)| *word == last) {
            tokens.extend(specific.iter().map(|token| token.to_string()));
        }
        tokens
    }

    /// Accounts whose own segment contains one of `tokens` — only when unique.
    fn find_with_tokens(&self, tokens: &[&str]) -> Option<String> {
        let hits: Vec<String> = self
            .all()
            .filter(|account| {
                account.split(':').any(|segment| {
                    tokens
                        .iter()
                        .any(|token| !token.is_empty() && segment.contains(&token.to_lowercase()))
                })
            })
            .cloned()
            .collect();
        match hits.len() {
            0 => None,
            1 => Some(hits[0].clone()),
            // Prefer the shallowest match so `assets:wallet` wins over
            // `assets:wallet:alipay` when the note only says "wallet".
            _ => hits
                .into_iter()
                .min_by_key(|account| account.matches(':').count()),
        }
    }

    /// Map a name the model made up onto an account that exists here: exact,
    /// unique suffix, closest existing ancestor, then a keyword match.
    pub fn resolve(&self, wanted: &str) -> Option<String> {
        let wanted = canonical_account(wanted);
        if wanted.is_empty() || self.is_empty() {
            return None;
        }
        if let Some(hit) = self.all().find(|account| **account == wanted) {
            return Some(hit.clone());
        }
        let suffix = format!(":{wanted}");
        let matches: Vec<&String> = self
            .all()
            .filter(|account| account.ends_with(&suffix))
            .collect();
        if matches.len() == 1 {
            return Some(matches[0].clone());
        }
        // `assets:wallet:alipay` → `assets:wallet` when the sub-account is not declared.
        let mut parts: Vec<&str> = wanted.split(':').collect();
        while parts.len() > 1 {
            parts.pop();
            let candidate = parts.join(":");
            if let Some(hit) = self.all().find(|account| **account == candidate) {
                return Some(hit.clone());
            }
        }
        let tokens = Self::tokens_of(&wanted);
        let borrowed: Vec<&str> = tokens.iter().map(String::as_str).collect();
        self.find_with_tokens(&borrowed)
    }

    /// The prompt block: grouped, capped, and explicit about the spelling.
    fn render(&self, per_group: usize) -> String {
        let mut out = String::new();
        for (label, group) in self.groups() {
            if group.is_empty() {
                continue;
            }
            out.push_str(label);
            out.push_str(":\n");
            for name in group.iter().take(per_group) {
                out.push_str("  - ");
                out.push_str(name);
                out.push('\n');
            }
            if group.len() > per_group {
                out.push_str(&format!(
                    "  … and {} more {}\n",
                    group.len() - per_group,
                    label.to_lowercase()
                ));
            }
        }
        if out.is_empty() {
            out.push_str(
                "(this ledger declares no accounts yet — use conventional names such as\
                 assets:cash, assets:bank, expenses:food, income:salary)\n",
            );
        }
        out
    }
}

/// The account a payment method named in the note most likely maps to.
pub fn payment_hint(note: &str, catalog: &AccountCatalog) -> Option<String> {
    if catalog.is_empty() {
        return None;
    }
    let lowered = note.to_lowercase();
    for (word, specific, generic) in PAYMENT_ALIASES {
        if !lowered.contains(&word.to_lowercase()) {
            continue;
        }
        if let Some(account) = catalog.find_with_tokens(specific) {
            return Some(account);
        }
        if let Some(account) = catalog.find_with_tokens(generic) {
            return Some(account);
        }
    }
    None
}

/// Nudge the model's draft onto accounts that actually exist, and let the note
/// override the payment method ("用微信付的" beats whatever the model guessed).
///
/// Anything that cannot be grounded is left alone: the engine then falls back to
/// a rule or to `expenses:unclassified` instead of inventing a silent account.
pub fn ground_draft(draft: &mut Draft, note: &str, catalog: &AccountCatalog) {
    if catalog.is_empty() {
        return;
    }
    if draft.direction == "expense" {
        if let Some(hint) = payment_hint(note, catalog) {
            draft.funding = Some(hint);
        }
    }
    for field in [
        &mut draft.category,
        &mut draft.transfer_to,
        &mut draft.funding,
    ] {
        if let Some(resolved) = field.as_deref().and_then(|name| catalog.resolve(name)) {
            *field = Some(resolved);
        }
    }
}

/// A question turned into a structured query. The model only chooses fields;
/// `commands::llm_ask` validates every one of them against the open ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanDraft {
    pub metric: String,
    pub accounts: Vec<String>,
    pub tags: Vec<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub group_by: Option<String>,
    pub limit: Option<usize>,
    pub min_amount: Option<String>,
}

const PLAN_METRICS: [&str; 5] = ["spend", "income", "net", "balance", "count"];
const PLAN_GROUPS: [&str; 3] = ["month", "account", "tag"];

/// Prompt for the read-only question path: the answer is a plan, not prose.
pub fn plan_system_prompt(
    today: &str,
    base_commodity: &str,
    catalog: &AccountCatalog,
    context: &str,
) -> String {
    let accounts = catalog.render(60);
    format!(
        r#"You turn a bookkeeping question into a JSON query plan for a plain-text ledger.
Today is {today}. The base commodity is {base_commodity}.

What the ledger looks like right now (computed by the engine — use it to choose the right window):
{context}

Return ONE JSON object and nothing else (no prose, no code fences):
{{"metric":"spend|income|net|balance|count",
  "accounts":["expenses:food"],
  "tags":["trip"],
  "date_from":"YYYY-MM or YYYY-MM-DD",
  "date_to":"YYYY-MM or YYYY-MM-DD",
  "group_by":"month|account|tag",
  "limit":5,
  "min_amount":"100"}}

Rules:
- metric spend = expenses, income = income, net = income minus expenses, balance = asset and
  liability balances, count = number of matching transactions.
- Use ONLY accounts from the list below. Pick the narrowest account that matches; an account
  prefix is allowed and means "this subtree".
- Omit a key (or use null) when the question does not mention it. Never guess a date range for
  "本月"/"今年" — resolve it from today's date yourself.
- "哪个月花得最多" means metric spend, group_by month. "哪类花得最多" means group_by account.
- limit defaults to 12 and must be between 1 and 50.
- min_amount is a decimal string, only when the question asks about large transactions.
- Default the window to the last 12 months. Use a single month or day ONLY when the question
  says so explicitly ("本月"/"今天"/"这个月").
- Questions like "能不能花 5000 买 X"/"够不够"/"有没有压力" are affordability questions, not
  category lookups: prefer metric balance (cash and debts), or metric spend grouped by month over
  12 months, and do not narrow to one account unless the question names a category.

Accounts in this ledger:
{accounts}
"#
    )
}

/// Parse and lightly normalise a plan. Anything unknown is dropped rather than
/// rejected, so a chatty model still produces a usable query.
pub fn plan_from_json(value: &Value) -> Result<PlanDraft, String> {
    let object = value.as_object().ok_or("expected a JSON object")?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty() && *text != "null")
            .map(str::to_string)
    };
    let list = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| {
                        item.trim_matches(|c| c == '+' || c == ':' || c == '#')
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let metric = text("metric")
        .map(|m| m.to_lowercase())
        .filter(|m| PLAN_METRICS.contains(&m.as_str()))
        .unwrap_or_else(|| "spend".to_string());
    let group_by = text("group_by")
        .map(|g| g.to_lowercase())
        .filter(|g| PLAN_GROUPS.contains(&g.as_str()));
    let limit = object
        .get("limit")
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64().map(|n| n as usize),
            Value::String(text) => text.trim().parse::<usize>().ok(),
            _ => None,
        })
        .map(|limit| limit.clamp(1, 50));

    Ok(PlanDraft {
        metric,
        accounts: list("accounts"),
        tags: list("tags"),
        date_from: text("date_from"),
        date_to: text("date_to"),
        group_by,
        limit,
        min_amount: text("min_amount"),
    })
}

/// Second call, only when the user wants prose: describe the numbers we computed.
pub fn answer_prompt(question: &str, facts: &str, context: &str) -> String {
    format!(
        "Question: {question}\n\nQuery result (verified numbers from the ledger):\n{facts}\n\nLedger overview (computed by the engine, equally authoritative):\n{context}\n\nWrite one short paragraph (2-3 sentences, Chinese) answering the question using ONLY these numbers. Do not invent figures, accounts or dates. If the query result is empty but the overview answers it, answer from the overview and say which figure you used. If neither has the number, say so plainly."
    )
}

/// The system prompt: the model returns one JSON object and nothing else.
pub fn system_prompt(today: &str, base_commodity: &str, catalog: &AccountCatalog) -> String {
    let list = catalog.render(80);

    fn example(
        today: &str,
        payee: &str,
        amount: &str,
        direction: &str,
        category: Option<&str>,
        funding: &str,
        transfer_to: Option<&str>,
    ) -> String {
        // Written by hand so the keys keep the same order as the schema above.
        let text = |value: &str| format!("\"{value}\"");
        let maybe = |value: Option<&str>| match value {
            Some(value) => text(value),
            None => "null".to_string(),
        };
        format!(
            concat!(
                "{{\"date\":{},\"payee\":{},\"amount\":{},\"direction\":{},",
                "\"category\":{},\"funding\":{},\"transfer_to\":{},",
                "\"commodity\":null,\"tags\":[],\"confidence\":0.9,\"notes\":null}}"
            ),
            text(today),
            text(payee),
            text(amount),
            text(direction),
            maybe(category),
            text(funding),
            maybe(transfer_to),
        )
    }

    // Few-shot examples use this ledger's own spellings, so the model copies real
    // accounts instead of inventing plausible ones. An example is only shown when
    // the ledger has an account that actually fits it.
    let pick = |options: &[String], fallback: &str| -> String {
        options
            .first()
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    let ex_expense = pick(&catalog.expenses, "expenses:food");
    let ex_income = pick(&catalog.income, "income:salary");
    let ex_wallet =
        payment_hint("微信", catalog).unwrap_or_else(|| pick(&catalog.assets, "assets:wallet"));
    let ex_bank = catalog
        .assets
        .iter()
        .find(|account| {
            ["bank", "招商", "checking", "储蓄"]
                .iter()
                .any(|token| account.contains(token))
        })
        .cloned()
        .unwrap_or_else(|| ex_wallet.clone());
    let ex_cash = catalog
        .assets
        .iter()
        .find(|account| account.contains("cash") || account.contains("现金"))
        .cloned()
        .unwrap_or_else(|| ex_wallet.clone());

    let mut examples = vec![
        (
            "昨天和同事在星巴克花了45，用微信付的",
            example(
                today,
                "星巴克",
                "45",
                "expense",
                Some(&ex_expense),
                &ex_wallet,
                None,
            ),
        ),
        (
            "工资到账 12000",
            example(
                today,
                "工资",
                "12000",
                "income",
                Some(&ex_income),
                &ex_bank,
                None,
            ),
        ),
    ];
    if ex_bank != ex_cash {
        examples.push((
            "从银行卡取现 500",
            example(
                today,
                "取现",
                "500",
                "transfer",
                None,
                &ex_bank,
                Some(&ex_cash),
            ),
        ));
    }
    let card = catalog.liabilities.iter().find(|account| {
        ["credit", "card", "信用卡", "花呗"]
            .iter()
            .any(|token| account.contains(token))
    });
    if let Some(card) = card {
        examples.push((
            "用招行还信用卡 2400",
            example(
                today,
                "还信用卡",
                "2400",
                "transfer",
                None,
                &ex_bank,
                Some(card),
            ),
        ));
        examples.push((
            "便利店买水 12，刷的信用卡",
            example(
                today,
                "便利店",
                "12",
                "expense",
                Some(&ex_expense),
                card,
                None,
            ),
        ));
    }
    let examples = examples
        .into_iter()
        .map(|(note, draft)| format!("note: {note}\n{draft}"))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r##"You turn one short bookkeeping note (usually Chinese) into a single double-entry journal entry
for a plain-text ledger.

Today is {today}. The base currency is {base_commodity}.

Accounts that already exist in this ledger — use these spellings exactly:
{list}
Reply with ONLY a JSON object, no prose, no code fence:
{{"date":"YYYY-MM-DD","payee":"...","amount":"123.45","direction":"expense|income|transfer","category":"expenses:food","funding":"assets:wallet","transfer_to":null,"commodity":null,"tags":["..."],"confidence":0.0,"notes":"..."}}

Field rules:
- "expense": money leaves an account to buy something; category MUST start with "expenses:".
- "income": money arrives; category MUST start with "income:".
- "transfer": money moves between two of the user's own accounts; leave category null and put the destination in transfer_to.
- "funding" is where the money comes from (expense) or goes to (income).
- "amount" is a positive decimal string, no currency symbol, no thousands separator.
- "commodity" is a 3-letter code only when the note names a foreign currency, otherwise null — never "NONE".
- "date" is the date the note mentions (relative words are relative to today), otherwise {today}.
- "tags" are short labels without "#" or ":".
- "confidence" is 0..1: how sure you are that the category is right.
- "notes" is one short sentence explaining an unusual choice, or null.

Account rules (the most common mistake is inventing accounts):
1. Pick names from the list above. Do NOT add extra segments: if "assets:wallet" is listed and
   "assets:wallet:wechat" is not, answer "assets:wallet" instead of inventing the sub-account.
2. If the note names a payment method, the funding account must be the listed account for that
   method (微信/wechat, 支付宝/alipay, 现金/cash, 信用卡/credit, 储蓄卡/debit). If only a parent
   such as "assets:wallet" exists, that parent is the correct answer.
3. If nothing in the list fits, use a conventional name (assets:cash, expenses:unclassified)
   rather than a deep invented path.
4. Category granularity follows the list: prefer the shorter listed account when both exist.

Examples from this ledger:
{examples}"##
    )
}

pub fn user_prompt(text: &str) -> String {
    format!("Note: {}", text.trim())
}

/// Request body for the configured protocol.
pub fn request_body(config: &LlmConfig, system: &str, user: &str) -> Value {
    match config.protocol {
        Protocol::OpenAi => json!({
            "model": config.model,
            "temperature": 0,
            "max_tokens": config.max_tokens,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }),
        Protocol::Anthropic => json!({
            "model": config.model,
            "max_tokens": config.max_tokens,
            "temperature": 0,
            "system": system,
            "messages": [{"role": "user", "content": user}],
        }),
    }
}

/// Assistant text out of a provider response.
pub fn extract_text(protocol: Protocol, body: &Value) -> Result<String, String> {
    let text = match protocol {
        Protocol::OpenAi => {
            let content = body.pointer("/choices/0/message/content").ok_or_else(|| {
                provider_error(body)
                    .unwrap_or_else(|| "响应里没有 choices[0].message.content".into())
            })?;
            match content {
                // Some gateways return content as an array of parts.
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(""),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            }
        }
        Protocol::Anthropic => body
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| provider_error(body).unwrap_or_else(|| "响应里没有 content".into()))?
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("模型返回了空内容。推理模型（gemma/nemotron 之类）的思考过程不算正文，\n把 max_tokens 调大或者换一个非推理模型；顾问也要多花几轮对话，模型最好在 4B 以上。".into());
    }
    Ok(text)
}

fn provider_error(body: &Value) -> Option<String> {
    let message = body.pointer("/error/message").and_then(Value::as_str)?;
    Some(format!("模型返回错误：{message}"))
}

/// Pull the JSON object out of a reply that may be wrapped in prose or fences.
pub fn extract_json(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    let candidate = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        let start = trimmed.find('{').ok_or("模型没有返回 JSON 对象")?;
        let end = trimmed.rfind('}').ok_or("模型返回的 JSON 不完整")?;
        if end <= start {
            return Err("模型返回的 JSON 不完整".into());
        }
        trimmed[start..=end].to_string()
    };
    serde_json::from_str(&candidate).map_err(|e| format!("无法解析模型返回的 JSON：{e}"))
}

/// `"¥1,234.56"` → `"1234.56"`, `"12,30"` → `"12.30"`.
///
/// A lone comma with one or two trailing digits is a decimal separator, every
/// other comma is a thousands separator; anything else is rejected rather than
/// guessed (getting 100× wrong silently is worse than failing).
fn normalise_amount(raw: &str) -> Option<String> {
    let stripped: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == '-')
        .collect();
    if stripped.is_empty() || stripped.starts_with('-') || stripped.matches('-').count() > 0 {
        return None;
    }
    let commas = stripped.matches(',').count();
    let dots = stripped.matches('.').count();
    let candidate = if commas == 0 {
        stripped
    } else if dots == 0 && commas == 1 {
        let (int_part, frac) = stripped.split_once(',')?;
        if matches!(frac.len(), 1 | 2) {
            format!("{int_part}.{frac}")
        } else {
            stripped.replace(',', "")
        }
    } else {
        stripped.replace(',', "")
    };
    if candidate.is_empty()
        || !candidate.chars().all(|c| c.is_ascii_digit() || c == '.')
        || candidate.matches('.').count() > 1
        || candidate.starts_with('.')
    {
        return None;
    }
    Some(candidate)
}

/// One drafted entry, straight from the model's JSON.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub date: Option<String>,
    pub payee: String,
    pub amount: String,
    pub direction: String,
    pub category: Option<String>,
    pub funding: Option<String>,
    pub transfer_to: Option<String>,
    pub commodity: Option<String>,
    pub tags: Vec<String>,
    pub confidence: Option<f64>,
    pub notes: Option<String>,
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// `"NONE"`, `"null"` or `"¥"` are not currencies — the model likes to fill
/// this field with placeholders even when told to use null.
fn clean_commodity(raw: &str) -> Option<String> {
    let token = raw.trim().trim_matches(['"', '\'']).to_ascii_uppercase();
    if token.is_empty() {
        return None;
    }
    let code = match token.as_str() {
        "NONE" | "NULL" | "NIL" | "NA" | "N/A" | "-" | "—" | "无" | "空" | "UNKNOWN" => {
            return None
        }
        "RMB" | "CNY" | "¥" | "￥" | "元" => "CNY",
        "$" | "US$" | "USD" => "USD",
        "€" | "EUR" => "EUR",
        "円" | "JPY" => "JPY",
        "HK$" | "HKD" => "HKD",
        "GBP£" | "GBP" => "GBP",
        other => other,
    };
    if code.len() < 3 || code.len() > 5 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(code.to_string())
}

/// `"45.0"` → `"45"`; a meaningful fraction such as `"45.00"` is left alone
/// (the ledger printer decides the final precision anyway).
fn tidy_amount(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('0').trim_end_matches('.');
    if raw.contains('.') && trimmed.parse::<u128>().is_err() {
        return raw.to_string();
    }
    if trimmed.is_empty() {
        return raw.to_string();
    }
    // Only drop the tail when the fraction is all zeros.
    match raw.split_once('.') {
        Some((_, fraction)) if !fraction.is_empty() && fraction.chars().all(|c| c == '0') => {
            trimmed.to_string()
        }
        _ => raw.to_string(),
    }
}

pub fn draft_from_json(value: &Value) -> Result<Draft, String> {
    let payee = string_field(value, "payee").ok_or("模型没有给出描述（payee）")?;
    let amount = string_field(value, "amount").ok_or("模型没有给出金额（amount）")?;
    let clean =
        normalise_amount(&amount).ok_or_else(|| format!("模型给出的金额无法识别：{amount}"))?;
    let direction = string_field(value, "direction")
        .unwrap_or_else(|| "expense".into())
        .to_ascii_lowercase();
    let tags = value
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|t| t.trim().trim_start_matches(['+', '#']).to_string())
                .filter(|t| !t.is_empty() && !t.contains(':'))
                .collect()
        })
        .unwrap_or_default();
    Ok(Draft {
        date: string_field(value, "date"),
        payee,
        amount: tidy_amount(clean.trim_start_matches('+')),
        direction,
        category: string_field(value, "category"),
        funding: string_field(value, "funding"),
        transfer_to: string_field(value, "transfer_to"),
        commodity: string_field(value, "commodity").and_then(|c| clean_commodity(&c)),
        tags,
        confidence: value.get("confidence").and_then(Value::as_f64),
        notes: string_field(value, "notes"),
    })
}

/// Turn the draft into one line of capture syntax.
pub fn synthesize_line(draft: &Draft, base_commodity: &str, today: &str) -> String {
    let mut line = String::new();
    if let Some(date) = draft.date.as_deref() {
        let date = date.trim();
        if !date.is_empty() && date != today {
            line.push('#');
            line.push_str(date);
            line.push(' ');
        }
    }
    line.push_str(draft.payee.trim());
    line.push(' ');

    let commodity = draft
        .commodity
        .as_deref()
        .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case(base_commodity));
    let amount = match commodity {
        Some(code) => format!("{}{}", draft.amount, code),
        None => draft.amount.clone(),
    };
    match draft.direction.as_str() {
        "income" => {
            line.push('+');
            line.push_str(&amount);
        }
        _ => line.push_str(&amount),
    }

    let transfer = draft.direction == "transfer";
    if transfer {
        let from = draft
            .funding
            .clone()
            .unwrap_or_else(|| "assets:cash".into());
        let to = draft
            .transfer_to
            .clone()
            .or_else(|| draft.category.clone())
            .unwrap_or_else(|| "assets:bank".into());
        line.push_str(&format!(" > @{from} > @{to}"));
    } else {
        if let Some(category) = draft.category.as_deref() {
            line.push_str(&format!(" @{}", category.trim()));
        }
        if let Some(funding) = draft.funding.as_deref() {
            line.push_str(&format!(" @{}", funding.trim()));
        }
    }
    for tag in &draft.tags {
        line.push_str(" +");
        line.push_str(tag);
    }
    line
}

/// A small ping used by 设置 → 测试连接.
pub fn test_prompt() -> (&'static str, &'static str) {
    (
        "You are a connectivity check. Reply with the single word OK.",
        "Reply with OK.",
    )
}

/// POST the request and return the assistant text plus how long it took.
pub fn complete(config: &LlmConfig, system: &str, user: &str) -> Result<(String, u128), String> {
    let body = request_body(config, system, user);
    let client = reqwest::blocking::Client::builder()
        .timeout(config.timeout)
        .build()
        .map_err(|e| format!("无法创建 HTTP 客户端：{e}"))?;
    let mut request = client.post(config.endpoint()).json(&body);
    request = match config.protocol {
        Protocol::OpenAi => {
            if config.api_key.is_empty() {
                request
            } else {
                request.bearer_auth(&config.api_key)
            }
        }
        Protocol::Anthropic => {
            let request = request
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json");
            if config.api_key.is_empty() {
                request
            } else {
                request.header("x-api-key", &config.api_key)
            }
        }
    };
    let started = Instant::now();
    let response = request
        .send()
        .map_err(|e| format!("请求失败：{e}（检查网络、Base URL 和代理）"))?;
    let status = response.status();
    let text = response.text().map_err(|e| format!("读取响应失败：{e}"))?;
    let elapsed = started.elapsed().as_millis();
    let parsed: Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "模型服务返回了非 JSON 响应（HTTP {status}）：{}",
            truncate(&text, 300)
        )
    })?;
    if !status.is_success() {
        let message = parsed
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(|m| m.to_string())
            .unwrap_or_else(|| truncate(&text, 300));
        return Err(format!("HTTP {}：{message}", status.as_u16()));
    }
    Ok((extract_text(config.protocol, &parsed)?, elapsed))
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(value: Value) -> Draft {
        draft_from_json(&value).expect("draft parses")
    }

    #[test]
    fn resolves_preset_defaults_and_overrides() {
        let cfg = LlmConfig::resolve("deepseek", "", "", "", "sk-x");
        assert_eq!(cfg.base_url, "https://api.deepseek.com/v1");
        assert_eq!(cfg.model, "deepseek-chat");
        assert_eq!(
            cfg.endpoint(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert!(cfg.is_ready());

        let custom =
            LlmConfig::resolve("custom", "anthropic", "http://box:8080/v1/", "my-model", "");
        assert_eq!(custom.protocol, Protocol::Anthropic);
        assert_eq!(custom.endpoint(), "http://box:8080/v1/messages");
        assert!(custom.is_ready(), "custom endpoints do not require a key");
    }

    #[test]
    fn normalises_pasted_endpoints() {
        let host_only =
            LlmConfig::resolve("custom", "openai", "http://192.168.110.82:1234", "m", "");
        assert_eq!(
            host_only.endpoint(),
            "http://192.168.110.82:1234/v1/chat/completions"
        );

        let full = LlmConfig::resolve(
            "custom",
            "openai",
            "http://box:1234/v1/chat/completions",
            "m",
            "",
        );
        assert_eq!(full.endpoint(), "http://box:1234/v1/chat/completions");

        let anthropic = LlmConfig::resolve("custom", "anthropic", "http://box:8080/v1/", "m", "");
        assert_eq!(anthropic.endpoint(), "http://box:8080/v1/messages");
    }

    #[test]
    fn reports_what_is_missing() {
        let cfg = LlmConfig::resolve("openai", "", "", "", "");
        assert_eq!(cfg.block_reason().as_deref(), Some("还没有填写 API Key"));
        let cfg = LlmConfig::resolve("custom", "", "", "", "");
        assert_eq!(cfg.block_reason().as_deref(), Some("还没有填写 Base URL"));
        let cfg = LlmConfig::resolve("ollama", "", "", "", "");
        assert!(cfg.is_ready(), "ollama needs no key");
    }

    #[test]
    fn builds_a_body_per_protocol() {
        let openai = LlmConfig::resolve("openai", "", "", "", "k");
        let body = request_body(&openai, "sys", "usr");
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body.get("system").is_none());

        let claude = LlmConfig::resolve("anthropic", "", "", "", "k");
        let body = request_body(&claude, "sys", "usr");
        assert_eq!(body["system"], "sys");
        assert_eq!(body["messages"][0]["content"], "usr");
        assert!(body.get("temperature").is_some());
    }

    #[test]
    fn reads_text_out_of_both_response_shapes() {
        let openai = json!({"choices":[{"message":{"content":"{\"payee\":\"咖啡\"}"}}]});
        assert_eq!(
            extract_text(Protocol::OpenAi, &openai).unwrap(),
            "{\"payee\":\"咖啡\"}"
        );
        let parts = json!({"choices":[{"message":{"content":[{"text":"a"},{"text":"b"}]}}]});
        assert_eq!(extract_text(Protocol::OpenAi, &parts).unwrap(), "ab");
        let claude = json!({"content":[{"type":"text","text":"hi"},{"type":"tool_use"}]});
        assert_eq!(extract_text(Protocol::Anthropic, &claude).unwrap(), "hi");
        let broken = json!({"error":{"message":"invalid api key"}});
        assert!(extract_text(Protocol::OpenAi, &broken)
            .unwrap_err()
            .contains("invalid api key"));
    }

    #[test]
    fn extracts_json_from_fenced_replies() {
        let fenced = "```json\n{\"payee\":\"咖啡\",\"amount\":\"4.50\"}\n```";
        assert_eq!(extract_json(fenced).unwrap()["payee"], "咖啡");
        let prose = "Sure! {\"payee\":\"茶\",\"amount\":\"3\"} hope that helps";
        assert_eq!(extract_json(prose).unwrap()["amount"], "3");
        assert!(extract_json("no json here").is_err());
    }

    #[test]
    fn sanitises_the_draft() {
        let d = draft(json!({
            "payee": " 星巴克 ",
            "amount": "¥45.00",
            "direction": "Expense",
            "category": "expenses:food:coffee",
            "funding": "assets:wallet:alipay",
            "commodity": "cny",
            "tags": ["+coffee", "#morning", "with:colon"],
            "confidence": 0.9,
        }));
        assert_eq!(d.payee, "星巴克");
        assert_eq!(d.amount, "45"); // pointless trailing zeros are dropped
        assert_eq!(d.commodity.as_deref(), Some("CNY"));
        assert_eq!(d.tags, vec!["coffee".to_string(), "morning".to_string()]);
        // A lone comma with two trailing digits is a decimal comma, not a group mark.
        assert_eq!(
            draft_from_json(&json!({"payee": "x", "amount": "12,30"}))
                .unwrap()
                .amount,
            "12.30"
        );
        assert_eq!(
            draft_from_json(&json!({"payee": "x", "amount": "¥1,234.56"}))
                .unwrap()
                .amount,
            "1234.56"
        );
        assert!(draft_from_json(&json!({"payee": "x", "amount": "12.3.4"})).is_err());
        assert!(draft_from_json(&json!({"payee": "x", "amount": "abc"})).is_err());
        assert!(draft_from_json(&json!({"amount": "1"})).is_err());
    }

    #[test]
    fn synthesises_capture_lines() {
        let today = "2026-10-25";
        let expense = draft(json!({
            "payee": "星巴克", "amount": "45", "direction": "expense",
            "category": "expenses:food:coffee", "funding": "assets:wallet:alipay",
            "tags": ["coffee"],
        }));
        assert_eq!(
            synthesize_line(&expense, "CNY", today),
            "星巴克 45 @expenses:food:coffee @assets:wallet:alipay +coffee"
        );

        let income = draft(json!({
            "payee": "工资", "amount": "12000", "direction": "income",
            "category": "income:salary", "funding": "assets:cash",
        }));
        assert_eq!(
            synthesize_line(&income, "CNY", today),
            "工资 +12000 @income:salary @assets:cash"
        );

        let transfer = draft(json!({
            "payee": "取现", "amount": "500", "direction": "transfer",
            "funding": "assets:bank:checking", "transfer_to": "assets:cash",
        }));
        assert_eq!(
            synthesize_line(&transfer, "CNY", today),
            "取现 500 > @assets:bank:checking > @assets:cash"
        );

        let dated = draft(json!({
            "payee": "昨天午饭", "amount": "32", "direction": "expense",
            "date": "2026-10-24", "category": "expenses:food",
            "funding": "assets:cash", "commodity": "USD",
        }));
        assert_eq!(
            synthesize_line(&dated, "CNY", today),
            "#2026-10-24 昨天午饭 32USD @expenses:food @assets:cash"
        );
        // The base commodity is never written out.
        assert!(!synthesize_line(&dated, "USD", today).contains("32USD"));
    }

    #[test]
    fn prompt_lists_accounts_by_kind_and_today() {
        let catalog = AccountCatalog::from_names([
            "assets:cash",
            "assets:wallet",
            "liabilities:creditcard",
            "expenses:food",
            "income:salary",
        ]);
        let prompt = system_prompt("2026-10-25", "CNY", &catalog);
        assert!(prompt.contains("Today is 2026-10-25"));
        assert!(prompt.contains("Assets:\n  - assets:cash\n  - assets:wallet"));
        assert!(prompt.contains("Liabilities:\n  - liabilities:creditcard"));
        assert!(prompt.contains("Income:\n  - income:salary"));
        assert!(prompt.contains("Expenses:\n  - expenses:food"));
        assert!(prompt.contains("\"direction\""));
        assert!(prompt.contains("Examples"));
        assert!(prompt.contains("Do NOT add extra segments"));
        let empty = system_prompt("2026-10-25", "CNY", &AccountCatalog::default());
        assert!(empty.contains("conventional names"));
    }

    #[test]
    fn prompt_uses_real_accounts_and_skips_examples_that_do_not_fit() {
        let catalog = AccountCatalog::from_names([
            "assets:现金",
            "assets:现金:微信钱包",
            "assets:现金:招商银行",
            "liabilities:商业房贷",
            "income:工资收入",
            "expenses:日常",
        ]);
        let prompt = system_prompt("2026-10-25", "CNY", &catalog);
        // Bare tree roots are never offered as postable accounts.
        assert!(!prompt.contains("  - assets\n"));
        assert!(prompt.contains("assets:现金:微信钱包"));
        // The wallet example uses the account the note's payment method maps to.
        assert!(prompt.contains("\"funding\":\"assets:现金:微信钱包\""));
        assert!(prompt.contains("\"category\":\"expenses:日常\""));
        // No credit card in this ledger, so no credit-card example either — the
        // mortgage still shows up in the account list, just not as a card.
        let examples = prompt.split("Examples from this ledger:").nth(1).unwrap();
        assert!(!examples.contains("还信用卡"));
        assert!(!examples.contains("商业房贷"));
        assert!(!examples.contains("liabilities"));
    }

    #[test]
    fn catalog_resolves_exact_suffix_and_ancestors() {
        let catalog = AccountCatalog::from_names([
            "assets:bank:cmb",
            "assets:wallet:wechat",
            "assets:wallet",
            "expenses:food:coffee",
            "expenses:food",
        ]);
        // exact
        assert_eq!(
            catalog.resolve("assets:bank:cmb").as_deref(),
            Some("assets:bank:cmb")
        );
        // unique suffix
        assert_eq!(
            catalog.resolve("bank:cmb").as_deref(),
            Some("assets:bank:cmb")
        );
        // invented sub-account collapses onto its existing parent
        assert_eq!(
            catalog.resolve("assets:wallet:alipay").as_deref(),
            Some("assets:wallet")
        );
        assert_eq!(
            catalog.resolve("Expenses:Food:Treats").as_deref(),
            Some("expenses:food")
        );
        // keyword match through the payment table
        assert_eq!(catalog.resolve("招行").as_deref(), Some("assets:bank:cmb"));
        assert_eq!(catalog.resolve("expenses:travel").as_deref(), None);
    }

    #[test]
    fn payment_hint_prefers_the_method_named_in_the_note() {
        let both = AccountCatalog::from_names([
            "assets:wallet:wechat",
            "assets:wallet:alipay",
            "assets:cash",
        ]);
        assert_eq!(
            payment_hint("昨天在星巴克花了45，用微信付的", &both).as_deref(),
            Some("assets:wallet:wechat")
        );
        assert_eq!(
            payment_hint("打车 30 支付宝", &both).as_deref(),
            Some("assets:wallet:alipay")
        );
        // Only a generic wallet exists: the parent is the right answer.
        let generic = AccountCatalog::from_names(["assets:wallet", "assets:cash"]);
        assert_eq!(
            payment_hint("昨天星巴克 45 微信", &generic).as_deref(),
            Some("assets:wallet")
        );
        assert_eq!(payment_hint("午饭 12", &generic), None);
    }

    #[test]
    fn grounds_the_draft_that_caused_the_wrong_wallet_bug() {
        let catalog = AccountCatalog::from_names([
            "assets:cash",
            "assets:wallet",
            "expenses:food",
            "expenses:food:coffee",
        ]);
        let mut grounded = draft(json!({
            "payee": "微信付款", "amount": "45", "direction": "expense",
            "category": "expenses:food:coffee", "funding": "assets:wallet:alipay",
            "commodity": "NONE",
        }));
        ground_draft(
            &mut grounded,
            "昨天和同事在星巴克花了45，用微信付的",
            &catalog,
        );
        assert_eq!(grounded.funding.as_deref(), Some("assets:wallet"));
        assert_eq!(grounded.category.as_deref(), Some("expenses:food:coffee"));
        assert_eq!(grounded.commodity, None);
        assert_eq!(
            synthesize_line(&grounded, "CNY", "2026-10-25"),
            "微信付款 45 @expenses:food:coffee @assets:wallet"
        );

        // The shape of the vault from the bug report: no wallet sub-accounts and
        // no coffee leaf, so everything collapses onto what is declared.
        let real = AccountCatalog::from_names([
            "assets:cash",
            "assets:bank",
            "assets:wallet",
            "liabilities:creditcard",
            "income:salary",
            "expenses:food",
            "expenses:unclassified",
            "equity:opening",
        ]);
        let mut mirrored = draft(json!({
            "payee": "微信付款", "amount": "45.0", "direction": "expense",
            "category": "expenses:food:coffee", "funding": "assets:wallet:alipay",
            "commodity": "NONE",
        }));
        ground_draft(&mut mirrored, "昨天和同事在星巴克花了45，用微信付的", &real);
        assert_eq!(
            synthesize_line(&mirrored, "CNY", "2026-10-25"),
            "微信付款 45 @expenses:food @assets:wallet"
        );
    }

    #[test]
    fn plan_prompt_names_the_schema_and_the_ledger_accounts() {
        let catalog =
            AccountCatalog::from_names(["assets:cash", "expenses:food:coffee", "income:salary"]);
        let prompt = plan_system_prompt(
            "2026-10-25",
            "CNY",
            &catalog,
            "净资产 100,000.00 CNY，现金 41,000.00 CNY，月支出 1,000.00 CNY，储备 41.0 个月",
        );
        assert!(prompt.contains("Today is 2026-10-25"));
        assert!(prompt.contains("expenses:food:coffee"));
        assert!(prompt.contains("group_by"));
        assert!(prompt.contains("month|account|tag"));
        // 没有上下文的计划会把「能不能花 5000」查成 expenses:food 的一天空窗。
        assert!(prompt.contains("储备 41.0 个月"));
        assert!(prompt.contains("Default the window to the last 12 months"));
    }

    #[test]
    fn plan_json_keeps_known_fields_and_drops_the_rest() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"metric":"SPEND","accounts":["expenses:food","+expenses:travel"],
                "tags":[":trip:"],"date_from":"2026-01","date_to":"2026-10",
                "group_by":"Account","limit":999,"min_amount":"100","unknown":true}"#,
        )
        .unwrap();
        let plan = plan_from_json(&value).unwrap();
        assert_eq!(plan.metric, "spend");
        assert_eq!(plan.accounts, vec!["expenses:food", "expenses:travel"]);
        assert_eq!(plan.tags, vec!["trip"]);
        assert_eq!(plan.group_by.as_deref(), Some("account"));
        assert_eq!(plan.limit, Some(50));
        assert_eq!(plan.min_amount.as_deref(), Some("100"));
    }

    #[test]
    fn plan_json_falls_back_to_spend_and_ignores_bad_enums() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"metric":"vibes","group_by":"colour"}"#).unwrap();
        let plan = plan_from_json(&value).unwrap();
        assert_eq!(plan.metric, "spend");
        assert_eq!(plan.group_by, None);
        assert_eq!(plan.limit, None);
        assert!(plan.accounts.is_empty());
    }

    #[test]
    fn plan_json_rejects_non_objects() {
        let value: serde_json::Value = serde_json::from_str(r#"["spend"]"#).unwrap();
        assert!(plan_from_json(&value).is_err());
    }

    #[test]
    fn answer_prompt_carries_only_the_computed_numbers() {
        let prompt = answer_prompt(
            "今年餐饮花了多少",
            "支出合计 2,200.00 CNY",
            "现金 41,000.00 CNY，月支出 1,000.00 CNY",
        );
        assert!(prompt.contains("今年餐饮花了多少"));
        assert!(prompt.contains("2,200.00 CNY"));
        assert!(prompt.contains("现金 41,000.00 CNY"));
        assert!(prompt.contains("ONLY"));
    }

    #[test]
    fn placeholders_are_not_commodities() {
        for junk in ["NONE", "null", "n/a", "无", "—", "", "元?"] {
            assert_eq!(clean_commodity(junk), None, "{junk}");
        }
        assert_eq!(clean_commodity("usd").as_deref(), Some("USD"));
        assert_eq!(clean_commodity("$").as_deref(), Some("USD"));
        assert_eq!(clean_commodity("RMB").as_deref(), Some("CNY"));
    }

    #[test]
    fn sloppy_amounts_are_tidied() {
        assert_eq!(tidy_amount("45.0"), "45");
        assert_eq!(tidy_amount("45.00"), "45");
        assert_eq!(tidy_amount("45.50"), "45.50");
        assert_eq!(tidy_amount("3200"), "3200");
        assert_eq!(tidy_amount("nope"), "nope");
    }
}

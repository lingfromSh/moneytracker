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
            max_tokens: 700,
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

/// The system prompt: the model returns one JSON object and nothing else.
pub fn system_prompt(today: &str, base_commodity: &str, accounts: &[String]) -> String {
    let mut list = String::new();
    for account in accounts.iter().take(120) {
        list.push_str("  - ");
        list.push_str(account);
        list.push('\n');
    }
    if list.is_empty() {
        list.push_str("  (the ledger has no accounts yet, invent sensible ones)\n");
    }
    format!(
        r##"You turn one short bookkeeping note (usually Chinese) into a single double-entry journal entry.

Today is {today}. The base currency is {base_commodity}.

Reply with ONLY a JSON object, no prose, no code fence:
{{"date":"YYYY-MM-DD","payee":"...","amount":"123.45","direction":"expense|income|transfer","category":"expenses:food:coffee","funding":"assets:wallet:alipay","transfer_to":null,"commodity":null,"tags":["..."],"confidence":0.0,"notes":"..."}}

Rules:
- "expense": money leaves an asset to buy something; category MUST start with "expenses:".
- "income": money arrives; category MUST start with "income:".
- "transfer": money moves between two assets; leave category null and put the destination asset in transfer_to.
- "funding" is the account the money comes from (expense) or goes to (income), usually an "assets:" account.
- Reuse the accounts below verbatim whenever one fits; only invent an account when nothing does:
{list}- "amount" is a positive decimal string without currency symbols or thousands separators.
- "commodity" is a 3-letter code only when the note names a foreign currency, otherwise null.
- "date" is the date the note mentions (relative words are relative to today), otherwise {today}.
- "tags" are short labels without "#" or ":".
- "confidence" is 0..1: how sure you are that the category is right.
- Keep "notes" to one short sentence explaining an unusual choice, or null."##
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
        return Err("模型返回了空内容".into());
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
        amount: clean.trim_start_matches('+').to_string(),
        direction,
        category: string_field(value, "category"),
        funding: string_field(value, "funding"),
        transfer_to: string_field(value, "transfer_to"),
        commodity: string_field(value, "commodity").map(|c| c.to_ascii_uppercase()),
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
        assert_eq!(d.amount, "45.00");
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
    fn prompt_lists_accounts_and_today() {
        let prompt = system_prompt(
            "2026-10-25",
            "CNY",
            &["assets:cash".into(), "expenses:food".into()],
        );
        assert!(prompt.contains("Today is 2026-10-25"));
        assert!(prompt.contains("assets:cash"));
        assert!(prompt.contains("expenses:food"));
        assert!(prompt.contains("\"direction\""));
        let empty = system_prompt("2026-10-25", "CNY", &[]);
        assert!(empty.contains("invent sensible ones"));
    }
}

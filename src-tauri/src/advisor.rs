//! 只读财务顾问。
//!
//! 和「问一句」同一个底线：模型只负责决定**看什么**和**怎么说**，每个数字都由引擎算出来。
//! 区别在于这次它拿到的是账本的全景（净资产、现金流、FIRE 目标、债务、预算、日程），
//! 并且可以多轮地要更多数字 —— 所以「这个月能不能花 5000 买手机」这种问题才有答案。

use std::path::Path;

use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

use ledger_engine::types::{AmountDto, PlanOverviewDto};
use ledger_engine::{run_plan, schedule, QueryPlan, Vault};

use crate::llm::{self, LlmConfig};

/// 会话文件的文件名，落在 App 配置目录里（`0600`），不进账本、不参与同步。
pub const FILE: &str = "advisor.json";
/// 送去模型的历史最多这么多条，再老的不带。
pub const MAX_TURNS: usize = 24;
/// 一轮问答最多让模型调这么多轮工具。
pub const MAX_ROUNDS: u32 = 4;
/// 汇总的时间窗口（月）。
const MONTHS: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AdvisorTurnDto {
    /// `user` | `assistant`
    pub role: String,
    pub content: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AdvisorToolDto {
    pub tool: String,
    pub input: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AdvisorReplyDto {
    pub content: String,
    pub tools: Vec<AdvisorToolDto>,
    pub rounds: u32,
    #[specta(type = f64)]
    pub context_chars: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AdvisorHistoryDto {
    pub turns: Vec<AdvisorTurnDto>,
    pub updated_at: Option<String>,
}

/// 账本全景。`headline` 是给「查询计划」用的几行摘要，`full` 给顾问和回答用。
pub struct Brief {
    pub headline: String,
    pub full: String,
}

fn money(amount: &AmountDto) -> String {
    amount.display.clone()
}

fn percent(value: f64) -> String {
    format!("{value:.1}%")
}

/// 把账本压成一段给模型看的文字。只读：这里没有任何写操作，也不会泄漏交易明细以外的东西。
pub fn brief(vault: &Vault, today: NaiveDate) -> Brief {
    let plan: PlanOverviewDto = ledger_engine::plan_overview(vault);
    let dash = ledger_engine::report::dashboard(vault, MONTHS);
    let budgets = ledger_engine::report::budgets(vault);
    let sched = schedule::overview(vault, 3, today);

    let (base, _) = vault.base_commodity();
    let mut full = String::new();

    full.push_str(&format!(
        "今天 {today}，基准币种 {base}，账本 {} 笔交易，最近一次记账 {}\n",
        vault.transactions().len(),
        dash.kpis.month
    ));
    full.push_str(&format!(
        "净资产 {}（本月收入 {}、支出 {}、储蓄率 {}）\n",
        money(&dash.kpis.net_worth),
        money(&dash.kpis.month_income),
        money(&dash.kpis.month_expense),
        percent(dash.kpis.savings_rate)
    ));

    full.push_str(&format!(
        "\n【财富自由】年支出 {}，可投资资产 {}，进度 {}，月结余 {}，实际收益率 {}\n",
        money(&plan.fire.annual_expense),
        money(&plan.fire.balance),
        percent(plan.fire.progress),
        money(&plan.fire.contribution),
        percent(plan.fire.real_return)
    ));
    for target in &plan.fire.targets {
        full.push_str(&format!(
            "  取 {}% 时目标 {}，{}（{}）\n",
            target.swr,
            money(&target.target),
            if target.achieved {
                "已达成"
            } else {
                "还差"
            },
            target.eta_label
        ));
    }
    if let Some(note) = plan.fire.note.as_deref() {
        full.push_str(&format!("  备注：{note}\n"));
    }

    full.push_str(&format!(
        "\n【现金与储备】现金 {}，月支出 {}，储备 {:.1} 个月（目标 {}）\n",
        money(&plan.reserve.cash),
        money(&plan.reserve.monthly_expense),
        plan.reserve.months,
        plan.reserve.target_months
    ));

    if !plan.pool.is_empty() {
        full.push_str("\n【可投资资产明细】\n");
        for member in &plan.pool {
            full.push_str(&format!("  {} {}\n", member.account, money(&member.amount)));
        }
    }

    if !plan.debts.is_empty() {
        full.push_str("\n【负债】\n");
        for debt in &plan.debts {
            full.push_str(&format!(
                "  {} 余额 {}{}{}\n",
                debt.label,
                money(&debt.balance),
                debt.rate
                    .map(|rate| format!("，利率 {rate}%"))
                    .unwrap_or_default(),
                debt.payoff_date
                    .as_deref()
                    .map(|date| format!("，按当前月供 {date} 还清"))
                    .unwrap_or_default()
            ));
        }
    }

    full.push_str("\n【逐月收支】\n");
    for point in dash.months.iter().rev().take(MONTHS) {
        full.push_str(&format!(
            "  {} 收入 {} 支出 {} 结余 {}\n",
            point.month, point.income_display, point.expense_display, point.net_display
        ));
    }

    let from = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
        .map(|date| shift_months(date, -(MONTHS as i32)))
        .unwrap_or(today);
    let categories = run_plan(
        vault,
        &QueryPlan {
            metric: "spend".into(),
            accounts: Vec::new(),
            tags: Vec::new(),
            date_from: Some(from.format("%Y-%m").to_string()),
            date_to: None,
            group_by: Some("account".into()),
            limit: 8,
            min_amount: None,
        },
    );
    if !categories.facts.is_empty() {
        full.push_str(&format!(
            "\n【近 {MONTHS} 个月支出前八】{}\n",
            categories.summary
        ));
        for fact in &categories.facts {
            full.push_str(&format!("  {} {}\n", fact.label, fact.display));
        }
    }

    if !budgets.is_empty() {
        full.push_str("\n【预算（每月）】\n");
        for row in &budgets {
            full.push_str(&format!("  {} {}\n", row.account, money(&row.amount)));
        }
    }

    if sched.has_schedule && !sched.templates.is_empty() {
        full.push_str("\n【固定收支日程】\n");
        for template in &sched.templates {
            full.push_str(&format!(
                "  {}（{}）下次 {}，收入 {}、支出 {}、到手 {}\n",
                template.label,
                template.period,
                template.next_date.as_deref().unwrap_or("—"),
                money(&template.income),
                money(&template.expense),
                money(&template.settle)
            ));
        }
    }

    let recent: Vec<String> = vault
        .transactions()
        .iter()
        .rev()
        .take(8)
        .map(|record| {
            let legs: Vec<String> = record
                .txn
                .postings
                .iter()
                .filter_map(|posting| {
                    let amount = posting.amount.as_ref()?;
                    Some(format!("{} {}", posting.account, amount.format()))
                })
                .collect();
            format!(
                "  {} {}：{}",
                record.txn.date,
                record.txn.payee,
                legs.join(" / ")
            )
        })
        .collect();
    if !recent.is_empty() {
        full.push_str("\n【最近几笔】\n");
        for line in recent {
            full.push_str(&line);
            full.push('\n');
        }
    }

    let headline = format!(
        "今天 {today}。净资产 {}，现金 {}，月支出 {}，储备 {:.1} 个月（目标 {}）；可投资资产 {}，FIRE 进度 {}。\
         本月收入 {}，支出 {}。",
        money(&dash.kpis.net_worth),
        money(&plan.reserve.cash),
        money(&plan.reserve.monthly_expense),
        plan.reserve.months,
        plan.reserve.target_months,
        money(&plan.fire.balance),
        percent(plan.fire.progress),
        money(&dash.kpis.month_income),
        money(&dash.kpis.month_expense),
    );

    Brief { headline, full }
}

fn shift_months(date: NaiveDate, delta: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + delta;
    let year = total.div_euclid(12);
    let month = total.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(date)
}

const TOOL_DOCS: &str = r#"- query      {"metric":"spend|income|net|balance|count","accounts":["expenses:food"],"tags":[],"date_from":"YYYY-MM","date_to":"YYYY-MM","group_by":"month|account|tag","limit":12}
- breakdown  {"dimension":"account|month","date_from":"YYYY-MM","date_to":"YYYY-MM","limit":8}
- cashflow   {"months":12}
- goals      {}
- simulate   {"amount":"5000","kind":"expense"}   假设花掉（或赚到）这笔钱之后的现金、储备月数、FIRE 进度"#;

pub fn system_prompt(today: NaiveDate, brief: &str) -> String {
    format!(
        r#"你是 MoneyTracker 的私人财务顾问，只读：不能改账本，只能查数字、做算术、解释。

今天是 {today}。下面是从本地账本算出来的事实，是你唯一的数字来源：

{brief}

每轮只回一个 JSON 对象，不要写任何别的文字：
{{"action":"tool","tool":"<名称>","input":{{...}}}}     还需要数字时
{{"action":"answer","text":"..."}}                       能回答时

可用工具：
{TOOL_DOCS}

规则：
- 只能引用上面事实或工具结果里的数字，绝不自己估算或编造；缺数字就调工具，工具也给不出就直说没有。
- 用中文回答，2-4 句；先给结论，再给支撑它的两三个数字。
- 判断「能不能花这笔钱」时，按这个顺序看：现金与储备月数（目标 6 个月）→ 本月现金流 → 对 FIRE 进度的影响；结论里要把这几个数字写出来。
- 不承诺收益，不给投资建议，只给账本口径的算术结论。
"#
    )
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Tool { tool: String, input: Value },
    Answer(String),
    Invalid(String),
}

/// 宽容地读模型的动作：多余的键忽略，缺 `action` 时按「答案」处理。
pub fn parse_action(value: &Value) -> Action {
    let Some(object) = value.as_object() else {
        return Action::Invalid("期望一个 JSON 对象".into());
    };
    match object.get("action").and_then(Value::as_str) {
        Some("tool") => {
            let tool = object
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if tool.is_empty() {
                return Action::Invalid("调工具却没写 tool 名称".into());
            }
            let input = object.get("input").cloned().unwrap_or_else(|| json!({}));
            Action::Tool { tool, input }
        }
        Some("answer") => match object.get("text").and_then(Value::as_str) {
            Some(text) if !text.trim().is_empty() => Action::Answer(text.trim().to_string()),
            _ => Action::Invalid("回答是空的".into()),
        },
        Some(other) => Action::Invalid(format!("未知的 action：{other}")),
        None => match object.get("text").and_then(Value::as_str) {
            Some(text) if !text.trim().is_empty() => Action::Answer(text.trim().to_string()),
            _ => Action::Invalid("缺少 action".into()),
        },
    }
}

fn text_of(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn strings_of(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn plan_of(input: &Value, metric: &str, group_by: Option<&str>) -> QueryPlan {
    QueryPlan {
        metric: text_of(input, "metric").unwrap_or_else(|| metric.to_string()),
        accounts: strings_of(input, "accounts"),
        tags: strings_of(input, "tags"),
        date_from: text_of(input, "date_from"),
        date_to: text_of(input, "date_to"),
        group_by: group_by
            .map(str::to_string)
            .or_else(|| text_of(input, "group_by")),
        limit: input
            .get("limit")
            .and_then(Value::as_u64)
            .map(|value| value.clamp(1, 50) as usize)
            .unwrap_or(12),
        min_amount: input
            .get("min_amount")
            .and_then(Value::as_str)
            .and_then(|raw| raw.replace(',', "").parse().ok()),
    }
}

/// 执行一个工具。返回给模型看的纯文本；`Err` 说明参数不可用。
pub fn run_tool(
    vault: &Vault,
    tool: &str,
    input: &Value,
    today: NaiveDate,
) -> Result<String, String> {
    match tool {
        "query" => {
            let plan = plan_of(input, "spend", None);
            let result = run_plan(vault, &plan);
            Ok(ledger_engine::plan_markdown(tool, &result))
        }
        "breakdown" => {
            let dimension = text_of(input, "dimension").unwrap_or_else(|| "account".into());
            if !matches!(dimension.as_str(), "account" | "month" | "tag") {
                return Err(format!(
                    "dimension 只能是 account/month/tag，收到 {dimension}"
                ));
            }
            let plan = plan_of(input, "spend", Some(&dimension));
            let result = run_plan(vault, &plan);
            Ok(ledger_engine::plan_markdown(tool, &result))
        }
        "cashflow" => {
            let months = input
                .get("months")
                .and_then(Value::as_u64)
                .map(|value| value.clamp(1, 36) as usize)
                .unwrap_or(MONTHS);
            let dash = ledger_engine::report::dashboard(vault, months);
            let mut out =
                String::from("| 月份 | 收入 | 支出 | 结余 |\n| --- | --- | --- | --- |\n");
            for point in dash.months.iter().rev() {
                out.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    point.month, point.income_display, point.expense_display, point.net_display
                ));
            }
            Ok(out)
        }
        "goals" => {
            let plan = ledger_engine::plan_overview(vault);
            let mut out = String::new();
            out.push_str(&format!("年支出 {}\n", money(&plan.fire.annual_expense)));
            out.push_str(&format!(
                "可投资资产 {}／进度 {}\n",
                money(&plan.fire.balance),
                percent(plan.fire.progress)
            ));
            for target in &plan.fire.targets {
                out.push_str(&format!(
                    "取 {}% → {}（{}）\n",
                    target.swr,
                    money(&target.target),
                    target.eta_label
                ));
            }
            out.push_str(&format!(
                "储备 {:.1} 个月／目标 {}\n",
                plan.reserve.months, plan.reserve.target_months
            ));
            for debt in &plan.debts {
                out.push_str(&format!(
                    "{} 余额 {}{}\n",
                    debt.label,
                    money(&debt.balance),
                    debt.payoff_date
                        .as_deref()
                        .map(|date| format!("／{date} 还清"))
                        .unwrap_or_default()
                ));
            }
            Ok(out)
        }
        "simulate" => {
            let amount: f64 = text_of(input, "amount")
                .or_else(|| {
                    input
                        .get("amount")
                        .and_then(Value::as_f64)
                        .map(|value| value.to_string())
                })
                .ok_or("simulate 需要 amount")?
                .replace(',', "")
                .parse()
                .map_err(|_| "amount 不是数字".to_string())?;
            let kind = text_of(input, "kind").unwrap_or_else(|| "expense".into());
            let sign = if kind == "income" { 1.0 } else { -1.0 };
            let delta = amount * sign;

            let plan = ledger_engine::plan_overview(vault);
            let cash = plan.reserve.cash.value;
            let pool = plan.fire.balance.value;
            let monthly = plan.reserve.monthly_expense.value;
            let target = plan
                .fire
                .targets
                .iter()
                .find(|target| (target.swr - 4.0).abs() < 0.01)
                .or_else(|| plan.fire.targets.first())
                .map(|target| target.target.value)
                .unwrap_or(0.0);

            let cash_after = cash + delta;
            let pool_after = pool + delta;
            let months_after = if monthly.ne(&0.0) {
                cash_after / monthly
            } else {
                f64::NAN
            };
            let progress_after = if target.ne(&0.0) {
                pool_after / target * 100.0
            } else {
                0.0
            };

            let mut out = format!(
                "假设一笔 {amount:.2} 的{}（今天 {today}）：\n",
                if sign < 0.0 { "支出" } else { "收入" }
            );
            out.push_str(&format!(
                "现金 {cash:.2} → {cash_after:.2}；储备 {:.1} → {:.1} 个月（目标 {}）\n",
                if monthly.ne(&0.0) {
                    cash / monthly
                } else {
                    f64::NAN
                },
                months_after,
                plan.reserve.target_months
            ));
            out.push_str(&format!(
                "可投资资产 {pool:.2} → {pool_after:.2}；FIRE 进度 {:.1}% → {progress_after:.1}%\n",
                if target.ne(&0.0) {
                    pool / target * 100.0
                } else {
                    0.0
                }
            ));
            let reserve_ok = months_after >= plan.reserve.target_months as f64;
            out.push_str(&format!(
                "结论：{}（按可投资资产口径估算，未考虑税费与手续费）",
                if reserve_ok {
                    "储备仍在目标之上，现金流可以承受"
                } else {
                    "储备会跌破目标，先补现金再花这笔钱"
                }
            ));
            Ok(out)
        }
        other => Err(format!("没有这个工具：{other}")),
    }
}

/// 把历史压成一段给模型看的对话记录。
fn transcript(history: &[AdvisorTurnDto]) -> String {
    let mut recent: Vec<&AdvisorTurnDto> = history.iter().rev().take(MAX_TURNS).collect();
    recent.reverse();
    let mut out = String::new();
    for turn in recent {
        let text: String = turn.content.chars().take(400).collect();
        let who = if turn.role == "assistant" {
            "顾问"
        } else {
            "用户"
        };
        out.push_str(&format!("{who}：{text}\n"));
    }
    out
}

/// 跑一轮对话：模型可以连续调几次工具，最后必须给出一句答案。
pub fn chat(
    vault: &Vault,
    config: &LlmConfig,
    history: &[AdvisorTurnDto],
    question: &str,
) -> Result<AdvisorReplyDto, String> {
    let today = Local::now().date_naive();
    let brief = brief(vault, today);
    let system = system_prompt(today, &brief.full);

    let mut scratch = transcript(history);
    scratch.push_str(&format!("用户：{}\n", question.trim()));

    let mut tools: Vec<AdvisorToolDto> = Vec::new();
    let mut rounds = 0;

    while rounds < MAX_ROUNDS {
        rounds += 1;
        let asked = format!(
            "{scratch}\n现在轮到顾问，请回一个 JSON 对象：需要数字就调工具，能回答就直接 {{\"action\":\"answer\",\"text\":\"…\"}}。"
        );
        let (reply, _) = llm::complete(config, &system, &asked)?;
        let value = llm::extract_json(&reply)?;
        match parse_action(&value) {
            Action::Answer(text) => {
                return Ok(AdvisorReplyDto {
                    content: text,
                    tools,
                    rounds,
                    context_chars: brief.full.chars().count(),
                    error: None,
                })
            }
            Action::Tool { tool, input } => {
                let output = match run_tool(vault, &tool, &input, today) {
                    Ok(text) => text,
                    Err(message) => format!("工具调用失败：{message}"),
                };
                scratch.push_str(&format!("\n[工具 {tool} 输入 {}]\n{output}\n", input));
                tools.push(AdvisorToolDto {
                    tool,
                    input: input.to_string(),
                    output: output.chars().take(1200).collect(),
                });
            }
            Action::Invalid(reason) => {
                scratch.push_str(&format!("\n[上一轮格式不对：{reason}]\n"));
            }
        }
    }

    // 轮数用完了还没回答：逼它用现有结果说一句。
    let forced = format!(
        "{scratch}\n不要再调工具了。只回 {{\"action\":\"answer\",\"text\":\"…\"}}，用上面的数字回答。"
    );
    let (reply, _) = llm::complete(config, &system, &forced)?;
    let value = llm::extract_json(&reply)?;
    let content = match parse_action(&value) {
        Action::Answer(text) => text,
        Action::Tool { .. } | Action::Invalid(_) => format!(
            "模型没给出结论，先把当前数字放在这里：{}。可以再问一次，或者把问题说得更具体些。",
            brief.headline
        ),
    };
    Ok(AdvisorReplyDto {
        content,
        tools,
        rounds,
        context_chars: brief.full.chars().count(),
        error: None,
    })
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    turns: Vec<AdvisorTurnDto>,
}

fn path_of(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(FILE)
}

pub fn load_history(config_dir: &Path) -> AdvisorHistoryDto {
    let stored: Stored = std::fs::read_to_string(path_of(config_dir))
        .ok()
        .and_then(|src| serde_json::from_str(&src).ok())
        .unwrap_or_default();
    AdvisorHistoryDto {
        turns: stored.turns,
        updated_at: stored.updated_at,
    }
}

/// 会话写在自己的文件里（`0600`），从不进账本 —— 也就不会被同步到别的设备。
pub fn save_history(config_dir: &Path, turns: &[AdvisorTurnDto]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|err| format!("无法创建配置目录：{err}"))?;
    let stored = Stored {
        version: 1,
        updated_at: Some(Local::now().to_rfc3339()),
        turns: turns.to_vec(),
    };
    let json = serde_json::to_string_pretty(&stored).map_err(|err| err.to_string())?;
    let path = path_of(config_dir);
    write_private(&path, &json)?;
    Ok(())
}

pub fn clear_history(config_dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(path_of(config_dir)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("无法删除会话：{err}")),
    }
}

fn write_private(path: &Path, body: &str) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| format!("无法写入 {}：{err}", path.display()))?;
    file.write_all(body.as_bytes())
        .map_err(|err| format!("无法写入 {}：{err}", path.display()))
}

pub fn now_stamp() -> String {
    Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_engine::VaultOptions;
    use std::path::PathBuf;

    #[test]
    fn parses_the_three_actions() {
        assert_eq!(
            parse_action(&json!({"action":"answer","text":" 可以 "})),
            Action::Answer("可以".into())
        );
        assert_eq!(
            parse_action(&json!({"action":"tool","tool":"goals"})),
            Action::Tool {
                tool: "goals".into(),
                input: json!({})
            }
        );
        assert!(matches!(
            parse_action(&json!({"action":"tool"})),
            Action::Invalid(_)
        ));
        assert!(matches!(
            parse_action(&json!({"action":"dance"})),
            Action::Invalid(_)
        ));
        // 有些模型会省掉 action 只回 text。
        assert_eq!(
            parse_action(&json!({"text":"好在"})),
            Action::Answer("好在".into())
        );
    }

    #[test]
    fn the_plan_reader_clamps_and_sanitises() {
        let plan = plan_of(
            &json!({"metric":"income","limit":999,"min_amount":"1,200"}),
            "spend",
            None,
        );
        assert_eq!(plan.metric, "income");
        assert_eq!(plan.limit, 50);
        assert_eq!(plan.min_amount, Some("1200".parse().expect("decimal")));
    }

    #[test]
    fn months_shift_across_years() {
        let date = NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
        assert_eq!(
            shift_months(date, -3),
            NaiveDate::from_ymd_opt(2025, 11, 1).unwrap()
        );
        assert_eq!(
            shift_months(date, 11),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn the_transcript_keeps_the_recent_turns_only() {
        let turns: Vec<AdvisorTurnDto> = (0..40)
            .map(|index| AdvisorTurnDto {
                role: if index % 2 == 0 {
                    "user".into()
                } else {
                    "assistant".into()
                },
                content: format!("第{index}条"),
                at: String::new(),
            })
            .collect();
        let text = transcript(&turns);
        assert!(text.contains("第39条"));
        assert!(!text.contains("第0条"));
        assert_eq!(text.lines().count(), MAX_TURNS);
    }

    fn showcase() -> Vault {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/vault/full");
        Vault::load(&root, &VaultOptions::default()).expect("fixture vault")
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 20).expect("date")
    }

    #[test]
    fn the_brief_carries_the_numbers_the_model_used_to_lack() {
        // 「问一句」原来答不上「能不能花 5000」，根因就是模型只拿到一个查询计划。
        let brief = brief(&showcase(), today());
        for needle in ["财富自由", "现金与储备", "负债", "逐月收支", "最近几笔"]
        {
            assert!(brief.full.contains(needle), "brief 里少了 {needle}");
        }
        assert!(!brief.headline.contains('\n'), "headline 必须是单行");
        assert!(
            brief.headline.chars().count() < 400,
            "headline 太长会挤掉提示词"
        );
    }

    #[test]
    fn the_system_prompt_documents_every_tool_it_can_call() {
        let prompt = system_prompt(today(), &brief(&showcase(), today()).full);
        assert!(prompt.contains("2026-11-20"));
        for tool in ["query", "breakdown", "cashflow", "goals", "simulate"] {
            assert!(prompt.contains(tool), "提示词里没有 {tool}");
        }
    }

    #[test]
    fn simulate_answers_can_i_spend_this_month() {
        let text = run_tool(
            &showcase(),
            "simulate",
            &json!({ "amount": "5000", "kind": "expense" }),
            today(),
        )
        .expect("simulate");
        assert!(text.contains("5,000") || text.contains("5000"));
        assert!(text.contains("储备") || text.contains("个月"));
    }
}

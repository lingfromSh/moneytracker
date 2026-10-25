//! CSV statement import.
//!
//! A statement is never trusted as-is: it is decoded, mapped, normalised and
//! fingerprinted, then shown as a preview. Committing turns each accepted row
//! into an ordinary balanced transaction through the same write path as the
//! capture window, so rules, snapshots, audit and conflict detection all apply.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use ledger_core::amount::Amount;
use ledger_core::ast::{Posting, Status, Tag, Transaction};

use crate::amounts::CommodityTable;
use crate::capture::{commit_to_day_file, ExpandedCapture};
use crate::rules::Direction;
use crate::types::*;
use crate::vault::VaultOptions;
use crate::{EngineError, Result, Vault};

/// Column names a Chinese bank or payment export tends to use.
const DATE_KEYS: [&str; 7] = [
    "交易时间",
    "交易日期",
    "记账日期",
    "日期",
    "时间",
    "date",
    "time",
];
const PAYEE_KEYS: [&str; 9] = [
    "交易对方",
    "对方",
    "商户名称",
    "商户",
    "交易备注",
    "摘要",
    "payee",
    "description",
    "merchant",
];
const MEMO_KEYS: [&str; 7] = ["商品说明", "商品", "备注", "说明", "用途", "memo", "note"];
const AMOUNT_KEYS: [&str; 4] = ["金额", "交易金额", "amount", "金额(元)"];
const OUTFLOW_KEYS: [&str; 5] = ["支出", "借方发生额", "付款金额", "debit", "withdrawal"];
const INFLOW_KEYS: [&str; 5] = ["收入", "贷方发生额", "收款金额", "credit", "deposit"];

/// Date layouts seen in exported statements, most specific first.
const DATE_FORMATS: [&str; 10] = [
    "%Y-%m-%d %H:%M:%S",
    "%Y/%m/%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
    "%Y/%m/%d %H:%M",
    "%Y-%m-%d",
    "%Y/%m/%d",
    "%Y%m%d",
    "%d/%m/%Y",
    "%m/%d/%Y",
    "%Y年%m月%d日",
];

/// Providers we can pre-fill from the extension and headers.
pub const PRESETS: [(&str, &str); 4] = [
    ("支付宝", "assets:wallet:alipay"),
    ("微信支付", "assets:wallet:wechat"),
    ("招商银行", "assets:bank:cmb"),
    ("工商银行", "assets:bank:icbc"),
];

/// Decode a statement to text, guessing the encoding.
pub fn decode(path: &Path) -> Result<(String, String)> {
    let bytes =
        fs::read(path).map_err(|cause| EngineError::Io(format!("{}: {cause}", path.display())))?;
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((String::from_utf8_lossy(rest).to_string(), "utf-8".into()));
    }
    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok((text, "utf-8".into())),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::GB18030.decode(&bytes);
            Ok((decoded.to_string(), "gb18030".into()))
        }
    }
}

fn sniff_delimiter(line: &str) -> char {
    let marks = [',', '\t', ';', '|'];
    let counts: Vec<usize> = marks
        .iter()
        .map(|mark| line.matches(*mark).count())
        .collect();
    let (index, count) = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)
        .unwrap_or((0, &0));
    if *count == 0 {
        ','
    } else {
        marks[index]
    }
}

struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    delimiter: char,
    encoding: String,
}

fn read_table(path: &Path, profile: Option<&ImportProfileDto>) -> Result<Table> {
    let (text, encoding) = decode(path)?;
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let delimiter = profile
        .and_then(|p| p.delimiter.as_deref())
        .and_then(|raw| raw.chars().next())
        .filter(|c| !c.is_alphanumeric())
        .unwrap_or_else(|| sniff_delimiter(first));

    let header_row = profile.map(|p| p.header_row).unwrap_or(true);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .flexible(true)
        .has_headers(false)
        .from_reader(text.as_bytes());

    let mut raw_rows: Vec<Vec<String>> = Vec::new();
    for record in reader.records() {
        let record =
            record.map_err(|cause| EngineError::Vault(format!("CSV 解析失败：{cause}")))?;
        let row: Vec<String> = record.iter().map(|cell| cell.trim().to_string()).collect();
        if row.iter().all(|cell| cell.is_empty()) {
            continue;
        }
        raw_rows.push(row);
    }

    let (headers, rows) = if header_row && !raw_rows.is_empty() {
        let headers = raw_rows.remove(0);
        (headers, raw_rows)
    } else {
        let width = raw_rows.iter().map(|row| row.len()).max().unwrap_or(0);
        let headers = (1..=width).map(|i| format!("列{i}")).collect();
        (headers, raw_rows)
    };

    Ok(Table {
        headers,
        rows,
        delimiter,
        encoding,
    })
}

fn find_column(headers: &[String], keys: &[&str]) -> Option<String> {
    headers
        .iter()
        .find(|header| {
            let lower = header.to_lowercase();
            keys.iter().any(|key| lower.contains(&key.to_lowercase()))
        })
        .cloned()
}

fn column_index(headers: &[String], name: &str) -> Option<usize> {
    headers
        .iter()
        .position(|header| header == name)
        .or_else(|| headers.iter().position(|header| header.contains(name)))
}

/// Inspect a statement and guess its shape, so the import dialog starts filled in.
pub fn probe(path: &Path) -> Result<ImportProbeDto> {
    let table = read_table(path, None)?;
    let date_col = find_column(&table.headers, &DATE_KEYS);
    let payee_col = find_column(&table.headers, &PAYEE_KEYS);
    let memo_col =
        find_column(&table.headers, &MEMO_KEYS).filter(|name| Some(name) != payee_col.as_ref());
    let amount_col = find_column(&table.headers, &AMOUNT_KEYS);
    let outflow_col = find_column(&table.headers, &OUTFLOW_KEYS);
    let inflow_col = find_column(&table.headers, &INFLOW_KEYS);
    let preset = [
        ("支付宝", &["支付宝", "alipay"]),
        ("微信支付", &["微信", "wechat"]),
        ("招商银行", &["招商", "cmb"]),
        ("工商银行", &["工商", "icbc"]),
    ]
    .into_iter()
    .find(|(_, needles)| {
        let haystack = format!("{} {}", path.display(), table.headers.join(" ")).to_lowercase();
        needles
            .iter()
            .any(|needle| haystack.contains(&needle.to_lowercase()))
    })
    .map(|(name, _)| name.to_string());

    let account = preset
        .as_deref()
        .and_then(|name| {
            PRESETS
                .iter()
                .find(|(preset, _)| *preset == name)
                .map(|(_, acct)| *acct)
        })
        .map(str::to_string);

    Ok(ImportProbeDto {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "statement".into()),
        encoding: table.encoding,
        delimiter: table.delimiter.to_string(),
        header_row: true,
        headers: table.headers.clone(),
        sample: table.rows.iter().take(5).cloned().collect(),
        row_count: table.rows.len(),
        guess: ImportGuessDto {
            date_col,
            payee_col,
            memo_col,
            amount_col,
            outflow_col,
            inflow_col,
            account,
            preset,
        },
        presets: PRESETS.iter().map(|(name, _)| name.to_string()).collect(),
    })
}

/// `1,234.56` / `¥-45.00` / `(45.00)` / `45.00元` → decimal.
pub fn parse_amount_cell(raw: &str) -> Option<Decimal> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let negative = (trimmed.starts_with('(') && trimmed.ends_with(')'))
        || trimmed.starts_with('-')
        || trimmed.starts_with('－');
    let mut cleaned: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == '-')
        .collect();
    cleaned = cleaned.replace('-', "");
    // A single comma followed by one or two digits is a decimal mark, otherwise it groups thousands.
    if let Some(index) = cleaned.rfind(',') {
        let tail = &cleaned[index + 1..];
        if cleaned.matches(',').count() == 1 && (1..=2).contains(&tail.len()) && !tail.contains('.')
        {
            cleaned = format!("{}.{}", cleaned[..index].replace(',', ""), tail);
        } else {
            cleaned = cleaned.replace(',', "");
        }
    }
    let value = Decimal::from_str(&cleaned).ok()?;
    Some(if negative { -value.abs() } else { value })
}

/// Parse a statement date, honouring an explicit layout when one is configured.
pub fn parse_date_cell(raw: &str, format: Option<&str>) -> Option<NaiveDate> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(format) = format.filter(|format| !format.trim().is_empty()) {
        if let Ok(date) = NaiveDate::parse_from_str(trimmed, format) {
            return Some(date);
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Some(date);
    }
    for format in DATE_FORMATS {
        if let Ok(date) = NaiveDate::parse_from_str(trimmed, format) {
            return Some(date);
        }
    }
    // `2026-10-25 12:33:00`, `2026/10/25 12:33`
    let head = trimmed.split_whitespace().next().unwrap_or(trimmed);
    [head, trimmed].iter().find_map(|candidate| {
        DATE_FORMATS
            .iter()
            .find_map(|format| NaiveDate::parse_from_str(candidate, format).ok())
    })
}

fn normalise_text(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Stable identity for a statement row, used to detect re-imports.
pub fn fingerprint(
    account: &str,
    date: NaiveDate,
    payee: &str,
    amount: Decimal,
    commodity: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(account.to_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(date.format("%Y-%m-%d").to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(normalise_text(payee).as_bytes());
    hasher.update(b"|");
    hasher.update(amount.normalize().to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(commodity.to_uppercase().as_bytes());
    let hex = hasher.finalize().to_hex().to_string();
    hex[..16].to_string()
}

/// Every posting already in the vault, as fingerprints.
pub fn existing_fingerprints(vault: &Vault) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for record in vault.transactions() {
        for value in &record.values {
            out.insert(fingerprint(
                &value.account,
                record.txn.date,
                &record.txn.payee,
                value.amount.quantity,
                &value.amount.commodity,
            ));
        }
    }
    out
}

fn mapping_for(
    profile: &ImportProfileDto,
    payee: &str,
    memo: &str,
) -> (Option<String>, Vec<String>) {
    let haystack = format!("{payee} {memo}").to_lowercase();
    for rule in &profile.mapping {
        let pattern = rule.pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        let matched = if let Some(regex) = pattern.strip_prefix("re:") {
            regex::Regex::new(regex)
                .map(|re| re.is_match(&haystack))
                .unwrap_or(false)
        } else {
            haystack.contains(&pattern.to_lowercase())
        };
        if matched {
            return (Some(rule.category.clone()), rule.tags.clone());
        }
    }
    (None, Vec::new())
}

fn skipped(profile: &ImportProfileDto, text: &str) -> bool {
    let lower = text.to_lowercase();
    profile
        .skip_patterns
        .iter()
        .any(|pattern| !pattern.trim().is_empty() && lower.contains(&pattern.trim().to_lowercase()))
}

/// Batch id, so a re-import of the same file lands in one tagged group.
pub fn batch_id(profile: &ImportProfileDto) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(profile.path.as_bytes());
    for row in &profile.mapping {
        hasher.update(row.pattern.as_bytes());
    }
    let hex = hasher.finalize().to_hex().to_string();
    let today = chrono::Local::now().format("%Y%m%d").to_string();
    format!("{today}-{}", &hex[..6])
}

/// Parse and classify every row, without touching the disk.
pub fn preview(vault: &Vault, profile: &ImportProfileDto) -> Result<ImportPreviewDto> {
    let path = Path::new(&profile.path);
    let table = read_table(path, Some(profile))?;
    let (account, warnings) = vault.resolve_account(&profile.account);
    let commodity = profile
        .commodity
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| vault.options().base_commodity.clone());
    let precision = vault.commodities.precision_of(&commodity);
    let existing = existing_fingerprints(vault);
    let batch = batch_id(profile);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut rows: Vec<ImportRowDto> = Vec::new();
    let mut errors: Vec<DiagnosticDto> = Vec::new();

    let index = |name: &Option<String>| -> Option<usize> {
        name.as_deref()
            .and_then(|name| column_index(&table.headers, name))
    };
    let date_idx = index(&profile.date_col);
    let payee_idx = index(&profile.payee_col);
    let memo_idx = index(&profile.memo_col);
    let amount_idx = index(&profile.amount_col);
    let outflow_idx = index(&profile.outflow_col);
    let inflow_idx = index(&profile.inflow_col);

    if date_idx.is_none() {
        errors.push(diagnostic(
            profile,
            "import.missing_date_column",
            "没有找到日期列，请在上方指定",
        ));
    }
    if amount_idx.is_none() && outflow_idx.is_none() && inflow_idx.is_none() {
        errors.push(diagnostic(
            profile,
            "import.missing_amount_column",
            "没有找到金额列（可以是「金额」，也可以是「支出／收入」两列）",
        ));
    }

    let cell = |row: &[String], idx: Option<usize>| -> String {
        idx.and_then(|i| row.get(i)).cloned().unwrap_or_default()
    };

    for (offset, row) in table.rows.iter().enumerate() {
        let line = offset + 2;
        let payee = cell(row, payee_idx);
        let memo = cell(row, memo_idx);
        if skipped(profile, &format!("{payee} {memo}")) {
            continue;
        }
        let date_raw = cell(row, date_idx);
        let Some(date) = parse_date_cell(&date_raw, profile.date_format.as_deref()) else {
            errors.push(diagnostic(
                profile,
                "import.bad_date",
                &format!("第 {line} 行的日期「{date_raw}」无法识别"),
            ));
            continue;
        };
        let amount = match (amount_idx, outflow_idx, inflow_idx) {
            (Some(idx), _, _) => parse_amount_cell(&cell(row, Some(idx))),
            (None, Some(out), inc) => {
                let spent = parse_amount_cell(&cell(row, Some(out)));
                let earned = inc.and_then(|idx| parse_amount_cell(&cell(row, Some(idx))));
                match (spent, earned) {
                    (Some(spent), _) if !spent.is_zero() => Some(spent),
                    (_, Some(earned)) if !earned.is_zero() => Some(-earned.abs()),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(mut value) = amount else {
            errors.push(diagnostic(
                profile,
                "import.bad_amount",
                &format!("第 {line} 行的金额无法识别"),
            ));
            continue;
        };
        if profile.flip_sign {
            value = -value;
        }
        let value = value.round_dp(precision);
        let payee = if payee.trim().is_empty() {
            if memo.trim().is_empty() {
                "未命名交易".to_string()
            } else {
                memo.clone()
            }
        } else {
            payee.clone()
        };
        let (mapped, tags) = mapping_for(profile, &payee, &memo);
        let shared = fingerprint(&account, date, &payee, value, &commodity);
        let duplicate = existing.contains(&shared) || !seen.insert(shared.clone());
        let category = mapped;
        rows.push(ImportRowDto {
            line,
            date: date.to_string(),
            payee,
            memo: if memo.trim().is_empty() {
                None
            } else {
                Some(memo)
            },
            amount: amount_dto(&vault.commodities, value, &commodity),
            fingerprint: shared,
            duplicate,
            category,
            tags,
            warning: None,
            selected: !duplicate,
        });
    }

    let total: Decimal = rows
        .iter()
        .map(|row| row.amount.quantity.parse::<Decimal>().unwrap_or_default())
        .sum();
    let duplicates = rows.iter().filter(|row| row.duplicate).count();
    let mut out_warnings = Vec::new();
    for warning in warnings {
        if warning.contains("账户") {
            out_warnings.push(warning);
        }
    }

    Ok(ImportPreviewDto {
        path: profile.path.clone(),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        batch_id: batch,
        account,
        commodity: commodity.clone(),
        rows,
        duplicates,
        errors,
        total: amount_dto(&vault.commodities, total, &commodity),
        warnings: out_warnings,
        delimiter: table.delimiter.to_string(),
        encoding: table.encoding,
        headers: table.headers,
    })
}

fn diagnostic(profile: &ImportProfileDto, code: &str, message: &str) -> DiagnosticDto {
    DiagnosticDto {
        severity: "error".into(),
        code: code.into(),
        message: message.into(),
        file: profile
            .path
            .rsplit('/')
            .next()
            .unwrap_or("statement.csv")
            .to_string(),
        line: 0,
        col: 0,
    }
}

fn amount_dto(table: &CommodityTable, quantity: Decimal, commodity: &str) -> AmountDto {
    crate::amounts::decimal_to_dto(quantity, commodity, table.precision_of(commodity))
}

/// Turn the accepted rows into transactions on disk.
pub fn commit(
    vault: &Vault,
    profile: &ImportProfileDto,
    opts: &VaultOptions,
) -> Result<ImportCommitDto> {
    let preview = preview(vault, profile)?;
    let (account, _) = vault.resolve_account(&profile.account);
    let commodity = preview.commodity.clone();
    let precision = vault.commodities.precision_of(&commodity);
    let default_unclassified = vault.options().default_unclassified.clone();
    let batch = preview.batch_id.clone();
    let mut files: BTreeSet<String> = BTreeSet::new();
    let mut written = 0usize;
    let mut skipped = 0usize;

    let only: BTreeSet<u32> = profile.include_lines.iter().copied().collect();
    for row in preview.rows.iter().filter(|row| row.selected) {
        if row.duplicate || (!only.is_empty() && !only.contains(&(row.line as u32))) {
            skipped += 1;
            continue;
        }
        let Ok(date) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") else {
            skipped += 1;
            continue;
        };
        let value: Decimal = row.amount.quantity.parse().unwrap_or_default();
        let incoming = value.is_sign_negative();
        let category = row.category.clone().unwrap_or_else(|| {
            if incoming {
                "income:unclassified".to_string()
            } else {
                default_unclassified.clone()
            }
        });

        let mut txn = Transaction::new(date, row.payee.clone());
        txn.status = Some(Status::Cleared);
        let mut tags = vec![
            Tag::kv("import", batch.clone()),
            Tag::kv("ref", row.fingerprint.clone()),
        ];
        for tag in &row.tags {
            let tag = tag.trim().trim_matches(':');
            if !tag.is_empty() {
                tags.push(Tag::bare(tag));
            }
        }
        if category == default_unclassified {
            tags.push(Tag::bare("inbox"));
        }
        // The imported amount keeps its sign: money out is a positive expense leg
        // funded by a negative account leg, money in is the mirror image.
        txn.postings = vec![
            Posting::new(
                category,
                Some(Amount::canonical(value, &commodity, precision)),
            ),
            Posting::new(
                account.clone(),
                Some(Amount::canonical(-value, &commodity, precision)),
            ),
        ];
        txn.tags = tags;

        let expanded = ExpandedCapture {
            txn,
            postings: Vec::new(),
            canonical: String::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            rule_id: None,
            direction: if incoming {
                Direction::Income
            } else {
                Direction::Expense
            },
            summary: row.payee.clone(),
        };
        let id = crate::capture::new_txn_id();
        let block = commit_to_day_file(vault, &expanded, &id, opts)?;
        files.insert(block.rel.clone());
        written += 1;
    }

    Ok(ImportCommitDto {
        batch_id: batch,
        written,
        skipped,
        files: files.into_iter().collect(),
        diagnostics: preview.errors,
    })
}

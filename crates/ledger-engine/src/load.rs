//! Loading a vault: include graph, parsing, indexing, balances.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;

use chrono::NaiveDate;
use ledger_core::{Diagnostic, Item, Severity, Span, Transaction, VirtualKind};
use ledger_query::TxnInfo;
use rust_decimal::Decimal;

use crate::amounts::{balance, CommodityTable};
use crate::fx::PriceTable;
use crate::rules::RuleSet;
use crate::types::{
    AccountNodeDto, AmountDto, BalanceRowDto, CommodityDto, DiagnosticDto, SnapshotDto, TxnDto,
    VaultInfoDto,
};
use crate::vault::{validate_vault, VaultOptions, INDEX_FILE, RULES_FILE};
use crate::writer::{self, FileState};
use crate::{EngineError, Result};

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub rel: String,
    pub abs: PathBuf,
    pub src: String,
    pub state: FileState,
    pub journal: ledger_core::Journal,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub name: String,
    pub kind: char,
    pub declared: bool,
}

#[derive(Debug, Clone)]
pub struct TxnValue {
    pub account: String,
    pub kind: char,
    pub amount: ledger_core::Amount,
    /// Amount converted to the base commodity (None when the rate is missing).
    pub base: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct TxnRecord {
    pub id: String,
    pub file: String,
    pub span: Span,
    pub txn: Transaction,
    pub balanced: bool,
    pub fx_missing: bool,
    /// Positive = money left the funding accounts (an expense).
    pub base_value: Option<Decimal>,
    pub values: Vec<TxnValue>,
    pub editable: bool,
}

/// Transaction record plus borrowed slices used for rule matching. Kept
/// separate so `TxnRecord` stays cheap to clone.
#[derive(Debug, Clone)]
pub struct TxnRecordInfo {
    pub account_names: Vec<String>,
    pub tag_pairs: Vec<(String, Option<String>)>,
    pub amounts: Vec<Decimal>,
    pub commodities: Vec<String>,
}

#[derive(Debug)]
pub struct Vault {
    pub root: PathBuf,
    pub opts: VaultOptions,
    pub files: Vec<FileEntry>,
    pub txns: Vec<TxnRecord>,
    pub info_slices: Vec<TxnRecordInfo>,
    pub prices: PriceTable,
    pub commodities: CommodityTable,
    pub accounts: BTreeMap<String, AccountInfo>,
    pub rules: RuleSet,
    pub diagnostics: Vec<Diagnostic>,
    pub warnings: Vec<String>,
    pub parse_ms: f64,
    pub first_date: Option<NaiveDate>,
    pub last_date: Option<NaiveDate>,
}

impl Vault {
    pub fn load(root: &Path, opts: &VaultOptions) -> Result<Self> {
        validate_vault(root)?;
        let started = Instant::now();
        let root = root
            .canonicalize()
            .map_err(|e| EngineError::Io(e.to_string()))?;

        let mut loader = Loader {
            root: root.clone(),
            files: Vec::new(),
            visiting: Vec::new(),
            loaded: HashSet::new(),
            diagnostics: Vec::new(),
            warnings: Vec::new(),
        };
        loader.load_file(&root.join(INDEX_FILE), false)?;

        let mut vault = Vault {
            root,
            opts: opts.clone(),
            files: loader.files,
            txns: Vec::new(),
            info_slices: Vec::new(),
            prices: PriceTable::default(),
            commodities: CommodityTable::new(&opts.base_commodity, opts.base_precision),
            accounts: BTreeMap::new(),
            rules: RuleSet::default(),
            diagnostics: loader.diagnostics,
            warnings: loader.warnings,
            parse_ms: 0.0,
            first_date: None,
            last_date: None,
        };
        vault.build_index();
        vault.parse_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok(vault)
    }

    fn build_index(&mut self) {
        // 1. commodity declarations decide the base commodity + precision
        let mut base: Option<(String, u32)> = None;
        for f in &self.files {
            for item in &f.journal.items {
                if let Item::Commodity(c) = item {
                    let prec = c
                        .sample
                        .as_ref()
                        .map(|s| s.precision())
                        .unwrap_or(self.opts.base_precision);
                    if base.is_none() || c.name == self.opts.base_commodity {
                        base = Some((c.name.clone(), prec));
                    }
                }
            }
        }
        if let Some((name, prec)) = base {
            self.commodities.base = name;
            self.commodities.base_precision = prec;
        }

        // 2. account declarations (type tag) and prices
        let mut decls: HashMap<String, char> = HashMap::new();
        let mut price_decls = Vec::new();
        for f in &self.files {
            for item in &f.journal.items {
                match item {
                    Item::Account(a) => {
                        let kind = a
                            .tags
                            .iter()
                            .find(|t| t.key == "type" || t.key == "account")
                            .and_then(|t| t.value.as_deref())
                            .and_then(kind_from_tag)
                            .unwrap_or('U');
                        if kind != 'U' {
                            decls.insert(normalize_account(&a.name), kind);
                        }
                        let name = normalize_account(&a.name);
                        let entry = self.accounts.entry(name.clone()).or_insert(AccountInfo {
                            name: name.clone(),
                            kind: 'U',
                            declared: true,
                        });
                        entry.declared = true;
                        if kind != 'U' {
                            entry.kind = kind;
                        }
                    }
                    Item::Price(p) => price_decls.push(p.clone()),
                    _ => {}
                }
            }
        }
        self.prices = PriceTable::from_decls(price_decls.iter());

        // 3. transactions
        //
        // Normalising a transaction only needs read access to the commodity table,
        // the price table and the account declarations, so the whole pass runs in
        // parallel and the results are folded into the shared indexes in order —
        // ordering keeps ids, diagnostics and accounts deterministic.
        let account_types = self.opts.account_type_map.clone();
        let built: Vec<BuiltTxn> = {
            let base_commodity = self.commodities.base.as_str();
            let flat: Vec<(&FileEntry, &Transaction)> = self
                .files
                .iter()
                .flat_map(|file| {
                    file.journal
                        .items
                        .iter()
                        .filter_map(move |item| match item {
                            Item::Transaction(txn) => Some((file, txn)),
                            _ => None,
                        })
                })
                .collect();
            let commodities = &self.commodities;
            let prices = &self.prices;
            let decls = &decls;
            let account_type_map = account_types.as_slice();
            flat.par_iter()
                .map(|(file, txn)| {
                    build_txn(
                        file,
                        txn,
                        commodities,
                        prices,
                        decls,
                        account_type_map,
                        base_commodity,
                    )
                })
                .collect()
        };

        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut txns = Vec::with_capacity(built.len());
        let mut info_slices = Vec::with_capacity(built.len());
        for mut b in built {
            let id = if seen_ids.contains(&b.id) {
                format!(
                    "{}-{}",
                    b.id,
                    &writer::hash_of(&format!("{}:{}", b.file, b.span.start))[..6]
                )
            } else {
                b.id.clone()
            };
            seen_ids.insert(id.clone());
            self.diagnostics.append(&mut b.diagnostics);
            for value in &b.values {
                self.commodities.observe(&value.amount);
            }
            for acc in &b.info.account_names {
                self.accounts
                    .entry(acc.clone())
                    .or_insert_with(|| AccountInfo {
                        name: acc.clone(),
                        kind: kind_of(acc, &decls, account_types.as_slice()),
                        declared: false,
                    });
                // ancestors
                let parts: Vec<&str> = acc.split(':').collect();
                for i in 1..parts.len() {
                    let parent = parts[..i].join(":");
                    self.accounts
                        .entry(parent.clone())
                        .or_insert_with(|| AccountInfo {
                            kind: kind_of(&parent, &decls, account_types.as_slice()),
                            name: parent,
                            declared: false,
                        });
                }
            }
            info_slices.push(b.info);
            txns.push(TxnRecord {
                id,
                file: b.file,
                span: b.span,
                txn: b.txn,
                balanced: b.balanced,
                fx_missing: b.fx_missing,
                base_value: b.base_value,
                values: b.values,
                editable: b.editable,
            });
        }

        // 4. fix up kinds for implicitly created accounts
        let account_type_map = account_types.as_slice();
        for info in self.accounts.values_mut() {
            if info.kind == 'U' {
                info.kind = kind_of(&info.name, &decls, account_type_map);
            }
        }
        for t in txns.iter_mut() {
            for v in t.values.iter_mut() {
                v.kind = kind_of(&v.account, &decls, account_type_map);
            }
        }

        self.first_date = txns.iter().map(|t| t.txn.date).min();
        self.last_date = txns.iter().map(|t| t.txn.date).max();
        self.txns = txns;
        self.info_slices = info_slices;

        // 5. rules live outside the include graph
        let rules_path = self.root.join(RULES_FILE);
        if rules_path.is_file() {
            match std::fs::read_to_string(&rules_path) {
                Ok(src) => {
                    let state = writer::file_state(&rules_path).ok();
                    let set = RuleSet::parse(RULES_FILE, &src, state);
                    self.diagnostics.extend(set.diagnostics.iter().cloned());
                    self.rules = set;
                }
                Err(e) => self.warnings.push(format!("cannot read rules.ledger: {e}")),
            }
        }

        // 6. keep diagnostics deterministic
        self.diagnostics.sort_by(|a, b| {
            (a.file.as_str(), a.span.start, a.code.as_str()).cmp(&(
                b.file.as_str(),
                b.span.start,
                b.code.as_str(),
            ))
        });
    }

    pub fn txn_info<'a>(&'a self, idx: usize) -> TxnInfo<'a> {
        let info = &self.info_slices[idx];
        let rec = &self.txns[idx];
        TxnInfo {
            date: Some(rec.txn.date),
            payee: &rec.txn.payee,
            code: rec.txn.code.as_deref().unwrap_or(""),
            status: rec.txn.status.map(|s| s.marker()).unwrap_or(""),
            accounts: &info.account_names,
            tags: &info.tag_pairs,
            amounts: &info.amounts,
            commodities: &info.commodities,
        }
    }

    pub fn transaction(&self, id: &str) -> Option<&TxnRecord> {
        self.txns.iter().find(|t| t.id == id)
    }

    pub fn transaction_index(&self, id: &str) -> Option<usize> {
        self.txns.iter().position(|t| t.id == id)
    }

    /// Resolve `微信` to `assets:wallet:wechat` when that is the only match.
    /// Account kind for a name that may not exist in the ledger yet (budget
    /// envelopes are often planned before their first transaction). Falls back to
    /// the nearest known parent, then to the configured top-level name map.
    pub fn kind_for(&self, name: &str) -> char {
        let name = normalize_account(name);
        if let Some(info) = self.accounts.get(&name) {
            return info.kind;
        }
        let mut current = name.as_str();
        while let Some((parent, _)) = current.rsplit_once(':') {
            if let Some(info) = self.accounts.get(parent) {
                return info.kind;
            }
            current = parent;
        }
        let decls = HashMap::new();
        kind_of(&name, &decls, self.opts.account_type_map.as_slice())
    }

    pub fn resolve_account(&self, name: &str) -> (String, Vec<String>) {
        let name = normalize_account(name);
        if self.accounts.contains_key(&name) {
            return (name, Vec::new());
        }
        let suffix = format!(":{name}");
        let matches: Vec<&String> = self
            .accounts
            .keys()
            .filter(|k| **k == name || k.ends_with(&suffix))
            .collect();
        match matches.len() {
            1 => (matches[0].clone(), Vec::new()),
            n if n > 1 => (
                name.clone(),
                vec![format!(
                    "`{name}` matches {n} accounts ({}); using it exactly as typed",
                    matches
                        .iter()
                        .take(4)
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )],
            ),
            _ => (name, Vec::new()),
        }
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    pub fn transactions(&self) -> &[TxnRecord] {
        &self.txns
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn file(&self, rel: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.rel == rel)
    }

    pub fn base_amount(&self, quantity: Decimal) -> AmountDto {
        crate::amounts::decimal_to_dto(
            quantity,
            &self.commodities.base,
            self.commodities.base_precision,
        )
    }

    pub fn info_dto(&self) -> VaultInfoDto {
        VaultInfoDto {
            path: self.root.to_string_lossy().to_string(),
            base_commodity: self.commodities.base.clone(),
            base_precision: self.commodities.base_precision,
            default_funding: self.opts.default_funding.clone(),
            default_unclassified: self.opts.default_unclassified.clone(),
            fx_enabled: self.opts.fx_enabled,
            transaction_count: self.txns.len(),
            file_count: self.files.len(),
            first_date: self.first_date.map(|d| d.to_string()),
            last_date: self.last_date.map(|d| d.to_string()),
            parse_ms: self.parse_ms,
        }
    }

    pub fn diagnostics_dto(&self) -> Vec<DiagnosticDto> {
        self.diagnostics.iter().map(diagnostic_dto).collect()
    }

    pub fn missing_fx_count(&self) -> usize {
        self.txns.iter().filter(|t| t.fx_missing).count()
    }

    /// Account tree with own + rolled-up balances, in base commodity.
    pub fn account_tree(&self) -> Vec<AccountNodeDto> {
        #[derive(Default)]
        struct Node {
            own: Decimal,
            total: Decimal,
            children: BTreeMap<String, Node>,
        }
        fn insert(node: &mut Node, parts: &[&str], amount: Decimal) {
            if parts.is_empty() {
                // `total` was already credited by the caller on the way down, so the
                // leaf must only add its own posting — otherwise every leaf reports
                // twice its balance while the ancestors stay correct.
                node.own += amount;
                return;
            }
            let child = node.children.entry(parts[0].to_string()).or_default();
            child.total += amount;
            insert(child, &parts[1..], amount);
        }
        let mut root = Node::default();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for t in &self.txns {
            for v in &t.values {
                if let Some(base) = v.base {
                    let parts: Vec<&str> = v.account.split(':').collect();
                    insert(&mut root, &parts, base);
                }
                *counts.entry(v.account.clone()).or_insert(0) += 1;
            }
        }
        fn convert(
            node_name: &str,
            prefix: &str,
            node: &Node,
            accounts: &BTreeMap<String, AccountInfo>,
            counts: &HashMap<String, usize>,
            table: &CommodityTable,
            depth: u32,
        ) -> AccountNodeDto {
            let full = if prefix.is_empty() {
                node_name.to_string()
            } else {
                format!("{prefix}:{node_name}")
            };
            let info = accounts.get(&full);
            AccountNodeDto {
                name: node_name.to_string(),
                full: full.clone(),
                kind: info
                    .map(|i| i.kind.to_string())
                    .unwrap_or_else(|| "U".into()),
                depth,
                declared: info.map(|i| i.declared).unwrap_or(false),
                own: vec![crate::amounts::decimal_to_dto(
                    node.own,
                    &table.base,
                    table.base_precision,
                )],
                total: vec![crate::amounts::decimal_to_dto(
                    node.total,
                    &table.base,
                    table.base_precision,
                )],
                txn_count: counts.get(&full).copied().unwrap_or(0),
                children: node
                    .children
                    .iter()
                    .map(|(name, child)| {
                        convert(name, &full, child, accounts, counts, table, depth + 1)
                    })
                    .collect(),
            }
        }
        root.children
            .iter()
            .map(|(name, node)| {
                convert(
                    name,
                    "",
                    node,
                    &self.accounts,
                    &counts,
                    &self.commodities,
                    0,
                )
            })
            .collect()
    }

    /// Flat balances per account (base commodity only).
    /// Native balances, one row per account and commodity — the same numbers
    /// `hledger balance --flat --no-total` prints. Base-commodity valuation lives in
    /// `net_worth()` and `account_tree()`, never here.
    pub fn balances(&self) -> Vec<BalanceRowDto> {
        let mut totals: BTreeMap<(String, String), Decimal> = BTreeMap::new();
        for t in &self.txns {
            for p in &t.txn.postings {
                let Some(amount) = p.amount.as_ref() else {
                    continue;
                };
                let commodity = if amount.commodity.is_empty() {
                    self.commodities.base.clone()
                } else {
                    amount.commodity.clone()
                };
                *totals
                    .entry((p.account.clone(), commodity))
                    .or_insert(Decimal::ZERO) += amount.quantity;
            }
        }
        totals
            .into_iter()
            .filter(|(_, qty)| !qty.is_zero())
            .map(|((account, commodity), qty)| {
                let precision = self.commodities.precision_of(&commodity);
                BalanceRowDto {
                    account,
                    quantity: qty.to_string(),
                    value: crate::amounts::f64_of(qty),
                    display: self
                        .commodities
                        .format(&ledger_core::Amount::canonical(qty, &commodity, precision)),
                    commodity,
                }
            })
            .collect()
    }

    pub fn net_worth(&self) -> Vec<AmountDto> {
        let mut total = Decimal::ZERO;
        for t in &self.txns {
            for v in &t.values {
                if matches!(v.kind, 'A' | 'L') {
                    if let Some(base) = v.base {
                        total += base;
                    }
                }
            }
        }
        vec![crate::amounts::decimal_to_dto(
            total,
            &self.commodities.base,
            self.commodities.base_precision,
        )]
    }

    pub fn commodities_dto(&self) -> Vec<CommodityDto> {
        let mut out: Vec<CommodityDto> = self
            .commodities
            .precision
            .iter()
            .map(|(symbol, prec)| CommodityDto {
                symbol: symbol.clone(),
                precision: *prec,
                is_base: *symbol == self.commodities.base,
                used: self.commodities.used.get(symbol).copied().unwrap_or(0),
            })
            .collect();
        if !out.iter().any(|c| c.is_base) {
            out.push(CommodityDto {
                symbol: self.commodities.base.clone(),
                precision: self.commodities.base_precision,
                is_base: true,
                used: 0,
            });
        }
        out.sort_by(|a, b| b.used.cmp(&a.used));
        out
    }

    pub fn snapshot(&self) -> SnapshotDto {
        SnapshotDto {
            vault: self.info_dto(),
            accounts: self.account_tree(),
            commodities: self.commodities_dto(),
            balances: self.balances(),
            net_worth: self.net_worth(),
            diagnostics: self.diagnostics_dto(),
            loaded_at: chrono::Local::now().to_rfc3339(),
        }
    }

    pub fn txn_dto(&self, idx: usize) -> TxnDto {
        let rec = &self.txns[idx];
        txn_dto(rec, &self.commodities)
    }
}

pub fn diagnostic_dto(d: &Diagnostic) -> DiagnosticDto {
    DiagnosticDto {
        severity: d.severity.as_str().to_string(),
        code: d.code.clone(),
        message: d.message.clone(),
        file: d.file.clone(),
        line: d.line,
        col: d.col,
    }
}

pub fn posting_dto(p: &ledger_core::Posting, table: &CommodityTable) -> crate::types::PostingDto {
    let amount = p
        .amount
        .as_ref()
        .map(|a| crate::amounts::to_dto_prec(a, table.precision_of(&a.commodity)));
    let effective = p.effective_amount();
    let (commodity, quantity, display) = match &effective {
        Some(a) => (a.commodity.clone(), a.quantity.to_string(), table.format(a)),
        None => (String::new(), String::new(), String::new()),
    };
    crate::types::PostingDto {
        account: p.account.clone(),
        amount,
        commodity,
        quantity,
        display,
        price: p
            .price
            .as_ref()
            .map(|pr| crate::amounts::to_dto(&pr.amount)),
        price_kind: p.price.as_ref().map(|pr| match pr.kind {
            ledger_core::PriceKind::Unit => "unit".to_string(),
            ledger_core::PriceKind::Total => "total".to_string(),
        }),
        assertion: p.balance_assertion.as_ref().map(crate::amounts::to_dto),
        comment: p.comment.clone(),
        virtual_kind: match p.virtual_kind {
            VirtualKind::Real => "real",
            VirtualKind::Virtual => "virtual",
            VirtualKind::BalancedVirtual => "balanced_virtual",
        }
        .to_string(),
        tags: p
            .tags
            .iter()
            .map(|t| crate::types::TagDto {
                key: t.key.clone(),
                value: t.value.clone(),
            })
            .collect(),
    }
}

pub fn txn_dto(rec: &TxnRecord, table: &CommodityTable) -> TxnDto {
    let postings = rec
        .txn
        .postings
        .iter()
        .map(|p| posting_dto(p, table))
        .collect();

    TxnDto {
        id: rec.id.clone(),
        date: rec.txn.date.to_string(),
        date2: rec.txn.date2.map(|d| d.to_string()),
        status: rec.txn.status.map(|s| match s {
            ledger_core::Status::Cleared => "cleared".to_string(),
            ledger_core::Status::Pending => "pending".to_string(),
        }),
        code: rec.txn.code.clone(),
        payee: rec.txn.payee.clone(),
        comment: strip_system_tokens(rec.txn.comment.as_deref()),
        tags: rec
            .txn
            .tags
            .iter()
            .filter(|t| !is_system_tag(&t.key))
            .map(|t| crate::types::TagDto {
                key: t.key.clone(),
                value: t.value.clone(),
            })
            .collect(),
        postings,
        file: rec.file.clone(),
        span_start: rec.span.start,
        span_end: rec.span.end,
        base_value: rec.base_value.map(crate::amounts::f64_of),
        base_value_display: rec.base_value.map(|v| {
            table.format(&ledger_core::Amount::canonical(
                v,
                &table.base,
                table.base_precision,
            ))
        }),
        balanced: rec.balanced,
        fx_missing: rec.fx_missing,
        editable: rec.editable,
    }
}

/// Tag keys that belong to the bookkeeping engine rather than to the user.
///
/// They are written by capture/rewrite paths and must survive every edit, so the
/// IPC surface hides them and the write path re-injects them.
pub const SYSTEM_TAG_KEYS: [&str; 4] = ["txn", "rule", "query", "recomputed"];

pub fn is_system_tag(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    SYSTEM_TAG_KEYS.contains(&key.as_str())
}

/// Splits a header comment into the engine-owned tokens (`txn:…`, `rule:…`, …)
/// and the free text the user typed.
pub fn split_comment(comment: Option<&str>) -> (Vec<String>, String) {
    let Some(comment) = comment else {
        return (Vec::new(), String::new());
    };
    let mut system = Vec::new();
    let mut user = Vec::new();
    for token in comment.split_whitespace() {
        let key = token.split(':').next().unwrap_or("");
        if token.contains(':') && !key.is_empty() && is_system_tag(key) {
            system.push(token.to_string());
        } else {
            user.push(token.to_string());
        }
    }
    (system, user.join(" "))
}

/// The user-facing part of a transaction comment, without identity tokens.
pub fn strip_system_tokens(comment: Option<&str>) -> Option<String> {
    let (_, user) = split_comment(comment);
    if user.is_empty() {
        None
    } else {
        Some(user)
    }
}

pub fn pseudo_id(rel: &str, txn: &Transaction) -> String {
    let seed = format!("{rel}|{}|{}|{}", txn.span.start, txn.date, txn.payee);
    let hash = blake3::hash(seed.as_bytes()).to_hex().to_string();
    format!("x-{}", &hash[..16])
}

pub fn normalize_account(name: &str) -> String {
    name.split(':')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(":")
        .to_lowercase()
}

pub fn kind_from_tag(value: &str) -> Option<char> {
    let v = value.trim().to_ascii_lowercase();
    let first = v.split(',').next().unwrap_or("").trim().to_string();
    match first.as_str() {
        "a" | "asset" | "assets" => Some('A'),
        "l" | "liability" | "liabilities" => Some('L'),
        "e" | "equity" => Some('E'),
        "r" | "revenue" | "revenues" | "income" => Some('R'),
        "x" | "expense" | "expenses" => Some('X'),
        _ => None,
    }
}

pub fn kind_of(account: &str, decls: &HashMap<String, char>, map: &[(String, String)]) -> char {
    let account = normalize_account(account);
    let mut best: Option<(usize, char)> = None;
    for (name, kind) in decls {
        if (account == *name || account.starts_with(&format!("{name}:")))
            && best.map(|(len, _)| name.len() > len).unwrap_or(true)
        {
            best = Some((name.len(), *kind));
        }
    }
    if let Some((_, kind)) = best {
        return kind;
    }
    let top = account.split(':').next().unwrap_or("");
    for (prefix, kind) in map {
        if prefix.eq_ignore_ascii_case(top) {
            return kind.chars().next().unwrap_or('U').to_ascii_uppercase();
        }
    }
    'U'
}

struct Loader {
    root: PathBuf,
    files: Vec<FileEntry>,
    visiting: Vec<PathBuf>,
    loaded: HashSet<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    warnings: Vec<String>,
}

impl Loader {
    fn load_file(&mut self, abs: &Path, optional: bool) -> Result<()> {
        let canonical = match abs.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                if optional {
                    return Ok(());
                }
                return Err(EngineError::Vault(format!(
                    "cannot open {}: {e}",
                    abs.display()
                )));
            }
        };
        if canonical.starts_with(self.root.join(crate::vault::BAK_DIR)) {
            return Ok(());
        }
        if self.visiting.contains(&canonical) {
            let rel = self.rel_of(&canonical);
            let message = format!("include cycle detected at {rel}");
            self.diagnostics.push(Diagnostic::error(
                "include.cycle",
                message,
                &rel,
                Span::default(),
            ));
            return Ok(());
        }
        if self.loaded.contains(&canonical) {
            return Ok(());
        }
        self.loaded.insert(canonical.clone());
        self.visiting.push(canonical.clone());

        let src = match std::fs::read_to_string(&canonical) {
            Ok(src) => src,
            Err(_) if optional => return Ok(()),
            Err(e) => return Err(EngineError::Io(format!("{}: {e}", canonical.display()))),
        };
        let state = writer::file_state(&canonical)?;
        let rel = self.rel_of(&canonical);
        let out = ledger_core::parse_journal(&rel, &src);
        self.diagnostics.extend(out.diagnostics.iter().cloned());

        let includes: Vec<ledger_core::Include> = out
            .journal
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Include(inc) => Some(inc.clone()),
                _ => None,
            })
            .collect();

        self.files.push(FileEntry {
            rel: rel.clone(),
            abs: canonical.clone(),
            src: src.clone(),
            state,
            journal: out.journal,
            diagnostics: out.diagnostics,
        });

        for inc in includes {
            let base = canonical.parent().unwrap_or(&self.root).to_path_buf();
            let pattern = inc.path.trim();
            if pattern.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error(
                        "core.parse.empty_include",
                        "include needs a path",
                        &rel,
                        inc.span,
                    )
                    .with_position(&src),
                );
                continue;
            }
            let is_glob = pattern.contains('*') || pattern.contains('?') || pattern.contains('[');
            let mut targets: Vec<PathBuf> = if is_glob {
                expand_glob(&base, pattern)
            } else {
                vec![base.join(pattern)]
            };
            targets.sort();
            if targets.is_empty() {
                if !inc.optional {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            "include.glob_empty",
                            format!("include `{pattern}` matched no files"),
                            &rel,
                            inc.span,
                        )
                        .with_position(&src),
                    );
                }
                continue;
            }
            for target in targets {
                match self.load_file(&target, inc.optional) {
                    Ok(()) => {}
                    Err(e) => {
                        self.diagnostics.push(
                            Diagnostic::error("include.missing", format!("{e}"), &rel, inc.span)
                                .with_position(&src),
                        );
                    }
                }
            }
        }

        self.visiting.pop();
        Ok(())
    }

    fn rel_of(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.root)
            .unwrap_or(abs)
            .to_string_lossy()
            .to_string()
    }
}

/// Minimal segment-wise glob: `*`, `?` and `**` (no braces, no char classes).
pub fn expand_glob(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let mut current = vec![base.to_path_buf()];
    for seg in segments {
        let mut next = Vec::new();
        for dir in &current {
            if seg == "**" {
                next.push(dir.clone());
                let mut stack = vec![dir.clone()];
                while let Some(d) = stack.pop() {
                    if let Ok(entries) = std::fs::read_dir(&d) {
                        for e in entries.flatten() {
                            let p = e.path();
                            if p.is_dir() {
                                next.push(p.clone());
                                stack.push(p);
                            }
                        }
                    }
                }
            } else if seg.contains('*') || seg.contains('?') {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if segment_matches(seg, &name) {
                            next.push(e.path());
                        }
                    }
                }
            } else {
                let p = dir.join(seg);
                if p.exists() {
                    next.push(p);
                }
            }
        }
        next.sort();
        next.dedup();
        current = next;
    }
    current.retain(|p| p.is_file());
    current
}

fn segment_matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    fn go(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&p[1..], &n[i..])),
            Some('?') => !n.is_empty() && go(&p[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && go(&p[1..], &n[1..]),
        }
    }
    go(&p, &n)
}

/// One transaction, normalised. Built in parallel; folded into the vault in order.
struct BuiltTxn {
    id: String,
    file: String,
    span: Span,
    txn: Transaction,
    balanced: bool,
    fx_missing: bool,
    base_value: Option<Decimal>,
    values: Vec<TxnValue>,
    info: TxnRecordInfo,
    diagnostics: Vec<Diagnostic>,
    editable: bool,
}

#[allow(clippy::too_many_arguments)]
fn build_txn(
    file: &FileEntry,
    txn: &Transaction,
    commodities: &CommodityTable,
    prices: &PriceTable,
    decls: &HashMap<String, char>,
    account_type_map: &[(String, String)],
    base_commodity: &str,
) -> BuiltTxn {
    let mut txn = txn.clone();
    let mut diagnostics = Vec::new();
    let balanced = match balance(&mut txn.postings, commodities, base_commodity) {
        Ok(()) => true,
        Err(issues) => {
            for issue in issues {
                diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: issue.code,
                    message: issue.message,
                    file: file.rel.clone(),
                    span: txn.span,
                    line: 0,
                    col: 0,
                });
            }
            false
        }
    };

    let id = match txn.tag("txn") {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => pseudo_id(&file.rel, &txn),
    };

    let mut values = Vec::new();
    let mut fx_missing = false;
    let mut cash = Decimal::ZERO;
    let mut cash_complete = true;
    for p in &txn.postings {
        let Some(eff) = p.effective_amount() else {
            continue;
        };
        let kind = kind_of(&p.account, decls, account_type_map);
        let base = if eff.commodity.is_empty() || eff.commodity == base_commodity {
            Some(eff.quantity)
        } else {
            prices
                .convert(&eff, base_commodity, txn.date)
                .map(|a| a.quantity)
        };
        if base.is_none() {
            fx_missing = true;
        }
        if p.is_balanced() && matches!(kind, 'A' | 'L') {
            match base {
                Some(b) => cash += b,
                None => cash_complete = false,
            }
        }
        values.push(TxnValue {
            account: p.account.clone(),
            kind,
            amount: eff,
            base,
        });
    }
    let base_value = if cash_complete && !fx_missing {
        Some(-cash)
    } else {
        None
    };

    let account_names: Vec<String> = txn.postings.iter().map(|p| p.account.clone()).collect();
    let tag_pairs: Vec<(String, Option<String>)> = txn
        .tags
        .iter()
        .map(|t| (t.key.clone(), t.value.clone()))
        .collect();
    let amounts: Vec<Decimal> = values.iter().map(|v| v.amount.quantity).collect();
    let commodities: Vec<String> = values.iter().map(|v| v.amount.commodity.clone()).collect();

    BuiltTxn {
        id,
        file: file.rel.clone(),
        span: txn.span,
        editable: !file.diagnostics.iter().any(|d| d.is_error()),
        txn,
        balanced,
        fx_missing,
        base_value,
        values,
        info: TxnRecordInfo {
            account_names,
            tag_pairs,
            amounts,
            commodities,
        },
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VaultCreateOptions;
    use crate::vault::{create_vault, VaultOptions};
    use tempfile::tempdir;

    fn vault_with(files: &[(&str, &str)]) -> (tempfile::TempDir, VaultOptions) {
        let dir = tempdir().unwrap();
        create_vault(
            dir.path(),
            &VaultCreateOptions {
                base_commodity: None,
                default_funding: None,
                default_unclassified: None,
                template: Some(true),
            },
        )
        .unwrap();
        for (rel, src) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, src).unwrap();
        }
        // Wire the real `index -> year -> month -> day` chain for every day
        // file the test dropped in, exactly as `capture_commit` does.
        let opts = VaultOptions::default();
        for (rel, _) in files {
            if let Some(date) = day_file_date(rel) {
                crate::writer::ensure_day_file(dir.path(), date, &opts).unwrap();
            }
        }
        (dir, opts)
    }

    /// `2026/2026-10-25.ledger` -> `2026-10-25`.
    fn day_file_date(rel: &str) -> Option<chrono::NaiveDate> {
        let name = rel.rsplit('/').next()?.strip_suffix(".ledger")?;
        chrono::NaiveDate::parse_from_str(name, "%Y-%m-%d").ok()
    }

    const TXN: &str = "2026-10-25 * 星巴克  ; txn:01JA3KQ7 :coffee:
    expenses:food:coffee      4.50 CNY
    assets:wallet:alipay     -4.50 CNY
";

    #[test]
    fn loads_transactions_through_the_include_graph() {
        let (dir, opts) = vault_with(&[
            ("2026/2026-10.ledger", "include 2026-10-25.ledger\n"),
            ("2026/2026-10-25.ledger", TXN),
        ]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert_eq!(vault.txns.len(), 1);
        // index, accounts, prices, budgets, year, month, day
        assert_eq!(vault.files.len(), 7);
        let t = &vault.txns[0];
        assert_eq!(t.id, "01JA3KQ7");
        assert_eq!(t.txn.payee, "星巴克");
        assert!(t.balanced);
        assert_eq!(t.base_value, Some(Decimal::new(450, 2)));
        let nw = vault.net_worth();
        assert_eq!(nw[0].quantity, "-4.50");
    }

    #[test]
    fn infers_account_kinds_from_declarations_and_top_level() {
        let (dir, opts) = vault_with(&[("2026/2026-10-25.ledger", TXN)]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert_eq!(vault.accounts["expenses:food:coffee"].kind, 'X');
        assert_eq!(vault.accounts["assets:wallet:alipay"].kind, 'A');
        assert_eq!(vault.accounts["income:salary"].kind, 'R');
        assert_eq!(vault.accounts["assets"].kind, 'A');
    }

    #[test]
    fn assigns_pseudo_ids_to_handwritten_transactions() {
        let (dir, opts) = vault_with(&[(
            "2026/2026-10-25.ledger",
            "2026-10-25 handwritten\n    expenses:food  1 CNY\n    assets:cash   -1 CNY\n",
        )]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert!(vault.txns[0].id.starts_with("x-"));
        assert_eq!(vault.txns[0].id.len(), 18);
    }

    #[test]
    fn reports_unbalanced_transactions_without_failing_the_load() {
        let (dir, opts) = vault_with(&[(
            "2026/2026-10-25.ledger",
            "2026-10-25 broken\n    expenses:food  10 CNY\n    assets:cash   -9 CNY\n",
        )]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert!(!vault.txns[0].balanced);
        assert!(vault
            .diagnostics
            .iter()
            .any(|d| d.code == "balance.not_zero"));
    }

    #[test]
    fn detects_include_cycles() {
        let (dir, opts) = vault_with(&[
            ("2026/2026-10.ledger", "include 2026-10-25.ledger\n"),
            (
                "2026/2026-10-25.ledger",
                "include ../2026/2026-10.ledger\n2026-10-25 x\n    expenses:a  1 CNY\n    assets:b\n",
            ),
        ]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert!(vault
            .diagnostics
            .iter()
            .any(|d| d.message.contains("include cycle")));
    }

    #[test]
    fn optional_missing_include_is_silent() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(INDEX_FILE), "!include nope.ledger\n").unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        assert!(vault.diagnostics.is_empty());
    }

    #[test]
    fn missing_include_errors_with_position() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(INDEX_FILE), "; x\ninclude nope.ledger\n").unwrap();
        let vault = Vault::load(dir.path(), &VaultOptions::default()).unwrap();
        let d = vault
            .diagnostics
            .iter()
            .find(|d| d.code == "include.missing")
            .unwrap();
        assert_eq!(d.line, 2);
    }

    #[test]
    fn glob_includes_load_all_matching_files() {
        let (dir, opts) = vault_with(&[
            (
                "2026/2026-10-24.ledger",
                "2026-10-24 a\n    expenses:a  1 CNY\n    assets:b\n",
            ),
            (
                "2026/2026-10-25.ledger",
                "2026-10-25 b\n    expenses:a  2 CNY\n    assets:b\n",
            ),
        ]);
        std::fs::write(
            dir.path().join("2026/2026-10.ledger"),
            "include 2026-10-*.ledger\n",
        )
        .unwrap();
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert_eq!(vault.txns.len(), 2);
    }

    #[test]
    fn fx_missing_is_tracked_and_excluded_from_base_value() {
        let (dir, opts) = vault_with(&[(
            "2026/2026-10-25.ledger",
            "2026-10-25 trip\n    expenses:travel  100 USD\n    assets:cash  -100 USD\n",
        )]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        let t = &vault.txns[0];
        assert!(t.fx_missing);
        assert_eq!(t.base_value, None);
    }

    #[test]
    fn fx_conversion_uses_the_price_table() {
        let (dir, opts) = vault_with(&[
            ("prices.ledger", "; rates\nP 2026-10-01 USD 7.10 CNY\n"),
            (
                "2026/2026-10-25.ledger",
                "2026-10-25 trip\n    expenses:travel  100 USD\n    assets:cash  -100 USD\n",
            ),
        ]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        let t = &vault.txns[0];
        assert!(!t.fx_missing);
        assert_eq!(t.base_value, Some(Decimal::new(71000, 2)));
    }

    #[test]
    fn duplicate_txn_ids_are_disambiguated() {
        let (dir, opts) = vault_with(&[(
            "2026/2026-10-25.ledger",
            "2026-10-25 a  ; txn:same\n    expenses:a  1 CNY\n    assets:b\n\n2026-10-25 b  ; txn:same\n    expenses:a  2 CNY\n    assets:b\n",
        )]);
        let vault = Vault::load(dir.path(), &opts).unwrap();
        assert_ne!(vault.txns[0].id, vault.txns[1].id);
    }

    #[test]
    fn segment_glob_matching() {
        assert!(segment_matches("*.ledger", "2026-10-25.ledger"));
        assert!(!segment_matches("*.ledger", "2026-10-25.txt"));
        assert!(segment_matches("2026-??-25.ledger", "2026-10-25.ledger"));
    }
}

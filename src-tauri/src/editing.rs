//! Span level rewrites: every mutation replaces exactly the bytes of one
//! transaction block (or appends a new block), never the whole file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Local;
use ledger_core::ast::{Posting, Status, Tag, Transaction, VirtualKind};
use ledger_core::{parse_amount, printer, Span};
use ledger_engine::amounts::{self, CommodityTable};
use ledger_engine::capture::{merge_tag_comment as merge_tags, ExpandedCapture};
use ledger_engine::rules::{Direction, RuleDef};
use ledger_engine::types::*;
use ledger_engine::vault::VaultOptions;
use ledger_engine::{writer, Vault};

use crate::state::{CmdError, CmdResult};

fn refused(msg: impl Into<String>) -> CmdError {
    CmdError::refused(msg)
}

fn now_iso() -> String {
    Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn splice(
    vault: &Vault,
    rel: &Path,
    span: Span,
    replacement: &str,
    opts: &VaultOptions,
    audit_path: &Path,
    op: &str,
) -> CmdResult<String> {
    // The state recorded when the vault was parsed is the contract: if the file
    // changed on disk since then, the write must be refused rather than applied to
    // bytes nobody has seen.
    let rel_key = rel.to_string_lossy().to_string();
    let recorded = vault
        .file(&rel_key)
        .map(|f| f.state.clone())
        .ok_or_else(|| {
            CmdError::new(
                "app.not_found",
                format!("{rel_key} is not part of the vault"),
            )
        })?;
    let (src, _current) = writer::read_rel(&vault.root, rel)?;
    if span.end > src.len() || span.start > span.end {
        return Err(refused(format!(
            "stale span {}..{} for {} (file is {} bytes)",
            span.start,
            span.end,
            rel.display(),
            src.len()
        )));
    }
    let mut next = String::with_capacity(src.len() + replacement.len());
    next.push_str(&src[..span.start]);
    next.push_str(replacement);
    next.push_str(&src[span.end..]);

    writer::snapshot(&vault.root, rel, opts.max_backups)?;
    writer::write_rel(&vault.root, rel, &next, Some(&recorded), opts)?;
    let _ = writer::append_audit(
        audit_path,
        &AuditEntryDto {
            ts: now_iso(),
            op: op.to_string(),
            file: rel.to_string_lossy().to_string(),
            span_start: span.start,
            span_end: span.end,
            before: src[span.start..span.end].chars().take(400).collect(),
            after: replacement.chars().take(400).collect(),
        },
    );
    Ok(next)
}

fn build_postings(
    patches: &[PostingPatch],
    table: &CommodityTable,
    base: &str,
) -> CmdResult<Vec<Posting>> {
    let mut out = Vec::new();
    for patch in patches {
        let account = ledger_engine::load::normalize_account(&patch.account);
        if account.is_empty() {
            return Err(refused("a posting needs an account"));
        }
        let amount = match patch.amount.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(raw) => {
                let parsed = parse_amount(raw)
                    .map_err(|e| refused(format!("cannot parse amount \"{raw}\": {e}")))?;
                let commodity = if parsed.commodity.is_empty() {
                    base.to_string()
                } else {
                    parsed.commodity.clone()
                };
                let precision = table.precision_of(&commodity);
                Some(ledger_core::Amount::canonical(
                    parsed.quantity,
                    &commodity,
                    precision,
                ))
            }
        };
        out.push(Posting {
            span: Span::new(0, 0),
            status: None,
            account,
            virtual_kind: VirtualKind::Real,
            amount,
            percent: None,
            price: None,
            balance_assertion: None,
            comment: patch.comment.clone().filter(|c| !c.trim().is_empty()),
            tags: Vec::new(),
        });
    }
    Ok(out)
}

fn balance_or_refuse(
    postings: &mut [Posting],
    table: &CommodityTable,
    base: &str,
) -> CmdResult<()> {
    match amounts::balance(postings, table, base) {
        Ok(()) => Ok(()),
        Err(issues) => Err(refused(
            issues
                .iter()
                .map(|i| i.message.clone())
                .collect::<Vec<_>>()
                .join("; "),
        )),
    }
}

/// Rewrite one transaction in place.
pub fn update_txn(
    vault: &Vault,
    id: &str,
    patch: &SaveTxnPatch,
    opts: &VaultOptions,
    audit_path: &Path,
) -> CmdResult<String> {
    let rec = vault
        .transaction(id)
        .ok_or_else(|| CmdError::new("app.not_found", format!("transaction {id} not found")))?;
    if !rec.editable {
        return Err(refused(
            "this transaction has parse errors; fix the file (or your input) before editing it here",
        ));
    }
    let rel = PathBuf::from(&rec.file);
    let span = rec.txn.span;
    let mut txn = rec.txn.clone();

    if let Some(date) = patch.date.as_deref().filter(|d| !d.trim().is_empty()) {
        txn.date = ledger_core::parser::parse_date_str(date.trim())
            .ok_or_else(|| refused(format!("invalid date \"{date}\"")))?;
    }
    if let Some(payee) = &patch.payee {
        txn.payee = payee.trim().to_string();
    }
    if let Some(status) = patch.status.as_deref() {
        txn.status = match status.trim() {
            "" | "unmarked" | "none" => None,
            "cleared" | "*" => Some(Status::Cleared),
            "pending" | "!" => Some(Status::Pending),
            other => return Err(refused(format!("unknown status \"{other}\""))),
        };
    }
    if let Some(tags) = &patch.tags {
        let tags: Vec<Tag> = tags
            .iter()
            .filter(|t| !t.key.trim().is_empty())
            .map(|t| match &t.value {
                Some(v) if !v.trim().is_empty() => Tag::kv(t.key.trim(), v.trim()),
                _ => Tag::bare(t.key.trim()),
            })
            .collect();
        txn.comment = merge_tags(txn.comment.as_deref(), &tags);
        txn.tags = tags;
    }
    if let Some(comment) = &patch.comment {
        let merged = merge_tags(
            Some(comment),
            &txn.tags
                .iter()
                .filter(|t| t.key != "txn")
                .cloned()
                .collect::<Vec<_>>(),
        );
        let with_id = match (&merged, txn.tag("txn")) {
            (Some(text), Some(id)) => Some(format!("txn:{id} {text}")),
            (None, Some(id)) => Some(format!("txn:{id}")),
            (m, None) => m.clone(),
        };
        txn.comment = with_id;
    }
    if let Some(patches) = &patch.postings {
        if patches.is_empty() {
            return Err(refused("a transaction needs at least two postings"));
        }
        txn.postings = build_postings(patches, &vault.commodities, &vault.opts.base_commodity)?;
    }

    let base = vault.opts.base_commodity.clone();
    balance_or_refuse(&mut txn.postings, &vault.commodities, &base)?;
    let canonical = printer::print_transaction(&txn);
    splice(
        vault,
        &rel,
        span,
        &canonical,
        opts,
        audit_path,
        "txn_update",
    )?;
    Ok(canonical)
}

/// Remove a transaction block (and the blank line that followed it).
pub fn delete_txn(
    vault: &Vault,
    id: &str,
    opts: &VaultOptions,
    audit_path: &Path,
) -> CmdResult<()> {
    let rec = vault
        .transaction(id)
        .ok_or_else(|| CmdError::new("app.not_found", format!("transaction {id} not found")))?;
    let rel = PathBuf::from(&rec.file);
    let (src, state) = writer::read_rel(&vault.root, &rel)?;
    let mut start = rec.txn.span.start;
    let mut end = rec.txn.span.end;
    while end < src.len() && (src.as_bytes()[end] == b'\n' || src.as_bytes()[end] == b'\r') {
        end += 1;
    }
    if end < src.len() && src.as_bytes()[end] == b'\n' {
        end += 1;
    }
    // Swallow one preceding blank line so repeated deletions do not leave a
    // growing run of empty lines behind.
    if start >= 2 && src.as_bytes()[start - 1] == b'\n' && src.as_bytes()[start - 2] == b'\n' {
        start -= 1;
    }
    let mut next = String::with_capacity(src.len());
    next.push_str(&src[..start]);
    next.push_str(&src[end..]);
    writer::snapshot(&vault.root, &rel, opts.max_backups)?;
    writer::write_rel(&vault.root, &rel, &next, Some(&state), opts)?;
    let _ = writer::append_audit(
        audit_path,
        &AuditEntryDto {
            ts: now_iso(),
            op: "txn_delete".into(),
            file: rel.to_string_lossy().to_string(),
            span_start: start,
            span_end: end,
            before: src[start..end].chars().take(400).collect(),
            after: String::new(),
        },
    );
    Ok(())
}

/// Apply tag/account changes to many transactions.
pub fn bulk_update(
    vault: &Vault,
    ids: &[String],
    patch: &BulkPatch,
    opts: &VaultOptions,
    audit_path: &Path,
) -> CmdResult<usize> {
    // Several transactions usually live in the same day file, so collect the edits
    // first and rewrite each file once — spans of later blocks stay valid.
    let mut per_file: BTreeMap<String, Vec<(Span, String)>> = BTreeMap::new();
    for id in ids {
        let Some(rec) = vault.transaction(id) else {
            continue;
        };
        let mut txn = rec.txn.clone();
        let mut tags: Vec<Tag> = txn.tags.clone();
        for raw in &patch.add_tags {
            let raw = raw.trim().trim_matches(':');
            if raw.is_empty() {
                continue;
            }
            let tag = match raw.split_once(':') {
                Some((k, v)) if !v.is_empty() => Tag::kv(k, v),
                _ => Tag::bare(raw),
            };
            if !tags
                .iter()
                .any(|t| t.key == tag.key && t.value == tag.value)
            {
                tags.push(tag);
            }
        }
        let removes: Vec<&str> = patch
            .remove_tags
            .iter()
            .map(|t| t.trim().trim_matches(':'))
            .collect();
        // `txn:` is the identity of the transaction and survives every edit.
        tags.retain(|t| {
            t.key == "txn"
                || !removes
                    .iter()
                    .any(|r| *r == t.key || r.eq_ignore_ascii_case(&t.key))
        });
        txn.comment = merge_tags(txn.comment.as_deref(), &tags);
        txn.tags = tags;
        if let Some((from, to)) = &patch.shift_account {
            let from = ledger_engine::load::normalize_account(from);
            let to = ledger_engine::load::normalize_account(to);
            for posting in &mut txn.postings {
                if posting.account == from || posting.account.starts_with(&format!("{from}:")) {
                    let suffix = posting.account[from.len()..].to_string();
                    posting.account = format!("{to}{suffix}");
                }
            }
        }
        let base = vault.opts.base_commodity.clone();
        balance_or_refuse(&mut txn.postings, &vault.commodities, &base)?;
        let canonical = printer::print_transaction(&txn);
        per_file
            .entry(rec.file.clone())
            .or_default()
            .push((rec.txn.span, canonical));
    }

    let mut done = 0usize;
    for (file, mut edits) in per_file {
        edits.sort_by_key(|(span, _)| span.start);
        let rel = PathBuf::from(&file);
        let recorded = vault.file(&file).map(|f| f.state.clone()).ok_or_else(|| {
            CmdError::new("app.not_found", format!("{file} is not part of the vault"))
        })?;
        let (src, _current) = writer::read_rel(&vault.root, &rel)?;

        let mut next = String::with_capacity(src.len());
        let mut cursor = 0usize;
        for (span, text) in &edits {
            if span.start < cursor || span.end > src.len() || span.start > span.end {
                return Err(refused(format!(
                    "stale span {}..{} for {file} (file is {} bytes)",
                    span.start,
                    span.end,
                    src.len()
                )));
            }
            next.push_str(&src[cursor..span.start]);
            next.push_str(text);
            cursor = span.end;
        }
        next.push_str(&src[cursor..]);

        writer::snapshot(&vault.root, &rel, opts.max_backups)?;
        writer::write_rel(&vault.root, &rel, &next, Some(&recorded), opts)?;
        let _ = writer::append_audit(
            audit_path,
            &AuditEntryDto {
                ts: now_iso(),
                op: "txn_bulk_update".into(),
                file: file.clone(),
                span_start: 0,
                span_end: next.len(),
                before: format!("{} transaction(s)", edits.len()),
                after: format!("{} block(s) rewritten", edits.len()),
            },
        );
        done += edits.len();
    }
    Ok(done)
}

/// Re-run a rule over one transaction, keeping the funding account.
pub fn expand_rule(vault: &Vault, txn: &Transaction, rule: &RuleDef) -> Option<Vec<Posting>> {
    let total = txn
        .postings
        .iter()
        .filter(|p| {
            let kind = vault
                .accounts
                .get(&p.account)
                .map(|a| a.kind)
                .unwrap_or('U');
            kind == 'X' || kind == 'R'
        })
        .filter_map(|p| p.effective_amount())
        .map(|a| a.quantity.abs())
        .max()?;
    let dir = if txn
        .postings
        .iter()
        .any(|p| vault.accounts.get(&p.account).map(|a| a.kind) == Some('R'))
    {
        Direction::Income
    } else {
        Direction::Expense
    };
    let expansion = rule.expand(
        &ledger_core::Amount {
            quantity: total,
            commodity: vault.opts.base_commodity.clone(),
            style: ledger_core::AmountStyle::default(),
        },
        dir,
    );
    let mut postings = Vec::new();
    let mut sum = rust_decimal::Decimal::ZERO;
    for (account, value) in expansion.postings {
        sum += value.quantity;
        postings.push(Posting {
            span: Span::new(0, 0),
            status: None,
            account,
            virtual_kind: VirtualKind::Real,
            amount: Some(value),
            percent: None,
            price: None,
            balance_assertion: None,
            comment: None,
            tags: Vec::new(),
        });
    }
    if let Some(funding) = expansion.funding {
        postings.push(Posting {
            span: Span::new(0, 0),
            status: None,
            account: funding,
            virtual_kind: VirtualKind::Real,
            amount: Some(ledger_core::Amount {
                quantity: -sum,
                commodity: vault.opts.base_commodity.clone(),
                style: ledger_core::AmountStyle::default(),
            }),
            percent: None,
            price: None,
            balance_assertion: None,
            comment: None,
            tags: Vec::new(),
        });
    }
    Some(postings)
}

pub fn recompute(
    vault: &Vault,
    ids: &[String],
    opts: &VaultOptions,
    audit_path: &Path,
) -> CmdResult<RuleRecomputeDto> {
    let mut updated = Vec::new();
    let mut skipped = Vec::new();
    for id in ids {
        let Some(rec) = vault.transaction(id) else {
            skipped.push(id.clone());
            continue;
        };
        let Some(rule_id) = rec.txn.tag("rule") else {
            skipped.push(id.clone());
            continue;
        };
        let Some(rule) = vault.rules.get(rule_id) else {
            skipped.push(id.clone());
            continue;
        };
        let Some(postings) = expand_rule(vault, &rec.txn, rule) else {
            skipped.push(id.clone());
            continue;
        };
        let mut txn = rec.txn.clone();
        txn.postings = postings;
        txn.comment = merge_tags(
            txn.comment.as_deref(),
            &[Tag::kv("recomputed", Local::now().to_rfc3339())],
        );
        let base = vault.opts.base_commodity.clone();
        balance_or_refuse(&mut txn.postings, &vault.commodities, &base)?;
        let canonical = printer::print_transaction(&txn);
        splice(
            vault,
            Path::new(&rec.file),
            rec.txn.span,
            &canonical,
            opts,
            audit_path,
            "rule_recompute",
        )?;
        updated.push(id.clone());
    }
    Ok(RuleRecomputeDto {
        updated,
        skipped,
        diagnostics: Vec::new(),
    })
}

/// Append a freshly captured transaction to its day file.
pub fn commit(
    vault: &Vault,
    expanded: &ExpandedCapture,
    txn_id: &str,
    opts: &VaultOptions,
    audit_path: &Path,
) -> CmdResult<CaptureCommitDto> {
    let block = ledger_engine::capture::commit_to_day_file(vault, expanded, txn_id, opts)?;
    let _ = writer::append_audit(
        audit_path,
        &AuditEntryDto {
            ts: now_iso(),
            op: "capture_commit".into(),
            file: block.rel.clone(),
            span_start: block.span.start,
            span_end: block.span.end,
            before: String::new(),
            after: block.canonical.chars().take(400).collect(),
        },
    );
    Ok(CaptureCommitDto {
        file: block.rel,
        span_start: block.span.start,
        span_end: block.span.end,
        txn_id: txn_id.to_string(),
        canonical_text: block.canonical,
        created_file: block.created_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger_engine::types::VaultCreateOptions;
    use ledger_engine::vault::create_vault;

    struct Fixture {
        dir: tempfile::TempDir,
        audit: PathBuf,
        opts: VaultOptions,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            create_vault(dir.path(), &VaultCreateOptions::default()).unwrap();
            let audit = dir.path().join("write-audit.jsonl");
            Self {
                dir,
                audit,
                opts: VaultOptions::default(),
            }
        }

        fn vault(&self) -> Vault {
            Vault::load(self.dir.path(), &self.opts).unwrap()
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        /// Capture `text` and commit it, returning the new transaction id.
        fn capture(&self, text: &str, id: &str) -> CaptureCommitDto {
            let vault = self.vault();
            let analysis = ledger_engine::analyze(&vault, text);
            assert!(
                analysis.errors.is_empty(),
                "capture errors for {text:?}: {:?}",
                analysis.errors
            );
            let expanded = analysis.expanded.expect("expanded capture");
            commit(&vault, &expanded, id, &self.opts, &self.audit).unwrap()
        }
    }

    fn patch(payee: &str) -> SaveTxnPatch {
        SaveTxnPatch {
            payee: Some(payee.into()),
            ..Default::default()
        }
    }

    #[test]
    fn commit_writes_a_canonical_block_and_reloads_with_the_same_id() {
        let fx = Fixture::new();
        let written = fx.capture("#2026-10-04 星巴克 45 @微信 +coffee", "01JA3KQ7TEST0001");

        assert_eq!(written.file, "2026/2026-10-04.ledger");
        assert!(written.created_file);
        let text = std::fs::read_to_string(fx.path(&written.file)).unwrap();
        assert!(text.contains("txn:01JA3KQ7TEST0001"), "{text}");
        assert!(text.contains(":coffee:"), "{text}");
        assert!(text.contains("assets:微信"), "{text}");
        // The include chain was created so the loader can see the file.
        let index = std::fs::read_to_string(fx.path("index.ledger")).unwrap();
        assert!(index.contains("include 2026/2026.ledger"));
        let year = std::fs::read_to_string(fx.path("2026/2026.ledger")).unwrap();
        assert!(year.contains("include 2026-10.ledger"), "{year}");

        let vault = fx.vault();
        let rec = vault.transaction("01JA3KQ7TEST0001").expect("reloaded");
        assert_eq!(rec.txn.payee, "星巴克");
        assert!(rec.balanced);
        assert!(rec.editable);
        assert_eq!(vault.transactions().len(), 1);
    }

    #[test]
    fn repeated_captures_keep_every_block_and_stay_ordered() {
        let fx = Fixture::new();
        for i in 1..=5 {
            fx.capture(
                &format!("#2026-10-04 记录 {i} {i}.00 @微信"),
                &format!("01JA3KQ7TEST000{i}"),
            );
        }
        let text = std::fs::read_to_string(fx.path("2026/2026-10-04.ledger")).unwrap();
        assert_eq!(text.matches("txn:").count(), 5, "{text}");
        assert!(text.ends_with('\n'));
        assert!(
            !text.contains("\n\n\n"),
            "blocks are separated by one blank line"
        );
        assert_eq!(fx.vault().transactions().len(), 5);
    }

    #[test]
    fn update_rewrites_only_the_target_block() {
        let fx = Fixture::new();
        fx.capture("#2026-10-04 星巴克 45 @微信", "01JA3KQ7TEST0001");
        fx.capture("#2026-10-04 地铁 6 @微信", "01JA3KQ7TEST0002");

        let vault = fx.vault();
        let rel = "2026/2026-10-04.ledger";
        let before = std::fs::read_to_string(fx.path(rel)).unwrap();
        let rec = vault.transaction("01JA3KQ7TEST0001").unwrap();
        let (start, end) = (rec.txn.span.start, rec.txn.span.end);

        update_txn(
            &vault,
            "01JA3KQ7TEST0001",
            &patch("星巴克臻选"),
            &fx.opts,
            &fx.audit,
        )
        .unwrap();
        let after = std::fs::read_to_string(fx.path(rel)).unwrap();

        assert_eq!(
            &before[..start],
            &after[..start],
            "prefix must be untouched"
        );
        let suffix_len = before.len() - end;
        assert_eq!(
            &before[end..],
            &after[after.len() - suffix_len..],
            "suffix must be untouched (only the block changed)"
        );
        assert!(after.contains("星巴克臻选"));
        assert!(!after.contains("星巴克 45"));
    }

    #[test]
    fn delete_removes_the_block_and_one_blank_line() {
        let fx = Fixture::new();
        fx.capture("#2026-10-04 星巴克 45 @微信", "01JA3KQ7TEST0001");
        fx.capture("#2026-10-04 地铁 6 @微信", "01JA3KQ7TEST0002");

        let vault = fx.vault();
        delete_txn(&vault, "01JA3KQ7TEST0001", &fx.opts, &fx.audit).unwrap();
        let text = std::fs::read_to_string(fx.path("2026/2026-10-04.ledger")).unwrap();

        assert!(!text.contains("星巴克"), "{text}");
        assert!(text.contains("地铁"), "{text}");
        assert!(!text.starts_with('\n'), "no leading blank line: {text:?}");
        let vault = fx.vault();
        assert_eq!(vault.transactions().len(), 1);
        assert!(vault.transaction("01JA3KQ7TEST0002").is_some());
    }

    #[test]
    fn a_snapshot_and_an_audit_entry_are_written_before_the_change() {
        let fx = Fixture::new();
        fx.capture("#2026-10-04 星巴克 45 @微信", "01JA3KQ7TEST0001");
        let vault = fx.vault();
        update_txn(
            &vault,
            "01JA3KQ7TEST0001",
            &patch("改名"),
            &fx.opts,
            &fx.audit,
        )
        .unwrap();

        let backups: Vec<_> = std::fs::read_dir(fx.dir.path().join(".bak/2026"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!backups.is_empty(), "a pre-write snapshot exists");
        let audit = std::fs::read_to_string(&fx.audit).unwrap();
        assert!(audit.contains("\"op\":\"txn_update\""), "{audit}");
        assert!(audit.contains("\"op\":\"capture_commit\""), "{audit}");
    }

    #[test]
    fn refuses_to_write_when_the_file_changed_outside_the_app() {
        let fx = Fixture::new();
        fx.capture("#2026-10-04 星巴克 45 @微信", "01JA3KQ7TEST0001");
        let vault = fx.vault();

        // Simulate an external editor touching the file after it was parsed.
        let rel = fx.path("2026/2026-10-04.ledger");
        let mut text = std::fs::read_to_string(&rel).unwrap();
        text.push_str("\n; touched by another process\n");
        std::fs::write(&rel, text).unwrap();

        let err = update_txn(
            &vault,
            "01JA3KQ7TEST0001",
            &patch("改名"),
            &fx.opts,
            &fx.audit,
        )
        .expect_err("write must be refused");
        assert_eq!(err.code, "engine.conflict");
        assert!(err.message.contains("modified outside"), "{}", err.message);
    }

    #[test]
    fn bulk_update_adds_and_removes_tags_without_touching_postings() {
        let fx = Fixture::new();
        fx.capture("#2026-10-04 星巴克 45 @微信", "01JA3KQ7TEST0001");
        fx.capture("#2026-10-04 地铁 6 @微信", "01JA3KQ7TEST0002");
        let vault = fx.vault();
        let before = std::fs::read_to_string(fx.path("2026/2026-10-04.ledger")).unwrap();

        let done = bulk_update(
            &vault,
            &["01JA3KQ7TEST0001".into(), "01JA3KQ7TEST0002".into()],
            &BulkPatch {
                add_tags: vec!["报销".into(), "trip:tokyo".into()],
                remove_tags: vec![],
                shift_account: None,
            },
            &fx.opts,
            &fx.audit,
        )
        .unwrap();
        assert_eq!(done, 2);

        let after = std::fs::read_to_string(fx.path("2026/2026-10-04.ledger")).unwrap();
        assert_eq!(after.matches(":报销:").count(), 2, "{after}");
        assert_eq!(after.matches("trip:tokyo").count(), 2, "{after}");
        // Postings are byte-identical: only the header comments changed.
        let postings = |s: &str| {
            s.lines()
                .filter(|l| l.starts_with("    "))
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(postings(&before), postings(&after));

        let vault = fx.vault();
        let ids: Vec<&str> = vault.transactions().iter().map(|t| t.id.as_str()).collect();
        let rec = vault
            .transaction("01JA3KQ7TEST0002")
            .unwrap_or_else(|| panic!("reloaded ids {ids:?} from\n{after}"));
        assert!(rec.txn.tags.iter().any(|t| t.key == "报销"));
        assert!(rec
            .txn
            .tags
            .iter()
            .any(|t| t.key == "trip" && t.value.as_deref() == Some("tokyo")));
    }
}

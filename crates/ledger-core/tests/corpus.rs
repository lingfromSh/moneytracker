//! Grammar corpus: every `fixtures/grammar/*.ledger` must parse cleanly, survive a
//! `parse → print → parse` round trip, and print idempotently. Every
//! `fixtures/errors/*.ledger` must fail with the documented code on the documented line.

use std::fs;
use std::path::{Path, PathBuf};

use ledger_core::{parse_journal, print_journal, Severity};

fn fixtures_dir(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(kind)
}

/// Byte spans legitimately move when the text is reformatted, so the round-trip
/// contract compares the semantic AST only.
fn strip_spans(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(key, _)| key != "span")
                .map(|(key, val)| (key, strip_spans(val)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(strip_spans).collect())
        }
        other => other,
    }
}

fn ledger_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "ledger"))
        .collect();
    files.sort();
    files
}

#[test]
fn grammar_corpus_parses_without_errors() {
    let dir = fixtures_dir("grammar");
    let files = ledger_files(&dir);
    assert!(
        !files.is_empty(),
        "expected grammar fixtures in {}",
        dir.display()
    );

    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let output = parse_journal(&name, &src);
        let errors: Vec<String> = output
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.display())
            .collect();
        assert!(errors.is_empty(), "{name} produced errors: {errors:#?}");
    }
}

#[test]
fn grammar_corpus_round_trips_idempotently() {
    for path in ledger_files(&fixtures_dir("grammar")) {
        let src = fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let first = parse_journal(&name, &src);
        let printed = print_journal(&first.journal);
        let second = parse_journal(&name, &printed);
        let printed_again = print_journal(&second.journal);

        assert_eq!(
            printed, printed_again,
            "{name}: printing is not idempotent (parse→print→parse changed the text)"
        );

        let left = strip_spans(serde_json::to_value(&first.journal).unwrap());
        let right = strip_spans(serde_json::to_value(&second.journal).unwrap());
        assert_eq!(left, right, "{name}: AST changed after a round trip");

        let leftover: Vec<String> = second
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.display())
            .collect();
        assert!(
            leftover.is_empty(),
            "{name}: reprint produced errors: {leftover:#?}"
        );
    }
}

#[test]
fn error_corpus_reports_documented_codes_and_lines() {
    let dir = fixtures_dir("errors");
    let expected_src = fs::read_to_string(dir.join("expected.txt")).unwrap();

    for line in expected_src.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split('|');
        let file = parts.next().unwrap().trim();
        let code = parts.next().unwrap().trim();
        let line_no: u32 = parts.next().unwrap().trim().parse().unwrap();

        let src = fs::read_to_string(dir.join(file)).unwrap();
        let output = parse_journal(file, &src);
        let hit = output
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error && d.code == code)
            .unwrap_or_else(|| {
                let seen: Vec<String> = output
                    .diagnostics
                    .iter()
                    .map(|d| format!("{}:{} {}", d.code, d.line, d.message))
                    .collect();
                panic!("{file}: expected error {code}, saw {seen:#?}")
            });

        assert_eq!(
            hit.line, line_no,
            "{file}: {code} reported on line {} instead of {line_no}",
            hit.line
        );
        assert!(hit.col > 0, "{file}: {code} has no column");
    }
}

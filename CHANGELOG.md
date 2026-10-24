# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The shortcut recorder records from a window-level listener (WebKit does not focus buttons on
  click), previews the modifiers as they are held, rejects `⇧`-only and macOS menu-owned chords
  inline, and cancels on Esc, a click elsewhere or a window blur.
- `shortcut_recording` command: the settings recorder releases the live global hotkey while it
  listens and restores it afterwards, so pressing the current chord cannot open the capture window
  and steal focus mid-recording.

- Shortcut recorder in **设置**: click and press the chord (Esc cancels, Backspace clears,
  重置 restores `CmdOrCtrl+Shift+Space`); the chord is validated before it is stored and a rejected
  accelerator is reported inline. New `shortcut_status` command exposes whether the global hotkey is
  actually registered.
- Dual-language documentation: `README.md` (English) and `README.zh-CN.md` (简体中文).
- CI workflow: `cargo fmt` / `clippy -D warnings` / `cargo test`, Vitest, `pnpm build`, a
  bindings-freshness check, and the `hledger` differential test on macOS runners.
- `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE` (MIT).
- `crates/ledger-engine/examples/bench_load.rs` — generates a 100 000-transaction vault and times
  a cold load, snapshot and dashboard render.

### Changed

- The IPC surface no longer exposes engine-owned transaction tokens: `txn:`, `rule:`, `query:` and
  `recomputed:` are stripped from `TxnDto.tags`/`TxnDto.comment` and re-injected by the write path,
  so editing a transaction can never drop its identity.

### Fixed

- Dashboard scrolled horizontally and clipped its right-hand column after the window was
  un-maximised. `react-grid-layout` only lays items out from the container width it saw on first
  render, and its stylesheet (`position: relative` for the grid) was never imported; the grid is now
  remounted on width change, the stylesheet is imported, every page scroller only scrolls
  vertically, and scrollbars are thin and themed.
- The `状态` select in **编辑交易** sat inline with its label and was narrower than the other fields
  (`Field` labels were inline elements and `Select` had no width), and the toolbar's `排序` select
  competed with the search input for width.
- The global capture hotkey toggled the capture window twice per keypress — the plugin-wide handler
  and a per-shortcut handler both fired — which looked like a dead shortcut. Only the plugin-wide
  handler remains.
- Changing the global shortcut now registers the new chord *before* releasing the old one, keeps the
  old registration when the chord is taken by another app, and reports the failure instead of
  silently persisting a shortcut that never worked.

## [0.1.0] — 2026-10-25

First complete release: the ledger format, the engine and every view are in place.

### Added

- **Ledger core** (`crates/ledger-core`): hand-written, line-oriented parser for a
  [hledger](https://hledger.org)-compatible subset with exact byte spans per item, a printer that is
  a fixed point (`print → parse → print` is byte-identical), and diagnostics that carry
  `file:line:col` plus a stable code. Anything outside the subset — amount expressions, lot prices
  `{}`, `C`/`N` conversions, timeclock, `csv:` rules — is rejected explicitly, never silently.
- **Query language** (`crates/ledger-query`): `acct:`/`account:`, `desc:`, `code:`, `status:`,
  `cur:`, `tag:` (bare or `k=v`), `date:` (prefix, comparison, range) and `amt:`, with
  `and`/`or`/`not` and parentheses.
- **Engine** (`crates/ledger-engine`): `index → year → month → day` include graph with globs and
  cycle detection; index of transactions, accounts (kind inference from `type:` tags or top-level
  name) and commodities; `rust_decimal` end to end; span-level rewrites; rules expanded at write
  time with `; rule:` provenance; FX from `P` directives with `fx_missing` tracking; budgets,
  forecast, Sankey, heatmap and KPI reports.
- **Capture language**: `[#date] [*|!] description amount [@account]… [+tag]… [; comment]`, with
  relative dates, inline prices (`100usd@@720cny`), transfers (`> @a > @b`), tag-derived
  categories, `=` rule creation, `/` search and `:` commands — parsed in Rust so the preview and the
  committed block cannot drift.
- **Write safety**: pre-write snapshots under `.bak/`, atomic replace with `fsync`, conflict
  detection on `(mtime, size, blake3)` recorded at parse time, and a JSONL write audit.
- **Desktop app** (`src-tauri`): 34 commands and 3 events exported to TypeScript via `tauri-specta`;
  frameless 640×148 capture window; tray menu with `TrayCenter` positioning; configurable global
  hotkey; `notify` watcher with 300 ms debounce that ignores the app's own writes.
- **UI** (`src`): capture palette with live preview and account completion; draggable dashboard
  (KPI, monthly bars, net-worth curve, category donut, stacked area, heatmap, budget vs actual,
  forecast, Sankey); windowed transaction list with inline editor and bulk edits; account tree;
  category view by account or tag; rule table with trial runs and recompute; settings for vault,
  theme, shortcut, commodity, backups and FX sync.
- **Tests**: grammar and error corpora, sample-vault integration tests, write-path tests, a
  differential test against the real `hledger` binary, 15 Vitest tests, and `scripts/smoke.sh` to
  run everything at once.

### Notes

- `specta` 2.0.0-rc.26 is vendored under `vendor/specta` with a two-line patch, because it uses the
  unstable `fmt::from_fn` helper and `tauri-specta` pins that exact version.
- The app targets macOS 13+; the Rust core is portable but the window/tray layer is not.

[Unreleased]: https://github.com/lingfromSh/moneytracker/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lingfromSh/moneytracker/releases/tag/v0.1.0

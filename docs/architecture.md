# Architecture

## Crates

| Crate | Responsibility |
| --- | --- |
| `crates/ledger-core` | AST, byte-span parser, canonical printer, diagnostics. No I/O. |
| `crates/ledger-query` | Query subset: `acct:` `desc:` `date:` `tag:` `amt:` `status:` `cur:` with `and` / `or` / `not` / parentheses. |
| `crates/ledger-engine` | Vault loading (include graph), index, balances, account types, capture parsing/expansion, rules, FX, budgets, forecast, reports, safe writing. |
| `src-tauri` | Tauri shell: windows, tray, global shortcut, file watcher, IPC command layer, generated TypeScript bindings. |
| `src` | React 19 UI. |

`ledger-engine` depends on `ledger-core` and `ledger-query`; `src-tauri` depends on all three;
the UI depends on nothing but the generated bindings.

## Data flow

```
file system ──▶ Vault::load ──▶ Arc<Vault> (immutable snapshot)
                    │                   │
          include graph, index          ├─▶ commands: snapshot / query / dashboard / rules …
          balances, FX, rules           └─▶ notify → ledger:changed → TanStack invalidate
```

`AppState` holds `Arc<RwLock<Option<Arc<Vault>>>>`, the settings file and the active
`notify::RecommendedWatcher`. Every window shares the same snapshot, so a dashboard and a
transaction list can never disagree about the books.

## Parser design

Line-oriented recursive descent, hand-written:

- `Parser` keeps `lines: Vec<Line{start, text}>` so every diagnostic and every AST node can carry
  a byte `Span`.
- Postings are recognised by the hledger account separator: a tab or **two or more spaces**.
  Amounts are parsed by `amount::parse_amount`, which handles prefix/suffix symbols, grouping and
  decimal commas without floating point.
- The printer is a fixed point: `print(parse(print(parse(x)))) == print(parse(x))`, asserted by the
  corpus tests. Padding is computed from display width (CJK counts as two columns) and is never
  less than two spaces, so a reprinted file stays parseable.
- The subset boundary is explicit: unsupported constructs raise a coded diagnostic instead of being
  silently skipped.

## Capture pipeline

```
text ──▶ capture::parse_capture ──▶ ParsedCapture
                                      │
                            resolve accounts (shorthands → full names)
                                      │
                        rules::RuleSet::first_match (order, then id)
                                      │
                     rules::RuleDef::expand (percent split, remainder to last)
                                      │
                    amounts::balance (fills the elided posting, checks zero)
                                      │
             print_transaction ──▶ CaptureParseDto (preview) or writer::append (commit)
```

`capture_parse` and `capture_commit` share that path, so the preview is exactly what gets written.

## Write path

`writer` is the only module that touches the disk for mutations:

- `atomic_write` — temp file in the target directory, `sync_all`, permission copy, `rename`, then
  `fsync` of the directory.
- `snapshot` — copy to `.bak/<rel>.<timestamp>`, prune to `max_backups`.
- `write_rel` — verifies the caller's `FileState` (mtime, size, blake3) before writing; a mismatch
  is a hard `EngineError::Conflict`.
- `ensure_day_file` — builds the include chain described in `docs/vault-format.md`.

`editing` composes those primitives for transaction updates, deletions, bulk tag/account changes
and rule recomputation; each call records an entry in `write-audit.jsonl`.

### Engine-owned transaction tokens

`txn:`, `rule:`, `query:` and `recomputed:` are bookkeeping metadata, not user tags. The IPC layer
hides them (`load::txn_dto` filters `tags` and strips the comment through `strip_system_tokens`), and
`editing::update_txn` re-injects them from the parsed record before writing. A client therefore never
has to echo the identity tag back, and an edit cannot silently turn a transaction into a
pseudo-id one.

## IPC surface

Commands are declared once with `#[tauri::command]` + `#[specta::specta]`, collected by
`build_specta()` in `src-tauri/src/lib.rs`, and exported to `src/lib/bindings.ts` by
`src-tauri/examples/export_bindings.rs` (`pnpm bindings`). That export is also how the frontend
build works without launching the GUI.

Return convention: `Result<T, CmdError>` becomes
`{status:"ok",data:T} | {status:"error",error:{code,message}}`; `src/lib/ipc.ts` unwraps it into a
throwing `ApiError`. Events: `ledger:changed`, `capture:saved`, `parse:diagnostics`.

`specta-typescript` 0.0.13 cannot export 64-bit integers, so exported numeric fields carry
`#[specta(type = f64)]` and command parameters use `u32`.

## Why specta is vendored

`tauri-specta` 2.0.0-rc.26 pins `specta =2.0.0-rc.26`, and that release calls the still-unstable
`std::fmt::from_fn`, which fails to compile on stable Rust 1.91. `specta` 2.0.0-rc.27 fixes it but
nothing in the `tauri-specta` line depends on rc.27 yet, so the newer version is unreachable from
the crate graph.

`vendor/specta` is the published rc.26 source with one local change in
`src/datatype/attributes.rs`: an equivalent `DynFmt` wrapper replaces `fmt::from_fn`, and
`Cargo.toml` applies it with

```toml
[patch.crates-io]
specta = { path = "vendor/specta" }
```

Drop the patch once a `tauri-specta` release depends on `specta >= 2.0.0-rc.27`.

## Frontend

- TanStack Query for server state (keys mirror command arguments, invalidated wholesale on
  `ledger:changed`), Zustand only for local UI state.
- Views: `Capture`, `Dashboard`, `Transactions`, `Accounts`, `Categories`, `Rules`, `Settings`,
  plus `Welcome` when no vault is open.
- `Dashboard` uses `react-grid-layout` 2.x (`GridLayout` + `gridConfig` / `dragConfig` /
  `resizeConfig`); the layout is persisted in settings, never in the vault.
- Charts are Recharts wrappers plus one `d3-sankey` diagram; every series coerces `null` to `0` for
  geometry while totals keep using the Rust-side decimal strings.
- `src/lib/window.ts` guards all Tauri window calls so components also render under Vitest.

### Global shortcut

One handler only. `tauri-plugin-global-shortcut` dispatches both the plugin-wide handler and any
per-shortcut handler for the same keypress, so registering the capture toggle in both places toggled
the window open and closed again. `shortcut::register` therefore only registers the chord; the
plugin-wide handler in `lib.rs` compares the pressed shortcut with `settings.shortcut` and toggles.
`shortcut::rebind` registers the new chord *before* releasing the old one, so a chord that another
application owns leaves the previous registration intact, and the failure is surfaced through
`shortcut_status` instead of being written to settings.

## Testing strategy

- `ledger-core`: grammar corpus (`fixtures/grammar/*.ledger`) parses clean, round-trips and prints
  idempotently; error corpus (`fixtures/errors/*.ledger` + `expected.txt`) asserts code and line.
- `ledger-engine`: include graph, globs, cycles, account kinds, FX, rules, capture expansion,
  reports, writer safety, and the checked-in `fixtures/vault/sample` vault end to end.
- `src`: mocked IPC with Testing Library; assertions on what would be sent over IPC, not on
  implementation details.
- `crates/ledger-engine/tests/hledger_diff.rs` (skipped when `hledger` is absent) compares
  `hledger balance --flat --no-total -O csv` with `Vault::balances()` for the sample vault, and
  asserts `hledger check` accepts a vault the engine just wrote; `scripts/smoke.sh` runs the same
  cross-check as part of the full pass.
- `Vault::balances()` reports native amounts (one row per account and commodity, exactly like
  hledger); base-commodity valuation only appears in `net_worth()` and the account tree totals.

## Performance

`crates/ledger-engine/examples/bench_load.rs` generates a synthetic vault and times a cold load:

```bash
cargo run -q --release -p ledger-engine --example bench_load -- 100000
```

Measured on an M-series laptop (release profile), 100 000 transactions spread over 5 183 files:

| Phase | Time |
|---|---|
| `Vault::load` (read + parse + index) | ~760 ms |
| `snapshot()` (account tree, balances) | ~70 ms |
| `dashboard(12 months)` | ~100 ms |
| parse alone (all sources, re-parse) | ~120 ms (≈112 MB/s) |

The transaction normalisation pass (balancing, FX conversion, account kinds) runs on a rayon pool
and is folded back into the shared indexes in document order, so ids, diagnostics and account
ordering stay deterministic. The frontend never loads the whole vault: `ledger_query` pages with
`limit` (default 100) and the Transactions view windows 200 rows at a time, which keeps first paint
under 2 s even for a 100 000-transaction vault.

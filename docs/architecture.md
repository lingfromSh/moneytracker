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

Two rules keep that lock out of trouble:

- **Commands that touch the vault are `#[tauri::command(async)]`**, so a parse of a large vault
  never runs on the event-loop thread — a synchronous command would freeze every window while it
  works.
- **Reloads go through `AppState::reload_from_disk`.** `std::sync::RwLock` is not re-entrant, so
  taking the write lock while the same thread still holds a read guard deadlocks it; because the
  caller is the event loop, that hangs the app permanently. The reload clones the current `Arc`,
  drops the guard, loads, and only then stores. The field is private and every path (commit,
  delete, bulk edit, watcher) funnels through that method.

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

### Multi-posting captures

`parse_capture` records one `Allocation { account, amount, percent }` per `@account` mention, and
`expand_split` takes over as soon as two accounts are present and at least one carries a value:

- percentages are resolved against the line total (`split_percent`, remainder to the last percent in
  a group that adds up to 100 %);
- each posting's side comes from the account kind (`A`/`L` → funding, `R`/`X`/`E` → category), so
  either side — or both — can be split;
- at most one account may stay elided; `amounts::balance` fills it, so an entry never has to be
  written twice just because two wallets paid;
- a group whose explicit amounts do not add up to the line total is reported
  (`分类合计 … 与总额 … 不一致`) before `balance()` runs, so the user sees the arithmetic error rather
  than a generic imbalance.

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

## Natural-language capture (optional)

`src-tauri/src/llm.rs` is an *adapter*, not a second writer. The capture palette has two modes and
<kbd>Tab</kbd> switches between them:

1. **Hledger mode** — `capture_parse(text)` → `capture_commit(text)`.
2. **自然语言 mode** — `llm_draft(text)` → the draft's `line` is parsed and previewed, and Enter
   commits *that line*, never the prose.

The split exists so the model can only produce a draft, never ledger text:

```
sentence ──► llm_draft ──► DraftLine {date, payee, amount, commodity, category, funding, tags}
                              │
                              ▼  (deterministic, in Rust)
                          "火锅 168 @微信 +餐饮" ──► capture_parse ──► capture_commit
```

Between the draft and the line sits a grounding pass, because models invent accounts and guess the
wrong wallet:

| Step | Function | Behaviour |
| --- | --- | --- |
| Prompt catalogue | `AccountCatalog::from_names` + `render` | The prompt lists the vault's own accounts grouped by kind. Bare tree roots (`assets`, `expenses`, …) are filtered out because posting to them only loses information. |
| Worked examples | `system_prompt` | Few-shot JSON examples are built from *this* ledger's spellings (`payment_hint("微信")`, the first income/asset account, a liability only when it looks like a card), so an example can never name an account that does not exist. |
| Payment method | `payment_hint` | 「用微信付的」 overrides the model's `funding` with the WeChat account, or with the wallet parent when no sub-account exists. |
| Account repair | `AccountCatalog::resolve` | exact → unique suffix (`bank:cmb` → `assets:bank:cmb`) → closest existing ancestor (`assets:wallet:alipay` → `assets:wallet`) → payment-keyword match. Unresolvable names are left alone so `Vault::resolve_account` can fall back to the rule or `:inbox:` path. |
| Value cleanup | `clean_commodity`, `tidy_amount` | `"NONE"`/`"null"`/`"无"` are not currencies and become `null`; symbols map to codes (`$` → `USD`, `RMB` → `CNY`); `45.0` becomes `45`. |

`synthesize_line()` then composes the capture grammar (date directive only when it is not
today, `@category`, `@funding`, `+tags`, quoted payee when it contains spaces). Accounts the model
returns are resolved against the vault exactly like typed ones (`Vault::resolve_account`), so an
unknown account is still an `:inbox:` fallback rather than a new account invented by a model.

Both request formats are supported by one `Protocol` enum and one `complete()` entry point:

| Protocol | Request | Response field |
| --- | --- | --- |
| `openai` | `POST {base}/chat/completions` with `response_format: json_object` | `choices[0].message.content` |
| `anthropic` | `POST {base}/messages` with `anthropic-version: 2023-06-01`, `system` + one user turn | `content[0].text` |

Everything except the network call is a pure function — `system_prompt`, `user_prompt`,
`request_body`, `text_of`, `extract_json`, `draft_from_json`, `ground_draft`, `payment_hint`,
`AccountCatalog::resolve`, `synthesize_line` — which is what the unit tests exercise: no key, no
network, no flakiness. `ground_draft` plus the real-vault account list is the regression test for
「星巴克 45 用微信付的」 landing in a made-up `assets:wallet:alipay`. Failures map to stable codes:
`llm.disabled`, `llm.not_configured` (with the reason, e.g. missing key or model),
`llm.request_failed` (transport or HTTP status, carrying the provider's own message),
`llm.bad_response` (unparseable JSON or a draft without an amount) and `llm.join`. A failure never
writes anything; the palette keeps the prose and shows the error.

Creditentials: the key lives in its own file, `<config dir>/llm.json`, created `0600`, and is read
into an `LlmConfig` only when a request is built (`state.llm_key()`). It is deliberately not part of
`Settings`, so it never travels through `settings_set`, the settings document, the IPC error paths
or the write audit.

## Planning and statements

Two read-only engine modules turn the same index into the planning and reporting views. Neither
writes to disk; `goals.ledger` is edited only through `goals_save`, which goes through the normal
snapshot → atomic write → audit path.

`crates/ledger-engine/src/plan.rs` parses `goals.ledger` (directive per line: `assume`, `pool`,
`alloc`, `goal fire`, `goal account`, `debt`) into `PlanSet` and derives `PlanOverviewDto`:

- `fire_projection()` walks months with a real (after-inflation) monthly rate, adding a contribution
  equal to the trailing 6-month average of the pool's net inflow (capped, floored at zero), and finds
  the first month the pool covers `annual_expense / swr` for SWR 3.0 / 3.5 / 4.0 %.
- `reserve()` = cash accounts ÷ trailing 3-month average monthly expense, against `assume reserve:`.
- `debt_rows()` amortises each `debt` line (`rate/12` per month, payment from the directive or the
  trailing average payment on that account), and re-simulates with +500 / +1000 / +2000 to report
  months and interest saved. Interest paid is read from `expenses:` accounts that mention the same
  debt keyword, so it stays derived data.
- `allocation_rows()` groups the pool by the `class:` tag on account declarations
  (`; class:equity`) and compares shares with `alloc`; drift is reported in percentage points.

`crates/ledger-engine/src/statements.rs` produces the three statements by folding `TxnValue`s:
`balance_sheet(as_of)` (assets, liabilities, equity, and the cumulative net that keeps
`assets − liabilities = equity + cumulative_net`), `income_statement(from, to)`, and
`net_worth_bridge(from, to)` where valuation is the residual between the derived closing net worth
and `opening + income − expense`, so multi-currency revaluation can never hide a real discrepancy.

## IPC surface

Commands are declared once with `#[tauri::command]` + `#[specta::specta]`, collected by
`build_specta()` in `src-tauri/src/lib.rs`, and exported to `src/lib/bindings.ts` by
`src-tauri/examples/export_bindings.rs` (`pnpm bindings`). That export is also how the frontend
build works without launching the GUI.

Return convention: `Result<T, CmdError>` becomes
`{status:"ok",data:T} | {status:"error",error:{code,message}}`; `src/lib/ipc.ts` unwraps it into a
throwing `ApiError`. Events: `ledger:changed`, `capture:saved`, `parse:diagnostics`.

The LLM layer adds `llm_status`, `llm_set_key`, `llm_test` and `llm_draft`; `shortcut_status` and
`shortcut_recording` cover the global hotkey. None of them can write to the ledger.

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

The settings recorder listens on `window` in the capture phase rather than on the button: WebKit does
not focus buttons on click, and the chord being recorded may well be the one that is currently
registered. `shortcut_recording(true|false)` therefore unregisters and re-registers the stored chord
around a recording session, and the component restores it on Esc, click-away, window blur and
unmount — a released hotkey must never outlive the recorder.

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

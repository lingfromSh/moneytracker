# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Android**: the app builds and runs on a phone from the same crates — `scripts/android-apk.sh`
  produces a signed release APK. Desktop-only layers (tray icon, menu-bar positioning, global
  hotkey, the always-on-top capture palette) are compiled out on mobile; capture becomes a page in
  the main window, the sidebar becomes a bottom tab bar and the dashboard stacks into one column.
  The first launch creates a vault in the app's private storage, so no folder picker is needed.

- Multi-device sync over any S3-compatible bucket (MinIO, R2, S3, B2). New crate `crates/ledger-sync`:
  hand-rolled SigV4 (verified against an independent HMAC computation), an `ObjectStore` trait with an
  in-memory store for tests, content-addressed blobs, a manifest published under `If-Match`, and a
  three-way diff against `.sync/index.json` so "edited here" and "deleted there" resolve without
  asking. A file edited on two devices **stops the run** and asks which side to keep; deletes go to
  `.sync/trash/` instead of being unlinked. 设置 → 同步 has endpoint / bucket / prefix / region /
  path-style, keys in `sync.json` (`0600`), 测试连接, 预览差异 (writes nothing), 立即同步, a per-file
  conflict picker, and an optional pull at launch (`sync-finished` event). Only `*.ledger` travels.
- Declared schedules (日程): `recurring.ledger` holds hand-editable `~ monthly` templates for money that
  happens every month — 工资 with its 五险二金 split (应发 and the employer side both booked), the
  mortgage payment split into interest and principal, rent income, subscriptions. The 日程 view lists
  the next 6/12/24 months with 记入 / 跳过 per occurrence, the dashboard gains a 固定收支 card, and
  工资模板 generates a complete salary block from one gross figure (个人 8%+2%+0.5%+7%+4%,
  公司 16%+8%+0.5%+0.5%+1%+7%+8%; 公积金按上海口径各付 7%). Occurrences are only written when asked; the 自动化 switch makes the
  monthly maintenance run book what is due. New engine module `crates/ledger-engine/src/schedule.rs` and
  commands `schedule_overview`, `schedule_commit`, `schedule_commit_due`, `schedule_skip`,
  `schedule_save`, `schedule_salary_preview`.

- Personalisation: dashboard card and KPI pickers (看板 → 自定义), density (舒适/紧凑) and week-start
  settings, and named transaction views saved from 流水. New settings `hidden_cards`, `kpi_cards`,
  `density`, `week_start`, `saved_views`.
- Insights panel on the dashboard: rule-based budget alerts, month-over-month category moves, unusual
  expenses, savings rate, net-worth highs and missing FX rates (`crates/ledger-engine/src/insights.rs`,
  `insights` command).
- Read-only questions on 规划 (`llm_ask`): the model returns a JSON query plan, the engine validates it
  against the ledger and computes every number, and the prose is written from those figures only.
- Recurring transactions: patterns (same payee, amount and postings in ≥3 months, 20–45 day spacing)
  are detected and offered as a banner on 流水 with 全部记入 / 本月不算. Written occurrences carry
  `recur:<id>@<YYYY-MM>`; skips are `; skip:<id>@<YYYY-MM>` lines in `recur.ledger`. New commands
  `recur_list`, `recur_commit`, `recur_skip`, engine module `crates/ledger-engine/src/recur.rs`.
- 设置 → 自动化 and `maintenance_run`: optional auto-confirmation of due recurrences, a monthly
  `reports/YYYY-MM.md`, a git snapshot when the vault is a repository, and budget alerts at 90 %.
  New settings `auto_recur` and `git_auto_commit`.
- 导入 (Import) view for 支付宝 / 微信支付 / bank CSV statements: encoding, delimiter and column
  sniffing with presets, `blake3(account|date|amount|commodity|payee)` dedupe against every existing
  transaction, a preview table with per-row selection, and keyword (or `re:`) → category rules. New
  commands `import_probe`, `import_preview`, `import_commit`; imported rows carry `import:<batch>`
  and `ref:<bank-id>`.
- 对账 (Reconciliation) on the accounts view: cleared (`status *`) versus uncleared money per account
  with a cut-off date, the balance assertion comparison, and `reconcile_preview` / `reconcile_clear`.
- 规划 (Planning) view driven by a plain-text `goals.ledger`: FIRE projection at SWR 3.0 / 3.5 / 4.0 %,
  reserve-months gauge, per-account goals with target dates, debt payoff dates with extra-payment
  scenarios, and allocation drift against `class:` tags. The dashboard gains a compact planning strip
  and a FIRE goal line on the net-worth curve.
- 报表 (Reports) view: balance sheet (with the `assets − liabilities = equity + cumulative net`
  identity), income statement per month/quarter/year, and a net-worth bridge that explains the
  closing net worth; `复制 Markdown` copies the current month.
- `crates/ledger-engine/src/plan.rs` and `statements.rs`, plus the `plan_overview`, `goals_get`,
  `goals_save`, `statement_*`, `report_periods`, `debt_progress`, `allocation` and `report_markdown`
  IPC commands.
- `fixtures/vault/full` — a 16-month, 336-entry showcase household ledger (cash, two bank accounts,
  e-wallets, a credit card, mortgage and car loans, a self-owned and a rented flat, fund/stock
  positions, interest, dividends, monthly and yearly envelopes, and big one-off expenses) plus
  `scripts/gen-full-vault.py` and an engine integration test that pins its balance sheet.
- Budget envelopes in `budgets.ledger` now cover **savings goals**, not just spending budgets: the
  positive leg of a `~ monthly` block is the envelope, an `assets:` / `liabilities:` account is
  measured by the money that actually moved into it this month, and the dashboard labels those rows
  储蓄 / 已存. Envelope accounts no longer have to exist in the ledger yet — the kind is resolved from
  the nearest known parent, then from the top-level name map.
- Multi-posting captures: any `@account` may carry its own amount (`火锅 168 @微信 100 @现金 68`) or a
  percentage of the line total (`@餐饮 70%`), so one expense can come from several wallets or be
  split across several categories in a single line. The side of each posting still follows the
  account type, at most one account may stay elided (the engine fills it), and a split that does not
  add up to the total is rejected before anything is written. Transfers take the same amounts
  (`转 500 > @招行 > @现金 200 > @支付宝 300`), so one transfer can reach several accounts.
- Optional natural-language capture (`src-tauri/src/llm.rs`): 设置 → AI 记一笔 configures a provider
  (OpenAI, Anthropic, DeepSeek, Ollama/local, or a custom endpoint), base URL, model, protocol and an
  API key stored separately in `llm.json` (mode `0600`). Pressing <kbd>Tab</kbd> in the capture
  palette switches between Hledger syntax and 自然语言; the model only returns a JSON draft (date,
  payee, amount, category, funding, tags, confidence) which becomes one ordinary capture line, so the
  parser, rules, balance check, snapshot and audit still apply.
  Commands: `llm_status`, `llm_set_key`, `llm_test`, `llm_draft`.
- Regression tests for the LLM plumbing: request/response shaping for both protocols, prompt
  construction, API-key storage, plus `LlmSettings` and natural-language `Capture` component tests.

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

- `recurring.ledger` is now included by `index.ledger`, so `hledger -f index.ledger reg --forecast`
  shows the declared salary, rent and loans next to the transactions that already happened. To keep the
  two readings apart, the budget envelope scan, the cash-flow forecast and the payoff estimate skip the
  file by name (`schedule::is_schedule_file`), and `budgets.ledger` in the showcase vault gained elided
  legs so its templates balance for `--forecast` too.
- Schedules express the billing day through the `from` date (`~ monthly from 2026-11-15`) instead of an
  app-only `day:15`, because hledger rejects that token once the file is part of the ledger. `day:` is
  still parsed for older files, with a `core.schedule.day_token` warning that points at the fix.

- New app icon drawn from `src-tauri/icons/icon.svg` (green squircle, ledger page, committed
  checkmark) instead of the generated green square, and a proper monochrome template glyph for the
  menu bar — the tray used to show the full-colour window icon, which reads as a smudge in dark mode.

- The IPC surface no longer exposes engine-owned transaction tokens: `txn:`, `rule:`, `query:` and
  `recomputed:` are stripped from `TxnDto.tags`/`TxnDto.comment` and re-injected by the write path,
  so editing a transaction can never drop its identity.

### Fixed

- The 快速记账 palette no longer turns into a miniature dashboard. Committing an entry navigated the
  capture window to `#/dashboard`, which left a 640×208 copy of the app floating above the main
  window; the palette now hides itself on the desktop (only the mobile shell, where capture is a page,
  navigates back) and the capture window renders the palette regardless of its URL.

- A dashboard panel added by a newer version no longer stays invisible for anyone who already saved a
  layout: missing panels (e.g. 固定收支) are appended below the existing arrangement instead of being
  dropped, so a saved layout keeps working across upgrades.

- Natural-language capture no longer invents accounts or the wrong wallet: the prompt now lists every
  account in the open ledger (grouped by kind, including accounts only named by `rules.ledger`) with
  worked examples written in those exact names, the payment method in the note overrides the model's
  guess (`用微信付的` → your WeChat account, or the wallet parent), unknown paths collapse onto the
  closest account that exists, and placeholder commodities such as `NONE` are dropped.

- Budgets on a parent account now measure the whole subtree: an `expenses:food` envelope counts
  `expenses:food:groceries` instead of showing 0 spent.
- Amounts rendered from a computed quantity keep the ledger's grouping style, so budget rows read
  `4,500.00 CNY` instead of `4500.00 CNY`.
- Switching the LLM provider in 设置 → AI 记一笔 silently snapped back to the previous provider:
  the select emitted four `update()` calls in one event and each one spread the render-time draft, so
  only the last change survived. `update()` is now a functional state update and the provider switch
  travels as a single `patch()`.
- The AI card now says what it is doing: editing any `llm_*` field shows a **未保存** badge with a
  保存 AI 设置 button, and 测试连接 persists those edits before it tests them, so the readiness line
  and the request always describe the values on screen (this is what made “填写好了，无效” happen —
  the fields were never saved).
- LLM endpoints are normalised: a host-only Base URL gets `/v1` appended and a pasted
  `…/chat/completions` (or `…/messages`) suffix is stripped, so `http://192.168.110.82:1234` works
  the way every OpenAI-compatible local server expects.
- The 设置 page could throw React error #310 (rendering more hooks than the previous render) because
  two `useCallback`s sat after the loading early-return; they are plain functions again.
- `~` / `=` blocks whose postings lost their indentation are accepted with a
  `core.parse.unindented_posting` warning instead of failing the whole block with
  `unknown_directive`, which is what made hand-edited `budgets.ledger` files silently do nothing.
- Every leaf account in **账户** reported twice its balance (and the `assets`/`income` parent rows
  looked half of their children) while the kind totals were correct. `account_tree()` credited a
  leaf's `total` both on the way down and in its own base case; a leaf now adds only its `own`
  postings, so `own == total` for leaves and every parent equals the sum of its children.
- Recording an entry froze the window. `refresh()` kept the vault `RwLock` read guard alive while
  storing the reloaded vault, and because `std::sync::RwLock` is not re-entrant the write lock
  blocked the event-loop thread forever. Reloads now funnel through `AppState::reload_from_disk`
  (clone the `Arc`, drop the guard, load, store) and every vault-touching command runs off the main
  thread, so a slow parse can no longer stall the UI.

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

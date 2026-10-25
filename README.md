# MoneyTracker

**Plain-text double-entry bookkeeping for macOS — your ledger stays a file you can read, grep, diff and commit.**

[![CI](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml/badge.svg)](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust tests](https://img.shields.io/badge/rust_tests-136_passing-brightgreen.svg)](#testing)
[![Front-end tests](https://img.shields.io/badge/vitest-38_passing-brightgreen.svg)](#testing)
[![Rust](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/tauri-2.x-24C8DB.svg)](https://tauri.app)
[![Platform](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](#requirements)

**English** · [简体中文](README.zh-CN.md)

MoneyTracker is a desktop app whose database is a [hledger](https://hledger.org)-compatible
plain-text ledger: a global-hotkey capture box, rules, multi-currency conversion, budgets and a
rich dashboard — with **zero proprietary storage**. Delete the app and you still have your books;
`hledger` can read every byte it writes.

```ledger
2026-10-04 * 星巴克  ; txn:01JA3KQ70SEED0005 rule:r-star query:desc:星巴克
    expenses:food:coffee         31.50 CNY
    expenses:food                13.50 CNY
    assets:wallet:alipay        -45.00 CNY
```

---

## Table of contents

- [Why another bookkeeping app](#why-another-bookkeeping-app)
- [Features](#features)
- [Requirements](#requirements)
- [Install](#install)
- [Usage](#usage)
- [Capture syntax](#capture-syntax)
- [Natural language (optional)](#natural-language-optional)
- [Where your data lives](#where-your-data-lives)
- [Safety guarantees](#safety-guarantees)
- [Development](#development)
- [Testing](#testing)
- [Project layout](#project-layout)
- [Limitations](#limitations)
- [Contributing](#contributing)
- [License](#license)

## Why another bookkeeping app

Most finance apps lock your history in a database only they can open. MoneyTracker makes the file
the source of truth and treats the UI as a fast editor for it:

- **Your ledger is a git repository.** Every transaction is a stable text block with a `txn:<id>`
  tag; edits touch only that block's byte span, so diffs stay small and reviewable.
- **Interop by construction.** `hledger balance`, `hledger register`, `hledger check` all work on
  the same vault — verified by a differential test against the real binary.
- **Fast entry, no ceremony.** One line in a floating window becomes a balanced, tagged,
  rule-expanded transaction.

## Features

| | |
| --- | --- |
| **Capture** | 640×208 always-on-top panel on a global hotkey; parses every keystroke in Rust, `Enter` saves and stays for the next entry |
| **Rules** | `= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝`, expanded at write time with `; rule:` provenance, decimal-exact split with the remainder on the last posting |
| **Multi-currency** | rates come only from `P` directives; missing rates are flagged, greyed out and excluded from totals — never assumed 1:1 |
| **Dashboard** | KPI + month-over-month, income/expense bars, net-worth curve, category donut, stacked area, calendar heatmap, budget vs actual, forecast, Sankey — draggable/resizable layout persisted per user |
| **Transactions** | windowed list, inline editor, bulk tag/account rename, saved queries (`acct:`, `desc:`, `date:`, `tag:`, `amt:` with `and`/`or`/`not`) |
| **Accounts & categories** | account tree with own/total balances and type inference; category view by account or by tag |
| **Bilingual UI strings** | interface is zh-CN; ledger content and code comments are language-neutral |
| **Safety** | pre-write snapshots, atomic replace, external-change detection, JSONL write audit |

## Requirements

- macOS 13+ (Apple Silicon or Intel). The capture window uses `macos-private-api` for transparency.
- Rust 1.91+ (stable) and Node.js 22+ with pnpm 10+ — only when building from source.

## Install

Prebuilt `.app` bundles are attached to [releases](https://github.com/lingfromSh/moneytracker/releases)
when published. To build it yourself:

```bash
git clone https://github.com/lingfromSh/moneytracker.git
cd moneytracker
pnpm install
pnpm tauri build                 # → src-tauri/target/release/bundle/macos/MoneyTracker.app
```

For development:

```bash
pnpm tauri dev                   # Vite on :1420 + the Tauri shell with hot reload
```

## Usage

First launch shows **Welcome**: create a vault, open an existing folder, or create the sample vault
(identical to `fixtures/vault/sample`).

| Shortcut | Action |
| --- | --- |
| `⌘⇧Space` | global capture window (configurable in Settings) |
| `⌘K` | command palette |
| `⌘N` | capture window |
| `⌘F` | transactions / search |
| `⌘1` … `⌘6` | dashboard, transactions, accounts, categories, rules, settings |
| `Enter` / `⌘Enter` / `Esc` | capture: save and stay / save and hide / hide |

The tray icon opens the capture panel next to the menu bar; `⌘,`-style settings cover theme,
shortcut, base commodity, default funding account, unclassified account, backups and FX sync.

## Capture syntax

```
[#date] [*|!] description amount [@account]... [+tag]... [; comment]
```

| Input | Result |
| --- | --- |
| `咖啡 4.5 @微信 +餐饮` | `expenses:餐饮 4.50 CNY` / `assets:微信 -4.50 CNY`, tag `:餐饮:` |
| `工资 +12000 @招行 +收入` | `assets:招行 12000.00 CNY` / `income:工资 -12000.00 CNY` |
| `转 500 > @招行 > @现金` | `assets:招行 -500` / `assets:现金 500` |
| `酒店 100usd@@720cny @旅行` | `expenses:旅行 100 USD @@ 720 CNY` / `assets:<default> -720 CNY` |
| `火锅 168 @微信 100 @现金 68` | one expense funded from two accounts |
| `超市 200 @餐饮 120 @日用 80 @现金` | one expense split across two categories, funding filled in |
| `聚餐 300 @餐饮 70% @娱乐 30% @招行` | percentages of the line total (remainder to the last) |
| `转 500 > @招行 > @现金 200 > @支付宝 300` | one transfer reaching two accounts |
| `"iPhone 15" 8999 @数码` | quoted description, `8999` is the amount |
| `#昨天 咖啡 4.5` | dated yesterday (`#前天`, `#+3d`, `#2026-10-01` also work) |
| `= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝` | create or update a rule |
| `/acct:餐饮` | open the transaction list with that query |
| `:settings` | command palette |

Sign convention: no sign or `-` is an **expense** (money leaves a funding account), `+` before the
amount is **income**. One `@account` leaves the other side to the account type (`A`/`L` → funding,
`R`/`X` → category) or to a matching rule; two `@` accounts read as `category funding`. An entry no
rule and no category covers lands in `expenses:unclassified` with an `:inbox:` tag.

**Splits.** Any `@account` may be followed by an amount, in which case the posting gets exactly that
amount instead of the whole line total: write `@微信 100 @现金 68` to fund one expense from two
wallets, or `@餐饮 120 @日用 80` to split it across two categories. The side of each posting still
comes from the account type, so both sides can be split at once. Exactly one account may be left
without an amount — the engine fills it so the transaction balances — and a bare percent
(`@餐饮 70%`) is read against the line total, with any rounding remainder going to the last percent.
Amounts that do not add up to the total are reported before anything is written.

## Natural language (optional)

The palette has two modes: **Hledger syntax** (default) and **自然语言**. Press <kbd>Tab</kbd> to
switch; the mode is remembered between sessions.

```
昨天和朋友吃火锅花了 168，微信付的
        ↓ Tab
将写入   火锅 168 @微信 +餐饮
```

**The model never writes ledger text.** It receives the sentence plus a compact account list and
returns a small JSON draft (date, payee, amount, commodity, category, funding, tags, confidence).
MoneyTracker turns that draft into one ordinary capture line, so the parser, rule expansion,
double-entry check, snapshot and audit all still apply — an LLM entry is exactly as safe as a typed
one, and you always see the line before it is written.

Configure it in **设置 → AI 记一笔**:

| Setting | Notes |
| --- | --- |
| Provider | OpenAI, Anthropic Claude, DeepSeek, Ollama / local models, or a custom endpoint |
| Base URL | Leave blank for the preset; point it at a gateway or proxy when you need one |
| Model | Free text — any model the endpoint accepts |
| Protocol | `OpenAI chat/completions` or `Anthropic messages`; only asked for custom endpoints |
| API key | Written to `llm.json` (mode `0600`) in the app config dir, never to the ledger or `settings.json` |

Off by default. With no key and no model configured the mode stays disabled and the palette says
what is missing. "测试连接" performs one tiny request so you can verify credentials before relying
on it. What leaves your machine is the sentence you typed plus your account names — nothing else,
and only when you press <kbd>Tab</kbd> into natural-language mode.

## Where your data lives

```
<vault>/
  index.ledger                 # the only entry point: options, declarations, includes
  accounts.ledger              # account declarations with `; type:A|L|E|R|X`
  prices.ledger                # `P` directives — the only source of exchange rates
  budgets.ledger               # `~ monthly` budget envelopes
  rules.ledger                 # MoneyTracker rules — intentionally NOT included by index.ledger
  2026/
    2026.ledger                # includes the month indexes
    2026-10.ledger             # includes the day files
    2026-10-04.ledger          # one day of transactions
  .bak/                        # pre-write snapshots (git-ignored)
```

Everything is plain hledger syntax: `include` / `!include` (globs and cycle detection), `account`
with `type:` tags, `commodity`, `D`, `Y`, `P`, `~` periodic blocks, `=` auto-postings,
`comment`/`end comment`, `;` `#` `%` `|` comments, virtual postings `(a)` / `[a]`, `@` / `@@`
prices, balance assertions and tags (`:a:b:` and `key:value`).

Anything outside that subset — amount expressions, lot prices `{}`, `C` / `N` conversions,
timeclock, `csv:` rules — fails loudly with `file:line:col` and a stable diagnostic code. The app
never silently drops a line it does not understand. Details: [`docs/vault-format.md`](docs/vault-format.md).

**Why `rules.ledger` is not included.** Rules are expanded when a transaction is written and the
result is stored as ordinary postings, with `; rule:<id> query:<original input>` recording the
provenance. Including the rule file would make `hledger --auto` apply them a second time and
produce different numbers.

**Budgets and savings goals.** `budgets.ledger` holds `~ monthly` periodic blocks. The positive leg
is the envelope: an `expenses:` (or any `X`) account is a spending budget, an `assets:` /
`liabilities:` account is a savings goal measured by how much money actually moved in this month.
The negative leg only exists to keep the block balanced.

```ledger
; 500 a month of eating out
~ monthly from 2026-01-01
    expenses:food        500.00 CNY
    assets:cash

; put 500 aside every month for retirement
~ monthly from 2026-01-01
    assets:invest:retirement   500.00 CNY
    assets:cash
```

MoneyTracker reads these files leniently: postings under `~` (and `=`) blocks that lost their
leading indentation are still accepted, with a `core.parse.unindented_posting` warning — hledger
itself requires the indentation, so the warning tells you the file is not portable yet.

## Safety guarantees

Every mutation goes through `crates/ledger-engine/src/writer.rs`:

1. the target file must have parsed without errors,
2. a snapshot is copied to `<vault>/.bak/<relative path>.<timestamp>` (200 kept by default),
3. new bytes go to a same-directory temp file → `fsync` → `rename` → directory `fsync`,
4. the `(mtime, size, blake3)` recorded at parse time must still match, otherwise the write is
   refused with *“file was modified outside MoneyTracker — refresh and try again”*,
5. the operation is appended to `<appSupport>/dev.moneytracker.app/write-audit.jsonl`.

Editing rewrites **only that transaction's byte span** — byte-for-byte asserted by the engine
tests. A `notify` watcher with a 300 ms debounce reloads external changes, so a `git pull` or a
`vim` edit appears live.

## Development

```bash
pnpm install
pnpm tauri dev                   # app with hot reload
pnpm bindings                    # regenerate src/lib/bindings.ts from the Rust commands
./scripts/smoke.sh               # the full verification pass (see below)
```

Prerequisites: Rust stable 1.91+, Node 22+, pnpm 10+, and optionally `hledger` for the differential
check (`brew install hledger`).

## Testing

```bash
cargo test --workspace                        # 110 Rust tests, 12 suites
cargo clippy --workspace --all-targets -D warnings
cargo fmt --all -- --check
pnpm test                                     # 15 Vitest tests
pnpm build                                    # tsc --noEmit && vite build
```

| Layer | What is covered |
| --- | --- |
| `crates/ledger-core/tests/corpus.rs` | `fixtures/grammar/*.ledger` parse clean, print→parse→print is byte-identical, and `fixtures/errors/*.ledger` produce the documented codes with the documented lines |
| `crates/ledger-engine` | include graph, globs, cycles, account kind inference, FX, rules, capture expansion, reports, writer safety, plus the checked-in sample vault end to end |
| `crates/ledger-engine/tests/hledger_diff.rs` | `Vault::balances()` equals `hledger balance --flat --no-total -O csv`; `hledger check` accepts a vault the engine just wrote (skipped when `hledger` is missing) |
| `src-tauri/src/editing.rs` | capture → commit → reload, span-level update, delete, bulk edit, and refusal when the file changed outside the app |
| `src/**/*.test.tsx` | capture palette, dashboard and transaction list against a mocked IPC layer |

`scripts/smoke.sh` runs all of the above plus the `hledger` cross-check and fails if the committed
TypeScript bindings are stale.

Performance, measured on an M-series laptop with a generated 100 000-transaction vault spread over
5 183 files (`cargo run -q --release -p ledger-engine --example bench_load -- 100000`):

| Phase | Time |
| --- | --- |
| cold `Vault::load` (read + parse + index) | ~760 ms |
| `snapshot()` (account tree, balances) | ~70 ms |
| `dashboard(12 months)` | ~100 ms |

## Project layout

```
crates/ledger-core     AST, byte-span parser, printer, diagnostics
crates/ledger-query    hledger query subset: acct/desc/date/tag/amt, and/or/not
crates/ledger-engine   include graph, index, balances, capture, rules, FX, reports, safe writes
src-tauri              windows, tray, global shortcut, watcher, IPC
src                    React 19 + Vite + Tailwind v4 + Recharts + d3-sankey
fixtures/              grammar and error corpora, sample vault
docs/                  architecture, vault format
```

Types are generated, never hand-written: `src-tauri/examples/export_bindings.rs` exports every
command and event through `tauri-specta` into `src/lib/bindings.ts`. See
[`docs/architecture.md`](docs/architecture.md) for the module map, the command surface and the
capture state machine.

## Limitations

- Deliberately out of scope: bank import/reconciliation, attachments, multi-user sync, automatic
  budget rollover, cloud storage (put the vault in iCloud or git yourself).
- The supported dialect is the subset above; everything else fails loudly instead of being
  approximated.
- `hledger` is not bundled — cross-checks run only when it is on `PATH`.
- macOS only for now; the Rust core is portable, the window/tray layer is not.
- `specta` 2.0.0-rc.26 is vendored in `vendor/specta` with a two-line patch, because stable Rust
  lacks the `fmt::from_fn` helper it uses and `tauri-specta` pins that exact version.

## Contributing

Bug reports, fixture files and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).
Please run `./scripts/smoke.sh` before opening a PR; the release notes live in
[CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE) © 2026 lingfromSh

---

**[简体中文文档 →](README.zh-CN.md)**

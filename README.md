# MoneyTracker

**Plain-text double-entry bookkeeping for macOS — your ledger stays a file you can read, grep, diff and commit.**

[![CI](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml/badge.svg)](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust tests](https://img.shields.io/badge/rust_tests-220_passing-brightgreen.svg)](#testing)
[![Front-end tests](https://img.shields.io/badge/vitest-77_passing-brightgreen.svg)](#testing)
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
- [Importing statements](#importing-statements)
- [Recurring entries & automation](#recurring-entries--automation)
- [Declared schedules](#declared-schedules)
- [Multi-device sync](#multi-device-sync)
- [Android](#android)
- [Insights & questions](#insights--questions)
- [Personalising](#personalising)
- [Planning & reports](#planning--reports)
- [Where your data lives](#where-your-data-lives)
- [Safety guarantees](#safety-guarantees)
- [Development](#development)
- [Testing](#testing)
  - [Sample data](#sample-data)
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
| **Import** | Alipay / WeChat Pay / bank CSV statements: auto-detected columns, dedupe by fingerprint, preview before writing, keyword→category rules, then reconcile the account against what the bank says |
| **Schedule** | `recurring.ledger` holds hand-written `~ monthly` templates — salary with the employer side and the 五险二金 split, mortgage interest + principal, rent, subscriptions; the 日程 page lists the next 6/12/24 months with 记入 / 跳过, and the dashboard gets a 固定收支 card |
| **Automation** | detects monthly patterns (payee + amount + postings, ≥3 months), offers a one-click "record next month", tags what it writes with `recur:<id>@<month>`, keeps a monthly report in `reports/`, can snapshot the vault into git, and warns when a budget passes 90 % |
| **Sync** | mirrors the vault into an S3-compatible bucket (MinIO, R2, S3) with a 3-way merge against a manifest; a file edited on two devices is **stopped on**, never silently overwritten, and the settings page asks which side to keep |
| **Planning** | `goals.ledger` drives a FIRE projection (SWR 3.0/3.5/4.0 %), a reserve-months gauge, per-account goals with target dates, debt payoff dates (plus what an extra payment saves) and allocation drift against `class:` tags |
| **Reports** | balance sheet, income statement for any month/quarter/year, and a net-worth bridge that reconciles closing net worth to opening plus income minus expense |
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
| `⌘1` … `⌘0` | dashboard, transactions, import, accounts, categories, rules, plan, schedule, reports, settings |
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

**The model never writes ledger text.** It receives the sentence plus your ledger's own account
catalogue — assets, liabilities, income, expenses and equity, including the accounts your
`rules.ledger` intends to use — with worked examples built from those very names, and returns a small
JSON draft (date, payee, amount, commodity, category, funding, tags, confidence). MoneyTracker then
turns that draft into one ordinary capture line, so the parser, rule expansion, double-entry check,
snapshot and audit all still apply — an LLM entry is exactly as safe as a typed one, and you always
see the line before it is written.

The draft is grounded before it becomes a line, because models like to invent accounts:

- a name that does not exist collapses onto the closest account you do have — `assets:wallet:alipay`
  becomes `assets:wallet` when that is all the ledger knows;
- the payment method in your sentence wins over the model's guess — 「用微信付的」 picks your WeChat
  wallet account (or the wallet parent when you keep no sub-accounts);
- placeholder commodities such as `NONE` are dropped, and `45.0` is written as `45`.

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

## Importing statements

支付宝 / 微信支付 / bank exports are messy by design; **Import** (`⌘3`) turns them into ledger
entries without you hand-typing anything, and never writes the same row twice.

1. Pick a CSV. MoneyTracker sniffs the encoding (UTF-8/GBK), the delimiter, the header row and the
   date/payee/amount columns, and recognises the common Alipay, WeChat Pay, CMB and ICBC layouts.
2. Check the guess — the account (`assets:bank:cmb` …), the columns, and an optional date format
   such as `%Y/%m/%d` when the file uses something unusual.
3. Add category rules, one per line: `星巴克 => expenses:food:coffee :coffee`. The first matching
   keyword (or `re:正则`) wins; unmatched rows land in `expenses:unclassified` with `:inbox:`.
4. Preview: duplicates are detected by `blake3(account|date|amount|commodity|payee)` against every
   existing transaction, so re-importing an overlapping statement is safe. Untick anything you do
   not want.
5. Import. Written rows carry `import:<batch>` plus `ref:<bank-id>`, so the batch stays auditable
   and a later re-import of the same file finds nothing new.

Reconciliation (`账户` → **对账**) is the other half: pick an account and a cut-off date, and the
ledger shows what is cleared (`status *`) versus uncleared. Clear the transactions that appear on
the statement; when the account carries a `= balance` assertion, the same panel tells you whether
the ledger balance matches the bank.

## Recurring entries & automation

The ledger already knows what repeats: a pattern needs the same payee, the same amount and the same
set of postings in at least three different months with a 20–45 day spacing. Those patterns show up as
a strip at the top of 流水 with **全部记入** (write next month's occurrence) and **本月不算** (skip this
cycle). Nothing is written unless you confirm it — or unless 设置 → 自动化 turns on 自动记入定期交易与固定收支.

Each written occurrence is an ordinary transaction tagged `recur:<id>@<YYYY-MM>`, so the batch is
auditable and re-running the detection never double-writes a month.

设置 → 自动化 also drives the maintenance pass (**立即维护**, and once at launch when enabled):

| Step | Effect |
| --- | --- |
| recurring | confirms every occurrence that is due |
| monthly report | writes `reports/YYYY-MM.md` once per month (Markdown, same numbers as 报表) |
| git snapshot | when the vault folder contains `.git`, stages and commits it |
| budget alerts | lists envelopes above 90 % (or over budget) in the settings card |

Skipped cycles are stored as `; skip:<id>@<YYYY-MM>` lines in `recur.ledger`, next to `rules.ledger`
and, like it, never included by `index.ledger`.

## Declared schedules

Detection (previous section) only sees what has already happened. The other half is "I know this
happens every month": salary, mortgage, rent, subscriptions. Write those into `recurring.ledger`, using
plain hledger periodic syntax:

```ledger
~ monthly from 2025-08-05  ; id:salary-main label:工资 kind:salary
    income:salary                        -30,000.00 CNY
    income:benefit                       -12,300.00 CNY  ; employer contributions
    assets:cn:fund                         4,200.00 CNY  ; housing fund: 2,100 you + 2,100 employer
    assets:cn:pension                      7,200.00 CNY  ; pension: 2,400 you + 4,800 employer
    assets:cn:annuity                      3,600.00 CNY  ; annuity: 1,200 you + 2,400 employer
    expenses:insurance:medical             3,000.00 CNY  ; 714 you + 2,400 employer
    expenses:insurance:unemployment          300.00 CNY
    expenses:insurance:injury                150.00 CNY
    expenses:insurance:maternity             300.00 CNY
    assets:bank:cmb                       23,550.00 CNY  ; take-home (the cash leg)
```

- The header comment carries the metadata: `id:`, `label:`, `kind:` (salary / loan / income / expense),
  `enabled:`. The day of the month comes from the `from` date (`from 2026-01-31` clamps to the end of
  short months), which is exactly how hledger reads it.
- **The last posting is the cash leg**: it is what the card shows as 现金流, and its amount may be
  left out for the engine to balance.
- 五险二金 is split by *withdrawability*: the housing fund, the personal pension account and the
  annuity stay `assets:cn:*` (still your money), while medical, unemployment, injury and maternity are
  `expenses:insurance:*` (pooled away). The employer side is booked too, so income is 42,300 — not
  30,000. The housing fund is 7% + 7% (Shanghai); change the two `bps` values in
  `crates/ledger-engine/src/schedule.rs` for another city.
- Declaring a template books nothing. 记入 writes one ordinary transaction into the day file tagged
  `sched:<id>`; 设置 → 自动化 → 自动记入固定收支 makes the monthly maintenance pass fill in whatever is
  due that month.
- 日程 → 工资模板 does the arithmetic for you: give it the gross figure and the account that receives
  the net pay, and it generates the whole block (个人 8%/2%/0.5%/7%/4%, 公司 16%/8%/0.5%/0.5%/1%/7%/8%),
  previewed before it is written.

`recurring.ledger` **is included by `index.ledger`**: hledger ignores `~` blocks unless you ask for
`--forecast`, so `hledger -f index.ledger reg --forecast` shows the salary that is still to come.
MoneyTracker's own budget envelope scan, cash-flow forecast and payoff estimate skip the file by name,
so a declared schedule is never counted as a month that already happened.

## Multi-device sync

The vault is a folder of text files, so syncing it is syncing files. MoneyTracker can mirror it to
any S3-compatible bucket (MinIO, Cloudflare R2, AWS S3, Backblaze B2 …) and keeps a manifest of the
hash every device agreed on last time, which turns "who changed what" into a three-way question:

| local | remote | base | result |
| --- | --- | --- | --- |
| changed | unchanged | — | upload |
| unchanged | changed | — | download |
| changed | changed | same hash | both sides agree: nothing to do |
| changed | changed | different | **conflict — the run stops and asks** |
| changed | deleted | — | delete on the other side |
| changed | changed | no base | **conflict** (a file both devices created) |

Setup lives in 设置 → 同步: endpoint, bucket, prefix (one bucket can hold several vaults), region,
path-style addressing for MinIO, and the two keys — those go into `sync.json` next to
`settings.json` with `0600`, never into the ledger. **测试连接** reads the bucket, **预览差异** shows
what a run would do without writing anything, **立即同步** does it.

- Only `*.ledger` files travel. `.bak/`, `.git/`, `.sync/` and editor temp files stay local.
- A file is written only after its blobs arrive and their blake3 hash matches, and always through
  the same atomic writer the UI uses, so a half-synced file can never appear.
- A deleted file is moved to `.sync/trash/<timestamp>/` rather than unlinked.
- The manifest is replaced under `If-Match`, so two devices pushing at the same moment get a
  retry instead of a lost update. Nothing resolves a conflict on your behalf: pick 保留本机 or 用云端
  per file, or open the file yourself and merge it.
- Plaintext in the bucket. Use a private bucket, HTTPS, and keys that can only touch that prefix.

The `auto` switch pulls once at launch (and reports through `sync-finished`) so a phone edit is
there before you start typing.

## Android

The phone build is not a port: it is the same `ledger-core` / `ledger-engine` / `ledger-sync` crates
with the desktop-only layers compiled out (tray icon, menu-bar positioning, global hotkey, the
always-on-top capture palette).

| on the phone | behaviour |
| --- | --- |
| first launch | creates a vault in the app's private storage (`/data/data/dev.moneytracker.app/files/ledger`) and opens it — no folder picker |
| capture | a full-page route inside the main window; the sidebar becomes a bottom tab bar |
| dashboard | cards stack in one column in the order you arranged them on the desktop; dragging is off so scrolling wins |
| sync | identical: fill in endpoint / bucket / keys under 设置 → 同步 |
| tray, global hotkey | not available — those concepts do not exist on a phone |

Build an installable APK:

```bash
scripts/android-apk.sh          # release, arm64, signed
adb install -r build/android/moneytracker-*.apk
```

It needs the Android SDK (`platform-tools`, `platforms;android-35`, `build-tools;35.0.0`,
`ndk;27.2.12479018`), a JDK 17+ and `rustup target add aarch64-linux-android`. The signing key lives
at `~/.moneytracker-android/moneytracker.jks`  — **keep it**: a new version
only upgrades an installed app when both are signed with the same key.

## Insights & questions

The dashboard's 洞察 panel is computed by rules, not by a model: budget envelopes above 90 %, the two
biggest category moves against last month, an unusually large expense (3× the six-month average), the
savings rate, a net-worth high, and missing FX rates. They are cheap, deterministic, and offline.

规划 also has a read-only 问一句 card. When an LLM is configured, the model is asked for a **JSON query
plan** — metric, accounts, tags, date range, grouping, limit — and nothing else. Every field is then
validated against the open ledger (unknown accounts are dropped, bad dates ignored, `limit` clamped),
the numbers are computed by the engine, and only then may the model phrase one paragraph from those
computed figures. Turn the switch off and the same question still gets its fact table; the prose is
the only thing that disappears.

## Personalising

Settings, not code. 看板 → 自定义 picks which cards are visible and which KPI chips appear on top;
设置 → 外观 switches the dashboard between 舒适 and 紧凑 densities and chooses the week start. Views you
build on 流水 (a query plus its sort order) can be saved as named chips and removed again with a ×;
they live in `settings.json` alongside the layout, so they follow the machine, not the vault.

## Planning & reports

`goals.ledger` is a small, plain-text plan file (like `rules.ledger`, it is **not** included by
`index.ledger` — hledger must never see it). One directive per line:

```ledger
assume  ; return:6% inflation:2.5% reserve:6          ; nominal return, inflation, emergency months
pool    ; accounts:assets:invest,assets:cash           ; what counts as "investable"
alloc   ; equity:60% bond:20% cash:20%                 ; target allocation by `class:` tag
goal fire 财富自由 ; swr:4%                            ; withdrawal rate used for the headline number
goal account 退休金 ; account:assets:invest:retirement target:1000000.00 CNY by:2040-01-01
debt 房贷 ; account:liabilities:mortgage rate:3.15% payment:8200.00 CNY
```

- **FIRE** = annual expenses ÷ withdrawal rate. Expenses come from the trailing 12 months (or
  `expenses:annual`), the pool is the current balance of the `pool` accounts, and the ETA uses the
  real (after-inflation) return with a 6-month average contribution.
- **Reserve months** = cash ÷ average monthly expenses; the target is `reserve:` in `assume`.
- **Debt progress** amortises each `debt` line monthly: payoff date, interest paid since the ledger
  began, and *what an extra 500 / 1000 / 2000 per month would save* — read-only, nothing is written.
- **Allocation drift** compares the `class:` tags in `accounts.ledger` (`; class:equity`) against
  `alloc`, flagging anything more than 5 percentage points off.

The **报表 (Reports)** view builds three statements from the same ledger: a balance sheet that
keeps the double-entry identity `assets − liabilities = equity + cumulative net` for every period, an
income statement for a month / quarter / year, and a net-worth bridge
(`opening + income − expense ± valuation = closing`). `复制 Markdown` puts the current month's
statement on the clipboard for your journal.

## Where your data lives

```
<vault>/
  index.ledger                 # the only entry point: options, declarations, includes
  accounts.ledger              # account declarations with `; type:A|L|E|R|X`
  prices.ledger                # `P` directives — the only source of exchange rates
  budgets.ledger               # `~ monthly` budget envelopes
  goals.ledger                 # plan: assumptions, FIRE goal, pool, allocation, debts (not included)
  rules.ledger                 # MoneyTracker rules — intentionally NOT included by index.ledger
  recurring.ledger             # declared schedules: salary, mortgage, rent
  recur.ledger                 # `; skip:<id>@<month>` decisions (not included)
  2026/
    2026.ledger                # includes the month indexes
    2026-10.ledger             # includes the day files
    2026-10-04.ledger          # one day of transactions
  .bak/                        # pre-write snapshots (git-ignored)
  .sync/                       # sync state: last agreed hashes, conflict trash (git-ignored)
```

Everything is plain hledger syntax: `include` / `!include` (globs and cycle detection), `account`
with `type:` tags, `commodity`, `D`, `Y`, `P`, `~` periodic blocks, `=` auto-postings,
`comment`/`end comment`, `;` `#` `%` `|` comments, virtual postings `(a)` / `[a]`, `@` / `@@`
prices, balance assertions and tags (`:a:b:` and `key:value`).

Anything outside that subset — amount expressions, lot prices `{}`, `C` / `N` conversions,
timeclock, `csv:` rules — fails loudly with `file:line:col` and a stable diagnostic code. The app
never silently drops a line it does not understand. Details: [`docs/vault-format.md`](docs/vault-format.md).

**Why `rules.ledger` and `goals.ledger` are not included.** Rules are expanded when a transaction is written and the
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
cargo test --workspace                        # 220 Rust tests, 15 suites
cargo clippy --workspace --all-targets -D warnings
cargo fmt --all -- --check
pnpm test                                     # 77 Vitest tests
pnpm build                                    # tsc --noEmit && vite build
```

| Layer | What is covered |
| --- | --- |
| `crates/ledger-core/tests/corpus.rs` | `fixtures/grammar/*.ledger` parse clean, print→parse→print is byte-identical, and `fixtures/errors/*.ledger` produce the documented codes with the documented lines |
| `crates/ledger-engine` | include graph, globs, cycles, account kind inference, FX, rules, capture expansion, reports, writer safety, plus the checked-in sample and showcase vaults end to end |
| `crates/ledger-engine/tests/hledger_diff.rs` | `Vault::balances()` equals `hledger balance --flat --no-total -O csv`; `hledger check` accepts a vault the engine just wrote (skipped when `hledger` is missing) |
| `src-tauri/src/editing.rs` | capture → commit → reload, span-level update, delete, bulk edit, and refusal when the file changed outside the app |
| `src/**/*.test.tsx` | capture palette, dashboard and transaction list against a mocked IPC layer |

`scripts/smoke.sh` runs all of the above plus the `hledger` cross-check and fails if the committed
TypeScript bindings are stale.

### Sample data

Two vaults are checked in, both plain hledger files you can open with `hledger` or point the app at
(**设置 → 账本 → 打开已有账本**):

| Vault | Contents |
| --- | --- |
| `fixtures/vault/sample` | 7 entries: salary, rent, a rule-split coffee, a USD trip — small enough to reason about |
| `fixtures/vault/full` | 16 months, 336 entries: cash, two bank accounts, e-wallets, a credit card, mortgage and car loans, a self-owned and a rented flat, fund/stock positions, interest and dividends, monthly + yearly envelopes and a handful of big one-offs (renovation 104,300, appliances 58,200, a car). Net worth 2,384,394 CNY |

`fixtures/vault/full` is generated by `scripts/gen-full-vault.py`; see
[`fixtures/vault/full/README.md`](fixtures/vault/full/README.md) for a map of which account or feature
demonstrates what.

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
crates/ledger-sync     S3/MinIO mirror: SigV4, 3-way diff, conflict detection, trash
src-tauri              windows, tray, global shortcut, watcher, IPC
src                    React 19 + Vite + Tailwind v4 + Recharts + d3-sankey
fixtures/              grammar and error corpora, sample + showcase vaults
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
- macOS and Android are the tested targets; Windows/Linux build from the same code but the
tray/menu-bar behaviour is only exercised on macOS.
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

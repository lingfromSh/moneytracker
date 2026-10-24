# MoneyTracker

A macOS-first desktop bookkeeping app whose database is **plain text you can read, grep, diff and
commit**: a [hledger](https://hledger.org)-compatible ledger on disk, with a fast capture box,
rules, multi-currency conversion and a rich dashboard on top.

- **On disk:** `index.ledger` → year index → month index → day file, all wired with `include`.
- **In the app:** a global-hotkey capture window, span-level editing, rule table, FX, budgets,
  forecast and a draggable dashboard.
- **Interop:** everything the app writes is readable by `hledger -f index.ledger …`. Nothing is
  stored in a proprietary format; delete the app and you still have your books.

```
2026-10-04 * 星巴克  ; txn:01JA3KQ70SEED0005 rule:r-star query:desc:星巴克
    expenses:food:coffee         31.50 CNY
    expenses:food                13.50 CNY
    assets:wallet:alipay        -45.00 CNY
```

---

## Quick start

```bash
pnpm install                 # front-end deps
pnpm tauri dev              # runs Vite on :1420 and the Tauri shell
pnpm tauri build            # bundled .app / .dmg in src-tauri/target/release/bundle
```

First launch shows **Welcome**: create a vault, open an existing one, or create the sample vault
(the same content as `fixtures/vault/sample`).

| Shortcut | Action |
| --- | --- |
| `⌘⇧Space` | global capture window (configurable in Settings) |
| `⌘K` | command palette |
| `⌘N` | capture window |
| `⌘F` | transactions / search |
| `⌘1`…`⌘6` | dashboard, transactions, accounts, categories, rules, settings |
| `Enter` / `⌘Enter` / `Esc` | capture: save and stay / save and hide / hide |

## Verification

```bash
cargo test --workspace         # 103 Rust tests (parser, engine, reports, write path, sample vault)
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
pnpm test                      # 15 Vitest tests (capture, dashboard, transactions)
pnpm build                     # tsc --noEmit && vite build
pnpm bindings                  # regenerate src/lib/bindings.ts from the Rust commands
./scripts/smoke.sh             # all of the above + optional hledger cross-check
```

`scripts/smoke.sh` compares `hledger -f fixtures/vault/sample/index.ledger balance --flat --no-total`
against the engine when `hledger` is installed, and skips that step otherwise.

Performance is checked with a generated vault (100 000 transactions over 5 183 files):

```bash
cargo run -q --release -p ledger-engine --example bench_load -- 100000
# loaded 100000 txns from 5183 files in ~760 ms · snapshot ~70 ms · dashboard ~100 ms
```

See `docs/architecture.md` for the measurement table and why the indexing pass is parallel.

---

## On-disk format (the contract)

```
<vault>/
  index.ledger                 # the only entry point: options + declarations + includes
  accounts.ledger              # account declarations with `; type:A|L|E|R|X`
  prices.ledger                # `P` directives — the only source of exchange rates
  budgets.ledger               # `~ monthly` recurring/budget transactions
  rules.ledger                 # MoneyTracker rules — NEVER included by index.ledger
  2026/
    2026.ledger                # includes the month indexes
    2026-10.ledger             # includes the day files
    2026-10-04.ledger          # the transactions of one day
  .bak/                        # pre-write snapshots (git-ignored)
```

`index.ledger`:

```ledger
commodity 1,000.00 CNY

include accounts.ledger
include prices.ledger
include budgets.ledger
include 2026/2026.ledger
```

**`rules.ledger` is intentionally not included.** Rules are expanded *when a transaction is
written* and the result is stored as ordinary postings; the `; rule:<id> query:<原始输入>`
tag records where it came from. Including the rule file would make `hledger --auto` apply them a
second time and produce different numbers. See `docs/vault-format.md` for the full grammar subset,
the transaction tag conventions (`txn:`, `rule:`, `recomputed:`) and the exact rejection rules.

### Supported hledger subset

Supported: `include` / `!include` (with globs and cycle detection), `account` (+ `type:` tags),
`commodity`, `D`, `Y`, `P`, `~` periodic blocks in `budgets.ledger`, `=` auto-postings rules,
`comment`/`end comment`, `;` `#` `%` `|` `*` comments, transaction headers
`DATE[=DATE2] [*|!] [(CODE)] DESC [; COMMENT]`, postings with elided amounts, virtual `(a)` /
`[a]`, `@` / `@@` prices, balance assertions, and tags (`:a:b:` and `key:value`).

Rejected with a diagnostic that carries file, line, column and a stable code: amount expressions,
lot/cost prices `{}`, `C` / `N` conversion directives, timeclock, `csv:` rules, `--auto` /
`--forecast` CLI semantics. The app never silently drops a line it does not understand.

---

## Capture syntax (one line → a canonical block)

Parsing happens in Rust (`capture_parse`); the UI only renders the preview.

```
[#date] [*|!] description amount [@account]... [+tag]... [; comment]
```

| Input | Result |
| --- | --- |
| `咖啡 4.5 @微信 +餐饮` | `expenses:餐饮 4.50 CNY` / `assets:微信 -4.50 CNY`, tag `:餐饮:` |
| `工资 +12000 @招行 +收入` | `assets:招行 12000.00 CNY` / `income:工资 -12000.00 CNY` |
| `转 500 > @招行 > @现金` | `assets:招行 -500` / `assets:现金 500` |
| `酒店 100usd@@720cny @旅行` | `expenses:旅行 100 USD @@ 720 CNY` / `assets:<default> -720 CNY` |
| `"iPhone 15" 8999 @数码` | description keeps the quotes, `8999` is the amount |
| `#昨天 咖啡 4.5` | dated yesterday |
| `= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝` | creates or updates a rule |
| `/acct:餐饮` | switches to the transaction list with that query |
| `:settings` | command palette |

Sign conventions: no sign or `-` is an **expense** (money leaves a funding account), `+` before a
number is **income**. `@account` once = one side, chosen by account type (`A`/`L` → funding,
`R`/`X` → category); `@` twice = `category funding`; no `@` = rules decide. When nothing matches
the entry lands in `expenses:unclassified` with an `:inbox:` tag so it shows up in the Inbox badge.

Rules are written with the flat form above and stored as native hledger auto-postings:

```ledger
= desc:星巴克  ; rule:r-star order:10 enabled:true from:assets:wallet:alipay
    expenses:food:coffee      70.00%
    expenses:food             30.00%
    assets:wallet:alipay
```

Percentages are split with `Decimal` (28 digits) and **the rounding remainder goes to the last
posting**, so a 70/30 split of `4.50` is exactly `3.15 + 1.35` — never off by a cent. The first
matching rule (by `order`) wins, at write time only; later rule edits do not rewrite history until
you explicitly press 重算 (`rule_recompute`), which appends `; recomputed:<ISO8601>`.

---

## Money and currencies

- The base commodity and its display precision come from `commodity 1,000.00 CNY`; amounts are
  kept as `rust_decimal` end to end and only turned into `f64` for chart geometry.
- `P 2026-10-01 USD 7.12 CNY` lines are the single source of truth. A transaction converts at the
  latest rate **on or before its date**. Without a rate the transaction is marked `fx_missing`,
  shown in a grey "未折算" column, and **excluded from totals** — the app never assumes 1:1.
- Settings → 汇率 can enable `fx_sync`, which fetches ECB rates from `api.frankfurter.app` and
  appends `P` lines to `prices.ledger`. Off by default, always a manual click.

Forecasts combine `~` budget envelopes with the six-month moving average per category. They exist
only in memory and are labelled 预测 everywhere.

---

## Write safety

Every mutation goes through `crates/ledger-engine/src/writer.rs`:

1. the target file must have parsed without errors (`editable` transaction),
2. a snapshot is copied to `<vault>/.bak/<relative path>.<timestamp>` (200 kept by default),
3. the new bytes go to a same-directory temp file → `fsync` → `rename` → directory `fsync`,
4. the `(mtime, size, blake3)` recorded at parse time must still match, otherwise the write is
   refused with *"file was modified outside MoneyTracker — refresh and try again"*,
5. the operation is appended to `<appSupport>/dev.moneytracker.app/write-audit.jsonl`.

Editing a transaction rewrites **only that block's byte span**; the rest of the file is untouched
(asserted byte-for-byte by the engine tests). A `notify` watcher with a 300 ms debounce reloads
external changes and broadcasts `ledger:changed`, so a `git pull` or `vim` edit shows up live.

---

## Architecture

```
crates/ledger-core     AST, byte-span parser, printer, diagnostics        (~4.0k lines)
crates/ledger-query    hledger query subset: acct:/desc:/date:/tag:/amt:, and/or/not
crates/ledger-engine   include graph, index, balances, capture, rules, reports, writer
src-tauri              windows, tray, global shortcut, watcher, IPC (tauri-specta)
src                    React 19 + Vite + Tailwind v4 + Recharts + d3-sankey
fixtures/              grammar + error corpora, sample vault
```

- **Types are generated, never hand-written:** `pnpm bindings` runs
  `src-tauri/examples/export_bindings.rs`, which exports every command and event through
  `tauri-specta` into `src/lib/bindings.ts`. `src/lib/ipc.ts` is the only place the UI touches IPC.
- **One snapshot per process:** `Arc<RwLock<Option<Arc<Vault>>>>`; every window reads the same
  immutable snapshot and TanStack Query invalidates on `ledger:changed`.
- **Parser choice:** hand-written line-oriented recursive descent instead of `nom`, because
  span-accurate rewrites, precise `line:col` diagnostics and graceful recovery matter more than
  combinator ergonomics.

`docs/architecture.md` has the module map, the command surface and the state machine of a capture.

---

## Known limitations

- Not implemented on purpose: bank import/reconciliation, attachments, multi-user sync, automatic
  budget rollover, and cloud storage (put the vault in iCloud/git yourself).
- The supported ledger dialect is the subset above; anything else fails loudly rather than being
  approximated.
- `hledger` is not bundled. Cross-checks run only when it is on `PATH`.
- `specta` 2.0.0-rc.26 is vendored in `vendor/specta` with a two-line patch (stable Rust lacks the
  `fmt::from_fn` it uses); see `docs/architecture.md` for why the version cannot be bumped yet.

## License

Private project; no license granted yet.

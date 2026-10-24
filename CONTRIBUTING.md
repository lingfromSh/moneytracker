# Contributing to MoneyTracker

Thanks for taking the time to help. This project has one hard rule: **the vault format is a
contract**. Anything that changes bytes on disk needs a test that proves `hledger` still reads the
result, and a note in `docs/vault-format.md` when the grammar subset changes.

## Getting set up

```bash
git clone https://github.com/lingfromSh/moneytracker.git
cd moneytracker
pnpm install
pnpm tauri dev        # app with hot reload
```

Requirements: macOS 13+, Rust 1.91+ stable, Node 22+, pnpm 10+. Install `hledger`
(`brew install hledger`) if you want the differential tests to run.

## Before you open a pull request

```bash
./scripts/smoke.sh
```

That script runs, in order: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
a bindings-freshness check, `pnpm test`, `pnpm build`, and the `hledger` cross-check (skipped when
`hledger` is not installed). CI runs the same steps, so a green smoke run locally means a green
pull request.

If you changed anything exported to the front end (a command, an event, a DTO), regenerate the
bindings and commit them:

```bash
pnpm bindings         # cargo run -p moneytracker --example export_bindings
```

## Icons

`src-tauri/icons/icon.svg` is the source of the app icon and `trayTemplate.svg` of the menu-bar
glyph; both are plain SVG so a change is reviewable. Regenerate the raster set with

```bash
rsvg-convert -w 1024 -h 1024 src-tauri/icons/icon.svg -o src-tauri/icons/icon.png
rsvg-convert -w 22 -h 22 src-tauri/icons/trayTemplate.svg -o src-tauri/icons/trayTemplate.png
rsvg-convert -w 44 -h 44 src-tauri/icons/trayTemplate.svg -o src-tauri/icons/trayTemplate@2x.png
pnpm tauri icon src-tauri/icons/icon.png
rm -rf src-tauri/icons/android src-tauri/icons/ios src-tauri/icons/Square*Logo.png \
       src-tauri/icons/StoreLogo.png src-tauri/icons/icon.ico
```

`pnpm tauri icon` also emits Windows and mobile assets; the last line drops them again because the
app ships for macOS only. The menu-bar glyph must stay pure black with alpha — macOS recolours
template images itself, so a coloured tray icon looks broken in dark mode.

## Where things go

| Change | Location |
| --- | --- |
| Ledger syntax, diagnostics, printing | `crates/ledger-core` |
| Query language | `crates/ledger-query` |
| Loading, indexing, capture, rules, reports, writing | `crates/ledger-engine` |
| Tauri commands, windows, tray, watcher | `src-tauri` |
| UI | `src` (`routes/` per view, `components/` primitives and charts) |
| Fixtures and grammars | `fixtures/grammar`, `fixtures/errors`, `fixtures/vault/sample` |

## Coding conventions

- **Rust:** `cargo fmt` is authoritative; clippy runs with `-D warnings`. Comments explain *why*,
  identifiers explain *what*. No `unwrap()` on user input — return `EngineError`/`CmdError`.
- **Money:** always `rust_decimal`; convert to `f64` only for chart geometry.
- **Errors:** every user-visible failure carries a stable code (`engine.conflict`,
  `core.parse.bad_amount`, …). Codes are part of the API — do not rename them casually.
- **TypeScript:** strict mode with `noUnusedLocals`; all Rust access goes through `src/lib/ipc.ts`,
  never `invoke()` directly. Types come from `src/lib/bindings.ts` — never hand-write DTOs.
- **UI text:** the interface is Chinese; use existing `ui.tsx` primitives instead of new one-off
  styles. `aria-label`s are required on inputs so Testing Library can find them.

## Tests

Add the smallest test that would have caught the bug:

- new ledger syntax → a file in `fixtures/grammar/` (and `fixtures/errors/` with an `expected.txt`
  row when it must be rejected);
- engine behaviour → a unit test in the module, or an integration test when a whole vault is needed;
- anything touching the write path → extend `src-tauri/src/editing.rs` tests, which assert that
  only the target byte span changed;
- UI behaviour → a Vitest test with a mocked IPC layer, asserting what would be sent to Rust.

## Commit messages

Conventional-ish (`feat:`, `fix:`, `docs:`, `test:`, `chore:`), imperative mood, with the reason in
the body when the diff alone does not explain it. English is fine, Chinese is fine too.

## Reporting bugs

Please include: macOS version, MoneyTracker version (`Settings → 关于`), the capture line or the
ledger snippet that misbehaved, and the resulting diagnostic (`file:line:col: error[code]`). If the
bug is about parsing, the smallest `.ledger` file that reproduces it is the most useful thing you
can attach.

## Security

Please do not open a public issue for anything that could destroy ledger data. Send a description to
the maintainer through GitHub (private vulnerability report) instead; see [SECURITY.md](SECURITY.md).

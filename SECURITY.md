# Security policy

MoneyTracker writes to your ledger files, so the interesting failure mode is **data loss or silent
corruption**, not remote compromise — the app has no network service, no telemetry and no cloud
account (the only outbound request is an optional, user-triggered FX fetch from
`api.frankfurter.app`).

## What is already guaranteed

- Every write is preceded by a snapshot under `<vault>/.bak/` (200 kept by default).
- Writes are atomic (temp file → `fsync` → `rename` → directory `fsync`).
- A file that changed on disk since it was parsed is never overwritten; the app refuses and asks
  you to refresh.
- Unsigned, unnotarized builds are the norm right now; download them only from this repository's
  releases page.

## Reporting a vulnerability

Please **do not open a public issue** for anything that could destroy or corrupt ledger data,
escape the vault directory, or execute code from ledger content. Use GitHub's private
vulnerability reporting on this repository instead. Include:

- affected version (Settings → 关于),
- the smallest `.ledger` file or capture line that reproduces it,
- what happened versus what you expected,
- whether a `.bak/` snapshot or git history can restore the lost state.

You can expect an acknowledgement within a few days. Fixes ship with a test that reproduces the
report, and the advisory credits you unless you prefer otherwise.

## Optional LLM assistance

Natural-language capture is off by default and only runs when you switch the palette to 自然语言
(<kbd>Tab</kbd>) with a provider configured in 设置 → AI 记一笔.

- What is sent: the sentence you typed, your account names, your base commodity and today's date.
  Nothing else from the ledger, no balances, no file paths.
- Where the credential lives: `<app config dir>/llm.json`, created with mode `0600`, containing only
  `{"api_key": "…"}`. It is never written into the ledger, `settings.json`, the write audit or logs.
- What the model can change: nothing. It returns a JSON draft; the app turns that into one capture
  line which still goes through the parser, the rule engine, the double-entry check and the same
  snapshot/audit write path as a hand-typed entry. The draft is shown before anything is written.
- Failure handling: a rejected request, a timeout or an unusable draft leaves the ledger untouched
  and surfaces the error in the palette; you can always switch back and type the line yourself.

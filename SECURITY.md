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

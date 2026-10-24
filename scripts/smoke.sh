#!/usr/bin/env bash
# Full verification pass: Rust, TypeScript, bindings freshness and — when available —
# a balance comparison against the real hledger binary.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -s "$HOME/.nvm/nvm.sh" ]]; then
  # shellcheck disable=SC1091
  source "$HOME/.nvm/nvm.sh" >/dev/null 2>&1 || true
  nvm use 22 >/dev/null 2>&1 || true
fi

step() { printf '\n\033[1m▸ %s\033[0m\n' "$*"; }

step "cargo fmt --check"
cargo fmt --all -- --check

step "cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

step "cargo test"
cargo test --workspace

step "type bindings are up to date"
before=$(shasum src/lib/bindings.ts | cut -d' ' -f1)
cargo run -q -p moneytracker --example export_bindings >/dev/null
after=$(shasum src/lib/bindings.ts | cut -d' ' -f1)
if [[ "$before" != "$after" ]]; then
  echo "src/lib/bindings.ts changed — commit the regenerated bindings" >&2
  exit 1
fi

step "front-end tests"
pnpm test

step "front-end build"
pnpm build

if command -v hledger >/dev/null 2>&1; then
  step "hledger cross-check (sample vault)"
  hledger -f fixtures/vault/sample/index.ledger balance --flat --no-total
  echo
  echo "Compare the figures above with the Dashboard / Accounts view of the sample vault."
else
  echo
  echo "hledger not installed — skipping the differential balance check."
fi

printf '\n\033[32mAll checks passed.\033[0m\n'

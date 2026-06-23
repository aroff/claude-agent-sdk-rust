#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../../.."

echo "== cargo fmt --check =="
cargo fmt --check

echo "== cargo clippy --all-targets -- -D warnings =="
cargo clippy --all-targets -- -D warnings

echo "== cargo test =="
cargo test

if command -v claude >/dev/null 2>&1; then
  echo "== claude --version =="
  claude --version

  echo "== cargo test --test live_query -- --nocapture --ignored =="
  cargo test --test live_query -- --nocapture --ignored

  echo "== cargo test --test live_client -- --nocapture --ignored =="
  cargo test --test live_client -- --nocapture --ignored
else
  echo "claude binary not found; skipped ignored live e2e tests" >&2
fi

#!/usr/bin/env bash
set -euo pipefail

iterations="${1:-3}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

for ((i = 1; i <= iterations; i++)); do
  echo "[verify-loop] iteration ${i}/${iterations}"
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo build --workspace --release
done

echo "[verify-loop] all checks passed"

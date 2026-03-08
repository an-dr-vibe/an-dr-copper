#!/usr/bin/env bash
set -euo pipefail

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup is required. Install from https://rustup.rs"
  exit 1
fi

rustup component add rustfmt clippy

if command -v deno >/dev/null 2>&1; then
  echo "deno found"
else
  echo "deno not found (optional for runtime execution). Install from https://deno.com"
fi

echo "Bootstrap complete."

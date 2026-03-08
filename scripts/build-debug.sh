#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo build --workspace
rm -rf "${repo_root}/target/debug/extensions"
cp -R "${repo_root}/extensions" "${repo_root}/target/debug/extensions"
echo "Debug build complete."

#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

output_dir="${OUTPUT_DIR:-$repo_root/dist/release}"
host_triple="$(rustc -vV | awk '/^host: / { print $2 }')"
bundle_name="copper-${host_triple}"
bundle_dir="${output_dir}/${bundle_name}"
binary_name="copperd"
binary_path="${repo_root}/target/release/${binary_name}"

cargo build --workspace --release

if [[ ! -f "$binary_path" ]]; then
  echo "Release binary not found: $binary_path" >&2
  exit 1
fi

rm -rf "$bundle_dir"
mkdir -p "$bundle_dir/extensions-published"

cp "$binary_path" "${bundle_dir}/${binary_name}"
cp "${repo_root}/README.md" "${bundle_dir}/README.md"
cp "${repo_root}/docs/QUICKSTART.md" "${bundle_dir}/QUICKSTART.md"
cp -R "${repo_root}/extensions" "${bundle_dir}/core-extensions"

shopt -s nullglob
for descriptor in "${repo_root}"/extensions/*/descriptor.json; do
  ext_dir="$(dirname "$descriptor")"
  ext_base="$(basename "$ext_dir")"

  id="$(grep -m1 '"id"' "$descriptor" | sed -E 's/.*"id"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"
  version="$(grep -m1 '"version"' "$descriptor" | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"
  if [[ -z "$id" || -z "$version" ]]; then
    echo "descriptor is missing id/version: $descriptor" >&2
    exit 1
  fi

  tar -czf "${bundle_dir}/extensions-published/${id}-${version}.tar.gz" \
    -C "${repo_root}/extensions" \
    "${ext_base}"
done
shopt -u nullglob

archive_path="${output_dir}/${bundle_name}.tar.gz"
mkdir -p "$output_dir"
rm -f "$archive_path"
tar -czf "$archive_path" -C "$output_dir" "$bundle_name"

echo "Release build complete."
echo "Bundle directory: $bundle_dir"
echo "Bundle archive:  $archive_path"

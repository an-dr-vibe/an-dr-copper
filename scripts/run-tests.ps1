#!/usr/bin/env pwsh
param(
  [string]$ExtensionsDir = "./extensions",
  [switch]$SkipClippy
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

function Invoke-Step {
  param([scriptblock]$Command, [string]$Description)
  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "$Description failed with exit code $LASTEXITCODE"
  }
}

# Bones is a pinned external workspace with its own formatting policy. Scope
# Copper's formatting gate to the package maintained by this repository.
Invoke-Step { cargo fmt -p copperd --check } "cargo fmt"
if (-not $SkipClippy) {
  Invoke-Step { cargo clippy --workspace --all-targets -- -D warnings } "cargo clippy"
}
Invoke-Step { cargo test -p copperd --test extension_utr } "extension UTR"
Invoke-Step { cargo test --workspace } "cargo test"
Invoke-Step { cargo run -p copperd -- verify --extensions-dir $ExtensionsDir } "copperd verify"

Write-Host "Tests and extension verification passed."

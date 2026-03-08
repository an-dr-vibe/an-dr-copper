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

Invoke-Step { cargo fmt --all --check } "cargo fmt"
if (-not $SkipClippy) {
  Invoke-Step { cargo clippy --workspace --all-targets -- -D warnings } "cargo clippy"
}
Invoke-Step { cargo test --workspace } "cargo test"
Invoke-Step { cargo run -p copperd -- verify --extensions-dir $ExtensionsDir } "copperd verify"

Write-Host "Tests and extension verification passed."

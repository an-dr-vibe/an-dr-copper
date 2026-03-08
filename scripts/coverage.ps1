#!/usr/bin/env pwsh
param(
  [string]$Toolchain = "1.88.0",
  [string]$IgnoreFilenameRegex = "(\\.cargo|rustc|tests/cli_integration.rs)"
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

# Coverage tooling on the default 1.86 toolchain is unreliable on Windows.
# Use a newer toolchain explicitly for stable llvm-cov output.
Invoke-Step { rustup toolchain install $Toolchain } "rustup toolchain install"
Invoke-Step { cargo +$Toolchain install cargo-llvm-cov } "cargo-llvm-cov install"
Invoke-Step {
  cargo +$Toolchain llvm-cov --workspace --summary-only --ignore-filename-regex $IgnoreFilenameRegex
} "cargo llvm-cov"

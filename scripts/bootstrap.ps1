#!/usr/bin/env pwsh
param()

$ErrorActionPreference = "Stop"

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
  Write-Error "rustup is required. Install from https://rustup.rs"
}

rustup component add rustfmt clippy
if ($LASTEXITCODE -ne 0) {
  throw "Failed to install rustfmt/clippy components."
}

Write-Host "Bootstrap complete."

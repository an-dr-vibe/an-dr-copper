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

if (Get-Command deno -ErrorAction SilentlyContinue) {
  Write-Host "deno found"
} else {
  Write-Host "deno not found (optional for runtime execution). Install from https://deno.com"
}

Write-Host "Bootstrap complete."

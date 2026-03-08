#!/usr/bin/env pwsh
param()

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

cargo build --workspace --release
if ($LASTEXITCODE -ne 0) {
  throw "Release build failed with exit code $LASTEXITCODE"
}
Write-Host "Release build complete."

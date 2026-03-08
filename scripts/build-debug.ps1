#!/usr/bin/env pwsh
param()

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

cargo build --workspace
if ($LASTEXITCODE -ne 0) {
  throw "Debug build failed with exit code $LASTEXITCODE"
}
Write-Host "Debug build complete."

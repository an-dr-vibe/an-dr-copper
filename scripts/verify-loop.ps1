#!/usr/bin/env pwsh
param(
  [int]$Iterations = 3
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

for ($i = 1; $i -le $Iterations; $i++) {
  Write-Host "[verify-loop] iteration $i/$Iterations"
  & (Join-Path $PSScriptRoot "run-tests.ps1")
  if ($LASTEXITCODE -ne 0) {
    throw "run-tests.ps1 failed with exit code $LASTEXITCODE"
  }
  cargo build --workspace --release
  if ($LASTEXITCODE -ne 0) {
    throw "Release build failed with exit code $LASTEXITCODE"
  }
}

Write-Host "[verify-loop] all checks passed"

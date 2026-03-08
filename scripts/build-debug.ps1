#!/usr/bin/env pwsh
param()

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

cargo build --workspace
if ($LASTEXITCODE -ne 0) {
  throw "Debug build failed with exit code $LASTEXITCODE"
}

$debugExtensionsDir = Join-Path $repoRoot "target/debug/extensions"
if (Test-Path $debugExtensionsDir) {
  Remove-Item $debugExtensionsDir -Recurse -Force
}
Copy-Item -Path (Join-Path $repoRoot "extensions") -Destination $debugExtensionsDir -Recurse -Force

Write-Host "Debug build complete."

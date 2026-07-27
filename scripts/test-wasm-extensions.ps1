#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$extensionsRoot = Join-Path $repoRoot "extensions"
$testTargetDir = Join-Path $repoRoot "target/wasm-component-tests"

$components = Get-ChildItem -LiteralPath $extensionsRoot -Directory |
  ForEach-Object {
    $manifestPath = Join-Path $_.FullName "manifest.json"
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.runtime -and $manifest.runtime.kind -eq "wasm-component") {
      [pscustomobject]@{
        Id = [string]$manifest.id
        Root = $_.FullName
        Cargo = Join-Path $_.FullName "component/Cargo.toml"
      }
    }
  }

if (-not $components) {
  throw "No shipped WASM Components found in $extensionsRoot"
}

foreach ($component in $components) {
  if (-not (Test-Path -LiteralPath $component.Cargo -PathType Leaf)) {
    throw "$($component.Id) does not include component/Cargo.toml"
  }

  cargo fmt --manifest-path $component.Cargo -- --check
  if ($LASTEXITCODE -ne 0) {
    throw "$($component.Id) component formatting failed with exit code $LASTEXITCODE"
  }
  cargo test `
    --manifest-path $component.Cargo `
    --locked `
    --target-dir $testTargetDir
  if ($LASTEXITCODE -ne 0) {
    throw "$($component.Id) component tests failed with exit code $LASTEXITCODE"
  }
  cargo clippy `
    --manifest-path $component.Cargo `
    --locked `
    --target-dir $testTargetDir `
    --target wasm32-wasip2 `
    --lib `
    -- `
    -D warnings
  if ($LASTEXITCODE -ne 0) {
    throw "$($component.Id) component Clippy failed with exit code $LASTEXITCODE"
  }
  & (Join-Path $PSScriptRoot "build-wasm-extension.ps1") `
    -ExtensionDir $component.Root `
    -Check `
    -SkipValidation
}

Write-Host "Verified $($components.Count) shipped WASM Component(s)."

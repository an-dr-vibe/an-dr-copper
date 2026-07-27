#!/usr/bin/env pwsh
param(
  [Parameter(Mandatory = $true)]
  [string]$ExtensionDir,
  [switch]$Package,
  [string]$PackageOutput = "",
  [string]$TargetDir = "",
  [switch]$SkipValidation
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$resolvedExtensionDir = (Resolve-Path -LiteralPath $ExtensionDir).Path
$manifestPath = Join-Path $resolvedExtensionDir "manifest.json"
$componentManifestPath = Join-Path $resolvedExtensionDir "component/Cargo.toml"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
  throw "Extension manifest not found: $manifestPath"
}
if (-not (Test-Path -LiteralPath $componentManifestPath -PathType Leaf)) {
  throw "Rust component manifest not found: $componentManifestPath"
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$id = [string]$manifest.id
$expectedArtifact = "$id.wasm"
if (
  -not $manifest.runtime -or
  $manifest.runtime.kind -ne "wasm-component" -or
  $manifest.runtime.abi -ne "copper.component/1" -or
  $manifest.runtime.artifact -ne $expectedArtifact
) {
  throw "Manifest runtime must declare wasm-component/copper.component/1 artifact $expectedArtifact"
}

rustup target add wasm32-wasip2
if ($LASTEXITCODE -ne 0) {
  throw "rustup target add wasm32-wasip2 failed with exit code $LASTEXITCODE"
}

$componentDir = Split-Path -Parent $componentManifestPath
$resolvedTargetDir = if ($TargetDir) {
  [IO.Path]::GetFullPath($TargetDir)
} else {
  Join-Path $repoRoot "target/wasm-components"
}
$lockPath = Join-Path $componentDir "Cargo.lock"
if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
  throw "Deterministic component builds require a committed Cargo.lock: $lockPath"
}

$previousIncremental = $env:CARGO_INCREMENTAL
$previousSourceDate = $env:SOURCE_DATE_EPOCH
$env:CARGO_INCREMENTAL = "0"
$env:SOURCE_DATE_EPOCH = "315532800"
try {
  cargo build `
    --manifest-path $componentManifestPath `
    --target-dir $resolvedTargetDir `
    --target wasm32-wasip2 `
    --release `
    --locked
  if ($LASTEXITCODE -ne 0) {
    throw "Rust component build failed with exit code $LASTEXITCODE"
  }
} finally {
  $env:CARGO_INCREMENTAL = $previousIncremental
  $env:SOURCE_DATE_EPOCH = $previousSourceDate
}

$crateArtifact = $id.Replace('-', '_') + ".wasm"
$builtArtifact = Join-Path $resolvedTargetDir "wasm32-wasip2/release/$crateArtifact"
if (-not (Test-Path -LiteralPath $builtArtifact -PathType Leaf)) {
  throw "Built component not found: $builtArtifact"
}
$packageArtifact = Join-Path $resolvedExtensionDir $expectedArtifact
Copy-Item -LiteralPath $builtArtifact -Destination $packageArtifact -Force

if (-not $SkipValidation) {
  Push-Location $repoRoot
  try {
    cargo run -p copperd --no-default-features -- validate $manifestPath
    if ($LASTEXITCODE -ne 0) {
      throw "Copper extension validation failed with exit code $LASTEXITCODE"
    }
  } finally {
    Pop-Location
  }
}

Write-Host "Built component: $packageArtifact"
if ($Package) {
  $packageArgs = @{
    ExtensionDir = $resolvedExtensionDir
  }
  if ($PackageOutput) {
    $packageArgs.OutputPath = $PackageOutput
  }
  if ($SkipValidation) {
    $packageArgs.SkipValidation = $true
  }
  & (Join-Path $PSScriptRoot "package-extension.ps1") @packageArgs
}

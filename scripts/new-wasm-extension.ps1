#!/usr/bin/env pwsh
param(
  [Parameter(Mandatory = $true)]
  [ValidatePattern('^[a-z0-9-]+$')]
  [string]$Id,
  [Parameter(Mandatory = $true)]
  [string]$Name,
  [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$templateRoot = Join-Path $repoRoot "sdk/templates/rust-component"
$destination = if ($OutputDir) {
  [IO.Path]::GetFullPath($OutputDir)
} else {
  Join-Path $repoRoot "extensions/$Id"
}

if (Test-Path -LiteralPath $destination) {
  throw "Refusing to overwrite existing extension directory: $destination"
}

New-Item -ItemType Directory -Path $destination | Out-Null
Copy-Item -LiteralPath (Join-Path $templateRoot "manifest.json") -Destination $destination
Copy-Item -LiteralPath (Join-Path $templateRoot "README.md") -Destination $destination
$componentDir = Join-Path $destination "component"
$componentSourceDir = Join-Path $componentDir "src"
New-Item -ItemType Directory -Path $componentSourceDir -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $templateRoot "component/Cargo.toml") -Destination $componentDir
Copy-Item -LiteralPath (Join-Path $templateRoot "component/Cargo.lock") -Destination $componentDir
Copy-Item -LiteralPath (Join-Path $templateRoot "component/src/lib.rs") -Destination $componentSourceDir

$manifestPath = Join-Path $destination "manifest.json"
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$manifest.id = $Id
$manifest.name = $Name
$manifest.trigger = $Id
$manifest.runtime.artifact = "$Id.wasm"
$manifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $manifestPath -Encoding utf8

$componentManifestPath = Join-Path $componentDir "Cargo.toml"
$componentManifest = Get-Content -LiteralPath $componentManifestPath -Raw
$sdkRelativePath = [IO.Path]::GetRelativePath($componentDir, (Join-Path $repoRoot "sdk/rust"))
$sdkCargoPath = $sdkRelativePath.Replace('\', '/')
$componentManifest = $componentManifest.Replace(
  'name = "copper-component-template"',
  "name = `"$Id`""
).Replace(
  'path = "../../../rust"',
  "path = `"$sdkCargoPath`""
)
Set-Content -LiteralPath $componentManifestPath -Value $componentManifest -Encoding utf8

$lockPath = Join-Path $componentDir "Cargo.lock"
$lock = Get-Content -LiteralPath $lockPath -Raw
$lock = $lock.Replace(
  'name = "copper-component-template"',
  "name = `"$Id`""
)
Set-Content -LiteralPath $lockPath -Value $lock -Encoding utf8

Write-Host "Created Copper WASM extension: $destination"
Write-Host "Build: ./scripts/build-wasm-extension.ps1 -ExtensionDir `"$destination`" -Package"

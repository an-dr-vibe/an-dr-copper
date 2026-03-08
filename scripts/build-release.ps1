#!/usr/bin/env pwsh
param(
  [string]$OutputDir = "./dist/release"
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

function Publish-ExtensionArchives {
  param(
    [string]$ExtensionsRoot,
    [string]$PublishRoot
  )

  New-Item -ItemType Directory -Path $PublishRoot -Force | Out-Null

  Get-ChildItem -Path $ExtensionsRoot -Directory | ForEach-Object {
    $extensionDir = $_.FullName
    $descriptorPath = Join-Path $extensionDir "manifest.json"
    if (-not (Test-Path $descriptorPath)) {
      return
    }

    $descriptor = Get-Content -Path $descriptorPath -Raw | ConvertFrom-Json
    $id = [string]$descriptor.id
    $version = [string]$descriptor.version
    if ([string]::IsNullOrWhiteSpace($id) -or [string]::IsNullOrWhiteSpace($version)) {
      throw "descriptor is missing id/version: $descriptorPath"
    }

    $archivePath = Join-Path $PublishRoot "$id-$version.zip"
    if (Test-Path $archivePath) {
      Remove-Item $archivePath -Force
    }
    Compress-Archive -Path (Join-Path $extensionDir "*") -DestinationPath $archivePath -Force
  }
}

cargo build --workspace --release
if ($LASTEXITCODE -ne 0) {
  throw "Release build failed with exit code $LASTEXITCODE"
}

$releaseExtensionsDir = Join-Path $repoRoot "target/release/extensions"
if (Test-Path $releaseExtensionsDir) {
  Remove-Item $releaseExtensionsDir -Recurse -Force
}
Copy-Item -Path (Join-Path $repoRoot "extensions") -Destination $releaseExtensionsDir -Recurse -Force

$hostTriple = (rustc -vV | Select-String "^host: ").ToString().Split(" ")[1].Trim()
$exeName = if ($IsWindows) { "copperd.exe" } else { "copperd" }
$binaryPath = Join-Path $repoRoot "target/release/$exeName"
if (-not (Test-Path $binaryPath)) {
  throw "Release binary not found: $binaryPath"
}

$resolvedOutputDir = (Resolve-Path -Path $OutputDir -ErrorAction SilentlyContinue)
if (-not $resolvedOutputDir) {
  New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
  $resolvedOutputDir = Resolve-Path -Path $OutputDir
}
$releaseRoot = [string]$resolvedOutputDir
$bundleName = "copper-$hostTriple"
$bundlePath = Join-Path $releaseRoot $bundleName

if (Test-Path $bundlePath) {
  Remove-Item $bundlePath -Recurse -Force
}

New-Item -ItemType Directory -Path $bundlePath -Force | Out-Null
Copy-Item -Path $binaryPath -Destination (Join-Path $bundlePath $exeName) -Force
Copy-Item -Path (Join-Path $repoRoot "README.md") -Destination (Join-Path $bundlePath "README.md") -Force
Copy-Item -Path (Join-Path $repoRoot "docs/QUICKSTART.md") -Destination (Join-Path $bundlePath "QUICKSTART.md") -Force

$bundleExtensions = Join-Path $bundlePath "extensions"
Copy-Item -Path (Join-Path $repoRoot "extensions") -Destination $bundleExtensions -Recurse -Force

$publishedExtensionsPath = Join-Path $bundlePath "extensions-published"
Publish-ExtensionArchives -ExtensionsRoot (Join-Path $repoRoot "extensions") -PublishRoot $publishedExtensionsPath

$releaseArchive = Join-Path $releaseRoot "$bundleName.zip"
if (Test-Path $releaseArchive) {
  Remove-Item $releaseArchive -Force
}
Compress-Archive -Path $bundlePath -DestinationPath $releaseArchive -Force

Write-Host "Release build complete."
Write-Host "Bundle directory: $bundlePath"
Write-Host "Bundle archive:  $releaseArchive"


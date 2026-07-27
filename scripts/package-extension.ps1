#!/usr/bin/env pwsh
param(
  [Parameter(Mandatory = $true)]
  [string]$ExtensionDir,
  [string]$OutputPath = "",
  [switch]$SkipValidation
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$resolvedExtensionDir = (Resolve-Path -LiteralPath $ExtensionDir).Path
$manifestPath = Join-Path $resolvedExtensionDir "manifest.json"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
  throw "Extension manifest not found: $manifestPath"
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$id = [string]$manifest.id
$version = [string]$manifest.version
if ([string]::IsNullOrWhiteSpace($id) -or [string]::IsNullOrWhiteSpace($version)) {
  throw "Extension manifest must contain id and version: $manifestPath"
}

$runtimeFile = if ($manifest.runtime -and $manifest.runtime.kind -eq "wasm-component") {
  if (
    $manifest.runtime.abi -ne "copper.component/1" -or
    $manifest.runtime.artifact -ne "$id.wasm"
  ) {
    throw "Component packages require copper.component/1 artifact $id.wasm"
  }
  [string]$manifest.runtime.artifact
} else {
  "main.ts"
}
$runtimePath = [IO.Path]::GetFullPath((Join-Path $resolvedExtensionDir $runtimeFile))
$extensionPrefix = $resolvedExtensionDir.TrimEnd(
  [IO.Path]::DirectorySeparatorChar,
  [IO.Path]::AltDirectorySeparatorChar
) + [IO.Path]::DirectorySeparatorChar
if (-not $runtimePath.StartsWith($extensionPrefix, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Extension runtime artifact escapes its package: $runtimeFile"
}
if (-not (Test-Path -LiteralPath $runtimePath -PathType Leaf)) {
  throw "Extension runtime artifact not found: $runtimePath"
}

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

if (-not $OutputPath) {
  $publishDir = Join-Path $repoRoot "dist/extensions"
  New-Item -ItemType Directory -Path $publishDir -Force | Out-Null
  $OutputPath = Join-Path $publishDir "$id-$version.zip"
}
$archivePath = [IO.Path]::GetFullPath($OutputPath)
$archiveParent = Split-Path -Parent $archivePath
New-Item -ItemType Directory -Path $archiveParent -Force | Out-Null

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$fixedTimestamp = [DateTimeOffset]::new(1980, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
$files = @(
  @{ Name = "manifest.json"; Path = $manifestPath },
  @{ Name = $runtimeFile.Replace('\', '/'); Path = $runtimePath }
) | Sort-Object { $_.Name }

$stream = [IO.File]::Open($archivePath, [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
  $archive = [IO.Compression.ZipArchive]::new(
    $stream,
    [IO.Compression.ZipArchiveMode]::Create,
    $false
  )
  try {
    foreach ($file in $files) {
      $entry = $archive.CreateEntry($file.Name, [IO.Compression.CompressionLevel]::Optimal)
      $entry.LastWriteTime = $fixedTimestamp
      $entryStream = $entry.Open()
      try {
        $bytes = [IO.File]::ReadAllBytes($file.Path)
        $entryStream.Write($bytes, 0, $bytes.Length)
      } finally {
        $entryStream.Dispose()
      }
    }
  } finally {
    $archive.Dispose()
  }
} finally {
  $stream.Dispose()
}

Write-Host "Packaged extension: $archivePath"

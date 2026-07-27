#!/usr/bin/env pwsh
param(
  [string]$OutputDir = "./dist/release",
  [string]$Target = ""
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
    & (Join-Path $PSScriptRoot "package-extension.ps1") `
      -ExtensionDir $extensionDir `
      -OutputPath $archivePath `
      -SkipValidation
    if ($LASTEXITCODE -ne 0) {
      throw "extension packaging failed with exit code $LASTEXITCODE"
    }
  }
}

function Set-MsvcCrossEnv {
  param([string]$Arch)
  $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { throw "vswhere.exe not found; install Visual Studio" }
  $vsPath = (& $vswhere -latest -property installationPath).Trim()
  $vcvarsall = Join-Path $vsPath "VC\Auxiliary\Build\vcvarsall.bat"
  if (-not (Test-Path $vcvarsall)) { throw "vcvarsall.bat not found at: $vcvarsall" }
  Write-Host "Configuring MSVC cross-compile environment: $Arch"
  $envLines = cmd /c "`"$vcvarsall`" $Arch > nul 2>&1 && set"
  foreach ($line in $envLines) {
    if ($line -match '^([^=]+)=(.*)$') {
      [System.Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
    }
  }
}

$hostTriple = (rustc -vV | Select-String "^host: ").ToString().Split(" ")[1].Trim()
$buildTarget = if ($Target -ne "") { $Target } else { $hostTriple }

if ($buildTarget -ne $hostTriple) {
  if ($buildTarget -eq 'aarch64-pc-windows-msvc' -and $hostTriple -match 'x86_64.*windows') {
    Set-MsvcCrossEnv -Arch 'x64_arm64'
  }
  rustup target add $buildTarget
  if ($LASTEXITCODE -ne 0) { throw "rustup target add $buildTarget failed" }
  cargo build --workspace --release --target $buildTarget
} else {
  cargo build --workspace --release
}
if ($LASTEXITCODE -ne 0) {
  throw "Release build failed with exit code $LASTEXITCODE"
}

$binaryDir = if ($buildTarget -ne $hostTriple) {
  Join-Path $repoRoot "target/$buildTarget/release"
} else {
  Join-Path $repoRoot "target/release"
}

$targetIsWindows = $buildTarget -match "windows"
$exeName = if ($targetIsWindows) { "copper.exe" } else { "copper" }
$binaryPath = Join-Path $binaryDir $exeName
if (-not (Test-Path $binaryPath)) {
  throw "Release binary not found: $binaryPath"
}

$resolvedOutputDir = (Resolve-Path -Path $OutputDir -ErrorAction SilentlyContinue)
if (-not $resolvedOutputDir) {
  New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
  $resolvedOutputDir = Resolve-Path -Path $OutputDir
}
$releaseRoot = [string]$resolvedOutputDir
$bundleName = "copper-$buildTarget"
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

$bundleUiDir = Join-Path $bundlePath "ui"
Copy-Item -Path (Join-Path $repoRoot "daemon/ui") -Destination $bundleUiDir -Recurse -Force

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


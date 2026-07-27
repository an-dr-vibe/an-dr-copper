#!/usr/bin/env pwsh
param(
  [string]$Toolchain = "",
  [double]$MinLineCoverage = 0.0,
  [switch]$FailOnUnderTarget,
  [switch]$ReuseBuild
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$IgnoreFilenameRegex = '(\.cargo|rustc|tests[/\\])'
$LcovPath = Join-Path $repoRoot "target/coverage-full.info"
$CoverageTarget = Join-Path $repoRoot "target/llvm-cov-target"
$CoverageRustVersion = "1.94.0"
$UseEmulatedCoverageToolchain = $false

if ([string]::IsNullOrWhiteSpace($Toolchain)) {
  if (
    $IsWindows -and
    [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq
      [System.Runtime.InteropServices.Architecture]::Arm64
  ) {
    # LLVM's native Windows ARM64 coverage runtime currently writes malformed
    # profile records. Windows ARM64 runs x64 binaries, so use the supported x64
    # toolchain for the audit while release builds remain native ARM64.
    $Toolchain = "$CoverageRustVersion-x86_64-pc-windows-msvc"
    $UseEmulatedCoverageToolchain = $true
  } else {
    $Toolchain = $CoverageRustVersion
  }
}

function Invoke-Step {
  param([scriptblock]$Command, [string]$Description)
  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "$Description failed with exit code $LASTEXITCODE"
  }
}

function Get-LineCoverageFromSummary {
  param([string[]]$CoverageOutput)

  $totalLine = $CoverageOutput | Where-Object { $_ -match '^\s*TOTAL\s+' } | Select-Object -Last 1
  if (-not $totalLine) {
    throw "failed to parse TOTAL row from llvm-cov output"
  }

  $tokens = ($totalLine -replace '\s+', ' ').Trim().Split(' ')
  if ($tokens.Length -lt 10) {
    throw "failed to parse coverage columns from TOTAL row: $totalLine"
  }

  return [double]($tokens[9].TrimEnd('%'))
}

function Is-DeclarationOnlyRustFile {
  param([string]$Path)

  $content = Get-Content -LiteralPath $Path -Raw
  $content = [regex]::Replace($content, '(?m)^\s*//.*$', '')
  $content = [regex]::Replace($content, '(?m)^\s*#!?\[.*\]\s*$', '')
  $content = [regex]::Replace(
    $content,
    '(?ms)^\s*(?:pub\s+)?use\s+.*?;\s*',
    ''
  )
  $content = [regex]::Replace(
    $content,
    '(?m)^\s*(?:pub\s+)?mod\s+[A-Za-z0-9_]+;\s*$',
    ''
  )
  return [string]::IsNullOrWhiteSpace($content)
}

function Get-RustSourceFiles {
  param([string]$Root)

  $ripgrep = Get-Command rg -ErrorAction SilentlyContinue
  if ($ripgrep) {
    return @(rg --files $Root -g "*.rs" | ForEach-Object { (Resolve-Path $_).Path })
  }

  return @(
    Get-ChildItem -Path $Root -Recurse -Filter "*.rs" -File |
      ForEach-Object { $_.FullName }
  )
}

function Get-LcovSourceFiles {
  param([string]$LcovFile)

  $ripgrep = Get-Command rg -ErrorAction SilentlyContinue
  if ($ripgrep) {
    return @(rg '^SF:' $LcovFile | ForEach-Object { $_.Substring(3) })
  }

  return @(
    Select-String -Path $LcovFile -Pattern '^SF:' |
      ForEach-Object { $_.Line.Substring(3) }
  )
}

function Assert-CoverageFileParity {
  param([string]$LcovFile)

  $sourceFiles = Get-RustSourceFiles "daemon/src"
  $coveredFiles = Get-LcovSourceFiles $LcovFile

  $coveredSet = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
  $coveredFiles | ForEach-Object { [void]$coveredSet.Add($_) }

  $missingActionable = New-Object System.Collections.Generic.List[string]
  $missingDeclarationOnly = New-Object System.Collections.Generic.List[string]

  foreach ($source in $sourceFiles) {
    if ([System.IO.Path]::GetFileName($source) -match '_tests\.rs$') {
      continue
    }
    if ($coveredSet.Contains($source)) {
      continue
    }

    if (Is-DeclarationOnlyRustFile $source) {
      [void]$missingDeclarationOnly.Add($source)
    } else {
      [void]$missingActionable.Add($source)
    }
  }

  if ($missingDeclarationOnly.Count -gt 0) {
    Write-Host "Coverage audit note: declaration-only files not present in LCOV:"
    $missingDeclarationOnly | Sort-Object | ForEach-Object { Write-Host "  $_" }
  }

  if ($missingActionable.Count -gt 0) {
    $list = ($missingActionable | Sort-Object | ForEach-Object { "  $_" }) -join "`n"
    throw "coverage audit failed: source files missing from LCOV SF list:`n$list"
  }
}

# Pin the first supported Rust toolchain for this Bones/Wasmtime generation.
# Newer stable toolchains remain valid for normal builds, but coverage must be
# reproducible.
$rustupInstallArgs = @("toolchain", "install", $Toolchain)
if ($UseEmulatedCoverageToolchain) {
  $rustupInstallArgs += "--force-non-host"
}
Invoke-Step { rustup @rustupInstallArgs } "rustup toolchain install"
Invoke-Step { cargo +$Toolchain install cargo-llvm-cov } "cargo-llvm-cov install"

# cargo-llvm-cov's own cleanup can retain instrumented binaries from another
# target architecture. Start the normal audit from a known single-architecture
# target; -ReuseBuild is reserved for iterating on an already matching build.
if (-not $ReuseBuild -and (Test-Path $CoverageTarget)) {
  Invoke-Step {
    cargo +$Toolchain clean --target-dir $CoverageTarget
  } "coverage target cleanup"
}

# Do not let profiles emitted by a previous toolchain/architecture contaminate
# this audit. cargo-llvm-cov cleans build artifacts but can leave raw profiles
# behind when a previous report failed before cleanup completed.
if (Test-Path $CoverageTarget) {
  Get-ChildItem -LiteralPath $CoverageTarget -Filter "*.profraw" -File |
    Remove-Item -Force
  foreach ($staleReport in @("an-dr-copper.profdata", "an-dr-copper-profraw-list")) {
    Remove-Item -LiteralPath (Join-Path $CoverageTarget $staleReport) -Force -ErrorAction SilentlyContinue
  }
}

$coverageRunArgs = @("+$Toolchain", "llvm-cov", "--workspace")
if ($ReuseBuild) {
  $coverageRunArgs += "--no-clean"
} else {
  $coverageRunArgs += "--no-report"
}
Invoke-Step {
  cargo @coverageRunArgs
} "cargo llvm-cov test run"

$coverageOutput = & cargo +$Toolchain llvm-cov report --summary-only --ignore-filename-regex $IgnoreFilenameRegex
if ($LASTEXITCODE -ne 0) {
  throw "cargo llvm-cov summary failed with exit code $LASTEXITCODE"
}

$coverageOutput | ForEach-Object { Write-Host $_ }
$lineCoverage = Get-LineCoverageFromSummary $coverageOutput
if ($FailOnUnderTarget -and $lineCoverage -lt $MinLineCoverage) {
  throw "line coverage $lineCoverage% is below required $MinLineCoverage%"
}

Invoke-Step {
  cargo +$Toolchain llvm-cov report --ignore-filename-regex $IgnoreFilenameRegex --lcov --output-path $LcovPath
} "cargo llvm-cov lcov"
Assert-CoverageFileParity $LcovPath

Write-Host "Coverage result (full/fair): $lineCoverage% lines"
if ($FailOnUnderTarget) {
  Write-Host "Coverage gate passed: $lineCoverage% >= $MinLineCoverage%"
}

#!/usr/bin/env pwsh
param(
  [string]$Toolchain = "1.88.0",
  [double]$MinLineCoverage = 0.0,
  [switch]$FailOnUnderTarget
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$IgnoreFilenameRegex = '(\.cargo|rustc|tests[/\\])'
$LcovPath = Join-Path $repoRoot "target/coverage-full.info"

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

  $lines = Get-Content $Path
  foreach ($line in $lines) {
    $trimmed = $line.Trim()
    if (
      $trimmed -eq "" -or
      $trimmed.StartsWith("//") -or
      $trimmed -match '^#!\[.*\]$' -or
      $trimmed -match '^#\[.*\]$' -or
      $trimmed -match '^(pub\s+)?mod\s+[A-Za-z0-9_]+;$' -or
      $trimmed -match '^(pub\s+)?use\s+.+;$'
    ) {
      continue
    }
    return $false
  }
  return $true
}

function Assert-CoverageFileParity {
  param([string]$LcovFile)

  $sourceFiles = rg --files daemon/src -g "*.rs" | ForEach-Object { (Resolve-Path $_).Path }
  $coveredFiles = rg '^SF:' $LcovFile | ForEach-Object { $_.Substring(3) }

  $coveredSet = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
  $coveredFiles | ForEach-Object { [void]$coveredSet.Add($_) }

  $missingActionable = New-Object System.Collections.Generic.List[string]
  $missingDeclarationOnly = New-Object System.Collections.Generic.List[string]

  foreach ($source in $sourceFiles) {
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

# Coverage tooling on the default 1.86 toolchain is unreliable on Windows.
# Use a newer toolchain explicitly for stable llvm-cov output.
Invoke-Step { rustup toolchain install $Toolchain } "rustup toolchain install"
Invoke-Step { cargo +$Toolchain install cargo-llvm-cov } "cargo-llvm-cov install"
$coverageOutput = & cargo +$Toolchain llvm-cov --workspace --summary-only --ignore-filename-regex $IgnoreFilenameRegex
if ($LASTEXITCODE -ne 0) {
  throw "cargo llvm-cov failed with exit code $LASTEXITCODE"
}

$coverageOutput | ForEach-Object { Write-Host $_ }
$lineCoverage = Get-LineCoverageFromSummary $coverageOutput
if ($FailOnUnderTarget -and $lineCoverage -lt $MinLineCoverage) {
  throw "line coverage $lineCoverage% is below required $MinLineCoverage%"
}

Invoke-Step {
  cargo +$Toolchain llvm-cov --workspace --ignore-filename-regex $IgnoreFilenameRegex --lcov --output-path $LcovPath
} "cargo llvm-cov lcov"
Assert-CoverageFileParity $LcovPath

Write-Host "Coverage result (full/fair): $lineCoverage% lines"
if ($FailOnUnderTarget) {
  Write-Host "Coverage gate passed: $lineCoverage% >= $MinLineCoverage%"
}

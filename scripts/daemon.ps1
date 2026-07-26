#!/usr/bin/env pwsh
param(
  [ValidateSet("run", "health", "list", "trigger", "reload", "verify", "shutdown", "ui-open")]
  [string]$Action = "run",
  [string]$BindAddr = "127.0.0.1:4765",
  [string]$ExtensionsDir = "./extensions",
  [int]$ReloadIntervalMs = 3000,
  [string]$ExtensionId = "",
  [string]$ActionId = "",
  [int]$UiIdleTimeoutMs = 300000
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

function Invoke-Copper {
  param([string[]]$Arguments, [string]$Description, [switch]$Rebuild)
  $binaryName = if ($IsWindows) { "copper.exe" } else { "copper" }
  $binaryPath = Join-Path $repoRoot "target/debug/$binaryName"
  if ($Rebuild -or -not (Test-Path -LiteralPath $binaryPath)) {
    Invoke-Step { cargo build -p copperd } "build copperd"
  }
  Invoke-Step { & $binaryPath @Arguments } $Description
}

switch ($Action) {
  "run" {
    Invoke-Copper -Arguments @(
      "run",
      "--extensions-dir", $ExtensionsDir,
      "--bind-addr", $BindAddr,
      "--reload-interval-ms", $ReloadIntervalMs
    ) -Description "run" -Rebuild
  }
  "health" {
    Invoke-Copper -Arguments @("daemon", "health", "--bind-addr", $BindAddr) -Description "daemon health"
  }
  "list" {
    Invoke-Copper -Arguments @("daemon", "list", "--bind-addr", $BindAddr) -Description "daemon list"
  }
  "trigger" {
    if ([string]::IsNullOrWhiteSpace($ExtensionId)) {
      throw "ExtensionId is required for trigger"
    }
    if ([string]::IsNullOrWhiteSpace($ActionId)) {
      Invoke-Copper -Arguments @(
        "daemon", "trigger", $ExtensionId,
        "--bind-addr", $BindAddr
      ) -Description "daemon trigger"
    } else {
      Invoke-Copper -Arguments @(
        "daemon", "trigger", $ExtensionId,
        "--action", $ActionId,
        "--bind-addr", $BindAddr
      ) -Description "daemon trigger"
    }
  }
  "reload" {
    Invoke-Copper -Arguments @("daemon", "reload", "--bind-addr", $BindAddr) -Description "daemon reload"
  }
  "verify" {
    Invoke-Copper -Arguments @("daemon", "verify", "--bind-addr", $BindAddr) -Description "daemon verify"
  }
  "shutdown" {
    Invoke-Copper -Arguments @("daemon", "shutdown", "--bind-addr", $BindAddr) -Description "daemon shutdown"
  }
  "ui-open" {
    if ([string]::IsNullOrWhiteSpace($ExtensionId)) {
      throw "ExtensionId is required for ui-open"
    }
    Invoke-Copper -Arguments @(
      "ui", "open",
      "--extension", $ExtensionId,
      "--extensions-dir", $ExtensionsDir,
      "--idle-timeout-ms", $UiIdleTimeoutMs
    ) -Description "ui open"
  }
}

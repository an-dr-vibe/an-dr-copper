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

switch ($Action) {
  "run" {
    Invoke-Step {
      cargo run -p copperd -- run --extensions-dir $ExtensionsDir --bind-addr $BindAddr --reload-interval-ms $ReloadIntervalMs
    } "run"
  }
  "health" {
    Invoke-Step { cargo run -p copperd -- daemon health --bind-addr $BindAddr } "daemon health"
  }
  "list" {
    Invoke-Step { cargo run -p copperd -- daemon list --bind-addr $BindAddr } "daemon list"
  }
  "trigger" {
    if ([string]::IsNullOrWhiteSpace($ExtensionId)) {
      throw "ExtensionId is required for trigger"
    }
    if ([string]::IsNullOrWhiteSpace($ActionId)) {
      Invoke-Step { cargo run -p copperd -- daemon trigger $ExtensionId --bind-addr $BindAddr } "daemon trigger"
    } else {
      Invoke-Step { cargo run -p copperd -- daemon trigger $ExtensionId --action $ActionId --bind-addr $BindAddr } "daemon trigger"
    }
  }
  "reload" {
    Invoke-Step { cargo run -p copperd -- daemon reload --bind-addr $BindAddr } "daemon reload"
  }
  "verify" {
    Invoke-Step { cargo run -p copperd -- daemon verify --bind-addr $BindAddr } "daemon verify"
  }
  "shutdown" {
    Invoke-Step { cargo run -p copperd -- daemon shutdown --bind-addr $BindAddr } "daemon shutdown"
  }
  "ui-open" {
    if ([string]::IsNullOrWhiteSpace($ExtensionId)) {
      throw "ExtensionId is required for ui-open"
    }
    Invoke-Step {
      cargo run -p copperd -- ui open --extension $ExtensionId --extensions-dir $ExtensionsDir --idle-timeout-ms $UiIdleTimeoutMs
    } "ui open"
  }
}

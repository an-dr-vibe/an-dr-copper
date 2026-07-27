#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

$sdkWit = [IO.File]::ReadAllText((Join-Path $repoRoot "sdk/wit/core.wit")).Replace("`r`n", "`n")
$bonesWit = [IO.File]::ReadAllText((Join-Path $repoRoot "bones/wit/core.wit")).Replace("`r`n", "`n")
if ($sdkWit -ne $bonesWit) {
  throw "sdk/wit/core.wit drifted from the pinned Bones component contract"
}

Push-Location $repoRoot
try {
  cargo test --manifest-path sdk/rust/Cargo.toml
  if ($LASTEXITCODE -ne 0) {
    throw "Copper component SDK tests failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

$testRoot = Join-Path ([IO.Path]::GetTempPath()) "copper-sdk-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
  $generatedDir = Join-Path $testRoot "generated"
  & (Join-Path $PSScriptRoot "new-wasm-extension.ps1") `
    -Id "generated-sample" `
    -Name "Generated Sample" `
    -OutputDir $generatedDir
  $generatedManifest = Get-Content (Join-Path $generatedDir "manifest.json") -Raw | ConvertFrom-Json
  if (
    $generatedManifest.id -ne "generated-sample" -or
    $generatedManifest.runtime.artifact -ne "generated-sample.wasm" -or
    -not (Test-Path (Join-Path $generatedDir "component/Cargo.lock"))
  ) {
    throw "WASM extension scaffold does not match the requested identity"
  }

  $archiveOne = Join-Path $testRoot "one.zip"
  $archiveTwo = Join-Path $testRoot "two.zip"
  & (Join-Path $PSScriptRoot "build-wasm-extension.ps1") `
    -ExtensionDir $generatedDir `
    -Package `
    -PackageOutput $archiveOne `
    -SkipValidation
  $generatedArtifact = Join-Path $generatedDir "generated-sample.wasm"
  $componentHashOne = (Get-FileHash -Algorithm SHA256 -LiteralPath $generatedArtifact).Hash
  & (Join-Path $PSScriptRoot "build-wasm-extension.ps1") `
    -ExtensionDir $generatedDir `
    -SkipValidation
  $componentHashTwo = (Get-FileHash -Algorithm SHA256 -LiteralPath $generatedArtifact).Hash
  if ($componentHashOne -ne $componentHashTwo) {
    throw "WASM Component build output is not deterministic"
  }
  & (Join-Path $PSScriptRoot "package-extension.ps1") `
    -ExtensionDir $generatedDir -OutputPath $archiveTwo -SkipValidation

  $hashOne = (Get-FileHash -Algorithm SHA256 -LiteralPath $archiveOne).Hash
  $hashTwo = (Get-FileHash -Algorithm SHA256 -LiteralPath $archiveTwo).Hash
  if ($hashOne -ne $hashTwo) {
    throw "Extension package output is not deterministic"
  }

  Add-Type -AssemblyName System.IO.Compression
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [IO.Compression.ZipFile]::OpenRead($archiveOne)
  try {
    $entries = @($archive.Entries | Sort-Object FullName)
    $names = @($entries | ForEach-Object FullName)
    if (($names -join ",") -ne "generated-sample.wasm,manifest.json") {
      throw "Unexpected package entries: $($names -join ', ')"
    }
    if ($entries | Where-Object {
      $_.LastWriteTime.Year -ne 1980 -or
      $_.LastWriteTime.Month -ne 1 -or
      $_.LastWriteTime.Day -ne 1
    }) {
      throw "Package entries do not use the fixed reproducible timestamp"
    }
  } finally {
    $archive.Dispose()
  }

  $copperName = if ($IsWindows) { "copper.exe" } else { "copper" }
  $copperPath = Join-Path $repoRoot "target/debug/$copperName"
  if (Test-Path -LiteralPath $copperPath -PathType Leaf) {
    & $copperPath validate (Join-Path $generatedDir "manifest.json")
    if ($LASTEXITCODE -ne 0) {
      throw "Generated Component manifest/artifact validation failed"
    }
    & $copperPath verify --extensions-dir $testRoot
    if ($LASTEXITCODE -ne 0) {
      throw "Generated Component package verification failed"
    }

    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    $bindAddr = "127.0.0.1:$port"
    $daemonOut = Join-Path $testRoot "daemon.out.log"
    $daemonErr = Join-Path $testRoot "daemon.err.log"
    $previousTraySetting = $env:COPPERD_DISABLE_TRAY
    $env:COPPERD_DISABLE_TRAY = "1"
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $copperPath
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @(
      "daemon", "run",
      "--extensions-dir", $testRoot,
      "--bind-addr", $bindAddr,
      "--reload-interval-ms", "100"
    )) {
      $startInfo.ArgumentList.Add($argument)
    }
    $daemon = [Diagnostics.Process]::Start($startInfo)
    $daemonOutTask = $daemon.StandardOutput.ReadToEndAsync()
    $daemonErrTask = $daemon.StandardError.ReadToEndAsync()
    try {
      $healthy = $false
      for ($attempt = 1; $attempt -le 80; $attempt++) {
        $health = (& $copperPath daemon health --bind-addr $bindAddr 2>&1 | Out-String)
        if ($health -match "daemon alive") {
          $healthy = $true
          break
        }
        Start-Sleep -Milliseconds 100
      }
      if (-not $healthy) {
        throw "Generated Component smoke daemon did not become healthy"
      }
      $baseline = (($health -replace '(?s)^[^{]*', '') | ConvertFrom-Json)
      $actionsBefore = [uint64]$baseline.bones.actionsDispatched
      $capabilitiesBefore = [uint64]$baseline.bones.capabilityCompleted

      $trigger = (& $copperPath daemon trigger generated-sample --action run --bind-addr $bindAddr 2>&1 | Out-String)
      if ($trigger -notmatch "trigger prepared") {
        throw "Generated Component action was not accepted: $trigger"
      }

      $completed = $false
      for ($attempt = 1; $attempt -le 80; $attempt++) {
        $health = (& $copperPath daemon health --bind-addr $bindAddr 2>&1 | Out-String)
        $current = (($health -replace '(?s)^[^{]*', '') | ConvertFrom-Json)
        if (
          [uint64]$current.bones.actionsDispatched -ge ($actionsBefore + 1) -and
          [uint64]$current.bones.capabilityCompleted -ge ($capabilitiesBefore + 1)
        ) {
          $completed = $true
          break
        }
        Start-Sleep -Milliseconds 100
      }
      if (-not $completed) {
        throw "Generated Component capability job did not complete: $health"
      }

      & $copperPath daemon shutdown --bind-addr $bindAddr | Out-Null
      if ($LASTEXITCODE -ne 0) {
        throw "Generated Component smoke daemon rejected shutdown"
      }
      $daemon.WaitForExit(5000) | Out-Null
    } finally {
      $env:COPPERD_DISABLE_TRAY = $previousTraySetting
      if (-not $daemon.HasExited) {
        $daemon.Kill($true)
      }
      $daemon.WaitForExit(5000) | Out-Null
      $daemonOutText = $daemonOutTask.GetAwaiter().GetResult()
      $daemonErrText = $daemonErrTask.GetAwaiter().GetResult()
      $daemon.Dispose()
      [IO.File]::WriteAllText($daemonOut, $daemonOutText)
      [IO.File]::WriteAllText($daemonErr, $daemonErrText)
    }
  } else {
    Write-Host "Host smoke skipped: build target/debug/$copperName first."
  }
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $testRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) {
      throw "Refusing to remove SDK test directory outside the system temp root: $testRoot"
    }
    for ($attempt = 1; $attempt -le 20; $attempt++) {
      try {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
        break
      } catch {
        if ($attempt -eq 20) {
          throw
        }
        Start-Sleep -Milliseconds 100
      }
    }
  }
}

Write-Host "Copper WASM SDK and deterministic packaging tests passed."

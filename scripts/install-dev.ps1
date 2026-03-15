#!/usr/bin/env pwsh
param(
  [string]$RepoRoot = "",
  [string]$InstallDir = "",
  [switch]$Force,
  [switch]$AutoStart,
  [switch]$NoAutoStart
)

$ErrorActionPreference = "Stop"

if ($AutoStart -and $NoAutoStart) {
  throw "Use either -AutoStart or -NoAutoStart, not both."
}

function Get-DefaultLinkedInstallDir {
  if ($IsWindows) {
    if (-not $env:LOCALAPPDATA) {
      throw "LOCALAPPDATA is not set."
    }
    return (Join-Path $env:LOCALAPPDATA "Copper-linked")
  }

  $home = [Environment]::GetFolderPath("UserProfile")
  if ([string]::IsNullOrWhiteSpace($home)) {
    throw "Home directory is not available."
  }
  return (Join-Path $home ".local/share/copper-linked")
}

function Initialize-InstallDir {
  param(
    [string]$TargetInstallDir,
    [switch]$Overwrite
  )

  if (Test-Path $TargetInstallDir) {
    if (-not $Overwrite) {
      throw "Install directory already exists: $TargetInstallDir. Re-run with -Force to replace it."
    }
    Remove-Item -Recurse -Force $TargetInstallDir
  }
  New-Item -ItemType Directory -Path $TargetInstallDir -Force | Out-Null
}

function Write-Utf8File {
  param(
    [string]$Path,
    [string]$Content
  )

  $parent = Split-Path -Parent $Path
  if (-not [string]::IsNullOrWhiteSpace($parent)) {
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
  }
  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Resolve-PwshPath {
  $pwsh = Get-Command pwsh -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $pwsh) {
    throw "pwsh is required to create Copper launchers."
  }
  return $pwsh.Source
}

function Resolve-CargoPath {
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $cargo) {
    throw "cargo is required for linked development installs."
  }
  return $cargo.Source
}

function Get-StartScriptPath {
  param([string]$TargetInstallDir)
  return (Join-Path $TargetInstallDir "copper-start.ps1")
}

function Get-StartLauncherPath {
  param([string]$TargetInstallDir)

  if ($IsWindows) {
    return (Join-Path $TargetInstallDir "copper-start.cmd")
  }

  return (Join-Path $TargetInstallDir "copper-start")
}

function New-LinkedInstallLaunchers {
  param(
    [string]$TargetInstallDir,
    [string]$ResolvedRepoRoot
  )

  $pwshPath = Resolve-PwshPath
  $cargoPath = Resolve-CargoPath
  $startScriptPath = Get-StartScriptPath -TargetInstallDir $TargetInstallDir
  $launcherPath = Get-StartLauncherPath -TargetInstallDir $TargetInstallDir
  $escapedRepoRoot = $ResolvedRepoRoot.Replace("'", "''")
  $escapedCargoPath = $cargoPath.Replace("'", "''")

  $startScript = @"
#!/usr/bin/env pwsh
param(
  [Parameter(ValueFromRemainingArguments = `$true)]
  [string[]]`$RemainingArgs
)

`$ErrorActionPreference = "Stop"
`$repoRoot = '$escapedRepoRoot'
`$cargoPath = '$escapedCargoPath'

if (-not (Test-Path (Join-Path `$repoRoot 'Cargo.toml'))) {
  throw "Linked Copper repo not found: `$repoRoot"
}

Push-Location `$repoRoot
try {
  & `$cargoPath run -p copperd -- @RemainingArgs
  exit `$LASTEXITCODE
} finally {
  Pop-Location
}
"@
  Write-Utf8File -Path $startScriptPath -Content $startScript

  if ($IsWindows) {
    $normalizedPwsh = $pwshPath -replace "/", "\"
    $launcher = @"
@echo off
setlocal
"$normalizedPwsh" -NoProfile -ExecutionPolicy Bypass -File "%~dp0copper-start.ps1" %*
"@
    Write-Utf8File -Path $launcherPath -Content $launcher
  } else {
    $launcher = @"
#!/bin/sh
SCRIPT_DIR=`$(CDPATH= cd -- "`$(dirname -- "`$0")" && pwd)
exec "$pwshPath" -NoProfile -File "`$SCRIPT_DIR/copper-start.ps1" "`$@"
"@
    Write-Utf8File -Path $launcherPath -Content $launcher
    & chmod +x $launcherPath
    if ($LASTEXITCODE -ne 0) {
      throw "Failed to mark launcher executable: $launcherPath"
    }
  }
}

function Register-CopperAutostart {
  param(
    [string]$Name,
    [string]$DisplayName,
    [string]$TargetInstallDir
  )

  $commandPath = Get-StartLauncherPath -TargetInstallDir $TargetInstallDir
  if (-not (Test-Path $commandPath)) {
    throw "Autostart launcher not found: $commandPath"
  }

  if ($IsWindows) {
    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    New-Item -Path $runKey -Force | Out-Null
    Set-ItemProperty -Path $runKey -Name $Name -Value ('"{0}"' -f $commandPath)
    Write-Host "Registered autostart in Windows Run key: $Name"
    return
  }

  if ($IsLinux) {
    $autostartDir = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".config/autostart"
    New-Item -ItemType Directory -Path $autostartDir -Force | Out-Null
    $desktopPath = Join-Path $autostartDir "$Name.desktop"
    $escapedCommand = $commandPath.Replace("\", "\\").Replace('"', '\"')
    $desktopEntry = @"
[Desktop Entry]
Type=Application
Version=1.0
Name=$DisplayName
Exec="$escapedCommand"
Terminal=false
X-GNOME-Autostart-enabled=true
"@
    Write-Utf8File -Path $desktopPath -Content $desktopEntry
    Write-Host "Registered autostart desktop entry: $desktopPath"
    return
  }

  if ($IsMacOS) {
    $launchAgentsDir = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Library/LaunchAgents"
    $logsDir = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Library/Logs/Copper"
    New-Item -ItemType Directory -Path $launchAgentsDir -Force | Out-Null
    New-Item -ItemType Directory -Path $logsDir -Force | Out-Null
    $label = "dev.copper.$Name"
    $plistPath = Join-Path $launchAgentsDir "$label.plist"
    $stdoutPath = Join-Path $logsDir "$Name.stdout.log"
    $stderrPath = Join-Path $logsDir "$Name.stderr.log"
    $escapedCommand = [System.Security.SecurityElement]::Escape($commandPath)
    $escapedWorkingDir = [System.Security.SecurityElement]::Escape($TargetInstallDir)
    $escapedStdout = [System.Security.SecurityElement]::Escape($stdoutPath)
    $escapedStderr = [System.Security.SecurityElement]::Escape($stderrPath)
    $plist = @"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$escapedCommand</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>WorkingDirectory</key>
  <string>$escapedWorkingDir</string>
  <key>StandardOutPath</key>
  <string>$escapedStdout</string>
  <key>StandardErrorPath</key>
  <string>$escapedStderr</string>
</dict>
</plist>
"@
    Write-Utf8File -Path $plistPath -Content $plist
    Write-Host "Registered LaunchAgent: $plistPath"
    return
  }

  throw "Autostart is not supported on this OS."
}

function Unregister-CopperAutostart {
  param([string]$Name)

  if ($IsWindows) {
    $runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    if (Test-Path $runKey) {
      Remove-ItemProperty -Path $runKey -Name $Name -ErrorAction SilentlyContinue
    }
    Write-Host "Removed Windows autostart entry: $Name"
    return
  }

  if ($IsLinux) {
    $desktopPath = Join-Path ([Environment]::GetFolderPath("UserProfile")) ".config/autostart/$Name.desktop"
    if (Test-Path $desktopPath) {
      Remove-Item -Force $desktopPath
    }
    Write-Host "Removed Linux autostart entry: $desktopPath"
    return
  }

  if ($IsMacOS) {
    $plistPath = Join-Path ([Environment]::GetFolderPath("UserProfile")) "Library/LaunchAgents/dev.copper.$Name.plist"
    if (Test-Path $plistPath) {
      Remove-Item -Force $plistPath
    }
    Write-Host "Removed macOS LaunchAgent: $plistPath"
    return
  }

  throw "Autostart is not supported on this OS."
}

function Finalize-Install {
  param(
    [string]$TargetInstallDir,
    [string]$ResolvedRepoRoot
  )

  New-LinkedInstallLaunchers -TargetInstallDir $TargetInstallDir -ResolvedRepoRoot $ResolvedRepoRoot
  Write-Utf8File -Path (Join-Path $TargetInstallDir "repo-root.txt") -Content ($ResolvedRepoRoot + [Environment]::NewLine)

  if ($AutoStart) {
    Register-CopperAutostart -Name "CopperLinked" -DisplayName "Copper (Linked Dev)" -TargetInstallDir $TargetInstallDir
  } elseif ($NoAutoStart) {
    Unregister-CopperAutostart -Name "CopperLinked"
  }

  $launcherPath = Get-StartLauncherPath -TargetInstallDir $TargetInstallDir
  Write-Host "Installed Copper linked-development launcher to: $TargetInstallDir"
  Write-Host "Repo root: $ResolvedRepoRoot"
  Write-Host "Launcher: $launcherPath"
  if ($AutoStart) {
    Write-Host "Autostart: enabled for the next login."
  } elseif ($NoAutoStart) {
    Write-Host "Autostart: disabled."
  } else {
    Write-Host "Autostart: unchanged. Use -AutoStart to enable it."
  }
}

$resolvedRepoRoot = if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
  Resolve-Path (Join-Path $PSScriptRoot "..")
} else {
  Resolve-Path $RepoRoot
}
$resolvedRepoRoot = [string]$resolvedRepoRoot

if (-not (Test-Path (Join-Path $resolvedRepoRoot "Cargo.toml"))) {
  throw "Repo root must contain Cargo.toml: $resolvedRepoRoot"
}
if (-not (Test-Path (Join-Path $resolvedRepoRoot "extensions"))) {
  throw "Repo root must contain extensions/: $resolvedRepoRoot"
}

$resolvedInstallDir = if ([string]::IsNullOrWhiteSpace($InstallDir)) {
  Get-DefaultLinkedInstallDir
} else {
  $InstallDir
}

Initialize-InstallDir -TargetInstallDir $resolvedInstallDir -Overwrite:$Force
Finalize-Install -TargetInstallDir $resolvedInstallDir -ResolvedRepoRoot $resolvedRepoRoot

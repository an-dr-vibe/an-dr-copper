#!/usr/bin/env pwsh
param(
  [string]$Owner = "an-dr-vibe",
  [string]$Repo = "an-dr-copper",
  [string]$Version = "latest",
  [string]$InstallDir = "",
  [string]$AssetName = "",
  [switch]$Force,
  [bool]$BuildFromSourceFallback = $true,
  [string]$SourceRef = "main",
  [switch]$AutoStart,
  [switch]$NoAutoStart
)

$ErrorActionPreference = "Stop"

if ($AutoStart -and $NoAutoStart) {
  throw "Use either -AutoStart or -NoAutoStart, not both."
}

function Get-TargetTriple {
  $arch = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
  $archPart = switch ($arch) {
    "X64" { "x86_64" }
    "Arm64" { "aarch64" }
    "X86" { "i686" }
    default { throw "Unsupported CPU architecture: $arch" }
  }

  $osPart = if ($IsWindows) {
    "pc-windows-msvc"
  } elseif ($IsLinux) {
    "unknown-linux-gnu"
  } elseif ($IsMacOS) {
    "apple-darwin"
  } else {
    throw "Unsupported OS."
  }

  return "$archPart-$osPart"
}

function Get-DefaultInstallDir {
  if ($IsWindows) {
    if (-not $env:LOCALAPPDATA) {
      throw "LOCALAPPDATA is not set."
    }
    return (Join-Path $env:LOCALAPPDATA "Copper")
  }

  $home = [Environment]::GetFolderPath("UserProfile")
  if ([string]::IsNullOrWhiteSpace($home)) {
    throw "Home directory is not available."
  }
  return (Join-Path $home ".local/share/copper")
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

function New-CopiedInstallLaunchers {
  param([string]$TargetInstallDir)

  $pwshPath = Resolve-PwshPath
  $exeName = if ($IsWindows) { "copperd.exe" } else { "copperd" }
  $startScriptPath = Get-StartScriptPath -TargetInstallDir $TargetInstallDir
  $launcherPath = Get-StartLauncherPath -TargetInstallDir $TargetInstallDir

  $startScript = @"
#!/usr/bin/env pwsh
param(
  [Parameter(ValueFromRemainingArguments = `$true)]
  [string[]]`$RemainingArgs
)

`$ErrorActionPreference = "Stop"
`$installRoot = Split-Path -Parent `$MyInvocation.MyCommand.Path
`$binaryPath = Join-Path `$installRoot "$exeName"
if (-not (Test-Path `$binaryPath)) {
  throw "Installed Copper binary not found: `$binaryPath"
}

& `$binaryPath @RemainingArgs
exit `$LASTEXITCODE
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

function Get-AutostartCommandPath {
  param([string]$TargetInstallDir)
  return (Get-StartLauncherPath -TargetInstallDir $TargetInstallDir)
}

function Register-CopperAutostart {
  param(
    [string]$Name,
    [string]$DisplayName,
    [string]$TargetInstallDir
  )

  $commandPath = Get-AutostartCommandPath -TargetInstallDir $TargetInstallDir
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
    [string]$DisplayName,
    [string]$AutoStartName
  )

  $exeName = if ($IsWindows) { "copperd.exe" } else { "copperd" }
  $installedBinary = Join-Path $TargetInstallDir $exeName
  $guiExeName = if ($IsWindows) { "copper.exe" } else { "copper" }
  $guiBinary = Join-Path $TargetInstallDir $guiExeName
  if (-not (Test-Path $installedBinary)) {
    throw "Install failed: binary missing at $installedBinary"
  }

  $extensionsPath = Join-Path $TargetInstallDir "extensions"
  if (-not (Test-Path $extensionsPath)) {
    throw "Install failed: required 'extensions' directory missing."
  }

  if (-not $IsWindows) {
    & chmod +x $installedBinary
    if ($LASTEXITCODE -ne 0) {
      throw "Failed to mark binary executable: $installedBinary"
    }
  }

  New-CopiedInstallLaunchers -TargetInstallDir $TargetInstallDir

  if ($AutoStart) {
    Register-CopperAutostart -Name $AutoStartName -DisplayName $DisplayName -TargetInstallDir $TargetInstallDir
  } elseif ($NoAutoStart) {
    Unregister-CopperAutostart -Name $AutoStartName
  }

  $launcherPath = Get-StartLauncherPath -TargetInstallDir $TargetInstallDir
  Write-Host "Installed $DisplayName to: $TargetInstallDir"
  Write-Host "Binary: $installedBinary"
  if (Test-Path $guiBinary) {
    Write-Host "Double-click launcher: $guiBinary"
  }
  Write-Host "Launcher: $launcherPath"
  if ($AutoStart) {
    Write-Host "Autostart: enabled for the next login."
  } elseif ($NoAutoStart) {
    Write-Host "Autostart: disabled."
  } else {
    Write-Host "Autostart: unchanged. Use -AutoStart to enable it."
  }
}

function Get-ReleaseMetadata {
  param(
    [string]$OwnerName,
    [string]$RepoName,
    [string]$RequestedVersion
  )

  $apiUrl = if ($RequestedVersion -eq "latest") {
    "https://api.github.com/repos/$OwnerName/$RepoName/releases/latest"
  } else {
    "https://api.github.com/repos/$OwnerName/$RepoName/releases/tags/$RequestedVersion"
  }

  $headers = @{
    "User-Agent" = "copper-installer"
    "Accept" = "application/vnd.github+json"
  }

  try {
    return Invoke-RestMethod -Uri $apiUrl -Headers $headers -Method Get
  } catch {
    throw "Failed to query release metadata from $apiUrl. $_"
  }
}

function Install-FromReleaseAsset {
  param(
    [object]$Release,
    [string]$SelectedAssetName,
    [string]$TargetInstallDir,
    [switch]$Overwrite
  )

  $asset = $Release.assets | Where-Object { $_.name -eq $SelectedAssetName } | Select-Object -First 1
  if (-not $asset) {
    $available = ($Release.assets | ForEach-Object { $_.name } | Sort-Object) -join ", "
    throw "Asset '$SelectedAssetName' was not found in release '$($Release.tag_name)'. Available assets: $available"
  }

  $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("copper-install-release-" + [Guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
  $zipPath = Join-Path $tempRoot $asset.name
  $extractDir = Join-Path $tempRoot "extract"
  New-Item -ItemType Directory -Path $extractDir -Force | Out-Null

  Write-Host "Downloading $($asset.name) from release $($Release.tag_name)..."
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing
  Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

  $exeName = if ($IsWindows) { "copperd.exe" } else { "copperd" }
  $binary = Get-ChildItem -Path $extractDir -Recurse -File -Filter $exeName | Select-Object -First 1
  if (-not $binary) {
    throw "Downloaded archive does not contain $exeName."
  }

  $bundleRoot = Split-Path -Path $binary.FullName -Parent
  Initialize-InstallDir -TargetInstallDir $TargetInstallDir -Overwrite:$Overwrite
  Copy-Item -Path (Join-Path $bundleRoot "*") -Destination $TargetInstallDir -Recurse -Force
}

function Resolve-SourceArchiveUrl {
  param(
    [string]$OwnerName,
    [string]$RepoName,
    [string]$RequestedVersion,
    [string]$RequestedSourceRef
  )

  if ($RequestedVersion -ne "latest") {
    return "https://github.com/$OwnerName/$RepoName/archive/refs/tags/$RequestedVersion.zip"
  }
  return "https://github.com/$OwnerName/$RepoName/archive/refs/heads/$RequestedSourceRef.zip"
}

function Install-FromSourceArchive {
  param(
    [string]$OwnerName,
    [string]$RepoName,
    [string]$RequestedVersion,
    [string]$RequestedSourceRef,
    [string]$TargetInstallDir,
    [switch]$Overwrite
  )

  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo is required for source fallback install. Install Rust from https://rustup.rs"
  }

  $archiveUrl = Resolve-SourceArchiveUrl -OwnerName $OwnerName -RepoName $RepoName -RequestedVersion $RequestedVersion -RequestedSourceRef $RequestedSourceRef
  $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("copper-install-source-" + [Guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
  $zipPath = Join-Path $tempRoot "source.zip"
  $extractDir = Join-Path $tempRoot "extract"
  New-Item -ItemType Directory -Path $extractDir -Force | Out-Null

  Write-Host "Downloading source archive: $archiveUrl"
  Invoke-WebRequest -Uri $archiveUrl -OutFile $zipPath -UseBasicParsing
  Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

  $sourceRoot = Get-ChildItem -Path $extractDir -Directory | Select-Object -First 1
  if (-not $sourceRoot) {
    throw "Source archive extraction failed."
  }

  Write-Host "Building copperd from source..."
  cargo build --release --manifest-path (Join-Path $sourceRoot.FullName "Cargo.toml") -p copperd
  if ($LASTEXITCODE -ne 0) {
    throw "Source build failed with exit code $LASTEXITCODE"
  }

  $exeName = if ($IsWindows) { "copperd.exe" } else { "copperd" }
  $builtBinary = Join-Path $sourceRoot.FullName "target/release/$exeName"
  $guiExeName = if ($IsWindows) { "copper.exe" } else { "copper" }
  $guiBinary = Join-Path $sourceRoot.FullName "target/release/$guiExeName"
  if (-not (Test-Path $builtBinary)) {
    throw "Source build completed but binary is missing: $builtBinary"
  }

  $sourceExtensions = Join-Path $sourceRoot.FullName "extensions"
  if (-not (Test-Path $sourceExtensions)) {
    throw "Source archive is missing extensions directory."
  }

  Initialize-InstallDir -TargetInstallDir $TargetInstallDir -Overwrite:$Overwrite
  Copy-Item -Path $builtBinary -Destination (Join-Path $TargetInstallDir $exeName) -Force
  if (Test-Path $guiBinary) {
    Copy-Item -Path $guiBinary -Destination (Join-Path $TargetInstallDir $guiExeName) -Force
  }
  Copy-Item -Path $sourceExtensions -Destination (Join-Path $TargetInstallDir "extensions") -Recurse -Force

  $readme = Join-Path $sourceRoot.FullName "README.md"
  if (Test-Path $readme) {
    Copy-Item -Path $readme -Destination (Join-Path $TargetInstallDir "README.md") -Force
  }
  $quickstart = Join-Path $sourceRoot.FullName "docs/QUICKSTART.md"
  if (Test-Path $quickstart) {
    Copy-Item -Path $quickstart -Destination (Join-Path $TargetInstallDir "QUICKSTART.md") -Force
  }
}

$resolvedInstallDir = if ([string]::IsNullOrWhiteSpace($InstallDir)) {
  Get-DefaultInstallDir
} else {
  $InstallDir
}

$triple = Get-TargetTriple
$resolvedAssetName = if ([string]::IsNullOrWhiteSpace($AssetName)) {
  "copper-$triple.zip"
} else {
  $AssetName
}

$release = $null
$releaseError = $null
try {
  $release = Get-ReleaseMetadata -OwnerName $Owner -RepoName $Repo -RequestedVersion $Version
} catch {
  $releaseError = $_.Exception.Message
}

if ($release) {
  try {
    Install-FromReleaseAsset -Release $release -SelectedAssetName $resolvedAssetName -TargetInstallDir $resolvedInstallDir -Overwrite:$Force
    Finalize-Install -TargetInstallDir $resolvedInstallDir -DisplayName "Copper" -AutoStartName "Copper"
    exit 0
  } catch {
    if (-not $BuildFromSourceFallback) {
      throw
    }
    Write-Warning "Release install failed: $($_.Exception.Message)"
    Write-Warning "Falling back to source install..."
  }
} else {
  if (-not $BuildFromSourceFallback) {
    throw "Release lookup failed and source fallback is disabled. $releaseError"
  }
  Write-Warning "Release lookup failed: $releaseError"
  Write-Warning "Falling back to source install..."
}

Install-FromSourceArchive -OwnerName $Owner -RepoName $Repo -RequestedVersion $Version -RequestedSourceRef $SourceRef -TargetInstallDir $resolvedInstallDir -Overwrite:$Force
Finalize-Install -TargetInstallDir $resolvedInstallDir -DisplayName "Copper" -AutoStartName "Copper"

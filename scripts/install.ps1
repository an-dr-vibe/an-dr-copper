#!/usr/bin/env pwsh
param(
  [string]$Owner = "an-dr-vibe",
  [string]$Repo = "an-dr-copper",
  [string]$Version = "latest",
  [string]$InstallDir = "",
  [string]$AssetName = "",
  [switch]$Force,
  [bool]$BuildFromSourceFallback = $true,
  [string]$SourceRef = "main"
)

$ErrorActionPreference = "Stop"

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

function Finalize-Install {
  param(
    [string]$TargetInstallDir
  )

  $exeName = if ($IsWindows) { "copperd.exe" } else { "copperd" }
  $installedBinary = Join-Path $TargetInstallDir $exeName
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

  Write-Host "Installed Copper to: $TargetInstallDir"
  Write-Host "Binary: $installedBinary"
  if ($IsWindows) {
    Write-Host "Run with: `"$installedBinary`""
    Write-Host "Optional: add '$TargetInstallDir' to PATH."
  } else {
    Write-Host "Run with: $installedBinary"
    Write-Host "Optional: add '$TargetInstallDir' to PATH."
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
  Finalize-Install -TargetInstallDir $TargetInstallDir
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
  if (-not (Test-Path $builtBinary)) {
    throw "Source build completed but binary is missing: $builtBinary"
  }

  $sourceExtensions = Join-Path $sourceRoot.FullName "extensions"
  if (-not (Test-Path $sourceExtensions)) {
    throw "Source archive is missing extensions directory."
  }

  Initialize-InstallDir -TargetInstallDir $TargetInstallDir -Overwrite:$Overwrite
  Copy-Item -Path $builtBinary -Destination (Join-Path $TargetInstallDir $exeName) -Force
  Copy-Item -Path $sourceExtensions -Destination (Join-Path $TargetInstallDir "extensions") -Recurse -Force

  $readme = Join-Path $sourceRoot.FullName "README.md"
  if (Test-Path $readme) {
    Copy-Item -Path $readme -Destination (Join-Path $TargetInstallDir "README.md") -Force
  }
  $quickstart = Join-Path $sourceRoot.FullName "docs/QUICKSTART.md"
  if (Test-Path $quickstart) {
    Copy-Item -Path $quickstart -Destination (Join-Path $TargetInstallDir "QUICKSTART.md") -Force
  }

  Finalize-Install -TargetInstallDir $TargetInstallDir
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

# Quickstart

## 0. Install Release (Optional)

```powershell
# one command: install + run daemon
pwsh -NoProfile -Command "$s=Invoke-RestMethod 'https://raw.githubusercontent.com/an-dr-vibe/an-dr-copper/main/scripts/install.ps1'; & ([ScriptBlock]::Create($s)) -Force; $dir=if($IsWindows){Join-Path $env:LOCALAPPDATA 'Copper'}else{Join-Path ([Environment]::GetFolderPath('UserProfile')) '.local/share/copper'}; $exe=if($IsWindows){'copperd.exe'}else{'copperd'}; & (Join-Path $dir $exe)"

./scripts/install.ps1
```

## 1. Bootstrap (PowerShell 7+)

```powershell
./scripts/bootstrap.ps1
```

## 2. Run Tests + Verification

```powershell
./scripts/verify-loop.ps1 -Iterations 5
./scripts/run-tests.ps1
./scripts/coverage.ps1
```

## 3. Run Daemon

```powershell
./scripts/daemon.ps1 -Action run
# or directly:
# ./target/release/copperd
# .\target\release\copperd.exe
# in another terminal:
./scripts/daemon.ps1 -Action health
./scripts/daemon.ps1 -Action list
# config UI is always available while daemon runs:
# http://127.0.0.1:4766
./target/release/copperd.exe ui open --extension desktop-torrent-organizer
./scripts/daemon.ps1 -Action shutdown
```

## 4. Build

```powershell
./scripts/build-debug.ps1
./scripts/build-release.ps1
```

Release output is written to `dist/release` and includes:

- Full daemon bundle (`copper-<host-triple>/`)
- Bundle archive (`copper-<host-triple>.zip`)
- Shipped core extensions (`extensions/`)
- Published extension archives (`extensions-published/`)

## 5. Validate Extensions

```powershell
cargo run -p copperd -- verify --extensions-dir ./extensions
cargo run -p copperd -- trigger session-counter --extensions-dir ./extensions
cargo run -p copperd -- trigger desktop-torrent-organizer --action move-torrents --extensions-dir ./extensions
```

## 6. Generate main.ts from manifest

```powershell
cargo run -p copperd -- generate-main ./extensions/sort-downloads/manifest.json
```


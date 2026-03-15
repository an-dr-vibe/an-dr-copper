# Quickstart

## 0. Install Release (Optional)

```powershell
# one command: copy install the released build
pwsh -NoProfile -Command "$s=Invoke-RestMethod 'https://raw.githubusercontent.com/an-dr-vibe/an-dr-copper/main/scripts/install.ps1'; & ([ScriptBlock]::Create($s)) -Force"

# copy install from this repo
./scripts/install.ps1

# copy install and register autostart
./scripts/install.ps1 -Force -AutoStart

# linked development install from this repo
./scripts/install-dev.ps1 -Force

# linked development install with autostart
./scripts/install-dev.ps1 -Force -AutoStart
```

`install.ps1` copies Copper into the install directory. `install-dev.ps1` keeps launchers pointed at the repo so code and bundled extensions stay live during development.
Once the daemon is running, you can also turn login startup on or off in the Copper UI under **Core -> Launch Copper at login**.
On Windows, installed bundles also include `copper.exe` as the no-terminal double-click launcher.

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
# Windows-only: `windows-display-manager` registers an additional tray icon.
# Left click toggles taskbar auto-hide. Right click opens resolution/scale/settings/exit menu.
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
cargo run -p copperd -- daemon trigger windows-display-manager --action status --bind-addr 127.0.0.1:4765
cargo run -p copperd -- daemon trigger windows-display-manager --action set-resolution --bind-addr 127.0.0.1:4765
```

`windows-display-manager` is a Windows-only host extension. On non-Windows hosts, trigger execution returns a platform support error.

## 6. Generate main.ts from manifest

```powershell
cargo run -p copperd -- generate-main ./extensions/sort-downloads/manifest.json
```


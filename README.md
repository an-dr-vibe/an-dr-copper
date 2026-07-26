# Copper

Copper is a cross-platform, manifest-first automation host focused on
AI-generated extensions.

Current status: the daemon, authenticated control plane, settings UI, extension
discovery, TypeScript execution, native host capabilities, tray integration,
and extension generation are implemented. Copper executes `main.ts` through a
Deno subprocess and a host JSON-RPC bridge while the
[Bones migration](docs/BONES_MIGRATION_PLAN.md) replaces that runtime with WASM
Components.

## Requirements

- Rust toolchain (rustup, cargo, rustc)
- Deno (required to execute current `main.ts` extension actions)

## Quick Start (Cross-Platform PowerShell)

All `.ps1` scripts are written for PowerShell 7+ (`pwsh`) and run on Windows/macOS/Linux.

```powershell
./scripts/bootstrap.ps1
./scripts/daemon.ps1 -Action run
./scripts/daemon.ps1 -Action health
./scripts/daemon.ps1 -Action list
# daemon also hosts config UI at:
# http://127.0.0.1:4766
# Windows: left click the main Copper tray icon to open the UI.
# extension settings: ~/.Copper/extensions/<extension-id>/config.json
# extension status:   ~/.Copper/extensions/<extension-id>/status.json
./scripts/daemon.ps1 -Action shutdown
.\target\release\copperd.exe ui open --extension desktop-torrent-organizer
./scripts/run-tests.ps1
./scripts/coverage.ps1
./scripts/build-debug.ps1
./scripts/build-release.ps1
```

## Install Copper (Cross-Platform PowerShell)

```powershell
# released/copy install from GitHub:
pwsh -NoProfile -Command "$s=Invoke-RestMethod 'https://raw.githubusercontent.com/an-dr-vibe/an-dr-copper/main/scripts/install.ps1'; & ([ScriptBlock]::Create($s)) -Force"

# released/copy install from cloned repo:
./scripts/install.ps1

# released/copy install with autostart:
./scripts/install.ps1 -Force -AutoStart

# released/copy install of a specific release tag:
./scripts/install.ps1 -Version v0.3.1

# linked development install from a cloned repo:
./scripts/install-dev.ps1 -Force

# linked development install with autostart:
./scripts/install-dev.ps1 -Force -AutoStart
```

Installer modes:
- `./scripts/install.ps1`: copies a released or locally-built Copper bundle into the install directory.
- `./scripts/install-dev.ps1`: installs only launchers and keeps execution rooted in the repo for simpler development.
- Both installers create a `copper-start` launcher in the install directory.
- On Windows, release/source installs also include `copper.exe` as the no-terminal double-click launcher.
- `-AutoStart` registers the launcher for the next login. `-NoAutoStart` removes that registration.
- While the daemon is running, the same login-start preference can also be toggled in the Copper UI on the **Core** settings page.

Copy installer behavior:
- Uses GitHub release asset `copper-<target-triple>.zip` when available.
- Falls back to source download + local release build when no release asset exists (requires `cargo`).

## CLI

```powershell
cargo run -p copperd -- doctor
cargo run -p copperd -- validate extensions/sort-downloads/manifest.json
cargo run -p copperd -- list --extensions-dir ./extensions
cargo run -p copperd -- verify --extensions-dir ./extensions
cargo run -p copperd -- trigger sort-downloads --extensions-dir ./extensions
cargo run -p copperd -- trigger session-counter --extensions-dir ./extensions
cargo run -p copperd -- trigger desktop-torrent-organizer --action move-torrents --extensions-dir ./extensions
cargo run -p copperd -- daemon trigger windows-display-manager --action status --bind-addr 127.0.0.1:4765
cargo run -p copperd -- daemon trigger windows-display-manager --action toggle-taskbar-autohide --bind-addr 127.0.0.1:4765
cargo run -p copperd -- ui open --extension desktop-torrent-organizer --extensions-dir ./extensions
# opens a native Tauri settings window by default:
cargo run -p copperd -- ui open --extension desktop-torrent-organizer --extensions-dir ./extensions
# browser fallback:
cargo run -p copperd -- ui open --extension desktop-torrent-organizer --extensions-dir ./extensions --browser
cargo run -p copperd -- generate-main extensions/sort-downloads/manifest.json
cargo run -p copperd -- run
cargo run -p copperd -- daemon health --bind-addr 127.0.0.1:4765
# daemon-hosted settings UI:
# http://127.0.0.1:4766
cargo run -p copperd -- daemon shutdown --bind-addr 127.0.0.1:4765
```

## Folder Map

- `daemon/` Rust host implementation
- `schemas/` descriptor schema contract
- `sdk/` TypeScript API type definitions
- `extensions/` sample extension pack (`sort-downloads`, `session-counter`, `desktop-torrent-organizer`, `windows-display-manager`)
- `scripts/` cross-platform build and verification scripts
- `docs/` architecture and usage docs

## Release Artifacts

`./scripts/build-release.ps1` produces a publishable bundle in `dist/release`:

- `dist/release/copper-<host-triple>/` with `copperd`, docs, and `extensions/`
- `dist/release/copper-<host-triple>.zip` full release archive
- `dist/release/copper-<host-triple>/extensions-published/*` per-extension archives ready to publish

Runtime extension roots:

- Core extensions: executable-adjacent `extensions/` (shipped with release)
- User extensions: `~/.Copper/extensions` (user-installed/custom)
- Runtime activation respects optional manifest `platforms` restrictions (`windows`, `macos`, `linux`).
- Core settings can disable specific extensions through `~/.Copper/extensions/copper-core/config.json` via `disabledExtensions`.
- Legacy `~/.Copper/extensions/copper-core/data.json` is still read as a fallback during migration.

Windows host extension note:
- On Windows, left click the main Copper tray icon to open the daemon-hosted UI. Right click opens the main tray menu.
- `windows-display-manager` executes taskbar/resolution/scale actions through daemon host APIs.
- Its settings page can save and immediately apply the configured taskbar, resolution, and scale values.
- `windows-display-manager` also declares a tray icon through manifest `tray` metadata, which the daemon loads through its tray provider API.
- On Windows, that tray provider registers an additional tray icon with:
  - Left click: toggle taskbar auto-hide
  - Right click: taskbar/resolution/scale menu, settings, exit
- It is Windows-only; on macOS/Linux the extension can still be configured but trigger execution returns a platform error.

## Documentation

- `docs/ARCHITECTURE.md`
- `docs/BONES_MIGRATION_PLAN.md`
- `docs/QUICKSTART.md`
- `docs/AI_AUTHORING.md`
- `docs/EXTENSION_UI_ACCESS.md`
- `AGENTS.md`


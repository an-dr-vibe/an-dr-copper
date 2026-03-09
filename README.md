# Copper

Copper is a cross-platform, manifest-first automation host focused on AI-generated extensions.

Current status: implemented MVP for schema validation, extension discovery, dry-run triggering, and TypeScript skeleton generation.

## Requirements

- Rust toolchain (rustup, cargo, rustc)
- Optional: Deno (only for future runtime execution of `main.ts`)

## Quick Start (Cross-Platform PowerShell)

All `.ps1` scripts are written for PowerShell 7+ (`pwsh`) and run on Windows/macOS/Linux.

```powershell
./scripts/bootstrap.ps1
./scripts/daemon.ps1 -Action run
./scripts/daemon.ps1 -Action health
./scripts/daemon.ps1 -Action list
# daemon also hosts config UI at:
# http://127.0.0.1:4766
./scripts/daemon.ps1 -Action shutdown
.\target\release\copperd.exe ui open --extension desktop-torrent-organizer
./scripts/run-tests.ps1
./scripts/coverage.ps1
./scripts/build-debug.ps1
./scripts/build-release.ps1
```

## Install Released Build (Cross-Platform PowerShell)

```powershell
# copy/paste one command (install + run):
pwsh -NoProfile -Command "$s=Invoke-RestMethod 'https://raw.githubusercontent.com/an-dr-vibe/an-dr-copper/main/scripts/install.ps1'; & ([ScriptBlock]::Create($s)) -Force; $dir=if($IsWindows){Join-Path $env:LOCALAPPDATA 'Copper'}else{Join-Path ([Environment]::GetFolderPath('UserProfile')) '.local/share/copper'}; $exe=if($IsWindows){'copperd.exe'}else{'copperd'}; & (Join-Path $dir $exe)"

# from cloned repo:
./scripts/install.ps1

# install a specific release tag:
./scripts/install.ps1 -Version v0.1.0

# overwrite existing install:
./scripts/install.ps1 -Force
```

Installer behavior:
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

Windows host extension note:
- `windows-display-manager` executes taskbar/resolution/scale actions through daemon host APIs.
- It is Windows-only; on macOS/Linux the extension can still be configured but trigger execution returns a platform error.

## Documentation

- `docs/ARCHITECTURE.md`
- `docs/QUICKSTART.md`
- `docs/AI_AUTHORING.md`
- `docs/EXTENSION_UI_ACCESS.md`
- `AGENTS.md`


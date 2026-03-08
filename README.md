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

## Alternative Bash Flow

```bash
./scripts/bootstrap.sh
./scripts/daemon.sh run
./scripts/daemon.sh health
./scripts/daemon.sh ui-open 127.0.0.1:4765 ./extensions 3000 desktop-torrent-organizer
./scripts/daemon.sh shutdown
./scripts/verify-loop.sh 3
./scripts/build-debug.sh
./scripts/build-release.sh
```

## CLI

```bash
cargo run -p copperd -- doctor
cargo run -p copperd -- validate extensions/sort-downloads/manifest.json
cargo run -p copperd -- list --extensions-dir ./extensions
cargo run -p copperd -- verify --extensions-dir ./extensions
cargo run -p copperd -- trigger sort-downloads --extensions-dir ./extensions
cargo run -p copperd -- trigger session-counter --extensions-dir ./extensions
cargo run -p copperd -- trigger desktop-torrent-organizer --action move-torrents --extensions-dir ./extensions
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
- `extensions/` sample extension pack (`sort-downloads`, `session-counter`, `desktop-torrent-organizer`)
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

## Documentation

- `docs/ARCHITECTURE.md`
- `docs/QUICKSTART.md`
- `docs/AI_AUTHORING.md`
- `docs/EXTENSION_UI_ACCESS.md`
- `AGENTS.md`


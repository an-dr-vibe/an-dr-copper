# Copper

Copper is a cross-platform, descriptor-first automation host focused on AI-generated extensions.

Current status: implemented MVP for schema validation, extension discovery, dry-run triggering, and TypeScript skeleton generation.

## Requirements

- Rust toolchain (rustup, cargo, rustc)
- Optional: Deno (only for future runtime execution of `main.ts`)

## Quick Start (Cross-Platform PowerShell)

All `.ps1` scripts are written for PowerShell 7+ (`pwsh`) and run on Windows/macOS/Linux.

```powershell
./scripts/bootstrap.ps1
./scripts/run-tests.ps1
./scripts/coverage.ps1
./scripts/build-debug.ps1
./scripts/build-release.ps1
```

## Alternative Bash Flow

```bash
./scripts/bootstrap.sh
./scripts/verify-loop.sh 3
./scripts/build-debug.sh
```

## CLI

```bash
cargo run -p copperd -- doctor
cargo run -p copperd -- validate extensions/sort-downloads/descriptor.json
cargo run -p copperd -- list --extensions-dir ./extensions
cargo run -p copperd -- verify --extensions-dir ./extensions
cargo run -p copperd -- trigger sort-downloads --extensions-dir ./extensions
cargo run -p copperd -- generate-main extensions/sort-downloads/descriptor.json
```

## Folder Map

- `daemon/` Rust host implementation
- `schemas/` descriptor schema contract
- `sdk/` TypeScript API type definitions
- `extensions/` sample extension pack
- `scripts/` cross-platform build and verification scripts
- `docs/` architecture and usage docs

## Documentation

- `docs/ARCHITECTURE.md`
- `docs/QUICKSTART.md`
- `docs/AI_AUTHORING.md`
- `AGENTS.md`

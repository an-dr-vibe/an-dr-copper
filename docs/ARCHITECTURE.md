# Copper Architecture

Version: 0.2.0  
Last updated: 2026-03-08

## 1. Overview

Copper is a cross-platform desktop automation platform optimized for AI-authored TypeScript extensions.

The core runtime is a long-running Rust daemon. Extensions are descriptor-first (`descriptor.json`) with a minimal API contract and schema validation.

## 2. Process Model

Two-process target model (same intent as original architecture):

1. Rust daemon (`copperd run`) - always-on background process.
2. UI window (planned) - spawned on demand for extension UI rendering.

Current implementation status:

- Implemented: always-on daemon, extension registry loading, periodic hot-reload, IPC control plane, descriptor validation, trigger preparation, skeleton generation.
- Planned: embedded `deno_core` runtime execution, tray/hotkey integration, on-demand Tauri UI renderer.

## 3. Implemented Daemon Core

Daemon capabilities:

- Binds to TCP IPC endpoint (default `127.0.0.1:4765`).
- Loads extensions from directory (`~/.Copper/extensions` by default).
- Validates descriptors against versioned schema.
- Periodically reloads extension registry (hot-reload behavior).
- Handles IPC operations:
  - `health`
  - `list`
  - `trigger`
  - `reload`
  - `verify`
  - `shutdown`

This restores the daemon as the center of system lifecycle.

## 4. Extension Contract

Extension folder:

```text
<extension>/
|- descriptor.json
`- main.ts
```

Schema source:

- `schemas/extension/1.0.0/descriptor.schema.json`

Type contract for AI generation:

- `sdk/api.d.ts`

## 5. Repository Layout

```text
.
|- daemon/
|  |- src/
|  |  |- api/        # host-side API module stubs (fs/shell/ui/notify/store)
|  |  |- runtime/    # runtime adapter abstraction
|  |  |- tray.rs     # tray controller placeholder
|  |  |- daemon.rs   # long-running daemon + IPC
|  |  |- cli.rs      # CLI and daemon control commands
|  |  `- ...
|- schemas/
|- sdk/
|- extensions/
|- scripts/
`- docs/
```

## 6. CLI Surface

Local utility commands:

- `validate`
- `list`
- `verify`
- `trigger`
- `generate-main`
- `doctor`
- `run`

Daemon control commands:

- `daemon run`
- `daemon health`
- `daemon list`
- `daemon trigger`
- `daemon reload`
- `daemon verify`
- `daemon shutdown`

## 7. Cross-Platform Strategy

- Rust host binaries for Windows/macOS/Linux.
- PowerShell scripts as the default cross-platform scripting path (`pwsh`).
- Bash variants retained for shell-native environments.

## 8. Verification

Primary checks:

1. `./scripts/run-tests.ps1`
2. `./scripts/coverage.ps1`
3. `./scripts/build-release.ps1`

Loop:

```powershell
for ($i = 1; $i -le 3; $i++) {
  ./scripts/run-tests.ps1
  ./scripts/coverage.ps1
  ./scripts/build-release.ps1
}
```

Release packaging:

- `./scripts/build-release.ps1` builds `copperd`, creates `dist/release/copper-<host-triple>/`, and publishes per-extension archives in `extensions-published/`.

## 9. Known Gaps vs Full Target Architecture

- `deno_core` is not embedded yet (dry-run/runtime adapter layer is in place).
- On-demand Tauri renderer is not wired yet.
- Tray and global hotkey behavior are scaffolded but not functional.

These gaps are additive roadmap work and do not change the daemon-first core architecture.

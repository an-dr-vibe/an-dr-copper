# Copper Architecture

Version: 0.2.0  
Last updated: 2026-03-15

## 1. Overview

Copper is a cross-platform desktop automation platform optimized for AI-authored TypeScript extensions.

The core runtime is a long-running Rust daemon. Extensions are manifest-first (`manifest.json`) with a minimal API contract and schema validation.

## 2. Process Model

Two-process target model (same intent as original architecture):

1. Rust daemon (`copperd run`) - always-on background process.
2. UI window (planned) - spawned on demand for extension UI rendering.

Current implementation status:

- Implemented: always-on daemon, extension registry loading, periodic hot-reload, IPC control plane, descriptor validation, trigger preparation, skeleton generation, local config UI (`ui open`), tray menu shortcut for desktop torrent extension config.
- Implemented: daemon-hosted always-on settings UI (`http://127.0.0.1:4766`) with Obsidian-style per-extension pages, separate Settings/Status views, and manifest-driven settings sections.
- Planned: embedded `deno_core` runtime execution, richer tray/hotkey integration, on-demand Tauri UI renderer.

## 3. Implemented Daemon Core

Daemon capabilities:

- Binds to TCP IPC endpoint (default `127.0.0.1:4765`).
- Loads extensions from merged roots:
  - executable-adjacent `extensions/`, parent `extensions/`, and workspace `extensions/` when present during local source runs (legacy `core-extensions/` still supported)
  - user directory `~/.Copper/extensions`
  - user extensions override same-id core extensions
- Validates extension manifests against versioned schema.
- Periodically reloads extension registry (hot-reload behavior).
- Runs background polling tasks for core extensions (currently `desktop-torrent-organizer` file polling).
- Executes host-native actions for `windows-display-manager` through daemon API bridges (taskbar auto-hide, display resolution, scale).
- Exposes manifest-driven additional tray icon API in daemon (`tray_extension`) so extensions can declare dedicated tray icons through descriptor metadata.
- Current implementation includes a `tray.provider = "windows-display"` host tray provider used by `windows-display-manager` for left-click toggle and right-click action menu behavior.
- Handles IPC operations:
  - `health`
  - `list`
  - `trigger`
  - `reload`
  - `verify`
  - `shutdown`
- Persists extension settings per extension in `~/.Copper/extensions/<extension-id>/config.json`.
- Persists runtime status per extension in `~/.Copper/extensions/<extension-id>/status.json`.
  - Legacy `data.json` is still read as a fallback during migration.
  - Includes action execution snapshots for host-native extensions (for example `windows-display-manager`).
- Config UI can save-and-apply host-native extension settings when the manifest declares `settings.applyActions`.

This restores the daemon as the center of system lifecycle.

## 4. Extension Contract

Extension folder:

```text
<extension>/
|- manifest.json
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
- `ui open`

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

- `./scripts/build-release.ps1` builds `copperd`, creates `dist/release/copper-<host-triple>/` with `extensions/`, and publishes per-extension archives in `extensions-published/`.

## 9. Known Gaps vs Full Target Architecture

- `deno_core` is not embedded yet (dry-run/runtime adapter layer is in place).
- On-demand Tauri renderer is not wired yet.
- Global hotkey behavior is not wired yet.

These gaps are additive roadmap work and do not change the daemon-first core architecture.


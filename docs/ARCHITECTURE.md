# Copper Architecture

Version: 0.3.2  
Last updated: 2026-05-31

## 1. Overview

Copper is a cross-platform desktop automation platform optimized for AI-authored TypeScript extensions.

The core runtime is a long-running Rust daemon. Extensions are manifest-first (`manifest.json`) with a minimal API contract and schema validation.

## 2. Process Model

Two-process target model (same intent as original architecture):

1. Rust daemon (`copperd run`) - always-on background process.
2. UI window (planned) - spawned on demand for extension UI rendering.

Current implementation status:

- Implemented: always-on daemon, extension registry loading, authenticated HTTP control plane, isolated runtime trigger preparation, scheduled reload/background polling, descriptor validation, skeleton generation, local config UI (`ui open`), and main tray icon UI launch on Windows.
- Implemented: daemon-hosted always-on settings UI (`http://127.0.0.1:4766`) with manifest-driven extension pages, optional manifest-defined tabs, and a core-managed extensions tab for enable/disable and command discovery.
- Implemented: Tauri-backed native window launcher for the settings UI; `ui open` and tray settings actions open native windows by default, with browser opening retained as an explicit fallback.
- Planned: embedded `deno_core` runtime execution, richer cross-platform tray/hotkey integration, on-demand Tauri UI renderer.

## 3. Implemented Daemon Core

Daemon capabilities:

- Binds to loopback HTTP control-plane endpoint (default `127.0.0.1:4765`).
- Requires a daemon-generated control-plane token for daemon IPC requests and daemon-hosted UI routes other than the initial HTML shell.
- Loads extensions from merged roots:
  - executable-adjacent `extensions/`, parent `extensions/`, and workspace `extensions/` when present during local source runs (legacy `core-extensions/` still supported)
  - user directory `~/.Copper/extensions`
  - user extensions override same-id core extensions
- Validates extension manifests against versioned schema.
- Runs reload cadence and host background polling through a dedicated `DaemonScheduler`.
- Filters runtime activation through manifest-declared host platforms and core config disable rules.
- Routes trigger preparation through a single `ExecutionEngine`, which combines the isolated runtime adapter, host capability registry, and shared state store.
- Uses a structured runtime ABI (`copper.runtime/1`) and executes trigger preparation through a subprocess runtime worker, so runtime planning is isolated from the daemon process.
- Routes daemon IPC request policy through a dedicated `DaemonControlService` so transport handling stays separate from registry/runtime/state orchestration.
- Routes config UI information and apply workflows through a dedicated `config_ui_service` layer so the HTTP/UI server stays thinner.
- Routes config UI HTTP parsing/serialization through `config_ui_http.rs` so UI transport concerns are separated from route/business logic.
- Splits oversized daemon/config UI/tray source files into multi-file modules and extracted test files so implementation details stay reviewable without mixing transport, rendering, platform code, and tests in one file.
- Runs host-native background tasks through `HostExtensionRegistry` capability specs instead of daemon-local extension ID branching.
- Executes host-native actions for built-in extensions through `HostExtensionRegistry` capability handlers with declared state contracts.
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
- Persists core daemon config in `~/.Copper/extensions/copper-core/config.json`.
- Uses atomic state-file replacement during writes and surfaces invalid JSON, non-object payloads, and legacy fallback usage through daemon health and config UI diagnostics.
  - `disabledExtensions` suppresses selected extensions from the active runtime while keeping them discoverable in the settings UI.
  - Legacy `data.json` is still read as a fallback during config migration.
  - Status files no longer fall back to legacy `data.json`; status is treated as its own state stream.
  - Host-managed status files now carry `_stateContract` metadata with capability ownership and schema version.
  - Includes action execution snapshots for host-native extensions (for example `windows-display-manager`).
- Uses a shared `ExtensionStateStore` for daemon, config UI, and tray persistence access.
- Config UI can save-and-apply host-native extension settings when the manifest declares `settings.applyActions`, and can surface host capability/state contract metadata for built-in extensions.

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

Runtime gating:

- Optional manifest field `platforms`: restricts runtime activation to `windows`, `macos`, and/or `linux`.
- Config UI still shows platform-restricted extensions so users can inspect settings on any host.

Type contract for AI generation:

- `sdk/api.d.ts`

## 5. Repository Layout

```text
.
|- daemon/
|  |- src/
|  |  |- api/        # host-side API modules (fs/shell/ui/notify/store/keyboard/secure_store)
|  |  |- runtime/    # runtime adapter abstraction
|  |  |- execution.rs        # shared trigger preparation and execution orchestration
|  |  |- daemon_scheduler.rs # reload/background scheduling policy
|  |  |- daemon_service.rs   # daemon control-plane service layer
|  |  |- daemon_transport.rs # daemon HTTP transport parsing/response mapping
|  |  |- config_ui_http.rs   # config UI HTTP parsing/serialization
|  |  |- config_ui_service.rs # config UI info/apply service layer
|  |  |- config_ui.rs        # config UI module root and shared state types
|  |  |- config_ui_server.rs # config UI request handling
|  |  |- config_ui_render.rs # config UI HTML shell composition
|  |  |- host_extensions.rs  # built-in host capability + state contract registry
|  |  |- control_plane.rs    # control-plane auth token lifecycle
|  |  |- state_store.rs      # shared config/status persistence service
|  |  |- tray.rs     # tray controller placeholder
|  |  |- tray_extension.rs # additional tray provider module root
|  |  |- daemon.rs   # long-running daemon lifecycle root
|  |  |- cli.rs      # CLI and daemon control commands
|  |  `- ...
|- daemon/tauri.conf.json # optional native settings window config
|- schemas/
|- sdk/
|- extensions/
|- scripts/
`- docs/
```

Module sizing guideline:

- Target roughly `300-600` lines for hand-maintained Rust source files.
- Treat `800+` lines as a refactor signal unless the file is mostly generated data or tightly scoped platform bindings.
- Split by responsibility first: service layer, transport, rendering/assets, platform-specific code, and tests should usually live in separate files.
- Test-heavy modules may use `include!`-backed test files when that keeps the production module readable without changing visibility or behavior.

## 6. Host API Surface

Each module lives in `daemon/src/api/<name>.rs` and has a matching entry in `sdk/api.d.ts`.
Adding a module requires 5 touch-points — see `agents/developer.md`.

| Module | Permission | Keyring backend | Status |
|---|---|---|---|
| `fs` | `fs` | — | stub |
| `shell` | `shell` | — | stub |
| `notify` | — | — | stub |
| `ui` | `ui` | — | stub |
| `store` | `store` | — | stub |
| `keyboard` | `keyboard` | — | partial: Windows `typeText` real |
| `secure_store` | `secure-store` | Windows Credential Manager / GNOME SecretService / macOS Keychain | **real** |

`secure_store` uses the `keyring` crate (`v3`, features `windows-native apple-native linux-native`).
Most other modules are stubs awaiting deeper runtime integration; `keyboard.typeText` uses host input on Windows.

## 8. CLI Surface

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

## 9. Cross-Platform Strategy

- Rust host binaries for Windows/macOS/Linux.
- PowerShell scripts as the default cross-platform scripting path (`pwsh`).
- Bash variants retained for shell-native environments.

## 10. Verification

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

## 11. Design Invariants

These must not be broken without a deliberate versioning decision:

| Invariant | Reason |
|---|---|
| Manifest is source of truth for all extension metadata | Schema validation is the only safety net for AI-authored extensions |
| Schema URL pinned in every manifest | Version drift breaks existing extensions silently |
| All state flows through `state_store` | No extension writes config/status files directly |
| HTTP control plane is loopback-only with per-session auth token | No remote attack surface |
| Trigger preparation runs in a subprocess, not in-process | Failures in runtime planning cannot crash the daemon |
| Cross-platform build must pass on Windows, macOS, and Linux | Platform-specific code goes behind `#[cfg]` or target sections in Cargo.toml |
| No mandatory GUI dependency in headless build path | CI must build without a display server |

## 12. Known Gaps vs Full Target Architecture

- `deno_core` is not embedded yet (the runtime boundary and ABI exist, but trigger preparation still uses a dry-run worker rather than executing TypeScript).
- On-demand Tauri renderer is not wired yet.
- Safe Input Key registers its saved hotkey through the daemon on Windows; richer cross-platform global hotkey behavior is still roadmap work.
- Some shipped extensions are still intentionally host-native or hybrid rather than purely TypeScript-executed; that ownership is now centralized in `host_extensions.rs` as explicit host capabilities.

These gaps are additive roadmap work and do not change the daemon-first core architecture.

# Copper Architecture

Version: 0.3.2  
Last updated: 2026-07-27

## 1. Overview

Copper is a cross-platform desktop automation platform optimized for
AI-authored extensions.

The current runtime is a long-running Rust daemon with an embedded Bones
engine. Extensions are manifest-first (`manifest.json`) with a minimal API
contract and schema validation. Every shipped extension runs as a WASM
Component; Deno-backed TypeScript remains available only for external
compatibility packages during the migration. See
`docs/BONES_MIGRATION_PLAN.md`.

## 2. Process Model

On-demand presentation model:

1. Rust daemon (`copperd run`) - always-on background process.
2. Detachable Bones web/Wry presentation - attached only while settings are open.

Current implementation status:

- Implemented: Copper embeds a step-driven, presentation-free Bones engine in
  the daemon, injects an external Copper control module through the public
  Bones module API, passes validated WASM Components from the filtered Copper
  registry into an explicit Bones startup catalog, and reports typed
  per-extension Bones lifecycle state through daemon health.
- Implemented: always-on daemon, extension registry loading, authenticated HTTP
  control plane, runtime-selected trigger dispatch, scheduled
  reload/background actions, descriptor validation, skeleton generation, local
  config UI (`ui open`), and main tray icon UI launch on Windows.
- Implemented: an on-demand Bones web/Wry settings presentation with
  manifest-driven extension pages, optional manifest-defined tabs, and a
  core-managed extensions tab for enable/disable and command discovery.
- Implemented: native UI requests and correlated responses use the versioned
  `copper.settings/1` protocol over owner-stamped Bones `web/*` messages. Main
  and extension tray actions attach the presentation to the live daemon bus;
  `ui open` uses the same presentation composition. An explicit
  `ui open --browser` fallback retains a temporary, authenticated loopback
  server without making UI HTTP part of the daemon lifecycle.
- Implemented: Component action execution through Bones plus temporary
  TypeScript action execution through an external Deno subprocess and the host
  JSON-RPC bridge in `sdk/bridge.ts`.
- Implemented: a Rust `copper.component/1` guest SDK with generated Bones WIT
  bindings, sender validation, asynchronous capability helpers, a scaffold,
  locked `wasm32-wasip2` builds, and deterministic extension archives.
- Implemented: all five shipped Component ports with live Bones-driver parity
  tests, committed reproducible artifacts, and no legacy entrypoints. Safe
  Input Key delegates keychain and keyboard work to native capabilities;
  Windows Display Manager delegates platform work while retaining native
  dynamic options, settings apply, and tray integration.
- Planned: richer cross-platform tray/hotkey integration.

## 3. Implemented Daemon Core

Daemon capabilities:

- Binds to loopback HTTP control-plane endpoint (default `127.0.0.1:4765`).
- Requires a daemon-generated control-plane token for daemon IPC requests.
- Loads extensions from merged roots:
  - executable-adjacent `extensions/`, parent `extensions/`, and workspace `extensions/` when present during local source runs (legacy `core-extensions/` still supported)
  - user directory `~/.Copper/extensions`
  - user extensions override same-id core extensions
- Validates extension manifests against versioned schema.
- Runs reload cadence and host background polling through a dedicated `DaemonScheduler`.
- Advances a headless Bones engine from the daemon event loop, maps
  disabled/platform policy to the Bones startup allow-list, observes catalog
  and lifecycle state, and performs orderly Bones shutdown with the daemon.
  Catalog topology changes build a candidate engine before cutover; component
  updates at a stable path use Bones' transactional supervisor reload.
- Registers the external `copper-capabilities` Bones module. Its synchronous
  validation boundary accepts only `copper.bus/1` capability requests from
  active WASM senders, checks the sender's manifest permission, rejects recent
  request replays, and returns a job ID without doing blocking work.
- Drains authorized jobs into a bounded native worker channel, polls completed
  work from the daemon loop, and delivers each result directly to the requesting
  extension endpoint. Store/config/status paths are derived exclusively from
  the stamped sender and remain under `ExtensionStateStore`. Per-sender queue
  limits keep one extension from consuming the shared worker boundary.
- Dispatches CLI and authenticated HTTP triggers for WASM extensions as direct,
  versioned `copper.bus/1` `action-request` messages from the fixed
  `copper-actions` endpoint. External legacy TypeScript extensions continue
  through the isolated subprocess adapter until the final cutover removes it.
- Schedules WASM background actions from optional manifest runtime metadata.
  Copper inspects each extension's scoped config at most once per second,
  applies the declared enable/interval keys, and records a run only after Bones
  accepts the action. Host-native polling is skipped for an extension as soon
  as its WASM schedule becomes authoritative.
- Filters runtime activation through manifest-declared host platforms and core config disable rules.
- Routes trigger preparation through a single `ExecutionEngine`, which combines the isolated runtime adapter, host capability registry, and shared state store.
- Uses a structured runtime ABI (`copper.runtime/1`) and executes trigger preparation through a subprocess runtime worker, so runtime planning is isolated from the daemon process.
- Executes the prepared TypeScript entrypoint in a separate Deno process. The
  process may read only its own entrypoint and the bridge checks manifest
  permissions before dispatching protected host API methods. The replacement
  Bones capability boundary retains and strengthens this policy.
- Routes daemon IPC request policy through a dedicated `DaemonControlService` so transport handling stays separate from registry/runtime/state orchestration.
- Routes config UI information and apply workflows through a dedicated
  `config_ui_service` layer shared by native Bones messages and browser
  fallback routes.
- Adapts the same settings routes to `copper.settings/1` in
  `config_ui_bones.rs`; `config_ui_http.rs` remains only for the explicit
  temporary browser fallback.
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
`- main.ts              # compatibility runtime when manifest.runtime is absent
```

or:

```text
<extension>/
|- manifest.json
`- <extension-id>.wasm  # runtime.kind = "wasm-component"
```

Schema source:

- `schemas/extension/1.0.0/descriptor.schema.json`

Runtime gating:

- Optional manifest field `platforms`: restricts runtime activation to `windows`, `macos`, and/or `linux`.
- Optional manifest object `runtime` selects a versioned WASM Component ABI and
  package-local artifact. Absence retains the legacy TypeScript contract.
- `runtime.background` may name one declared action plus optional scoped config
  keys for enablement and interval. `defaultIntervalSeconds` is required and
  bounded to 1–86,400 seconds.
- WASM artifacts are identity-bound to the manifest ID and cannot resolve
  outside their package.
- Config UI still shows platform-restricted extensions so users can inspect settings on any host.

Type contract for AI generation:

- Components: `sdk/COMPONENT_API.md`, `sdk/rust`, and `sdk/wit/core.wit`
- Temporary TypeScript compatibility: `sdk/api.d.ts`

## 5. Repository Layout

```text
.
|- daemon/
|  |- src/
|  |  |- api/        # host-side API modules (fs/shell/ui/notify/store/keyboard/secure_store)
|  |  |- bones_integration/ # external native modules registered with Bones
|  |  |- runtime/    # runtime adapter abstraction
|  |  |- execution.rs        # shared trigger preparation and execution orchestration
|  |  |- daemon_scheduler.rs # reload/background scheduling policy
|  |  |- daemon_service.rs   # daemon control-plane service layer
|  |  |- daemon_transport.rs # daemon HTTP transport parsing/response mapping
|  |  |- config_ui_http.rs   # config UI HTTP parsing/serialization
|  |  |- config_ui_bones.rs  # versioned Bones transport + Wry lifecycle
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
|- schemas/
|- sdk/
|  |- rust/             # copper.component/1 guest SDK
|  |- wit/              # pinned generated Bones guest contract
|  `- templates/        # Rust Component scaffold
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
| `fs` | `fs` | — | native worker: list/move/delete |
| `shell` | `shell` | — | native worker: direct executable/args and lookup |
| `notify` | — | — | native worker: platform notification |
| `ui` | `ui` | — | native worker route; presentation remains placeholder until M4 |
| `store` | `store` | — | native worker: scoped store/config/status |
| `keyboard` | `keyboard` | — | native worker: normalization and platform input |
| `secure_store` | `secure-store` | Windows Credential Manager / GNOME SecretService / macOS Keychain | native worker: get/set/delete |
| `windows_display` | `windows-display` | — | Windows-gated native worker |

`secure_store` uses the `keyring` crate (`v3`, features `windows-native apple-native linux-native`).
`keyboard.typeText` uses host input on Windows; the other input functions retain
their current platform behavior. The additive `windows-display` permission
separates display mutation from generic UI access. UI calls already cross the
authorized asynchronous boundary, but their placeholder host sink is replaced
by Bones web/wry presentation in M4.

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
| Legacy trigger preparation runs in a subprocess; component triggers use targeted Bones messages | Failures stay outside the daemon and one extension cannot receive another extension's action |
| Cross-platform build must pass on Windows, macOS, and Linux | Platform-specific code goes behind `#[cfg]` or target sections in Cargo.toml |
| No mandatory GUI dependency in headless build path | CI must build without a display server |
| Bones sender identity is the capability principal | Guest payloads cannot select or impersonate an extension identity |
| Native jobs never run in Bones handlers or frame steps | Filesystem, process, keychain, and platform latency cannot stall message dispatch |

## 12. Known Gaps vs Full Target Architecture

- Legacy TypeScript execution remains as compatibility scaffolding until the
  shipped extension ports are complete.
- The explicit browser fallback still uses the temporary authenticated UI
  server; the default native path uses only Bones messages.
- Safe Input Key registers its saved hotkey through the daemon on Windows; richer cross-platform global hotkey behavior is still roadmap work.
- Some shipped extensions are still intentionally host-native or hybrid rather than purely TypeScript-executed; that ownership is now centralized in `host_extensions.rs` as explicit host capabilities.

The migration preserves the daemon-first product contract while replacing its
runtime and presentation implementation.

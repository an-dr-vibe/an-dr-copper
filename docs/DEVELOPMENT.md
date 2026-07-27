# Development Guide

Project-specific recipes and reference for working on Copper.
For architecture and constraints read `docs/ARCHITECTURE.md` first.

## Build & run

```powershell
cargo build -p copperd                  # debug
cargo build -p copperd --release        # release
cargo tree -p copperd --no-default-features -e normal # headless graph excludes Bones web/platform
./scripts/build-release.ps1             # full dist packaging → dist/release/

./scripts/daemon.ps1 -Action run        # start daemon  (terminal A)
./scripts/daemon.ps1 -Action health     # health check  (terminal B)
./scripts/daemon.ps1 -Action shutdown   # stop daemon
cargo run -p copperd -- ui open --extension desktop-torrent-organizer
```

`daemon.ps1 -Action run` builds once before launching. Concurrent control
actions reuse that binary so Cargo does not hold a build lock for the daemon's
lifetime and Windows never attempts to replace a running executable.

The settings window uses the detachable Bones web/Wry presentation and is
enabled by default. Native requests use the versioned `copper.settings/1`
message protocol on the Bones bus. Use `ui open --browser` for the temporary
authenticated browser fallback, or build with `--no-default-features` only
when intentionally checking a headless/no-native-ui path.

Run `cargo fmt -p copperd --check` for Copper's formatting gate. The pinned
`bones/` checkout is an external workspace with its own formatting policy and
must not be rewritten by Copper's validation scripts.

Copper consumes `bones/core/runner` with default features disabled. Do not
enable Bones' `presentation` feature in the daemon; the settings window will
use the separate on-demand presentation composition.

The daemon constructs SDL/Wry resources only after a settings request and on
the daemon main thread. Closing the window detaches both `web` and
`copper-settings` endpoints. A catalog rebuild reattaches an open settings
presentation to the candidate engine; a stable catalog reload leaves it
untouched.

The daemon supplies each validated WASM Component to Bones with
`catalog_extension(manifest.id, artifact_path)` and adds only the already
disabled/platform-filtered runtime registry to the startup allow-list. Health
reports `catalogExtensions`, `catalogRebuilds`, lifecycle decode errors, and
the latest typed lifecycle state for each active component. Registry reloads
that keep the same ID-to-path catalog do not rebuild Bones; file changes are
left to Bones' transactional component supervisor.

CLI and authenticated HTTP triggers select the declared action before sending
a targeted `copper.bus/1` `action-request` to the component from
`copper-actions`. An empty response means accepted; a component may instead
return `job-accepted` or `job-result`. Legacy manifests without `runtime`
continue through the subprocess execution adapter during migration.

WASM guests request native work with a direct Bones `send` to
`copper-capabilities`. The payload is a `copper.bus/1` `capability-request`;
the host ignores any identity-like values inside `args` and authorizes only the
sender stamped by Bones. Validation is synchronous and bounded to 64 KiB; an
accepted request returns `job-accepted`, while execution and `job-result`
delivery are asynchronous. `notify` retains Copper's permissionless legacy
behavior; all other defined capabilities require the matching manifest
permission. Health exposes accepted, rejected, and pending request counts.

Accepted jobs cross a bounded channel to `copper-capability-worker`; the daemon
step only submits work, polls completions, and performs a targeted direct send
back to the requesting extension from `copper-jobs`. The distinct result sender
lets a result handler issue a follow-up request without creating a synchronous
direct-send cycle. Store operations are `get` and `set`.
`config.get`, `config.merge`, `status.get`, and `status.merge` expose the two
structured state streams. Every operation derives its extension scope from the
authorized job, never from payload arguments. The legacy `data.json` location
remains the guest key/value store for behavioral parity, but all access now
from WASM guests flows through `ExtensionStateStore` and atomic writes.

Native operation names and argument objects are:

- `fs`: `list {path}`, `move {src,dst}`, and `delete {path}`.
- `shell`: `run {cmd,args}` and `which {binary}`. Commands are launched
  directly with an argument vector; Copper does not interpolate a shell
  command string.
- `notify`: `show {message}`. Notifications retain the permissionless legacy
  contract, but the sender must still be an active WASM extension.
- `ui`: `show {markup}` and `update {state}`. Both require `ui`; the current
  host sink preserves legacy no-op behavior until Bones presentation lands.
- `keyboard`: `type-text {text}`, `send-key {key}`, `send-combo {combo}`, and
  `normalize-combo {combo}`.
- `secure-store`: `get {service,key}`, `set {service,key,value}`, and
  `delete {service,key}`. Results are delivered only to the requesting sender.
- `windows-display`: `status`, `toggle-taskbar-autohide`,
  `set-taskbar-autohide {autoHide}`,
  `set-resolution {width,height,refreshRate}`, and
  `set-scale {scalePercent}`. This family requires the dedicated
  `windows-display` permission and returns `platform-unsupported` away from
  Windows.

Paths, commands, messages, arrays, and structured UI values are type- and
size-checked before native execution. Filesystem, shell, and UI requests still
require their manifest permissions. Worker shutdown waits briefly for ordinary
jobs and then detaches a blocking native call so daemon shutdown cannot hang.
`windows-display` is an additive schema 1.0 permission: older manifests remain
valid, while extensions requesting display control must declare it explicitly.

## Recipe: add a host API module

Update the native implementation, permission policy, protocol, and guest SDK:

1. **`daemon/src/api/<name>.rs`** — implement public functions.
   Use `fs.rs` (stub) or `secure_store.rs` (real crate) as template.
2. **`daemon/src/api/mod.rs`** — `pub mod <name>;` (keep alphabetical).
3. **`daemon/src/descriptor.rs`** — add variant to `Permission` enum.
   Serde uses `#[serde(rename_all = "kebab-case")]`, so `SecureStore` → `"secure-store"`.
4. **`daemon/src/bones_integration/`** — validate and execute the capability.
5. **`daemon/src/cli.rs`** — add permission formatting when applicable.
6. **`schemas/extension/1.0.0/descriptor.schema.json`** — add the permission.
7. **`sdk/rust` and `sdk/COMPONENT_API.md`** — expose and document the helper.

Verify: `cargo test -p copperd --lib api::<name>` must pass.

## Recipe: add a new extension

The supported Component path is generated and built through PowerShell:

```powershell
./scripts/new-wasm-extension.ps1 -Id my-ext -Name "My Extension"
./scripts/build-wasm-extension.ps1 -ExtensionDir ./extensions/my-ext -Package
```

```
extensions/<id>/
  manifest.json
  <id>.wasm
  component/
    Cargo.toml
    Cargo.lock
    src/lib.rs
```

The scaffold declares `copper.component/1`, uses the Rust SDK in `sdk/rust`,
and generates Bones guest bindings from `sdk/wit/core.wit`. The build targets
`wasm32-wasip2` with locked dependencies and incremental compilation disabled,
copies `<id>.wasm` beside the manifest, validates the pair, and optionally
creates a deterministic two-file archive. See `sdk/COMPONENT_API.md`.

Minimal Component manifest:
```json
{
  "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
  "id": "my-ext",
  "name": "My Extension",
  "version": "0.1.0",
  "trigger": "my-ext",
  "runtime": {
    "kind": "wasm-component",
    "abi": "copper.component/1",
    "artifact": "my-ext.wasm"
  },
  "actions": [{ "id": "run", "label": "Run", "script": "onTrigger" }]
}
```

Verify: `cargo run -p copperd -- validate extensions/<id>/manifest.json`

The registry requires
`runtime: { "kind": "wasm-component", "abi": "copper.component/1",
"artifact": "<id>.wasm" }` and rejects omitted, missing, renamed, traversing,
or package-external artifacts.

To schedule one component action in the background, add the optional runtime
metadata below. Both config-key fields refer to the extension's scoped
`config.json`; omit either one to use its default directly.

```json
"background": {
  "action": "scan",
  "enabledConfig": "autoRun",
  "enabledByDefault": false,
  "intervalSecondsConfig": "pollIntervalSeconds",
  "defaultIntervalSeconds": 30
}
```

The action must appear in `actions`. Intervals are 1–86,400 seconds. Copper
checks schedules at one-second resolution and marks a run only after successful
Bones dispatch, so transient failures are retried.

## Recipe: add a built-in host extension

1. Scaffold `extensions/<id>/manifest.json` plus its Component.
2. Add only sensitive or platform-specific native operations to the relevant
   module under `daemon/src/bones_integration/`.
3. Declare the minimum manifest permissions and exercise them through UTR.

## Recipe: add a Cargo dependency

```toml
# daemon/Cargo.toml — explicit features always, no implicit defaults
[dependencies]
my-crate = { version = "X", features = ["needed-feature"] }

# Platform-specific only:
[target.'cfg(windows)'.dependencies]
my-crate = { version = "X", features = ["windows-feature"] }
```

## State files

| File | Owner | Purpose |
|---|---|---|
| `~/.Copper/extensions/copper-core/config.json` | daemon | `disabledExtensions` list |
| `~/.Copper/extensions/<id>/config.json` | user / settings UI | extension settings |
| `~/.Copper/extensions/<id>/status.json` | daemon | runtime status + `_stateContract` metadata |

Write state only through `ExtensionStateStore`. Never write JSON files directly.

## Key source files quick-reference

| File | Purpose |
|---|---|
| `daemon/src/api/mod.rs` | API module registry |
| `daemon/src/bones_integration/` | External Copper modules built on public Bones contracts |
| `daemon/src/descriptor.rs` | Manifest types + Permission enum |
| `daemon/src/host_extensions.rs` | Built-in host capability registry |
| `daemon/src/state_store.rs` | Config/status persistence |
| `sdk/COMPONENT_API.md` | Component guest contract |
| `schemas/extension/1.0.0/descriptor.schema.json` | Manifest schema |

## Module sizing guideline

- Target 300–600 lines per `.rs` file.
- 800+ lines → split by responsibility: service / transport / render / platform / tests.
- Test-heavy modules: extract to `<module>_tests.rs` via `include!`.

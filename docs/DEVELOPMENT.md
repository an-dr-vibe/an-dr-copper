# Development Guide

Project-specific recipes and reference for working on Copper.
For architecture and constraints read `docs/ARCHITECTURE.md` first.

## Build & run

```powershell
cargo build -p copperd                  # debug
cargo build -p copperd --release        # release
cargo tree -p copperd -e normal         # headless graph must not include Bones platform/renderer/ui
./scripts/build-release.ps1             # full dist packaging → dist/release/

./scripts/daemon.ps1 -Action run        # start daemon  (terminal A)
./scripts/daemon.ps1 -Action health     # health check  (terminal B)
./scripts/daemon.ps1 -Action shutdown   # stop daemon
cargo run -p copperd -- ui open --extension desktop-torrent-organizer
```

`daemon.ps1 -Action run` builds once before launching. Concurrent control
actions reuse that binary so Cargo does not hold a build lock for the daemon's
lifetime and Windows never attempts to replace a running executable.

The settings window is Tauri-backed and enabled by default. Use `ui open --browser` for browser fallback, or build with `--no-default-features` only when intentionally checking a headless/no-native-ui path.

Run `cargo fmt -p copperd --check` for Copper's formatting gate. The pinned
`bones/` checkout is an external workspace with its own formatting policy and
must not be rewritten by Copper's validation scripts.

Copper consumes `bones/core/runner` with default features disabled. Do not
enable Bones' `presentation` feature in the daemon; the settings window will
use the separate on-demand presentation composition.

The daemon supplies each validated WASM Component to Bones with
`catalog_extension(manifest.id, artifact_path)` and adds only the already
disabled/platform-filtered runtime registry to the startup allow-list. Health
reports `catalogExtensions`, `catalogRebuilds`, lifecycle decode errors, and
the latest typed lifecycle state for each active component. Registry reloads
that keep the same ID-to-path catalog do not rebuild Bones; file changes are
left to Bones' transactional component supervisor.

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

## Recipe: add a host API module

Five touch-points in Rust, then schema + SDK:

1. **`daemon/src/api/<name>.rs`** — implement public functions.
   Use `fs.rs` (stub) or `secure_store.rs` (real crate) as template.
2. **`daemon/src/api/mod.rs`** — `pub mod <name>;` (keep alphabetical).
3. **`daemon/src/descriptor.rs`** — add variant to `Permission` enum.
   Serde uses `#[serde(rename_all = "kebab-case")]`, so `SecureStore` → `"secure-store"`.
4. **`daemon/src/execution.rs`** — add arm to `permissions_as_strings` match.
5. **`daemon/src/cli.rs`** — add arm to the `format_permissions` match.
6. **`schemas/extension/1.0.0/descriptor.schema.json`** — add string to `permissions.items.enum`.
7. **`sdk/api.d.ts`** — add namespace to `Api` interface + entry to `Permission` union.

Verify: `cargo test -p copperd --lib api::<name>` must pass.

## Recipe: add a new extension

```
extensions/<id>/
  manifest.json
  main.ts         ← compatibility runtime; scaffold with generate-main
  <id>.wasm       ← component runtime alternative declared by manifest.runtime
```

Minimal manifest:
```json
{
  "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
  "id": "my-ext",
  "name": "My Extension",
  "version": "0.1.0",
  "trigger": "my-ext",
  "actions": [{ "id": "run", "label": "Run", "script": "onTrigger" }]
}
```

Verify: `cargo run -p copperd -- validate extensions/<id>/manifest.json`

For a WASM Component package, add
`runtime: { "kind": "wasm-component", "abi": "copper.component/1",
"artifact": "<id>.wasm" }`. The registry rejects missing, renamed, traversing,
or package-external artifacts.

## Recipe: add a built-in host extension

1. Add `extensions/<id>/manifest.json` + `main.ts`.
2. In `daemon/src/host_extensions.rs`:
   - Create a struct implementing `HostExtensionHandler`.
   - Add a `CapabilitySpec` entry in `capability_specs()`.
   - Implement `trigger_payload`, `apply_settings`, `dynamic_options`, `tick_background` as needed.
3. Register handler in `HostExtensionRegistry::new`.

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
| `daemon/src/execution.rs` | Trigger preparation + permission serialization |
| `daemon/src/host_extensions.rs` | Built-in host capability registry |
| `daemon/src/state_store.rs` | Config/status persistence |
| `sdk/api.d.ts` | TypeScript API contract (read by extension authors) |
| `schemas/extension/1.0.0/descriptor.schema.json` | Manifest schema |

## Module sizing guideline

- Target 300–600 lines per `.rs` file.
- 800+ lines → split by responsibility: service / transport / render / platform / tests.
- Test-heavy modules: extract to `<module>_tests.rs` via `include!`.

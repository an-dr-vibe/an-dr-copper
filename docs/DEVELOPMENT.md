# Development Guide

Project-specific recipes and reference for working on Copper.
For architecture and constraints read `docs/ARCHITECTURE.md` first.

## Build & run

```powershell
cargo build -p copperd                  # debug
cargo build -p copperd --release        # release
./scripts/build-release.ps1             # full dist packaging → dist/release/

./scripts/daemon.ps1 -Action run        # start daemon  (terminal A)
./scripts/daemon.ps1 -Action health     # health check  (terminal B)
./scripts/daemon.ps1 -Action shutdown   # stop daemon
cargo run -p copperd -- ui open --extension desktop-torrent-organizer
```

The settings window is Tauri-backed and enabled by default. Use `ui open --browser` for browser fallback, or build with `--no-default-features` only when intentionally checking a headless/no-native-ui path.

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
  main.ts         ← scaffold: cargo run -p copperd -- generate-main extensions/<id>/manifest.json
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

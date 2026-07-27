# AI Authoring Guide

This project is built so AI can safely create/modify extensions with predictable validation.

## Contract Inputs for AI

When requesting extension generation, always include:

1. Manifest schema: `schemas/extension/1.0.0/descriptor.schema.json`
2. Component contract: `sdk/COMPONENT_API.md` and `sdk/rust`
3. Task statement: what the extension should do
4. Target platforms: Windows/macOS/Linux expectations

## Expected AI Output

- `manifest.json` valid against schema
- Rust source under `component/`, with a committed `Cargo.lock`
- Component runtime: `<extension-id>.wasm` matching `manifest.runtime`
- `platforms` included only when the extension is intentionally OS-specific

## Verification Flow

After AI changes:

1. `./scripts/build-wasm-extension.ps1 -ExtensionDir <dir> -Package`
2. `cargo run -p copperd -- verify --extensions-dir <parent-dir>`
3. `./scripts/verify-loop.ps1`

## Design Rule

Manifest is the source of truth. If generated runtime code conflicts with
manifest permissions/actions, fix the manifest first, then rebuild the WASM
artifact.

## Runtime artifact

Every extension uses the Rust Component scaffold and must declare its runtime:

```powershell
./scripts/new-wasm-extension.ps1 -Id my-extension -Name "My Extension"
```

A packaged WASM Component declares the versioned runtime explicitly:

```json
"runtime": {
  "kind": "wasm-component",
  "abi": "copper.component/1",
  "artifact": "my-extension.wasm",
  "background": {
    "action": "scan",
    "enabledConfig": "autoRun",
    "enabledByDefault": false,
    "intervalSecondsConfig": "pollIntervalSeconds",
    "defaultIntervalSeconds": 30
  }
}
```

The artifact must be beside `manifest.json`, must be named exactly
`<manifest id>.wasm`, and must resolve inside the extension package. Component
packages contain no script entrypoint. The `background` object is optional. Its
action must be declared in `actions`; config-key fields read only this
extension's scoped settings, and the default interval must be 1–86,400 seconds.

## Platform Restriction

Use the optional manifest field below only when the extension should load on a subset of hosts:

```json
{
  "platforms": ["windows"]
}
```

Supported values are `windows`, `macos`, and `linux`. Omit the field for cross-platform extensions.

# Copper Architecture (Implemented MVP)

Version: 0.1.0  
Last updated: 2026-03-08

## 1. Scope

This document is derived from the original architecture draft and updated to match what is implemented in this repository.

Copper is a cross-platform extension host focused on AI-authored automations. The current implementation is intentionally descriptor-first and verification-first.

## 2. Key Changes vs Original Draft

The original draft assumed embedded `deno_core` + on-demand Tauri UI. For MVP, I changed this to:

1. Rust-first host and verifier (implemented now).
2. TypeScript runtime execution is optional and external (Deno CLI), not embedded.
3. UI is represented in descriptor/types but not rendered by a native window yet.

Why this change:

- Keeps build and tests reliable on Windows/macOS/Linux without heavy native GUI dependencies.
- Makes verification loops fast and deterministic.
- Preserves the extension contract so runtime/UI can be upgraded later without breaking descriptors.

## 3. Current Process Model

Single process in v0.1:

- `copperd` (Rust binary)
  - validates descriptors against JSON schema
  - discovers extension folders
  - provides dry-run trigger introspection
  - generates `main.ts` skeletons from descriptors

Planned next step (compatible with current model):

- add runtime adapter that executes `main.ts` with Deno when present.

## 4. Repository Layout

```text
.
|- daemon/                 # Rust crate (CLI host)
|- docs/                   # architecture + usage docs
|- extensions/             # sample extension(s)
|- schemas/                # JSON schema contracts
|- sdk/                    # TypeScript API types
|- scripts/                # bootstrap/build/verify scripts
|- AGENTS.md               # AI collaboration guide for this repo
`- Cargo.toml              # workspace root
```

## 5. Descriptor Contract

The source of truth remains `descriptor.json` and schema validation:

- Schema file: `schemas/extension/1.0.0/descriptor.schema.json`
- Supported `$schema` URL:
  `https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json`

### Intentional tightening

I made `actions` required in schema (not optional) so every extension is executable by default. This reduces ambiguous AI output and improves verification quality.

## 6. CLI Surface

`copperd` commands:

- `doctor` - checks required/optional tooling availability
- `validate <descriptor>` - validates one descriptor
- `list --extensions-dir <dir>` - discovers extensions
- `verify --extensions-dir <dir>` - verification pass for extension pack
- `trigger <id> [--action <id>]` - dry-run action inspection
- `generate-main <descriptor>` - generate TypeScript skeleton

## 7. Cross-Platform Strategy

- Core logic implemented in stable Rust.
- No OS-specific APIs required for build/test path.
- Scripts provided in both Bash and PowerShell.
- Optional Deno usage is feature-adjacent, not mandatory for core verification.

## 8. Testing and Verification Loops

Test strategy:

- Unit tests for schema validation behavior.
- Unit tests for extension discovery and permission checks.
- Unit tests for `main.ts` generation.

Loop strategy:

- `scripts/verify-loop.sh [n]`
- `scripts/verify-loop.ps1 -Iterations n`

Each iteration runs:

1. `cargo fmt --all --check`
2. `cargo test --workspace`
3. `cargo build --workspace --release`

## 9. Evolution Plan

1. Add runtime adapter trait with implementations:
   - `DryRunRuntime` (existing behavior)
   - `DenoRuntime` (optional)
2. Add permission enforcement at runtime boundary.
3. Add on-demand UI renderer process with the existing `ui` contract.
4. Keep schema backward compatibility by versioned URLs.

## 10. Non-Goals (Current)

- Extension marketplace
- Cloud sync
- Full daemonized tray lifecycle
- Embedded JS runtime in v0.1
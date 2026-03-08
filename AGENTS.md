# AGENTS.md

## Mission

Maintain Copper as a cross-platform, descriptor-first automation host that is easy to evolve through AI requests.

## Read First

Before editing code, read these files in order:

1. `docs/ARCHITECTURE.md`
2. `schemas/extension/1.0.0/descriptor.schema.json`
3. `sdk/api.d.ts`
4. `README.md`

## Operating Rules

1. Keep descriptor JSON schema backward compatible unless version is bumped.
2. Treat `descriptor.json` as source of truth for extension metadata/permissions/actions.
3. Keep the build and verification flow cross-platform (Windows/macOS/Linux).
4. Prefer PowerShell (`.ps1`) scripts as the cross-platform default (`pwsh` on Windows/macOS/Linux).
5. Do not introduce mandatory GUI/runtime dependencies that break headless CI builds.
6. Update docs (`docs/`) whenever architecture or CLI behavior changes.

## Validation Checklist

Run this before finalizing changes:

1. `cargo fmt --all --check`
2. `./scripts/run-tests.ps1`
3. `cargo build --workspace --release`

For stability checks, run:

- `./scripts/verify-loop.ps1 -Iterations 3`
- Coverage loop:
  - `for ($i = 1; $i -le 3; $i++) { ./scripts/run-tests.ps1; ./scripts/coverage.ps1; ./scripts/build-release.ps1 }`

## Extension Authoring Rules

When generating or editing extensions:

1. Validate descriptor against `schemas/extension/1.0.0/descriptor.schema.json`.
2. Keep `$schema` set to:
   `https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json`
3. Ensure every extension has both files:
   - `descriptor.json`
   - `main.ts`
4. Keep permissions minimal and explicit.

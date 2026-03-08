# AGENTS.md

## Mission

Maintain Copper as a cross-platform, manifest-first automation host that is easy to evolve through AI requests.

## Read First

Before editing code, read these files in order:

1. `docs/ARCHITECTURE.md`
2. `schemas/extension/1.0.0/descriptor.schema.json`
3. `sdk/api.d.ts`
4. `README.md`

## Operating Rules

1. Keep manifest schema compatibility unless schema version is bumped.
2. Treat `manifest.json` as source of truth for extension metadata/permissions/actions.
3. Keep the build and verification flow cross-platform (Windows/macOS/Linux).
4. Prefer PowerShell (`.ps1`) scripts as the cross-platform default (`pwsh` on Windows/macOS/Linux).
5. Do not introduce mandatory GUI/runtime dependencies that break headless CI builds.
6. Update docs (`docs/`) whenever architecture or CLI behavior changes.
7. Use TDD for extension work: write or update extension UTR tests first, then implement code, then refactor.

## Validation Checklist

Run this before finalizing changes:

1. `cargo fmt --all --check`
2. `cargo test -p copperd --test extension_utr`
3. `./scripts/run-tests.ps1`
4. `cargo build --workspace --release`
5. Daemon smoke:
   `./scripts/daemon.ps1 -Action run` (terminal A)
   `./scripts/daemon.ps1 -Action health` (terminal B)
   `./scripts/daemon.ps1 -Action shutdown` (terminal B)

TDD loop (required for extension changes):

1. Add/adjust a failing test in `daemon/tests/extension_utr.rs` (RED).
2. Implement extension change until test passes (GREEN).
3. Improve code/docs without changing behavior (REFACTOR).
4. Re-run `./scripts/run-tests.ps1` and `./scripts/build-release.ps1`.

For stability checks, run:

- `./scripts/verify-loop.ps1 -Iterations 3`
- `./scripts/coverage.ps1` (the only coverage mode: full/fair over production code)
- `./scripts/coverage.ps1 -FailOnUnderTarget -MinLineCoverage <target>` when enforcing a minimum gate
- Double-audit rule (anti-cheating, required):
  - Audit 1: summary coverage with no app-code exclusions (only toolchain/tests ignored).
  - Audit 2: LCOV `SF:` file parity check vs `daemon/src/**/*.rs` (declaration-only modules may be omitted).
- Coverage loop:
  - `for ($i = 1; $i -le 3; $i++) { ./scripts/run-tests.ps1; ./scripts/coverage.ps1; ./scripts/build-release.ps1 }`

## Extension Authoring Rules

When generating or editing extensions:

1. Validate manifest against `schemas/extension/1.0.0/descriptor.schema.json`.
2. Keep `$schema` set to:
   `https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json`
3. Ensure every extension has both files:
   - `manifest.json`
   - `main.ts`
4. Keep permissions minimal and explicit.


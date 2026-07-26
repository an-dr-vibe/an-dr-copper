# AGENTS.md

## Mission

Maintain Copper as a cross-platform, manifest-first automation host that is easy to evolve through AI requests.

## Role-Specific Context

Pick the file that matches your task. Each agent file describes the role behavior and tells you exactly which project docs to read for specifics — **project knowledge lives in `docs/`, not in agent files**.

| Role | Agent file | Project docs it points to |
|---|---|---|
| Architect | `agents/architect.md` | `docs/ARCHITECTURE.md` |
| Developer | `agents/developer.md` | `docs/DEVELOPMENT.md`, `sdk/api.d.ts`, `schemas/` |
| Tester | `agents/tester.md` | `docs/TESTING.md` |

## Project Docs Map

| Doc | Contains |
|---|---|
| `docs/ARCHITECTURE.md` | System overview, module layout, API surface table, design invariants, known gaps |
| `docs/BONES_MIGRATION_PLAN.md` | Accepted Copper-on-Bones direction, decisions, milestones, risks, and cutover gates |
| `docs/DEVELOPMENT.md` | Build commands, change recipes, state file locations, key source files |
| `docs/TESTING.md` | Test commands, file locations, TDD workflow, coverage rules, known failures |
| `docs/AI_AUTHORING.md` | How to generate and verify extensions |
| `sdk/api.d.ts` | TypeScript API contract for extension authors |
| `schemas/extension/1.0.0/descriptor.schema.json` | Manifest validation schema |

## Read First

Before editing code, read these files in order:

1. `docs/ARCHITECTURE.md`
2. `docs/DEVELOPMENT.md`
3. `sdk/api.d.ts`
4. `schemas/extension/1.0.0/descriptor.schema.json`

Before work related to the Bones migration, also read
`docs/BONES_MIGRATION_PLAN.md` and update its tracker when milestone status,
dependencies, decisions, or exit criteria change.

## Operating Rules

1. Keep manifest schema compatibility unless schema version is bumped.
2. Treat `manifest.json` as source of truth for extension metadata/permissions/actions.
3. Keep the build and verification flow cross-platform (Windows/macOS/Linux).
4. Prefer PowerShell (`.ps1`) scripts as the cross-platform default (`pwsh` on Windows/macOS/Linux).
5. Do not introduce mandatory GUI/runtime dependencies that break headless CI builds.
6. Update docs (`docs/`) whenever architecture or CLI behavior changes.
7. Do not create commits unless the user explicitly asks for a commit.
8. Use TDD for extension work: write or update extension UTR tests first, then implement code, then refactor.

## Validation Checklist

Run this before finalizing changes:

1. `cargo fmt -p copperd --check` (Bones is a pinned external workspace)
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
3. Ensure every extension has `manifest.json` plus its declared runtime
   artifact:
   - no `runtime` block: `main.ts`
   - `runtime.kind = "wasm-component"`: `<extension-id>.wasm`
4. Keep permissions minimal and explicit.

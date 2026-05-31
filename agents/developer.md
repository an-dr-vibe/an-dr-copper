# Developer Agent

## Role

Implement a planned change by following existing project patterns exactly.
Read the relevant recipe before writing any code; don't invent structure.

## Process

1. **Read project context** — see "What to read" below.
2. **Find the analogous existing code** — before writing, locate something similar already in the codebase.
3. **Follow the recipe** — use the step-by-step recipe for your change type.
4. **Verify after each touch-point** — compile and run the relevant test subset; don't batch failures.
5. **Run the full suite** — before declaring done, run `./scripts/run-tests.ps1`.

## What to read for this project

| Need | File |
|---|---|
| Step-by-step recipes for every change type | `docs/DEVELOPMENT.md` |
| Build commands, state file locations, key source files | `docs/DEVELOPMENT.md` |
| TypeScript API contract for extensions | `sdk/api.d.ts` |
| Manifest schema (validation rules) | `schemas/extension/1.0.0/descriptor.schema.json` |
| Architecture overview (module layout, why things are where they are) | `docs/ARCHITECTURE.md` |

## What to read for specific change types

| Change | Recipe location |
|---|---|
| Add a host API module | `docs/DEVELOPMENT.md` — "Recipe: add a host API module" |
| Add a new extension | `docs/DEVELOPMENT.md` — "Recipe: add a new extension" |
| Add a built-in host extension | `docs/DEVELOPMENT.md` — "Recipe: add a built-in host extension" |
| Add a Cargo dependency | `docs/DEVELOPMENT.md` — "Recipe: add a Cargo dependency" |
| Write tests | `docs/TESTING.md` |

## General rules

- Match the style of the file you are editing — naming, formatting, error handling, comment density.
- Never add a dependency without explicit feature flags.
- Never write state files directly; use `ExtensionStateStore`.
- Stubs are intentional — do not "fix" a stub by adding behavior unless that is the task.
- Do not create commits unless the user explicitly asks for a commit.

# AI Authoring Guide

This project is built so AI can safely create/modify extensions with predictable validation.

## Contract Inputs for AI

When requesting extension generation, always include:

1. Manifest schema: `schemas/extension/1.0.0/descriptor.schema.json`
2. API types: `sdk/api.d.ts`
3. Task statement: what the extension should do
4. Target platforms: Windows/macOS/Linux expectations

## Expected AI Output

- `manifest.json` valid against schema
- `main.ts` using only APIs declared in `sdk/api.d.ts`
- `platforms` included only when the extension is intentionally OS-specific

## Verification Flow

After AI changes:

1. `cargo run -p copperd -- validate <manifest-path>`
2. `cargo run -p copperd -- verify --extensions-dir <dir>`
3. `./scripts/verify-loop.(sh|ps1)`

## Design Rule

Manifest is the source of truth. If generated `main.ts` conflicts with manifest permissions/actions, fix manifest first, then regenerate/update `main.ts`.

## Platform Restriction

Use the optional manifest field below only when the extension should load on a subset of hosts:

```json
{
  "platforms": ["windows"]
}
```

Supported values are `windows`, `macos`, and `linux`. Omit the field for cross-platform extensions.

# AI Authoring Guide

This project is built so AI can safely create/modify extensions with predictable validation.

## Contract Inputs for AI

When requesting extension generation, always include:

1. Descriptor schema: `schemas/extension/1.0.0/descriptor.schema.json`
2. API types: `sdk/api.d.ts`
3. Task statement: what the extension should do
4. Target platforms: Windows/macOS/Linux expectations

## Expected AI Output

- `descriptor.json` valid against schema
- `main.ts` using only APIs declared in `sdk/api.d.ts`

## Verification Flow

After AI changes:

1. `cargo run -p copperd -- validate <descriptor-path>`
2. `cargo run -p copperd -- verify --extensions-dir <dir>`
3. `./scripts/verify-loop.(sh|ps1)`

## Design Rule

Descriptor is the source of truth. If generated `main.ts` conflicts with descriptor permissions/actions, fix descriptor first, then regenerate/update `main.ts`.
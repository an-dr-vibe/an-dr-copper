# Copper on Bones

Summary: Copper is an external Bones distribution as decided by
[ADR-001](../adr/ADR-001-build-copper-as-an-external-bones-distribution.md).

## Composition

The Copper executable is the application composition root. It constructs a
headless Bones engine and registers focused native modules behind a small
Copper facade:

```text
Copper executable
|- Bones kernel and WASM host
|- Copper control and lifecycle module
|- Copper capability and authorization modules
|- Copper state module
|- Copper platform modules
`- Bones web presentation with the Copper settings frontend
```

The always-on daemon does not construct presentation resources until a tray or
CLI request opens the settings UI.

## Extension package

A Copper extension package contains a validated `manifest.json` and a runtime
artifact. Existing schema 1.0 TypeScript packages remain valid during
construction. The released architecture uses a WASM Component implementing the
versioned Copper guest contract.

The manifest remains authoritative for:

- identity and version;
- supported platforms;
- actions and inputs;
- permissions;
- settings, status, and tray metadata.

Until the Bones catalog accepts explicit identities, the manifest ID and WASM
file stem are identical.

## Runtime messages

Extensions exchange versioned payloads through the Bones bus. Bones stamps the
sender identity; Copper capability modules authorize that identity against the
validated manifest before accepting work.

Potentially blocking operations create jobs. Native workers perform the work
away from the Bones loop and publish a versioned result or error. A component
callback never waits on shell execution, filesystem traversal, keychain UI, or
display reconfiguration.

## State

`ExtensionStateStore` remains the only owner of Copper config and status files.
Capability endpoints scope access to the calling extension. Bones opaque
persistence may support private guest state, but it does not replace Copper
configuration, status contracts, diagnostics, or migration behavior.

## Presentation

Copper owns the settings HTML, styling, behavior, and message contract. The
Bones web module owns the native window and `wry` webview. UI-specific fetch
traffic moves to versioned `web/*` and Copper bus messages; authenticated HTTP
remains available for daemon control operations.

## Current gaps

- TODO: add an event-driven headless Bones driver and on-demand presentation.
- TODO: define the versioned Copper guest and capability message contracts.
- TODO: add manifest runtime-artifact metadata without breaking schema 1.0.
- TODO: port the shipped extensions to WASM Components.
- TODO: remove the Deno and Tauri compatibility paths after parity.

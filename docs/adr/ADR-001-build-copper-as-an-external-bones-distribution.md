# ADR-001: Build Copper as an external Bones distribution

## Problem

Copper duplicates runtime concerns that Bones already implements: WASM
component isolation, lifecycle, hot reload, watchdogs, a message bus, and native
module composition. Replacing Copper with the stock Bones application would,
however, discard Copper's manifest, daemon, settings, state, permissions, and
OS automation contracts.

## Decision

Copper is a Bones-based distribution with its own executable composition root.
Trusted Copper infrastructure is implemented as external native Bones modules;
product behavior is implemented as manifest-authored WASM Components.

Bones owns component execution, lifecycle, messaging, and native web
presentation. Copper owns manifests, authorization, actions, state, the
authenticated daemon control plane, settings behavior, and product-specific OS
capabilities. The settings frontend is presented through the Bones web module,
currently backed by `wry`.

The replacement is constructed through verifiable increments and released as
one cutover. Temporary compatibility adapters do not become a permanent second
runtime.

## Rationale

- Uses Bones through the public extension points it is designed to expose.
- Keeps Copper product policy out of the generic engine.
- Makes WASM isolation and watchdog behavior consistent for every extension.
- Preserves a headless daemon while allowing native presentation on demand.
- Allows reusable integration gaps to be proven externally before upstreaming.

## Rejected alternatives

- **Fork Bones into Copper** — gives unrestricted access but couples product
  policy to the engine and makes upstream maintenance expensive.
- **Use the stock Bones application** — is initially smaller but its
  filename-only catalog and windowed composition cannot preserve Copper's
  manifest-first daemon contract.
- **Keep Copper as the host and use only the Bones WASM loader** — reduces the
  first change but leaves two lifecycle, scheduling, and presentation models.
- **Make Copper a WASM extension** — cannot safely own trusted daemon, keychain,
  tray, hotkey, display, and capability-policy responsibilities.

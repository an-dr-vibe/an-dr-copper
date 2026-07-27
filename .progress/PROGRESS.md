# Progress

**Flow:** Detailed Auto
**Phase:** COMMIT
**Goal:** Migrate Copper into a Bones-based application with external native Copper modules, a Bones wry-backed settings UI, and the existing Copper extensions rewritten as WASM Components while preserving observable functionality.
**Done when:** The Definition of Done and validation gates in docs/BONES_MIGRATION_PLAN.md pass, all shipped extensions run through Bones as WASM Components, the settings UI uses Bones web/wry presentation, and temporary Deno/Tauri runtime paths are removed from the released architecture.
**Constraints:** Preserve manifest-first compatibility, state and security boundaries, authenticated loopback control, headless cross-platform builds, and existing supported platform behavior; use TDD for extension changes; update Bones only for generic reusable gaps; no push or merge before final user approval.
**Out of scope:** Unrelated Bones game, renderer, audio, or game-core work; replacing Copper manifests with bones.toml; broad ambient WASI access; permanent dual runtimes; pushing or merging without final approval.

## Iterations

- [x] **Architecture record and behavioral baseline** (completed) — ~280 lines: add the Copper-on-Bones ADR/design record, correct stale runtime documentation, and add baseline registry/action parity tests before runtime changes.
- [x] **State and security parity fixtures** (completed) — ~260 lines: add atomic state migration fixtures plus control-plane authentication and undeclared-permission regression tests.
- [x] **External Bones module foundation** (completed) — ~280 lines: add pinned public Bones crate dependencies, a focused Copper facade/control module, and headless construction tests.
- [x] **Headless Bones daemon driver** (completed) — ~300 lines: integrate a stepped/event-driven Bones engine into daemon start, health, reload, and shutdown without activating product extensions.
- [x] **Versioned runtime artifacts** (completed) — ~280 lines: extend the manifest contract compatibly for WASM artifacts, validate ID/artifact pairing, and update registry tests and SDK documentation.
- [x] **Bones catalog lifecycle bridge** (completed) — ~300 lines: map validated manifests and disabled/platform policy to Bones activation, observe lifecycle events, and make reload transactions testable.
- [x] **Capability protocol and authorization** (completed) — ~300 lines: define versioned action/job envelopes and enforce host-stamped sender permissions with adversarial tests.
- [x] **Asynchronous jobs and scoped state** (completed) — ~300 lines: execute blocking work away from the Bones loop and expose scoped Copper config/status/store operations.
- [x] **Filesystem shell notification and UI capabilities** (completed) — ~300 lines: provide permissioned job handlers for filesystem, shell, notification, and UI result operations with negative tests.
- [x] **Keyboard secure-store and display capabilities** (completed) — ~300 lines: expose platform-gated keyboard, keychain, and Windows display operations through the capability broker.
- [x] **Bones-backed control plane and scheduling** (completed) — ~300 lines: route Copper trigger/background workflows through Bones messages while preserving authenticated CLI/HTTP behavior.
- [x] **On-demand Bones wry presentation** (completed) — ~300 lines: upstream generic lazy web/window lifecycle support to Bones and verify a headless engine can repeatedly open and close wry presentation.
- [x] **Copper settings UI on Bones web** (completed) — ~300 lines: port settings frontend transport to Bones web messages, preserve settings behavior, and remove Tauri window use.
- [x] **WASM guest SDK and packaging** (completed) — ~300 lines: add the versioned Copper guest contract, generated Rust bindings/templates, deterministic component builds, and manifest-plus-WASM packaging.
- [ ] **Session counter and sort downloads WASM ports** (verified) — ~280 lines: write UTR tests first, port both extensions to WASM Components, and remove their legacy execution paths after parity.
- [ ] **Desktop torrent organizer WASM port** (planned) — ~300 lines: test then port actions, background monitoring, scoped file moves, settings, and status behavior.
- [ ] **Safe Input Key and Windows display WASM ports** (planned) — ~300 lines: test then port orchestration while retaining sensitive hotkey, keychain, tray, and display work in native Copper capabilities.
- [ ] **Cutover cleanup release and full verification** (planned) — ~300 lines: remove Deno/Tauri/temporary adapters, update installers and docs, close the migration tracker, and run all release, coverage, smoke, and stability gates.

_Last updated: 2026-07-27T01:56:05.1779893Z_
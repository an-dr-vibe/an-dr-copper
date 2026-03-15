# Architecture Review

Review date: 2026-03-15

## Scope

This review covers the declared architecture, the daemon implementation, shipped extensions, host API surface, config UI, tray integration, and the current test suite.

Read sources first, per repository instructions:

- `docs/ARCHITECTURE.md`
- `schemas/extension/1.0.0/descriptor.schema.json`
- `sdk/api.d.ts`
- `README.md`

Implementation and tests reviewed:

- `daemon/src/*.rs`
- `daemon/src/api/*.rs`
- `daemon/src/runtime/mod.rs`
- `daemon/tests/*.rs`
- `extensions/*/manifest.json`
- `extensions/*/main.ts`
- `docs/*.md`

## High-Level Assessment

Copper already has several good architectural foundations:

- A versioned manifest schema exists and is enforced.
- Extension discovery and override rules are explicit.
- Config and status are split into separate files.
- The daemon is clearly the lifecycle center.
- Windows-specific display work is mostly isolated from the cross-platform core.

The main architectural problem is not missing structure. It is boundary erosion.

The repository describes a manifest-first extension host with a runtime abstraction, but the current implementation executes key product behavior through daemon-side special cases keyed by extension IDs and tray providers. The result is a mixed architecture:

- declarative extension model in schema/docs
- partially stubbed runtime contract in `sdk/api.d.ts`
- imperative host-owned behavior in `daemon.rs`, `config_ui.rs`, and `tray_extension.rs`

That split is now the dominant source of complexity, drift risk, and privilege leakage.

## Intended Boundaries

Based on the docs and contracts, the intended boundaries appear to be:

1. Manifest contract
   - `manifest.json` is the source of truth for extension metadata, permissions, actions, settings, and tray metadata.
2. Runtime boundary
   - the host loads an extension, chooses an action, and executes through a runtime adapter.
3. Host capability boundary
   - extensions call stable host APIs declared in `sdk/api.d.ts`; host internals stay behind that boundary.
4. Control-plane boundary
   - CLI, daemon IPC, and settings UI are transport surfaces over daemon services, not places where business rules live.
5. Persistence boundary
   - config, status, and migration logic should be centralized and reused.
6. Platform boundary
   - Windows-only functionality should sit behind capability/provider abstractions, not leak across the daemon core.

## Boundary Map in the Current Implementation

What is actually present:

- CLI and daemon IPC are implemented and functional.
- Schema validation is real.
- Registry loading is real.
- Runtime abstraction exists in `daemon/src/runtime/mod.rs`.
- The runtime abstraction is not used by daemon trigger flow.
- Host-native execution for specific extensions is implemented directly in the daemon.
- The config UI contains routing, persistence, dynamic option generation, settings application, and HTML rendering in one module.
- The tray layer contains a generic tray plus a provider-specific Windows tray implementation with its own persistence and status logic.

## Findings

### 1. Critical: Privileged local control planes are unauthenticated

Boundary violated:

- control plane vs privileged host operations

Evidence:

- Daemon IPC accepts plain JSON over localhost TCP and executes `trigger`, `reload`, `verify`, and `shutdown` without authentication in `daemon/src/daemon.rs:279-396`.
- The always-on settings UI binds to a localhost HTTP socket and accepts mutating POSTs for `/config/core`, `/config/extension/<id>`, and `/apply/extension/<id>` in `daemon/src/config_ui.rs:138-170` and `daemon/src/config_ui.rs:267-355`.
- Settings apply can invoke host-native Windows display actions directly in `daemon/src/config_ui.rs:676-720`.

Why this matters:

- Any local process can control the daemon.
- Any local process can modify extension config and trigger host actions.
- A browser-based localhost attack is plausible because the UI uses a plain HTTP server with no session token, no origin validation, and no CSRF defense.

Reinforcement:

- Replace TCP localhost IPC with OS-scoped local IPC where possible:
  - Windows named pipes
  - Unix domain sockets on macOS/Linux
- Add a daemon-generated auth token for all control surfaces.
- Add CSRF and origin checks for HTTP UI mutations.
- Split read-only and mutating endpoints and require explicit capability checks for mutating calls.

### 2. Critical: The runtime boundary exists on paper but is bypassed in execution

Boundary violated:

- runtime adapter vs daemon business logic

Evidence:

- `RuntimeAdapter` and `DryRunRuntime` are defined in `daemon/src/runtime/mod.rs:3-49`.
- No production caller uses that runtime abstraction.
- Actual trigger flow is implemented directly in `daemon/src/daemon.rs:402-445`.
- That flow returns manifest script text and `mainTsPath`, then performs daemon-owned special cases through `maybe_increment_session_counter` and `maybe_execute_windows_display_action` in `daemon/src/daemon.rs:431-516`.

Why this matters:

- The system cannot claim a stable extension execution boundary yet.
- The daemon, not the runtime, decides which extensions truly do work.
- `main.ts` becomes an optional artifact instead of executable truth.

Reinforcement:

- Introduce a production `ExecutionEngine` that every trigger path must use.
- Route CLI trigger, daemon trigger, UI apply, and background scheduling through the same execution path.
- Model host-native actions as runtime-backed capabilities, not daemon exceptions.

### 3. High: The daemon is coupled directly to shipped extension identities

Boundary violated:

- host platform services vs extension package identity

Evidence:

- Hardcoded extension IDs in `daemon/src/daemon.rs:23-26`.
- Desktop torrent monitor background work is hardwired in `daemon/src/daemon.rs:155-175` and `daemon/src/daemon.rs:588-730`.
- Session counter behavior is hardwired in `daemon/src/daemon.rs:460-488`.
- Windows display action execution is hardwired in `daemon/src/daemon.rs:490-560`.
- UI dynamic options special-case `windows-display-manager` in `daemon/src/config_ui.rs:652-656`.
- UI settings apply special-case `windows-display-manager` in `daemon/src/config_ui.rs:692-709`.
- Tray provider support is hardcoded to `"windows-display"` in `daemon/src/tray_extension.rs:14-60`.

Why this matters:

- Sample extensions are acting like built-in product modules.
- Adding a second host-native extension currently requires editing multiple core modules.
- Extension packaging is not independent of host release cadence.

Reinforcement:

- Add a host capability registry:
  - action provider by capability
  - tray provider by provider ID
  - dynamic option provider by option source
  - settings apply handler by declared host action type
- Remove extension ID checks from the daemon core.
- If some extensions are truly built-ins, formalize that with an explicit built-in host module concept instead of treating them as normal extensions.

### 4. High: Persistence and status projection are duplicated across modules

Boundary violated:

- persistence service vs transport/UI/platform features

Evidence:

- Daemon owns config/status paths and JSON mutation in `daemon/src/daemon.rs:757-780` plus Windows display status updates in `daemon/src/daemon.rs:520-560` and desktop torrent status updates in `daemon/src/daemon.rs:700-730`.
- Config UI defines its own config/status path helpers and merge logic in `daemon/src/config_ui.rs:505-564` and load logic in `daemon/src/config_ui.rs:585-595`.
- Tray extension defines separate path helpers and save/load/update logic in `daemon/src/tray_extension.rs:720-805`.

Why this matters:

- Migration logic is repeated.
- Error handling is inconsistent.
- Config and status semantics can drift between daemon, UI, and tray code.

Reinforcement:

- Create a single `ExtensionStateStore` service for:
  - path resolution
  - `config.json` / `status.json` / legacy `data.json`
  - merge semantics
  - migration
  - atomic writes
- Make daemon, UI, and tray consumers depend on that service only.

### 5. High: The shipped `main.ts` files are not authoritative and can drift from host behavior

Boundary violated:

- manifest/runtime artifacts vs daemon-owned behavior

Evidence:

- `windows-display-manager/main.ts` only renders explanatory UI and tells the user to run the daemon trigger command in `extensions/windows-display-manager/main.ts:19-44`.
- Real execution for that extension is in `daemon/src/daemon.rs:490-560`, `daemon/src/api/windows_display.rs`, and `daemon/src/tray_extension.rs:546-585`.
- `desktop-torrent-organizer/main.ts` implements real move/install/show logic in `extensions/desktop-torrent-organizer/main.ts:168-194`, but the daemon separately runs a Rust background torrent mover in `daemon/src/daemon.rs:161-175` and `daemon/src/daemon.rs:623-730`.
- `session-counter/main.ts` increments via store in `extensions/session-counter/main.ts:5-22`, while the daemon separately mutates status for the same concept in `daemon/src/daemon.rs:460-488`.

Why this matters:

- There are multiple truths for the same extension.
- Reviewing an extension package does not tell you what the host will actually do.
- Extension portability is weakened because behavior lives outside the extension.

Reinforcement:

- Decide per extension whether it is:
  - runtime-executed TypeScript
  - host-native built-in
  - hybrid with explicit host-backed actions
- Encode that choice in the manifest schema instead of inferring it from ID.
- Add tests that assert behavior ownership and forbid silent duplication.

### 6. High: The host API contract diverges sharply from the actual host implementation

Boundary violated:

- published SDK contract vs host implementation

Evidence:

- `sdk/api.d.ts` declares real async host APIs for fs, shell, ui, store, windows display, and tray in `sdk/api.d.ts:38-90`.
- Current host API modules are placeholders:
  - `daemon/src/api/fs.rs:1-10`
  - `daemon/src/api/shell.rs:1-14`
  - `daemon/src/api/notify.rs:1`
  - `daemon/src/api/ui.rs:1-3`
  - `daemon/src/api/store.rs:1-16`

Why this matters:

- AI-generated extensions are being authored against a richer contract than the host currently executes.
- The repository reads as though a runtime exists, but the host APIs are mostly no-op stubs.
- This hides integration gaps and increases surprise when runtime execution is eventually added.

Reinforcement:

- Either reduce the published contract to what is actually implemented now, or
- mark the contract as planned and introduce feature-level status metadata, or
- complete the runtime and host adapters before adding more contract surface.

### 7. Medium: The config UI module is doing too many jobs

Boundary violated:

- transport/router vs application service vs storage vs presentation

Evidence:

- Socket server startup in `daemon/src/config_ui.rs:138-170`.
- Request parsing and routing in `daemon/src/config_ui.rs:267-355`.
- Config persistence in `daemon/src/config_ui.rs:521-564`.
- Extension info assembly in `daemon/src/config_ui.rs:629-674`.
- Settings application in `daemon/src/config_ui.rs:676-720`.
- HTML rendering and front-end script generation in `daemon/src/config_ui.rs:750+`.

Why this matters:

- The module is expensive to change safely.
- Security and product logic are mixed together.
- Testing at the right abstraction level is harder than it should be.

Reinforcement:

- Split into:
  - HTTP transport
  - UI application service
  - state store
  - extension capability adapters
  - HTML/view assets

### 8. Medium: Tray provider architecture is still provider-specific code in disguise

Boundary violated:

- tray extension metadata vs generic provider runtime

Evidence:

- Only one provider string is recognized in `daemon/src/tray_extension.rs:14-60`.
- Provider-specific state, persistence, status parsing, and command execution all live in one Windows-specific block in `daemon/src/tray_extension.rs:233-805`.
- The provider uses a process-global mutable pointer via `WINDOWS_DISPLAY_STATE` and `state_mut()` in `daemon/src/tray_extension.rs:1014`.

Why this matters:

- The trait boundary for tray providers does not exist yet.
- Adding another provider will likely repeat the same structure and complexity.
- Unsafe global state increases maintenance risk.

Reinforcement:

- Define a `TrayProvider` trait with lifecycle, menu model, and action dispatch.
- Move provider persistence access out of the provider implementation.
- Eliminate the process-global mutable pointer by using window user data or a provider-owned registry.

### 9. Medium: Validation is strong on schema shape, weak on architectural consistency

Boundary violated:

- manifest-first contract vs actual behavior validation

Evidence:

- Schema validation is real, but `parse_and_validate` recompiles the JSON schema every call in `daemon/src/schema.rs:21-30`.
- Verify flows mostly check manifest validity and `main.ts` presence in `daemon/src/cli.rs`, `daemon/src/daemon.rs:138-152`, and `daemon/tests/extension_utr.rs:43-60`.
- The UTR suite checks string presence in `main.ts`, not that runtime behavior matches manifest or host implementation in `daemon/tests/extension_utr.rs:151-165` and `daemon/tests/extension_utr.rs:281-300`.

Why this matters:

- Hot reload does unnecessary schema compilation work.
- Architectural drift between manifest, `main.ts`, and host behavior is not detected.

Reinforcement:

- Cache the compiled schema.
- Add consistency tests for:
  - manifest permissions vs actual capability usage
  - manifest actions vs executable action handlers
  - host-native declared actions vs registered providers
  - no extension-ID special cases outside approved registries

### 10. Medium: Docs describe a cleaner architecture than the code currently enforces

Boundary violated:

- architecture documentation vs implementation truth

Evidence:

- Docs present a manifest-first, daemon-centered, runtime-adapter direction.
- The repository also states “Manifest is the source of truth” in `docs/AI_AUTHORING.md`.
- In practice, important behaviors are driven by daemon-side ID checks and provider checks, not solely by manifest data.

Why this matters:

- Contributors will optimize for the documented abstraction and accidentally deepen the hidden coupling.
- AI-assisted changes are especially vulnerable because the visible contract is cleaner than the executed one.

Reinforcement:

- Document the current hybrid model explicitly.
- Add a “host-native built-ins” section if that is the intended transitional state.
- Document which extension fields are descriptive only versus executable today.

## Most Important Architectural Violations

These are the issues I would treat as the top refactoring targets:

1. Unauthenticated privileged local surfaces.
2. Trigger execution bypassing the runtime abstraction.
3. Extension identity hardcoded into core host behavior.
4. Persistence logic duplicated across daemon, UI, and tray.

If these four are not corrected, the system will keep accreting product logic in transport and platform modules.

## Recommended Target Shape

### Core services

- `ExtensionRegistry`
  - discovery, schema validation, override rules
- `ExecutionEngine`
  - one entry point for trigger execution
- `CapabilityRegistry`
  - maps declared host-backed actions/providers to implementations
- `ExtensionStateStore`
  - config/status/migration and atomic persistence
- `ControlPlane`
  - authenticated IPC and authenticated HTTP UI

### Extension execution modes

Make execution explicit in the manifest or internal descriptor model:

- `runtime = "typescript"`
- `runtime = "host-native"`
- `runtime = "hybrid"`

For host-native or hybrid actions, declare action-to-provider mapping instead of inferring by extension ID.

### UI and tray model

- UI reads descriptor + current state + provider metadata from services.
- UI never contains extension-specific `match descriptor.id`.
- Tray loads providers from a registry and consumes normalized menu/state models.

## Suggested Refactor Order

1. Secure control surfaces.
2. Centralize persistence/state store.
3. Introduce capability/provider registries.
4. Route trigger flow through one execution engine.
5. Remove extension-ID checks from daemon core.
6. Split config UI into smaller layers.
7. Convert tray provider logic behind a trait boundary.

## Test Gaps to Add

- Regression test that all mutating control-plane endpoints reject missing/invalid auth.
- Test that every trigger path uses the same execution engine.
- Test that host-native actions are registered through providers, not extension ID checks.
- Test that manifest-declared `settings.applyActions` map to registered host capabilities.
- Test that `main.ts`, manifest, and provider registrations stay aligned for hybrid extensions.
- Static grep-style architecture test forbidding sample extension IDs in core modules outside an approved registry module.

## Conclusion

Copper is at an important transition point.

The project already has a coherent contract story, but the implementation has started to pull real behavior into daemon-side exceptions. That is understandable for an MVP, but it is the main force weakening the architecture now.

The next phase should not be “add more extensions.” It should be “finish the architectural seams already implied by the docs”:

- secure the control plane
- make execution go through one engine
- replace extension-ID branching with provider registration
- centralize persistence

Once those seams are reinforced, the manifest-first model becomes real rather than aspirational.

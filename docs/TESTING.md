# Testing Guide

Project-specific test commands, locations, and patterns for Copper.

## Commands

```powershell
# Full suite — run before every commit
./scripts/run-tests.ps1

# Scoped runs
cargo test -p copperd --lib                     # all unit tests
cargo test -p copperd --lib api::<module>       # single API module
cargo test -p copperd --test extension_utr      # extension integration tests
cargo test -p copperd --lib -- --nocapture      # show stdout/stderr (pipe through 2>&1)

# Coverage
./scripts/coverage.ps1
./scripts/coverage.ps1 -FailOnUnderTarget -MinLineCoverage <n>

# Stability (catches flaky tests)
./scripts/verify-loop.ps1 -Iterations 3

# Rust guest SDK, generated Component template, and deterministic archives
./scripts/test-wasm-sdk.ps1

# Shipped Component unit tests, Clippy, reproducible artifacts, and manifests
./scripts/test-wasm-extensions.ps1
```

## Test file locations

| Scope | Location |
|---|---|
| API unit tests | inline `#[cfg(test)]` in `daemon/src/api/<name>.rs` |
| Extension integration | `daemon/tests/extension_utr.rs` |
| Config UI | `daemon/src/config_ui_tests.rs` (included via `include!` macro) |
| Daemon service | inline in `daemon/src/daemon_service.rs` |

## TDD workflow (required for extension changes)

1. **RED** — add/adjust a failing test in `daemon/tests/extension_utr.rs`.
2. **GREEN** — implement until the test passes.
3. **REFACTOR** — clean up without changing behavior.
4. Re-run `./scripts/run-tests.ps1`.

## Pattern for API module tests

Every `daemon/src/api/<name>.rs` needs inline tests covering at minimum:

- Happy path (correct value returned)
- Missing/not-found case (`None` or empty)
- No-op safety (write/delete on missing entry does not panic)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn <operation>_<expected_behavior>() {
        // arrange → act → assert
    }
}
```

Reference: `daemon/src/api/secure_store.rs` (real implementation with roundtrip test).

## Coverage audit rules

Two audits required when enforcing a coverage gate:

1. **Summary audit** — run with no app-code exclusions (exclude only toolchain noise and test files).
2. **File parity audit** — verify that `SF:` entries in the LCOV output match `daemon/src/**/*.rs`; declaration-only stubs may be omitted.

## Known pre-existing failures

These two tests fail in the current codebase and are not caused by API-layer changes:

- `config_ui::tests::daemon_ui_server_handles_core_and_extension_routes`
- `config_ui::tests::open_extension_config_closes_on_close_route`

Do not count these as regressions when verifying your own changes.

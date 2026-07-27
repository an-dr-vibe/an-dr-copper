# Copper Component API

`copper.component/1` is Copper's first WASM Component guest ABI. The component
implements the generated `bones:core/extension@0.1.0` world in
[`wit/core.wit`](wit/core.wit), while Copper-owned messages use JSON
`copper.bus/1` envelopes over the world’s byte-oriented `send` and
`on-message` functions.

The Rust SDK in [`rust/`](rust/) generates its bindings from that WIT contract,
validates host senders, encodes capability requests, and correlates accepted
jobs. A guest handles:

- `action-request` from `copper-actions`;
- `job-result` and `error` from `copper-jobs`;
- the immediate `job-accepted` or `error` reply returned by
  `send("copper-capabilities", ...)`.

Capability results are asynchronous. Pass a request ID that remains unique for
the extension across component reloads. Action handlers should reuse the
host-issued `HostEvent::Action::request_id`; if one action sends multiple
capability requests, derive a distinct token from that ID for each call. Keep
the returned `request_id` or `job_id`, then continue the workflow when the
matching host event arrives.
The host authorizes only the Bones-stamped component identity and manifest
permissions; guest-provided identity fields have no authority.

Rust components target `wasm32-wasip2`. Copper's build script pins dependency
resolution with `Cargo.lock`, disables incremental compilation, copies exactly
`<manifest id>.wasm` beside the manifest, validates the pair, and can create a
deterministic package archive.

use super::{Capability, CopperEnvelope, COPPER_BUS_PROTOCOL_V1};
use crate::descriptor::Permission;
use crate::extension::Registry;
use bones_bus::{Envelope, Handler, Module, ModuleContext};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

/// Direct Bones endpoint used by WASM guests to request native work.
pub const COPPER_CAPABILITY_ENDPOINT: &str = "copper-capabilities";
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_PENDING_JOBS: usize = 256;
const MAX_PENDING_PER_EXTENSION: usize = 32;
const MAX_REPLAY_KEYS: usize = 1024;
pub(crate) type CapabilityPolicies = BTreeMap<String, Vec<Permission>>;

/// Permission-checked work item ready for asynchronous native execution.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizedCapabilityJob {
    pub job_id: String,
    pub extension_id: String,
    pub request_id: String,
    pub capability: Capability,
    pub operation: String,
    pub args: Map<String, Value>,
}

#[derive(Debug, Default)]
struct CapabilityState {
    policies: CapabilityPolicies,
    pending: VecDeque<AuthorizedCapabilityJob>,
    replay_keys: BTreeSet<(String, String)>,
    replay_order: VecDeque<(String, String)>,
    next_job_id: u64,
    accepted: u64,
    rejected: u64,
}

/// Shared queue and policy handle owned by the daemon-side driver.
#[derive(Debug, Clone, Default)]
pub struct CopperCapabilityHandle {
    state: Arc<Mutex<CapabilityState>>,
}

impl CopperCapabilityHandle {
    fn from_registry(registry: &Registry) -> Self {
        let handle = Self::default();
        handle.replace_registry(registry);
        handle
    }

    /// Replaces manifest policy and drops queued jobs no longer authorized.
    pub fn replace_registry(&self, registry: &Registry) {
        if let Ok(mut state) = self.state.lock() {
            state.policies = build_policies(registry);
            let policies = state.policies.clone();
            state.pending.retain(|job| {
                policies.get(&job.extension_id).is_some_and(|permissions| {
                    is_capability_authorized(permissions, job.capability)
                })
            });
        }
    }

    /// Removes the oldest authorized job without blocking the Bones loop.
    pub fn pop_pending(&self) -> Option<AuthorizedCapabilityJob> {
        self.state.lock().ok()?.pending.pop_front()
    }

    pub(crate) fn requeue_front(&self, job: AuthorizedCapabilityJob) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.push_front(job);
        }
    }

    pub fn pending_count(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.pending.len())
            .unwrap_or_default()
    }

    pub fn accepted_count(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.accepted)
            .unwrap_or_default()
    }

    pub fn rejected_count(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.rejected)
            .unwrap_or_default()
    }
}

/// Native Bones module that validates and queues capability direct calls.
pub struct CopperCapabilityModule {
    handle: CopperCapabilityHandle,
}

impl CopperCapabilityModule {
    pub fn new(registry: &Registry) -> (Self, CopperCapabilityHandle) {
        let handle = CopperCapabilityHandle::from_registry(registry);
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }

    fn authorize(&self, sender: &str, payload: &[u8]) -> CopperEnvelope {
        if payload.len() > MAX_REQUEST_BYTES {
            return self.reject(
                None,
                "request-too-large",
                "capability request exceeds 64 KiB",
            );
        }
        let request = match serde_json::from_slice::<CopperEnvelope>(payload) {
            Ok(CopperEnvelope::CapabilityRequest {
                protocol,
                request_id,
                capability,
                operation,
                args,
            }) => (protocol, request_id, capability, operation, args),
            Ok(_) => {
                return self.reject(
                    None,
                    "invalid-message",
                    "capability endpoint accepts only capability-request messages",
                );
            }
            Err(err) => {
                return self.reject(None, "invalid-json", format!("invalid request: {err}"));
            }
        };
        let (protocol, request_id, capability, operation, args) = request;
        if protocol != COPPER_BUS_PROTOCOL_V1 {
            return self.reject(
                Some(request_id),
                "protocol-mismatch",
                format!("expected protocol {COPPER_BUS_PROTOCOL_V1}"),
            );
        }
        if !valid_token(&request_id) {
            return self.reject(
                Some(request_id),
                "invalid-request-id",
                "requestId must be 1-128 ASCII letters, digits, '.', '_', or '-'",
            );
        }
        if !valid_token(&operation) {
            return self.reject(
                Some(request_id),
                "invalid-operation",
                "operation must be 1-128 ASCII letters, digits, '.', '_', or '-'",
            );
        }

        let mut state = match self.handle.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return CopperEnvelope::error(
                    Some(request_id),
                    "internal",
                    "capability policy unavailable",
                );
            }
        };
        let Some(permissions) = state.policies.get(sender) else {
            return reject_locked(
                &mut state,
                Some(request_id),
                "unknown-sender",
                "sender is not an active Copper WASM extension",
            );
        };
        if !is_capability_authorized(permissions, capability) {
            return reject_locked(
                &mut state,
                Some(request_id),
                "permission-denied",
                "manifest does not declare the required permission",
            );
        }
        let replay_key = (sender.to_string(), request_id.clone());
        if state.replay_keys.contains(&replay_key) {
            return reject_locked(
                &mut state,
                Some(request_id),
                "replayed-request",
                "requestId was already accepted for this sender",
            );
        }
        if state
            .pending
            .iter()
            .filter(|job| job.extension_id == sender)
            .count()
            >= MAX_PENDING_PER_EXTENSION
        {
            return reject_locked(
                &mut state,
                Some(request_id),
                "sender-queue-full",
                "extension capability queue is full",
            );
        }
        if state.pending.len() >= MAX_PENDING_JOBS {
            return reject_locked(
                &mut state,
                Some(request_id),
                "queue-full",
                "capability queue is full",
            );
        }

        state.next_job_id = state.next_job_id.saturating_add(1);
        let job_id = format!("job-{}", state.next_job_id);
        remember_replay_key(&mut state, replay_key);
        state.pending.push_back(AuthorizedCapabilityJob {
            job_id: job_id.clone(),
            extension_id: sender.to_string(),
            request_id: request_id.clone(),
            capability,
            operation,
            args,
        });
        state.accepted = state.accepted.saturating_add(1);
        CopperEnvelope::JobAccepted {
            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
            request_id,
            job_id,
        }
    }

    fn reject(
        &self,
        request_id: Option<String>,
        code: &str,
        message: impl Into<String>,
    ) -> CopperEnvelope {
        if let Ok(mut state) = self.handle.state.lock() {
            state.rejected = state.rejected.saturating_add(1);
        }
        CopperEnvelope::error(request_id, code, message)
    }
}

impl Handler for CopperCapabilityModule {
    fn handle(&mut self, _envelope: &Envelope) {}
}

impl Module for CopperCapabilityModule {
    fn name(&self) -> &str {
        COPPER_CAPABILITY_ENDPOINT
    }

    fn init(&mut self, _context: &mut ModuleContext) -> Result<(), String> {
        Ok(())
    }

    fn respond(&mut self, sender: &str, payload: &[u8]) -> Option<Vec<u8>> {
        serde_json::to_vec(&self.authorize(sender, payload)).ok()
    }
}

pub(crate) fn build_policies(registry: &Registry) -> CapabilityPolicies {
    registry
        .list()
        .filter(|extension| extension.wasm_component_path().is_some())
        .map(|extension| {
            (
                extension.descriptor.id.clone(),
                extension.descriptor.permissions.clone(),
            )
        })
        .collect()
}

pub(crate) fn is_capability_authorized(permissions: &[Permission], capability: Capability) -> bool {
    capability
        .required_permission()
        .is_none_or(|permission| permissions.contains(&permission))
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn remember_replay_key(state: &mut CapabilityState, key: (String, String)) {
    if state.replay_keys.len() >= MAX_REPLAY_KEYS {
        if let Some(expired) = state.replay_order.pop_front() {
            state.replay_keys.remove(&expired);
        }
    }
    state.replay_keys.insert(key.clone());
    state.replay_order.push_back(key);
}

fn reject_locked(
    state: &mut CapabilityState,
    request_id: Option<String>,
    code: &str,
    message: impl Into<String>,
) -> CopperEnvelope {
    state.rejected = state.rejected.saturating_add(1);
    CopperEnvelope::error(request_id, code, message)
}

#[cfg(test)]
mod tests {
    use super::{CopperCapabilityModule, COPPER_CAPABILITY_ENDPOINT};
    use crate::bones_integration::{Capability, CopperEnvelope, COPPER_BUS_PROTOCOL_V1};
    use crate::descriptor::COMPONENT_ABI_V1;
    use crate::extension::Registry;
    use bones_bus::Module;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn declared_permission_accepts_and_queues_host_stamped_sender() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "allowed", &["fs"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);

        let reply = respond(
            &mut module,
            "allowed",
            capability_request("request-1", "fs"),
        );
        assert!(matches!(reply, CopperEnvelope::JobAccepted { .. }));
        let job = handle.pop_pending().expect("authorized job");
        assert_eq!(job.extension_id, "allowed");
        assert_eq!(job.capability, Capability::Fs);
        assert_eq!(handle.accepted_count(), 1);
        assert_eq!(module.name(), COPPER_CAPABILITY_ENDPOINT);
    }

    #[test]
    fn undeclared_unknown_and_legacy_senders_are_denied() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "restricted", &[]);
        write_legacy(temp.path(), "legacy", &["fs"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);

        assert_error(
            respond(
                &mut module,
                "restricted",
                capability_request("request-1", "fs"),
            ),
            "permission-denied",
        );
        assert_error(
            respond(
                &mut module,
                "spoofed-extension",
                capability_request("request-2", "notify"),
            ),
            "unknown-sender",
        );
        assert_error(
            respond(&mut module, "legacy", capability_request("request-3", "fs")),
            "unknown-sender",
        );
        assert_eq!(handle.rejected_count(), 3);
        assert_eq!(handle.pending_count(), 0);
    }

    #[test]
    fn clock_and_notification_remain_permissionless_for_active_wasm_senders() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "notifier", &[]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);

        assert!(matches!(
            respond(
                &mut module,
                "notifier",
                capability_request("request-1", "notify")
            ),
            CopperEnvelope::JobAccepted { .. }
        ));
        assert!(matches!(
            respond(
                &mut module,
                "notifier",
                capability_request("request-2", "clock")
            ),
            CopperEnvelope::JobAccepted { .. }
        ));
        assert_eq!(
            handle.pop_pending().expect("notification job").capability,
            Capability::Notify
        );
        assert_eq!(
            handle.pop_pending().expect("clock job").capability,
            Capability::Clock
        );
    }

    #[test]
    fn windows_display_requires_its_dedicated_manifest_permission() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "display", &["windows-display"]);
        write_component(temp.path(), "generic-ui", &["ui"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);

        assert!(matches!(
            respond(
                &mut module,
                "display",
                capability_request("request-1", "windows-display")
            ),
            CopperEnvelope::JobAccepted { .. }
        ));
        assert_error(
            respond(
                &mut module,
                "generic-ui",
                capability_request("request-2", "windows-display"),
            ),
            "permission-denied",
        );
        assert_eq!(
            handle.pop_pending().expect("display job").capability,
            Capability::WindowsDisplay
        );
    }

    #[test]
    fn replay_and_protocol_mismatch_fail_without_duplicate_jobs() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "allowed", &["store"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);
        let request = capability_request("request-1", "store");

        assert!(matches!(
            respond(&mut module, "allowed", request.clone()),
            CopperEnvelope::JobAccepted { .. }
        ));
        assert_error(respond(&mut module, "allowed", request), "replayed-request");
        let mismatch = json!({
            "type": "capability-request",
            "protocol": "copper.bus/999",
            "requestId": "request-2",
            "capability": "store",
            "operation": "get",
            "args": {}
        });
        assert_error(
            respond(&mut module, "allowed", mismatch),
            "protocol-mismatch",
        );
        assert_eq!(handle.pending_count(), 1);
    }

    #[test]
    fn policy_refresh_drops_jobs_and_denies_permissions_removed_from_manifest() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "mutable", &["fs"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);
        assert!(matches!(
            respond(
                &mut module,
                "mutable",
                capability_request("request-1", "fs")
            ),
            CopperEnvelope::JobAccepted { .. }
        ));

        write_manifest(temp.path(), "mutable", &[], true);
        let restricted = Registry::load_from_dir(temp.path()).expect("restricted registry");
        handle.replace_registry(&restricted);

        assert_eq!(handle.pending_count(), 0);
        assert_error(
            respond(
                &mut module,
                "mutable",
                capability_request("request-2", "fs"),
            ),
            "permission-denied",
        );
    }

    #[test]
    fn malformed_wrong_kind_and_oversized_payloads_are_rejected() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "allowed", &["fs"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);

        assert_error(respond_bytes(&mut module, "allowed", b"{"), "invalid-json");
        assert_error(
            respond(
                &mut module,
                "allowed",
                json!({
                    "type": "action-request",
                    "protocol": COPPER_BUS_PROTOCOL_V1,
                    "requestId": "request-1",
                    "actionId": "run"
                }),
            ),
            "invalid-message",
        );
        assert_error(
            respond_bytes(&mut module, "allowed", &vec![b'x'; 64 * 1024 + 1]),
            "request-too-large",
        );
        assert_eq!(handle.rejected_count(), 3);
        assert_eq!(handle.pending_count(), 0);
    }

    #[test]
    fn one_sender_cannot_consume_the_entire_shared_queue() {
        let temp = tempdir().expect("tempdir");
        write_component(temp.path(), "noisy", &["store"]);
        write_component(temp.path(), "peer", &["store"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let (mut module, handle) = CopperCapabilityModule::new(&registry);

        for index in 0..32 {
            assert!(matches!(
                respond(
                    &mut module,
                    "noisy",
                    capability_request(&format!("request-{index}"), "store")
                ),
                CopperEnvelope::JobAccepted { .. }
            ));
        }
        assert_error(
            respond(
                &mut module,
                "noisy",
                capability_request("request-overflow", "store"),
            ),
            "sender-queue-full",
        );
        assert!(matches!(
            respond(
                &mut module,
                "peer",
                capability_request("peer-request", "store")
            ),
            CopperEnvelope::JobAccepted { .. }
        ));
        assert_eq!(handle.pending_count(), 33);
    }

    fn capability_request(request_id: &str, capability: &str) -> serde_json::Value {
        json!({
            "type": "capability-request",
            "protocol": COPPER_BUS_PROTOCOL_V1,
            "requestId": request_id,
            "capability": capability,
            "operation": "get",
            "args": {}
        })
    }

    fn respond(
        module: &mut CopperCapabilityModule,
        sender: &str,
        request: serde_json::Value,
    ) -> CopperEnvelope {
        let payload = serde_json::to_vec(&request).expect("request JSON");
        respond_bytes(module, sender, &payload)
    }

    fn respond_bytes(
        module: &mut CopperCapabilityModule,
        sender: &str,
        payload: &[u8],
    ) -> CopperEnvelope {
        let reply = module.respond(sender, payload).expect("reply");
        serde_json::from_slice(&reply).expect("reply JSON")
    }

    fn assert_error(reply: CopperEnvelope, expected_code: &str) {
        assert!(matches!(
            reply,
            CopperEnvelope::Error { code, .. } if code == expected_code
        ));
    }

    fn write_component(parent: &Path, id: &str, permissions: &[&str]) {
        write_manifest(parent, id, permissions, true);
        fs::write(parent.join(id).join(format!("{id}.wasm")), b"\0asm").expect("component");
    }

    fn write_legacy(parent: &Path, id: &str, permissions: &[&str]) {
        write_manifest(parent, id, permissions, false);
        fs::write(
            parent.join(id).join("main.ts"),
            "export default function(){}",
        )
        .expect("main.ts");
    }

    fn write_manifest(parent: &Path, id: &str, permissions: &[&str], component: bool) {
        let root = parent.join(id);
        fs::create_dir_all(&root).expect("extension root");
        let runtime = component.then(|| {
            format!(
                r#","runtime": {{
                    "kind": "wasm-component",
                    "abi": "{COMPONENT_ABI_V1}",
                    "artifact": "{id}.wasm"
                }}"#
            )
        });
        fs::write(
            root.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "{id}",
                    "version": "1.0.0",
                    "trigger": "{id}",
                    "permissions": {},
                    "actions": [{{ "id": "run", "label": "Run", "script": "run" }}]
                    {}
                }}"#,
                serde_json::to_string(permissions).expect("permissions"),
                runtime.unwrap_or_default()
            ),
        )
        .expect("manifest");
    }
}

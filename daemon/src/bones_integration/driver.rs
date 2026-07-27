use super::jobs::{CapabilityWorker, CompletedCapabilityJob, SubmitError, COPPER_JOB_SENDER};
use super::{
    CopperCapabilityHandle, CopperCapabilityModule, CopperControlHandle, CopperControlModule,
    CopperEnvelope, CopperLifecycleState, COPPER_BUS_PROTOCOL_V1,
};
use crate::extension::Registry;
use crate::state_store::ExtensionStateStore;
use bones_logging::{Level, LogSink, Logger};
use bones_runner::{BuiltEngine, Engine};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

type CatalogSnapshot = BTreeMap<String, PathBuf>;
const MAX_JOB_PUMP_PER_STEP: usize = 32;
pub const COPPER_ACTION_SENDER: &str = "copper-actions";

struct CopperBonesLogSink;

impl LogSink for CopperBonesLogSink {
    fn log(&self, level: Level, category: &str, message: &str) {
        let message = format!("[bones/{category}] {message}");
        match level {
            Level::Debug => {}
            Level::Info | Level::Warn => crate::logging::info(message),
            Level::Error => crate::logging::error(message),
        }
    }
}

/// Serializable health snapshot for Copper's embedded Bones runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BonesRuntimeStatus {
    pub headless: bool,
    pub frames: u64,
    pub registry_reloads: u64,
    pub catalog_extensions: usize,
    pub catalog_rebuilds: u64,
    pub lifecycle_events: usize,
    pub lifecycle_decode_errors: usize,
    pub extensions: BTreeMap<String, CopperLifecycleState>,
    pub capability_accepted: u64,
    pub capability_rejected: u64,
    pub capability_pending: usize,
    pub capability_completed: u64,
    pub capability_failed: u64,
    pub capability_delivery_failures: u64,
    pub actions_dispatched: u64,
    pub action_dispatch_failures: u64,
    pub shutdown: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BonesActionDispatch {
    pub extension_id: String,
    pub action_id: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<CopperEnvelope>,
}

/// Single-threaded, step-driven Bones engine owned by the Copper daemon.
pub struct BonesDaemonDriver {
    engine: BuiltEngine,
    control: CopperControlHandle,
    capabilities: CopperCapabilityHandle,
    capability_worker: CapabilityWorker,
    catalog: CatalogSnapshot,
    frames: u64,
    registry_reloads: u64,
    catalog_rebuilds: u64,
    capability_completed: u64,
    capability_failed: u64,
    capability_delivery_failures: u64,
    next_action_id: u64,
    actions_dispatched: u64,
    action_dispatch_failures: u64,
    shutdown: bool,
}

impl BonesDaemonDriver {
    pub fn new(registry: &Registry, state_store: ExtensionStateStore) -> Result<Self, String> {
        let capability_worker = CapabilityWorker::new(state_store, registry)?;
        let (control_module, control) = CopperControlModule::new();
        let (capability_module, capabilities) = CopperCapabilityModule::new(registry);
        let catalog = catalog_snapshot(registry);
        let mut engine = build_engine(registry, control_module, capability_module)?;
        dispatch_pending(&mut engine);
        Ok(Self {
            engine,
            control,
            capabilities,
            capability_worker,
            catalog,
            frames: 0,
            registry_reloads: 0,
            catalog_rebuilds: 0,
            capability_completed: 0,
            capability_failed: 0,
            capability_delivery_failures: 0,
            next_action_id: 0,
            actions_dispatched: 0,
            action_dispatch_failures: 0,
            shutdown: false,
        })
    }

    /// Replaces the Bones catalog only when component identity or location
    /// changed. In-place component updates remain owned by Bones' transactional
    /// supervisor reload.
    pub fn replace_catalog(&mut self, registry: &Registry) -> Result<bool, String> {
        if self.shutdown {
            return Err("cannot replace the catalog after Bones shutdown".to_string());
        }
        self.registry_reloads = self.registry_reloads.saturating_add(1);
        let catalog = catalog_snapshot(registry);
        if catalog == self.catalog {
            self.capability_worker.replace_registry(registry);
            self.capabilities.replace_registry(registry);
            return Ok(false);
        }

        let module = CopperControlModule::from_handle(self.control.clone());
        let (capability_module, capabilities) = CopperCapabilityModule::new(registry);
        let mut candidate = build_engine(registry, module, capability_module)?;
        self.capability_worker.replace_registry(registry);
        self.engine.shutdown();
        dispatch_pending(&mut candidate);
        self.engine = candidate;
        self.capabilities = capabilities;
        self.catalog = catalog;
        self.catalog_rebuilds = self.catalog_rebuilds.saturating_add(1);
        Ok(true)
    }

    /// Advances Bones once using the daemon loop's measured elapsed time.
    pub fn step(&mut self, elapsed: Duration) {
        if self.shutdown {
            return;
        }
        self.pump_capability_jobs();
        self.engine.supervisor.check();
        self.engine.runner.step(elapsed.as_secs_f32().min(1.0));
        self.engine.supervisor.check();
        self.frames = self.frames.saturating_add(1);
    }

    pub fn status(&self) -> BonesRuntimeStatus {
        BonesRuntimeStatus {
            headless: self.engine.is_headless(),
            frames: self.frames,
            registry_reloads: self.registry_reloads,
            catalog_extensions: self.catalog.len(),
            catalog_rebuilds: self.catalog_rebuilds,
            lifecycle_events: self.control.lifecycle_event_count(),
            lifecycle_decode_errors: self.control.lifecycle_decode_errors(),
            extensions: self.control.extensions(),
            capability_accepted: self.capabilities.accepted_count(),
            capability_rejected: self.capabilities.rejected_count(),
            capability_pending: self.capabilities.pending_count(),
            capability_completed: self.capability_completed,
            capability_failed: self.capability_failed,
            capability_delivery_failures: self.capability_delivery_failures,
            actions_dispatched: self.actions_dispatched,
            action_dispatch_failures: self.action_dispatch_failures,
            shutdown: self.shutdown,
        }
    }

    #[cfg(feature = "native-ui")]
    pub(crate) fn presentation_bus(&self) -> bones_bus::Bus {
        self.engine.runner.bus().clone()
    }

    #[cfg(feature = "native-ui")]
    pub(crate) fn presentation_registry(&self) -> bones_bus::Registry {
        self.engine.supervisor.registry.clone()
    }

    pub fn dispatch_action(
        &mut self,
        extension_id: &str,
        action_id: &str,
        input: serde_json::Map<String, serde_json::Value>,
    ) -> Result<BonesActionDispatch, String> {
        if self.shutdown {
            return Err("cannot dispatch an action after Bones shutdown".to_string());
        }
        if !self.catalog.contains_key(extension_id) {
            return Err(format!(
                "extension '{extension_id}' is not an active Copper WASM component"
            ));
        }
        self.next_action_id = self.next_action_id.saturating_add(1);
        let request_id = format!("action-{}", self.next_action_id);
        let envelope = CopperEnvelope::ActionRequest {
            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
            request_id: request_id.clone(),
            action_id: action_id.to_string(),
            input,
        };
        let payload = match serde_json::to_vec(&envelope) {
            Ok(payload) => payload,
            Err(error) => {
                self.action_dispatch_failures = self.action_dispatch_failures.saturating_add(1);
                return Err(format!("failed to encode action request: {error}"));
            }
        };
        let response =
            match self
                .engine
                .supervisor
                .registry
                .call(COPPER_ACTION_SENDER, extension_id, &payload)
            {
                Ok(response) if response.is_empty() => None,
                Ok(response) => {
                    let envelope = match serde_json::from_slice::<CopperEnvelope>(&response) {
                        Ok(envelope) => envelope,
                        Err(error) => {
                            self.action_dispatch_failures =
                                self.action_dispatch_failures.saturating_add(1);
                            return Err(format!("extension returned an invalid response: {error}"));
                        }
                    };
                    if envelope_protocol(&envelope) != COPPER_BUS_PROTOCOL_V1 {
                        self.action_dispatch_failures =
                            self.action_dispatch_failures.saturating_add(1);
                        return Err("extension returned a mismatched response protocol".to_string());
                    }
                    if envelope_request_id(&envelope) != Some(request_id.as_str()) {
                        self.action_dispatch_failures =
                            self.action_dispatch_failures.saturating_add(1);
                        return Err(
                            "extension returned a mismatched response request ID".to_string()
                        );
                    }
                    if let CopperEnvelope::Error { code, message, .. } = &envelope {
                        self.action_dispatch_failures =
                            self.action_dispatch_failures.saturating_add(1);
                        return Err(format!("extension rejected action [{code}]: {message}"));
                    }
                    if !matches!(
                        &envelope,
                        CopperEnvelope::JobAccepted { .. } | CopperEnvelope::JobResult { .. }
                    ) {
                        self.action_dispatch_failures =
                            self.action_dispatch_failures.saturating_add(1);
                        return Err(
                            "extension returned an invalid action response kind".to_string()
                        );
                    }
                    Some(envelope)
                }
                Err(error) => {
                    self.action_dispatch_failures = self.action_dispatch_failures.saturating_add(1);
                    return Err(format!("failed to dispatch action: {error:?}"));
                }
            };
        self.actions_dispatched = self.actions_dispatched.saturating_add(1);
        Ok(BonesActionDispatch {
            extension_id: extension_id.to_string(),
            action_id: action_id.to_string(),
            request_id,
            response,
        })
    }

    #[cfg(test)]
    pub(crate) fn insert_test_responder(
        &mut self,
        extension_id: &str,
        responder: Arc<dyn bones_bus::Respond>,
    ) {
        self.engine
            .supervisor
            .registry
            .insert(extension_id, responder);
    }

    fn pump_capability_jobs(&mut self) {
        for _ in 0..MAX_JOB_PUMP_PER_STEP {
            let Some(completed) = self.capability_worker.try_complete() else {
                break;
            };
            self.deliver_capability_result(completed);
        }

        for _ in 0..MAX_JOB_PUMP_PER_STEP {
            let Some(job) = self.capabilities.pop_pending() else {
                break;
            };
            match self.capability_worker.try_submit(job) {
                Ok(()) => {}
                Err(SubmitError::Full(job)) => {
                    self.capabilities.requeue_front(*job);
                    break;
                }
                Err(SubmitError::Disconnected(job)) => {
                    self.deliver_capability_result(CompletedCapabilityJob::worker_unavailable(
                        *job,
                    ));
                    break;
                }
            }
        }
    }

    fn deliver_capability_result(&mut self, completed: CompletedCapabilityJob) {
        if completed.succeeded {
            self.capability_completed = self.capability_completed.saturating_add(1);
        } else {
            self.capability_failed = self.capability_failed.saturating_add(1);
        }
        let Ok(payload) = serde_json::to_vec(&completed.envelope) else {
            self.capability_delivery_failures = self.capability_delivery_failures.saturating_add(1);
            return;
        };
        if self
            .engine
            .supervisor
            .registry
            .call(COPPER_JOB_SENDER, &completed.extension_id, &payload)
            .is_err()
        {
            self.capability_delivery_failures = self.capability_delivery_failures.saturating_add(1);
        }
    }

    pub fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.engine.shutdown();
        self.shutdown = true;
    }
}

fn envelope_protocol(envelope: &CopperEnvelope) -> &str {
    match envelope {
        CopperEnvelope::ActionRequest { protocol, .. }
        | CopperEnvelope::CapabilityRequest { protocol, .. }
        | CopperEnvelope::JobAccepted { protocol, .. }
        | CopperEnvelope::JobResult { protocol, .. }
        | CopperEnvelope::Error { protocol, .. } => protocol,
    }
}

fn envelope_request_id(envelope: &CopperEnvelope) -> Option<&str> {
    match envelope {
        CopperEnvelope::ActionRequest { request_id, .. }
        | CopperEnvelope::CapabilityRequest { request_id, .. }
        | CopperEnvelope::JobAccepted { request_id, .. }
        | CopperEnvelope::JobResult { request_id, .. } => Some(request_id),
        CopperEnvelope::Error { request_id, .. } => request_id.as_deref(),
    }
}

fn build_engine(
    registry: &Registry,
    control_module: CopperControlModule,
    capability_module: CopperCapabilityModule,
) -> Result<BuiltEngine, String> {
    let mut builder = Engine::new()
        .logger(Logger::new(Arc::new(CopperBonesLogSink)))
        .module(control_module)
        .module(capability_module)
        .extension_controller(super::COPPER_CONTROL_ENDPOINT)
        .read_only_persistence()
        .saves_dir(crate::extension::default_extensions_dir().join("copper-core/bones-saves"));
    for extension in registry.list() {
        if let Some(path) = extension.wasm_component_path() {
            builder = builder
                .catalog_extension(&extension.descriptor.id, path)
                .startup_extension(&extension.descriptor.id);
        }
    }
    let engine = builder
        .build()
        .map_err(|err| format!("failed to build headless Bones engine: {err}"))?;
    if !engine.is_headless() {
        return Err("Copper daemon constructed a non-headless Bones engine".to_string());
    }
    Ok(engine)
}

fn catalog_snapshot(registry: &Registry) -> CatalogSnapshot {
    let mut snapshot = BTreeMap::new();
    for extension in registry.list() {
        if let Some(path) = extension.wasm_component_path() {
            snapshot.insert(extension.descriptor.id.clone(), path.to_path_buf());
        }
    }
    snapshot
}

fn dispatch_pending(engine: &mut BuiltEngine) {
    engine.runner.begin_frame();
    engine.runner.bus().dispatch();
    engine.supervisor.check();
}

impl Drop for BonesDaemonDriver {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{BonesDaemonDriver, COPPER_ACTION_SENDER, COPPER_JOB_SENDER};
    use crate::bones_integration::{
        CopperEnvelope, CopperLifecycleState, COPPER_BUS_PROTOCOL_V1, COPPER_CAPABILITY_ENDPOINT,
    };
    use crate::core_config::CoreConfig;
    use crate::descriptor::COMPONENT_ABI_V1;
    use crate::extension::{current_platform, Registry};
    use crate::state_store::ExtensionStateStore;
    use bones_bus::Respond;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn driver_is_headless_step_driven_and_shutdown_is_idempotent() {
        let temp = tempdir().expect("tempdir");
        let registry = Registry::load_from_dir(temp.path()).expect("empty registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        let mut driver = BonesDaemonDriver::new(&registry, store).expect("headless engine");
        assert!(driver.status().headless);
        assert_eq!(driver.status().frames, 0);

        driver.step(Duration::from_millis(20));
        assert!(!driver.replace_catalog(&registry).expect("same catalog"));
        assert_eq!(driver.status().frames, 1);
        assert_eq!(driver.status().registry_reloads, 1);
        assert_eq!(driver.status().catalog_rebuilds, 0);

        driver.shutdown();
        driver.shutdown();
        driver.step(Duration::from_millis(20));
        assert!(driver.status().shutdown);
        assert_eq!(driver.status().frames, 1);
    }

    #[test]
    fn driver_maps_component_catalog_and_observes_lifecycle_faults() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "broken-component");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");

        let store = ExtensionStateStore::new(temp.path().join("state"));
        let mut driver = BonesDaemonDriver::new(&registry, store).expect("driver");
        let status = driver.status();
        assert_eq!(status.catalog_extensions, 1);
        assert_eq!(
            status.extensions.get("broken-component"),
            Some(&CopperLifecycleState::Faulted)
        );

        fs::write(
            temp.path().join("broken-component/broken-component.wasm"),
            b"\0asm-updated",
        )
        .expect("updated component");
        let updated_registry = Registry::load_from_dir(temp.path()).expect("updated registry");
        assert!(
            !driver
                .replace_catalog(&updated_registry)
                .expect("stable catalog"),
            "in-place component replacement must stay with Bones' supervisor"
        );
        assert_eq!(driver.status().catalog_rebuilds, 0);
    }

    #[test]
    fn driver_catalog_uses_coppers_filtered_runtime_registry() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "enabled-component");
        write_component_extension(temp.path(), "disabled-component");
        let registry = Registry::load_from_dir(temp.path())
            .expect("registry")
            .filter_for_runtime(
                &CoreConfig {
                    disabled_extensions: BTreeSet::from(["disabled-component".to_string()]),
                },
                current_platform(),
            );

        let store = ExtensionStateStore::new(temp.path().join("state"));
        let driver = BonesDaemonDriver::new(&registry, store).expect("driver");
        let status = driver.status();
        assert_eq!(status.catalog_extensions, 1);
        assert!(status.extensions.contains_key("enabled-component"));
        assert!(!status.extensions.contains_key("disabled-component"));
    }

    #[test]
    fn driver_pumps_state_jobs_off_loop_and_delivers_targeted_results() {
        let temp = tempdir().expect("tempdir");
        write_component_extension_with_permissions(temp.path(), "state-component", &["store"]);
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        let mut driver = BonesDaemonDriver::new(&registry, store.clone()).expect("driver");
        let capture = Capture::default();
        driver
            .engine
            .supervisor
            .registry
            .insert("state-component", Arc::new(capture.clone()));

        let request = serde_json::to_vec(&serde_json::json!({
            "type": "capability-request",
            "protocol": COPPER_BUS_PROTOCOL_V1,
            "requestId": "request-1",
            "capability": "store",
            "operation": "set",
            "args": { "key": "count", "value": 7 }
        }))
        .expect("request");
        let reply = driver
            .engine
            .supervisor
            .registry
            .call("state-component", COPPER_CAPABILITY_ENDPOINT, &request)
            .expect("capability call");
        assert!(matches!(
            serde_json::from_slice(&reply).expect("accepted reply"),
            CopperEnvelope::JobAccepted { .. }
        ));

        for _ in 0..100 {
            driver.step(Duration::from_millis(1));
            if driver.status().capability_completed == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(driver.status().capability_completed, 1);
        assert_eq!(driver.status().capability_delivery_failures, 0);
        assert_eq!(capture.last_sender().as_deref(), Some(COPPER_JOB_SENDER));
        assert_eq!(
            store.load_store("state-component").expect("state")["count"],
            serde_json::json!(7)
        );
        assert!(matches!(
            serde_json::from_slice(&capture.last().expect("targeted result"))
                .expect("result envelope"),
            CopperEnvelope::JobResult { .. }
        ));
    }

    #[test]
    fn driver_dispatches_versioned_actions_to_one_target_component() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "action-component");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        let mut driver = BonesDaemonDriver::new(&registry, store).expect("driver");
        let capture = Capture::default();
        driver
            .engine
            .supervisor
            .registry
            .insert("action-component", Arc::new(capture.clone()));

        let dispatch = driver
            .dispatch_action(
                "action-component",
                "run",
                serde_json::Map::from_iter([("source".to_string(), serde_json::json!("cli"))]),
            )
            .expect("dispatch");

        assert_eq!(dispatch.extension_id, "action-component");
        assert_eq!(dispatch.action_id, "run");
        assert_eq!(capture.last_sender().as_deref(), Some(COPPER_ACTION_SENDER));
        assert!(matches!(
            serde_json::from_slice(&capture.last().expect("action request"))
                .expect("request envelope"),
            CopperEnvelope::ActionRequest {
                protocol,
                action_id,
                input,
                ..
            } if protocol == COPPER_BUS_PROTOCOL_V1
                && action_id == "run"
                && input["source"] == serde_json::json!("cli")
        ));
        assert_eq!(driver.status().actions_dispatched, 1);
        assert_eq!(driver.status().action_dispatch_failures, 0);
    }

    #[test]
    fn driver_rejects_wrong_action_response_kinds() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "action-component");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        let mut driver = BonesDaemonDriver::new(&registry, store).expect("driver");
        let response = serde_json::to_vec(&CopperEnvelope::ActionRequest {
            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
            request_id: "action-1".to_string(),
            action_id: "nested".to_string(),
            input: serde_json::Map::new(),
        })
        .expect("response");
        driver.engine.supervisor.registry.insert(
            "action-component",
            Arc::new(Capture::with_response(response)),
        );

        let error = driver
            .dispatch_action("action-component", "run", serde_json::Map::new())
            .expect_err("wrong response kind");
        assert!(error.contains("invalid action response kind"));
        assert_eq!(driver.status().actions_dispatched, 0);
        assert_eq!(driver.status().action_dispatch_failures, 1);
    }

    #[test]
    fn driver_rejects_action_responses_for_another_request() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "action-component");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        let mut driver = BonesDaemonDriver::new(&registry, store).expect("driver");
        let response = serde_json::to_vec(&CopperEnvelope::JobAccepted {
            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
            request_id: "stale-request".to_string(),
            job_id: "job-1".to_string(),
        })
        .expect("response");
        driver.engine.supervisor.registry.insert(
            "action-component",
            Arc::new(Capture::with_response(response)),
        );

        let error = driver
            .dispatch_action("action-component", "run", serde_json::Map::new())
            .expect_err("mismatched response");
        assert!(error.contains("mismatched response request ID"));
        assert_eq!(driver.status().actions_dispatched, 0);
        assert_eq!(driver.status().action_dispatch_failures, 1);
    }

    fn write_component_extension(parent: &Path, id: &str) {
        write_component_extension_with_permissions(parent, id, &[]);
    }

    fn write_component_extension_with_permissions(parent: &Path, id: &str, permissions: &[&str]) {
        let root = parent.join(id);
        fs::create_dir_all(&root).expect("extension root");
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
                    "runtime": {{
                        "kind": "wasm-component",
                        "abi": "{COMPONENT_ABI_V1}",
                        "artifact": "{id}.wasm"
                    }},
                    "actions": [{{ "id": "run", "label": "Run", "script": "run" }}]
                }}"#,
                serde_json::to_string(permissions).expect("permissions")
            ),
        )
        .expect("manifest");
        fs::write(root.join(format!("{id}.wasm")), b"\0asm").expect("component");
    }

    #[derive(Clone, Default)]
    struct Capture {
        payloads: Arc<Mutex<Vec<Vec<u8>>>>,
        senders: Arc<Mutex<Vec<String>>>,
        response: Arc<Mutex<Option<Vec<u8>>>>,
    }

    impl Capture {
        fn with_response(response: Vec<u8>) -> Self {
            Self {
                response: Arc::new(Mutex::new(Some(response))),
                ..Self::default()
            }
        }

        fn last(&self) -> Option<Vec<u8>> {
            self.payloads.lock().ok()?.last().cloned()
        }

        fn last_sender(&self) -> Option<String> {
            self.senders.lock().ok()?.last().cloned()
        }
    }

    impl Respond for Capture {
        fn respond(&self, sender: &str, payload: &[u8]) -> Option<Vec<u8>> {
            self.senders.lock().ok()?.push(sender.to_string());
            self.payloads.lock().ok()?.push(payload.to_vec());
            Some(self.response.lock().ok()?.clone().unwrap_or_default())
        }
    }
}

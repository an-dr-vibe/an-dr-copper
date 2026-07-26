use super::capability::{build_policies, is_capability_authorized, CapabilityPolicies};
use super::{AuthorizedCapabilityJob, Capability, CopperEnvelope, COPPER_BUS_PROTOCOL_V1};
use crate::extension::Registry;
use crate::state_store::ExtensionStateStore;
use serde_json::{Map, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

const WORKER_QUEUE_CAPACITY: usize = 64;
pub(super) const COPPER_JOB_SENDER: &str = "copper-jobs";

pub(super) struct CompletedCapabilityJob {
    pub extension_id: String,
    pub envelope: CopperEnvelope,
    pub succeeded: bool,
}

impl CompletedCapabilityJob {
    pub fn worker_unavailable(job: AuthorizedCapabilityJob) -> Self {
        Self {
            extension_id: job.extension_id,
            envelope: CopperEnvelope::Error {
                protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
                request_id: Some(job.request_id),
                job_id: Some(job.job_id),
                code: "worker-unavailable".to_string(),
                message: "capability worker is unavailable".to_string(),
            },
            succeeded: false,
        }
    }
}

#[derive(Debug)]
pub(super) enum SubmitError {
    Full(Box<AuthorizedCapabilityJob>),
    Disconnected(Box<AuthorizedCapabilityJob>),
}

/// Single worker that keeps state I/O off the step-driven Bones thread.
pub(super) struct CapabilityWorker {
    sender: Option<SyncSender<AuthorizedCapabilityJob>>,
    completed: Receiver<CompletedCapabilityJob>,
    policies: Arc<Mutex<CapabilityPolicies>>,
    join: Option<JoinHandle<()>>,
}

impl CapabilityWorker {
    pub fn new(state_store: ExtensionStateStore, registry: &Registry) -> Result<Self, String> {
        let (sender, jobs) = sync_channel::<AuthorizedCapabilityJob>(WORKER_QUEUE_CAPACITY);
        let (results, completed) = channel();
        let policies = Arc::new(Mutex::new(build_policies(registry)));
        let worker_policies = policies.clone();
        let join = thread::Builder::new()
            .name("copper-capability-worker".to_string())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let panic_job = job.clone();
                    let completion = catch_unwind(AssertUnwindSafe(|| {
                        execute_state_job(&state_store, &worker_policies, job)
                    }))
                    .unwrap_or_else(|_| CompletedCapabilityJob {
                        extension_id: panic_job.extension_id,
                        envelope: CopperEnvelope::Error {
                            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
                            request_id: Some(panic_job.request_id),
                            job_id: Some(panic_job.job_id),
                            code: "worker-panic".to_string(),
                            message: "capability worker panicked".to_string(),
                        },
                        succeeded: false,
                    });
                    if results.send(completion).is_err() {
                        break;
                    }
                }
            })
            .map_err(|err| format!("failed to start capability worker: {err}"))?;
        Ok(Self {
            sender: Some(sender),
            completed,
            policies,
            join: Some(join),
        })
    }

    pub fn replace_registry(&self, registry: &Registry) {
        if let Ok(mut policies) = self.policies.lock() {
            *policies = build_policies(registry);
        }
    }

    pub fn try_submit(&self, job: AuthorizedCapabilityJob) -> Result<(), SubmitError> {
        let Some(sender) = &self.sender else {
            return Err(SubmitError::Disconnected(Box::new(job)));
        };
        sender.try_send(job).map_err(|err| match err {
            TrySendError::Full(job) => SubmitError::Full(Box::new(job)),
            TrySendError::Disconnected(job) => SubmitError::Disconnected(Box::new(job)),
        })
    }

    pub fn try_complete(&self) -> Option<CompletedCapabilityJob> {
        self.completed.try_recv().ok()
    }
}

impl Drop for CapabilityWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn execute_state_job(
    store: &ExtensionStateStore,
    policies: &Arc<Mutex<CapabilityPolicies>>,
    job: AuthorizedCapabilityJob,
) -> CompletedCapabilityJob {
    let result = if !job_remains_authorized(policies, &job) {
        Err((
            "permission-revoked",
            "manifest permission was revoked before execution".to_string(),
        ))
    } else {
        match job.capability {
            Capability::Store => execute_store_operation(store, &job),
            _ => Err((
                "unsupported-capability",
                "capability has no native worker handler yet".to_string(),
            )),
        }
    };
    let (envelope, succeeded) = match result {
        Ok(result) => (
            CopperEnvelope::JobResult {
                protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
                request_id: job.request_id.clone(),
                job_id: job.job_id.clone(),
                result,
            },
            true,
        ),
        Err((code, message)) => (
            CopperEnvelope::Error {
                protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
                request_id: Some(job.request_id.clone()),
                job_id: Some(job.job_id.clone()),
                code: code.to_string(),
                message,
            },
            false,
        ),
    };
    CompletedCapabilityJob {
        extension_id: job.extension_id,
        envelope,
        succeeded,
    }
}

fn job_remains_authorized(
    policies: &Arc<Mutex<CapabilityPolicies>>,
    job: &AuthorizedCapabilityJob,
) -> bool {
    policies
        .lock()
        .ok()
        .and_then(|policies| policies.get(&job.extension_id).cloned())
        .is_some_and(|permissions| is_capability_authorized(&permissions, job.capability))
}

fn execute_store_operation(
    store: &ExtensionStateStore,
    job: &AuthorizedCapabilityJob,
) -> Result<Value, (&'static str, String)> {
    let extension_id = &job.extension_id;
    match job.operation.as_str() {
        "get" => {
            let key = read_key(&job.args)?;
            let state = store.load_store(extension_id).map_err(state_io_error)?;
            Ok(state.get(key).cloned().unwrap_or(Value::Null))
        }
        "set" => {
            let key = read_key(&job.args)?;
            let value = job
                .args
                .get("value")
                .cloned()
                .ok_or_else(|| ("invalid-args", "store.set requires a value".to_string()))?;
            store
                .set_store_value(extension_id, key, value)
                .map_err(state_io_error)?;
            Ok(Value::Null)
        }
        "config.get" => store.load_config(extension_id).map_err(state_io_error),
        "config.merge" => {
            let value = read_object(&job.args)?;
            store
                .merge_config(extension_id, &Value::Object(value))
                .map_err(state_io_error)
        }
        "status.get" => store.load_status(extension_id).map_err(state_io_error),
        "status.merge" => {
            let value = read_object(&job.args)?;
            store
                .merge_status(extension_id, &Value::Object(value))
                .map_err(state_io_error)
        }
        operation => Err((
            "unknown-operation",
            format!("unsupported store operation '{operation}'"),
        )),
    }
}

fn read_key(args: &Map<String, Value>) -> Result<&str, (&'static str, String)> {
    let key = args.get("key").and_then(Value::as_str).unwrap_or_default();
    if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
        return Err((
            "invalid-args",
            "store key must be 1-256 characters without control characters".to_string(),
        ));
    }
    Ok(key)
}

fn read_object(args: &Map<String, Value>) -> Result<Map<String, Value>, (&'static str, String)> {
    args.get("value")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| {
            (
                "invalid-args",
                "state merge requires an object value".to_string(),
            )
        })
}

fn state_io_error(error: std::io::Error) -> (&'static str, String) {
    ("state-io", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::CapabilityWorker;
    use crate::bones_integration::{AuthorizedCapabilityJob, Capability, CopperEnvelope};
    use crate::descriptor::COMPONENT_ABI_V1;
    use crate::extension::Registry;
    use crate::state_store::ExtensionStateStore;
    use serde_json::{json, Map};
    use std::fs;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn worker_roundtrips_store_and_scopes_state_to_stamped_extension() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        write_store_component(temp.path(), "alpha");
        write_store_component(temp.path(), "beta");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let worker = CapabilityWorker::new(store.clone(), &registry).expect("worker");
        worker
            .try_submit(job(
                "alpha",
                "request-1",
                "set",
                json!({"key":"count","value":1}),
            ))
            .expect("submit alpha");
        worker
            .try_submit(job(
                "beta",
                "request-2",
                "set",
                json!({"key":"count","value":9,"extensionId":"alpha"}),
            ))
            .expect("submit beta");
        let first = wait_for_completion(&worker);
        let second = wait_for_completion(&worker);

        assert!(first.succeeded && second.succeeded);
        assert_eq!(
            store.load_store("alpha").expect("alpha state")["count"],
            json!(1)
        );
        assert_eq!(
            store.load_store("beta").expect("beta state")["count"],
            json!(9)
        );
    }

    #[test]
    fn worker_returns_versioned_errors_for_invalid_or_unsupported_jobs() {
        let temp = tempdir().expect("tempdir");
        write_store_component(temp.path(), "alpha");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let worker = CapabilityWorker::new(
            ExtensionStateStore::new(temp.path().join("state")),
            &registry,
        )
        .expect("worker");
        worker
            .try_submit(job("alpha", "request-1", "set", json!({})))
            .expect("submit invalid");
        let invalid = wait_for_completion(&worker);
        assert!(matches!(
            invalid.envelope,
            CopperEnvelope::Error { code, job_id: Some(_), .. } if code == "invalid-args"
        ));

        let mut unsupported = job("alpha", "request-2", "run", json!({}));
        unsupported.capability = Capability::Shell;
        worker.try_submit(unsupported).expect("submit unsupported");
        assert!(matches!(
            wait_for_completion(&worker).envelope,
            CopperEnvelope::Error { code, .. } if code == "unsupported-capability"
        ));
    }

    #[test]
    fn worker_rechecks_permission_before_executing_queued_work() {
        let temp = tempdir().expect("tempdir");
        write_store_component(temp.path(), "alpha");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let worker = CapabilityWorker::new(
            ExtensionStateStore::new(temp.path().join("state")),
            &registry,
        )
        .expect("worker");
        let empty = Registry::load_from_dir(&temp.path().join("empty")).expect("empty registry");
        worker.replace_registry(&empty);
        worker
            .try_submit(job(
                "alpha",
                "request-1",
                "set",
                json!({"key":"count","value":1}),
            ))
            .expect("submit revoked");

        assert!(matches!(
            wait_for_completion(&worker).envelope,
            CopperEnvelope::Error { code, .. } if code == "permission-revoked"
        ));
    }

    fn job(
        extension_id: &str,
        request_id: &str,
        operation: &str,
        args: serde_json::Value,
    ) -> AuthorizedCapabilityJob {
        AuthorizedCapabilityJob {
            job_id: format!("job-{request_id}"),
            extension_id: extension_id.to_string(),
            request_id: request_id.to_string(),
            capability: Capability::Store,
            operation: operation.to_string(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        }
    }

    fn wait_for_completion(worker: &CapabilityWorker) -> super::CompletedCapabilityJob {
        for _ in 0..100 {
            if let Some(completed) = worker.try_complete() {
                return completed;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("worker did not complete");
    }

    fn write_store_component(parent: &Path, id: &str) {
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
                    "permissions": ["store", "shell"],
                    "runtime": {{
                        "kind": "wasm-component",
                        "abi": "{COMPONENT_ABI_V1}",
                        "artifact": "{id}.wasm"
                    }},
                    "actions": [{{ "id": "run", "label": "Run", "script": "run" }}]
                }}"#
            ),
        )
        .expect("manifest");
        fs::write(root.join(format!("{id}.wasm")), b"\0asm").expect("component");
    }
}

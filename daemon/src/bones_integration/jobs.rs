use super::capability::{build_policies, is_capability_authorized, CapabilityPolicies};
use super::platform_jobs::{
    execute_keyboard_operation, execute_secure_store_operation, execute_windows_display_operation,
};
use super::{AuthorizedCapabilityJob, Capability, CopperEnvelope, COPPER_BUS_PROTOCOL_V1};
use crate::api;
use crate::extension::Registry;
use crate::state_store::ExtensionStateStore;
use serde_json::{Map, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{
    channel, sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError,
};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const WORKER_QUEUE_CAPACITY: usize = 64;
const WORKER_SHUTDOWN_GRACE: Duration = Duration::from_millis(250);
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
    stopped: Receiver<()>,
    policies: Arc<Mutex<CapabilityPolicies>>,
    join: Option<JoinHandle<()>>,
}

impl CapabilityWorker {
    pub fn new(state_store: ExtensionStateStore, registry: &Registry) -> Result<Self, String> {
        let (sender, jobs) = sync_channel::<AuthorizedCapabilityJob>(WORKER_QUEUE_CAPACITY);
        let (results, completed) = channel();
        let (stopped_sender, stopped) = sync_channel(1);
        let policies = Arc::new(Mutex::new(build_policies(registry)));
        let worker_policies = policies.clone();
        let join = thread::Builder::new()
            .name("copper-capability-worker".to_string())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let panic_job = job.clone();
                    let completion = catch_unwind(AssertUnwindSafe(|| {
                        execute_capability_job(&state_store, &worker_policies, job)
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
                let _ = stopped_sender.send(());
            })
            .map_err(|err| format!("failed to start capability worker: {err}"))?;
        Ok(Self {
            sender: Some(sender),
            completed,
            stopped,
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
        match self.stopped.recv_timeout(WORKER_SHUTDOWN_GRACE) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(join) = self.join.take() {
                    let _ = join.join();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // A native operation may still be blocking. Detach it so daemon
                // shutdown is bounded; process exit will terminate the worker.
                self.join.take();
            }
        }
    }
}

fn execute_capability_job(
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
            Capability::Fs => execute_fs_operation(&job),
            Capability::Keyboard => execute_keyboard_operation(&job),
            Capability::Notify => execute_notify_operation(&job),
            Capability::SecureStore => execute_secure_store_operation(&job),
            Capability::Shell => execute_shell_operation(&job),
            Capability::Store => execute_store_operation(store, &job),
            Capability::Ui => execute_ui_operation(&job),
            Capability::WindowsDisplay => execute_windows_display_operation(&job),
            Capability::Network => Err((
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

pub(super) type JobResult = Result<Value, (&'static str, String)>;

fn execute_fs_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    match job.operation.as_str() {
        "list" => {
            let path = read_path(&job.args, "path")?;
            serde_json::to_value(api::fs::list(path))
                .map_err(|error| ("result-encoding", error.to_string()))
        }
        "move" => {
            let source = read_path(&job.args, "src")?;
            let destination = read_path(&job.args, "dst")?;
            api::fs::move_file(source, destination)
                .map(|()| Value::Null)
                .map_err(native_io_error)
        }
        "delete" => {
            let path = read_path(&job.args, "path")?;
            api::fs::delete(path)
                .map(|()| Value::Null)
                .map_err(native_io_error)
        }
        operation => unknown_operation("filesystem", operation),
    }
}

fn execute_shell_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    match job.operation.as_str() {
        "run" => {
            let command = read_string(&job.args, "cmd", 4096)?;
            let args = read_string_array(&job.args, "args", 128, 8192)?;
            let result = api::shell::run(command, &args);
            Ok(serde_json::json!({
                "code": result.code,
                "stdout": result.stdout,
                "stderr": result.stderr,
            }))
        }
        "which" => {
            let binary = read_binary_name(&job.args)?;
            Ok(api::shell::which(binary)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        operation => unknown_operation("shell", operation),
    }
}

fn execute_notify_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    match job.operation.as_str() {
        "show" => {
            let message = read_string(&job.args, "message", 8192)?;
            api::notify::notify(message);
            Ok(Value::Null)
        }
        operation => unknown_operation("notification", operation),
    }
}

fn execute_ui_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    match job.operation.as_str() {
        "show" => {
            let markup = read_object_field(&job.args, "markup")?;
            api::ui::show(&Value::Object(markup));
            Ok(Value::Null)
        }
        "update" => {
            let state = read_object_field(&job.args, "state")?;
            api::ui::update(&Value::Object(state));
            Ok(Value::Null)
        }
        operation => unknown_operation("ui", operation),
    }
}

fn execute_store_operation(
    store: &ExtensionStateStore,
    job: &AuthorizedCapabilityJob,
) -> JobResult {
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

fn read_path<'a>(
    args: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, (&'static str, String)> {
    let path = read_string(args, field, 32 * 1024)?;
    if path.chars().any(char::is_control) {
        return Err((
            "invalid-args",
            format!("'{field}' must not contain control characters"),
        ));
    }
    Ok(path)
}

fn read_binary_name(args: &Map<String, Value>) -> Result<&str, (&'static str, String)> {
    let binary = read_string(args, "binary", 256)?;
    if !binary
        .chars()
        .all(|character| character.is_alphanumeric() || "._+-".contains(character))
    {
        return Err((
            "invalid-args",
            "'binary' must be a program name without path or option characters".to_string(),
        ));
    }
    Ok(binary)
}

pub(super) fn read_string<'a>(
    args: &'a Map<String, Value>,
    field: &str,
    max_bytes: usize,
) -> Result<&'a str, (&'static str, String)> {
    let Some(value) = args.get(field).and_then(Value::as_str) else {
        return Err((
            "invalid-args",
            format!("'{field}' must be a non-empty string"),
        ));
    };
    if value.is_empty() || value.len() > max_bytes || value.contains('\0') {
        return Err((
            "invalid-args",
            format!("'{field}' must be 1-{max_bytes} bytes without NUL characters"),
        ));
    }
    Ok(value)
}

fn read_string_array(
    args: &Map<String, Value>,
    field: &str,
    max_items: usize,
    max_item_bytes: usize,
) -> Result<Vec<String>, (&'static str, String)> {
    let Some(items) = args.get(field).and_then(Value::as_array) else {
        return Err((
            "invalid-args",
            format!("'{field}' must be an array of strings"),
        ));
    };
    if items.len() > max_items {
        return Err((
            "invalid-args",
            format!("'{field}' must contain at most {max_items} items"),
        ));
    }
    items
        .iter()
        .map(|item| {
            let Some(value) = item.as_str() else {
                return Err((
                    "invalid-args",
                    format!("'{field}' must contain only strings"),
                ));
            };
            if value.len() > max_item_bytes || value.contains('\0') {
                return Err((
                    "invalid-args",
                    format!(
                        "'{field}' items must be at most {max_item_bytes} bytes without NUL characters"
                    ),
                ));
            }
            Ok(value.to_string())
        })
        .collect()
}

fn read_object(args: &Map<String, Value>) -> Result<Map<String, Value>, (&'static str, String)> {
    read_object_field(args, "value")
}

pub(super) fn read_object_field(
    args: &Map<String, Value>,
    field: &str,
) -> Result<Map<String, Value>, (&'static str, String)> {
    args.get(field)
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| ("invalid-args", format!("'{field}' must be an object")))
}

fn state_io_error(error: std::io::Error) -> (&'static str, String) {
    ("state-io", error.to_string())
}

fn native_io_error(error: std::io::Error) -> (&'static str, String) {
    ("native-io", error.to_string())
}

fn unknown_operation(family: &str, operation: &str) -> JobResult {
    Err((
        "unknown-operation",
        format!("unsupported {family} operation '{operation}'"),
    ))
}

#[cfg(test)]
mod tests {
    use super::CapabilityWorker;
    use crate::bones_integration::{AuthorizedCapabilityJob, Capability, CopperEnvelope};
    use crate::descriptor::COMPONENT_ABI_V1;
    use crate::extension::Registry;
    use crate::state_store::ExtensionStateStore;
    use serde_json::{json, Map, Value};
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};
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
    fn worker_returns_versioned_errors_for_invalid_jobs() {
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

        let mut invalid_shell = job("alpha", "request-2", "run", json!({}));
        invalid_shell.capability = Capability::Shell;
        worker
            .try_submit(invalid_shell)
            .expect("submit invalid shell job");
        assert!(matches!(
            wait_for_completion(&worker).envelope,
            CopperEnvelope::Error { code, .. } if code == "invalid-args"
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

    #[test]
    fn worker_executes_filesystem_shell_and_ui_operations() {
        let temp = tempdir().expect("tempdir");
        let working = temp.path().join("working");
        fs::create_dir_all(&working).expect("working directory");
        fs::write(working.join("source.txt"), "data").expect("source");
        write_native_component(temp.path(), "alpha");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let worker = CapabilityWorker::new(
            ExtensionStateStore::new(temp.path().join("state")),
            &registry,
        )
        .expect("worker");

        worker
            .try_submit(capability_job(
                "alpha",
                "request-list",
                Capability::Fs,
                "list",
                json!({"path": working}),
            ))
            .expect("list");
        let listed = wait_for_completion(&worker);
        assert!(matches!(
            listed.envelope,
            CopperEnvelope::JobResult { result, .. }
                if result.as_array().is_some_and(|entries| entries.iter().any(
                    |entry| entry["name"] == json!("source.txt") && entry["isDir"] == json!(false)
                ))
        ));

        let destination = working.join("destination.txt");
        worker
            .try_submit(capability_job(
                "alpha",
                "request-move",
                Capability::Fs,
                "move",
                json!({"src": working.join("source.txt"), "dst": destination}),
            ))
            .expect("move");
        assert!(wait_for_completion(&worker).succeeded);
        assert!(destination.exists());

        worker
            .try_submit(capability_job(
                "alpha",
                "request-delete",
                Capability::Fs,
                "delete",
                json!({"path": destination}),
            ))
            .expect("delete");
        assert!(wait_for_completion(&worker).succeeded);
        assert!(!destination.exists());

        let (command, arguments) = if cfg!(windows) {
            ("cmd", json!(["/c", "echo copper"]))
        } else {
            ("sh", json!(["-c", "printf copper"]))
        };
        worker
            .try_submit(capability_job(
                "alpha",
                "request-run",
                Capability::Shell,
                "run",
                json!({"cmd": command, "args": arguments}),
            ))
            .expect("run");
        assert!(matches!(
            wait_for_completion(&worker).envelope,
            CopperEnvelope::JobResult { result, .. }
                if result["code"] == json!(0)
                    && result["stdout"].as_str().is_some_and(|output| output.contains("copper"))
        ));

        worker
            .try_submit(capability_job(
                "alpha",
                "request-which",
                Capability::Shell,
                "which",
                json!({"binary": if cfg!(windows) { "cmd" } else { "sh" }}),
            ))
            .expect("which");
        assert!(matches!(
            wait_for_completion(&worker).envelope,
            CopperEnvelope::JobResult {
                result: Value::String(_),
                ..
            }
        ));

        worker
            .try_submit(capability_job(
                "alpha",
                "request-notify",
                Capability::Notify,
                "show",
                json!({"message": "Copper capability test completed"}),
            ))
            .expect("notify");
        assert!(wait_for_completion(&worker).succeeded);

        worker
            .try_submit(capability_job(
                "alpha",
                "request-ui",
                Capability::Ui,
                "show",
                json!({"markup": {"type": "toast", "message": "done"}}),
            ))
            .expect("ui");
        assert!(wait_for_completion(&worker).succeeded);

        worker
            .try_submit(capability_job(
                "alpha",
                "request-ui-update",
                Capability::Ui,
                "update",
                json!({"state": {"progress": 1}}),
            ))
            .expect("ui update");
        assert!(wait_for_completion(&worker).succeeded);
    }

    #[test]
    fn worker_rejects_invalid_native_capability_arguments() {
        let temp = tempdir().expect("tempdir");
        write_native_component(temp.path(), "alpha");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let worker = CapabilityWorker::new(
            ExtensionStateStore::new(temp.path().join("state")),
            &registry,
        )
        .expect("worker");

        for (request_id, capability, operation, args) in [
            ("request-fs", Capability::Fs, "list", json!({"path": ""})),
            (
                "request-shell",
                Capability::Shell,
                "run",
                json!({"cmd": "tool", "args": "not-an-array"}),
            ),
            (
                "request-notify",
                Capability::Notify,
                "show",
                json!({"message": ""}),
            ),
            ("request-ui", Capability::Ui, "update", json!({"state": []})),
        ] {
            worker
                .try_submit(capability_job(
                    "alpha", request_id, capability, operation, args,
                ))
                .expect("submit invalid");
            assert!(matches!(
                wait_for_completion(&worker).envelope,
                CopperEnvelope::Error { code, .. } if code == "invalid-args"
            ));
        }
    }

    #[test]
    fn worker_drop_does_not_wait_for_a_blocking_native_job() {
        let temp = tempdir().expect("tempdir");
        write_native_component(temp.path(), "alpha");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let worker = CapabilityWorker::new(
            ExtensionStateStore::new(temp.path().join("state")),
            &registry,
        )
        .expect("worker");
        let (command, arguments) = if cfg!(windows) {
            ("ping", json!(["-n", "3", "127.0.0.1"]))
        } else {
            ("sleep", json!(["2"]))
        };
        worker
            .try_submit(capability_job(
                "alpha",
                "request-blocking",
                Capability::Shell,
                "run",
                json!({"cmd": command, "args": arguments}),
            ))
            .expect("submit blocking job");

        let started = Instant::now();
        drop(worker);
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "worker shutdown exceeded its grace period"
        );
    }

    fn job(
        extension_id: &str,
        request_id: &str,
        operation: &str,
        args: serde_json::Value,
    ) -> AuthorizedCapabilityJob {
        capability_job(extension_id, request_id, Capability::Store, operation, args)
    }

    fn capability_job(
        extension_id: &str,
        request_id: &str,
        capability: Capability,
        operation: &str,
        args: serde_json::Value,
    ) -> AuthorizedCapabilityJob {
        AuthorizedCapabilityJob {
            job_id: format!("job-{request_id}"),
            extension_id: extension_id.to_string(),
            request_id: request_id.to_string(),
            capability,
            operation: operation.to_string(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        }
    }

    fn wait_for_completion(worker: &CapabilityWorker) -> super::CompletedCapabilityJob {
        for _ in 0..400 {
            if let Some(completed) = worker.try_complete() {
                return completed;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("worker did not complete");
    }

    fn write_store_component(parent: &Path, id: &str) {
        write_component_with_permissions(parent, id, &["store", "shell"]);
    }

    fn write_native_component(parent: &Path, id: &str) {
        write_component_with_permissions(parent, id, &["fs", "shell", "ui"]);
    }

    fn write_component_with_permissions(parent: &Path, id: &str, permissions: &[&str]) {
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
}

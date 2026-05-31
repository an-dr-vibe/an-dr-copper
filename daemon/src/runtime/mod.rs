pub mod deno_runner;

use crate::descriptor::Action;
use crate::execution::permissions_as_strings;
use crate::extension::Extension;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use thiserror::Error;

pub const RUNTIME_ABI_VERSION: &str = "copper.runtime/1";

pub trait RuntimeAdapter {
    fn on_load(&self, _extension: &Extension) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn prepare_trigger(
        &self,
        extension: &Extension,
        action_id: Option<&str>,
    ) -> Result<RuntimeTriggerResult, RuntimeError>;

    fn on_unload(&self, _extension: &Extension) -> Result<(), RuntimeError> {
        Ok(())
    }
}

pub fn default_runtime_adapter() -> Result<Box<dyn RuntimeAdapter>, RuntimeError> {
    if cfg!(test) {
        Ok(Box::new(DryRunRuntime))
    } else {
        Ok(Box::new(SubprocessRuntime::for_current_process()?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadata {
    pub abi_version: String,
    pub executor: String,
    pub isolated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTriggerResult {
    pub extension_id: String,
    pub action_id: String,
    pub permissions: Vec<String>,
    pub script: String,
    pub main_ts_path: String,
    pub runtime: RuntimeMetadata,
    #[serde(default)]
    pub extras: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RuntimeProtocolRequest {
    abi_version: String,
    extension: Extension,
    action_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RuntimeProtocolResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<RuntimeTriggerResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RuntimeProtocolError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RuntimeProtocolError {
    code: String,
    message: String,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("runtime protocol error: {0}")]
    Protocol(String),
    #[error("runtime execution failed [{code}]: {message}")]
    Execution { code: String, message: String },
}

#[derive(Debug, Default, Clone)]
pub struct DryRunRuntime;

impl RuntimeAdapter for DryRunRuntime {
    fn prepare_trigger(
        &self,
        extension: &Extension,
        action_id: Option<&str>,
    ) -> Result<RuntimeTriggerResult, RuntimeError> {
        let action =
            select_action(extension, action_id).map_err(|message| RuntimeError::Execution {
                code: "invalid-action".to_string(),
                message,
            })?;

        Ok(RuntimeTriggerResult {
            extension_id: extension.descriptor.id.clone(),
            action_id: action.id.clone(),
            permissions: permissions_as_strings(&extension.descriptor.permissions),
            script: action.script.clone(),
            main_ts_path: extension.main_ts_path.display().to_string(),
            runtime: RuntimeMetadata {
                abi_version: RUNTIME_ABI_VERSION.to_string(),
                executor: "in-process-dry-run".to_string(),
                isolated: false,
            },
            extras: Map::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SubprocessRuntime {
    executable: PathBuf,
}

impl SubprocessRuntime {
    pub fn for_current_process() -> Result<Self, RuntimeError> {
        Ok(Self {
            executable: std::env::current_exe().map_err(RuntimeError::Io)?,
        })
    }

    #[cfg(test)]
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }
}

impl RuntimeAdapter for SubprocessRuntime {
    fn prepare_trigger(
        &self,
        extension: &Extension,
        action_id: Option<&str>,
    ) -> Result<RuntimeTriggerResult, RuntimeError> {
        let request = RuntimeProtocolRequest {
            abi_version: RUNTIME_ABI_VERSION.to_string(),
            extension: extension.clone(),
            action_id: action_id.map(str::to_string),
        };
        let mut child = Command::new(&self.executable)
            .args(["internal", "runtime-trigger"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stdin) = child.stdin.as_mut() {
            serde_json::to_writer(&mut *stdin, &request)?;
            stdin.flush()?;
        } else {
            return Err(RuntimeError::Protocol(
                "runtime worker stdin unavailable".to_string(),
            ));
        }

        let output = child.wait_with_output()?;
        if output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RuntimeError::Protocol(format!(
                "runtime worker returned no response: {stderr}"
            )));
        }

        let response: RuntimeProtocolResponse = serde_json::from_slice(&output.stdout)?;
        if response.ok {
            return response
                .result
                .ok_or_else(|| RuntimeError::Protocol("missing runtime result".to_string()));
        }

        let error = response.error.unwrap_or(RuntimeProtocolError {
            code: "unknown".to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
        Err(RuntimeError::Execution {
            code: error.code,
            message: error.message,
        })
    }
}

pub fn run_protocol_worker<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
) -> Result<(), RuntimeError> {
    let mut raw = String::new();
    reader.read_to_string(&mut raw)?;
    let request: RuntimeProtocolRequest = serde_json::from_str(&raw)?;

    let response = if request.abi_version != RUNTIME_ABI_VERSION {
        RuntimeProtocolResponse {
            ok: false,
            result: None,
            error: Some(RuntimeProtocolError {
                code: "abi-mismatch".to_string(),
                message: format!(
                    "unsupported runtime ABI '{}'; expected '{}'",
                    request.abi_version, RUNTIME_ABI_VERSION
                ),
            }),
        }
    } else {
        let runtime = DryRunRuntime;
        match runtime.prepare_trigger(&request.extension, request.action_id.as_deref()) {
            Ok(result) => RuntimeProtocolResponse {
                ok: true,
                result: Some(RuntimeTriggerResult {
                    runtime: RuntimeMetadata {
                        abi_version: RUNTIME_ABI_VERSION.to_string(),
                        executor: "subprocess-runtime".to_string(),
                        isolated: true,
                    },
                    ..result
                }),
                error: None,
            },
            Err(err) => RuntimeProtocolResponse {
                ok: false,
                result: None,
                error: Some(runtime_error_to_protocol(err)),
            },
        }
    };

    serde_json::to_writer(&mut writer, &response)?;
    writer.flush()?;
    Ok(())
}

fn runtime_error_to_protocol(error: RuntimeError) -> RuntimeProtocolError {
    match error {
        RuntimeError::Execution { code, message } => RuntimeProtocolError { code, message },
        RuntimeError::Protocol(message) => RuntimeProtocolError {
            code: "protocol".to_string(),
            message,
        },
        RuntimeError::Io(err) => RuntimeProtocolError {
            code: "io".to_string(),
            message: err.to_string(),
        },
        RuntimeError::Serde(err) => RuntimeProtocolError {
            code: "serde".to_string(),
            message: err.to_string(),
        },
    }
}

fn select_action<'a>(
    extension: &'a Extension,
    action_id: Option<&str>,
) -> Result<&'a Action, String> {
    if let Some(id) = action_id {
        return extension
            .descriptor
            .actions
            .iter()
            .find(|candidate| candidate.id == id)
            .ok_or_else(|| format!("action '{id}' not found"));
    }

    extension
        .descriptor
        .actions
        .first()
        .ok_or_else(|| "no action defined".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        run_protocol_worker, DryRunRuntime, RuntimeAdapter, RuntimeProtocolRequest,
        RuntimeProtocolResponse, RuntimeTriggerResult, RUNTIME_ABI_VERSION,
    };
    use crate::descriptor::{Action, Descriptor};
    use crate::extension::Extension;
    use std::path::PathBuf;

    fn extension_with_actions(actions: Vec<Action>) -> Extension {
        Extension {
            root: PathBuf::from("C:/tmp/ext"),
            main_ts_path: PathBuf::from("C:/tmp/ext/main.ts"),
            descriptor: Descriptor {
                schema: None,
                id: "sample".to_string(),
                name: "Sample".to_string(),
                version: "1.0.0".to_string(),
                trigger: "sample".to_string(),
                platforms: vec![],
                permissions: vec![],
                inputs: vec![],
                actions,
                ui: None,
                settings: None,
                tray: None,
            },
        }
    }

    fn prepare(
        runtime: &dyn RuntimeAdapter,
        extension: &Extension,
        action_id: Option<&str>,
    ) -> RuntimeTriggerResult {
        runtime
            .prepare_trigger(extension, action_id)
            .expect("prepared trigger")
    }

    #[test]
    fn trigger_uses_first_action_when_action_id_missing() {
        let runtime = DryRunRuntime;
        let extension = extension_with_actions(vec![
            Action {
                id: "first".to_string(),
                label: "First".to_string(),
                description: None,
                script: "return 1;".to_string(),
            },
            Action {
                id: "second".to_string(),
                label: "Second".to_string(),
                description: None,
                script: "return 2;".to_string(),
            },
        ]);

        let value = prepare(&runtime, &extension, None);
        assert_eq!(value.action_id, "first");
        assert!(!value.runtime.isolated);
    }

    #[test]
    fn trigger_uses_requested_action_when_present() {
        let runtime = DryRunRuntime;
        let extension = extension_with_actions(vec![
            Action {
                id: "first".to_string(),
                label: "First".to_string(),
                description: None,
                script: "return 1;".to_string(),
            },
            Action {
                id: "second".to_string(),
                label: "Second".to_string(),
                description: None,
                script: "return 2;".to_string(),
            },
        ]);

        let value = prepare(&runtime, &extension, Some("second"));
        assert_eq!(value.action_id, "second");
    }

    #[test]
    fn trigger_errors_for_unknown_action() {
        let runtime = DryRunRuntime;
        let extension = extension_with_actions(vec![Action {
            id: "first".to_string(),
            label: "First".to_string(),
            description: None,
            script: "return 1;".to_string(),
        }]);

        let err = runtime
            .prepare_trigger(&extension, Some("missing"))
            .expect_err("unknown action should fail");
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn trigger_errors_when_no_actions_exist() {
        let runtime = DryRunRuntime;
        let extension = extension_with_actions(vec![]);

        let err = runtime
            .prepare_trigger(&extension, None)
            .expect_err("empty actions should fail");
        assert!(err.to_string().contains("no action defined"));
    }

    #[test]
    fn protocol_worker_returns_isolated_runtime_metadata() {
        let request = RuntimeProtocolRequest {
            abi_version: RUNTIME_ABI_VERSION.to_string(),
            extension: extension_with_actions(vec![Action {
                id: "run".to_string(),
                label: "Run".to_string(),
                description: None,
                script: "return 1;".to_string(),
            }]),
            action_id: Some("run".to_string()),
        };

        let mut output = Vec::new();
        run_protocol_worker(
            std::io::Cursor::new(serde_json::to_vec(&request).expect("request")),
            &mut output,
        )
        .expect("worker");

        let response: RuntimeProtocolResponse =
            serde_json::from_slice(&output).expect("parse response");
        assert!(response.ok);
        let result = response.result.expect("result");
        assert!(result.runtime.isolated);
        assert_eq!(result.runtime.executor, "subprocess-runtime");
    }

    #[test]
    fn protocol_worker_rejects_mismatched_abi_version() {
        let request = RuntimeProtocolRequest {
            abi_version: "other/1".to_string(),
            extension: extension_with_actions(vec![Action {
                id: "run".to_string(),
                label: "Run".to_string(),
                description: None,
                script: "return 1;".to_string(),
            }]),
            action_id: Some("run".to_string()),
        };

        let mut output = Vec::new();
        run_protocol_worker(
            std::io::Cursor::new(serde_json::to_vec(&request).expect("request")),
            &mut output,
        )
        .expect("worker");

        let response: RuntimeProtocolResponse =
            serde_json::from_slice(&output).expect("parse response");
        assert!(!response.ok);
        assert_eq!(response.error.expect("error").code, "abi-mismatch");
    }
}

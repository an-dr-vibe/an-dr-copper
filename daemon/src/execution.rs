use crate::descriptor::Permission;
use crate::extension::Extension;
use crate::host_extensions::HostExtensionRegistry;
use crate::runtime::{deno_runner, RuntimeAdapter, RuntimeMetadata};
use crate::state_store::ExtensionStateStore;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct PreparedTrigger {
    #[serde(rename = "extensionId")]
    pub extension_id: String,
    #[serde(rename = "actionId")]
    pub action_id: String,
    pub permissions: Vec<String>,
    pub script: String,
    #[serde(rename = "mainTsPath")]
    pub main_ts_path: String,
    pub runtime: RuntimeMetadata,
    #[serde(flatten)]
    pub extras: serde_json::Map<String, serde_json::Value>,
}

pub struct ExecutionEngine<'a> {
    runtime: &'a dyn RuntimeAdapter,
    host_extensions: &'a HostExtensionRegistry,
    state_store: &'a ExtensionStateStore,
}

impl<'a> ExecutionEngine<'a> {
    pub fn new(
        runtime: &'a dyn RuntimeAdapter,
        host_extensions: &'a HostExtensionRegistry,
        state_store: &'a ExtensionStateStore,
    ) -> Self {
        Self {
            runtime,
            host_extensions,
            state_store,
        }
    }

    pub fn prepare_trigger(
        &self,
        extension: &Extension,
        action_id: Option<&str>,
    ) -> Result<PreparedTrigger, String> {
        let runtime_payload = self
            .runtime
            .prepare_trigger(extension, action_id)
            .map_err(|err| format!("runtime trigger failed: {err}"))?;
        let mut extras = runtime_payload.extras.clone();

        if let Some(object) = self
            .host_extensions
            .trigger_payload(
                &extension.descriptor.id,
                self.state_store,
                &runtime_payload.action_id,
            )
            .map_err(|err| format!("host extension trigger failed: {err}"))?
            .as_object()
            .cloned()
        {
            extras.extend(object);
        }

        Ok(PreparedTrigger {
            extension_id: runtime_payload.extension_id,
            action_id: runtime_payload.action_id,
            permissions: runtime_payload.permissions,
            script: runtime_payload.script,
            main_ts_path: runtime_payload.main_ts_path,
            runtime: runtime_payload.runtime,
            extras,
        })
    }

    pub fn execute_trigger(
        &self,
        prepared: &PreparedTrigger,
        inputs: &Value,
    ) -> Result<(), String> {
        if cfg!(test) {
            return Ok(());
        }
        let store_path = self
            .state_store
            .data_root()
            .join(&prepared.extension_id)
            .join("store.json")
            .display()
            .to_string();
        // Always inject the resolved action_id so extensions can route on inputs.action.
        // Caller-supplied "action" takes precedence (allows override via --input action=…).
        let merged = match inputs.clone() {
            Value::Object(mut map) => {
                map.entry("action".to_string())
                    .or_insert_with(|| Value::String(prepared.action_id.clone()));
                Value::Object(map)
            }
            other => serde_json::json!({ "action": prepared.action_id, "_raw": other }),
        };
        deno_runner::execute_extension(
            &prepared.extension_id,
            &prepared.main_ts_path,
            &store_path,
            &prepared.permissions,
            &merged,
        )
    }
}

pub fn permissions_as_strings(permissions: &[Permission]) -> Vec<String> {
    permissions
        .iter()
        .map(|permission| match permission {
            Permission::Fs => "fs".to_string(),
            Permission::Keyboard => "keyboard".to_string(),
            Permission::Network => "network".to_string(),
            Permission::SecureStore => "secure-store".to_string(),
            Permission::Shell => "shell".to_string(),
            Permission::Store => "store".to_string(),
            Permission::Ui => "ui".to_string(),
            Permission::WindowsDisplay => "windows-display".to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ExecutionEngine;
    use crate::descriptor::{Action, Descriptor};
    use crate::extension::Extension;
    use crate::host_extensions::HostExtensionRegistry;
    use crate::runtime::DryRunRuntime;
    use crate::state_store::ExtensionStateStore;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn prepare_trigger_uses_runtime_and_host_registry() {
        let temp = tempdir().expect("tempdir");
        let extension = Extension {
            root: PathBuf::from("C:/tmp/ext"),
            main_ts_path: PathBuf::from("C:/tmp/ext/main.ts"),
            wasm_component_path: None,
            descriptor: Descriptor {
                schema: None,
                id: "session-counter".to_string(),
                name: "Session Counter".to_string(),
                version: "1.0.0".to_string(),
                trigger: "session".to_string(),
                runtime: None,
                platforms: vec![],
                permissions: vec![],
                inputs: vec![],
                actions: vec![Action {
                    id: "increment".to_string(),
                    label: "Increment".to_string(),
                    description: None,
                    script: "return;".to_string(),
                }],
                ui: None,
                settings: None,
                tray: None,
            },
        };
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let runtime = DryRunRuntime;
        let registry = HostExtensionRegistry::new();
        let engine = ExecutionEngine::new(&runtime, &registry, &store);

        let prepared = engine.prepare_trigger(&extension, None).expect("prepared");
        assert_eq!(prepared.action_id, "increment");
        assert!(!prepared.runtime.isolated);
    }
}

use crate::descriptor::Permission;
use crate::extension::Extension;
use crate::host_extensions::HostExtensionRegistry;
use crate::runtime::RuntimeAdapter;
use crate::state_store::ExtensionStateStore;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PreparedTrigger {
    #[serde(rename = "extensionId")]
    pub extension_id: String,
    #[serde(rename = "actionId")]
    pub action_id: String,
    pub permissions: Vec<&'static str>,
    pub script: String,
    #[serde(rename = "mainTsPath")]
    pub main_ts_path: String,
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
        let action = if let Some(id) = action_id {
            extension
                .descriptor
                .actions
                .iter()
                .find(|candidate| candidate.id == id)
                .ok_or_else(|| {
                    format!(
                        "action '{id}' not found in extension '{}'",
                        extension.descriptor.id
                    )
                })?
        } else {
            extension.descriptor.actions.first().ok_or_else(|| {
                format!(
                    "extension '{}' contains no executable actions",
                    extension.descriptor.id
                )
            })?
        };

        let runtime_payload = self
            .runtime
            .on_trigger(extension, Some(&action.id))
            .map_err(|err| format!("runtime trigger failed: {err}"))?;
        let mut extras = serde_json::Map::new();
        if let Some(object) = runtime_payload.as_object() {
            for (key, value) in object {
                if key == "extensionId" || key == "actionId" || key == "script" {
                    continue;
                }
                extras.insert(key.clone(), value.clone());
            }
        }

        if let Some(object) = self
            .host_extensions
            .trigger_payload(&extension.descriptor.id, self.state_store, &action.id)
            .map_err(|err| format!("host extension trigger failed: {err}"))?
            .as_object()
            .cloned()
        {
            extras.extend(object);
        }

        Ok(PreparedTrigger {
            extension_id: extension.descriptor.id.clone(),
            action_id: action.id.clone(),
            permissions: permissions_as_strings(&extension.descriptor.permissions),
            script: action.script.clone(),
            main_ts_path: extension.main_ts_path.display().to_string(),
            extras,
        })
    }
}

pub fn permissions_as_strings(permissions: &[Permission]) -> Vec<&'static str> {
    permissions
        .iter()
        .map(|permission| match permission {
            Permission::Fs => "fs",
            Permission::Shell => "shell",
            Permission::Network => "network",
            Permission::Store => "store",
            Permission::Ui => "ui",
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
            descriptor: Descriptor {
                schema: None,
                id: "session-counter".to_string(),
                name: "Session Counter".to_string(),
                version: "1.0.0".to_string(),
                trigger: "session".to_string(),
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
        assert_eq!(
            prepared
                .extras
                .get("sessionCount")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
    }
}

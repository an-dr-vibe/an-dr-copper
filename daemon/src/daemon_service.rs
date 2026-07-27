use crate::bones_integration::BonesRuntimeStatus;
use crate::execution::{permissions_as_strings, ExecutionEngine};
use crate::extension::Registry;
use crate::host_extensions::HostExtensionRegistry;
use crate::runtime::default_runtime_adapter;
use crate::state_store::ExtensionStateStore;
use serde_json::Value;
use std::path::Path;

pub struct DaemonControlService<'a> {
    user_extensions_dir: &'a Path,
    core_extensions_dir: Option<&'a Path>,
    registry: &'a Registry,
    host_extensions: &'a HostExtensionRegistry,
    state_store: &'a ExtensionStateStore,
    bones_status: BonesRuntimeStatus,
}

impl<'a> DaemonControlService<'a> {
    pub fn new(
        user_extensions_dir: &'a Path,
        core_extensions_dir: Option<&'a Path>,
        registry: &'a Registry,
        host_extensions: &'a HostExtensionRegistry,
        state_store: &'a ExtensionStateStore,
        bones_status: BonesRuntimeStatus,
    ) -> Self {
        Self {
            user_extensions_dir,
            core_extensions_dir,
            registry,
            host_extensions,
            state_store,
            bones_status,
        }
    }

    pub fn health_payload(&self) -> Result<Value, String> {
        let state_warnings = self
            .state_store
            .collect_warnings()
            .map_err(|err| format!("failed to inspect state files: {err}"))?;
        Ok(serde_json::json!({
            "userExtensionsDir": self.user_extensions_dir.display().to_string(),
            "coreExtensionsDir": self
                .core_extensions_dir
                .map(|path| path.display().to_string()),
            "extensionsLoaded": self.registry.list().count(),
            "bones": self.bones_status,
            "configUiUrl": "bones://settings",
            "configUi": {
                "transport": "bones-web",
                "presentation": "wry",
                "onDemand": true,
            },
            "stateWarnings": state_warnings,
        }))
    }

    pub fn list_payload(&self) -> Value {
        let list = self
            .registry
            .list()
            .map(|ext| {
                serde_json::json!({
                    "id": ext.descriptor.id,
                    "name": ext.descriptor.name,
                    "version": ext.descriptor.version,
                    "trigger": ext.descriptor.trigger,
                    "permissions": permissions_as_strings(&ext.descriptor.permissions),
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!(list)
    }

    pub fn verify_registry(&self) -> Result<usize, String> {
        let mut found = 0usize;
        for ext in self.registry.list() {
            found += 1;
            if ext.descriptor.actions.is_empty() {
                return Err(format!("extension {} has no actions", ext.descriptor.id));
            }
            if !ext.runtime_artifact_path().exists() {
                let missing = ext
                    .wasm_component_path()
                    .map(|path| format!("runtime artifact {}", path.display()))
                    .unwrap_or_else(|| "main.ts".to_string());
                return Err(format!(
                    "extension {} is missing {missing}",
                    ext.descriptor.id
                ));
            }
        }
        Ok(found)
    }

    pub fn trigger_payload(&self, id: &str, action: Option<&str>) -> Result<Value, String> {
        let ext = self
            .registry
            .get(id)
            .ok_or_else(|| format!("extension '{id}' not found"))?;
        let runtime = default_runtime_adapter().map_err(|err| err.to_string())?;
        let engine = ExecutionEngine::new(runtime.as_ref(), self.host_extensions, self.state_store);
        let prepared = engine.prepare_trigger(ext, action)?;
        engine.execute_trigger(&prepared, &serde_json::json!({}))?;
        serde_json::to_value(prepared).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonControlService;
    use crate::bones_integration::BonesRuntimeStatus;
    use crate::extension::Registry;
    use crate::host_extensions::HostExtensionRegistry;
    use crate::state_store::ExtensionStateStore;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn bones_status() -> BonesRuntimeStatus {
        BonesRuntimeStatus {
            headless: true,
            frames: 4,
            registry_reloads: 1,
            catalog_extensions: 0,
            catalog_rebuilds: 0,
            lifecycle_events: 0,
            lifecycle_decode_errors: 0,
            extensions: BTreeMap::new(),
            capability_accepted: 0,
            capability_rejected: 0,
            capability_pending: 0,
            capability_completed: 0,
            capability_failed: 0,
            capability_delivery_failures: 0,
            actions_dispatched: 0,
            action_dispatch_failures: 0,
            shutdown: false,
        }
    }

    fn write_extension(root: &Path) {
        let ext_root = root.join("alpha-ext");
        fs::create_dir_all(&ext_root).expect("create ext root");
        fs::write(
            ext_root.join("manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "alpha-ext",
                "name": "Alpha",
                "version": "1.0.0",
                "trigger": "alpha",
                "actions": [
                    { "id": "run", "label": "Run", "script": "return;" }
                ]
            }"#,
        )
        .expect("write manifest");
        fs::write(ext_root.join("main.ts"), "export default function(){}").expect("write main");
    }

    #[test]
    fn health_payload_includes_state_warnings() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        fs::create_dir_all(store.data_root().join("alpha-ext")).expect("create data dir");
        fs::write(store.config_path("alpha-ext"), "{bad-json").expect("write invalid config");
        write_extension(temp.path());
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let host_extensions = HostExtensionRegistry::new();
        let service = DaemonControlService::new(
            temp.path(),
            Some(Path::new("C:/tmp/core")),
            &registry,
            &host_extensions,
            &store,
            bones_status(),
        );

        let payload = service.health_payload().expect("health payload");
        let warnings = payload
            .get("stateWarnings")
            .and_then(|value| value.as_array())
            .expect("warnings array");
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].get("code").and_then(|value| value.as_str()),
            Some("invalid-json")
        );
        assert_eq!(
            payload
                .get("bones")
                .and_then(|value| value.get("headless"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn trigger_payload_uses_runtime_engine() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        write_extension(temp.path());
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let host_extensions = HostExtensionRegistry::new();
        let service = DaemonControlService::new(
            temp.path(),
            Some(Path::new("C:/tmp/core")),
            &registry,
            &host_extensions,
            &store,
            bones_status(),
        );

        let payload = service
            .trigger_payload("alpha-ext", Some("run"))
            .expect("trigger payload");
        assert_eq!(
            payload.get("extensionId").and_then(|value| value.as_str()),
            Some("alpha-ext")
        );
        assert_eq!(
            payload.get("actionId").and_then(|value| value.as_str()),
            Some("run")
        );
        assert_eq!(
            payload
                .get("runtime")
                .and_then(|value| value.get("isolated"))
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }
}

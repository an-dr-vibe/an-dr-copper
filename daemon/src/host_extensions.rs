use crate::api::windows_display;
use crate::state_store::{read_json_object, unix_now_secs, write_json_object, ExtensionStateStore};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

pub const WINDOWS_DISPLAY_MANAGER_ID: &str = "windows-display-manager";

#[derive(Debug, Default, Clone)]
pub struct HostExtensionRegistry;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostStateContract {
    pub capability_id: &'static str,
    pub config_schema_version: u32,
    pub status_schema_version: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilityInfo {
    pub id: &'static str,
    pub extension_ids: Vec<&'static str>,
    pub background_polling: bool,
    pub state_contract: HostStateContract,
}

#[derive(Debug, Clone)]
pub struct AppliedActionResult {
    pub action_id: String,
    pub result: Value,
}

struct HostCapabilitySpec {
    id: &'static str,
    extension_ids: &'static [&'static str],
    background_polling: bool,
    config_schema_version: u32,
    status_schema_version: u32,
    handler: &'static dyn HostExtensionHandler,
}

pub trait HostExtensionHandler: Sync {
    fn apply_settings(
        &self,
        _store: &ExtensionStateStore,
        _apply_actions: &[String],
    ) -> Result<Vec<AppliedActionResult>, std::io::Error> {
        Ok(Vec::new())
    }

    fn dynamic_options(&self, _config: &Value) -> Result<Value, std::io::Error> {
        Ok(serde_json::json!({}))
    }
}

impl HostExtensionRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn apply_settings(
        &self,
        extension_id: &str,
        store: &ExtensionStateStore,
        apply_actions: &[String],
    ) -> Result<Vec<AppliedActionResult>, std::io::Error> {
        match self.handler(extension_id) {
            Some(handler) => handler.apply_settings(store, apply_actions),
            None => Ok(Vec::new()),
        }
    }

    pub fn dynamic_options(
        &self,
        extension_id: &str,
        config: &Value,
    ) -> Result<Value, std::io::Error> {
        match self.handler(extension_id) {
            Some(handler) => handler.dynamic_options(config),
            None => Ok(serde_json::json!({})),
        }
    }

    pub fn capability_info(&self, extension_id: &str) -> Option<HostCapabilityInfo> {
        self.capability(extension_id).map(capability_info)
    }

    pub fn state_contract(&self, extension_id: &str) -> Option<HostStateContract> {
        self.capability(extension_id).map(state_contract)
    }

    fn capability(&self, extension_id: &str) -> Option<&'static HostCapabilitySpec> {
        capability_specs()
            .iter()
            .find(|capability| capability.extension_ids.contains(&extension_id))
    }

    fn handler(&self, extension_id: &str) -> Option<&'static dyn HostExtensionHandler> {
        self.capability(extension_id)
            .map(|capability| capability.handler)
    }
}

fn capability_specs() -> &'static [HostCapabilitySpec] {
    static CAPABILITY_SPECS: [HostCapabilitySpec; 1] = [HostCapabilitySpec {
        id: "host.windows-display",
        extension_ids: &[WINDOWS_DISPLAY_MANAGER_ID],
        background_polling: false,
        config_schema_version: 1,
        status_schema_version: 1,
        handler: &WINDOWS_DISPLAY_HANDLER,
    }];
    &CAPABILITY_SPECS
}

fn state_contract(capability: &HostCapabilitySpec) -> HostStateContract {
    HostStateContract {
        capability_id: capability.id,
        config_schema_version: capability.config_schema_version,
        status_schema_version: capability.status_schema_version,
    }
}

fn capability_info(capability: &HostCapabilitySpec) -> HostCapabilityInfo {
    HostCapabilityInfo {
        id: capability.id,
        extension_ids: capability.extension_ids.to_vec(),
        background_polling: capability.background_polling,
        state_contract: state_contract(capability),
    }
}

fn stamp_status_contract(extension_id: &str, status: &mut Value) {
    let Some(capability) = capability_specs()
        .iter()
        .find(|capability| capability.extension_ids.contains(&extension_id))
    else {
        return;
    };

    if !status.is_object() {
        *status = serde_json::json!({});
    }

    if let Some(map) = status.as_object_mut() {
        map.insert(
            "_stateContract".to_string(),
            serde_json::json!({
                "capabilityId": capability.id,
                "schemaVersion": capability.status_schema_version,
                "stateKind": "status",
            }),
        );
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct WindowsDisplayHandler;

impl HostExtensionHandler for WindowsDisplayHandler {
    fn apply_settings(
        &self,
        store: &ExtensionStateStore,
        apply_actions: &[String],
    ) -> Result<Vec<AppliedActionResult>, std::io::Error> {
        apply_actions
            .iter()
            .map(|action_id| {
                Ok(AppliedActionResult {
                    action_id: action_id.clone(),
                    result: execute_windows_display_action(store, action_id)?,
                })
            })
            .collect()
    }

    fn dynamic_options(&self, config: &Value) -> Result<Value, std::io::Error> {
        Ok(windows_display_dynamic_options(config))
    }
}

static WINDOWS_DISPLAY_HANDLER: WindowsDisplayHandler = WindowsDisplayHandler;

pub fn execute_windows_display_action(
    store: &ExtensionStateStore,
    action_id: &str,
) -> Result<Value, std::io::Error> {
    store.ensure_root()?;
    let config = store.load_config(WINDOWS_DISPLAY_MANAGER_ID)?;
    let path = store.status_path(WINDOWS_DISPLAY_MANAGER_ID);
    let mut state = read_json_object(&path)?;
    let execution = match windows_display::execute_action(action_id, &config) {
        Ok(value) => value,
        Err(err) => {
            update_windows_display_status(&mut state, action_id, None, Some(&err));
            write_json_object(&path, &state)?;
            return Err(std::io::Error::other(err));
        }
    };

    update_windows_display_status(&mut state, action_id, Some(&execution), None);
    write_json_object(&path, &state)?;
    Ok(execution)
}

fn update_windows_display_status(
    state: &mut Value,
    action_id: &str,
    execution: Option<&Value>,
    error: Option<&str>,
) {
    if !state.is_object() {
        *state = serde_json::json!({});
    }

    if let Some(map) = state.as_object_mut() {
        map.insert("lastActionId".to_string(), serde_json::json!(action_id));
        map.insert(
            "lastActionUnix".to_string(),
            serde_json::json!(unix_now_secs()),
        );
        map.insert(
            "lastActionOk".to_string(),
            serde_json::json!(error.is_none()),
        );
        if let Some(execution) = execution {
            map.insert("lastResult".to_string(), execution.clone());
            map.remove("lastError");

            if let Some(taskbar_auto_hide) = execution.get("taskbarAutoHide") {
                map.insert("taskbarAutoHide".to_string(), taskbar_auto_hide.clone());
            }
            if let Some(scale_current) =
                execution.get("scale").and_then(|v| v.get("currentPercent"))
            {
                map.insert("scalePercent".to_string(), scale_current.clone());
            }
            if let Some(resolution) = execution.get("resolution") {
                if let Some(width) = resolution.get("width") {
                    map.insert("resolutionWidth".to_string(), width.clone());
                }
                if let Some(height) = resolution.get("height") {
                    map.insert("resolutionHeight".to_string(), height.clone());
                }
                if let Some(refresh_rate) = resolution.get("refreshRate") {
                    map.insert("refreshRate".to_string(), refresh_rate.clone());
                }
            }
        } else if let Some(error) = error {
            map.insert("lastError".to_string(), serde_json::json!(error));
        }
    }
    stamp_status_contract(WINDOWS_DISPLAY_MANAGER_ID, state);
}

fn windows_display_dynamic_options(config: &Value) -> Value {
    let fallback_resolution =
        configured_resolution_mode(config).unwrap_or_else(|| "1920x1080@60".to_string());
    let fallback_scale = configured_scale_percent(config).unwrap_or(100);

    #[cfg(target_os = "windows")]
    let live_status = windows_display::execute_action("status", config).ok();
    #[cfg(not(target_os = "windows"))]
    let live_status: Option<Value> = None;

    let live_resolution_modes = live_status
        .as_ref()
        .and_then(|status| status.get("resolution"))
        .and_then(|value| value.get("availableModes"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    Some(format!(
                        "{}x{}@{}",
                        value.get("width")?.as_i64()?,
                        value.get("height")?.as_i64()?,
                        value.get("refreshRate")?.as_i64()?
                    ))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let live_scales = live_status
        .as_ref()
        .and_then(|status| status.get("scale"))
        .and_then(|value| value.get("availablePercentages"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_i64())
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let configured_presets = config
        .get("trayResolutionPresets")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let resolution_modes = unique_preserving_order(
        live_resolution_modes
            .into_iter()
            .chain(configured_presets.iter().cloned())
            .chain(std::iter::once(fallback_resolution.clone()))
            .collect(),
    );
    let scale_percentages = unique_preserving_order(
        live_scales
            .into_iter()
            .chain(std::iter::once(fallback_scale.to_string()))
            .chain(
                ["100", "125", "150", "175", "200"]
                    .into_iter()
                    .map(str::to_string),
            )
            .collect(),
    );

    serde_json::json!({
        "resolutionModes": resolution_modes.clone(),
        "trayResolutionPresets": resolution_modes,
        "scalePercentages": scale_percentages,
    })
}

fn unique_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    unique
}

fn configured_resolution_mode(config: &Value) -> Option<String> {
    if let Some(mode) = config.get("resolutionMode").and_then(Value::as_str) {
        return Some(mode.to_string());
    }

    Some(format!(
        "{}x{}@{}",
        config.get("resolutionWidth")?.as_i64()?,
        config.get("resolutionHeight")?.as_i64()?,
        config.get("refreshRate")?.as_i64()?
    ))
}

fn configured_scale_percent(config: &Value) -> Option<i64> {
    config.get("scalePercent").and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str()?.parse::<i64>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::{
        execute_windows_display_action, HostExtensionRegistry, WINDOWS_DISPLAY_MANAGER_ID,
    };
    use crate::state_store::{write_json_object, ExtensionStateStore};
    use tempfile::tempdir;

    #[test]
    fn windows_display_status_persists_snapshot() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        write_json_object(
            &store.config_path(WINDOWS_DISPLAY_MANAGER_ID),
            &serde_json::json!({
                "resolutionWidth": 1920,
                "resolutionHeight": 1080,
                "refreshRate": 60,
                "scalePercent": 100
            }),
        )
        .expect("write config");

        let result = execute_windows_display_action(&store, "status");
        if cfg!(target_os = "windows") {
            let value = result.expect("status");
            assert!(value.get("resolution").is_some());
        } else {
            let err = result.expect_err("non windows");
            assert_eq!(err.kind(), std::io::ErrorKind::Other);
        }
    }

    #[test]
    fn windows_display_dynamic_options_degrade_off_windows() {
        let registry = HostExtensionRegistry::new();
        let options = registry
            .dynamic_options(WINDOWS_DISPLAY_MANAGER_ID, &serde_json::json!({}))
            .expect("dynamic options");

        assert!(options.get("resolutionModes").is_some());
        assert!(options.get("scalePercentages").is_some());
        assert!(options.get("trayResolutionPresets").is_some());
    }

    #[test]
    fn registry_exposes_capability_metadata() {
        let registry = HostExtensionRegistry::new();
        let capability = registry
            .capability_info(WINDOWS_DISPLAY_MANAGER_ID)
            .expect("capability");
        assert_eq!(capability.id, "host.windows-display");
        assert_eq!(capability.state_contract.status_schema_version, 1);
    }
}

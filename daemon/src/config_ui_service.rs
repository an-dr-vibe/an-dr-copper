use crate::config_ui::{UiConfigError, UiServerState};
use crate::descriptor::Descriptor;
use serde_json::Value;

pub(crate) fn build_core_info(state: &UiServerState) -> Result<Value, UiConfigError> {
    Ok(serde_json::json!({
        "extensionsLoaded": state.descriptors.len(),
        "hostPlatform": crate::extension::current_platform().as_str(),
        "userExtensionsDir": state.user_extensions_dir.display().to_string(),
        "coreExtensionsDir": state
            .core_extensions_dir
            .as_ref()
            .map(|path| path.display().to_string()),
        "runtimeExtensionRoots": state
            .runtime_extension_roots
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "dataRoot": state.state_store.data_root().display().to_string(),
        "coreDataPath": state.state_store.core_config_path().display().to_string(),
        "stateWarnings": state.state_store.collect_warnings()?,
    }))
}

pub(crate) fn build_extension_info(
    state: &UiServerState,
    descriptor: &Descriptor,
) -> Result<Value, UiConfigError> {
    let config = state.state_store.inspect_config(&descriptor.id)?;
    let status = state.state_store.inspect_status(&descriptor.id)?;
    let mut state_warnings = config.warnings.clone();
    state_warnings.extend(status.warnings.clone());
    let apply_actions = descriptor
        .settings
        .as_ref()
        .map(|settings| settings.apply_actions.clone())
        .unwrap_or_default();
    let commands = descriptor
        .actions
        .iter()
        .filter_map(|action| build_user_command_info(state, descriptor, action, &apply_actions))
        .collect::<Vec<_>>();
    let dynamic_options = state
        .host_extensions
        .dynamic_options(&descriptor.id, &config.value)?;
    let host_capability = state.host_extensions.capability_info(&descriptor.id);

    Ok(serde_json::json!({
        "extensionId": descriptor.id,
        "name": descriptor.name,
        "status": status.value,
        "statusMeta": descriptor
            .settings
            .as_ref()
            .and_then(|settings| settings.status.clone()),
        "applyActions": apply_actions,
        "commands": commands,
        "dynamicOptions": dynamic_options,
        "hostCapability": host_capability,
        "stateWarnings": state_warnings,
    }))
}

pub(crate) fn apply_extension_settings(
    state: &UiServerState,
    descriptor: &Descriptor,
) -> Result<Value, UiConfigError> {
    let apply_actions = descriptor
        .settings
        .as_ref()
        .map(|settings| settings.apply_actions.clone())
        .unwrap_or_default();
    if apply_actions.is_empty() {
        return Err(UiConfigError::Request(format!(
            "extension '{}' does not declare settings apply actions",
            descriptor.id
        )));
    }

    let executions = state
        .host_extensions
        .apply_settings(&descriptor.id, &state.state_store, &apply_actions)?
        .into_iter()
        .map(|execution| {
            serde_json::json!({
                "actionId": execution.action_id,
                "result": execution.result,
            })
        })
        .collect::<Vec<_>>();
    if executions.is_empty() {
        return Err(UiConfigError::Request(format!(
            "extension '{}' does not support settings apply actions in the host",
            descriptor.id
        )));
    }

    let status = state.state_store.inspect_status(&descriptor.id)?;
    Ok(serde_json::json!({
        "ok": true,
        "extensionId": descriptor.id,
        "executions": executions,
        "status": status.value,
        "stateWarnings": status.warnings,
    }))
}

fn build_user_command_info(
    state: &UiServerState,
    descriptor: &Descriptor,
    action: &crate::descriptor::Action,
    apply_actions: &[String],
) -> Option<Value> {
    let mut usage = Vec::new();

    if apply_actions
        .iter()
        .any(|candidate| candidate == &action.id)
    {
        usage.push("Update the saved settings above, then click Save and apply.".to_string());
    }

    if descriptor.runtime.is_some() {
        usage.push(format!(
            "Run `copperd trigger {} --action {}`.",
            descriptor.id, action.id
        ));
    } else if state
        .host_extensions
        .supports_cli_trigger(&descriptor.id, &action.id)
    {
        usage.push(format!(
            "Run `copperd daemon trigger {} --action {}` while the daemon is running.",
            descriptor.id, action.id
        ));
    }

    if usage.is_empty() {
        return None;
    }

    Some(serde_json::json!({
        "id": action.id,
        "label": action.label,
        "description": action.description,
        "usage": usage,
    }))
}

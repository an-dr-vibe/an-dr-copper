use crate::extension::Extension;

pub trait RuntimeAdapter {
    fn on_load(&self, _extension: &Extension) -> Result<(), String> {
        Ok(())
    }

    fn on_trigger(
        &self,
        _extension: &Extension,
        _action_id: Option<&str>,
    ) -> Result<serde_json::Value, String>;

    fn on_unload(&self, _extension: &Extension) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct DryRunRuntime;

impl RuntimeAdapter for DryRunRuntime {
    fn on_trigger(
        &self,
        extension: &Extension,
        action_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let action = if let Some(id) = action_id {
            extension
                .descriptor
                .actions
                .iter()
                .find(|candidate| candidate.id == id)
                .ok_or_else(|| format!("action '{id}' not found"))?
        } else {
            extension
                .descriptor
                .actions
                .first()
                .ok_or_else(|| "no action defined".to_string())?
        };

        Ok(serde_json::json!({
            "extensionId": extension.descriptor.id,
            "actionId": action.id,
            "script": action.script
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{DryRunRuntime, RuntimeAdapter};
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
                permissions: vec![],
                inputs: vec![],
                actions,
                ui: None,
                settings: None,
                tray: None,
            },
        }
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

        let value = runtime
            .on_trigger(&extension, None)
            .expect("default action");
        assert_eq!(
            value.get("actionId").and_then(|v| v.as_str()),
            Some("first")
        );
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

        let value = runtime
            .on_trigger(&extension, Some("second"))
            .expect("selected action");
        assert_eq!(
            value.get("actionId").and_then(|v| v.as_str()),
            Some("second")
        );
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
            .on_trigger(&extension, Some("missing"))
            .expect_err("unknown action should fail");
        assert!(err.contains("not found"));
    }

    #[test]
    fn trigger_errors_when_no_actions_exist() {
        let runtime = DryRunRuntime;
        let extension = extension_with_actions(vec![]);

        let err = runtime
            .on_trigger(&extension, None)
            .expect_err("empty actions should fail");
        assert!(err.contains("no action defined"));
    }

    #[test]
    fn default_hooks_return_ok() {
        let runtime = DryRunRuntime;
        let extension = extension_with_actions(vec![Action {
            id: "first".to_string(),
            label: "First".to_string(),
            description: None,
            script: "return 1;".to_string(),
        }]);
        assert!(runtime.on_load(&extension).is_ok());
        assert!(runtime.on_unload(&extension).is_ok());
    }
}

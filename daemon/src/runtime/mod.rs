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

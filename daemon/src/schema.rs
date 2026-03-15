use crate::descriptor::{Descriptor, SUPPORTED_SCHEMA_URL};
use jsonschema::JSONSchema;
use std::sync::OnceLock;
use thiserror::Error;

static SCHEMA_JSON: &str = include_str!("../../schemas/extension/1.0.0/descriptor.schema.json");
static VALIDATOR: OnceLock<JSONSchema> = OnceLock::new();

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("schema compilation failed: {0}")]
    SchemaCompilation(String),
    #[error("descriptor schema validation failed:\n{0}")]
    SchemaViolation(String),
    #[error("unsupported schema url: {0}")]
    UnsupportedSchema(String),
    #[error("version field is not valid semver: {0}")]
    InvalidVersion(String),
}

pub fn validator() -> Result<&'static JSONSchema, ValidationError> {
    if let Some(validator) = VALIDATOR.get() {
        return Ok(validator);
    }

    let schema_value: serde_json::Value =
        serde_json::from_str(SCHEMA_JSON).map_err(ValidationError::InvalidJson)?;
    let compiled = JSONSchema::compile(&schema_value)
        .map_err(|e| ValidationError::SchemaCompilation(e.to_string()))?;
    Ok(VALIDATOR.get_or_init(|| compiled))
}

pub fn parse_and_validate(raw: &str) -> Result<Descriptor, ValidationError> {
    let validator = validator()?;
    let value: serde_json::Value = serde_json::from_str(raw)?;
    if let Err(errors) = validator.validate(&value) {
        let joined = errors
            .map(|err| format!("- {}", err))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(ValidationError::SchemaViolation(joined));
    }

    let descriptor: Descriptor = serde_json::from_value(value)?;

    if let Some(schema) = descriptor.schema.clone() {
        if schema != SUPPORTED_SCHEMA_URL {
            return Err(ValidationError::UnsupportedSchema(schema));
        }
    }

    descriptor
        .parsed_version()
        .map_err(|_| ValidationError::InvalidVersion(descriptor.version.clone()))?;

    Ok(descriptor)
}

#[cfg(test)]
mod tests {
    use super::parse_and_validate;
    use crate::descriptor::SUPPORTED_SCHEMA_URL;

    #[test]
    fn validates_valid_descriptor() {
        let raw = format!(
            r#"{{
                "$schema": "{SUPPORTED_SCHEMA_URL}",
                "id": "sort-downloads",
                "name": "Sort Downloads",
                "version": "1.0.0",
                "trigger": "sort-dl",
                "permissions": ["fs"],
                "actions": [
                    {{
                        "id": "sort",
                        "label": "Sort by extension",
                        "script": "const files = await api.fs.list(inputs.folder);"
                    }}
                ]
            }}"#
        );

        let descriptor = parse_and_validate(&raw).expect("descriptor should pass validation");
        assert_eq!(descriptor.id, "sort-downloads");
        assert_eq!(descriptor.trigger, "sort-dl");
    }

    #[test]
    fn rejects_invalid_id_pattern() {
        let raw = format!(
            r#"{{
                "$schema": "{SUPPORTED_SCHEMA_URL}",
                "id": "SortDownloads",
                "name": "Sort Downloads",
                "version": "1.0.0",
                "trigger": "sort-dl",
                "actions": [
                    {{
                        "id": "sort",
                        "label": "Sort by extension",
                        "script": "return;"
                    }}
                ]
            }}"#
        );

        let error = parse_and_validate(&raw).expect_err("invalid id should fail");
        assert!(error
            .to_string()
            .contains("descriptor schema validation failed"));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let raw = r#"{
            "$schema": "https://Copper.dev/schemas/extension/9.9.9/descriptor.schema.json",
            "id": "sort-downloads",
            "name": "Sort Downloads",
            "version": "1.0.0",
            "trigger": "sort-dl",
            "actions": [
                {
                    "id": "sort",
                    "label": "Sort by extension",
                    "script": "return;"
                }
            ]
        }"#;

        let error = parse_and_validate(raw).expect_err("schema URL should be rejected");
        assert!(error.to_string().contains("unsupported schema url"));
    }

    #[test]
    fn rejects_descriptor_without_actions() {
        let raw = format!(
            r#"{{
                "$schema": "{SUPPORTED_SCHEMA_URL}",
                "id": "sort-downloads",
                "name": "Sort Downloads",
                "version": "1.0.0",
                "trigger": "sort-dl"
            }}"#
        );

        let error = parse_and_validate(&raw).expect_err("missing actions should fail");
        assert!(error
            .to_string()
            .contains("descriptor schema validation failed"));
    }

    #[test]
    fn rejects_invalid_semver_version() {
        let raw = format!(
            r#"{{
                "$schema": "{SUPPORTED_SCHEMA_URL}",
                "id": "sort-downloads",
                "name": "Sort Downloads",
                "version": "01.0.0",
                "trigger": "sort-dl",
                "actions": [
                    {{
                        "id": "sort",
                        "label": "Sort by extension",
                        "script": "return;"
                    }}
                ]
            }}"#
        );

        let error = parse_and_validate(&raw).expect_err("invalid semver should fail");
        assert!(error
            .to_string()
            .contains("version field is not valid semver"));
    }

    #[test]
    fn rejects_invalid_json_payload() {
        let error = parse_and_validate("{not-json").expect_err("invalid json should fail");
        assert!(error.to_string().contains("invalid JSON"));
    }

    #[test]
    fn validates_descriptor_with_settings_metadata() {
        let raw = format!(
            r#"{{
                "$schema": "{SUPPORTED_SCHEMA_URL}",
                "id": "windows-display-manager",
                "name": "Windows Display Manager",
                "version": "1.0.0",
                "trigger": "windows-display",
                "inputs": [
                    {{
                        "id": "taskbarAutoHide",
                        "type": "boolean",
                        "label": "Taskbar auto-hide",
                        "description": "Hide the taskbar until the pointer touches the screen edge.",
                        "default": false
                    }},
                    {{
                        "id": "trayResolutionPresets",
                        "type": "multi-select",
                        "label": "Tray resolution presets",
                        "description": "Choose which resolutions appear in the tray menu.",
                        "default": ["1920x1080@60"],
                        "optionsSource": "dynamicOptions.trayResolutionPresets"
                    }}
                ],
                "actions": [
                    {{
                        "id": "status",
                        "label": "Refresh status",
                        "description": "Query the current Windows display state.",
                        "script": "return;"
                    }}
                ],
                "settings": {{
                    "title": "Display",
                    "description": "Configure display behavior and review the latest runtime status.",
                    "applyActions": [
                        "set-taskbar-autohide",
                        "set-resolution",
                        "set-scale"
                    ],
                    "sections": [
                        {{
                            "id": "taskbar",
                            "title": "Taskbar",
                            "description": "Taskbar behavior settings.",
                            "inputs": ["taskbarAutoHide"]
                        }}
                    ],
                    "status": {{
                        "title": "Current status",
                        "description": "Latest values reported by the daemon.",
                        "fields": [
                            {{
                                "key": "lastActionUnix",
                                "label": "Last updated",
                                "format": "date-time"
                            }}
                        ]
                    }}
                }},
                "tray": {{
                    "provider": "windows-display",
                    "title": "Windows Display Manager",
                    "tooltip": "Taskbar and display shortcuts"
                }}
            }}"#
        );

        let descriptor = parse_and_validate(&raw).expect("descriptor should pass validation");
        let settings = descriptor.settings.expect("settings metadata");
        assert_eq!(settings.title.as_deref(), Some("Display"));
        assert_eq!(settings.sections.len(), 1);
        assert_eq!(settings.sections[0].inputs, vec!["taskbarAutoHide"]);
        assert_eq!(
            settings.apply_actions,
            vec![
                "set-taskbar-autohide".to_string(),
                "set-resolution".to_string(),
                "set-scale".to_string()
            ]
        );
        assert_eq!(
            descriptor.inputs[1].options_source.as_deref(),
            Some("dynamicOptions.trayResolutionPresets")
        );
        let status = settings.status.expect("status metadata");
        assert_eq!(status.fields.len(), 1);
        assert_eq!(status.fields[0].key, "lastActionUnix");
        let tray = descriptor.tray.expect("tray metadata");
        assert_eq!(tray.provider, "windows-display");
        assert_eq!(tray.title, "Windows Display Manager");
        assert_eq!(
            tray.tooltip.as_deref(),
            Some("Taskbar and display shortcuts")
        );
    }
}

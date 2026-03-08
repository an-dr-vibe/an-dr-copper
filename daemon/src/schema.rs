use crate::descriptor::{Descriptor, SUPPORTED_SCHEMA_URL};
use jsonschema::JSONSchema;
use thiserror::Error;

static SCHEMA_JSON: &str = include_str!("../../schemas/extension/1.0.0/descriptor.schema.json");

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

pub fn validator() -> Result<JSONSchema, ValidationError> {
    let schema_value: serde_json::Value =
        serde_json::from_str(SCHEMA_JSON).map_err(ValidationError::InvalidJson)?;
    JSONSchema::compile(&schema_value)
        .map_err(|e| ValidationError::SchemaCompilation(e.to_string()))
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
}

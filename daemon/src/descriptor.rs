use semver::Version;
use serde::{Deserialize, Serialize};

pub const SUPPORTED_SCHEMA_URL: &str =
    "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    Fs,
    Shell,
    Network,
    Store,
    Ui,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum InputType {
    Text,
    Number,
    Boolean,
    FolderPicker,
    FilePicker,
    Select,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputField {
    pub id: String,
    #[serde(rename = "type")]
    pub field_type: InputType,
    pub label: String,
    #[serde(default)]
    pub default: serde_json::Value,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiDescriptor {
    #[serde(rename = "type")]
    pub ui_type: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub on_select: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Descriptor {
    #[serde(default, rename = "$schema")]
    pub schema: Option<String>,
    pub id: String,
    pub name: String,
    pub version: String,
    pub trigger: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub inputs: Vec<InputField>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub ui: Option<UiDescriptor>,
}

impl Descriptor {
    pub fn parsed_version(&self) -> Result<Version, semver::Error> {
        Version::parse(&self.version)
    }
}

use semver::Version;
use serde::{Deserialize, Serialize};

pub const SUPPORTED_SCHEMA_URL: &str =
    "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json";
pub const COMPONENT_ABI_V1: &str = "copper.component/1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    WasmComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDescriptor {
    pub kind: RuntimeKind,
    pub abi: String,
    pub artifact: String,
    #[serde(default)]
    pub background: Option<BackgroundDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundDescriptor {
    pub action: String,
    #[serde(default)]
    pub enabled_config: Option<String>,
    #[serde(default)]
    pub enabled_by_default: bool,
    #[serde(default)]
    pub interval_seconds_config: Option<String>,
    pub default_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    Fs,
    Keyboard,
    Network,
    SecureStore,
    Shell,
    Store,
    Ui,
    WindowsDisplay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum InputType {
    Text,
    Number,
    Boolean,
    FolderPicker,
    FilePicker,
    Hotkey,
    Select,
    ListSelect,
    MultiSelect,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputField {
    pub id: String,
    #[serde(rename = "type")]
    pub field_type: InputType,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: serde_json::Value,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default, rename = "optionsSource")]
    pub options_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
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
pub struct TrayDescriptor {
    pub provider: String,
    pub title: String,
    #[serde(default)]
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum StatusFieldFormat {
    Text,
    Boolean,
    Number,
    Path,
    DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingsSection {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingsTab {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default, rename = "showStatus")]
    pub show_status: bool,
    #[serde(default, rename = "showCommands")]
    pub show_commands: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub format: Option<StatusFieldFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusDescriptor {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Vec<StatusField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingsDescriptor {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "applyActions")]
    pub apply_actions: Vec<String>,
    #[serde(default)]
    pub tabs: Vec<SettingsTab>,
    #[serde(default)]
    pub sections: Vec<SettingsSection>,
    #[serde(default)]
    pub status: Option<StatusDescriptor>,
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
    pub runtime: Option<RuntimeDescriptor>,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub inputs: Vec<InputField>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub ui: Option<UiDescriptor>,
    #[serde(default)]
    pub settings: Option<SettingsDescriptor>,
    #[serde(default)]
    pub tray: Option<TrayDescriptor>,
}

impl Descriptor {
    pub fn parsed_version(&self) -> Result<Version, semver::Error> {
        Version::parse(&self.version)
    }
}

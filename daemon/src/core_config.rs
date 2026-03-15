use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoreConfig {
    pub disabled_extensions: BTreeSet<String>,
}

impl CoreConfig {
    pub fn is_extension_enabled(&self, extension_id: &str) -> bool {
        !self.disabled_extensions.contains(extension_id)
    }
}

pub fn load_core_config() -> Result<CoreConfig, std::io::Error> {
    let data_root = copper_data_root()?;
    load_core_config_from(&data_root)
}

pub fn load_core_config_from(data_root: &Path) -> Result<CoreConfig, std::io::Error> {
    let path = core_data_path_in(data_root);
    if !path.exists() {
        return Ok(CoreConfig::default());
    }

    let raw = fs::read_to_string(path)?;
    let parsed: Value = serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));

    let disabled_extensions = parsed
        .get("disabledExtensions")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    Ok(CoreConfig {
        disabled_extensions,
    })
}

pub fn core_data_path_in(data_root: &Path) -> PathBuf {
    data_root.join("copper-core").join("data.json")
}

fn copper_data_root() -> Result<PathBuf, std::io::Error> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot resolve home directory for Copper data",
        )
    })?;
    Ok(copper_data_root_from_home(&home))
}

fn copper_data_root_from_home(home: &Path) -> PathBuf {
    home.join(".Copper").join("extensions")
}

#[cfg(test)]
mod tests {
    use super::{core_data_path_in, load_core_config_from, CoreConfig};
    use std::collections::BTreeSet;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn missing_core_config_defaults_to_all_enabled() {
        let temp = tempdir().expect("tempdir");
        let config = load_core_config_from(temp.path()).expect("load");
        assert_eq!(config, CoreConfig::default());
        assert!(config.is_extension_enabled("alpha-ext"));
    }

    #[test]
    fn core_config_reads_disabled_extensions() {
        let temp = tempdir().expect("tempdir");
        let path = core_data_path_in(temp.path());
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(
            &path,
            r#"{
                "disabledExtensions": ["alpha-ext", "beta-ext", 42]
            }"#,
        )
        .expect("write config");

        let config = load_core_config_from(temp.path()).expect("load");
        assert_eq!(
            config.disabled_extensions,
            BTreeSet::from(["alpha-ext".to_string(), "beta-ext".to_string()])
        );
        assert!(!config.is_extension_enabled("alpha-ext"));
        assert!(config.is_extension_enabled("gamma-ext"));
    }

    #[test]
    fn core_config_ignores_invalid_payloads() {
        let temp = tempdir().expect("tempdir");
        let path = core_data_path_in(temp.path());
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(&path, r#"{"disabledExtensions":"nope"}"#).expect("write config");

        let config = load_core_config_from(temp.path()).expect("load");
        assert!(config.disabled_extensions.is_empty());
    }
}

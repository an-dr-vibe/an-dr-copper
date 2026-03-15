use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const COPPER_HOME_DIR: &str = ".Copper";
const EXTENSIONS_DIR: &str = "extensions";
const CORE_EXTENSION_ID: &str = "copper-core";

#[derive(Debug, Clone)]
pub struct ExtensionStateStore {
    data_root: PathBuf,
}

impl ExtensionStateStore {
    pub fn new(data_root: PathBuf) -> Self {
        Self { data_root }
    }

    pub fn for_current_user() -> Result<Self, std::io::Error> {
        let home = dirs::home_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not available")
        })?;
        Ok(Self::from_home_dir(home))
    }

    pub fn from_home_dir(home: impl Into<PathBuf>) -> Self {
        Self::new(home.into().join(COPPER_HOME_DIR).join(EXTENSIONS_DIR))
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn core_config_path(&self) -> PathBuf {
        self.config_path(CORE_EXTENSION_ID)
    }

    pub fn config_path(&self, extension_id: &str) -> PathBuf {
        self.data_root.join(extension_id).join("config.json")
    }

    pub fn status_path(&self, extension_id: &str) -> PathBuf {
        self.data_root.join(extension_id).join("status.json")
    }

    pub fn legacy_path(&self, extension_id: &str) -> PathBuf {
        self.data_root.join(extension_id).join("data.json")
    }

    pub fn load_config(&self, extension_id: &str) -> Result<Value, std::io::Error> {
        self.load_path_or_legacy(
            &self.config_path(extension_id),
            Some(&self.legacy_path(extension_id)),
        )
    }

    pub fn load_status(&self, extension_id: &str) -> Result<Value, std::io::Error> {
        self.load_path_or_legacy(
            &self.status_path(extension_id),
            Some(&self.legacy_path(extension_id)),
        )
    }

    pub fn load_path_or_legacy(
        &self,
        path: &Path,
        legacy_path: Option<&Path>,
    ) -> Result<Value, std::io::Error> {
        if path.exists() {
            return read_json_object(path);
        }
        if let Some(legacy_path) = legacy_path {
            if legacy_path.exists() {
                return read_json_object(legacy_path);
            }
        }
        Ok(serde_json::json!({}))
    }

    pub fn write_config(&self, extension_id: &str, value: &Value) -> Result<(), std::io::Error> {
        write_json_object(&self.config_path(extension_id), value)
    }

    pub fn write_status(&self, extension_id: &str, value: &Value) -> Result<(), std::io::Error> {
        write_json_object(&self.status_path(extension_id), value)
    }

    pub fn merge_config(&self, extension_id: &str, value: &Value) -> Result<Value, std::io::Error> {
        merge_json_object(&self.config_path(extension_id), value)
    }

    pub fn ensure_root(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.data_root)
    }
}

pub fn read_json_object(path: &Path) -> Result<Value, std::io::Error> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let raw = fs::read_to_string(path)?;
    Ok(sanitize_object(
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| serde_json::json!({})),
    ))
}

pub fn write_json_object(path: &Path, value: &Value) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_string_pretty(&sanitize_object(value.clone()))?,
    )
}

pub fn merge_json_object(path: &Path, value: &Value) -> Result<Value, std::io::Error> {
    let mut merged = read_json_object(path)?;
    if let (Some(target), Some(source)) = (merged.as_object_mut(), value.as_object()) {
        let remove_keys = source
            .get("__remove")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();

        for (key, item) in source {
            if key == "__remove" {
                continue;
            }
            target.insert(key.clone(), item.clone());
        }
        for key in remove_keys {
            target.remove(&key);
        }
    } else {
        merged = sanitize_object(value.clone());
    }

    write_json_object(path, &merged)?;
    Ok(merged)
}

pub fn sanitize_object(value: Value) -> Value {
    if value.is_object() {
        value
    } else {
        serde_json::json!({})
    }
}

pub fn unix_now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        merge_json_object, read_json_object, unix_now_secs, write_json_object, ExtensionStateStore,
    };
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn store_uses_expected_paths() {
        let store = ExtensionStateStore::new("C:/tmp/.Copper/extensions".into());
        assert_eq!(
            store.config_path("alpha-ext"),
            std::path::PathBuf::from("C:/tmp/.Copper/extensions/alpha-ext/config.json")
        );
        assert_eq!(
            store.status_path("alpha-ext"),
            std::path::PathBuf::from("C:/tmp/.Copper/extensions/alpha-ext/status.json")
        );
        assert_eq!(
            store.legacy_path("alpha-ext"),
            std::path::PathBuf::from("C:/tmp/.Copper/extensions/alpha-ext/data.json")
        );
    }

    #[test]
    fn json_object_helpers_roundtrip_and_merge() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");

        write_json_object(&path, &serde_json::json!({"desktopFolder":"~/Desktop"})).expect("write");
        let merged = merge_json_object(
            &path,
            &serde_json::json!({
                "desktopFolder": "D:/Desktop",
                "__remove": ["missingKey"]
            }),
        )
        .expect("merge");
        assert_eq!(
            merged.get("desktopFolder").and_then(|value| value.as_str()),
            Some("D:/Desktop")
        );

        fs::write(&path, "[]").expect("write bad object");
        assert_eq!(
            read_json_object(&path).expect("read"),
            serde_json::json!({})
        );
    }

    #[test]
    fn unix_now_secs_returns_non_zeroish_timestamp() {
        assert!(unix_now_secs() > 1_700_000_000);
    }
}

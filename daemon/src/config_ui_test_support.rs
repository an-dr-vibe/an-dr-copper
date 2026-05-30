use super::UiConfigError;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn core_data_path_for(data_root: &Path) -> PathBuf {
    extension_config_path_for(data_root, "copper-core")
}

pub(super) fn extension_config_path_for(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("config.json")
}

pub(super) fn extension_status_path_for(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("status.json")
}

pub(super) fn load_config(path: &Path) -> Result<Value, UiConfigError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let raw = fs::read_to_string(path)?;
    let parsed: Value = serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
    Ok(if parsed.is_object() {
        parsed
    } else {
        serde_json::json!({})
    })
}

pub(super) fn store_config(path: &Path, value: &Value) -> Result<(), UiConfigError> {
    let mut merged = load_config(path)?;
    if let (Some(target), Some(source)) = (merged.as_object_mut(), value.as_object()) {
        let mut remove_keys = Vec::new();
        if let Some(remove) = source.get("__remove").and_then(|v| v.as_array()) {
            for item in remove {
                if let Some(key) = item.as_str() {
                    remove_keys.push(key.to_string());
                }
            }
        }

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
        merged = value.clone();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&merged)?)?;
    Ok(())
}

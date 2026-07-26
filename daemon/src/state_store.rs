use serde::Serialize;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const COPPER_HOME_DIR: &str = ".Copper";
const EXTENSIONS_DIR: &str = "extensions";
const CORE_EXTENSION_ID: &str = "copper-core";

#[derive(Debug, Clone)]
pub struct ExtensionStateStore {
    data_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateStoreWarning {
    pub scope: String,
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct LoadedState {
    pub value: Value,
    pub warnings: Vec<StateStoreWarning>,
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
        Ok(self.inspect_config(extension_id)?.value)
    }

    pub fn load_status(&self, extension_id: &str) -> Result<Value, std::io::Error> {
        Ok(self.inspect_status(extension_id)?.value)
    }

    pub fn load_store(&self, extension_id: &str) -> Result<Value, std::io::Error> {
        self.load_path_or_legacy(&self.legacy_path(extension_id), None)
    }

    pub fn inspect_core_config(&self) -> Result<LoadedState, std::io::Error> {
        self.inspect_path_or_legacy(
            "copper-core.config",
            &self.core_config_path(),
            Some(&self.legacy_path(CORE_EXTENSION_ID)),
        )
    }

    pub fn inspect_config(&self, extension_id: &str) -> Result<LoadedState, std::io::Error> {
        self.inspect_path_or_legacy(
            format!("{extension_id}.config"),
            &self.config_path(extension_id),
            Some(&self.legacy_path(extension_id)),
        )
    }

    pub fn inspect_status(&self, extension_id: &str) -> Result<LoadedState, std::io::Error> {
        self.inspect_path(
            format!("{extension_id}.status"),
            &self.status_path(extension_id),
        )
    }

    pub fn load_path_or_legacy(
        &self,
        path: &Path,
        legacy_path: Option<&Path>,
    ) -> Result<Value, std::io::Error> {
        Ok(self
            .inspect_path_or_legacy("state", path, legacy_path)?
            .value)
    }

    pub fn inspect_path_or_legacy(
        &self,
        scope: impl Into<String>,
        path: &Path,
        legacy_path: Option<&Path>,
    ) -> Result<LoadedState, std::io::Error> {
        let scope = scope.into();
        if path.exists() {
            return read_json_object_detailed(path, scope);
        }
        if let Some(legacy_path) = legacy_path {
            if legacy_path.exists() {
                let mut loaded = read_json_object_detailed(legacy_path, scope.clone())?;
                loaded.warnings.insert(
                    0,
                    StateStoreWarning {
                        scope,
                        code: "legacy-path-in-use".to_string(),
                        path: legacy_path.display().to_string(),
                        message: format!(
                            "Using legacy state file '{}' because the primary file is missing",
                            legacy_path.display()
                        ),
                    },
                );
                return Ok(loaded);
            }
        }
        Ok(LoadedState {
            value: serde_json::json!({}),
            warnings: Vec::new(),
        })
    }

    pub fn inspect_path(
        &self,
        scope: impl Into<String>,
        path: &Path,
    ) -> Result<LoadedState, std::io::Error> {
        if path.exists() {
            return read_json_object_detailed(path, scope);
        }
        Ok(LoadedState {
            value: serde_json::json!({}),
            warnings: Vec::new(),
        })
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

    pub fn merge_status(&self, extension_id: &str, value: &Value) -> Result<Value, std::io::Error> {
        merge_json_object(&self.status_path(extension_id), value)
    }

    pub fn merge_store(&self, extension_id: &str, value: &Value) -> Result<Value, std::io::Error> {
        merge_json_object(&self.legacy_path(extension_id), value)
    }

    pub fn set_store_value(
        &self,
        extension_id: &str,
        key: &str,
        value: Value,
    ) -> Result<(), std::io::Error> {
        let mut store = self.load_store(extension_id)?;
        if let Some(object) = store.as_object_mut() {
            object.insert(key.to_string(), value);
        }
        write_json_object(&self.legacy_path(extension_id), &store)
    }

    pub fn ensure_root(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.data_root)
    }

    pub fn collect_warnings(&self) -> Result<Vec<StateStoreWarning>, std::io::Error> {
        if !self.data_root.exists() {
            return Ok(Vec::new());
        }

        let mut warnings = Vec::new();
        for entry in fs::read_dir(&self.data_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let extension_id = match entry.file_name().into_string() {
                Ok(value) => value,
                Err(_) => continue,
            };

            warnings.extend(self.inspect_config(&extension_id)?.warnings);
            warnings.extend(self.inspect_status(&extension_id)?.warnings);
        }
        Ok(warnings)
    }
}

pub fn read_json_object(path: &Path) -> Result<Value, std::io::Error> {
    Ok(read_json_object_detailed(path, "state")?.value)
}

pub fn write_json_object(path: &Path, value: &Value) -> Result<(), std::io::Error> {
    let payload = serde_json::to_vec_pretty(&sanitize_object(value.clone()))
        .map_err(std::io::Error::other)?;
    write_file_atomically(path, &payload)
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

fn read_json_object_detailed(
    path: &Path,
    scope: impl Into<String>,
) -> Result<LoadedState, std::io::Error> {
    if !path.exists() {
        return Ok(LoadedState {
            value: serde_json::json!({}),
            warnings: Vec::new(),
        });
    }

    let scope = scope.into();
    let raw = fs::read_to_string(path)?;
    match serde_json::from_str::<Value>(&raw) {
        Ok(parsed) if parsed.is_object() => Ok(LoadedState {
            value: parsed,
            warnings: Vec::new(),
        }),
        Ok(_) => Ok(LoadedState {
            value: serde_json::json!({}),
            warnings: vec![StateStoreWarning {
                scope,
                code: "non-object-json".to_string(),
                path: path.display().to_string(),
                message: format!(
                    "State file '{}' must contain a top-level JSON object",
                    path.display()
                ),
            }],
        }),
        Err(err) => Ok(LoadedState {
            value: serde_json::json!({}),
            warnings: vec![StateStoreWarning {
                scope,
                code: "invalid-json".to_string(),
                path: path.display().to_string(),
                message: format!("Failed to parse state file '{}': {err}", path.display()),
            }],
        }),
    }
}

fn write_file_atomically(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = atomic_temp_path(path);
    let write_result = (|| -> Result<(), std::io::Error> {
        let mut file = File::create(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        replace_file(&temp_path, path)
    })();

    if write_result.is_err() && temp_path.exists() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn atomic_temp_path(path: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("state.json");
    let unique = format!(".{file_name}.{}.{}.tmp", std::process::id(), nanos);
    path.with_file_name(unique)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::rename(source, destination)
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
    fn scoped_store_config_and_status_never_cross_extension_roots() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        store
            .merge_store("alpha-ext", &serde_json::json!({"count": 1}))
            .expect("alpha store");
        store
            .merge_config("alpha-ext", &serde_json::json!({"enabled": true}))
            .expect("alpha config");
        store
            .merge_status("beta-ext", &serde_json::json!({"running": true}))
            .expect("beta status");

        assert_eq!(
            store
                .load_store("alpha-ext")
                .expect("load store")
                .get("count"),
            Some(&serde_json::json!(1))
        );
        assert!(store
            .load_store("beta-ext")
            .expect("beta store")
            .as_object()
            .is_some_and(|value| value.is_empty()));
        assert_eq!(
            store
                .load_config("alpha-ext")
                .expect("alpha config")
                .get("enabled"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            store
                .load_status("beta-ext")
                .expect("beta status")
                .get("running"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn inspect_config_reports_invalid_json_warning() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let path = store.config_path("alpha-ext");
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(&path, "{bad-json").expect("write invalid");

        let loaded = store.inspect_config("alpha-ext").expect("inspect");
        assert_eq!(loaded.value, serde_json::json!({}));
        assert_eq!(loaded.warnings.len(), 1);
        assert_eq!(loaded.warnings[0].code, "invalid-json");
        assert!(loaded.warnings[0].scope.contains("alpha-ext.config"));
    }

    #[test]
    fn inspect_config_reports_legacy_fallback_warning() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let legacy = store.legacy_path("alpha-ext");
        fs::create_dir_all(legacy.parent().expect("parent")).expect("create parent");
        fs::write(&legacy, r#"{"count":1}"#).expect("write legacy");

        let loaded = store.inspect_config("alpha-ext").expect("inspect");
        assert_eq!(
            loaded.value.get("count").and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(loaded.warnings.len(), 1);
        assert_eq!(loaded.warnings[0].code, "legacy-path-in-use");
    }

    #[test]
    fn inspect_status_does_not_use_legacy_data_file() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let legacy = store.legacy_path("alpha-ext");
        fs::create_dir_all(legacy.parent().expect("parent")).expect("create parent");
        fs::write(&legacy, r#"{"count":1}"#).expect("write legacy");

        let loaded = store.inspect_status("alpha-ext").expect("inspect");
        assert_eq!(loaded.value, serde_json::json!({}));
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn collect_warnings_scans_each_extension_directory() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let config = store.config_path("alpha-ext");
        fs::create_dir_all(config.parent().expect("parent")).expect("create parent");
        fs::write(config, "[]").expect("write invalid config");

        let warnings = store.collect_warnings().expect("warnings");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "non-object-json");
    }

    #[test]
    fn write_json_object_replaces_existing_file_contents() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");
        fs::write(&path, r#"{"stale":true}"#).expect("seed");

        write_json_object(&path, &serde_json::json!({"fresh": true})).expect("write");

        let written = read_json_object(&path).expect("read");
        assert!(written.get("stale").is_none());
        assert_eq!(
            written.get("fresh").and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn unix_now_secs_returns_non_zeroish_timestamp() {
        assert!(unix_now_secs() > 1_700_000_000);
    }
}

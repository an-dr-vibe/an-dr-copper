use std::path::Path;

pub fn get(store_path: &str, key: &str) -> Option<serde_json::Value> {
    let data = std::fs::read_to_string(store_path).ok()?;
    let obj: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&data).ok()?;
    obj.get(key).cloned()
}

pub fn set(store_path: &str, key: &str, value: serde_json::Value) -> Result<(), std::io::Error> {
    let mut obj: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(store_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default();

    if let Some(parent) = Path::new(store_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    obj.insert(key.to_string(), value);
    let serialized = serde_json::to_string_pretty(&obj).map_err(std::io::Error::other)?;
    std::fs::write(store_path, serialized)
}

#[cfg(test)]
mod tests {
    use super::{get, set};
    use tempfile::tempdir;

    #[test]
    fn set_and_get_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("store.json").display().to_string();
        set(&path, "answer", serde_json::json!(42)).expect("set");
        assert_eq!(get(&path, "answer").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn get_on_missing_file_returns_none() {
        assert!(get("/no/such/path/store.json", "key").is_none());
    }

    #[test]
    fn get_missing_key_returns_none() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("store.json").display().to_string();
        set(&path, "a", serde_json::json!(1)).expect("set");
        assert!(get(&path, "b").is_none());
    }

    #[test]
    fn set_overwrites_existing_value() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("store.json").display().to_string();
        set(&path, "key", serde_json::json!("old")).expect("set");
        set(&path, "key", serde_json::json!("new")).expect("set");
        assert_eq!(
            get(&path, "key").as_ref().and_then(|v| v.as_str()),
            Some("new")
        );
    }

    #[test]
    fn set_creates_intermediate_dirs() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sub/dir/store.json").display().to_string();
        set(&path, "k", serde_json::json!("v")).expect("set with new dirs");
        assert_eq!(get(&path, "k").as_ref().and_then(|v| v.as_str()), Some("v"));
    }
}

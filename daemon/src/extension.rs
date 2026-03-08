use crate::descriptor::{Descriptor, Permission};
use crate::schema::{parse_and_validate, ValidationError};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Extension {
    pub root: PathBuf,
    pub descriptor: Descriptor,
    pub main_ts_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("descriptor validation error: {0}")]
    Validation(#[from] ValidationError),
    #[error("extension is missing required file: {0}")]
    MissingFile(String),
}

#[derive(Debug, Clone)]
pub struct Registry {
    entries: BTreeMap<String, Extension>,
}

impl Registry {
    pub fn load_from_dir(root: &Path) -> Result<Self, ExtensionError> {
        let mut entries = BTreeMap::new();
        if !root.exists() {
            return Ok(Self { entries });
        }

        for entry in WalkDir::new(root).min_depth(1).max_depth(1) {
            let entry = match entry {
                Ok(v) => v,
                Err(err) => {
                    return Err(ExtensionError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        err.to_string(),
                    )));
                }
            };
            if !entry.file_type().is_dir() {
                continue;
            }
            let folder = entry.into_path();
            let descriptor_path = folder.join("descriptor.json");
            let main_ts_path = folder.join("main.ts");
            if !descriptor_path.exists() {
                continue;
            }
            if !main_ts_path.exists() {
                return Err(ExtensionError::MissingFile(format!(
                    "{} does not contain main.ts",
                    folder.display()
                )));
            }
            let descriptor_raw = fs::read_to_string(&descriptor_path)?;
            let descriptor = parse_and_validate(&descriptor_raw)?;
            entries.insert(
                descriptor.id.clone(),
                Extension {
                    root: folder,
                    descriptor,
                    main_ts_path,
                },
            );
        }
        Ok(Self { entries })
    }

    pub fn list(&self) -> impl Iterator<Item = &Extension> {
        self.entries.values()
    }

    pub fn get(&self, id: &str) -> Option<&Extension> {
        self.entries.get(id)
    }
}

pub fn default_extensions_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".Copper").join("extensions");
    }
    PathBuf::from(".").join("extensions")
}

pub fn check_permission(ext: &Extension, permission: Permission) -> bool {
    ext.descriptor.permissions.contains(&permission)
}

#[cfg(test)]
mod tests {
    use super::{check_permission, ExtensionError, Registry};
    use crate::descriptor::Permission;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_extensions_from_directory() {
        let temp = tempdir().expect("tempdir");
        let ext_dir = temp.path().join("sort-downloads");
        fs::create_dir_all(&ext_dir).expect("create extension dir");
        fs::write(
            ext_dir.join("descriptor.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "sort-downloads",
                "name": "Sort Downloads",
                "version": "1.0.0",
                "trigger": "sort-dl",
                "permissions": ["fs"],
                "actions": [
                    { "id": "sort", "label": "Sort by extension", "script": "return;" }
                ]
            }"#,
        )
        .expect("write descriptor");
        fs::write(
            ext_dir.join("main.ts"),
            "export default function(){ return {}; }",
        )
        .expect("write main.ts");

        let registry = Registry::load_from_dir(temp.path()).expect("load registry");
        let extension = registry.get("sort-downloads").expect("extension exists");
        assert!(check_permission(extension, Permission::Fs));
        assert!(!check_permission(extension, Permission::Shell));
    }

    #[test]
    fn ignores_folders_without_descriptor() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("notes")).expect("create dir");
        let registry = Registry::load_from_dir(temp.path()).expect("load");
        assert_eq!(registry.list().count(), 0);
    }

    #[test]
    fn returns_empty_registry_if_directory_is_missing() {
        let temp = tempdir().expect("tempdir");
        let missing = temp.path().join("does-not-exist");
        let registry = Registry::load_from_dir(&missing).expect("load");
        assert_eq!(registry.list().count(), 0);
    }

    #[test]
    fn fails_if_descriptor_exists_without_main_ts() {
        let temp = tempdir().expect("tempdir");
        let ext_dir = temp.path().join("broken-ext");
        fs::create_dir_all(&ext_dir).expect("create extension dir");
        fs::write(
            ext_dir.join("descriptor.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "broken-ext",
                "name": "Broken Ext",
                "version": "1.0.0",
                "trigger": "broken",
                "actions": [
                    { "id": "run", "label": "Run", "script": "return;" }
                ]
            }"#,
        )
        .expect("write descriptor");

        let error = Registry::load_from_dir(temp.path()).expect_err("should fail without main.ts");
        match error {
            ExtensionError::MissingFile(message) => assert!(message.contains("main.ts")),
            other => panic!("unexpected error: {other}"),
        }
    }
}

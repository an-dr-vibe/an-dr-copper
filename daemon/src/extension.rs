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
    #[error("manifest validation error: {0}")]
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
        Self::load_from_dirs([root])
    }

    pub fn load_from_dirs<'a, I>(roots: I) -> Result<Self, ExtensionError>
    where
        I: IntoIterator<Item = &'a Path>,
    {
        let mut entries = BTreeMap::new();

        for root in roots {
            if !root.exists() {
                continue;
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
                let descriptor_path = folder.join("manifest.json");
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

pub fn core_extensions_dir() -> Option<PathBuf> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;
    core_extensions_dir_from_exe_dir(exe_dir)
}

pub fn runtime_extension_roots(user_extensions_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            roots.extend(core_extension_roots_from_exe_dir(exe_dir));
        }
    } else if let Some(core) = core_extensions_dir() {
        roots.push(core);
    }
    roots.push(user_extensions_dir.to_path_buf());

    let mut deduped = Vec::new();
    for root in roots {
        let normalized = fs::canonicalize(&root).unwrap_or(root);
        if !deduped
            .iter()
            .any(|existing: &PathBuf| existing == &normalized)
        {
            deduped.push(normalized);
        }
    }
    deduped
}

pub fn load_runtime_registry(user_extensions_dir: &Path) -> Result<Registry, ExtensionError> {
    let roots = runtime_extension_roots(user_extensions_dir);
    Registry::load_from_dirs(roots.iter().map(PathBuf::as_path))
}

fn core_extensions_dir_from_exe_dir(exe_dir: &Path) -> Option<PathBuf> {
    core_extension_roots_from_exe_dir(exe_dir)
        .into_iter()
        .next_back()
}

fn core_extension_roots_from_exe_dir(exe_dir: &Path) -> Vec<PathBuf> {
    let candidates = [
        exe_dir.join("..").join("..").join("extensions"),
        exe_dir.join("..").join("extensions"),
        exe_dir.join("extensions"),
        // Backward compatibility for older bundles:
        exe_dir.join("..").join("core-extensions"),
        exe_dir.join("core-extensions"),
    ];

    let mut roots = Vec::new();
    for candidate in candidates {
        if candidate.exists()
            && !roots
                .iter()
                .any(|existing: &PathBuf| existing == &candidate)
        {
            roots.push(candidate);
        }
    }
    roots
}

pub fn check_permission(ext: &Extension, permission: Permission) -> bool {
    ext.descriptor.permissions.contains(&permission)
}

#[cfg(test)]
mod tests {
    use super::{
        check_permission, core_extension_roots_from_exe_dir, core_extensions_dir_from_exe_dir,
        runtime_extension_roots, ExtensionError, Registry,
    };
    use crate::descriptor::Permission;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn loads_extensions_from_directory() {
        let temp = tempdir().expect("tempdir");
        let ext_dir = temp.path().join("sort-downloads");
        fs::create_dir_all(&ext_dir).expect("create extension dir");
        fs::write(
            ext_dir.join("manifest.json"),
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
            ext_dir.join("manifest.json"),
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

    #[test]
    fn load_from_dirs_allows_later_root_to_override_extension_id() {
        let temp = tempdir().expect("tempdir");
        let core_root = temp.path().join("core");
        let user_root = temp.path().join("user");
        fs::create_dir_all(core_root.join("same-id")).expect("core dir");
        fs::create_dir_all(user_root.join("same-id")).expect("user dir");

        fs::write(
            core_root.join("same-id/manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "same-id",
                "name": "Core Extension",
                "version": "1.0.0",
                "trigger": "core",
                "actions": [{ "id": "run", "label": "Run", "script": "return;" }]
            }"#,
        )
        .expect("core descriptor");
        fs::write(
            core_root.join("same-id/main.ts"),
            "export default function(){}",
        )
        .expect("core main");

        fs::write(
            user_root.join("same-id/manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "same-id",
                "name": "User Extension",
                "version": "1.0.0",
                "trigger": "user",
                "actions": [{ "id": "run", "label": "Run", "script": "return;" }]
            }"#,
        )
        .expect("user descriptor");
        fs::write(
            user_root.join("same-id/main.ts"),
            "export default function(){}",
        )
        .expect("user main");

        let registry =
            Registry::load_from_dirs([core_root.as_path(), user_root.as_path()]).expect("registry");
        let ext = registry.get("same-id").expect("extension");
        assert_eq!(ext.descriptor.name, "User Extension");
    }

    #[test]
    fn runtime_roots_deduplicate_identical_paths() {
        let user = PathBuf::from("C:/tmp/extensions");
        let roots = runtime_extension_roots(&user);
        assert!(!roots.is_empty());
    }

    #[test]
    fn core_extensions_detects_candidate_path() {
        let temp = tempdir().expect("tempdir");
        let exe_dir = temp.path().join("bin");
        fs::create_dir_all(exe_dir.join("extensions")).expect("extensions dir");
        let detected = core_extensions_dir_from_exe_dir(&exe_dir);
        assert_eq!(detected, Some(exe_dir.join("extensions")));
    }

    #[test]
    fn core_extensions_detects_parent_extensions_candidate() {
        let temp = tempdir().expect("tempdir");
        let exe_dir = temp.path().join("bin").join("target");
        fs::create_dir_all(temp.path().join("bin").join("extensions")).expect("extensions dir");
        let detected = core_extensions_dir_from_exe_dir(&exe_dir);
        assert_eq!(detected, Some(exe_dir.join("..").join("extensions")));
    }

    #[test]
    fn core_extensions_detects_legacy_core_extensions_candidate() {
        let temp = tempdir().expect("tempdir");
        let exe_dir = temp.path().join("bin");
        fs::create_dir_all(exe_dir.join("core-extensions")).expect("core-extensions dir");
        let detected = core_extensions_dir_from_exe_dir(&exe_dir);
        assert_eq!(detected, Some(exe_dir.join("core-extensions")));
    }

    #[test]
    fn core_extension_roots_include_workspace_fallback_before_adjacent_root() {
        let temp = tempdir().expect("tempdir");
        let exe_dir = temp.path().join("target").join("debug");
        fs::create_dir_all(temp.path().join("extensions")).expect("workspace extensions");
        fs::create_dir_all(exe_dir.join("extensions")).expect("adjacent extensions");

        let detected = core_extension_roots_from_exe_dir(&exe_dir)
            .into_iter()
            .map(|path| fs::canonicalize(path).expect("canonical path"))
            .collect::<Vec<_>>();

        assert_eq!(
            detected,
            vec![
                fs::canonicalize(temp.path().join("extensions")).expect("workspace canonical path"),
                fs::canonicalize(exe_dir.join("extensions")).expect("adjacent canonical path"),
            ]
        );
    }
}

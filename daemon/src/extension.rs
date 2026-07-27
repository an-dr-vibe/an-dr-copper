use crate::core_config::{load_core_config, CoreConfig};
use crate::descriptor::{Descriptor, Permission, Platform};
use crate::schema::{parse_and_validate, ValidationError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Extension {
    pub root: PathBuf,
    pub descriptor: Descriptor,
    pub wasm_component_path: PathBuf,
}

impl Extension {
    pub fn runtime_artifact_path(&self) -> &Path {
        &self.wasm_component_path
    }
}

#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest validation error: {0}")]
    Validation(#[from] ValidationError),
    #[error("extension is missing required file: {0}")]
    MissingFile(String),
    #[error("invalid extension runtime artifact: {0}")]
    InvalidArtifact(String),
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
                if !descriptor_path.exists() {
                    continue;
                }
                let descriptor_raw = fs::read_to_string(&descriptor_path)?;
                let descriptor = parse_and_validate(&descriptor_raw)?;
                let wasm_component_path = match &descriptor.runtime {
                    Some(runtime) => {
                        let expected = format!("{}.wasm", descriptor.id);
                        if runtime.artifact != expected {
                            return Err(ExtensionError::InvalidArtifact(format!(
                                "extension {} artifact must be named {expected}, got {}",
                                descriptor.id, runtime.artifact
                            )));
                        }
                        let artifact_path = folder.join(&runtime.artifact);
                        if !artifact_path.exists() {
                            return Err(ExtensionError::MissingFile(format!(
                                "{} does not contain {}",
                                folder.display(),
                                runtime.artifact
                            )));
                        }
                        if !fs::metadata(&artifact_path)?.is_file() {
                            return Err(ExtensionError::InvalidArtifact(format!(
                                "extension {} artifact {} is not a file",
                                descriptor.id,
                                artifact_path.display()
                            )));
                        }
                        let canonical_root = fs::canonicalize(&folder)?;
                        let canonical_artifact = fs::canonicalize(&artifact_path)?;
                        if canonical_artifact.parent() != Some(canonical_root.as_path()) {
                            return Err(ExtensionError::InvalidArtifact(format!(
                                "extension {} artifact resolves outside its package",
                                descriptor.id
                            )));
                        }
                        artifact_path
                    }
                    None => return Err(ExtensionError::InvalidArtifact(format!(
                        "extension {} must declare a wasm-component runtime; TypeScript compatibility was removed",
                        descriptor.id
                    ))),
                };
                entries.insert(
                    descriptor.id.clone(),
                    Extension {
                        root: folder,
                        descriptor,
                        wasm_component_path,
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

    pub fn filter_for_runtime(mut self, core_config: &CoreConfig, platform: Platform) -> Self {
        self.entries.retain(|extension_id, extension| {
            core_config.is_extension_enabled(extension_id) && supports_platform(extension, platform)
        });
        self
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
    let registry = load_discoverable_registry(user_extensions_dir)?;
    let core_config = load_core_config()?;
    Ok(registry.filter_for_runtime(&core_config, current_platform()))
}

pub fn load_discoverable_registry(user_extensions_dir: &Path) -> Result<Registry, ExtensionError> {
    let roots = runtime_extension_roots(user_extensions_dir);
    Registry::load_from_dirs(roots.iter().map(PathBuf::as_path))
}

fn core_extensions_dir_from_exe_dir(exe_dir: &Path) -> Option<PathBuf> {
    core_extension_roots_from_exe_dir(exe_dir)
        .into_iter()
        .next_back()
}

fn core_extension_roots_from_exe_dir(exe_dir: &Path) -> Vec<PathBuf> {
    let parent = exe_dir.parent();
    let grandparent = parent.and_then(Path::parent);
    let mut candidates = Vec::with_capacity(5);
    candidates.push(exe_dir.join("extensions"));
    if let Some(parent) = parent {
        candidates.push(parent.join("extensions"));
    }
    if let Some(grandparent) = grandparent {
        candidates.push(grandparent.join("extensions"));
    }
    // Backward compatibility for older bundles:
    candidates.push(exe_dir.join("core-extensions"));
    if let Some(parent) = parent {
        candidates.push(parent.join("core-extensions"));
    }

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

pub fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    }
}

fn supports_platform(extension: &Extension, platform: Platform) -> bool {
    extension.descriptor.platforms.is_empty() || extension.descriptor.platforms.contains(&platform)
}

#[cfg(test)]
mod tests {
    use super::{
        check_permission, core_extension_roots_from_exe_dir, core_extensions_dir_from_exe_dir,
        current_platform, runtime_extension_roots, ExtensionError, Registry,
    };
    use crate::core_config::CoreConfig;
    use crate::descriptor::{Permission, Platform};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
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
                "runtime": {
                    "kind": "wasm-component",
                    "abi": "copper.component/1",
                    "artifact": "sort-downloads.wasm"
                },
                "permissions": ["fs"],
                "actions": [
                    { "id": "sort", "label": "Sort by extension", "script": "return;" }
                ]
            }"#,
        )
        .expect("write descriptor");
        fs::write(ext_dir.join("sort-downloads.wasm"), b"\0asm").expect("write component");

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
    fn fails_if_descriptor_omits_component_runtime() {
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

        let error = Registry::load_from_dir(temp.path()).expect_err("should require runtime");
        match error {
            ExtensionError::InvalidArtifact(message) => {
                assert!(message.contains("must declare a wasm-component runtime"))
            }
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
                "runtime": {"kind":"wasm-component","abi":"copper.component/1","artifact":"same-id.wasm"},
                "actions": [{ "id": "run", "label": "Run", "script": "return;" }]
            }"#,
        )
        .expect("core descriptor");
        fs::write(core_root.join("same-id/same-id.wasm"), b"\0asm").expect("core component");

        fs::write(
            user_root.join("same-id/manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "same-id",
                "name": "User Extension",
                "version": "1.0.0",
                "trigger": "user",
                "runtime": {"kind":"wasm-component","abi":"copper.component/1","artifact":"same-id.wasm"},
                "actions": [{ "id": "run", "label": "Run", "script": "return;" }]
            }"#,
        )
        .expect("user descriptor");
        fs::write(user_root.join("same-id/same-id.wasm"), b"\0asm").expect("user component");

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
        assert_eq!(
            detected.as_deref().map(normalize_path),
            Some(normalize_path(&temp.path().join("bin").join("extensions")))
        );
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
    fn core_extension_roots_prefer_adjacent_root_before_workspace_fallback() {
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
                fs::canonicalize(exe_dir.join("extensions")).expect("adjacent canonical path"),
                fs::canonicalize(temp.path().join("extensions")).expect("workspace canonical path"),
            ]
        );
    }

    #[test]
    fn runtime_core_roots_allow_workspace_manifest_to_override_stale_bundle() {
        let temp = tempdir().expect("tempdir");
        let exe_dir = temp.path().join("target").join("release");
        let workspace_root = temp.path().join("extensions");
        let bundled_root = exe_dir.join("extensions");
        fs::create_dir_all(workspace_root.join("same-id")).expect("workspace extension dir");
        fs::create_dir_all(bundled_root.join("same-id")).expect("bundled extension dir");

        fs::write(
            bundled_root.join("same-id").join("manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "same-id",
                "name": "Bundled Extension",
                "version": "1.0.0",
                "trigger": "bundle",
                "runtime": {"kind":"wasm-component","abi":"copper.component/1","artifact":"same-id.wasm"},
                "actions": [{ "id": "run", "label": "Run", "script": "return;" }]
            }"#,
        )
        .expect("write bundled descriptor");
        fs::write(bundled_root.join("same-id").join("same-id.wasm"), b"\0asm")
            .expect("write bundled component");

        fs::write(
            workspace_root.join("same-id").join("manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "same-id",
                "name": "Workspace Extension",
                "version": "1.0.0",
                "trigger": "workspace",
                "runtime": {"kind":"wasm-component","abi":"copper.component/1","artifact":"same-id.wasm"},
                "actions": [{ "id": "run", "label": "Run", "script": "return;" }]
            }"#,
        )
        .expect("write workspace descriptor");
        fs::write(
            workspace_root.join("same-id").join("same-id.wasm"),
            b"\0asm",
        )
        .expect("write workspace component");

        let roots = core_extension_roots_from_exe_dir(&exe_dir);
        let registry =
            Registry::load_from_dirs(roots.iter().map(PathBuf::as_path)).expect("registry");
        let extension = registry.get("same-id").expect("extension");
        assert_eq!(extension.descriptor.name, "Workspace Extension");
    }

    #[test]
    fn filter_for_runtime_skips_disabled_extensions() {
        let temp = tempdir().expect("tempdir");
        write_extension_with_platforms(temp.path(), "alpha-ext", &[]);
        write_extension_with_platforms(temp.path(), "beta-ext", &[]);

        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let filtered = registry.filter_for_runtime(
            &CoreConfig {
                disabled_extensions: BTreeSet::from(["beta-ext".to_string()]),
            },
            current_platform(),
        );

        assert!(filtered.get("alpha-ext").is_some());
        assert!(filtered.get("beta-ext").is_none());
    }

    #[test]
    fn filter_for_runtime_skips_platform_mismatches() {
        let temp = tempdir().expect("tempdir");
        write_extension_with_platforms(temp.path(), "current-ext", &[current_platform()]);
        write_extension_with_platforms(temp.path(), "other-ext", &[other_platform()]);

        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let filtered = registry.filter_for_runtime(&CoreConfig::default(), current_platform());

        assert!(filtered.get("current-ext").is_some());
        assert!(filtered.get("other-ext").is_none());
    }

    fn write_extension_with_platforms(root: &Path, id: &str, platforms: &[Platform]) {
        let ext_dir = root.join(id);
        fs::create_dir_all(&ext_dir).expect("create extension dir");
        let platforms_json = if platforms.is_empty() {
            String::new()
        } else {
            format!(
                "\"platforms\": [{}],",
                platforms
                    .iter()
                    .map(|platform| format!("\"{}\"", platform.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        fs::write(
            ext_dir.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "Test Extension",
                    "version": "1.0.0",
                    "trigger": "test",
                    "runtime": {{
                        "kind": "wasm-component",
                        "abi": "copper.component/1",
                        "artifact": "{id}.wasm"
                    }},
                    {platforms_json}
                    "actions": [
                        {{ "id": "run", "label": "Run", "script": "return;" }}
                    ]
                }}"#
            ),
        )
        .expect("write descriptor");
        fs::write(ext_dir.join(format!("{id}.wasm")), b"\0asm").expect("write component");
    }

    fn other_platform() -> Platform {
        match current_platform() {
            Platform::Windows => Platform::Linux,
            Platform::Macos => Platform::Windows,
            Platform::Linux => Platform::Windows,
        }
    }

    fn normalize_path(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

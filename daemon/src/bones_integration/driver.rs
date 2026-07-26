use super::{CopperControlHandle, CopperControlModule, CopperLifecycleState};
use crate::extension::Registry;
use bones_logging::{Level, LogSink, Logger};
use bones_runner::{BuiltEngine, Engine};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

type CatalogSnapshot = BTreeMap<String, PathBuf>;

struct CopperBonesLogSink;

impl LogSink for CopperBonesLogSink {
    fn log(&self, level: Level, category: &str, message: &str) {
        let message = format!("[bones/{category}] {message}");
        match level {
            Level::Debug => {}
            Level::Info | Level::Warn => crate::logging::info(message),
            Level::Error => crate::logging::error(message),
        }
    }
}

/// Serializable health snapshot for Copper's embedded Bones runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BonesRuntimeStatus {
    pub headless: bool,
    pub frames: u64,
    pub registry_reloads: u64,
    pub catalog_extensions: usize,
    pub catalog_rebuilds: u64,
    pub lifecycle_events: usize,
    pub lifecycle_decode_errors: usize,
    pub extensions: BTreeMap<String, CopperLifecycleState>,
    pub shutdown: bool,
}

/// Single-threaded, step-driven Bones engine owned by the Copper daemon.
pub struct BonesDaemonDriver {
    engine: BuiltEngine,
    control: CopperControlHandle,
    catalog: CatalogSnapshot,
    frames: u64,
    registry_reloads: u64,
    catalog_rebuilds: u64,
    shutdown: bool,
}

impl BonesDaemonDriver {
    pub fn new(registry: &Registry) -> Result<Self, String> {
        let (control_module, control) = CopperControlModule::new();
        let catalog = catalog_snapshot(registry);
        let mut engine = build_engine(registry, control_module)?;
        dispatch_pending(&mut engine);
        Ok(Self {
            engine,
            control,
            catalog,
            frames: 0,
            registry_reloads: 0,
            catalog_rebuilds: 0,
            shutdown: false,
        })
    }

    /// Replaces the Bones catalog only when component identity or location
    /// changed. In-place component updates remain owned by Bones' transactional
    /// supervisor reload.
    pub fn replace_catalog(&mut self, registry: &Registry) -> Result<bool, String> {
        if self.shutdown {
            return Err("cannot replace the catalog after Bones shutdown".to_string());
        }
        self.registry_reloads = self.registry_reloads.saturating_add(1);
        let catalog = catalog_snapshot(registry);
        if catalog == self.catalog {
            return Ok(false);
        }

        let module = CopperControlModule::from_handle(self.control.clone());
        let mut candidate = build_engine(registry, module)?;
        self.engine.shutdown();
        dispatch_pending(&mut candidate);
        self.engine = candidate;
        self.catalog = catalog;
        self.catalog_rebuilds = self.catalog_rebuilds.saturating_add(1);
        Ok(true)
    }

    /// Advances Bones once using the daemon loop's measured elapsed time.
    pub fn step(&mut self, elapsed: Duration) {
        if self.shutdown {
            return;
        }
        self.engine.supervisor.check();
        self.engine.runner.step(elapsed.as_secs_f32().min(1.0));
        self.engine.supervisor.check();
        self.frames = self.frames.saturating_add(1);
    }

    pub fn status(&self) -> BonesRuntimeStatus {
        BonesRuntimeStatus {
            headless: self.engine.is_headless(),
            frames: self.frames,
            registry_reloads: self.registry_reloads,
            catalog_extensions: self.catalog.len(),
            catalog_rebuilds: self.catalog_rebuilds,
            lifecycle_events: self.control.lifecycle_event_count(),
            lifecycle_decode_errors: self.control.lifecycle_decode_errors(),
            extensions: self.control.extensions(),
            shutdown: self.shutdown,
        }
    }

    pub fn shutdown(&mut self) {
        if self.shutdown {
            return;
        }
        self.engine.shutdown();
        self.shutdown = true;
    }
}

fn build_engine(
    registry: &Registry,
    control_module: CopperControlModule,
) -> Result<BuiltEngine, String> {
    let mut builder = Engine::new()
        .logger(Logger::new(Arc::new(CopperBonesLogSink)))
        .module(control_module)
        .extension_controller(super::COPPER_CONTROL_ENDPOINT)
        .read_only_persistence()
        .saves_dir(crate::extension::default_extensions_dir().join("copper-core/bones-saves"));
    for extension in registry.list() {
        if let Some(path) = extension.wasm_component_path() {
            builder = builder
                .catalog_extension(&extension.descriptor.id, path)
                .startup_extension(&extension.descriptor.id);
        }
    }
    let engine = builder
        .build()
        .map_err(|err| format!("failed to build headless Bones engine: {err}"))?;
    if !engine.is_headless() {
        return Err("Copper daemon constructed a non-headless Bones engine".to_string());
    }
    Ok(engine)
}

fn catalog_snapshot(registry: &Registry) -> CatalogSnapshot {
    let mut snapshot = BTreeMap::new();
    for extension in registry.list() {
        if let Some(path) = extension.wasm_component_path() {
            snapshot.insert(extension.descriptor.id.clone(), path.to_path_buf());
        }
    }
    snapshot
}

fn dispatch_pending(engine: &mut BuiltEngine) {
    engine.runner.begin_frame();
    engine.runner.bus().dispatch();
    engine.supervisor.check();
}

impl Drop for BonesDaemonDriver {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::BonesDaemonDriver;
    use crate::bones_integration::CopperLifecycleState;
    use crate::core_config::CoreConfig;
    use crate::descriptor::COMPONENT_ABI_V1;
    use crate::extension::{current_platform, Registry};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn driver_is_headless_step_driven_and_shutdown_is_idempotent() {
        let temp = tempdir().expect("tempdir");
        let registry = Registry::load_from_dir(temp.path()).expect("empty registry");
        let mut driver = BonesDaemonDriver::new(&registry).expect("headless engine");
        assert!(driver.status().headless);
        assert_eq!(driver.status().frames, 0);

        driver.step(Duration::from_millis(20));
        assert!(!driver.replace_catalog(&registry).expect("same catalog"));
        assert_eq!(driver.status().frames, 1);
        assert_eq!(driver.status().registry_reloads, 1);
        assert_eq!(driver.status().catalog_rebuilds, 0);

        driver.shutdown();
        driver.shutdown();
        driver.step(Duration::from_millis(20));
        assert!(driver.status().shutdown);
        assert_eq!(driver.status().frames, 1);
    }

    #[test]
    fn driver_maps_component_catalog_and_observes_lifecycle_faults() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "broken-component");
        let registry = Registry::load_from_dir(temp.path()).expect("registry");

        let mut driver = BonesDaemonDriver::new(&registry).expect("driver");
        let status = driver.status();
        assert_eq!(status.catalog_extensions, 1);
        assert_eq!(
            status.extensions.get("broken-component"),
            Some(&CopperLifecycleState::Faulted)
        );

        fs::write(
            temp.path().join("broken-component/broken-component.wasm"),
            b"\0asm-updated",
        )
        .expect("updated component");
        let updated_registry = Registry::load_from_dir(temp.path()).expect("updated registry");
        assert!(
            !driver
                .replace_catalog(&updated_registry)
                .expect("stable catalog"),
            "in-place component replacement must stay with Bones' supervisor"
        );
        assert_eq!(driver.status().catalog_rebuilds, 0);
    }

    #[test]
    fn driver_catalog_uses_coppers_filtered_runtime_registry() {
        let temp = tempdir().expect("tempdir");
        write_component_extension(temp.path(), "enabled-component");
        write_component_extension(temp.path(), "disabled-component");
        let registry = Registry::load_from_dir(temp.path())
            .expect("registry")
            .filter_for_runtime(
                &CoreConfig {
                    disabled_extensions: BTreeSet::from(["disabled-component".to_string()]),
                },
                current_platform(),
            );

        let driver = BonesDaemonDriver::new(&registry).expect("driver");
        let status = driver.status();
        assert_eq!(status.catalog_extensions, 1);
        assert!(status.extensions.contains_key("enabled-component"));
        assert!(!status.extensions.contains_key("disabled-component"));
    }

    fn write_component_extension(parent: &Path, id: &str) {
        let root = parent.join(id);
        fs::create_dir_all(&root).expect("extension root");
        fs::write(
            root.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "{id}",
                    "version": "1.0.0",
                    "trigger": "{id}",
                    "runtime": {{
                        "kind": "wasm-component",
                        "abi": "{COMPONENT_ABI_V1}",
                        "artifact": "{id}.wasm"
                    }},
                    "actions": [{{ "id": "run", "label": "Run", "script": "run" }}]
                }}"#
            ),
        )
        .expect("manifest");
        fs::write(root.join(format!("{id}.wasm")), b"\0asm").expect("component");
    }
}

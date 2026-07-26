use super::{CopperControlHandle, CopperControlModule};
use bones_logging::{Level, LogSink, Logger};
use bones_runner::{BuiltEngine, Engine};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BonesRuntimeStatus {
    pub headless: bool,
    pub frames: u64,
    pub registry_reloads: u64,
    pub lifecycle_events: usize,
    pub shutdown: bool,
}

/// Single-threaded, step-driven Bones engine owned by the Copper daemon.
///
/// Product extensions are intentionally not configured here. Catalog mapping
/// is added separately once Copper manifests can identify versioned runtime
/// artifacts.
pub struct BonesDaemonDriver {
    engine: BuiltEngine,
    control: CopperControlHandle,
    frames: u64,
    registry_reloads: u64,
    shutdown: bool,
}

impl BonesDaemonDriver {
    pub fn new() -> Result<Self, String> {
        let (control_module, control) = CopperControlModule::new();
        let engine = Engine::new()
            .logger(Logger::new(Arc::new(CopperBonesLogSink)))
            .module(control_module)
            .read_only_persistence()
            .build()
            .map_err(|err| format!("failed to build headless Bones engine: {err}"))?;
        if !engine.is_headless() {
            return Err("Copper daemon constructed a non-headless Bones engine".to_string());
        }
        Ok(Self {
            engine,
            control,
            frames: 0,
            registry_reloads: 0,
            shutdown: false,
        })
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

    /// Records the legacy registry reload boundary until catalog bridging is
    /// introduced. This keeps health and lifecycle behavior observable.
    pub fn note_registry_reload(&mut self) {
        if self.shutdown {
            return;
        }
        self.registry_reloads = self.registry_reloads.saturating_add(1);
        self.engine.supervisor.check();
    }

    pub fn status(&self) -> BonesRuntimeStatus {
        BonesRuntimeStatus {
            headless: self.engine.is_headless(),
            frames: self.frames,
            registry_reloads: self.registry_reloads,
            lifecycle_events: self.control.lifecycle_event_count(),
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

impl Drop for BonesDaemonDriver {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::BonesDaemonDriver;
    use std::time::Duration;

    #[test]
    fn driver_is_headless_step_driven_and_shutdown_is_idempotent() {
        let mut driver = BonesDaemonDriver::new().expect("headless engine");
        assert!(driver.status().headless);
        assert_eq!(driver.status().frames, 0);

        driver.step(Duration::from_millis(20));
        driver.note_registry_reload();
        assert_eq!(driver.status().frames, 1);
        assert_eq!(driver.status().registry_reloads, 1);

        driver.shutdown();
        driver.shutdown();
        driver.step(Duration::from_millis(20));
        assert!(driver.status().shutdown);
        assert_eq!(driver.status().frames, 1);
    }
}

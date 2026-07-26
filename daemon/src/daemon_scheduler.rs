use crate::core_config::CoreConfig;
use crate::extension::Registry;
use crate::host_extensions::HostExtensionRegistry;
use crate::logging;
use crate::state_store::ExtensionStateStore;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const BACKGROUND_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const MAX_BACKGROUND_INTERVAL_SECONDS: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledAction {
    pub extension_id: String,
    pub action_id: String,
}

#[derive(Debug)]
pub struct DaemonScheduler {
    reload_interval: Duration,
    last_reload: Instant,
    last_background_scan: Instant,
    last_legacy_background: BTreeMap<String, Instant>,
    last_bones_background: BTreeMap<String, Instant>,
}

impl DaemonScheduler {
    pub fn new(reload_interval: Duration) -> Self {
        Self {
            reload_interval,
            last_reload: Instant::now(),
            last_background_scan: Instant::now()
                .checked_sub(BACKGROUND_SCAN_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_legacy_background: BTreeMap::new(),
            last_bones_background: BTreeMap::new(),
        }
    }

    pub fn reload_due(&self) -> bool {
        self.last_reload.elapsed() >= self.reload_interval
    }

    pub fn mark_reload(&mut self) {
        self.last_reload = Instant::now();
    }

    pub fn tick_background(
        &mut self,
        registry: &Registry,
        host_extensions: &HostExtensionRegistry,
        state_store: &ExtensionStateStore,
        core_config: &CoreConfig,
    ) {
        for capability in host_extensions.background_capabilities() {
            if registry
                .get(capability.extension_id)
                .is_some_and(|extension| extension.wasm_component_path().is_some())
            {
                continue;
            }
            if !core_config.is_extension_enabled(capability.extension_id) {
                continue;
            }
            let last_run = self
                .last_legacy_background
                .get(capability.capability_id)
                .copied();
            match host_extensions.tick_background(capability.extension_id, state_store, last_run) {
                Ok(true) => {
                    self.last_legacy_background
                        .insert(capability.capability_id.to_string(), Instant::now());
                }
                Ok(false) => {}
                Err(err) => logging::error(format!(
                    "background capability error [{} -> {}]: {}",
                    capability.capability_id, capability.extension_id, err
                )),
            }
        }
    }

    pub fn due_bones_background(
        &mut self,
        registry: &Registry,
        state_store: &ExtensionStateStore,
    ) -> Result<Vec<ScheduledAction>, std::io::Error> {
        self.due_bones_background_at(registry, state_store, Instant::now())
    }

    fn due_bones_background_at(
        &mut self,
        registry: &Registry,
        state_store: &ExtensionStateStore,
        now: Instant,
    ) -> Result<Vec<ScheduledAction>, std::io::Error> {
        if now.saturating_duration_since(self.last_background_scan) < BACKGROUND_SCAN_INTERVAL {
            return Ok(Vec::new());
        }
        self.last_background_scan = now;

        let mut due = Vec::new();
        for extension in registry.list() {
            let Some(background) = extension
                .descriptor
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.background.as_ref())
            else {
                continue;
            };
            if extension.wasm_component_path().is_none() {
                continue;
            }
            let config = state_store.load_config(&extension.descriptor.id)?;
            let enabled = background
                .enabled_config
                .as_deref()
                .and_then(|key| config.get(key))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(background.enabled_by_default);
            if !enabled {
                continue;
            }
            let interval_seconds = background
                .interval_seconds_config
                .as_deref()
                .and_then(|key| config.get(key))
                .and_then(serde_json::Value::as_u64)
                .filter(|seconds| (1..=MAX_BACKGROUND_INTERVAL_SECONDS).contains(seconds))
                .unwrap_or(background.default_interval_seconds);
            let schedule_key = schedule_key(&extension.descriptor.id, &background.action);
            let is_due = self
                .last_bones_background
                .get(&schedule_key)
                .is_none_or(|last_run| {
                    now.saturating_duration_since(*last_run)
                        >= Duration::from_secs(interval_seconds)
                });
            if is_due {
                due.push(ScheduledAction {
                    extension_id: extension.descriptor.id.clone(),
                    action_id: background.action.clone(),
                });
            }
        }
        Ok(due)
    }

    pub fn mark_bones_background_run(&mut self, action: &ScheduledAction) {
        self.mark_bones_background_run_at(action, Instant::now());
    }

    fn mark_bones_background_run_at(&mut self, action: &ScheduledAction, now: Instant) {
        self.last_bones_background
            .insert(schedule_key(&action.extension_id, &action.action_id), now);
    }
}

fn schedule_key(extension_id: &str, action_id: &str) -> String {
    format!("{extension_id}:{action_id}")
}

#[cfg(test)]
mod tests {
    use super::DaemonScheduler;
    use crate::core_config::CoreConfig;
    use crate::descriptor::COMPONENT_ABI_V1;
    use crate::extension::Registry;
    use crate::host_extensions::HostExtensionRegistry;
    use crate::state_store::{read_json_object, write_json_object, ExtensionStateStore};
    use std::collections::BTreeSet;
    use std::fs;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    #[test]
    fn reload_due_tracks_interval() {
        let scheduler = DaemonScheduler::new(std::time::Duration::from_millis(0));
        assert!(scheduler.reload_due());
    }

    #[test]
    fn background_tick_uses_capability_identity() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        write_json_object(
            &store.config_path("desktop-torrent-organizer"),
            &serde_json::json!({
                "desktopFolder": temp.path().join("Desktop").display().to_string(),
                "torrentsFolder": temp.path().join("Desktop/Torrents").display().to_string(),
                "autoRun": false,
                "pollIntervalSeconds": 1
            }),
        )
        .expect("write config");

        let registry = HostExtensionRegistry::new();
        let extensions = Registry::load_from_dir(temp.path()).expect("extensions");
        let mut scheduler = DaemonScheduler::new(std::time::Duration::from_secs(10));
        scheduler.tick_background(&extensions, &registry, &store, &CoreConfig::default());

        let status =
            read_json_object(&store.status_path("desktop-torrent-organizer")).expect("read status");
        assert_eq!(status, serde_json::json!({}));
    }

    #[test]
    fn background_tick_skips_disabled_extensions() {
        let temp = tempdir().expect("tempdir");
        let desktop = temp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).expect("desktop");
        std::fs::write(desktop.join("movie.torrent"), "data").expect("write torrent");

        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        write_json_object(
            &store.config_path("desktop-torrent-organizer"),
            &serde_json::json!({
                "desktopFolder": desktop.display().to_string(),
                "torrentsFolder": temp.path().join("Desktop/Torrents").display().to_string(),
                "autoRun": true,
                "pollIntervalSeconds": 1
            }),
        )
        .expect("write config");

        let core_config = CoreConfig {
            disabled_extensions: BTreeSet::from(["desktop-torrent-organizer".to_string()]),
        };

        let registry = HostExtensionRegistry::new();
        let extensions = Registry::load_from_dir(temp.path()).expect("extensions");
        let mut scheduler = DaemonScheduler::new(std::time::Duration::from_secs(10));
        scheduler.tick_background(&extensions, &registry, &store, &core_config);

        // torrent must still be on the desktop — the disabled extension should not have moved it
        assert!(desktop.join("movie.torrent").exists());
    }

    #[test]
    fn manifest_schedule_emits_bones_actions_at_configured_intervals() {
        let temp = tempdir().expect("tempdir");
        write_scheduled_component(temp.path());
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        store
            .write_config(
                "watcher",
                &serde_json::json!({"autoRun": true, "pollIntervalSeconds": 5}),
            )
            .expect("config");
        let mut scheduler = DaemonScheduler::new(Duration::from_secs(10));
        let now = Instant::now();

        let first = scheduler
            .due_bones_background_at(&registry, &store, now)
            .expect("first schedule");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].extension_id, "watcher");
        assert_eq!(first[0].action_id, "poll");
        scheduler.mark_bones_background_run_at(&first[0], now);

        assert!(scheduler
            .due_bones_background_at(&registry, &store, now + Duration::from_secs(2))
            .expect("not due")
            .is_empty());
        assert_eq!(
            scheduler
                .due_bones_background_at(&registry, &store, now + Duration::from_secs(5))
                .expect("due again")
                .len(),
            1
        );
    }

    #[test]
    fn manifest_schedule_honors_its_enabled_config() {
        let temp = tempdir().expect("tempdir");
        write_scheduled_component(temp.path());
        let registry = Registry::load_from_dir(temp.path()).expect("registry");
        let store = ExtensionStateStore::new(temp.path().join("state"));
        store
            .write_config("watcher", &serde_json::json!({"autoRun": false}))
            .expect("config");
        let mut scheduler = DaemonScheduler::new(Duration::from_secs(10));

        assert!(scheduler
            .due_bones_background(&registry, &store)
            .expect("schedule")
            .is_empty());
    }

    fn write_scheduled_component(root: &std::path::Path) {
        let extension = root.join("watcher");
        fs::create_dir_all(&extension).expect("extension");
        fs::write(
            extension.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "watcher",
                    "name": "Watcher",
                    "version": "1.0.0",
                    "trigger": "watcher",
                    "runtime": {{
                        "kind": "wasm-component",
                        "abi": "{COMPONENT_ABI_V1}",
                        "artifact": "watcher.wasm",
                        "background": {{
                            "action": "poll",
                            "enabledConfig": "autoRun",
                            "enabledByDefault": false,
                            "intervalSecondsConfig": "pollIntervalSeconds",
                            "defaultIntervalSeconds": 10
                        }}
                    }},
                    "actions": [{{ "id": "poll", "label": "Poll", "script": "poll" }}]
                }}"#
            ),
        )
        .expect("manifest");
        fs::write(extension.join("watcher.wasm"), b"\0asm").expect("component");
    }
}

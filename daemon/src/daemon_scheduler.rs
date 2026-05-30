use crate::core_config::CoreConfig;
use crate::host_extensions::HostExtensionRegistry;
use crate::logging;
use crate::state_store::ExtensionStateStore;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct DaemonScheduler {
    reload_interval: Duration,
    last_reload: Instant,
    last_background_poll: BTreeMap<String, Instant>,
}

impl DaemonScheduler {
    pub fn new(reload_interval: Duration) -> Self {
        Self {
            reload_interval,
            last_reload: Instant::now(),
            last_background_poll: BTreeMap::new(),
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
        host_extensions: &HostExtensionRegistry,
        state_store: &ExtensionStateStore,
        core_config: &CoreConfig,
    ) {
        for capability in host_extensions.background_capabilities() {
            if !core_config.is_extension_enabled(capability.extension_id) {
                continue;
            }
            let last_run = self
                .last_background_poll
                .get(capability.capability_id)
                .copied();
            match host_extensions.tick_background(capability.extension_id, state_store, last_run) {
                Ok(true) => {
                    self.last_background_poll
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
}

#[cfg(test)]
mod tests {
    use super::DaemonScheduler;
    use crate::core_config::CoreConfig;
    use crate::host_extensions::HostExtensionRegistry;
    use crate::state_store::{read_json_object, write_json_object, ExtensionStateStore};
    use std::collections::BTreeSet;
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
        let mut scheduler = DaemonScheduler::new(std::time::Duration::from_secs(10));
        scheduler.tick_background(&registry, &store, &CoreConfig::default());

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
        let mut scheduler = DaemonScheduler::new(std::time::Duration::from_secs(10));
        scheduler.tick_background(&registry, &store, &core_config);

        // torrent must still be on the desktop — the disabled extension should not have moved it
        assert!(desktop.join("movie.torrent").exists());
    }
}

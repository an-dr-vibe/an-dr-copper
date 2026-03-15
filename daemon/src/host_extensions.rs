use crate::api::windows_display;
use crate::state_store::{read_json_object, unix_now_secs, write_json_object, ExtensionStateStore};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DESKTOP_TORRENT_ORGANIZER_ID: &str = "desktop-torrent-organizer";
pub const SESSION_COUNTER_ID: &str = "session-counter";
pub const WINDOWS_DISPLAY_MANAGER_ID: &str = "windows-display-manager";
const SESSION_COUNTER_INCREMENT_ACTION: &str = "increment";

#[derive(Debug, Default, Clone)]
pub struct HostExtensionRegistry;

#[derive(Debug, Clone)]
pub struct AppliedActionResult {
    pub action_id: String,
    pub result: Value,
}

pub trait HostExtensionHandler {
    fn supports_cli_trigger(&self, _action_id: &str) -> bool {
        false
    }

    fn trigger_payload(
        &self,
        _store: &ExtensionStateStore,
        _action_id: &str,
    ) -> Result<Value, std::io::Error> {
        Ok(serde_json::json!({}))
    }

    fn apply_settings(
        &self,
        _store: &ExtensionStateStore,
        _apply_actions: &[String],
    ) -> Result<Vec<AppliedActionResult>, std::io::Error> {
        Ok(Vec::new())
    }

    fn dynamic_options(&self, _config: &Value) -> Result<Value, std::io::Error> {
        Ok(serde_json::json!({}))
    }

    fn tick_background(
        &self,
        _store: &ExtensionStateStore,
        _last_run: Option<Instant>,
    ) -> Result<bool, std::io::Error> {
        Ok(false)
    }
}

impl HostExtensionRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn trigger_payload(
        &self,
        extension_id: &str,
        store: &ExtensionStateStore,
        action_id: &str,
    ) -> Result<Value, std::io::Error> {
        match self.handler(extension_id) {
            Some(handler) => handler.trigger_payload(store, action_id),
            None => Ok(serde_json::json!({})),
        }
    }

    pub fn apply_settings(
        &self,
        extension_id: &str,
        store: &ExtensionStateStore,
        apply_actions: &[String],
    ) -> Result<Vec<AppliedActionResult>, std::io::Error> {
        match self.handler(extension_id) {
            Some(handler) => handler.apply_settings(store, apply_actions),
            None => Ok(Vec::new()),
        }
    }

    pub fn dynamic_options(
        &self,
        extension_id: &str,
        config: &Value,
    ) -> Result<Value, std::io::Error> {
        match self.handler(extension_id) {
            Some(handler) => handler.dynamic_options(config),
            None => Ok(serde_json::json!({})),
        }
    }

    pub fn tick_background(
        &self,
        extension_id: &str,
        store: &ExtensionStateStore,
        last_run: Option<Instant>,
    ) -> Result<bool, std::io::Error> {
        match self.handler(extension_id) {
            Some(handler) => handler.tick_background(store, last_run),
            None => Ok(false),
        }
    }

    pub fn supports_cli_trigger(&self, extension_id: &str, action_id: &str) -> bool {
        match self.handler(extension_id) {
            Some(handler) => handler.supports_cli_trigger(action_id),
            None => false,
        }
    }

    pub fn background_extension_ids(&self) -> &'static [&'static str] {
        &[DESKTOP_TORRENT_ORGANIZER_ID]
    }

    fn handler(&self, extension_id: &str) -> Option<&'static dyn HostExtensionHandler> {
        match extension_id {
            SESSION_COUNTER_ID => Some(&SESSION_COUNTER_HANDLER),
            WINDOWS_DISPLAY_MANAGER_ID => Some(&WINDOWS_DISPLAY_HANDLER),
            DESKTOP_TORRENT_ORGANIZER_ID => Some(&DESKTOP_TORRENT_HANDLER),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SessionCounterHandler;

impl HostExtensionHandler for SessionCounterHandler {
    fn supports_cli_trigger(&self, action_id: &str) -> bool {
        action_id == SESSION_COUNTER_INCREMENT_ACTION
    }

    fn trigger_payload(
        &self,
        store: &ExtensionStateStore,
        action_id: &str,
    ) -> Result<Value, std::io::Error> {
        if action_id != SESSION_COUNTER_INCREMENT_ACTION {
            return Ok(serde_json::json!({}));
        }

        store.ensure_root()?;
        let path = store.status_path(SESSION_COUNTER_ID);
        let mut status = read_json_object(&path)?;
        let current = status.get("count").and_then(Value::as_u64).unwrap_or(0);
        let next = current.saturating_add(1);
        status["count"] = serde_json::json!(next);
        status["lastIncrementUnix"] = serde_json::json!(unix_now_secs());
        status["lastActionId"] = serde_json::json!(SESSION_COUNTER_INCREMENT_ACTION);
        write_json_object(&path, &status)?;
        Ok(serde_json::json!({ "sessionCount": next }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct WindowsDisplayHandler;

impl HostExtensionHandler for WindowsDisplayHandler {
    fn supports_cli_trigger(&self, action_id: &str) -> bool {
        matches!(
            action_id,
            "status"
                | "toggle-taskbar-autohide"
                | "set-taskbar-autohide"
                | "set-resolution"
                | "set-scale"
        )
    }

    fn trigger_payload(
        &self,
        store: &ExtensionStateStore,
        action_id: &str,
    ) -> Result<Value, std::io::Error> {
        Ok(serde_json::json!({
            "hostExecution": execute_windows_display_action(store, action_id)?
        }))
    }

    fn apply_settings(
        &self,
        store: &ExtensionStateStore,
        apply_actions: &[String],
    ) -> Result<Vec<AppliedActionResult>, std::io::Error> {
        apply_actions
            .iter()
            .map(|action_id| {
                Ok(AppliedActionResult {
                    action_id: action_id.clone(),
                    result: execute_windows_display_action(store, action_id)?,
                })
            })
            .collect()
    }

    fn dynamic_options(&self, config: &Value) -> Result<Value, std::io::Error> {
        let status =
            windows_display::execute_action("status", config).map_err(std::io::Error::other)?;
        let presets = status
            .get("resolution")
            .and_then(|value| value.get("availableModes"))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| {
                        Some(format!(
                            "{}x{}@{}",
                            value.get("width")?.as_i64()?,
                            value.get("height")?.as_i64()?,
                            value.get("refreshRate")?.as_i64()?
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(serde_json::json!({
            "trayResolutionPresets": presets
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DesktopTorrentHandler;

impl HostExtensionHandler for DesktopTorrentHandler {
    fn tick_background(
        &self,
        store: &ExtensionStateStore,
        last_run: Option<Instant>,
    ) -> Result<bool, std::io::Error> {
        let config = load_torrent_monitor_config(store)?;
        if !config.enabled {
            return Ok(false);
        }
        if let Some(last_run) = last_run {
            if last_run.elapsed() < config.poll_interval {
                return Ok(false);
            }
        }

        let report = run_torrent_move(&config)?;
        write_desktop_torrent_status(store, &config, report)?;
        Ok(true)
    }
}

static SESSION_COUNTER_HANDLER: SessionCounterHandler = SessionCounterHandler;
static WINDOWS_DISPLAY_HANDLER: WindowsDisplayHandler = WindowsDisplayHandler;
static DESKTOP_TORRENT_HANDLER: DesktopTorrentHandler = DesktopTorrentHandler;

pub fn execute_windows_display_action(
    store: &ExtensionStateStore,
    action_id: &str,
) -> Result<Value, std::io::Error> {
    store.ensure_root()?;
    let config = store.load_config(WINDOWS_DISPLAY_MANAGER_ID)?;
    let path = store.status_path(WINDOWS_DISPLAY_MANAGER_ID);
    let mut state = read_json_object(&path)?;
    let execution = match windows_display::execute_action(action_id, &config) {
        Ok(value) => value,
        Err(err) => {
            update_windows_display_status(&mut state, action_id, None, Some(&err));
            write_json_object(&path, &state)?;
            return Err(std::io::Error::other(err));
        }
    };

    update_windows_display_status(&mut state, action_id, Some(&execution), None);
    write_json_object(&path, &state)?;
    Ok(execution)
}

fn update_windows_display_status(
    state: &mut Value,
    action_id: &str,
    execution: Option<&Value>,
    error: Option<&str>,
) {
    if !state.is_object() {
        *state = serde_json::json!({});
    }

    if let Some(map) = state.as_object_mut() {
        map.insert("lastActionId".to_string(), serde_json::json!(action_id));
        map.insert(
            "lastActionUnix".to_string(),
            serde_json::json!(unix_now_secs()),
        );
        map.insert(
            "lastActionOk".to_string(),
            serde_json::json!(error.is_none()),
        );
        if let Some(execution) = execution {
            map.insert("lastResult".to_string(), execution.clone());
            map.remove("lastError");

            if let Some(taskbar_auto_hide) = execution.get("taskbarAutoHide") {
                map.insert("taskbarAutoHide".to_string(), taskbar_auto_hide.clone());
            }
            if let Some(scale_current) =
                execution.get("scale").and_then(|v| v.get("currentPercent"))
            {
                map.insert("scalePercent".to_string(), scale_current.clone());
            }
            if let Some(resolution) = execution.get("resolution") {
                if let Some(width) = resolution.get("width") {
                    map.insert("resolutionWidth".to_string(), width.clone());
                }
                if let Some(height) = resolution.get("height") {
                    map.insert("resolutionHeight".to_string(), height.clone());
                }
                if let Some(refresh_rate) = resolution.get("refreshRate") {
                    map.insert("refreshRate".to_string(), refresh_rate.clone());
                }
            }
        } else if let Some(error) = error {
            map.insert("lastError".to_string(), serde_json::json!(error));
        }
    }
}

#[derive(Debug, Clone)]
struct TorrentMonitorConfig {
    enabled: bool,
    poll_interval: Duration,
    desktop_folder: PathBuf,
    torrents_folder: PathBuf,
}

#[derive(Debug, Clone, Copy, Default)]
struct TorrentMoveReport {
    found: u64,
    moved: u64,
    failed: u64,
}

fn load_torrent_monitor_config(
    store: &ExtensionStateStore,
) -> Result<TorrentMonitorConfig, std::io::Error> {
    let config = store.load_config(DESKTOP_TORRENT_ORGANIZER_ID)?;

    let enabled = config
        .get("autoRun")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let poll_secs = config
        .get("pollIntervalSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 3600);
    let desktop_folder = expand_home(
        config
            .get("desktopFolder")
            .and_then(Value::as_str)
            .unwrap_or("~/Desktop"),
    );
    let torrents_folder = expand_home(
        config
            .get("torrentsFolder")
            .and_then(Value::as_str)
            .unwrap_or("~/Desktop/Torrents"),
    );

    Ok(TorrentMonitorConfig {
        enabled,
        poll_interval: Duration::from_secs(poll_secs),
        desktop_folder,
        torrents_folder,
    })
}

fn run_torrent_move(config: &TorrentMonitorConfig) -> Result<TorrentMoveReport, std::io::Error> {
    std::fs::create_dir_all(&config.torrents_folder)?;

    let mut report = TorrentMoveReport::default();
    let read_dir = match std::fs::read_dir(&config.desktop_folder) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(err) => return Err(err),
    };

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if !file_name.to_ascii_lowercase().ends_with(".torrent") {
            continue;
        }

        report.found = report.found.saturating_add(1);
        let destination = next_available_destination(&config.torrents_folder, entry.file_name());
        match std::fs::rename(&path, &destination) {
            Ok(()) => {
                report.moved = report.moved.saturating_add(1);
            }
            Err(_) => {
                match std::fs::copy(&path, &destination).and_then(|_| std::fs::remove_file(&path)) {
                    Ok(()) => {
                        report.moved = report.moved.saturating_add(1);
                    }
                    Err(_) => {
                        report.failed = report.failed.saturating_add(1);
                    }
                }
            }
        }
    }

    Ok(report)
}

fn write_desktop_torrent_status(
    store: &ExtensionStateStore,
    config: &TorrentMonitorConfig,
    report: TorrentMoveReport,
) -> Result<(), std::io::Error> {
    store.ensure_root()?;
    let path = store.status_path(DESKTOP_TORRENT_ORGANIZER_ID);
    let mut status = read_json_object(&path)?;
    status["autoRun"] = serde_json::json!(config.enabled);
    status["pollIntervalSeconds"] = serde_json::json!(config.poll_interval.as_secs());
    status["desktopFolder"] = serde_json::json!(config.desktop_folder.display().to_string());
    status["torrentsFolder"] = serde_json::json!(config.torrents_folder.display().to_string());
    status["lastScanUnix"] = serde_json::json!(unix_now_secs());
    status["lastScanFound"] = serde_json::json!(report.found);
    status["lastScanMoved"] = serde_json::json!(report.moved);
    status["lastScanFailed"] = serde_json::json!(report.failed);
    if report.moved > 0 {
        status["lastMoveUnix"] = serde_json::json!(unix_now_secs());
    }
    write_json_object(&path, &status)
}

fn next_available_destination(target_dir: &Path, file_name: std::ffi::OsString) -> PathBuf {
    let original = target_dir.join(&file_name);
    if !original.exists() {
        return original;
    }

    let file_name_lossy = file_name.to_string_lossy();
    let (base, ext) = split_name_and_extension(&file_name_lossy);
    for idx in 1..=9999u32 {
        let candidate_name = if ext.is_empty() {
            format!("{base}-{idx}")
        } else {
            format!("{base}-{idx}.{ext}")
        };
        let candidate = target_dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    target_dir.join(format!(
        "{}-{}.{}",
        base,
        unix_now_secs(),
        if ext.is_empty() { "torrent" } else { ext }
    ))
}

fn split_name_and_extension(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base, ext),
        _ => (name, ""),
    }
}

fn expand_home(raw: &str) -> PathBuf {
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::{
        execute_windows_display_action, HostExtensionRegistry, DESKTOP_TORRENT_ORGANIZER_ID,
        SESSION_COUNTER_ID, WINDOWS_DISPLAY_MANAGER_ID,
    };
    use crate::state_store::{read_json_object, write_json_object, ExtensionStateStore};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn session_counter_handler_updates_status() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let registry = HostExtensionRegistry::new();

        let payload = registry
            .trigger_payload(SESSION_COUNTER_ID, &store, "increment")
            .expect("payload");
        assert_eq!(
            payload.get("sessionCount").and_then(|value| value.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn windows_display_status_persists_snapshot() {
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        write_json_object(
            &store.config_path(WINDOWS_DISPLAY_MANAGER_ID),
            &serde_json::json!({
                "resolutionWidth": 1920,
                "resolutionHeight": 1080,
                "refreshRate": 60,
                "scalePercent": 100
            }),
        )
        .expect("write config");

        let result = execute_windows_display_action(&store, "status");
        if cfg!(target_os = "windows") {
            let value = result.expect("status");
            assert!(value.get("resolution").is_some());
        } else {
            let err = result.expect_err("non windows");
            assert_eq!(err.kind(), std::io::ErrorKind::Other);
        }
    }

    #[test]
    fn desktop_torrent_tick_moves_torrents() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let store = ExtensionStateStore::new(data_root.clone());
        let desktop = temp.path().join("Desktop");
        let torrents = desktop.join("Torrents");
        fs::create_dir_all(&desktop).expect("desktop");
        fs::write(desktop.join("movie.torrent"), "data").expect("write torrent");
        write_json_object(
            &store.config_path(DESKTOP_TORRENT_ORGANIZER_ID),
            &serde_json::json!({
                "desktopFolder": desktop.display().to_string(),
                "torrentsFolder": torrents.display().to_string(),
                "autoRun": true,
                "pollIntervalSeconds": 1
            }),
        )
        .expect("write config");

        let registry = HostExtensionRegistry::new();
        let ran = registry
            .tick_background(DESKTOP_TORRENT_ORGANIZER_ID, &store, None)
            .expect("tick");
        assert!(ran);
        assert!(torrents.join("movie.torrent").exists());

        let status =
            read_json_object(&store.status_path(DESKTOP_TORRENT_ORGANIZER_ID)).expect("status");
        assert_eq!(
            status.get("lastScanMoved").and_then(|value| value.as_u64()),
            Some(1)
        );
    }
}

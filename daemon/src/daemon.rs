use crate::autostart;
use crate::config_ui::{start_daemon_ui_server, DEFAULT_DAEMON_UI_BIND};
use crate::control_plane::ControlPlaneAuth;
use crate::core_config::{load_core_config, CoreConfig};
use crate::daemon_scheduler::DaemonScheduler;
use crate::extension::{
    core_extensions_dir, default_extensions_dir, load_runtime_registry, Registry,
};
use crate::host_extensions::HostExtensionRegistry;
use crate::logging;
use crate::state_store::ExtensionStateStore;
use crate::tray::TrayController;
use crate::tray_extension::AdditionalTrayController;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use thiserror::Error;
use tiny_http::Server;

#[path = "daemon_transport.rs"]
mod transport;

use transport::handle_http_request;
pub use transport::send_request;
#[cfg(test)]
use transport::{parse_http_response, request_url};

#[cfg(test)]
use crate::host_extensions::{
    DESKTOP_TORRENT_ORGANIZER_ID, SESSION_COUNTER_ID, WINDOWS_DISPLAY_MANAGER_ID,
};
#[cfg(test)]
use std::ffi::OsString;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:4765";
pub const DEFAULT_RELOAD_INTERVAL_MS: u64 = 3_000;
#[cfg(test)]
const SESSION_COUNTER_INCREMENT_ACTION: &str = "increment";
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub extensions_dir: PathBuf,
    pub bind_addr: String,
    pub reload_interval: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            extensions_dir: default_extensions_dir(),
            bind_addr: DEFAULT_BIND_ADDR.to_string(),
            reload_interval: Duration::from_millis(DEFAULT_RELOAD_INTERVAL_MS),
        }
    }
}

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Extension(#[from] crate::extension::ExtensionError),
    #[error("signal handler error: {0}")]
    SignalHandler(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("tray error: {0}")]
    Tray(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum IpcRequest {
    Health,
    List,
    Trigger { id: String, action: Option<String> },
    Reload,
    Verify,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl IpcResponse {
    pub fn ok(message: impl Into<String>, data: Option<serde_json::Value>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            data,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Debug)]
struct DaemonState {
    user_extensions_dir: PathBuf,
    core_extensions_dir: Option<PathBuf>,
    registry: Registry,
    core_config: CoreConfig,
    auth_token: Option<String>,
    state_store: ExtensionStateStore,
    host_extensions: HostExtensionRegistry,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct TorrentMonitorConfig {
    enabled: bool,
    poll_interval: Duration,
    desktop_folder: PathBuf,
    torrents_folder: PathBuf,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
struct TorrentMoveReport {
    found: u64,
    moved: u64,
    failed: u64,
}

impl DaemonState {
    fn load(user_extensions_dir: &Path) -> Result<Self, DaemonError> {
        let registry = load_runtime_registry(user_extensions_dir)?;
        let core_config = load_core_config().unwrap_or_default();
        Ok(Self {
            user_extensions_dir: user_extensions_dir.to_path_buf(),
            core_extensions_dir: core_extensions_dir(),
            registry,
            core_config,
            auth_token: None,
            state_store: ExtensionStateStore::for_current_user()?,
            host_extensions: HostExtensionRegistry::new(),
        })
    }

    fn reload(&mut self) -> Result<usize, DaemonError> {
        self.registry = load_runtime_registry(&self.user_extensions_dir)?;
        self.core_extensions_dir = core_extensions_dir();
        self.core_config = load_core_config().unwrap_or_default();
        Ok(self.registry.list().count())
    }
}

pub fn run_daemon(config: DaemonConfig) -> Result<(), DaemonError> {
    let server = Server::http(&config.bind_addr).map_err(|err| {
        DaemonError::Protocol(format!("failed to bind daemon control plane: {err}"))
    })?;
    let running = Arc::new(AtomicBool::new(true));
    let signal_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_flag.store(false, Ordering::Relaxed);
    })
    .map_err(|e| DaemonError::SignalHandler(e.to_string()))?;

    let mut state = DaemonState::load(&config.extensions_dir)?;
    match state.state_store.inspect_core_config() {
        Ok(core_config) => {
            for warning in core_config.warnings {
                logging::error(format!(
                    "warning: state diagnostic [{}] {}",
                    warning.code, warning.message
                ));
            }
            if let Err(err) = autostart::sync_from_core_config(&core_config.value) {
                logging::error(format!(
                    "warning: failed to synchronize autostart setting: {err}"
                ));
            }
        }
        Err(err) => {
            logging::error(format!(
                "warning: failed to read core config for autostart sync: {err}"
            ));
        }
    }
    let auth = ControlPlaneAuth::ensure_persisted()?;
    state.auth_token = Some(auth.token().to_string());
    let daemon_ui_bind = std::env::var("COPPERD_DAEMON_UI_BIND")
        .unwrap_or_else(|_| DEFAULT_DAEMON_UI_BIND.to_string());
    let daemon_ui = start_daemon_ui_server(
        config.extensions_dir.clone(),
        daemon_ui_bind,
        Arc::clone(&running),
        auth.clone(),
    )
    .map_err(|err| DaemonError::Protocol(format!("failed to start daemon UI server: {err}")))?;
    let disable_tray = std::env::var("COPPERD_DISABLE_TRAY")
        .map(|value| value == "1")
        .unwrap_or(false);
    let _tray = if disable_tray {
        None
    } else {
        Some(
            TrayController::initialize(
                Arc::clone(&running),
                config.extensions_dir.clone(),
                daemon_ui.url.clone(),
            )
            .map_err(|err| DaemonError::Tray(err.to_string()))?,
        )
    };
    let additional_trays = if disable_tray {
        None
    } else {
        Some(
            AdditionalTrayController::initialize(
                Arc::clone(&running),
                daemon_ui.url.clone(),
                &state.registry,
            )
            .map_err(|err| DaemonError::Tray(err.to_string()))?,
        )
    };
    let additional_tray_count = additional_trays
        .as_ref()
        .map(|controller| controller.specs().len())
        .unwrap_or(0);

    logging::info(format!(
        "Daemon started on {} (user extensions: {}, core extensions: {}, config UI: {}, additional tray icons: {})",
        config.bind_addr,
        config.extensions_dir.display(),
        state
            .core_extensions_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<not found>".to_string()),
        daemon_ui.url,
        additional_tray_count
    ));

    let mut scheduler = DaemonScheduler::new(config.reload_interval);
    while running.load(Ordering::Relaxed) {
        match server.recv_timeout(Duration::from_millis(50)) {
            Ok(Some(request)) => handle_http_request(request, &mut state, &running)?,
            Ok(None) => {}
            Err(err) => {
                return Err(DaemonError::Protocol(format!(
                    "daemon control-plane receive failed: {err}"
                )))
            }
        }

        if scheduler.reload_due() {
            let _ = state.reload()?;
            scheduler.mark_reload();
        }
        scheduler.tick_background(&state.host_extensions, &state.state_store, &state.core_config);
    }

    logging::info("Daemon stopped");
    Ok(())
}

#[cfg(test)]
pub fn maybe_increment_session_counter(
    extension_id: &str,
    action_id: &str,
) -> Result<Option<u64>, std::io::Error> {
    let data_root = copper_data_root()?;
    maybe_increment_session_counter_in(&data_root, extension_id, action_id)
}

#[cfg(test)]
fn maybe_increment_session_counter_in(
    data_root: &Path,
    extension_id: &str,
    action_id: &str,
) -> Result<Option<u64>, std::io::Error> {
    if extension_id != SESSION_COUNTER_ID || action_id != SESSION_COUNTER_INCREMENT_ACTION {
        return Ok(None);
    }

    fs::create_dir_all(data_root)?;
    let path = extension_status_path_in(data_root, SESSION_COUNTER_ID);

    let mut status = read_json_object(&path)?;
    let current = status.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    let next = current.saturating_add(1);
    status["count"] = serde_json::json!(next);
    status["lastIncrementUnix"] = serde_json::json!(unix_now_secs());
    status["lastActionId"] = serde_json::json!(SESSION_COUNTER_INCREMENT_ACTION);
    write_json_object(&path, &status)?;
    Ok(Some(next))
}

#[cfg(test)]
fn execute_windows_display_action_with_runner_in<F>(
    data_root: &Path,
    action_id: &str,
    runner: F,
) -> Result<serde_json::Value, std::io::Error>
where
    F: Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String>,
{
    fs::create_dir_all(data_root)?;
    let config = load_extension_config_object(data_root, WINDOWS_DISPLAY_MANAGER_ID)?;
    let path = extension_status_path_in(data_root, WINDOWS_DISPLAY_MANAGER_ID);
    let mut state = read_json_object(&path)?;
    let execution = match runner(action_id, &config) {
        Ok(value) => value,
        Err(err) => {
            let message = err.clone();
            if let Some(map) = state.as_object_mut() {
                map.insert("lastActionId".to_string(), serde_json::json!(action_id));
                map.insert(
                    "lastActionUnix".to_string(),
                    serde_json::json!(unix_now_secs()),
                );
                map.insert("lastActionOk".to_string(), serde_json::json!(false));
                map.insert("lastError".to_string(), serde_json::json!(err));
            }
            write_json_object(&path, &state)?;
            return Err(std::io::Error::other(message));
        }
    };

    if let Some(map) = state.as_object_mut() {
        map.insert("lastActionId".to_string(), serde_json::json!(action_id));
        map.insert(
            "lastActionUnix".to_string(),
            serde_json::json!(unix_now_secs()),
        );
        map.insert("lastActionOk".to_string(), serde_json::json!(true));
        map.remove("lastError");
        map.insert("lastResult".to_string(), execution.clone());

        if let Some(taskbar_auto_hide) = execution.get("taskbarAutoHide") {
            map.insert("taskbarAutoHide".to_string(), taskbar_auto_hide.clone());
        }
        if let Some(scale_current) = execution.get("scale").and_then(|v| v.get("currentPercent")) {
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
    }

    write_json_object(&path, &state)?;
    Ok(execution)
}

#[cfg(test)]
fn load_torrent_monitor_config() -> Result<TorrentMonitorConfig, std::io::Error> {
    let data_root = copper_data_root()?;
    load_torrent_monitor_config_from(&data_root)
}

#[cfg(test)]
fn load_torrent_monitor_config_from(
    data_root: &Path,
) -> Result<TorrentMonitorConfig, std::io::Error> {
    let config = load_extension_config_object(data_root, DESKTOP_TORRENT_ORGANIZER_ID)?;

    let enabled = config
        .get("autoRun")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let poll_secs = config
        .get("pollIntervalSeconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .clamp(1, 3600);
    let desktop_folder = expand_home(
        config
            .get("desktopFolder")
            .and_then(|v| v.as_str())
            .unwrap_or("~/Desktop"),
    );
    let torrents_folder = expand_home(
        config
            .get("torrentsFolder")
            .and_then(|v| v.as_str())
            .unwrap_or("~/Desktop/Torrents"),
    );

    Ok(TorrentMonitorConfig {
        enabled,
        poll_interval: Duration::from_secs(poll_secs),
        desktop_folder,
        torrents_folder,
    })
}

#[cfg(test)]
fn run_torrent_move(config: &TorrentMonitorConfig) -> Result<TorrentMoveReport, std::io::Error> {
    fs::create_dir_all(&config.torrents_folder)?;

    let mut report = TorrentMoveReport::default();
    let read_dir = match fs::read_dir(&config.desktop_folder) {
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
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if !file_name.to_ascii_lowercase().ends_with(".torrent") {
            continue;
        }

        report.found = report.found.saturating_add(1);
        let destination = next_available_destination(&config.torrents_folder, entry.file_name());
        match fs::rename(&path, &destination) {
            Ok(()) => {
                report.moved = report.moved.saturating_add(1);
            }
            Err(_) => match fs::copy(&path, &destination).and_then(|_| fs::remove_file(&path)) {
                Ok(()) => {
                    report.moved = report.moved.saturating_add(1);
                }
                Err(_) => {
                    report.failed = report.failed.saturating_add(1);
                }
            },
        }
    }
    Ok(report)
}

#[cfg(test)]
fn next_available_destination(target_dir: &Path, file_name: OsString) -> PathBuf {
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

#[cfg(test)]
fn split_name_and_extension(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base, ext),
        _ => (name, ""),
    }
}

#[cfg(test)]
fn write_desktop_torrent_status_in(
    data_root: &Path,
    config: &TorrentMonitorConfig,
    report: TorrentMoveReport,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(data_root)?;
    let path = extension_status_path_in(data_root, DESKTOP_TORRENT_ORGANIZER_ID);

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

#[cfg(test)]
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
fn copper_data_root() -> Result<PathBuf, std::io::Error> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not available")
    })?;
    Ok(copper_data_root_from_home(&home))
}

#[cfg(test)]
fn copper_data_root_from_home(home: &Path) -> PathBuf {
    home.join(".Copper").join("extensions")
}

#[cfg(test)]
fn extension_config_path_in(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("config.json")
}

#[cfg(test)]
fn extension_status_path_in(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("status.json")
}

#[cfg(test)]
fn extension_legacy_data_path_in(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("data.json")
}

#[cfg(test)]
fn load_extension_config_object(
    data_root: &Path,
    extension_id: &str,
) -> Result<serde_json::Value, std::io::Error> {
    let config_path = extension_config_path_in(data_root, extension_id);
    if config_path.exists() {
        return read_json_object(&config_path);
    }
    let legacy_path = extension_legacy_data_path_in(data_root, extension_id);
    if legacy_path.exists() {
        return read_json_object(&legacy_path);
    }
    Ok(serde_json::json!({}))
}

#[cfg(test)]
fn read_json_object(path: &Path) -> Result<serde_json::Value, std::io::Error> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let raw = fs::read_to_string(path)?;
    let parsed =
        serde_json::from_str::<serde_json::Value>(&raw).unwrap_or_else(|_| serde_json::json!({}));
    Ok(if parsed.is_object() {
        parsed
    } else {
        serde_json::json!({})
    })
}

#[cfg(test)]
fn write_json_object(path: &Path, value: &serde_json::Value) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)
}

#[cfg(test)]
fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
fn handle_request(
    state: &mut DaemonState,
    request: IpcRequest,
    running: &AtomicBool,
) -> IpcResponse {
    transport::handle_request(state, request, running)
        .expect("daemon request should produce a response")
}

#[cfg(test)]
include!("daemon_tests.rs");

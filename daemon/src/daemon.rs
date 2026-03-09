use crate::config_ui::{start_daemon_ui_server, DEFAULT_DAEMON_UI_BIND};
use crate::descriptor::Permission;
use crate::extension::{
    core_extensions_dir, default_extensions_dir, load_runtime_registry, Registry,
};
use crate::tray::TrayController;
use crate::tray_extension::AdditionalTrayController;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:4765";
pub const DEFAULT_RELOAD_INTERVAL_MS: u64 = 3_000;
const DESKTOP_TORRENT_ORGANIZER_ID: &str = "desktop-torrent-organizer";
const SESSION_COUNTER_ID: &str = "session-counter";
const SESSION_COUNTER_INCREMENT_ACTION: &str = "increment";
const WINDOWS_DISPLAY_MANAGER_ID: &str = "windows-display-manager";

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
    last_torrent_poll: Option<Instant>,
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

impl DaemonState {
    fn load(user_extensions_dir: &Path) -> Result<Self, DaemonError> {
        let registry = load_runtime_registry(user_extensions_dir)?;
        Ok(Self {
            user_extensions_dir: user_extensions_dir.to_path_buf(),
            core_extensions_dir: core_extensions_dir(),
            registry,
            last_torrent_poll: None,
        })
    }

    fn reload(&mut self) -> Result<usize, DaemonError> {
        self.registry = load_runtime_registry(&self.user_extensions_dir)?;
        self.core_extensions_dir = core_extensions_dir();
        Ok(self.registry.list().count())
    }

    fn verify_registry(&self) -> Result<usize, String> {
        let mut found = 0usize;
        for ext in self.registry.list() {
            found += 1;
            if ext.descriptor.actions.is_empty() {
                return Err(format!("extension {} has no actions", ext.descriptor.id));
            }
            if !ext.main_ts_path.exists() {
                return Err(format!(
                    "extension {} is missing main.ts",
                    ext.descriptor.id
                ));
            }
        }
        Ok(found)
    }

    fn tick_background_tasks(&mut self) {
        if let Err(err) = self.tick_torrent_monitor() {
            eprintln!("desktop torrent monitor error: {err}");
        }
    }

    fn tick_torrent_monitor(&mut self) -> Result<(), String> {
        let config = load_torrent_monitor_config().map_err(|err| err.to_string())?;
        if !config.enabled {
            return Ok(());
        }
        if let Some(last) = self.last_torrent_poll {
            if last.elapsed() < config.poll_interval {
                return Ok(());
            }
        }
        self.last_torrent_poll = Some(Instant::now());

        let report = run_torrent_move(&config).map_err(|err| err.to_string())?;
        write_desktop_torrent_status(&config, report).map_err(|err| err.to_string())?;
        Ok(())
    }
}

pub fn run_daemon(config: DaemonConfig) -> Result<(), DaemonError> {
    let listener = TcpListener::bind(&config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let running = Arc::new(AtomicBool::new(true));
    let signal_flag = Arc::clone(&running);
    ctrlc::set_handler(move || {
        signal_flag.store(false, Ordering::Relaxed);
    })
    .map_err(|e| DaemonError::SignalHandler(e.to_string()))?;

    let mut state = DaemonState::load(&config.extensions_dir)?;
    let daemon_ui_bind = std::env::var("COPPERD_DAEMON_UI_BIND")
        .unwrap_or_else(|_| DEFAULT_DAEMON_UI_BIND.to_string());
    let daemon_ui = start_daemon_ui_server(
        config.extensions_dir.clone(),
        daemon_ui_bind,
        Arc::clone(&running),
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
        let windows_display_enabled = state.registry.get(WINDOWS_DISPLAY_MANAGER_ID).is_some();
        Some(
            AdditionalTrayController::initialize(
                Arc::clone(&running),
                daemon_ui.url.clone(),
                windows_display_enabled,
            )
            .map_err(|err| DaemonError::Tray(err.to_string()))?,
        )
    };
    let additional_tray_count = additional_trays
        .as_ref()
        .map(|controller| controller.specs().len())
        .unwrap_or(0);

    println!(
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
    );

    let mut last_reload = Instant::now();
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // Keep accepted sockets in blocking mode so per-connection reads are deterministic.
                stream.set_nonblocking(false)?;
                if let Err(err) = handle_connection(stream, &mut state, &running) {
                    if !is_would_block_daemon(&err) {
                        return Err(err);
                    }
                }
            }
            Err(err) if is_would_block_io(&err) => {}
            Err(err) => return Err(DaemonError::Io(err)),
        }

        if last_reload.elapsed() >= config.reload_interval {
            let _ = state.reload()?;
            last_reload = Instant::now();
        }
        state.tick_background_tasks();
        std::thread::sleep(Duration::from_millis(50));
    }

    println!("Daemon stopped");
    Ok(())
}

fn is_would_block_io(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock || err.raw_os_error() == Some(10035)
}

fn is_would_block_daemon(err: &DaemonError) -> bool {
    matches!(err, DaemonError::Io(io) if is_would_block_io(io))
}

pub fn send_request(bind_addr: &str, request: &IpcRequest) -> Result<IpcResponse, DaemonError> {
    let mut stream = TcpStream::connect(bind_addr)?;
    let payload = format!("{}\n", serde_json::to_string(request)?);
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut response_line = String::new();
    let mut reader = BufReader::new(stream);
    let bytes = reader.read_line(&mut response_line)?;
    if bytes == 0 {
        return Err(DaemonError::Protocol(
            "daemon closed connection without a response".to_string(),
        ));
    }

    let response: IpcResponse = serde_json::from_str(response_line.trim())?;
    Ok(response)
}

fn handle_connection(
    stream: TcpStream,
    state: &mut DaemonState,
    running: &AtomicBool,
) -> Result<(), DaemonError> {
    let mut request_line = String::new();
    {
        let mut reader = BufReader::new(stream.try_clone()?);
        let read = match reader.read_line(&mut request_line) {
            Ok(read) => read,
            Err(err) if is_would_block_io(&err) => return Ok(()),
            Err(err) => return Err(DaemonError::Io(err)),
        };
        if read == 0 {
            return Ok(());
        }
    }

    let response = match serde_json::from_str::<IpcRequest>(request_line.trim()) {
        Ok(request) => handle_request(state, request, running),
        Err(err) => IpcResponse::err(format!("invalid request: {err}")),
    };

    let mut writer = stream;
    let response_body = format!("{}\n", serde_json::to_string(&response)?);
    if let Err(err) = writer.write_all(response_body.as_bytes()) {
        if !is_would_block_io(&err) {
            return Err(DaemonError::Io(err));
        }
    }
    if let Err(err) = writer.flush() {
        if !is_would_block_io(&err) {
            return Err(DaemonError::Io(err));
        }
    }
    Ok(())
}

fn handle_request(
    state: &mut DaemonState,
    request: IpcRequest,
    running: &AtomicBool,
) -> IpcResponse {
    match request {
        IpcRequest::Health => IpcResponse::ok(
            "daemon alive",
            Some(serde_json::json!({
                "userExtensionsDir": state.user_extensions_dir.display().to_string(),
                "coreExtensionsDir": state
                    .core_extensions_dir
                    .as_ref()
                    .map(|path| path.display().to_string()),
                "extensionsLoaded": state.registry.list().count(),
                "configUiUrl": format!(
                    "http://{}",
                    std::env::var("COPPERD_DAEMON_UI_BIND")
                        .unwrap_or_else(|_| DEFAULT_DAEMON_UI_BIND.to_string())
                )
            })),
        ),
        IpcRequest::List => {
            let list = state
                .registry
                .list()
                .map(|ext| {
                    serde_json::json!({
                        "id": ext.descriptor.id,
                        "name": ext.descriptor.name,
                        "version": ext.descriptor.version,
                        "trigger": ext.descriptor.trigger,
                        "permissions": permissions_as_strings(&ext.descriptor.permissions),
                    })
                })
                .collect::<Vec<_>>();
            IpcResponse::ok("extensions listed", Some(serde_json::json!(list)))
        }
        IpcRequest::Trigger { id, action } => {
            match trigger_payload(state, &id, action.as_deref()) {
                Ok(data) => IpcResponse::ok("trigger prepared", Some(data)),
                Err(message) => IpcResponse::err(message),
            }
        }
        IpcRequest::Reload => match state.reload() {
            Ok(count) => IpcResponse::ok(
                format!("reloaded {count} extension(s)"),
                Some(serde_json::json!({ "extensionsLoaded": count })),
            ),
            Err(err) => IpcResponse::err(err.to_string()),
        },
        IpcRequest::Verify => match state.verify_registry() {
            Ok(count) => IpcResponse::ok(
                format!("verified {count} extension(s)"),
                Some(serde_json::json!({ "extensionsVerified": count })),
            ),
            Err(err) => IpcResponse::err(err),
        },
        IpcRequest::Shutdown => {
            running.store(false, Ordering::Relaxed);
            IpcResponse::ok("shutdown signal accepted", None)
        }
    }
}

fn trigger_payload(
    state: &DaemonState,
    id: &str,
    action: Option<&str>,
) -> Result<serde_json::Value, String> {
    let ext = state
        .registry
        .get(id)
        .ok_or_else(|| format!("extension '{id}' not found"))?;
    let selected_action = if let Some(action_id) = action {
        ext.descriptor
            .actions
            .iter()
            .find(|candidate| candidate.id == action_id)
            .ok_or_else(|| format!("action '{action_id}' not found in extension '{id}'"))?
    } else {
        ext.descriptor
            .actions
            .first()
            .ok_or_else(|| format!("extension '{id}' contains no executable actions"))?
    };

    let mut payload = serde_json::json!({
        "extensionId": ext.descriptor.id,
        "actionId": selected_action.id,
        "permissions": permissions_as_strings(&ext.descriptor.permissions),
        "script": selected_action.script,
        "mainTsPath": ext.main_ts_path.display().to_string(),
    });

    if let Some(count) = maybe_increment_session_counter(&ext.descriptor.id, &selected_action.id)
        .map_err(|err| format!("failed to update session counter status: {err}"))?
    {
        payload["sessionCount"] = serde_json::json!(count);
    }

    if let Some(execution) =
        maybe_execute_windows_display_action(&ext.descriptor.id, &selected_action.id)
            .map_err(|err| format!("failed to execute windows display action: {err}"))?
    {
        payload["hostExecution"] = execution;
    }

    Ok(payload)
}

fn permissions_as_strings(permissions: &[Permission]) -> Vec<&'static str> {
    permissions
        .iter()
        .map(|permission| match permission {
            Permission::Fs => "fs",
            Permission::Shell => "shell",
            Permission::Network => "network",
            Permission::Store => "store",
            Permission::Ui => "ui",
        })
        .collect()
}

pub fn maybe_increment_session_counter(
    extension_id: &str,
    action_id: &str,
) -> Result<Option<u64>, std::io::Error> {
    let data_root = copper_data_root()?;
    maybe_increment_session_counter_in(&data_root, extension_id, action_id)
}

fn maybe_increment_session_counter_in(
    data_root: &Path,
    extension_id: &str,
    action_id: &str,
) -> Result<Option<u64>, std::io::Error> {
    if extension_id != SESSION_COUNTER_ID || action_id != SESSION_COUNTER_INCREMENT_ACTION {
        return Ok(None);
    }

    fs::create_dir_all(data_root)?;
    let path = extension_data_path_in(data_root, SESSION_COUNTER_ID);

    let mut status = read_json_object(&path)?;
    let current = status.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
    let next = current.saturating_add(1);
    status["count"] = serde_json::json!(next);
    status["lastIncrementUnix"] = serde_json::json!(unix_now_secs());
    status["lastActionId"] = serde_json::json!(SESSION_COUNTER_INCREMENT_ACTION);
    write_json_object(&path, &status)?;
    Ok(Some(next))
}

fn maybe_execute_windows_display_action(
    extension_id: &str,
    action_id: &str,
) -> Result<Option<serde_json::Value>, std::io::Error> {
    if extension_id != WINDOWS_DISPLAY_MANAGER_ID {
        return Ok(None);
    }

    let data_root = copper_data_root()?;
    maybe_execute_windows_display_action_in(&data_root, action_id)
}

fn maybe_execute_windows_display_action_in(
    data_root: &Path,
    action_id: &str,
) -> Result<Option<serde_json::Value>, std::io::Error> {
    fs::create_dir_all(data_root)?;
    let path = extension_data_path_in(data_root, WINDOWS_DISPLAY_MANAGER_ID);
    let mut state = read_json_object(&path)?;
    let execution = match crate::api::windows_display::execute_action(action_id, &state) {
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
    Ok(Some(execution))
}

fn load_torrent_monitor_config() -> Result<TorrentMonitorConfig, std::io::Error> {
    let data_root = copper_data_root()?;
    load_torrent_monitor_config_from(&data_root)
}

fn load_torrent_monitor_config_from(
    data_root: &Path,
) -> Result<TorrentMonitorConfig, std::io::Error> {
    let path = extension_data_path_in(data_root, DESKTOP_TORRENT_ORGANIZER_ID);
    let config = read_json_object(&path)?;

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

fn split_name_and_extension(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() => (base, ext),
        _ => (name, ""),
    }
}

fn write_desktop_torrent_status(
    config: &TorrentMonitorConfig,
    report: TorrentMoveReport,
) -> Result<(), std::io::Error> {
    let data_root = copper_data_root()?;
    write_desktop_torrent_status_in(&data_root, config, report)
}

fn write_desktop_torrent_status_in(
    data_root: &Path,
    config: &TorrentMonitorConfig,
    report: TorrentMoveReport,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(data_root)?;
    let path = extension_data_path_in(data_root, DESKTOP_TORRENT_ORGANIZER_ID);

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

fn copper_data_root() -> Result<PathBuf, std::io::Error> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not available")
    })?;
    Ok(copper_data_root_from_home(&home))
}

fn copper_data_root_from_home(home: &Path) -> PathBuf {
    home.join(".Copper").join("extensions")
}

fn extension_data_path_in(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("data.json")
}

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

fn write_json_object(path: &Path, value: &serde_json::Value) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        copper_data_root_from_home, expand_home, extension_data_path_in, handle_connection,
        handle_request, is_would_block_daemon, is_would_block_io, load_torrent_monitor_config,
        load_torrent_monitor_config_from, maybe_increment_session_counter,
        maybe_increment_session_counter_in, next_available_destination, read_json_object,
        run_torrent_move, send_request, split_name_and_extension, trigger_payload,
        write_desktop_torrent_status_in, write_json_object, DaemonConfig, DaemonError, DaemonState,
        IpcRequest, TorrentMonitorConfig, DEFAULT_BIND_ADDR, DEFAULT_RELOAD_INTERVAL_MS,
    };
    use std::fs;
    use std::io::{self, Read};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    use tempfile::tempdir;

    fn write_extension(root: &Path, id: &str) {
        write_extension_with_action(root, id, "run");
    }

    fn write_extension_with_action(root: &Path, id: &str, action_id: &str) {
        let ext = root.join(id);
        fs::create_dir_all(&ext).expect("create extension directory");
        fs::write(
            ext.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "Test Extension",
                    "version": "1.0.0",
                    "trigger": "test",
                    "permissions": ["fs", "ui"],
                    "actions": [
                        {{ "id": "{action_id}", "label": "Run", "script": "return;" }}
                    ]
                }}"#
            ),
        )
        .expect("write descriptor");
        fs::write(
            ext.join("main.ts"),
            "export default function(){ return {}; }",
        )
        .expect("write main.ts");
    }

    fn write_windows_display_extension(root: &Path) {
        let ext = root.join("windows-display-manager");
        fs::create_dir_all(&ext).expect("create extension directory");
        fs::write(
            ext.join("manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "windows-display-manager",
                "name": "Windows Display Manager",
                "version": "1.0.0",
                "trigger": "windows-display",
                "permissions": ["ui", "store"],
                "actions": [
                    { "id": "status", "label": "Status", "script": "status" },
                    { "id": "toggle-taskbar-autohide", "label": "Toggle", "script": "toggle" }
                ]
            }"#,
        )
        .expect("write descriptor");
        fs::write(
            ext.join("main.ts"),
            "export default function(){ return {}; }",
        )
        .expect("write main.ts");
    }

    #[test]
    fn daemon_config_default_matches_public_constants() {
        let config = DaemonConfig::default();
        assert_eq!(config.bind_addr, DEFAULT_BIND_ADDR);
        assert_eq!(
            config.reload_interval,
            Duration::from_millis(DEFAULT_RELOAD_INTERVAL_MS)
        );
        assert!(!config.extensions_dir.as_os_str().is_empty());
    }

    #[test]
    fn health_request_returns_extension_count() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::Health, &running);
        assert!(response.ok);
        let count = response
            .data
            .expect("data")
            .get("extensionsLoaded")
            .and_then(|v| v.as_u64())
            .expect("count");
        assert!(
            count >= 1,
            "health should report at least the temp extension plus any shipped core extensions"
        );
    }

    #[test]
    fn trigger_request_returns_error_for_missing_extension() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "missing".to_string(),
                action: None,
            },
            &running,
        );
        assert!(!response.ok);
        assert!(response.message.contains("not found"));
    }

    #[test]
    fn reload_request_picks_new_extension() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let baseline_count = state.registry.list().count() as u64;

        write_extension(temp.path(), "new-ext");
        let response = handle_request(&mut state, IpcRequest::Reload, &running);
        assert!(response.ok);
        let count = response
            .data
            .expect("data")
            .get("extensionsLoaded")
            .and_then(|v| v.as_u64())
            .expect("count");
        assert_eq!(count, baseline_count + 1);
    }

    #[test]
    fn would_block_helper_accepts_error_kind() {
        let err = io::Error::new(io::ErrorKind::WouldBlock, "busy");
        assert!(is_would_block_io(&err));
    }

    #[test]
    fn would_block_helper_accepts_windows_10035() {
        let err = io::Error::from_raw_os_error(10035);
        assert!(is_would_block_io(&err));
    }

    #[test]
    fn would_block_daemon_helper_checks_io_variant_only() {
        let io_err = DaemonError::Io(io::Error::new(io::ErrorKind::WouldBlock, "busy"));
        assert!(is_would_block_daemon(&io_err));
        let protocol = DaemonError::Protocol("nope".to_string());
        assert!(!is_would_block_daemon(&protocol));
    }

    #[test]
    fn trigger_session_counter_includes_incremented_count() {
        let temp = tempdir().expect("tempdir");
        let status_home = temp.path().join("home");
        let data_root = copper_data_root_from_home(&status_home);
        let count1 = maybe_increment_session_counter_in(&data_root, "session-counter", "increment")
            .expect("increment")
            .expect("count");
        let count2 = maybe_increment_session_counter_in(&data_root, "session-counter", "increment")
            .expect("increment again")
            .expect("count again");
        let skipped = maybe_increment_session_counter_in(&data_root, "session-counter", "other")
            .expect("skip");
        assert_eq!(count1, 1);
        assert_eq!(count2, 2);
        assert!(skipped.is_none());
    }

    #[test]
    fn load_torrent_monitor_config_reads_polling_fields() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let ext_dir = data_root.join("desktop-torrent-organizer");
        fs::create_dir_all(&ext_dir).expect("create extension data dir");
        fs::write(
            ext_dir.join("data.json"),
            r#"{
              "autoRun": false,
              "pollIntervalSeconds": 12,
              "desktopFolder": "/tmp/desktop",
              "torrentsFolder": "/tmp/desktop/Torrents"
            }"#,
        )
        .expect("write config");

        let cfg = load_torrent_monitor_config_from(&data_root).expect("load");
        assert!(!cfg.enabled);
        assert_eq!(cfg.poll_interval.as_secs(), 12);
        assert_eq!(cfg.desktop_folder, PathBuf::from("/tmp/desktop"));
        assert_eq!(cfg.torrents_folder, PathBuf::from("/tmp/desktop/Torrents"));
    }

    #[test]
    fn run_torrent_move_moves_only_torrent_files() {
        let temp = tempdir().expect("tempdir");
        let desktop = temp.path().join("Desktop");
        let torrents = desktop.join("Torrents");
        fs::create_dir_all(&desktop).expect("create desktop");
        fs::write(desktop.join("movie.torrent"), "data").expect("write torrent");
        fs::write(desktop.join("note.txt"), "data").expect("write non-torrent");

        let cfg = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(1),
            desktop_folder: desktop.clone(),
            torrents_folder: torrents.clone(),
        };
        let report = run_torrent_move(&cfg).expect("run move");
        assert_eq!(report.found, 1);
        assert_eq!(report.moved, 1);
        assert_eq!(report.failed, 0);
        assert!(torrents.join("movie.torrent").exists());
        assert!(desktop.join("note.txt").exists());
    }

    #[test]
    fn list_request_returns_permissions_as_strings() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::List, &running);
        assert!(response.ok);
        let list = response
            .data
            .expect("data")
            .as_array()
            .expect("array")
            .to_vec();
        assert!(!list.is_empty());
        let permissions = list[0]
            .get("permissions")
            .and_then(|v| v.as_array())
            .expect("permissions");
        assert!(permissions.iter().any(|value| value.as_str() == Some("fs")));
    }

    #[test]
    fn shutdown_request_flips_running_flag() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::Shutdown, &running);
        assert!(response.ok);
        assert!(!running.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn split_name_and_extension_handles_edge_cases() {
        assert_eq!(
            split_name_and_extension("movie.torrent"),
            ("movie", "torrent")
        );
        assert_eq!(split_name_and_extension("archive"), ("archive", ""));
        assert_eq!(split_name_and_extension(".hidden"), (".hidden", ""));
    }

    #[test]
    fn next_available_destination_uses_suffix_on_collision() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("movie.torrent"), "existing").expect("write existing");
        fs::write(target.join("movie-1.torrent"), "existing").expect("write existing suffix");

        let candidate = next_available_destination(target, "movie.torrent".into());
        assert_eq!(
            candidate.file_name().and_then(|v| v.to_str()),
            Some("movie-2.torrent")
        );
    }

    #[test]
    fn json_object_helpers_roundtrip_and_sanitize() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("status.json");

        write_json_object(&path, &serde_json::json!({"ok":true})).expect("write object");
        let object = read_json_object(&path).expect("read object");
        assert_eq!(object.get("ok").and_then(|v| v.as_bool()), Some(true));

        fs::write(&path, "[]").expect("write non-object");
        assert_eq!(
            read_json_object(&path).expect("read non-object"),
            serde_json::json!({})
        );

        fs::write(&path, "{bad-json").expect("write invalid");
        assert_eq!(
            read_json_object(&path).expect("read invalid"),
            serde_json::json!({})
        );
    }

    #[test]
    fn write_json_object_creates_missing_parent_directories() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("x").join("y").join("data.json");
        write_json_object(&nested, &serde_json::json!({"ok": true})).expect("write nested");
        assert!(nested.exists());
    }

    #[test]
    fn extension_data_path_is_scoped_to_extension() {
        let root = PathBuf::from("C:/tmp/.Copper/extensions");
        let path = extension_data_path_in(&root, "desktop-torrent-organizer");
        assert_eq!(
            path,
            root.join("desktop-torrent-organizer").join("data.json")
        );
    }

    #[test]
    fn verify_request_reports_verified_count() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::Verify, &running);
        assert!(response.ok);
        assert!(response.message.contains("verified"));
    }

    #[test]
    fn trigger_request_errors_for_unknown_action() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "alpha-ext".to_string(),
                action: Some("missing-action".to_string()),
            },
            &running,
        );
        assert!(!response.ok);
        assert!(response.message.contains("not found"));
    }

    #[test]
    fn run_torrent_move_handles_missing_desktop_folder() {
        let temp = tempdir().expect("tempdir");
        let cfg = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(1),
            desktop_folder: temp.path().join("does-not-exist"),
            torrents_folder: temp.path().join("Torrents"),
        };

        let report = run_torrent_move(&cfg).expect("missing folder should not fail");
        assert_eq!(report.found, 0);
        assert_eq!(report.moved, 0);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn run_torrent_move_errors_when_desktop_is_not_directory() {
        let temp = tempdir().expect("tempdir");
        let desktop_file = temp.path().join("Desktop");
        fs::write(&desktop_file, "not a dir").expect("write desktop file");
        let cfg = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(1),
            desktop_folder: desktop_file,
            torrents_folder: temp.path().join("Torrents"),
        };
        let err = run_torrent_move(&cfg).expect_err("must fail for non-directory desktop");
        assert!(
            err.kind() == std::io::ErrorKind::NotADirectory
                || err.kind() == std::io::ErrorKind::Other
        );
    }

    #[test]
    fn next_available_destination_handles_names_without_extension() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("README"), "existing").expect("write existing");

        let candidate = next_available_destination(target, "README".into());
        assert_eq!(
            candidate.file_name().and_then(|v| v.to_str()),
            Some("README-1")
        );
    }

    #[test]
    fn next_available_destination_uses_timestamp_fallback_after_many_collisions() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("movie.torrent"), "existing").expect("seed");
        for idx in 1..=9999u32 {
            fs::write(target.join(format!("movie-{idx}.torrent")), "existing")
                .expect("seed suffix");
        }

        let candidate = next_available_destination(target, "movie.torrent".into());
        let name = candidate
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name")
            .to_string();
        assert!(name.starts_with("movie-"));
        assert!(name.ends_with(".torrent"));
        assert!(!target.join(&name).exists());
    }

    #[test]
    fn write_desktop_torrent_status_persists_scan_fields() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let config = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(5),
            desktop_folder: temp.path().join("Desktop"),
            torrents_folder: temp.path().join("Desktop/Torrents"),
        };
        let report = super::TorrentMoveReport {
            found: 3,
            moved: 2,
            failed: 1,
        };

        write_desktop_torrent_status_in(&data_root, &config, report).expect("write status");
        let stored = read_json_object(&extension_data_path_in(
            &data_root,
            "desktop-torrent-organizer",
        ))
        .expect("read status");
        assert_eq!(stored.get("autoRun").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            stored.get("pollIntervalSeconds").and_then(|v| v.as_u64()),
            Some(5)
        );
        assert_eq!(
            stored.get("lastScanFound").and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(
            stored.get("lastScanMoved").and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(
            stored.get("lastScanFailed").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert!(stored
            .get("lastMoveUnix")
            .and_then(|v| v.as_u64())
            .is_some());
    }

    fn free_addr() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free");
        let addr = listener.local_addr().expect("local addr");
        format!("127.0.0.1:{}", addr.port())
    }

    #[test]
    fn send_request_errors_when_daemon_closes_without_reply() {
        let addr = free_addr();
        let listener = TcpListener::bind(&addr).expect("bind");
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            drop(stream);
        });

        let err = send_request(&addr, &IpcRequest::Health).expect_err("must fail");
        match err {
            DaemonError::Protocol(message) => assert!(message.contains("without a response")),
            DaemonError::Io(io) => {
                // Windows may surface a connection-reset instead of EOF on immediate close.
                assert!(
                    io.kind() == std::io::ErrorKind::ConnectionReset
                        || io.kind() == std::io::ErrorKind::ConnectionAborted
                );
            }
            other => panic!("unexpected error: {other}"),
        }
        server.join().expect("join server");
    }

    #[test]
    fn send_request_errors_on_invalid_json_response() {
        let addr = free_addr();
        let listener = TcpListener::bind(&addr).expect("bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 256];
            let _ = stream.read(&mut request);
            std::io::Write::write_all(&mut stream, b"not-json\n").expect("write invalid");
            std::io::Write::flush(&mut stream).expect("flush");
        });

        let err = send_request(&addr, &IpcRequest::Health).expect_err("must fail");
        match err {
            DaemonError::Serde(_) => {}
            other => panic!("unexpected error: {other}"),
        }
        server.join().expect("join server");
    }

    #[test]
    fn send_request_reports_protocol_error_on_clean_eof() {
        let addr = free_addr();
        let listener = TcpListener::bind(&addr).expect("bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 256];
            let _ = stream.read(&mut request);
            stream.shutdown(Shutdown::Write).expect("shutdown write");
            std::thread::sleep(Duration::from_millis(20));
        });

        let err = send_request(&addr, &IpcRequest::Health).expect_err("must fail");
        match err {
            DaemonError::Protocol(message) => assert!(message.contains("without a response")),
            other => panic!("unexpected error: {other}"),
        }
        server.join().expect("join server");
    }

    #[test]
    fn raw_invalid_request_over_socket_returns_error_response() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let addr = free_addr();
        let listener = TcpListener::bind(&addr).expect("bind");

        let client = std::thread::spawn({
            let addr = addr.clone();
            move || {
                let mut stream = TcpStream::connect(&addr).expect("connect");
                std::io::Write::write_all(&mut stream, b"{not-json}\n").expect("write request");
                std::io::Write::flush(&mut stream).expect("flush");
                let mut response = String::new();
                stream.read_to_string(&mut response).expect("read response");
                response
            }
        });

        let (stream, _) = listener.accept().expect("accept");
        super::handle_connection(stream, &mut state, &running).expect("handle connection");

        let response = client.join().expect("join");
        assert!(response.contains("\"ok\":false"));
        assert!(response.contains("invalid request"));
    }

    #[test]
    fn send_request_parses_successful_json_response() {
        let addr = free_addr();
        let listener = TcpListener::bind(&addr).expect("bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 256];
            let _ = stream.read(&mut request);
            std::io::Write::write_all(
                &mut stream,
                br#"{"ok":true,"message":"healthy","data":{"x":1}}"#,
            )
            .expect("write");
            std::io::Write::write_all(&mut stream, b"\n").expect("newline");
            std::io::Write::flush(&mut stream).expect("flush");
        });

        let response = send_request(&addr, &IpcRequest::Health).expect("response");
        assert!(response.ok);
        assert_eq!(response.message, "healthy");
        server.join().expect("join");
    }

    #[test]
    fn handle_connection_returns_ok_on_empty_read() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let addr = free_addr();
        let listener = TcpListener::bind(&addr).expect("bind");

        let client = std::thread::spawn({
            let addr = addr.clone();
            move || {
                let stream = TcpStream::connect(&addr).expect("connect");
                drop(stream);
            }
        });

        let (stream, _) = listener.accept().expect("accept");
        handle_connection(stream, &mut state, &running).expect("empty request should be ok");
        client.join().expect("join");
    }

    #[test]
    fn trigger_request_without_action_uses_first_action() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "alpha-ext".to_string(),
                action: None,
            },
            &running,
        );
        assert!(response.ok);
        let data = response.data.expect("payload");
        assert_eq!(data.get("actionId").and_then(|v| v.as_str()), Some("run"));
    }

    #[test]
    fn trigger_request_session_counter_includes_count_payload() {
        let temp = tempdir().expect("tempdir");
        write_extension_with_action(temp.path(), "session-counter", "increment");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "session-counter".to_string(),
                action: Some("increment".to_string()),
            },
            &running,
        );
        assert!(response.ok);
        let data = response.data.expect("payload");
        assert!(data.get("sessionCount").and_then(|v| v.as_u64()).is_some());
    }

    #[test]
    fn trigger_payload_windows_display_status_reports_host_execution() {
        let temp = tempdir().expect("tempdir");
        write_windows_display_extension(temp.path());
        let state = DaemonState::load(temp.path()).expect("state");

        let result = trigger_payload(&state, "windows-display-manager", Some("status"));
        if cfg!(target_os = "windows") {
            let payload = result.expect("payload");
            assert_eq!(
                payload.get("extensionId").and_then(|v| v.as_str()),
                Some("windows-display-manager")
            );
            assert_eq!(
                payload.get("actionId").and_then(|v| v.as_str()),
                Some("status")
            );
            assert!(
                payload.get("hostExecution").is_some(),
                "windows-display status action should include host execution payload"
            );
        } else {
            let err = result.expect_err("non-windows should not support display manager");
            assert!(err.contains("only supported on Windows"));
        }
    }

    #[test]
    fn verify_request_errors_when_loaded_extension_loses_main_file() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        fs::remove_file(temp.path().join("alpha-ext").join("main.ts")).expect("remove main");
        let running = AtomicBool::new(true);
        let response = handle_request(&mut state, IpcRequest::Verify, &running);
        assert!(!response.ok);
        assert!(response.message.contains("missing main.ts"));
    }

    #[test]
    fn reload_request_reports_error_for_invalid_manifest() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let broken = temp.path().join("broken-ext");
        fs::create_dir_all(&broken).expect("create broken dir");
        fs::write(broken.join("manifest.json"), "{}").expect("write invalid manifest");
        fs::write(broken.join("main.ts"), "export default function(){}").expect("write main");
        let running = AtomicBool::new(true);
        let response = handle_request(&mut state, IpcRequest::Reload, &running);
        assert!(!response.ok);
        assert!(response.message.contains("manifest"));
    }

    #[test]
    fn maybe_increment_session_counter_handles_missing_or_present_home() {
        match maybe_increment_session_counter("not-session-counter", "noop") {
            Ok(value) => assert!(value.is_none()),
            Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn load_torrent_monitor_config_handles_missing_or_present_home() {
        match load_torrent_monitor_config() {
            Ok(cfg) => {
                let secs = cfg.poll_interval.as_secs();
                assert!((1..=3600).contains(&secs));
            }
            Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn expand_home_handles_tilde_variants() {
        let expanded_home = expand_home("~");
        let expanded_child = expand_home("~/Desktop");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded_home, home);
            assert_eq!(expanded_child, home.join("Desktop"));
        } else {
            assert_eq!(expanded_home, PathBuf::from("~"));
            assert_eq!(expanded_child, PathBuf::from("~/Desktop"));
        }

        let literal = expand_home("C:/tmp/Desktop");
        assert_eq!(literal, PathBuf::from("C:/tmp/Desktop"));
    }

    #[test]
    fn write_desktop_torrent_status_skips_last_move_when_nothing_moved() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let config = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(5),
            desktop_folder: temp.path().join("Desktop"),
            torrents_folder: temp.path().join("Desktop/Torrents"),
        };
        let report = super::TorrentMoveReport {
            found: 1,
            moved: 0,
            failed: 1,
        };

        write_desktop_torrent_status_in(&data_root, &config, report).expect("write status");
        let stored = read_json_object(&extension_data_path_in(
            &data_root,
            "desktop-torrent-organizer",
        ))
        .expect("read status");
        assert!(stored.get("lastMoveUnix").is_none());
    }
}

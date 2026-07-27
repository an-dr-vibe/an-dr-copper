use crate::autostart;
use crate::bones_integration::BonesDaemonDriver;
use crate::config_ui::settings_ui_channel;
#[cfg(feature = "native-ui")]
use crate::config_ui::NativeSettingsPresentation;
use crate::control_plane::ControlPlaneAuth;
use crate::core_config::{load_core_config, CoreConfig};
use crate::daemon_scheduler::DaemonScheduler;
use crate::extension::{
    core_extensions_dir, default_extensions_dir, load_runtime_registry, Registry,
};
use crate::host_extensions::HostExtensionRegistry;
use crate::hotkey::HotkeyController;
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
use std::time::{Duration, Instant};
use thiserror::Error;
use tiny_http::Server;

#[path = "daemon_transport.rs"]
mod transport;

use transport::handle_http_request;
pub use transport::send_request;
#[cfg(test)]
use transport::{parse_http_response, request_url};

#[cfg(test)]
use crate::host_extensions::WINDOWS_DISPLAY_MANAGER_ID;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:4765";
pub const DEFAULT_RELOAD_INTERVAL_MS: u64 = 3_000;

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
    #[error("Bones runtime error: {0}")]
    Bones(String),
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

struct DaemonState {
    user_extensions_dir: PathBuf,
    core_extensions_dir: Option<PathBuf>,
    registry: Registry,
    core_config: CoreConfig,
    auth_token: Option<String>,
    state_store: ExtensionStateStore,
    host_extensions: HostExtensionRegistry,
    bones: BonesDaemonDriver,
}

impl DaemonState {
    fn load(user_extensions_dir: &Path) -> Result<Self, DaemonError> {
        let registry = load_runtime_registry(user_extensions_dir)?;
        let core_config = load_core_config().unwrap_or_default();
        let state_store = ExtensionStateStore::for_current_user()?;
        let bones =
            BonesDaemonDriver::new(&registry, state_store.clone()).map_err(DaemonError::Bones)?;
        Ok(Self {
            user_extensions_dir: user_extensions_dir.to_path_buf(),
            core_extensions_dir: core_extensions_dir(),
            registry,
            core_config,
            auth_token: None,
            state_store,
            host_extensions: HostExtensionRegistry::new(),
            bones,
        })
    }

    fn reload(&mut self) -> Result<usize, DaemonError> {
        let registry = load_runtime_registry(&self.user_extensions_dir)?;
        let core_extensions_dir = core_extensions_dir();
        let core_config = load_core_config().unwrap_or_default();
        self.bones
            .replace_catalog(&registry)
            .map_err(DaemonError::Bones)?;
        self.registry = registry;
        self.core_extensions_dir = core_extensions_dir;
        self.core_config = core_config;
        Ok(self.registry.list().count())
    }

    fn control_service(&self) -> crate::daemon_service::DaemonControlService<'_> {
        crate::daemon_service::DaemonControlService::new(
            &self.user_extensions_dir,
            self.core_extensions_dir.as_deref(),
            &self.registry,
            &self.host_extensions,
            &self.state_store,
            self.bones.status(),
        )
    }

    fn trigger_payload(
        &mut self,
        extension_id: &str,
        action_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let extension = self
            .registry
            .get(extension_id)
            .ok_or_else(|| format!("extension '{extension_id}' not found"))?;
        if extension.wasm_component_path().is_none() {
            return self
                .control_service()
                .trigger_payload(extension_id, action_id);
        }
        let action = match action_id {
            Some(action_id) => extension
                .descriptor
                .actions
                .iter()
                .find(|action| action.id == action_id)
                .ok_or_else(|| format!("action '{action_id}' not found"))?,
            None => extension
                .descriptor
                .actions
                .first()
                .ok_or_else(|| "no action defined".to_string())?,
        };
        let action_id = action.id.clone();
        let permissions =
            crate::execution::permissions_as_strings(&extension.descriptor.permissions);
        let dispatch = self.bones.dispatch_action(
            extension_id,
            &action_id,
            serde_json::Map::from_iter([(
                "source".to_string(),
                serde_json::Value::String("control-plane".to_string()),
            )]),
        )?;
        let mut payload = serde_json::to_value(dispatch).map_err(|error| error.to_string())?;
        if let Some(object) = payload.as_object_mut() {
            object.insert("permissions".to_string(), serde_json::json!(permissions));
            object.insert(
                "runtime".to_string(),
                serde_json::json!({
                    "abiVersion": crate::bones_integration::COPPER_BUS_PROTOCOL_V1,
                    "executor": "bones",
                    "isolated": true,
                }),
            );
        }
        Ok(payload)
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
    let (settings_ui, settings_requests) = settings_ui_channel();
    #[cfg(not(feature = "native-ui"))]
    let _ = settings_requests;
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
                settings_ui.clone(),
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
                settings_ui,
                &state.registry,
            )
            .map_err(|err| DaemonError::Tray(err.to_string()))?,
        )
    };
    let additional_tray_count = additional_trays
        .as_ref()
        .map(|controller| controller.specs().len())
        .unwrap_or(0);
    let _hotkeys = HotkeyController::initialize(
        Arc::clone(&running),
        config.bind_addr.clone(),
        state.state_store.clone(),
    )
    .map_err(DaemonError::Tray)?;

    logging::info(format!(
        "Daemon started on {} (user extensions: {}, core extensions: {}, config UI: Bones/wry on demand, additional tray icons: {})",
        config.bind_addr,
        config.extensions_dir.display(),
        state
            .core_extensions_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<not found>".to_string()),
        additional_tray_count
    ));

    let mut scheduler = DaemonScheduler::new(config.reload_interval);
    let mut last_bones_step = Instant::now();
    #[cfg(feature = "native-ui")]
    let mut settings_presentation: Option<NativeSettingsPresentation> = None;
    #[cfg(feature = "native-ui")]
    let mut settings_selection: Option<Option<String>> = None;
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
        scheduler.tick_background(
            &state.registry,
            &state.host_extensions,
            &state.state_store,
            &state.core_config,
        );
        match scheduler.due_bones_background(&state.registry, &state.state_store) {
            Ok(actions) => {
                for action in actions {
                    let input = serde_json::Map::from_iter([(
                        "source".to_string(),
                        serde_json::Value::String("background".to_string()),
                    )]);
                    match state.bones.dispatch_action(
                        &action.extension_id,
                        &action.action_id,
                        input,
                    ) {
                        Ok(_) => scheduler.mark_bones_background_run(&action),
                        Err(error) => logging::error(format!(
                            "Bones background action error [{} -> {}]: {}",
                            action.extension_id, action.action_id, error
                        )),
                    }
                }
            }
            Err(error) => logging::error(format!(
                "failed to inspect manifest background schedules: {error}"
            )),
        }
        let now = Instant::now();
        #[cfg(feature = "native-ui")]
        {
            for selected_extension_id in settings_requests.try_iter() {
                settings_selection = Some(selected_extension_id.clone());
                if let Some(mut presentation) = settings_presentation.take() {
                    presentation.close();
                }
                match NativeSettingsPresentation::open(
                    state.bones.presentation_bus(),
                    state.bones.presentation_registry(),
                    &config.extensions_dir,
                    selected_extension_id.as_deref(),
                ) {
                    Ok(presentation) => settings_presentation = Some(presentation),
                    Err(error) => {
                        logging::error(format!("failed to open Bones settings UI: {error}"))
                    }
                }
            }
            let presentation_lost_during_reload = settings_presentation.is_some()
                && !state
                    .bones
                    .presentation_registry()
                    .contains(bones_messages::web::ENDPOINT);
            if presentation_lost_during_reload {
                if let Some(mut presentation) = settings_presentation.take() {
                    presentation.close();
                }
                if let Some(selected_extension_id) = settings_selection.as_ref() {
                    match NativeSettingsPresentation::open(
                        state.bones.presentation_bus(),
                        state.bones.presentation_registry(),
                        &config.extensions_dir,
                        selected_extension_id.as_deref(),
                    ) {
                        Ok(presentation) => settings_presentation = Some(presentation),
                        Err(error) => logging::error(format!(
                            "failed to restore Bones settings UI after reload: {error}"
                        )),
                    }
                }
            }
            let should_close = settings_presentation
                .as_mut()
                .map(NativeSettingsPresentation::update)
                .unwrap_or(false);
            if should_close {
                settings_selection = None;
                if let Some(mut presentation) = settings_presentation.take() {
                    presentation.close();
                }
            }
        }
        state
            .bones
            .step(now.saturating_duration_since(last_bones_step));
        last_bones_step = now;
    }

    #[cfg(feature = "native-ui")]
    if let Some(mut presentation) = settings_presentation {
        presentation.close();
    }
    state.bones.shutdown();
    logging::info("Daemon stopped");
    Ok(())
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

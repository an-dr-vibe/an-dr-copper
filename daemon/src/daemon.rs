use crate::descriptor::Permission;
use crate::extension::{default_extensions_dir, Registry};
use crate::tray::TrayController;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use thiserror::Error;

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
    extensions_dir: PathBuf,
    registry: Registry,
}

impl DaemonState {
    fn load(extensions_dir: &Path) -> Result<Self, DaemonError> {
        let registry = Registry::load_from_dir(extensions_dir)?;
        Ok(Self {
            extensions_dir: extensions_dir.to_path_buf(),
            registry,
        })
    }

    fn reload(&mut self) -> Result<usize, DaemonError> {
        self.registry = Registry::load_from_dir(&self.extensions_dir)?;
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
    let _tray = TrayController::initialize(Arc::clone(&running))
        .map_err(|err| DaemonError::Tray(err.to_string()))?;

    println!(
        "Daemon started on {} (extensions: {})",
        config.bind_addr,
        config.extensions_dir.display()
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
                "extensionsDir": state.extensions_dir.display().to_string(),
                "extensionsLoaded": state.registry.list().count()
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

    Ok(serde_json::json!({
        "extensionId": ext.descriptor.id,
        "actionId": selected_action.id,
        "permissions": permissions_as_strings(&ext.descriptor.permissions),
        "script": selected_action.script,
        "mainTsPath": ext.main_ts_path.display().to_string(),
    }))
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

#[cfg(test)]
mod tests {
    use super::{handle_request, is_would_block_io, DaemonState, IpcRequest};
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    fn write_extension(root: &Path, id: &str) {
        let ext = root.join(id);
        fs::create_dir_all(&ext).expect("create extension directory");
        fs::write(
            ext.join("descriptor.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "Test Extension",
                    "version": "1.0.0",
                    "trigger": "test",
                    "permissions": ["fs", "ui"],
                    "actions": [
                        {{ "id": "run", "label": "Run", "script": "return;" }}
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
        assert_eq!(count, 1);
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

        write_extension(temp.path(), "new-ext");
        let response = handle_request(&mut state, IpcRequest::Reload, &running);
        assert!(response.ok);
        let count = response
            .data
            .expect("data")
            .get("extensionsLoaded")
            .and_then(|v| v.as_u64())
            .expect("count");
        assert_eq!(count, 1);
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
}

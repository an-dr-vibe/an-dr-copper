use crate::daemon::execute_windows_display_action_in;
use crate::descriptor::Descriptor;
use crate::extension::{core_extensions_dir, load_runtime_registry, runtime_extension_roots};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use thiserror::Error;

const DEFAULT_UI_BIND: &str = "127.0.0.1:0";
pub const DEFAULT_DAEMON_UI_BIND: &str = "127.0.0.1:4766";

#[derive(Debug, Clone)]
pub struct UiOpenOptions {
    pub bind_addr: String,
    pub open_browser: bool,
    pub idle_timeout: Duration,
}

impl Default for UiOpenOptions {
    fn default() -> Self {
        Self {
            bind_addr: DEFAULT_UI_BIND.to_string(),
            open_browser: true,
            idle_timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Error)]
pub enum UiConfigError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Extension(#[from] crate::extension::ExtensionError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("extension '{0}' not found")]
    ExtensionNotFound(String),
    #[error("invalid request: {0}")]
    Request(String),
    #[error("failed to open browser: {0}")]
    Browser(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HttpMethod {
    Get,
    Post,
}

#[derive(Debug)]
struct HttpRequest {
    method: HttpMethod,
    path: String,
    body: Vec<u8>,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl HttpResponse {
    fn ok_json(value: &Value) -> Result<Self, UiConfigError> {
        Ok(Self {
            status: 200,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(value)?,
        })
    }

    fn ok_html(html: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: html.into_bytes(),
        }
    }

    fn no_content() -> Self {
        Self {
            status: 204,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        let payload = serde_json::json!({ "error": message.into() });
        Self {
            status: 400,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&payload)
                .unwrap_or_else(|_| b"{\"error\":\"bad request\"}".to_vec()),
        }
    }

    fn not_found() -> Self {
        let payload = serde_json::json!({ "error": "not found" });
        Self {
            status: 404,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&payload)
                .unwrap_or_else(|_| b"{\"error\":\"not found\"}".to_vec()),
        }
    }
}

#[derive(Debug, Clone)]
struct UiServerState {
    selected_extension_id: String,
    descriptors: Vec<Descriptor>,
    extension_ids: HashSet<String>,
    user_extensions_dir: PathBuf,
    core_extensions_dir: Option<PathBuf>,
    runtime_extension_roots: Vec<PathBuf>,
    data_root: PathBuf,
    allow_close: bool,
}

pub struct PersistentUiServer {
    pub url: String,
    _thread: JoinHandle<()>,
}

pub fn start_daemon_ui_server(
    extensions_dir: PathBuf,
    bind_addr: String,
    running: Arc<AtomicBool>,
) -> Result<PersistentUiServer, UiConfigError> {
    let listener = TcpListener::bind(&bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{}", local_addr);

    let thread_url = url.clone();
    let thread = std::thread::spawn(move || {
        while running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let state = match build_ui_state(&extensions_dir, None, false) {
                        Ok(state) => state,
                        Err(err) => {
                            eprintln!("failed to refresh UI state: {err}");
                            continue;
                        }
                    };
                    if let Err(err) = handle_connection(stream, &state) {
                        eprintln!("config UI request error: {err}");
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.raw_os_error() == Some(10035) =>
                {
                    std::thread::sleep(Duration::from_millis(30));
                }
                Err(err) => {
                    eprintln!("config UI server socket error on {thread_url}: {err}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });

    Ok(PersistentUiServer {
        url,
        _thread: thread,
    })
}

fn build_ui_state(
    extensions_dir: &Path,
    selected_extension_id: Option<&str>,
    allow_close: bool,
) -> Result<UiServerState, UiConfigError> {
    let registry = load_runtime_registry(extensions_dir)?;
    let mut descriptors = registry
        .list()
        .map(|extension| extension.descriptor.clone())
        .collect::<Vec<_>>();
    descriptors.sort_by(|a, b| a.id.cmp(&b.id));

    let extension_ids = descriptors
        .iter()
        .map(|descriptor| descriptor.id.clone())
        .collect::<HashSet<_>>();

    let selected_extension_id = if let Some(selected) = selected_extension_id {
        if !extension_ids.contains(selected) {
            return Err(UiConfigError::ExtensionNotFound(selected.to_string()));
        }
        selected.to_string()
    } else if extension_ids.contains("desktop-torrent-organizer") {
        "desktop-torrent-organizer".to_string()
    } else {
        descriptors
            .first()
            .map(|descriptor| descriptor.id.clone())
            .unwrap_or_default()
    };

    let data_root = copper_data_root()?;
    fs::create_dir_all(&data_root)?;

    Ok(UiServerState {
        selected_extension_id,
        descriptors,
        extension_ids,
        user_extensions_dir: extensions_dir.to_path_buf(),
        core_extensions_dir: core_extensions_dir(),
        runtime_extension_roots: runtime_extension_roots(extensions_dir),
        data_root,
        allow_close,
    })
}

pub fn open_extension_config(
    extensions_dir: &Path,
    extension_id: &str,
    options: UiOpenOptions,
) -> Result<String, UiConfigError> {
    let state = build_ui_state(extensions_dir, Some(extension_id), true)?;

    let listener = TcpListener::bind(&options.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{}", local_addr);

    if options.open_browser {
        open_in_browser(&url)?;
    }

    let mut should_stop = false;
    let mut last_activity = Instant::now();
    while !should_stop && last_activity.elapsed() < options.idle_timeout {
        match listener.accept() {
            Ok((stream, _)) => {
                last_activity = Instant::now();
                should_stop = handle_connection(stream, &state)?;
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.raw_os_error() == Some(10035) =>
            {
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(err) => return Err(UiConfigError::Io(err)),
        }
    }

    Ok(url)
}

fn handle_connection(mut stream: TcpStream, state: &UiServerState) -> Result<bool, UiConfigError> {
    let request = match parse_request(stream.try_clone()?) {
        Ok(request) => request,
        Err(err) => {
            let _ = write_response(&mut stream, HttpResponse::bad_request(err.to_string()));
            return Ok(false);
        }
    };

    let mut stop_after = false;
    let response = if request.method == HttpMethod::Get && request.path == "/" {
        HttpResponse::ok_html(render_html(state))
    } else if request.method == HttpMethod::Get && request.path == "/descriptor" {
        HttpResponse::ok_json(&serde_json::json!({
            "selectedExtensionId": state.selected_extension_id,
            "descriptors": state.descriptors,
        }))?
    } else if request.method == HttpMethod::Get && request.path == "/config/core" {
        HttpResponse::ok_json(&load_saved_config(
            &core_data_path_for(&state.data_root),
            Some(&legacy_data_path_for(&state.data_root, "copper-core")),
        )?)?
    } else if request.method == HttpMethod::Post && request.path == "/config/core" {
        match parse_json_object(&request.body) {
            Ok(value) => {
                store_config(&core_data_path_for(&state.data_root), &value)?;
                HttpResponse::ok_json(&serde_json::json!({ "ok": true }))?
            }
            Err(err) => HttpResponse::bad_request(err.to_string()),
        }
    } else if request.method == HttpMethod::Post && request.path == "/close" {
        if state.allow_close {
            stop_after = true;
            HttpResponse::no_content()
        } else {
            HttpResponse::bad_request("close is disabled for daemon-hosted UI")
        }
    } else if request.method == HttpMethod::Get && request.path == "/info/core" {
        HttpResponse::ok_json(&build_core_info(state))?
    } else if let Some(extension_id) = request.path.strip_prefix("/apply/extension/") {
        if request.method != HttpMethod::Post || !state.extension_ids.contains(extension_id) {
            HttpResponse::not_found()
        } else {
            let descriptor = state
                .descriptors
                .iter()
                .find(|descriptor| descriptor.id == extension_id)
                .ok_or_else(|| UiConfigError::ExtensionNotFound(extension_id.to_string()))?;
            match apply_extension_settings(state, descriptor) {
                Ok(result) => HttpResponse::ok_json(&result)?,
                Err(err) => HttpResponse::bad_request(err.to_string()),
            }
        }
    } else if let Some(extension_id) = request.path.strip_prefix("/info/extension/") {
        if !state.extension_ids.contains(extension_id) {
            HttpResponse::not_found()
        } else {
            let descriptor = state
                .descriptors
                .iter()
                .find(|descriptor| descriptor.id == extension_id)
                .ok_or_else(|| UiConfigError::ExtensionNotFound(extension_id.to_string()))?;
            HttpResponse::ok_json(&build_extension_info(state, descriptor)?)?
        }
    } else if let Some(extension_id) = request.path.strip_prefix("/config/extension/") {
        if !state.extension_ids.contains(extension_id) {
            HttpResponse::not_found()
        } else {
            let path = extension_config_path_for(&state.data_root, extension_id);
            match request.method {
                HttpMethod::Get => HttpResponse::ok_json(&load_saved_config(
                    &path,
                    Some(&legacy_data_path_for(&state.data_root, extension_id)),
                )?)?,
                HttpMethod::Post => match parse_json_object(&request.body) {
                    Ok(value) => {
                        store_config(&path, &value)?;
                        HttpResponse::ok_json(&serde_json::json!({ "ok": true }))?
                    }
                    Err(err) => HttpResponse::bad_request(err.to_string()),
                },
            }
        }
    } else {
        HttpResponse::not_found()
    };

    write_response(&mut stream, response)?;
    Ok(stop_after)
}

fn parse_json_object(raw: &[u8]) -> Result<Value, UiConfigError> {
    let parsed: Value = serde_json::from_slice(raw)
        .map_err(|e| UiConfigError::Request(format!("invalid JSON body: {e}")))?;
    if !parsed.is_object() {
        return Err(UiConfigError::Request(
            "config payload must be a JSON object".to_string(),
        ));
    }
    Ok(parsed)
}

fn parse_request(stream: TcpStream) -> Result<HttpRequest, UiConfigError> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    let bytes = reader.read_line(&mut first_line)?;
    if bytes == 0 {
        return Err(UiConfigError::Request("empty request".to_string()));
    }

    let mut parts = first_line.split_whitespace();
    let method = match parts.next() {
        Some("GET") => HttpMethod::Get,
        Some("POST") => HttpMethod::Post,
        Some(other) => {
            return Err(UiConfigError::Request(format!(
                "unsupported method '{other}'"
            )))
        }
        None => return Err(UiConfigError::Request("missing method".to_string())),
    };
    let path = parts
        .next()
        .ok_or_else(|| UiConfigError::Request("missing path".to_string()))?
        .to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                value.trim().trim_end_matches('\r').to_string(),
            );
        }
    }

    let body = if let Some(content_length) = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
    {
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body)?;
        }
        body
    } else if headers
        .get("transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        read_chunked_body(&mut reader)?
    } else {
        Vec::new()
    };

    Ok(HttpRequest { method, path, body })
}

fn read_chunked_body<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, UiConfigError> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        let read = reader.read_line(&mut size_line)?;
        if read == 0 {
            return Err(UiConfigError::Request(
                "unexpected EOF while reading chunk size".to_string(),
            ));
        }

        let size_hex = size_line
            .trim()
            .split(';')
            .next()
            .ok_or_else(|| UiConfigError::Request("invalid chunk header".to_string()))?;
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| UiConfigError::Request("invalid chunk size".to_string()))?;

        if size == 0 {
            loop {
                let mut trailer = String::new();
                let read = reader.read_line(&mut trailer)?;
                if read == 0 || trailer == "\r\n" {
                    break;
                }
            }
            break;
        }

        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk)?;
        body.extend_from_slice(&chunk);

        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
        if crlf != [b'\r', b'\n'] {
            return Err(UiConfigError::Request(
                "invalid chunk terminator".to_string(),
            ));
        }
    }
    Ok(body)
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> Result<(), UiConfigError> {
    let status_text = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status_text,
        response.content_type,
        response.body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !response.body.is_empty() {
        stream.write_all(&response.body)?;
    }
    stream.flush()?;
    Ok(())
}

fn copper_data_root() -> Result<PathBuf, UiConfigError> {
    let home = dirs::home_dir().ok_or_else(|| {
        UiConfigError::Request("cannot resolve home directory for extension storage".to_string())
    })?;
    Ok(home.join(".Copper").join("extensions"))
}

fn core_data_path_for(data_root: &Path) -> PathBuf {
    extension_config_path_for(data_root, "copper-core")
}

fn extension_config_path_for(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("config.json")
}

fn extension_status_path_for(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("status.json")
}

fn legacy_data_path_for(data_root: &Path, extension_id: &str) -> PathBuf {
    data_root.join(extension_id).join("data.json")
}

fn load_config(path: &Path) -> Result<Value, UiConfigError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let raw = fs::read_to_string(path)?;
    let parsed: Value = serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
    Ok(if parsed.is_object() {
        parsed
    } else {
        serde_json::json!({})
    })
}

fn store_config(path: &Path, value: &Value) -> Result<(), UiConfigError> {
    let mut merged = load_config(path)?;
    if let (Some(target), Some(source)) = (merged.as_object_mut(), value.as_object()) {
        let mut remove_keys = Vec::new();
        if let Some(remove) = source.get("__remove").and_then(|v| v.as_array()) {
            for item in remove {
                if let Some(key) = item.as_str() {
                    remove_keys.push(key.to_string());
                }
            }
        }

        for (key, item) in source {
            if key == "__remove" {
                continue;
            }
            target.insert(key.clone(), item.clone());
        }
        for key in remove_keys {
            target.remove(&key);
        }
    } else {
        merged = value.clone();
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&merged)?)?;
    Ok(())
}

fn build_core_info(state: &UiServerState) -> Value {
    serde_json::json!({
        "selectedExtensionId": state.selected_extension_id,
        "extensionsLoaded": state.descriptors.len(),
        "userExtensionsDir": state.user_extensions_dir.display().to_string(),
        "coreExtensionsDir": state
            .core_extensions_dir
            .as_ref()
            .map(|path| path.display().to_string()),
        "runtimeExtensionRoots": state
            .runtime_extension_roots
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "dataRoot": state.data_root.display().to_string(),
        "coreDataPath": core_data_path_for(&state.data_root).display().to_string()
    })
}

fn load_saved_config(path: &Path, legacy_path: Option<&Path>) -> Result<Value, UiConfigError> {
    if path.exists() {
        return load_config(path);
    }
    if let Some(legacy_path) = legacy_path {
        if legacy_path.exists() {
            return load_config(legacy_path);
        }
    }
    Ok(serde_json::json!({}))
}

fn open_in_browser(url: &str) -> Result<(), UiConfigError> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .map_err(|e| UiConfigError::Browser(e.to_string()))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| UiConfigError::Browser(e.to_string()))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .status()
            .map_err(|e| UiConfigError::Browser(e.to_string()))?;
    }

    Ok(())
}

pub fn open_url_in_browser(url: &str) -> Result<(), UiConfigError> {
    open_in_browser(url)
}

fn build_extension_info(
    state: &UiServerState,
    descriptor: &Descriptor,
) -> Result<Value, UiConfigError> {
    let config = load_saved_config(
        &extension_config_path_for(&state.data_root, &descriptor.id),
        Some(&legacy_data_path_for(&state.data_root, &descriptor.id)),
    )?;
    let status = load_saved_config(
        &extension_status_path_for(&state.data_root, &descriptor.id),
        Some(&legacy_data_path_for(&state.data_root, &descriptor.id)),
    )?;
    let commands = descriptor
        .actions
        .iter()
        .map(|action| {
            serde_json::json!({
                "id": action.id,
                "label": action.label,
                "description": action.description,
            })
        })
        .collect::<Vec<_>>();
    let dynamic_options = if descriptor.id == "windows-display-manager" {
        build_windows_display_dynamic_options(&config)
    } else {
        serde_json::json!({})
    };

    Ok(serde_json::json!({
        "extensionId": descriptor.id,
        "name": descriptor.name,
        "status": status,
        "statusMeta": descriptor
            .settings
            .as_ref()
            .and_then(|settings| settings.status.clone()),
        "applyActions": descriptor
            .settings
            .as_ref()
            .map(|settings| settings.apply_actions.clone())
            .unwrap_or_default(),
        "commands": commands,
        "dynamicOptions": dynamic_options,
    }))
}

fn apply_extension_settings(
    state: &UiServerState,
    descriptor: &Descriptor,
) -> Result<Value, UiConfigError> {
    let apply_actions = descriptor
        .settings
        .as_ref()
        .map(|settings| settings.apply_actions.clone())
        .unwrap_or_default();
    if apply_actions.is_empty() {
        return Err(UiConfigError::Request(format!(
            "extension '{}' does not declare settings apply actions",
            descriptor.id
        )));
    }

    let mut executions = Vec::new();
    match descriptor.id.as_str() {
        "windows-display-manager" => {
            for action_id in &apply_actions {
                let execution = execute_windows_display_action_in(&state.data_root, action_id)
                    .map_err(UiConfigError::Io)?;
                executions.push(serde_json::json!({
                    "actionId": action_id,
                    "result": execution,
                }));
            }
        }
        _ => {
            return Err(UiConfigError::Request(format!(
                "extension '{}' does not support settings apply actions in the host",
                descriptor.id
            )));
        }
    }

    let status = load_saved_config(
        &extension_status_path_for(&state.data_root, &descriptor.id),
        Some(&legacy_data_path_for(&state.data_root, &descriptor.id)),
    )?;
    Ok(serde_json::json!({
        "ok": true,
        "extensionId": descriptor.id,
        "executions": executions,
        "status": status,
    }))
}

fn build_windows_display_dynamic_options(config: &Value) -> Value {
    let status = crate::api::windows_display::execute_action("status", config).ok();
    let presets = status
        .as_ref()
        .and_then(|value| value.get("resolution"))
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
    serde_json::json!({
        "trayResolutionPresets": presets
    })
}

fn render_html(state: &UiServerState) -> String {
    let model = serde_json::json!({
        "selectedExtensionId": state.selected_extension_id,
        "descriptors": state.descriptors,
        "allowClose": state.allow_close,
    });
    let model_inline = serde_json::to_string(&model).unwrap_or_else(|_| "{}".to_string());

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Copper Settings</title>
  <style>
    :root {{
      --bg:#14161a; --panel:#1d2026; --panel2:#181b20; --panel3:#111319; --line:#303540; --text:#e6e9ef; --muted:#9aa3b2; --accent:#7aa2f7; --accent-soft:rgba(122,162,247,.14);
    }}
    * {{ box-sizing:border-box; }}
    body {{ margin:0; background:var(--bg); color:var(--text); font-family:Segoe UI, Arial, sans-serif; }}
    .layout {{ display:grid; grid-template-columns:280px 1fr; min-height:100vh; }}
    .sidebar {{ background:var(--panel2); border-right:1px solid var(--line); padding:18px 14px; }}
    .main {{ padding:28px; max-width:980px; width:100%; }}
    .title {{ font-size:18px; font-weight:700; margin:4px 0 4px; }}
    .subtitle {{ color:var(--muted); font-size:13px; margin:0 0 18px; }}
    .nav-btn {{
      width:100%; text-align:left; border:1px solid var(--line); background:transparent; color:var(--text);
      padding:12px; margin-bottom:8px; border-radius:10px; cursor:pointer;
    }}
    .nav-btn.active {{ background:var(--accent-soft); border-color:var(--accent); }}
    .nav-name {{ display:block; font-weight:600; }}
    .nav-meta {{ display:block; color:var(--muted); font-size:12px; margin-top:3px; }}
    .page-eyebrow {{ color:var(--muted); text-transform:uppercase; letter-spacing:.08em; font-size:12px; margin-bottom:10px; }}
    .page-title {{ font-size:30px; line-height:1.15; margin:0 0 8px; }}
    .page-sub {{ color:var(--muted); margin:0 0 18px; max-width:720px; }}
    .tab-row {{ display:flex; gap:8px; margin:0 0 20px; flex-wrap:wrap; }}
    .tab-btn {{
      border:1px solid var(--line); border-radius:999px; background:transparent; color:var(--muted);
      padding:8px 14px; cursor:pointer; font-size:13px; font-weight:600;
    }}
    .tab-btn.active {{ color:var(--text); border-color:var(--accent); background:var(--accent-soft); }}
    .card {{ background:var(--panel); border:1px solid var(--line); border-radius:14px; padding:18px; margin-bottom:14px; }}
    .card-title {{ font-size:18px; font-weight:700; margin:0 0 6px; }}
    .card-sub {{ color:var(--muted); margin:0 0 14px; font-size:14px; }}
    label {{ display:block; font-weight:600; margin:10px 0 6px; }}
    .field-help {{ color:var(--muted); font-size:13px; margin:0 0 8px; }}
    input, select {{
      width:100%; border:1px solid var(--line); border-radius:10px; background:var(--panel3); color:var(--text);
      padding:10px;
    }}
    .btn-row {{ display:flex; gap:10px; margin-top:16px; flex-wrap:wrap; }}
    button {{
      border:1px solid var(--line); border-radius:10px; background:var(--panel3); color:var(--text); padding:10px 14px; cursor:pointer;
    }}
    button.primary {{ background:var(--accent); border-color:transparent; color:#0b1020; font-weight:700; }}
    button[hidden] {{ display:none; }}
    .status-msg {{ color:var(--muted); margin-top:8px; min-height:20px; }}
    .empty {{ color:var(--muted); font-style:italic; }}
    .kv-list {{ display:grid; grid-template-columns:minmax(180px, 240px) 1fr; gap:10px 16px; }}
    .kv-key {{ color:var(--muted); }}
    .kv-value {{ word-break:break-word; }}
    .command-list {{ display:grid; gap:10px; }}
    .command-item {{ border:1px solid var(--line); border-radius:12px; padding:12px; background:var(--panel3); }}
    .command-title {{ font-weight:700; margin:0 0 4px; }}
    .command-meta {{ color:var(--muted); font-size:12px; margin:0 0 6px; }}
    .command-desc {{ color:var(--text); margin:0; font-size:14px; }}
    .mono {{ font-family:Consolas, monospace; }}
    .checkbox-list {{ display:grid; gap:8px; }}
    .checkbox-item {{
      display:flex; gap:10px; align-items:flex-start; padding:10px 12px; border:1px solid var(--line);
      border-radius:10px; background:var(--panel3);
    }}
    .checkbox-item input {{ width:auto; margin-top:3px; }}
    @media (max-width: 900px) {{
      .layout {{ grid-template-columns:1fr; }}
      .sidebar {{ border-right:none; border-bottom:1px solid var(--line); }}
      .main {{ padding:20px; }}
      .kv-list {{ grid-template-columns:1fr; }}
    }}
  </style>
</head>
<body>
  <div class="layout">
    <aside class="sidebar">
      <div class="title">Settings</div>
      <p class="subtitle">Per-extension pages with separate status.</p>
      <div id="nav"></div>
    </aside>
    <main class="main">
      <div class="page-eyebrow" id="pageEyebrow">Extension</div>
      <h1 class="page-title" id="pageTitle">Copper</h1>
      <p class="page-sub" id="pageSub">Core Copper configuration</p>
      <div class="tab-row" id="tabs"></div>
      <section id="settingsView"></section>
      <section id="statusView" hidden></section>
      <div class="btn-row">
        <button class="primary" id="saveBtn">Save settings</button>
        <button id="closeBtn">Close UI Server</button>
      </div>
      <div class="status-msg" id="statusMsg"></div>
    </main>
  </div>

  <script>
    const model = {model_inline};
    const descriptors = model.descriptors || [];
    const byId = Object.fromEntries(descriptors.map(d => [d.id, d]));
    let currentSection = (() => {{
      const defaultSection = model.selectedExtensionId ? `ext:${{model.selectedExtensionId}}` : 'core';
      const params = new URLSearchParams(window.location.search);
      const requested = params.get('section');
      if (!requested) {{
        return defaultSection;
      }}
      if (requested === 'core') {{
        return 'core';
      }}
      if (requested.startsWith('ext:')) {{
        const id = requested.slice(4);
        if (byId[id]) {{
          return requested;
        }}
      }}
      return defaultSection;
    }})();
    let currentTab = (() => {{
      const params = new URLSearchParams(window.location.search);
      return params.get('tab') === 'status' ? 'status' : 'settings';
    }})();

    const navEl = document.getElementById('nav');
    const settingsViewEl = document.getElementById('settingsView');
    const statusViewEl = document.getElementById('statusView');
    const statusMsgEl = document.getElementById('statusMsg');
    const closeBtn = document.getElementById('closeBtn');
    const pageEyebrowEl = document.getElementById('pageEyebrow');
    const pageTitleEl = document.getElementById('pageTitle');
    const pageSubEl = document.getElementById('pageSub');
    const tabsEl = document.getElementById('tabs');
    const saveBtn = document.getElementById('saveBtn');

    function currentDescriptor() {{
      return currentSection.startsWith('ext:') ? byId[currentSection.slice(4)] : null;
    }}

    if (!model.allowClose && closeBtn) {{
      closeBtn.style.display = 'none';
    }}

    function setStatus(msg) {{
      statusMsgEl.textContent = msg;
    }}

    function updateUrl() {{
      const params = new URLSearchParams();
      params.set('section', currentSection);
      params.set('tab', currentTab);
      window.history.replaceState(null, '', `${{window.location.pathname}}?${{params.toString()}}`);
    }}

    function humanizeKey(value) {{
      return String(value || '')
        .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
        .replace(/[-_]/g, ' ')
        .replace(/^./, ch => ch.toUpperCase());
    }}

    function formatValue(value, format) {{
      if (value === null || value === undefined || value === '') return 'Not set';
      if (format === 'date-time' && typeof value === 'number') {{
        return new Date(value * 1000).toLocaleString();
      }}
      if (typeof value === 'boolean') return value ? 'Enabled' : 'Disabled';
      if (typeof value === 'object') return JSON.stringify(value);
      return String(value);
    }}

    function createCard(title, description) {{
      const card = document.createElement('section');
      card.className = 'card';
      const heading = document.createElement('h2');
      heading.className = 'card-title';
      heading.textContent = title;
      card.appendChild(heading);
      if (description) {{
        const desc = document.createElement('p');
        desc.className = 'card-sub';
        desc.textContent = description;
        card.appendChild(desc);
      }}
      return card;
    }}

    function getByPath(target, path) {{
      return String(path || '')
        .split('.')
        .filter(Boolean)
        .reduce((value, key) => (value && value[key] !== undefined ? value[key] : undefined), target);
    }}

    function resolveInputOptions(input, info) {{
      const inline = Array.isArray(input.options) ? input.options : [];
      if (inline.length > 0) return inline.map(String);
      const sourced = input.optionsSource ? getByPath(info, input.optionsSource) : undefined;
      return Array.isArray(sourced) ? sourced.map(String) : [];
    }}

    function createNavButton(key, label) {{
      const btn = document.createElement('button');
      btn.className = 'nav-btn' + (key === currentSection ? ' active' : '');
      const isCore = key === 'core';
      btn.innerHTML = `<span class="nav-name">${{label}}</span><span class="nav-meta">${{isCore ? 'Core settings' : 'Extension settings'}}</span>`;
      btn.addEventListener('click', () => {{
        currentSection = key;
        currentTab = 'settings';
        updateUrl();
        renderNav();
        renderSection().catch(err => setStatus('Load failed: ' + err));
      }});
      return btn;
    }}

    function renderNav() {{
      navEl.innerHTML = '';
      navEl.appendChild(createNavButton('core', 'Copper'));
      descriptors.forEach(d => navEl.appendChild(createNavButton(`ext:${{d.id}}`, d.name)));
    }}

    function createInput(input, value, info) {{
      const wrapper = document.createElement('div');
      const label = document.createElement('label');
      label.textContent = input.label;
      wrapper.appendChild(label);
      if (input.description) {{
        const help = document.createElement('div');
        help.className = 'field-help';
        help.textContent = input.description;
        wrapper.appendChild(help);
      }}

      let control;
      if (input.type === 'boolean') {{
        control = document.createElement('select');
        control.innerHTML = '<option value="true">Enabled</option><option value="false">Disabled</option>';
        control.value = String(value ?? input.default ?? false);
      }} else if (input.type === 'multi-select') {{
        const options = resolveInputOptions(input, info);
        const selected = Array.isArray(value)
          ? value.map(String)
          : Array.isArray(input.default)
            ? input.default.map(String)
            : [];
        const selectedSet = new Set(selected);
        control = document.createElement('div');
        control.className = 'checkbox-list';
        if (options.length === 0) {{
          const empty = document.createElement('div');
          empty.className = 'empty';
          empty.textContent = 'No options available right now.';
          control.appendChild(empty);
        }} else {{
          options.forEach(opt => {{
            const item = document.createElement('label');
            item.className = 'checkbox-item';
            const checkbox = document.createElement('input');
            checkbox.type = 'checkbox';
            checkbox.checked = selectedSet.has(opt);
            checkbox.dataset.inputId = input.id;
            checkbox.dataset.inputType = input.type;
            checkbox.dataset.optionValue = opt;
            const text = document.createElement('span');
            text.textContent = opt;
            item.appendChild(checkbox);
            item.appendChild(text);
            control.appendChild(item);
          }});
        }}
      }} else if (input.type === 'select') {{
        control = document.createElement('select');
        resolveInputOptions(input, info).forEach(opt => {{
          const o = document.createElement('option');
          o.value = opt;
          o.textContent = opt;
          control.appendChild(o);
        }});
        if (value !== undefined && value !== null) control.value = String(value);
      }} else {{
        control = document.createElement('input');
        control.type = (input.type === 'number') ? 'number' : 'text';
        control.value = String(value ?? input.default ?? '');
      }}

      if (input.type !== 'multi-select') {{
        control.dataset.inputId = input.id;
        control.dataset.inputType = input.type;
      }}
      wrapper.appendChild(control);
      return wrapper;
    }}

    function inferSections(inputs, descriptor) {{
      const byInputId = Object.fromEntries((inputs || []).map(input => [input.id, input]));
      const declared = (((descriptor || {{}}).settings || {{}}).sections || []);
      const used = new Set();
      const sections = [];

      declared.forEach(section => {{
        const inputDefs = (section.inputs || []).map(id => byInputId[id]).filter(Boolean);
        inputDefs.forEach(input => used.add(input.id));
        if (inputDefs.length > 0) {{
          sections.push({{
            id: section.id,
            title: section.title,
            description: section.description || '',
            inputDefs
          }});
        }}
      }});

      const remaining = (inputs || []).filter(input => !used.has(input.id));
      if (sections.length === 0 && remaining.length > 0) {{
        return [{{ id: 'settings', title: 'Settings', description: '', inputDefs: remaining }}];
      }}
      if (remaining.length > 0) {{
        sections.push({{ id: 'other', title: 'Other settings', description: '', inputDefs: remaining }});
      }}
      return sections;
    }}

    function renderKeyValueCard(target, title, description, rows) {{
      const card = createCard(title, description);
      if (!rows.length) {{
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'Nothing to show yet.';
        card.appendChild(empty);
      }} else {{
        const list = document.createElement('div');
        list.className = 'kv-list';
        rows.forEach(row => {{
          const keyEl = document.createElement('div');
          keyEl.className = 'kv-key';
          keyEl.textContent = row.label;
          const valueEl = document.createElement('div');
          valueEl.className = 'kv-value' + (row.format === 'path' || row.mono ? ' mono' : '');
          valueEl.textContent = formatValue(row.value, row.format);
          list.appendChild(keyEl);
          list.appendChild(valueEl);
        }});
        card.appendChild(list);
      }}
      target.appendChild(card);
    }}

    function renderCommands(target, commands) {{
      const card = createCard('Commands', 'Manual operations exposed by this extension.');
      if (!commands.length) {{
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'This extension does not expose any manual commands.';
        card.appendChild(empty);
      }} else {{
        const list = document.createElement('div');
        list.className = 'command-list';
        commands.forEach(command => {{
          const item = document.createElement('div');
          item.className = 'command-item';
          item.innerHTML =
            `<div class="command-title">${{command.label || command.id}}</div>` +
            `<div class="command-meta mono">${{command.id}}</div>` +
            `<p class="command-desc">${{command.description || 'No description provided.'}}</p>`;
          list.appendChild(item);
        }});
        card.appendChild(list);
      }}
      target.appendChild(card);
    }}

    function renderTabs() {{
      tabsEl.innerHTML = '';
      [['settings', 'Settings'], ['status', 'Status']].forEach(([id, label]) => {{
        const btn = document.createElement('button');
        btn.className = 'tab-btn' + (currentTab === id ? ' active' : '');
        btn.textContent = label;
        btn.addEventListener('click', () => {{
          currentTab = id;
          updateUrl();
          renderTabs();
          toggleViews();
        }});
        tabsEl.appendChild(btn);
      }});
    }}

    function toggleViews() {{
      const showSettings = currentTab === 'settings';
      settingsViewEl.hidden = !showSettings;
      statusViewEl.hidden = showSettings;
      saveBtn.hidden = !showSettings;
    }}

    async function loadJson(url) {{
      const res = await fetch(url);
      if (!res.ok) {{
        throw new Error((await res.text()) || ('HTTP ' + res.status));
      }}
      return await res.json();
    }}

    async function renderSection() {{
      settingsViewEl.innerHTML = '';
      statusViewEl.innerHTML = '';
      setStatus('');
      renderTabs();
      toggleViews();

      function coreFieldDefs() {{
        return [
          {{
            id: 'userExtensionsDir',
            label: 'User extensions directory',
            description: 'Folder where user-installed extensions are discovered.',
            type: 'text',
            default: '~/.Copper/extensions'
          }},
          {{
            id: 'uiTheme',
            label: 'UI theme',
            description: 'Name of the preferred host settings theme.',
            type: 'text',
            default: 'obsidian'
          }},
          {{
            id: 'startupExtension',
            label: 'Startup extension id',
            description: 'Extension page selected when the settings UI opens.',
            type: 'text',
            default: model.selectedExtensionId || ''
          }}
        ];
      }}

      const configTarget = currentSection === 'core'
        ? '/config/core'
        : '/config/extension/' + encodeURIComponent(currentSection.slice(4));
      const infoTarget = currentSection === 'core'
        ? '/info/core'
        : '/info/extension/' + encodeURIComponent(currentSection.slice(4));
      const [config, info] = await Promise.all([loadJson(configTarget), loadJson(infoTarget)]);

      if (currentSection === 'core') {{
        pageEyebrowEl.textContent = 'Core';
        pageTitleEl.textContent = 'Copper';
        pageSubEl.textContent = 'Application-wide settings stay separate from extension settings.';
        saveBtn.textContent = 'Save settings';

        const settingsCard = createCard('Settings', 'Core Copper configuration.');
        coreFieldDefs().forEach(field => {{
          settingsCard.appendChild(createInput(field, config[field.id], info));
        }});
        settingsViewEl.appendChild(settingsCard);

        const coreRows = [
          {{ label: 'Selected extension', value: info.selectedExtensionId || 'Not set' }},
          {{ label: 'Extensions loaded', value: info.extensionsLoaded ?? 0 }},
          {{ label: 'User extensions directory', value: info.userExtensionsDir, format: 'path', mono: true }},
          {{ label: 'Core extensions directory', value: info.coreExtensionsDir || 'Not available', format: 'path', mono: true }},
          {{ label: 'Runtime extension roots', value: (info.runtimeExtensionRoots || []).join(', '), format: 'path', mono: true }}
        ];
        renderKeyValueCard(statusViewEl, 'Status', 'Current Copper environment information.', coreRows);
        return;
      }}

      const extensionId = currentSection.slice(4);
      const descriptor = byId[extensionId];
      if (!descriptor) {{
        throw new Error('Unknown extension section: ' + extensionId);
      }}

      const settingsMeta = descriptor.settings || {{}};
      const applyActions = Array.isArray(settingsMeta.applyActions) ? settingsMeta.applyActions : [];
      pageEyebrowEl.textContent = 'Extension';
      pageTitleEl.textContent = settingsMeta.title || descriptor.name;
      pageSubEl.textContent =
        settingsMeta.description ||
        'Configure this extension on one page and review its latest runtime status separately.';
      saveBtn.textContent = applyActions.length > 0 ? 'Save and apply' : 'Save settings';

      const sections = inferSections(descriptor.inputs || [], descriptor);
      if (sections.length === 0) {{
        const emptyCard = createCard('Settings', 'This extension does not expose editable settings yet.');
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'No configurable fields were declared in the manifest.';
        emptyCard.appendChild(empty);
        settingsViewEl.appendChild(emptyCard);
      }} else {{
        sections.forEach(section => {{
          const card = createCard(section.title, section.description);
          section.inputDefs.forEach(input => {{
            card.appendChild(createInput(input, config[input.id], info));
          }});
          settingsViewEl.appendChild(card);
        }});
      }}

      const statusMeta = (info && info.statusMeta) || ((settingsMeta || {{}}).status) || {{}};
      const status = (info && info.status) || {{}};
      const fieldDefs = (statusMeta.fields && statusMeta.fields.length)
        ? statusMeta.fields
        : Object.keys(status).sort().map(key => ({{ key, label: humanizeKey(key) }}));
      const statusRows = fieldDefs.map(field => ({{
        label: field.label || humanizeKey(field.key),
        value: status[field.key],
        format: field.format
      }}));
      renderKeyValueCard(
        statusViewEl,
        (statusMeta && statusMeta.title) || 'Recent status',
        (statusMeta && statusMeta.description) || 'Latest runtime values reported by the daemon for this extension.',
        statusRows
      );
      renderCommands(statusViewEl, (info && info.commands) || []);
    }}

    function collectCurrentPayload() {{
      const payload = {{}};
      const remove = [];
      const sameValue = (a, b) => JSON.stringify(a) === JSON.stringify(b);
      const addKey = (id, value, defaultValue) => {{
        const isEmpty = value === '' || value === null || value === undefined;
        if (sameValue(value, defaultValue) || (defaultValue === undefined && isEmpty)) {{
          remove.push(id);
        }} else {{
          payload[id] = value;
        }}
      }};

      if (!currentSection.startsWith('ext:')) {{
        const coreDefaults = {{
          userExtensionsDir: '~/.Copper/extensions',
          uiTheme: 'obsidian',
          startupExtension: model.selectedExtensionId || ''
        }};
        const controls = settingsViewEl.querySelectorAll('[data-input-id]');
        const handled = new Set();
        controls.forEach(ctrl => {{
          const id = ctrl.dataset.inputId;
          if (handled.has(id)) return;
          handled.add(id);
          const type = ctrl.dataset.inputType;
          let value;
          if (type === 'boolean') {{
            value = ctrl.value === 'true';
          }} else if (type === 'multi-select') {{
            value = Array.from(settingsViewEl.querySelectorAll(`[data-input-id="${{id}}"][data-input-type="multi-select"]`))
              .filter(option => option.checked)
              .map(option => option.dataset.optionValue);
          }} else if (type === 'number') {{
            value = ctrl.value === '' ? null : Number(ctrl.value);
          }} else {{
            value = ctrl.value;
          }}
          addKey(id, value, coreDefaults[id]);
        }});
        if (remove.length > 0) payload.__remove = remove;
        return payload;
      }}

      const controls = settingsViewEl.querySelectorAll('[data-input-id]');
      const extensionId = currentSection.slice(4);
      const descriptor = byId[extensionId];
      const inputDefaults = {{}};
      (descriptor && descriptor.inputs ? descriptor.inputs : []).forEach(input => {{
        inputDefaults[input.id] = input.default;
      }});
      const handled = new Set();
      controls.forEach(ctrl => {{
        const id = ctrl.dataset.inputId;
        if (handled.has(id)) return;
        handled.add(id);
        const type = ctrl.dataset.inputType;
        let value;
        if (type === 'boolean') {{
          value = ctrl.value === 'true';
        }} else if (type === 'multi-select') {{
          value = Array.from(settingsViewEl.querySelectorAll(`[data-input-id="${{id}}"][data-input-type="multi-select"]`))
            .filter(option => option.checked)
            .map(option => option.dataset.optionValue);
        }} else if (type === 'number') {{
          value = ctrl.value === '' ? null : Number(ctrl.value);
        }} else {{
          value = ctrl.value;
        }}
        addKey(id, value, inputDefaults[id]);
      }});
      if (remove.length > 0) payload.__remove = remove;
      return payload;
    }}

    saveBtn.addEventListener('click', async () => {{
      try {{
        const payload = collectCurrentPayload();
        const descriptor = currentDescriptor();
        const applyActions = Array.isArray(descriptor && descriptor.settings && descriptor.settings.applyActions)
          ? descriptor.settings.applyActions
          : [];
        const target = currentSection === 'core'
          ? '/config/core'
          : '/config/extension/' + encodeURIComponent(currentSection.slice(4));
        const res = await fetch(target, {{
          method: 'POST',
          headers: {{ 'content-type': 'application/json' }},
          body: JSON.stringify(payload)
        }});
        if (!res.ok) {{
          throw new Error(await res.text());
        }}
        if (descriptor && applyActions.length > 0) {{
          const applyRes = await fetch(
            '/apply/extension/' + encodeURIComponent(descriptor.id),
            {{ method: 'POST' }}
          );
          if (!applyRes.ok) {{
            throw new Error('settings were saved, but apply failed: ' + await applyRes.text());
          }}
          await renderSection();
          setStatus('Settings saved and applied to the current system.');
        }} else {{
          await renderSection();
          setStatus('Settings saved successfully.');
        }}
      }} catch (err) {{
        setStatus('Save failed: ' + err);
      }}
    }});

    if (closeBtn) {{
      closeBtn.addEventListener('click', async () => {{
        try {{
          await fetch('/close', {{ method: 'POST' }});
          setStatus('UI server closed. You can close this tab.');
        }} catch (err) {{
          setStatus('Close failed: ' + err);
        }}
      }});
    }}

    updateUrl();
    renderNav();
    renderSection().catch(err => setStatus('Load failed: ' + err));
  </script>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_core_info, build_ui_state, core_data_path_for, extension_config_path_for,
        extension_status_path_for, load_config, parse_json_object, parse_request,
        read_chunked_body, render_html, start_daemon_ui_server, store_config, write_response,
        HttpMethod, HttpResponse, UiConfigError, UiOpenOptions,
    };
    use crate::descriptor::{
        Action, Descriptor, InputField, InputType, SettingsDescriptor, SettingsSection,
        StatusDescriptor, StatusField, StatusFieldFormat, UiDescriptor,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::io::{BufReader, Cursor, Read};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn sample_descriptor() -> Descriptor {
        Descriptor {
            schema: Some(
                "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json".to_string(),
            ),
            id: "desktop-torrent-organizer".to_string(),
            name: "Desktop Torrent Organizer".to_string(),
            version: "1.0.0".to_string(),
            trigger: "desktop-torrents".to_string(),
            permissions: vec![],
            inputs: vec![InputField {
                id: "desktopFolder".to_string(),
                field_type: InputType::FolderPicker,
                label: "Desktop folder".to_string(),
                description: Some("Folder that Copper scans for incoming .torrent files.".to_string()),
                default: serde_json::json!("~/Desktop"),
                options: vec![],
                options_source: None,
            }],
            actions: vec![Action {
                id: "move-torrents".to_string(),
                label: "Move .torrent files".to_string(),
                description: Some("Run the organizer immediately using the saved settings.".to_string()),
                script: "Move .torrent files".to_string(),
            }],
            ui: Some(UiDescriptor {
                ui_type: "form".to_string(),
                source: None,
                on_select: None,
            }),
            settings: Some(SettingsDescriptor {
                title: Some("Desktop Torrent Organizer".to_string()),
                description: Some(
                    "Configure how Copper watches the desktop and review the latest monitor status."
                        .to_string(),
                ),
                apply_actions: vec![],
                sections: vec![SettingsSection {
                    id: "monitor".to_string(),
                    title: "Monitor".to_string(),
                    description: Some(
                        "Settings used by the background desktop torrent watcher.".to_string(),
                    ),
                    inputs: vec!["desktopFolder".to_string()],
                }],
                status: Some(StatusDescriptor {
                    title: Some("Current status".to_string()),
                    description: Some(
                        "Latest runtime values reported by the daemon.".to_string(),
                    ),
                    fields: vec![StatusField {
                        key: "lastScanUnix".to_string(),
                        label: "Last scan".to_string(),
                        format: Some(StatusFieldFormat::DateTime),
                    }],
                }),
            }),
            tray: None,
        }
    }

    fn sample_state() -> super::UiServerState {
        let descriptor = sample_descriptor();
        super::UiServerState {
            selected_extension_id: descriptor.id.clone(),
            extension_ids: [descriptor.id.clone()].into_iter().collect::<HashSet<_>>(),
            descriptors: vec![descriptor],
            user_extensions_dir: PathBuf::from("C:/tmp/copper-user"),
            core_extensions_dir: Some(PathBuf::from("C:/tmp/copper-core")),
            runtime_extension_roots: vec![
                PathBuf::from("C:/tmp/copper-core"),
                PathBuf::from("C:/tmp/copper-user"),
            ],
            data_root: PathBuf::from("C:/tmp/copper-user"),
            allow_close: true,
        }
    }

    #[test]
    fn ui_open_options_default_values_are_stable() {
        let options = UiOpenOptions::default();
        assert_eq!(options.bind_addr, "127.0.0.1:0");
        assert!(options.open_browser);
        assert_eq!(options.idle_timeout, Duration::from_secs(300));
    }

    fn write_extension(root: &std::path::Path, descriptor: &Descriptor) {
        let ext = root.join(&descriptor.id);
        fs::create_dir_all(&ext).expect("create extension dir");
        let manifest = serde_json::json!({
            "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
            "id": descriptor.id,
            "name": descriptor.name,
            "version": descriptor.version,
            "trigger": descriptor.trigger,
            "permissions": [],
            "inputs": [{
                "id": "desktopFolder",
                "type": "folder-picker",
                "label": "Desktop folder",
                "default": "~/Desktop"
            }],
            "actions": [{
                "id": "move-torrents",
                "label": "Move .torrent files",
                "script": "return;"
            }],
            "ui": { "type": "form" }
        });
        fs::write(
            ext.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).expect("descriptor json"),
        )
        .expect("write manifest");
        fs::write(
            ext.join("main.ts"),
            "export default function(){ return { onTrigger(){ return {}; } }; }",
        )
        .expect("write main.ts");
    }

    fn parse_http_url(url: &str) -> String {
        url.strip_prefix("http://").expect("http url").to_string()
    }

    #[test]
    fn build_ui_state_rejects_unknown_selected_extension() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);
        let err =
            build_ui_state(temp.path(), Some("missing-extension"), true).expect_err("must fail");
        match err {
            UiConfigError::ExtensionNotFound(id) => assert_eq!(id, "missing-extension"),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn build_ui_state_selects_valid_default_extension() {
        let temp = tempdir().expect("tempdir");
        let mut descriptor = sample_descriptor();
        descriptor.id = "alpha-ext".to_string();
        descriptor.name = "Alpha Extension".to_string();
        write_extension(temp.path(), &descriptor);
        let state = build_ui_state(temp.path(), None, true).expect("build state");
        assert!(state.extension_ids.contains(&state.selected_extension_id));
        if state.extension_ids.contains("desktop-torrent-organizer") {
            assert_eq!(state.selected_extension_id, "desktop-torrent-organizer");
        } else {
            assert_eq!(state.selected_extension_id, "alpha-ext");
        }
    }

    fn http_request(addr: &str, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
        let payload = body.unwrap_or("");
        let mut stream = TcpStream::connect(addr).expect("connect");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        std::io::Write::write_all(&mut stream, request.as_bytes()).expect("send request");
        std::io::Write::flush(&mut stream).expect("flush request");

        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("read response");
        let mut lines = raw.lines();
        let status_line = lines.next().unwrap_or_default().to_string();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse::<u16>().ok())
            .expect("status code");
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
        (status, body)
    }

    #[test]
    fn render_html_contains_extension_pages_and_status_view() {
        let html = render_html(&sample_state());
        assert!(html.contains("Settings"));
        assert!(html.contains("Copper"));
        assert!(html.contains("Desktop Torrent Organizer"));
        assert!(html.contains("Save settings"));
        assert!(html.contains("Status"));
        assert!(html.contains("Commands"));
        assert!(html.contains("Recent status"));
        assert!(html.contains("URLSearchParams(window.location.search)"));
    }

    #[test]
    fn config_and_status_paths_are_separated() {
        let root = PathBuf::from("C:/tmp/copper-user");
        assert_eq!(
            core_data_path_for(&root),
            root.join("copper-core").join("config.json")
        );
        assert_eq!(
            extension_config_path_for(&root, "desktop-torrent-organizer"),
            root.join("desktop-torrent-organizer").join("config.json")
        );
        assert_eq!(
            extension_status_path_for(&root, "desktop-torrent-organizer"),
            root.join("desktop-torrent-organizer").join("status.json")
        );
    }

    #[test]
    fn core_info_includes_runtime_roots() {
        let state = sample_state();
        let info = build_core_info(&state);
        let roots = info
            .get("runtimeExtensionRoots")
            .and_then(|v| v.as_array())
            .expect("roots array");
        assert_eq!(roots.len(), 2);
        assert!(
            info.get("dataRoot").is_some(),
            "core info should include extension data root"
        );
    }

    #[test]
    fn render_html_hides_close_button_when_close_disabled() {
        let mut state = sample_state();
        state.allow_close = false;
        let html = render_html(&state);
        assert!(html.contains("if (!model.allowClose && closeBtn)"));
    }

    #[test]
    fn reads_chunked_body() {
        let raw = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let body = read_chunked_body(&mut reader).expect("chunked body");
        assert_eq!(body, b"Wikipedia");
    }

    #[test]
    fn rejects_invalid_chunk_size() {
        let raw = b"ZZ\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let err = read_chunked_body(&mut reader).expect_err("invalid chunk size");
        assert!(err.to_string().contains("invalid chunk size"));
    }

    #[test]
    fn store_config_merges_and_removes_keys_without_touching_status() {
        let temp = tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("desktop-torrent-organizer")
            .join("config.json");
        let status_path = temp
            .path()
            .join("desktop-torrent-organizer")
            .join("status.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(
            &path,
            r#"{
              "desktopFolder":"~/Desktop",
              "obsoleteAction":"move-torrents"
            }"#,
        )
        .expect("seed data");
        fs::write(&status_path, r#"{ "lastScanUnix": 1 }"#).expect("seed status");

        let update = serde_json::json!({
            "desktopFolder": "D:/Desktop",
            "__remove": ["obsoleteAction"]
        });
        store_config(&path, &update).expect("store config");

        let stored = load_config(&path).expect("load");
        assert_eq!(
            stored.get("desktopFolder").and_then(|v| v.as_str()),
            Some("D:/Desktop")
        );
        assert!(stored.get("obsoleteAction").is_none());

        let status = load_config(&status_path).expect("load status");
        assert_eq!(status.get("lastScanUnix").and_then(|v| v.as_u64()), Some(1));
    }

    fn parse_over_loopback(raw_request: &'static str) -> super::HttpRequest {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(&mut client, raw_request.as_bytes()).expect("write request");
        });

        let (stream, _) = listener.accept().expect("accept");
        let parsed = parse_request(stream).expect("parse request");
        sender.join().expect("join sender");
        parsed
    }

    #[test]
    fn parse_json_object_validates_top_level_type() {
        let err = parse_json_object(br#"["not","object"]"#).expect_err("must reject non-object");
        assert!(err.to_string().contains("JSON object"));

        let ok = parse_json_object(br#"{"enabled":true}"#).expect("object");
        assert_eq!(ok.get("enabled").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn parse_request_reads_content_length_body() {
        let request = parse_over_loopback(
            "POST /config/core HTTP/1.1\r\nHost: localhost\r\nContent-Length: 12\r\n\r\n{\"k\":\"v123\"}",
        );
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/config/core");
        assert_eq!(request.body, br#"{"k":"v123"}"#);
    }

    #[test]
    fn parse_request_reads_chunked_payload() {
        let request = parse_over_loopback(
            "POST /config/core HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n",
        );
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/config/core");
        assert_eq!(request.body, b"Wikipedia");
    }

    #[test]
    fn parse_request_rejects_unsupported_method() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(
                &mut client,
                b"PUT /config/core HTTP/1.1\r\nHost: localhost\r\n\r\n",
            )
            .expect("write");
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("unsupported method");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("unsupported method"));
    }

    #[test]
    fn parse_request_rejects_empty_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let stream = TcpStream::connect(addr).expect("connect");
            drop(stream);
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("empty request");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("empty request"));
    }

    #[test]
    fn parse_request_rejects_missing_path() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(&mut client, b"GET\r\n\r\n").expect("write");
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("missing path");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("missing path"));
    }

    #[test]
    fn parse_request_rejects_missing_method() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(&mut client, b"\r\n\r\n").expect("write");
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("missing method");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("missing method"));
    }

    #[test]
    fn read_chunked_body_rejects_unexpected_eof() {
        let raw = b"";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let err = read_chunked_body(&mut reader).expect_err("unexpected eof");
        assert!(err.to_string().contains("unexpected EOF"));
    }

    #[test]
    fn read_chunked_body_rejects_invalid_chunk_terminator() {
        let raw = b"1\r\naZZ0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let err = read_chunked_body(&mut reader).expect_err("invalid chunk terminator");
        assert!(err.to_string().contains("invalid chunk terminator"));
    }

    #[test]
    fn read_chunked_body_skips_trailers() {
        let raw = b"1\r\na\r\n0\r\nX-Test: 1\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let body = read_chunked_body(&mut reader).expect("chunked with trailer");
        assert_eq!(body, b"a");
    }

    #[test]
    fn load_config_sanitizes_invalid_and_non_object_payloads() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("bad.json");

        fs::write(&path, "[]").expect("write non-object");
        let non_object = load_config(&path).expect("read non-object");
        assert_eq!(non_object, serde_json::json!({}));

        fs::write(&path, "{this-is-not-json").expect("write invalid");
        let invalid = load_config(&path).expect("read invalid");
        assert_eq!(invalid, serde_json::json!({}));
    }

    #[test]
    fn load_config_returns_empty_for_missing_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("missing.json");
        let value = load_config(&path).expect("load missing");
        assert_eq!(value, serde_json::json!({}));
    }

    #[test]
    fn store_config_replaces_with_non_object_payload_and_creates_parent() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("nested").join("config.json");
        store_config(&path, &serde_json::json!("raw-string")).expect("store non-object");
        let raw = fs::read_to_string(&path).expect("read file");
        assert!(raw.contains("raw-string"));
    }

    #[test]
    fn write_response_uses_ok_text_for_unknown_status_code() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            let mut raw = String::new();
            stream.read_to_string(&mut raw).expect("read");
            raw
        });

        let (mut stream, _) = listener.accept().expect("accept");
        write_response(
            &mut stream,
            HttpResponse {
                status: 500,
                content_type: "text/plain; charset=utf-8",
                body: b"boom".to_vec(),
            },
        )
        .expect("write response");
        drop(stream);

        let raw = client.join().expect("join");
        assert!(raw.starts_with("HTTP/1.1 500 OK"));
    }

    #[test]
    fn daemon_ui_server_handles_core_and_extension_routes() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);

        let running = Arc::new(AtomicBool::new(true));
        let server = start_daemon_ui_server(
            temp.path().to_path_buf(),
            "127.0.0.1:0".to_string(),
            Arc::clone(&running),
        )
        .expect("start daemon ui");
        let addr = parse_http_url(&server.url);

        let (status_root, body_root) = http_request(&addr, "GET", "/", None);
        assert_eq!(status_root, 200);
        assert!(body_root.contains("Copper Settings"));

        let (status_descriptor, body_descriptor) = http_request(&addr, "GET", "/descriptor", None);
        assert_eq!(status_descriptor, 200);
        assert!(body_descriptor.contains("desktop-torrent-organizer"));

        let (status_core_get, body_core_get) = http_request(&addr, "GET", "/config/core", None);
        assert_eq!(status_core_get, 200);
        assert!(body_core_get.contains("{"));

        let (status_core_post, body_core_post) = http_request(
            &addr,
            "POST",
            "/config/core",
            Some(r#"{"uiTheme":"obsidian"}"#),
        );
        assert_eq!(status_core_post, 200);
        assert!(body_core_post.contains("\"ok\": true"));

        let (status_core_bad, body_core_bad) =
            http_request(&addr, "POST", "/config/core", Some(r#"["not-object"]"#));
        assert_eq!(status_core_bad, 400);
        assert!(body_core_bad.contains("JSON object"));

        let (status_info_core, body_info_core) = http_request(&addr, "GET", "/info/core", None);
        assert_eq!(status_info_core, 200);
        assert!(body_info_core.contains("runtimeExtensionRoots"));

        let ext_path = "/config/extension/desktop-torrent-organizer";
        let (status_ext_get, _) = http_request(&addr, "GET", ext_path, None);
        assert_eq!(status_ext_get, 200);

        let (status_ext_post, body_ext_post) = http_request(
            &addr,
            "POST",
            ext_path,
            Some(r#"{"desktopFolder":"D:/Desktop"}"#),
        );
        assert_eq!(status_ext_post, 200);
        assert!(body_ext_post.contains("\"ok\": true"));

        let (status_info_ext, body_info_ext) = http_request(
            &addr,
            "GET",
            "/info/extension/desktop-torrent-organizer",
            None,
        );
        assert_eq!(status_info_ext, 200);
        assert!(body_info_ext.contains("\"commands\""));

        let (status_missing_ext_get, _) =
            http_request(&addr, "GET", "/config/extension/missing", None);
        assert_eq!(status_missing_ext_get, 404);

        let (status_missing_ext_post, _) = http_request(
            &addr,
            "POST",
            "/config/extension/missing",
            Some(r#"{"x":1}"#),
        );
        assert_eq!(status_missing_ext_post, 404);

        let (status_close, body_close) = http_request(&addr, "POST", "/close", Some("{}"));
        assert_eq!(status_close, 400);
        assert!(body_close.contains("close is disabled"));

        let (status_not_found, _) = http_request(&addr, "GET", "/does-not-exist", None);
        assert_eq!(status_not_found, 404);

        running.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(80));
    }

    #[test]
    fn open_extension_config_closes_on_close_route() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let addr = listener.local_addr().expect("local addr");
        drop(listener);
        let bind = format!("127.0.0.1:{}", addr.port());

        let extensions_dir = temp.path().to_path_buf();
        let bind_for_thread = bind.clone();
        let handle = thread::spawn(move || {
            super::open_extension_config(
                &extensions_dir,
                "desktop-torrent-organizer",
                UiOpenOptions {
                    bind_addr: bind_for_thread,
                    open_browser: false,
                    idle_timeout: Duration::from_secs(2),
                },
            )
        });

        let mut up = false;
        for _ in 0..20 {
            if TcpStream::connect(&bind).is_ok() {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(up, "config UI should accept connections");

        let (status_close, _) = http_request(&bind, "POST", "/close", Some("{}"));
        assert_eq!(status_close, 204);

        let url = handle
            .join()
            .expect("join")
            .expect("open extension config should succeed");
        assert!(url.starts_with("http://127.0.0.1:"));
    }
}

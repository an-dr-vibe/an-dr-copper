use super::browser::open_in_browser;
use super::render::render_html;
use super::window::open_in_native_window;
use super::{PersistentUiServer, UiConfigError, UiOpenOptions, UiServerState};
use crate::config_ui_http::{parse_request, write_response, HttpMethod, HttpRequest, HttpResponse};
use crate::config_ui_service::{apply_extension_settings, build_core_info, build_extension_info};
use crate::control_plane::{ControlPlaneAuth, UI_AUTH_HEADER};
use crate::core_config::{load_core_config, CoreConfig};
use crate::descriptor::Descriptor;
use crate::extension::{core_extensions_dir, load_discoverable_registry, runtime_extension_roots};
use crate::host_extensions::HostExtensionRegistry;
use crate::logging;
use crate::state_store::{merge_json_object, ExtensionStateStore};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub(crate) fn start_daemon_ui_server(
    extensions_dir: std::path::PathBuf,
    bind_addr: String,
    running: Arc<AtomicBool>,
    auth: ControlPlaneAuth,
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
                    let state = match build_ui_state(
                        &extensions_dir,
                        None,
                        false,
                        auth.clone(),
                        format!("http://{}", local_addr),
                    ) {
                        Ok(state) => state,
                        Err(err) => {
                            logging::error(format!("failed to refresh UI state: {err}"));
                            continue;
                        }
                    };
                    if let Err(err) = handle_connection(stream, &state) {
                        logging::error(format!("config UI request error: {err}"));
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.raw_os_error() == Some(10035) =>
                {
                    std::thread::sleep(Duration::from_millis(30));
                }
                Err(err) => {
                    logging::error(format!(
                        "config UI server socket error on {thread_url}: {err}"
                    ));
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

pub(super) fn build_ui_state(
    extensions_dir: &Path,
    selected_extension_id: Option<&str>,
    allow_close: bool,
    auth: ControlPlaneAuth,
    origin: String,
) -> Result<UiServerState, UiConfigError> {
    let registry = load_discoverable_registry(extensions_dir)?;
    let mut discoverable_descriptors = registry
        .list()
        .map(|extension| extension.descriptor.clone())
        .collect::<Vec<_>>();
    discoverable_descriptors.sort_by(|a, b| a.id.cmp(&b.id));

    let core_config = load_core_config()?;
    let descriptors = visible_descriptors(&discoverable_descriptors, &core_config);
    let user_extensions_root = normalize_path(extensions_dir);
    let core_extension_ids = registry
        .list()
        .filter(|extension| {
            extension
                .root
                .parent()
                .map(normalize_path)
                .map(|root| root != user_extensions_root)
                .unwrap_or(true)
        })
        .map(|extension| extension.descriptor.id.clone())
        .collect::<HashSet<_>>();

    let extension_ids = descriptors
        .iter()
        .map(|descriptor| descriptor.id.clone())
        .collect::<HashSet<_>>();

    let selected_extension_id = if let Some(selected) = selected_extension_id {
        if !extension_ids.contains(selected) {
            return Err(UiConfigError::ExtensionNotFound(selected.to_string()));
        }
        selected.to_string()
    } else {
        String::new()
    };

    let state_store = ExtensionStateStore::for_current_user()?;
    state_store.ensure_root()?;

    Ok(UiServerState {
        selected_extension_id,
        descriptors,
        discoverable_descriptors,
        core_extension_ids,
        extension_ids,
        user_extensions_dir: extensions_dir.to_path_buf(),
        core_extensions_dir: core_extensions_dir(),
        runtime_extension_roots: runtime_extension_roots(extensions_dir),
        state_store,
        host_extensions: HostExtensionRegistry::new(),
        auth_token: auth.token().to_string(),
        origin,
        allow_close,
    })
}

fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn visible_descriptors(
    discoverable_descriptors: &[Descriptor],
    core_config: &CoreConfig,
) -> Vec<Descriptor> {
    discoverable_descriptors
        .iter()
        .filter(|descriptor| core_config.is_extension_enabled(&descriptor.id))
        .cloned()
        .collect()
}

pub(super) fn find_discoverable_descriptor<'a>(
    state: &'a UiServerState,
    extension_id: &str,
) -> Option<&'a Descriptor> {
    state
        .discoverable_descriptors
        .iter()
        .find(|descriptor| descriptor.id == extension_id)
}

pub(crate) fn open_extension_config(
    extensions_dir: &Path,
    extension_id: &str,
    options: UiOpenOptions,
) -> Result<String, UiConfigError> {
    let auth = ControlPlaneAuth::ephemeral();

    let listener = TcpListener::bind(&options.bind_addr)?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{}", local_addr);
    let state = build_ui_state(extensions_dir, Some(extension_id), true, auth, url.clone())?;

    if options.open_browser {
        open_in_browser(&url)?;
    }

    if options.open_window {
        std::thread::spawn(move || {
            if let Err(err) = serve_ui_listener(listener, state, options.idle_timeout) {
                logging::error(format!("config UI server error: {err}"));
            }
        });
        open_in_native_window(&url)?;
        return Ok(url);
    }

    serve_ui_listener(listener, state, options.idle_timeout)?;
    Ok(url)
}

fn serve_ui_listener(
    listener: TcpListener,
    state: UiServerState,
    idle_timeout: Duration,
) -> Result<(), UiConfigError> {
    let mut should_stop = false;
    let mut last_activity = Instant::now();
    while !should_stop && last_activity.elapsed() < idle_timeout {
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

    Ok(())
}

pub(super) fn handle_connection(
    mut stream: TcpStream,
    state: &UiServerState,
) -> Result<bool, UiConfigError> {
    let request = match parse_request(stream.try_clone()?) {
        Ok(request) => request,
        Err(err) => {
            let _ = write_response(&mut stream, HttpResponse::bad_request(err.to_string()));
            return Ok(false);
        }
    };

    let requires_auth = !(request.method == HttpMethod::Get && request.path == "/");
    if requires_auth && !request_is_authorized(&request, state) {
        write_response(
            &mut stream,
            HttpResponse::forbidden("missing or invalid control-plane token"),
        )?;
        return Ok(false);
    }

    let mut stop_after = false;
    let response = if request.method == HttpMethod::Get && request.path == "/" {
        HttpResponse::ok_html(render_html(state))
    } else if request.method == HttpMethod::Get && request.path == "/descriptor" {
        HttpResponse::ok_json(&serde_json::json!({
            "selectedExtensionId": state.selected_extension_id,
            "descriptors": state.descriptors,
            "discoverableDescriptors": state.discoverable_descriptors,
            "coreExtensionIds": state.core_extension_ids,
        }))?
    } else if request.method == HttpMethod::Get && request.path == "/config/core" {
        HttpResponse::ok_json(&state.state_store.inspect_core_config()?.value)?
    } else if request.method == HttpMethod::Post && request.path == "/config/core" {
        match parse_json_object(&request.body) {
            Ok(value) => {
                let merged = merge_json_object(&state.state_store.core_config_path(), &value)?;
                match apply_core_settings(&merged) {
                    Ok(()) => HttpResponse::ok_json(&serde_json::json!({ "ok": true }))?,
                    Err(err) => HttpResponse::bad_request(format!(
                        "core settings were saved, but applying them failed: {err}"
                    )),
                }
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
        HttpResponse::ok_json(&build_core_info(state)?)?
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
        if let Some(descriptor) = find_discoverable_descriptor(state, extension_id) {
            HttpResponse::ok_json(&build_extension_info(state, descriptor)?)?
        } else {
            HttpResponse::not_found()
        }
    } else if let Some(extension_id) = request.path.strip_prefix("/config/extension/") {
        if !state.extension_ids.contains(extension_id) {
            HttpResponse::not_found()
        } else {
            match request.method {
                HttpMethod::Get => {
                    HttpResponse::ok_json(&state.state_store.inspect_config(extension_id)?.value)?
                }
                HttpMethod::Post => match parse_json_object(&request.body) {
                    Ok(value) => {
                        let path = state.state_store.config_path(extension_id);
                        merge_json_object(&path, &value)?;
                        HttpResponse::ok_json(&serde_json::json!({ "ok": true }))?
                    }
                    Err(err) => HttpResponse::bad_request(err.to_string()),
                },
            }
        }
    } else if let Some(rest) = request.path.strip_prefix("/trigger/extension/") {
        if request.method != HttpMethod::Post {
            HttpResponse::not_found()
        } else {
            let mut parts = rest.splitn(2, '/');
            let extension_id = parts.next().unwrap_or("");
            let action_id = parts.next();
            if extension_id.is_empty() || !state.extension_ids.contains(extension_id) {
                HttpResponse::not_found()
            } else {
                let inputs = if request.body.is_empty() {
                    state.state_store.inspect_config(extension_id)?.value
                } else {
                    parse_json_object(&request.body)?
                };
                match handle_trigger_extension(state, extension_id, action_id, inputs) {
                    Ok(resp) => resp,
                    Err(err) => HttpResponse::bad_request(err.to_string()),
                }
            }
        }
    } else {
        HttpResponse::not_found()
    };

    write_response(&mut stream, response)?;
    Ok(stop_after)
}

fn handle_trigger_extension(
    state: &UiServerState,
    extension_id: &str,
    action_id: Option<&str>,
    inputs: serde_json::Value,
) -> Result<HttpResponse, UiConfigError> {
    use crate::execution::ExecutionEngine;
    use crate::extension::load_runtime_registry;
    use crate::runtime::default_runtime_adapter;

    let registry = load_runtime_registry(&state.user_extensions_dir)?;
    let ext = registry
        .get(extension_id)
        .ok_or_else(|| UiConfigError::ExtensionNotFound(extension_id.to_string()))?;
    let runtime = default_runtime_adapter().map_err(|e| UiConfigError::Request(e.to_string()))?;
    let engine = ExecutionEngine::new(runtime.as_ref(), &state.host_extensions, &state.state_store);
    let prepared = engine
        .prepare_trigger(ext, action_id)
        .map_err(UiConfigError::Request)?;
    engine
        .execute_trigger(&prepared, &inputs)
        .map_err(UiConfigError::Request)?;
    HttpResponse::ok_json(&serde_json::json!({
        "ok": true,
        "extensionId": prepared.extension_id,
        "actionId": prepared.action_id,
    }))
}

fn request_is_authorized(request: &HttpRequest, state: &UiServerState) -> bool {
    let token_matches = request
        .headers
        .get(UI_AUTH_HEADER)
        .map(|value| value == &state.auth_token)
        .unwrap_or(false);
    if !token_matches {
        return false;
    }

    if request.method == HttpMethod::Post {
        if let Some(origin) = request.headers.get("origin") {
            return origin == &state.origin;
        }
    }

    true
}

pub(super) fn parse_json_object(raw: &[u8]) -> Result<Value, UiConfigError> {
    let parsed: Value = serde_json::from_slice(raw)
        .map_err(|e| UiConfigError::Request(format!("invalid JSON body: {e}")))?;
    if !parsed.is_object() {
        return Err(UiConfigError::Request(
            "config payload must be a JSON object".to_string(),
        ));
    }
    Ok(parsed)
}

fn apply_core_settings(config: &Value) -> Result<(), UiConfigError> {
    crate::autostart::sync_from_core_config(config)?;
    Ok(())
}

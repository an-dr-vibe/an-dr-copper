use crate::descriptor::Descriptor;
use crate::extension::load_runtime_registry;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use thiserror::Error;

const DEFAULT_UI_BIND: &str = "127.0.0.1:0";

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
    ui_config_dir: PathBuf,
}

pub fn open_extension_config(
    extensions_dir: &Path,
    extension_id: &str,
    options: UiOpenOptions,
) -> Result<String, UiConfigError> {
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

    if !extension_ids.contains(extension_id) {
        return Err(UiConfigError::ExtensionNotFound(extension_id.to_string()));
    }

    let ui_config_dir = ui_config_dir()?;
    fs::create_dir_all(&ui_config_dir)?;

    let state = UiServerState {
        selected_extension_id: extension_id.to_string(),
        descriptors,
        extension_ids,
        ui_config_dir,
    };

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
        HttpResponse::ok_json(&load_config(&core_config_path(&state.ui_config_dir))?)?
    } else if request.method == HttpMethod::Post && request.path == "/config/core" {
        match parse_json_object(&request.body) {
            Ok(value) => {
                store_config(&core_config_path(&state.ui_config_dir), &value)?;
                HttpResponse::ok_json(&serde_json::json!({ "ok": true }))?
            }
            Err(err) => HttpResponse::bad_request(err.to_string()),
        }
    } else if request.method == HttpMethod::Post && request.path == "/close" {
        stop_after = true;
        HttpResponse::no_content()
    } else if let Some(extension_id) = request.path.strip_prefix("/config/extension/") {
        if !state.extension_ids.contains(extension_id) {
            HttpResponse::not_found()
        } else {
            let path = extension_config_path_for(&state.ui_config_dir, extension_id);
            match request.method {
                HttpMethod::Get => HttpResponse::ok_json(&load_config(&path)?)?,
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

fn ui_config_dir() -> Result<PathBuf, UiConfigError> {
    let home = dirs::home_dir().ok_or_else(|| {
        UiConfigError::Request("cannot resolve home directory for config storage".to_string())
    })?;
    Ok(home.join(".Copper").join("ui-config"))
}

fn core_config_path(ui_config_dir: &Path) -> PathBuf {
    ui_config_dir.join("copper-core.json")
}

fn extension_config_path_for(ui_config_dir: &Path, extension_id: &str) -> PathBuf {
    ui_config_dir.join(format!("{extension_id}.json"))
}

fn load_config(path: &Path) -> Result<Value, UiConfigError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn store_config(path: &Path, value: &Value) -> Result<(), UiConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
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

fn render_html(state: &UiServerState) -> String {
    let model = serde_json::json!({
        "selectedExtensionId": state.selected_extension_id,
        "descriptors": state.descriptors,
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
      --bg:#16181d; --panel:#20232a; --panel2:#1b1e24; --line:#3a3f4b; --text:#e7e9ee; --muted:#a6acb9; --accent:#7aa2f7;
    }}
    * {{ box-sizing:border-box; }}
    body {{ margin:0; background:var(--bg); color:var(--text); font-family:Segoe UI, Arial, sans-serif; }}
    .layout {{ display:grid; grid-template-columns:280px 1fr; min-height:100vh; }}
    .sidebar {{ background:var(--panel2); border-right:1px solid var(--line); padding:14px; }}
    .main {{ padding:20px; }}
    .title {{ font-size:18px; font-weight:700; margin:4px 0 12px; }}
    .nav-btn {{
      width:100%; text-align:left; border:1px solid var(--line); background:transparent; color:var(--text);
      padding:10px 12px; margin-bottom:8px; border-radius:8px; cursor:pointer;
    }}
    .nav-btn.active {{ background:rgba(122,162,247,.2); border-color:var(--accent); }}
    .section-title {{ font-size:22px; margin:0 0 6px; }}
    .section-sub {{ color:var(--muted); margin:0 0 14px; }}
    .card {{ background:var(--panel); border:1px solid var(--line); border-radius:10px; padding:14px; margin-bottom:14px; }}
    label {{ display:block; font-weight:600; margin:10px 0 6px; }}
    input, select {{
      width:100%; border:1px solid var(--line); border-radius:8px; background:#111319; color:var(--text);
      padding:10px;
    }}
    .actions {{ display:flex; flex-wrap:wrap; gap:8px; margin-top:8px; }}
    .chip {{ border:1px solid var(--line); border-radius:999px; padding:6px 10px; color:var(--muted); }}
    .btn-row {{ display:flex; gap:10px; margin-top:14px; }}
    button {{
      border:1px solid var(--line); border-radius:8px; background:#111319; color:var(--text); padding:10px 14px; cursor:pointer;
    }}
    button.primary {{ background:var(--accent); border-color:transparent; color:#0b1020; font-weight:700; }}
    .status {{ color:var(--muted); margin-top:8px; min-height:20px; }}
  </style>
</head>
<body>
  <div class="layout">
    <aside class="sidebar">
      <div class="title">Settings</div>
      <div id="nav"></div>
    </aside>
    <main class="main">
      <h1 class="section-title" id="sectionTitle">Copper</h1>
      <p class="section-sub" id="sectionSub">Core Copper configuration</p>
      <div class="card">
        <div id="form"></div>
        <div class="btn-row">
          <button class="primary" id="saveBtn">Save Section</button>
          <button id="closeBtn">Close UI Server</button>
        </div>
        <div class="status" id="status"></div>
      </div>
      <div class="card">
        <strong>Actions</strong>
        <div class="actions" id="actions"></div>
      </div>
    </main>
  </div>

  <script>
    const model = {model_inline};
    const descriptors = model.descriptors || [];
    const byId = Object.fromEntries(descriptors.map(d => [d.id, d]));
    let currentSection = model.selectedExtensionId ? `ext:${{model.selectedExtensionId}}` : 'core';

    const navEl = document.getElementById('nav');
    const formEl = document.getElementById('form');
    const actionsEl = document.getElementById('actions');
    const statusEl = document.getElementById('status');
    const sectionTitleEl = document.getElementById('sectionTitle');
    const sectionSubEl = document.getElementById('sectionSub');

    function setStatus(msg) {{ statusEl.textContent = msg; }}

    function createNavButton(key, label) {{
      const btn = document.createElement('button');
      btn.className = 'nav-btn' + (key === currentSection ? ' active' : '');
      btn.textContent = label;
      btn.addEventListener('click', () => {{
        currentSection = key;
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

    function createInput(input, value) {{
      const wrapper = document.createElement('div');
      const label = document.createElement('label');
      label.textContent = input.label + ' (' + input.id + ')';
      wrapper.appendChild(label);

      let control;
      if (input.type === 'boolean') {{
        control = document.createElement('select');
        control.innerHTML = '<option value="true">true</option><option value="false">false</option>';
        control.value = String(value ?? input.default ?? false);
      }} else if (input.type === 'select') {{
        control = document.createElement('select');
        (input.options || []).forEach(opt => {{
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

      control.dataset.inputId = input.id;
      control.dataset.inputType = input.type;
      wrapper.appendChild(control);
      return wrapper;
    }}

    async function loadJson(url) {{
      const res = await fetch(url);
      if (!res.ok) {{
        throw new Error((await res.text()) || ('HTTP ' + res.status));
      }}
      return await res.json();
    }}

    async function renderSection() {{
      formEl.innerHTML = '';
      actionsEl.innerHTML = '';
      setStatus('');

      if (currentSection === 'core') {{
        sectionTitleEl.textContent = 'Copper';
        sectionSubEl.textContent = 'Core Copper configuration (separate from extension settings)';
        const config = await loadJson('/config/core');

        const fields = [
          {{ id: 'userExtensionsDir', label: 'User extensions directory', type: 'text', default: '~/.Copper/extensions' }},
          {{ id: 'uiTheme', label: 'UI Theme', type: 'text', default: 'obsidian' }},
          {{ id: 'startupExtension', label: 'Startup extension id', type: 'text', default: model.selectedExtensionId || '' }}
        ];

        fields.forEach(field => {{
          const row = createInput(field, config[field.id]);
          formEl.appendChild(row);
        }});

        const chip = document.createElement('div');
        chip.className = 'chip';
        chip.textContent = 'Core settings are stored in a dedicated section';
        actionsEl.appendChild(chip);
        return;
      }}

      const extensionId = currentSection.slice(4);
      const descriptor = byId[extensionId];
      if (!descriptor) {{
        throw new Error('Unknown extension section: ' + extensionId);
      }}

      sectionTitleEl.textContent = descriptor.name;
      sectionSubEl.textContent = `Extension id: ${{descriptor.id}}`;

      const config = await loadJson('/config/extension/' + encodeURIComponent(extensionId));

      const actionLabel = document.createElement('label');
      actionLabel.textContent = 'Action';
      formEl.appendChild(actionLabel);
      const actionSelect = document.createElement('select');
      actionSelect.id = 'actionSelect';
      (descriptor.actions || []).forEach(action => {{
        const opt = document.createElement('option');
        opt.value = action.id;
        opt.textContent = action.label + ' (' + action.id + ')';
        actionSelect.appendChild(opt);
      }});
      actionSelect.value = config.action || ((descriptor.actions && descriptor.actions[0] && descriptor.actions[0].id) || '');
      formEl.appendChild(actionSelect);

      (descriptor.inputs || []).forEach(input => {{
        formEl.appendChild(createInput(input, config[input.id]));
      }});

      (descriptor.actions || []).forEach(action => {{
        const chip = document.createElement('div');
        chip.className = 'chip';
        chip.textContent = action.id + ': ' + action.label;
        actionsEl.appendChild(chip);
      }});
    }}

    function collectCurrentPayload() {{
      const payload = {{}};
      if (currentSection.startsWith('ext:')) {{
        const actionSelect = document.getElementById('actionSelect');
        if (actionSelect) payload.action = actionSelect.value;
      }}

      const controls = formEl.querySelectorAll('[data-input-id]');
      controls.forEach(ctrl => {{
        const id = ctrl.dataset.inputId;
        const type = ctrl.dataset.inputType;
        if (type === 'boolean') {{
          payload[id] = ctrl.value === 'true';
        }} else if (type === 'number') {{
          payload[id] = ctrl.value === '' ? null : Number(ctrl.value);
        }} else {{
          payload[id] = ctrl.value;
        }}
      }});
      return payload;
    }}

    document.getElementById('saveBtn').addEventListener('click', async () => {{
      try {{
        const payload = collectCurrentPayload();
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
        setStatus('Section saved successfully.');
      }} catch (err) {{
        setStatus('Save failed: ' + err);
      }}
    }});

    document.getElementById('closeBtn').addEventListener('click', async () => {{
      try {{
        await fetch('/close', {{ method: 'POST' }});
        setStatus('UI server closed. You can close this tab.');
      }} catch (err) {{
        setStatus('Close failed: ' + err);
      }}
    }});

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
    use super::{core_config_path, extension_config_path_for, read_chunked_body, render_html};
    use crate::descriptor::{Action, Descriptor, InputField, InputType, UiDescriptor};
    use std::collections::HashSet;
    use std::io::{BufReader, Cursor};
    use std::path::PathBuf;

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
                default: serde_json::json!("~/Desktop"),
                options: vec![],
            }],
            actions: vec![Action {
                id: "move-torrents".to_string(),
                label: "Move .torrent files".to_string(),
                script: "Move .torrent files".to_string(),
            }],
            ui: Some(UiDescriptor {
                ui_type: "form".to_string(),
                source: None,
                on_select: None,
            }),
        }
    }

    fn sample_state() -> super::UiServerState {
        let descriptor = sample_descriptor();
        super::UiServerState {
            selected_extension_id: descriptor.id.clone(),
            extension_ids: [descriptor.id.clone()].into_iter().collect::<HashSet<_>>(),
            descriptors: vec![descriptor],
            ui_config_dir: PathBuf::from("C:/tmp/copper-ui"),
        }
    }

    #[test]
    fn render_html_contains_sectioned_settings_layout() {
        let html = render_html(&sample_state());
        assert!(html.contains("Settings"));
        assert!(html.contains("Copper"));
        assert!(html.contains("Desktop Torrent Organizer"));
        assert!(html.contains("Save Section"));
    }

    #[test]
    fn config_paths_are_sectioned() {
        let root = PathBuf::from("C:/tmp/copper-ui");
        assert_eq!(core_config_path(&root), root.join("copper-core.json"));
        assert_eq!(
            extension_config_path_for(&root, "desktop-torrent-organizer"),
            root.join("desktop-torrent-organizer.json")
        );
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
}

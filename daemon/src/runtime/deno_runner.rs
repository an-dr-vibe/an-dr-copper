use crate::api;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const BRIDGE_TS: &str = include_str!("../../../sdk/bridge.ts");
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn find_deno() -> Option<PathBuf> {
    let locator = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut command = Command::new(locator);
    command.arg("deno");
    apply_hidden_window(&mut command);
    if let Ok(output) = command.output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout);
            let first = path.lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                let p = PathBuf::from(first);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    // Fallback: ~/.deno/bin/deno[.exe]
    if let Some(home) = dirs::home_dir() {
        let name = if cfg!(target_os = "windows") {
            "deno.exe"
        } else {
            "deno"
        };
        let p = home.join(".deno").join("bin").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn execute_extension(
    extension_id: &str,
    main_ts_path: &str,
    store_json_path: &str,
    permissions: &[String],
    inputs: &Value,
) -> Result<(), String> {
    let deno =
        find_deno().ok_or_else(|| "deno not found; install from https://deno.land".to_string())?;

    // Write the embedded bridge to a temp file so Deno can run it.
    let bridge_path =
        std::env::temp_dir().join(format!("copper-bridge-{}.ts", rand::random::<u64>()));
    std::fs::write(&bridge_path, BRIDGE_TS)
        .map_err(|e| format!("failed to write bridge.ts: {e}"))?;

    let inputs_json = serde_json::to_string(inputs).unwrap_or_else(|_| "{}".to_string());

    let mut command = Command::new(&deno);
    command
        .args(["run", "--no-check"])
        .arg(format!("--allow-read={main_ts_path}"))
        .arg("--allow-env=COPPER_MAIN_TS,COPPER_INPUTS")
        .arg(&bridge_path)
        .env("COPPER_MAIN_TS", main_ts_path)
        .env("COPPER_INPUTS", &inputs_json)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    apply_hidden_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to spawn deno: {e}"))?;

    let result = drive_extension(&mut child, extension_id, store_json_path, permissions);
    let _ = std::fs::remove_file(&bridge_path);
    result
}

fn apply_hidden_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn drive_extension(
    child: &mut Child,
    extension_id: &str,
    store_path: &str,
    permissions: &[String],
) -> Result<(), String> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "no stdin handle".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout handle".to_string())?;

    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        let line = line.map_err(|e| format!("read error: {e}"))?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Terminal signals from the extension
        if msg.get("_done").and_then(|v| v.as_bool()).unwrap_or(false) {
            break;
        }
        if let Some(err) = msg.get("_error").and_then(|v| v.as_str()) {
            let _ = child.wait();
            return Err(format!("extension error: {err}"));
        }

        // Dispatch JSON-RPC call
        let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = msg
            .get("params")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let response_value =
            match dispatch_api(method, &params, extension_id, store_path, permissions) {
                Ok(v) => serde_json::json!({ "id": id, "result": v }),
                Err(e) => serde_json::json!({ "id": id, "error": e }),
            };

        let response_line = serde_json::to_string(&response_value).unwrap() + "\n";
        if stdin.write_all(response_line.as_bytes()).is_err() {
            break;
        }
        let _ = stdin.flush();
    }

    drop(stdin);
    let _ = child.wait();
    Ok(())
}

fn dispatch_api(
    method: &str,
    params: &Value,
    _extension_id: &str,
    store_path: &str,
    permissions: &[String],
) -> Result<Value, String> {
    authorize_method(method, permissions)?;
    match method {
        "fs.list" => {
            let path = params["path"].as_str().unwrap_or("");
            let entries = api::fs::list(path);
            serde_json::to_value(&entries).map_err(|e| e.to_string())
        }
        "fs.move" => {
            let src = params["src"].as_str().unwrap_or("");
            let dst = params["dst"].as_str().unwrap_or("");
            api::fs::move_file(src, dst)
                .map(|_| Value::Null)
                .map_err(|e| e.to_string())
        }
        "fs.delete" => {
            let path = params["path"].as_str().unwrap_or("");
            api::fs::delete(path)
                .map(|_| Value::Null)
                .map_err(|e| e.to_string())
        }
        "shell.run" => {
            let cmd = params["cmd"].as_str().unwrap_or("");
            let args: Vec<String> = params["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let r = api::shell::run(cmd, &args);
            Ok(serde_json::json!({
                "code": r.code,
                "stdout": r.stdout,
                "stderr": r.stderr,
            }))
        }
        "shell.which" => {
            let binary = params["binary"].as_str().unwrap_or("");
            Ok(api::shell::which(binary)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "notify" => {
            let message = params["message"].as_str().unwrap_or("");
            api::notify::notify(message);
            Ok(Value::Null)
        }
        "store.get" => {
            let key = params["key"].as_str().unwrap_or("");
            Ok(api::store::get(store_path, key).unwrap_or(Value::Null))
        }
        "store.set" => {
            let key = params["key"].as_str().unwrap_or("");
            let value = params["value"].clone();
            api::store::set(store_path, key, value)
                .map(|_| Value::Null)
                .map_err(|e| e.to_string())
        }
        "ui.show" | "ui.update" => Ok(Value::Null),
        "keyboard.typeText" => {
            let text = params["text"].as_str().unwrap_or("");
            api::keyboard::type_text(text);
            Ok(Value::Null)
        }
        "keyboard.sendKey" => {
            let key = params["key"].as_str().unwrap_or("");
            api::keyboard::send_key(key);
            Ok(Value::Null)
        }
        "keyboard.sendCombo" => {
            let combo = params["combo"].as_str().unwrap_or("");
            api::keyboard::send_combo(combo);
            Ok(Value::Null)
        }
        "keyboard.normalizeCombo" => {
            let combo = params["combo"].as_str().unwrap_or("");
            let r = api::keyboard::normalize_combo(combo);
            Ok(serde_json::json!({ "combo": r.combo, "label": r.label }))
        }
        "secureStore.get" => {
            let service = params["service"].as_str().unwrap_or("");
            let key = params["key"].as_str().unwrap_or("");
            Ok(api::secure_store::get(service, key)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "secureStore.set" => {
            let service = params["service"].as_str().unwrap_or("");
            let key = params["key"].as_str().unwrap_or("");
            let value = params["value"].as_str().unwrap_or("");
            api::secure_store::set(service, key, value);
            Ok(Value::Null)
        }
        "secureStore.delete" => {
            let service = params["service"].as_str().unwrap_or("");
            let key = params["key"].as_str().unwrap_or("");
            api::secure_store::delete(service, key);
            Ok(Value::Null)
        }
        m if m.starts_with("windows.display.") => dispatch_windows_display(m, params),
        unknown => Err(format!("unknown API method: {unknown}")),
    }
}

fn authorize_method(method: &str, permissions: &[String]) -> Result<(), String> {
    let required = if method.starts_with("fs.") {
        Some("fs")
    } else if method.starts_with("shell.") {
        Some("shell")
    } else if method.starts_with("store.") {
        Some("store")
    } else if method.starts_with("ui.") {
        Some("ui")
    } else if method.starts_with("keyboard.") {
        Some("keyboard")
    } else if method.starts_with("secureStore.") {
        Some("secure-store")
    } else {
        None
    };

    match required {
        Some(required) if !permissions.iter().any(|permission| permission == required) => Err(
            format!("permission '{required}' is required for API method '{method}'"),
        ),
        _ => Ok(()),
    }
}

#[cfg(target_os = "windows")]
fn dispatch_windows_display(method: &str, params: &Value) -> Result<Value, String> {
    let (action_id, config) = match method {
        "windows.display.status" => ("status", serde_json::json!({})),
        "windows.display.toggleTaskbarAutoHide" => {
            ("toggle-taskbar-autohide", serde_json::json!({}))
        }
        "windows.display.setTaskbarAutoHide" => {
            let enabled = params["autoHide"].as_bool().unwrap_or(false);
            (
                "set-taskbar-autohide",
                serde_json::json!({ "enabled": enabled }),
            )
        }
        "windows.display.setResolution" => (
            "set-resolution",
            serde_json::json!({
                "resolutionWidth": params["width"],
                "resolutionHeight": params["height"],
                "refreshRate": params["refreshRate"],
            }),
        ),
        "windows.display.setScale" => (
            "set-scale",
            serde_json::json!({ "scalePercent": params["scalePercent"] }),
        ),
        other => return Err(format!("unknown windows.display method: {other}")),
    };
    api::windows_display::execute_action(action_id, &config).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn dispatch_windows_display(method: &str, _params: &Value) -> Result<Value, String> {
    Err(format!(
        "{method}: windows.display API is only available on Windows"
    ))
}

#[cfg(test)]
mod tests {
    use super::authorize_method;

    #[test]
    fn authorization_rejects_undeclared_permission() {
        let error = authorize_method("fs.list", &["ui".to_string()]).expect_err("denied");
        assert!(error.contains("permission 'fs'"));
        assert!(error.contains("fs.list"));
    }

    #[test]
    fn authorization_accepts_declared_permission() {
        authorize_method("secureStore.get", &["secure-store".to_string()]).expect("authorized");
    }

    #[test]
    fn authorization_allows_permissionless_notification() {
        authorize_method("notify", &[]).expect("notification has no manifest permission");
    }
}

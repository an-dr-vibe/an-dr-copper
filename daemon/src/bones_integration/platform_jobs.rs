use super::jobs::{read_string, JobResult};
use super::AuthorizedCapabilityJob;
use crate::api;
use serde_json::{Map, Value};

pub(super) fn execute_keyboard_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    match job.operation.as_str() {
        "type-text" => {
            let text = read_value_string(&job.args, "text", 60 * 1024)?;
            api::keyboard::type_text(text);
            Ok(Value::Null)
        }
        "send-key" => {
            let key = read_identifier(&job.args, "key", 64)?;
            api::keyboard::send_key(key);
            Ok(Value::Null)
        }
        "send-combo" => {
            let combo = read_identifier(&job.args, "combo", 256)?;
            api::keyboard::send_combo(combo);
            Ok(Value::Null)
        }
        "normalize-combo" => {
            let combo = read_identifier(&job.args, "combo", 256)?;
            let normalized = api::keyboard::normalize_combo(combo);
            Ok(serde_json::json!({
                "combo": normalized.combo,
                "label": normalized.label,
            }))
        }
        operation => unknown_operation("keyboard", operation),
    }
}

pub(super) fn execute_secure_store_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    match job.operation.as_str() {
        "get" => {
            let (service, key) = read_secure_identity(&job.args)?;
            Ok(api::secure_store::get(service, key)
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "set" => {
            let (service, key) = read_secure_identity(&job.args)?;
            let value = read_value_string(&job.args, "value", 60 * 1024)?;
            api::secure_store::set(service, key, value);
            Ok(Value::Null)
        }
        "delete" => {
            let (service, key) = read_secure_identity(&job.args)?;
            api::secure_store::delete(service, key);
            Ok(Value::Null)
        }
        operation => unknown_operation("secure-store", operation),
    }
}

fn read_secure_identity(args: &Map<String, Value>) -> Result<(&str, &str), (&'static str, String)> {
    Ok((
        read_identifier(args, "service", 256)?,
        read_identifier(args, "key", 256)?,
    ))
}

pub(super) fn execute_windows_display_operation(job: &AuthorizedCapabilityJob) -> JobResult {
    let (action_id, config) = match job.operation.as_str() {
        "status" => ("status", Map::new()),
        "toggle-taskbar-autohide" => ("toggle-taskbar-autohide", Map::new()),
        "set-taskbar-autohide" => (
            "set-taskbar-autohide",
            map_entry(
                "taskbarAutoHide",
                Value::Bool(read_bool(&job.args, "autoHide")?),
            ),
        ),
        "set-resolution" => {
            let mut config = Map::new();
            config.insert(
                "resolutionWidth".to_string(),
                read_integer(&job.args, "width", 640, 16_384)?.into(),
            );
            config.insert(
                "resolutionHeight".to_string(),
                read_integer(&job.args, "height", 480, 16_384)?.into(),
            );
            config.insert(
                "refreshRate".to_string(),
                read_integer(&job.args, "refreshRate", 1, 480)?.into(),
            );
            ("set-resolution", config)
        }
        "set-scale" => (
            "set-scale",
            map_entry(
                "scalePercent",
                read_integer(&job.args, "scalePercent", 100, 350)?.into(),
            ),
        ),
        operation => return unknown_operation("windows-display", operation),
    };

    #[cfg(target_os = "windows")]
    {
        api::windows_display::execute_action(action_id, &Value::Object(config))
            .map_err(|message| ("native-operation", message))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (action_id, config);
        Err((
            "platform-unsupported",
            "windows-display capability is only available on Windows".to_string(),
        ))
    }
}

fn read_identifier<'a>(
    args: &'a Map<String, Value>,
    field: &str,
    max_bytes: usize,
) -> Result<&'a str, (&'static str, String)> {
    let value = read_string(args, field, max_bytes)?;
    if value.chars().any(char::is_control) {
        return Err((
            "invalid-args",
            format!("'{field}' must not contain control characters"),
        ));
    }
    Ok(value)
}

fn read_value_string<'a>(
    args: &'a Map<String, Value>,
    field: &str,
    max_bytes: usize,
) -> Result<&'a str, (&'static str, String)> {
    let Some(value) = args.get(field).and_then(Value::as_str) else {
        return Err(("invalid-args", format!("'{field}' must be a string")));
    };
    if value.len() > max_bytes || value.contains('\0') {
        return Err((
            "invalid-args",
            format!("'{field}' must be at most {max_bytes} bytes without NUL characters"),
        ));
    }
    Ok(value)
}

fn read_bool(args: &Map<String, Value>, field: &str) -> Result<bool, (&'static str, String)> {
    args.get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| ("invalid-args", format!("'{field}' must be a boolean")))
}

fn read_integer(
    args: &Map<String, Value>,
    field: &str,
    minimum: i64,
    maximum: i64,
) -> Result<i64, (&'static str, String)> {
    args.get(field)
        .and_then(Value::as_i64)
        .filter(|value| (minimum..=maximum).contains(value))
        .ok_or_else(|| {
            (
                "invalid-args",
                format!("'{field}' must be an integer from {minimum} through {maximum}"),
            )
        })
}

fn map_entry(key: &str, value: Value) -> Map<String, Value> {
    [(key.to_string(), value)].into_iter().collect()
}

fn unknown_operation(family: &str, operation: &str) -> JobResult {
    Err((
        "unknown-operation",
        format!("unsupported {family} operation '{operation}'"),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        execute_keyboard_operation, execute_secure_store_operation,
        execute_windows_display_operation,
    };
    use crate::bones_integration::{AuthorizedCapabilityJob, Capability};
    use serde_json::{json, Map, Value};

    #[test]
    fn keyboard_operations_preserve_normalized_result_shapes() {
        let normalized = execute_keyboard_operation(&job(
            Capability::Keyboard,
            "normalize-combo",
            json!({"combo": "shift+ctrl+a"}),
        ))
        .expect("normalize");
        assert_eq!(normalized["combo"], json!("ctrl+shift+a"));
        assert_eq!(normalized["label"], json!("Ctrl + Shift + A"));

        assert_eq!(
            execute_keyboard_operation(&job(
                Capability::Keyboard,
                "type-text",
                json!({"text": "test"})
            ))
            .expect("type text"),
            Value::Null
        );
    }

    #[test]
    fn secure_store_operations_roundtrip_through_native_api() {
        let service = format!("copper-platform-job-test-{}", std::process::id());
        let key = "secret";
        execute_secure_store_operation(&job(
            Capability::SecureStore,
            "set",
            json!({"service": service, "key": key, "value": "classified"}),
        ))
        .expect("set");
        assert_eq!(
            execute_secure_store_operation(&job(
                Capability::SecureStore,
                "get",
                json!({"service": service, "key": key}),
            ))
            .expect("get"),
            json!("classified")
        );
        execute_secure_store_operation(&job(
            Capability::SecureStore,
            "delete",
            json!({"service": service, "key": key}),
        ))
        .expect("delete");
    }

    #[test]
    fn platform_operations_reject_malformed_arguments_before_side_effects() {
        for (capability, operation, args) in [
            (Capability::Keyboard, "send-key", json!({"key": "bad\nkey"})),
            (
                Capability::SecureStore,
                "set",
                json!({"service": "service", "key": "key"}),
            ),
            (
                Capability::WindowsDisplay,
                "set-scale",
                json!({"scalePercent": 50}),
            ),
            (
                Capability::WindowsDisplay,
                "set-resolution",
                json!({"width": 1920, "height": 1080, "refreshRate": "60"}),
            ),
        ] {
            let result = match capability {
                Capability::Keyboard => {
                    execute_keyboard_operation(&job(capability, operation, args))
                }
                Capability::SecureStore => {
                    execute_secure_store_operation(&job(capability, operation, args))
                }
                Capability::WindowsDisplay => {
                    execute_windows_display_operation(&job(capability, operation, args))
                }
                _ => unreachable!("test capability"),
            };
            assert!(matches!(result, Err(("invalid-args", _))));
        }
        assert!(matches!(
            execute_secure_store_operation(&job(Capability::SecureStore, "unknown", json!({}))),
            Err(("unknown-operation", _))
        ));
    }

    #[test]
    fn windows_display_status_is_explicitly_platform_gated() {
        let result = execute_windows_display_operation(&job(
            Capability::WindowsDisplay,
            "status",
            json!({}),
        ));
        if cfg!(target_os = "windows") {
            assert!(result.is_ok(), "Windows status failed: {result:?}");
        } else {
            assert!(matches!(result, Err(("platform-unsupported", _))));
        }
    }

    fn job(capability: Capability, operation: &str, args: Value) -> AuthorizedCapabilityJob {
        AuthorizedCapabilityJob {
            job_id: "job-1".to_string(),
            extension_id: "extension".to_string(),
            request_id: "request-1".to_string(),
            capability,
            operation: operation.to_string(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        }
    }
}

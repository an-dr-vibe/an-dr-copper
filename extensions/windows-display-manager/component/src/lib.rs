#[cfg(target_family = "wasm")]
use copper_component_sdk::host_api::{log, Level};
#[cfg(target_family = "wasm")]
use copper_component_sdk::{
    decode_host_event, request, Capability, Guest, HostEvent, COPPER_ACTIONS_ENDPOINT,
};
#[cfg(target_family = "wasm")]
use serde_json::json;
use serde_json::{Map, Value};
#[cfg(target_family = "wasm")]
use std::cell::RefCell;
#[cfg(target_family = "wasm")]
use std::collections::BTreeMap;

#[cfg(target_family = "wasm")]
thread_local! {
    static STEPS: RefCell<BTreeMap<String, Step>> = const { RefCell::new(BTreeMap::new()) };
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone)]
enum Step {
    Config {
        action: String,
    },
    Native {
        action: String,
    },
    Clock {
        action: String,
        native_result: Value,
        error: Option<String>,
    },
    Status {
        action: String,
        native_result: Value,
        error: Option<String>,
    },
    Done,
}

#[cfg(target_family = "wasm")]
struct Component;

#[cfg(target_family = "wasm")]
impl Guest for Component {
    fn init() {
        log(Level::Info, "Windows Display Manager initialized");
    }

    fn shutdown() {
        STEPS.with(|steps| steps.borrow_mut().clear());
    }

    fn on_tick(_dt: f32) {}

    fn on_message(_topic: String, sender: String, payload: Vec<u8>) -> Option<Vec<u8>> {
        match decode_host_event(&sender, &payload) {
            Ok(HostEvent::Action {
                request_id,
                action_id,
                ..
            }) if sender == COPPER_ACTIONS_ENDPOINT && is_action(&action_id) => {
                if requires_config(&action_id) {
                    queue(
                        format!("{request_id}.config"),
                        Step::Config { action: action_id },
                        Capability::Store,
                        "config.get",
                        Map::new(),
                    );
                } else {
                    queue_native(request_id, action_id, Map::new());
                }
            }
            Ok(HostEvent::JobResult {
                request_id, result, ..
            }) => {
                if let Some(step) = take_step(&request_id) {
                    advance(request_id, step, result);
                }
            }
            Ok(HostEvent::JobError {
                request_id,
                code,
                message,
                ..
            }) => {
                if let Some(request_id) = request_id {
                    if let Some(Step::Native { action }) = take_step(&request_id) {
                        queue_clock(request_id, action, Value::Null, Some(message.clone()));
                    }
                }
                log(Level::Error, &format!("{code}: {message}"));
            }
            Ok(_) => {}
            Err(error) => log(Level::Warn, &error.to_string()),
        }
        None
    }
}

#[cfg(target_family = "wasm")]
fn advance(request_id: String, step: Step, result: Value) {
    match step {
        Step::Config { action } => {
            let args = display_args(&action, &result);
            queue_native(request_id, action, args);
        }
        Step::Native { action } => queue_clock(request_id, action, result, None),
        Step::Clock {
            action,
            native_result,
            error,
        } => {
            let timestamp = result_timestamp(&result);
            let status = status_value(&action, timestamp, &native_result, error.as_deref());
            queue(
                format!("{request_id}.status"),
                Step::Status {
                    action,
                    native_result,
                    error,
                },
                Capability::Store,
                "status.merge",
                map([("value", status)]),
            );
        }
        Step::Status {
            action,
            native_result,
            error,
        } => {
            queue(
                format!("{request_id}.notify"),
                Step::Done,
                Capability::Notify,
                "show",
                map([(
                    "message",
                    json!(notification_message(
                        &action,
                        &native_result,
                        error.as_deref()
                    )),
                )]),
            );
        }
        Step::Done => {}
    }
}

#[cfg(target_family = "wasm")]
fn queue_native(request_id: String, action: String, args: Map<String, Value>) {
    queue(
        format!("{request_id}.native"),
        Step::Native {
            action: action.clone(),
        },
        Capability::WindowsDisplay,
        &action,
        args,
    );
}

#[cfg(target_family = "wasm")]
fn queue_clock(request_id: String, action: String, result: Value, error: Option<String>) {
    queue(
        format!("{request_id}.clock"),
        Step::Clock {
            action,
            native_result: result,
            error,
        },
        Capability::Clock,
        "unix-now",
        Map::new(),
    );
}

#[cfg(target_family = "wasm")]
fn is_action(action: &str) -> bool {
    matches!(
        action,
        "status"
            | "toggle-taskbar-autohide"
            | "set-taskbar-autohide"
            | "set-resolution"
            | "set-scale"
    )
}

#[cfg(target_family = "wasm")]
fn requires_config(action: &str) -> bool {
    matches!(
        action,
        "set-taskbar-autohide" | "set-resolution" | "set-scale"
    )
}

fn display_args(action: &str, config: &Value) -> Map<String, Value> {
    match action {
        "set-taskbar-autohide" => map([(
            "autoHide",
            Value::Bool(
                config
                    .get("taskbarAutoHide")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        )]),
        "set-resolution" => {
            let (width, height, refresh_rate) = configured_resolution(config);
            map([
                ("width", Value::from(width)),
                ("height", Value::from(height)),
                ("refreshRate", Value::from(refresh_rate)),
            ])
        }
        "set-scale" => map([(
            "scalePercent",
            Value::from(
                config
                    .get("scalePercent")
                    .and_then(Value::as_i64)
                    .unwrap_or(100),
            ),
        )]),
        _ => Map::new(),
    }
}

fn configured_resolution(config: &Value) -> (i64, i64, i64) {
    if let Some(mode) = config.get("resolutionMode").and_then(Value::as_str) {
        if let Some((dimensions, refresh)) = mode.split_once('@') {
            if let Some((width, height)) = dimensions.split_once('x') {
                if let (Ok(width), Ok(height), Ok(refresh)) =
                    (width.parse(), height.parse(), refresh.parse())
                {
                    return (width, height, refresh);
                }
            }
        }
    }
    (
        config
            .get("resolutionWidth")
            .and_then(Value::as_i64)
            .unwrap_or(1920),
        config
            .get("resolutionHeight")
            .and_then(Value::as_i64)
            .unwrap_or(1080),
        config
            .get("refreshRate")
            .and_then(Value::as_i64)
            .unwrap_or(60),
    )
}

#[cfg(target_family = "wasm")]
fn result_timestamp(value: &Value) -> u64 {
    value.as_u64().unwrap_or_default()
}

fn status_value(action: &str, timestamp: u64, result: &Value, error: Option<&str>) -> Value {
    let mut status = serde_json::json!({
        "lastActionId": action,
        "lastActionUnix": timestamp,
        "lastActionOk": error.is_none(),
        "_stateContract": {
            "capabilityId": "host.windows-display",
            "schemaVersion": 1,
            "stateKind": "status"
        }
    });
    if let Some(error) = error {
        status["lastError"] = Value::String(error.to_string());
        return status;
    }
    status["lastResult"] = result.clone();
    status["lastError"] = Value::Null;
    if let Some(value) = result.get("taskbarAutoHide") {
        status["taskbarAutoHide"] = value.clone();
    }
    if let Some(value) = result.pointer("/scale/currentPercent") {
        status["scalePercent"] = value.clone();
    }
    if let Some(resolution) = result.get("resolution") {
        for (source, target) in [
            ("width", "resolutionWidth"),
            ("height", "resolutionHeight"),
            ("refreshRate", "refreshRate"),
        ] {
            if let Some(value) = resolution.get(source) {
                status[target] = value.clone();
            }
        }
    }
    status
}

fn notification_message(action: &str, result: &Value, error: Option<&str>) -> String {
    if let Some(error) = error {
        return format!("Windows Display Manager: {action} failed: {error}");
    }
    match action {
        "status" => format!(
            "Display {}x{}@{}Hz | Scale {}% | Taskbar auto-hide: {}",
            result
                .pointer("/resolution/width")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            result
                .pointer("/resolution/height")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            result
                .pointer("/resolution/refreshRate")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            result
                .pointer("/scale/currentPercent")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            result
                .get("taskbarAutoHide")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        ),
        "toggle-taskbar-autohide" => format!(
            "Taskbar auto-hide: {}",
            if result
                .get("taskbarAutoHide")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "enabled"
            } else {
                "disabled"
            }
        ),
        "set-taskbar-autohide" => format!(
            "Taskbar auto-hide set to: {}",
            if result
                .get("taskbarAutoHide")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "enabled"
            } else {
                "disabled"
            }
        ),
        "set-resolution" => format!(
            "Resolution set to {}x{}@{}Hz",
            result
                .pointer("/resolution/width")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            result
                .pointer("/resolution/height")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            result
                .pointer("/resolution/refreshRate")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        ),
        "set-scale" => format!(
            "Display scale set to {}%",
            result
                .pointer("/scale/currentPercent")
                .and_then(Value::as_i64)
                .unwrap_or_default()
        ),
        _ => format!("Windows Display Manager: {action} completed"),
    }
}

#[cfg(target_family = "wasm")]
fn queue(
    request_id: String,
    step: Step,
    capability: Capability,
    operation: &str,
    args: Map<String, Value>,
) {
    STEPS.with(|steps| {
        steps.borrow_mut().insert(request_id.clone(), step);
    });
    if let Err(error) = request(request_id.clone(), capability, operation, args) {
        take_step(&request_id);
        log(Level::Error, &error.to_string());
    }
}

#[cfg(target_family = "wasm")]
fn take_step(request_id: &str) -> Option<Step> {
    STEPS.with(|steps| steps.borrow_mut().remove(request_id))
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(target_family = "wasm")]
copper_component_sdk::bindings::export!(
    Component with_types_in copper_component_sdk::bindings
);

#[cfg(test)]
mod tests {
    use super::{configured_resolution, display_args, notification_message, status_value};
    use serde_json::json;

    #[test]
    fn saved_display_settings_become_bounded_native_arguments() {
        let config = json!({
            "taskbarAutoHide": true,
            "resolutionMode": "2560x1440@120",
            "scalePercent": 150
        });
        assert_eq!(configured_resolution(&config), (2560, 1440, 120));
        assert_eq!(
            display_args("set-taskbar-autohide", &config)["autoHide"],
            json!(true)
        );
        assert_eq!(
            display_args("set-resolution", &config)["width"],
            json!(2560)
        );
        assert_eq!(
            display_args("set-scale", &config)["scalePercent"],
            json!(150)
        );
    }

    #[test]
    fn native_result_is_flattened_into_the_existing_status_contract() {
        let status = status_value(
            "status",
            42,
            &json!({
                "taskbarAutoHide": true,
                "resolution": {"width": 1920, "height": 1080, "refreshRate": 60},
                "scale": {"currentPercent": 125}
            }),
            None,
        );
        assert_eq!(status["lastActionUnix"], json!(42));
        assert_eq!(status["lastActionOk"], json!(true));
        assert_eq!(status["resolutionWidth"], json!(1920));
        assert_eq!(status["scalePercent"], json!(125));
        assert_eq!(
            notification_message("set-scale", &status["lastResult"], None),
            "Display scale set to 125%"
        );
    }
}

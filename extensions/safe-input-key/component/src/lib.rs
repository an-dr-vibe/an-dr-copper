#[cfg(target_family = "wasm")]
use copper_component_sdk::host_api::{log, Level};
#[cfg(target_family = "wasm")]
use copper_component_sdk::{
    decode_host_event, request, Capability, Guest, HostEvent, COPPER_ACTIONS_ENDPOINT,
};
use serde_json::Value;
#[cfg(target_family = "wasm")]
use serde_json::{json, Map};

#[cfg(target_family = "wasm")]
const SERVICE: &str = "SafeInputKey";
#[cfg(target_family = "wasm")]
const KEY_TEXT: &str = "stored_text";

#[cfg(target_family = "wasm")]
struct Component;

#[cfg(target_family = "wasm")]
impl Guest for Component {
    fn init() {
        log(Level::Info, "Safe Input Key initialized");
    }

    fn shutdown() {}

    fn on_tick(_dt: f32) {}

    fn on_message(_topic: String, sender: String, payload: Vec<u8>) -> Option<Vec<u8>> {
        match decode_host_event(&sender, &payload) {
            Ok(HostEvent::Action {
                request_id,
                action_id,
                input,
            }) if sender == COPPER_ACTIONS_ENDPOINT => match action_id.as_str() {
                "type-text" => {
                    secure_request(format!("{request_id}.secret"), "get", secure_args(None))
                }
                "setup" => {
                    if let Some(text) = select_setup_text(&input, &Value::Null) {
                        save_text(request_id, text);
                    } else {
                        send(
                            format!("{request_id}.setup-config"),
                            Capability::Store,
                            "config.get",
                            Map::new(),
                        );
                    }
                }
                "clear" => {
                    secure_request(format!("{request_id}.clear"), "delete", secure_args(None))
                }
                _ => {}
            },
            Ok(HostEvent::JobResult {
                request_id, result, ..
            }) if request_id.ends_with(".secret") => {
                let action = request_id.trim_end_matches(".secret");
                if let Some(text) = result.as_str().filter(|text| !text.is_empty()) {
                    send(
                        format!("{action}.type"),
                        Capability::Keyboard,
                        "type-text",
                        map([("text", json!(text))]),
                    );
                } else {
                    notify(action, "Safe Input Key: no text saved — run setup first.");
                }
            }
            Ok(HostEvent::JobResult {
                request_id, result, ..
            }) if request_id.ends_with(".setup-config") => {
                let action = request_id.trim_end_matches(".setup-config");
                if let Some(text) = select_setup_text(&Map::new(), &result) {
                    save_text(action.to_string(), text);
                } else {
                    notify(action, "Safe Input Key: provide the text to save.");
                }
            }
            Ok(HostEvent::JobResult { request_id, .. }) if request_id.ends_with(".save") => {
                notify(
                    request_id.trim_end_matches(".save"),
                    "Safe Input Key: text saved to OS keychain.",
                );
            }
            Ok(HostEvent::JobResult { request_id, .. }) if request_id.ends_with(".clear") => {
                notify(
                    request_id.trim_end_matches(".clear"),
                    "Safe Input Key: stored text cleared from OS keychain.",
                );
            }
            Ok(HostEvent::JobError { code, message, .. }) => {
                log(Level::Error, &format!("{code}: {message}"));
            }
            Ok(_) => {}
            Err(error) => log(Level::Warn, &error.to_string()),
        }
        None
    }
}

fn select_setup_text(input: &serde_json::Map<String, Value>, config: &Value) -> Option<String> {
    input
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            config
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        })
        .map(str::to_string)
}

#[cfg(target_family = "wasm")]
fn save_text(request_id: String, text: String) {
    secure_request(format!("{request_id}.save"), "set", secure_args(Some(text)));
}

#[cfg(target_family = "wasm")]
fn secure_request(request_id: String, operation: &str, args: Map<String, Value>) {
    send(request_id, Capability::SecureStore, operation, args);
}

#[cfg(target_family = "wasm")]
fn secure_args(value: Option<String>) -> Map<String, Value> {
    let mut args = map([("service", json!(SERVICE)), ("key", json!(KEY_TEXT))]);
    if let Some(value) = value {
        args.insert("value".to_string(), json!(value));
    }
    args
}

#[cfg(target_family = "wasm")]
fn notify(request_id: &str, message: &str) {
    send(
        format!("{request_id}.notify"),
        Capability::Notify,
        "show",
        map([("message", json!(message))]),
    );
}

#[cfg(target_family = "wasm")]
fn send(request_id: String, capability: Capability, operation: &str, args: Map<String, Value>) {
    if let Err(error) = request(request_id, capability, operation, args) {
        log(Level::Error, &error.to_string());
    }
}

#[cfg(target_family = "wasm")]
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
    use super::select_setup_text;
    use serde_json::{json, Map};

    #[test]
    fn explicit_text_precedes_saved_config_without_accepting_empty_values() {
        let input = Map::from_iter([("text".to_string(), json!("direct"))]);
        assert_eq!(
            select_setup_text(&input, &json!({"text": "saved"})).as_deref(),
            Some("direct")
        );
        assert_eq!(
            select_setup_text(&Map::new(), &json!({"text": "saved"})).as_deref(),
            Some("saved")
        );
        assert_eq!(select_setup_text(&Map::new(), &json!({"text": ""})), None);
    }
}

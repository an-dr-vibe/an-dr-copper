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
const COUNTER_KEY: &str = "session-counter/runs";
#[cfg(target_family = "wasm")]
const ACTION_ID: &str = "increment";

#[cfg(target_family = "wasm")]
struct Component;

#[cfg(target_family = "wasm")]
impl Guest for Component {
    fn init() {
        log(Level::Info, "Session Counter initialized");
    }

    fn shutdown() {}

    fn on_tick(_dt: f32) {}

    fn on_message(_topic: String, sender: String, payload: Vec<u8>) -> Option<Vec<u8>> {
        match decode_host_event(&sender, &payload) {
            Ok(HostEvent::Action {
                request_id,
                action_id,
                ..
            }) if sender == COPPER_ACTIONS_ENDPOINT && action_id == ACTION_ID => {
                send(
                    format!("{request_id}.get"),
                    Capability::Store,
                    "get",
                    map([("key", json!(COUNTER_KEY))]),
                );
            }
            Ok(HostEvent::JobResult {
                request_id, result, ..
            }) if request_id.ends_with(".get") => {
                let next = counter_value(&result).saturating_add(1);
                let action_request = request_id.trim_end_matches(".get");
                send(
                    format!("{action_request}.set.{next}"),
                    Capability::Store,
                    "set",
                    map([("key", json!(COUNTER_KEY)), ("value", json!(next))]),
                );
            }
            Ok(HostEvent::JobResult { request_id, .. }) => {
                if let Some(next) = set_result_count(&request_id) {
                    send(
                        format!("{request_id}.toast"),
                        Capability::Ui,
                        "show",
                        map([(
                            "markup",
                            json!({"type": "toast", "message": format!("Session count: {next}")}),
                        )]),
                    );
                }
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

fn set_result_count(request_id: &str) -> Option<i64> {
    let (_, count) = request_id.rsplit_once(".set.")?;
    count.parse().ok()
}

fn counter_value(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .or_else(|| value.as_str().and_then(|number| number.parse().ok()))
        .unwrap_or_default()
}

#[cfg(target_family = "wasm")]
fn map<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(target_family = "wasm")]
fn send(request_id: String, capability: Capability, operation: &str, args: Map<String, Value>) {
    if let Err(error) = request(request_id, capability, operation, args) {
        log(Level::Error, &error.to_string());
    }
}

#[cfg(target_family = "wasm")]
copper_component_sdk::bindings::export!(
    Component with_types_in copper_component_sdk::bindings
);

#[cfg(test)]
mod tests {
    use super::{counter_value, set_result_count};
    use serde_json::json;

    #[test]
    fn legacy_counter_values_increment_from_numbers_strings_and_null() {
        assert_eq!(counter_value(&json!(4)), 4);
        assert_eq!(counter_value(&json!("7")), 7);
        assert_eq!(counter_value(&json!(null)), 0);
    }

    #[test]
    fn only_store_set_results_advance_to_the_toast() {
        assert_eq!(set_result_count("action-1.set.8"), Some(8));
        assert_eq!(set_result_count("action-1.set.8.toast"), None);
    }
}

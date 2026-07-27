#[cfg(target_family = "wasm")]
use copper_component_sdk::host_api::{log, Level};
#[cfg(target_family = "wasm")]
use copper_component_sdk::{
    decode_host_event, request, Capability, Guest, HostEvent, COPPER_ACTIONS_ENDPOINT,
};
#[cfg(target_family = "wasm")]
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[cfg(target_family = "wasm")]
const ACTION_ID: &str = "sort";
#[cfg(target_family = "wasm")]
const DEFAULT_FOLDER: &str = "~/Downloads";

thread_local! {
    static FOLDERS: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

#[cfg(target_family = "wasm")]
struct Component;

#[cfg(target_family = "wasm")]
impl Guest for Component {
    fn init() {
        log(Level::Info, "Sort Downloads initialized");
    }

    fn shutdown() {
        FOLDERS.with(|folders| folders.borrow_mut().clear());
    }

    fn on_tick(_dt: f32) {}

    fn on_message(_topic: String, sender: String, payload: Vec<u8>) -> Option<Vec<u8>> {
        match decode_host_event(&sender, &payload) {
            Ok(HostEvent::Action {
                request_id,
                action_id,
                input,
            }) if sender == COPPER_ACTIONS_ENDPOINT && action_id == ACTION_ID => {
                let folder = input
                    .get("folder")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_FOLDER)
                    .to_string();
                let list_request = format!("{request_id}.list");
                FOLDERS.with(|folders| {
                    folders
                        .borrow_mut()
                        .insert(list_request.clone(), folder.clone());
                });
                if !send(
                    list_request.clone(),
                    Capability::Fs,
                    "list",
                    map([("path", json!(folder))]),
                ) {
                    forget_folder(&list_request);
                }
            }
            Ok(HostEvent::JobResult {
                request_id, result, ..
            }) if request_id.ends_with(".list") => {
                let folder = take_folder(&request_id).unwrap_or_else(|| DEFAULT_FOLDER.to_string());
                let count = result.as_array().map(Vec::len).unwrap_or_default();
                send(
                    format!("{request_id}.notify"),
                    Capability::Notify,
                    "show",
                    map([("message", json!(format!("Found {count} files in {folder}")))]),
                );
            }
            Ok(HostEvent::JobResult { request_id, .. }) if request_id.ends_with(".notify") => {
                send(
                    format!("{request_id}.toast"),
                    Capability::Ui,
                    "show",
                    map([(
                        "markup",
                        json!({"type": "toast", "message": "Sort Downloads completed"}),
                    )]),
                );
            }
            Ok(HostEvent::JobError {
                request_id,
                code,
                message,
                ..
            }) => {
                if let Some(request_id) = request_id {
                    forget_folder(&request_id);
                }
                log(Level::Error, &format!("{code}: {message}"));
            }
            Ok(_) => {}
            Err(error) => log(Level::Warn, &error.to_string()),
        }
        None
    }
}

fn take_folder(request_id: &str) -> Option<String> {
    FOLDERS.with(|folders| folders.borrow_mut().remove(request_id))
}

fn forget_folder(request_id: &str) {
    FOLDERS.with(|folders| {
        folders.borrow_mut().remove(request_id);
    });
}

#[cfg(target_family = "wasm")]
fn map<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(target_family = "wasm")]
fn send(
    request_id: String,
    capability: Capability,
    operation: &str,
    args: Map<String, Value>,
) -> bool {
    match request(request_id, capability, operation, args) {
        Ok(_) => true,
        Err(error) => {
            log(Level::Error, &error.to_string());
            false
        }
    }
}

#[cfg(target_family = "wasm")]
copper_component_sdk::bindings::export!(
    Component with_types_in copper_component_sdk::bindings
);

#[cfg(test)]
mod tests {
    use super::{forget_folder, take_folder, FOLDERS};

    #[test]
    fn in_flight_folder_state_is_correlated_and_removable() {
        FOLDERS.with(|folders| {
            folders
                .borrow_mut()
                .insert("action-1.list".to_string(), "C:/Downloads".to_string());
        });
        assert_eq!(
            take_folder("action-1.list").as_deref(),
            Some("C:/Downloads")
        );
        forget_folder("missing");
    }
}

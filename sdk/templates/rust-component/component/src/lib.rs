use copper_component_sdk::host_api::{log, Level};
use copper_component_sdk::{
    decode_host_event, request, Capability, Guest, HostEvent, COPPER_ACTIONS_ENDPOINT,
};
use serde_json::Map;

struct Component;

impl Guest for Component {
    fn init() {
        log(Level::Info, "Copper component initialized");
    }

    fn shutdown() {}

    fn on_tick(_dt: f32) {}

    fn on_message(_topic: String, sender: String, payload: Vec<u8>) -> Option<Vec<u8>> {
        match decode_host_event(&sender, &payload) {
            Ok(HostEvent::Action {
                request_id,
                action_id,
                ..
            }) if sender == COPPER_ACTIONS_ENDPOINT && action_id == "run" => {
                if let Err(error) = request(request_id, Capability::Store, "config.get", Map::new())
                {
                    log(Level::Error, &error.to_string());
                }
            }
            Ok(HostEvent::JobResult { .. }) => {
                log(Level::Info, "Copper component action completed");
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

copper_component_sdk::bindings::export!(
    Component with_types_in copper_component_sdk::bindings
);

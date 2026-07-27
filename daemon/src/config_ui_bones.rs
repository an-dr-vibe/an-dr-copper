#[cfg(any(feature = "native-ui", test))]
use super::server::route_request;
#[cfg(feature = "native-ui")]
use super::server::{build_ui_state, refresh_ui_state};
use super::UiConfigError;
#[cfg(any(feature = "native-ui", test))]
use super::UiServerState;
#[cfg(feature = "native-ui")]
use super::UiTransport;
#[cfg(any(feature = "native-ui", test))]
use crate::config_ui_http::{HttpMethod, HttpRequest};
#[cfg(feature = "native-ui")]
use crate::control_plane::ControlPlaneAuth;
#[cfg(any(feature = "native-ui", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "native-ui", test))]
use serde_json::Value;
#[cfg(any(feature = "native-ui", test))]
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};

pub(crate) const COPPER_SETTINGS_PROTOCOL_V1: &str = "copper.settings/1";
#[cfg(feature = "native-ui")]
const SETTINGS_ENDPOINT: &str = "copper-settings";
#[cfg(feature = "native-ui")]
const SETTINGS_PANEL: &str = "main";

#[derive(Debug, Clone)]
pub struct SettingsUiHandle {
    sender: Sender<Option<String>>,
}

impl SettingsUiHandle {
    pub fn open(&self, extension_id: Option<&str>) -> Result<(), UiConfigError> {
        self.sender
            .send(extension_id.map(str::to_string))
            .map_err(|_| UiConfigError::Window("settings UI is unavailable".to_string()))
    }
}

pub(crate) fn settings_ui_channel() -> (SettingsUiHandle, Receiver<Option<String>>) {
    let (sender, receiver) = mpsc::channel();
    (SettingsUiHandle { sender }, receiver)
}

#[cfg(test)]
mod channel_tests {
    use super::settings_ui_channel;

    #[test]
    fn settings_handle_preserves_requested_extension_selection() {
        let (handle, requests) = settings_ui_channel();
        handle.open(None).expect("core request");
        handle
            .open(Some("desktop-torrent-organizer"))
            .expect("extension request");

        assert_eq!(requests.recv().expect("core selection"), None);
        assert_eq!(
            requests.recv().expect("extension selection").as_deref(),
            Some("desktop-torrent-organizer")
        );
    }
}

#[cfg(any(feature = "native-ui", test))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsRequest {
    protocol: String,
    request_id: String,
    method: String,
    path: String,
    #[serde(default)]
    body: Option<Value>,
}

#[cfg(any(feature = "native-ui", test))]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsResponse {
    protocol: &'static str,
    request_id: String,
    ok: bool,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[cfg(any(feature = "native-ui", test))]
pub(crate) fn dispatch_bones_request(state: &UiServerState, json: &str) -> String {
    let request_id = serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|value| value.get("requestId")?.as_str().map(str::to_string))
        .unwrap_or_default();
    let response = match serde_json::from_str::<SettingsRequest>(json) {
        Ok(request) => dispatch_request(state, request),
        Err(error) => {
            SettingsResponse::error(request_id, format!("invalid settings request: {error}"))
        }
    };
    serde_json::to_string(&response).unwrap_or_else(|_| {
        r#"{"protocol":"copper.settings/1","requestId":"","ok":false,"status":500,"error":"failed to encode settings response"}"#.to_string()
    })
}

#[cfg(any(feature = "native-ui", test))]
fn dispatch_request(state: &UiServerState, request: SettingsRequest) -> SettingsResponse {
    if request.protocol != COPPER_SETTINGS_PROTOCOL_V1 {
        return SettingsResponse::error(
            request.request_id,
            format!("unsupported settings protocol '{}'", request.protocol),
        );
    }
    let method = match request.method.as_str() {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        other => {
            return SettingsResponse::error(
                request.request_id,
                format!("unsupported settings method '{other}'"),
            )
        }
    };
    let body = request
        .body
        .map(|body| serde_json::to_vec(&body))
        .transpose();
    let body = match body {
        Ok(Some(body)) => body,
        Ok(None) => Vec::new(),
        Err(error) => {
            return SettingsResponse::error(
                request.request_id,
                format!("invalid settings body: {error}"),
            )
        }
    };
    let route = HttpRequest {
        method,
        path: request.path,
        headers: HashMap::new(),
        body,
    };
    match route_request(state, &route) {
        Ok((response, _)) => {
            let data = if response.body.is_empty() {
                None
            } else {
                serde_json::from_slice(&response.body).ok()
            };
            if response.status < 400 {
                SettingsResponse {
                    protocol: COPPER_SETTINGS_PROTOCOL_V1,
                    request_id: request.request_id,
                    ok: true,
                    status: response.status,
                    data,
                    error: None,
                }
            } else {
                let error = data
                    .as_ref()
                    .and_then(|value| value.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("settings request failed")
                    .to_string();
                SettingsResponse {
                    protocol: COPPER_SETTINGS_PROTOCOL_V1,
                    request_id: request.request_id,
                    ok: false,
                    status: response.status,
                    data: None,
                    error: Some(error),
                }
            }
        }
        Err(error) => SettingsResponse::error(request.request_id, error.to_string()),
    }
}

#[cfg(any(feature = "native-ui", test))]
impl SettingsResponse {
    fn error(request_id: String, error: String) -> Self {
        Self {
            protocol: COPPER_SETTINGS_PROTOCOL_V1,
            request_id,
            ok: false,
            status: 400,
            data: None,
            error: Some(error),
        }
    }
}

#[cfg(feature = "native-ui")]
mod native {
    use super::*;
    use bones_bus::{
        Bus, Envelope, Handler, Module, ModuleContext, ModuleRegistration, Registry,
        ServiceRegistry,
    };
    use bones_logging::Logger;
    use bones_messages::web::{
        Command, OpenPanel, PageMessage, PanelSource, SendJson, ENDPOINT as WEB_ENDPOINT,
    };
    use bones_messages::{DecodeMessage, Message};
    use bones_web::WryPresentation;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    pub struct NativeSettingsPresentation {
        web: WryPresentation,
        settings: ModuleRegistration,
        close_requested: Arc<AtomicBool>,
    }

    impl NativeSettingsPresentation {
        pub fn open(
            bus: Bus,
            registry: Registry,
            extensions_dir: &Path,
            selected_extension_id: Option<&str>,
        ) -> Result<Self, UiConfigError> {
            let mut state = build_ui_state(
                extensions_dir,
                selected_extension_id,
                true,
                ControlPlaneAuth::ephemeral(),
                "bones://settings".to_string(),
            )?;
            state.transport = UiTransport::Bones;
            let html = super::super::render::render_html(&state);
            let close_requested = Arc::new(AtomicBool::new(false));
            let mut services = ServiceRegistry::new();
            let settings = ModuleRegistration::attach(
                bus.clone(),
                registry.clone(),
                &mut services,
                SettingsModule {
                    registry: registry.clone(),
                    state,
                    close_requested: Arc::clone(&close_requested),
                },
            )
            .map_err(UiConfigError::Window)?;
            let mut web = match WryPresentation::open(
                bus,
                registry.clone(),
                Logger::default(),
                "Copper Settings",
                1100,
                760,
            ) {
                Ok(web) => web,
                Err(error) => return Err(UiConfigError::Window(error)),
            };
            if let Err(error) = registry.call(
                SETTINGS_ENDPOINT,
                WEB_ENDPOINT,
                &Command::Open(OpenPanel {
                    panel: SETTINGS_PANEL,
                    source: PanelSource::Html(&html),
                })
                .encode(),
            ) {
                web.close();
                return Err(UiConfigError::Window(format!(
                    "failed to open settings panel: {error:?}"
                )));
            }
            Ok(Self {
                web,
                settings,
                close_requested,
            })
        }

        pub fn update(&mut self) -> bool {
            self.web.update() || self.close_requested.load(Ordering::Relaxed)
        }

        pub fn close(&mut self) {
            self.web.close();
            self.settings.detach();
        }
    }

    impl Drop for NativeSettingsPresentation {
        fn drop(&mut self) {
            self.close();
        }
    }

    struct SettingsModule {
        registry: Registry,
        state: UiServerState,
        close_requested: Arc<AtomicBool>,
    }

    impl Handler for SettingsModule {
        fn handle(&mut self, envelope: &Envelope) {
            if envelope.topic != PageMessage::TOPIC {
                return;
            }
            let Ok(message) = PageMessage::decode(&envelope.payload) else {
                return;
            };
            if envelope.sender != WEB_ENDPOINT
                || message.owner != SETTINGS_ENDPOINT
                || message.panel != SETTINGS_PANEL
            {
                return;
            }
            let close = serde_json::from_str::<SettingsRequest>(message.json)
                .map(|request| {
                    request.protocol == COPPER_SETTINGS_PROTOCOL_V1
                        && request.method == "POST"
                        && request.path == "/close"
                })
                .unwrap_or(false);
            if let Err(error) = refresh_ui_state(&mut self.state) {
                crate::logging::error(format!(
                    "failed to refresh settings catalog before request: {error}"
                ));
            }
            let response = dispatch_bones_request(&self.state, message.json);
            let _ = self.registry.call(
                SETTINGS_ENDPOINT,
                WEB_ENDPOINT,
                &Command::SendJson(SendJson {
                    panel: SETTINGS_PANEL,
                    json: &response,
                })
                .encode(),
            );
            let succeeded = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|response| response.get("ok").and_then(Value::as_bool))
                .unwrap_or(false);
            if close && succeeded {
                self.close_requested.store(true, Ordering::Relaxed);
            }
        }
    }

    impl Module for SettingsModule {
        fn name(&self) -> &str {
            SETTINGS_ENDPOINT
        }

        fn init(&mut self, context: &mut ModuleContext) -> Result<(), String> {
            context.subscribe(PageMessage::TOPIC);
            Ok(())
        }
    }

    pub fn open_in_native_window(
        extensions_dir: &Path,
        selected_extension_id: Option<&str>,
    ) -> Result<(), UiConfigError> {
        let bus = Bus::new();
        let registry = Registry::new();
        let mut presentation = NativeSettingsPresentation::open(
            bus.clone(),
            registry,
            extensions_dir,
            selected_extension_id,
        )?;
        while !presentation.update() {
            bus.begin_frame();
            bus.dispatch();
            std::thread::sleep(Duration::from_millis(8));
        }
        presentation.close();
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bones_bus::Respond;
        use bones_messages::{EncodeMessage, Message};
        use std::sync::Mutex;
        use tempfile::tempdir;

        #[derive(Default)]
        struct Capture {
            payload: Mutex<Option<Vec<u8>>>,
        }

        impl Respond for Capture {
            fn respond(&self, _sender: &str, payload: &[u8]) -> Option<Vec<u8>> {
                *self.payload.lock().expect("capture") = Some(payload.to_vec());
                Some(Vec::new())
            }
        }

        #[test]
        fn settings_module_accepts_only_its_owner_and_returns_correlated_json() {
            let temp = tempdir().expect("tempdir");
            let mut state = build_ui_state(
                temp.path(),
                None,
                true,
                ControlPlaneAuth::ephemeral(),
                "bones://settings".to_string(),
            )
            .expect("state");
            state.state_store =
                crate::state_store::ExtensionStateStore::new(temp.path().join("state"));
            state.transport = UiTransport::Bones;
            let bus = Bus::new();
            let registry = Registry::new();
            let capture = Arc::new(Capture::default());
            registry.insert(WEB_ENDPOINT, capture.clone());
            let close_requested = Arc::new(AtomicBool::new(false));
            let mut services = ServiceRegistry::new();
            let _settings = ModuleRegistration::attach(
                bus.clone(),
                registry.clone(),
                &mut services,
                SettingsModule {
                    registry,
                    state,
                    close_requested,
                },
            )
            .expect("settings module");

            let request = r#"{"protocol":"copper.settings/1","requestId":"request-4","method":"GET","path":"/config/core"}"#;
            bus.publish(Envelope {
                topic: PageMessage::TOPIC.to_string(),
                sender: WEB_ENDPOINT.to_string(),
                correlation: None,
                payload: PageMessage {
                    owner: "another-owner",
                    panel: SETTINGS_PANEL,
                    json: request,
                }
                .encode(),
            });
            bus.dispatch();
            assert!(capture.payload.lock().expect("capture").is_none());

            bus.publish(Envelope {
                topic: PageMessage::TOPIC.to_string(),
                sender: "forged-extension".to_string(),
                correlation: None,
                payload: PageMessage {
                    owner: SETTINGS_ENDPOINT,
                    panel: SETTINGS_PANEL,
                    json: request,
                }
                .encode(),
            });
            bus.dispatch();
            assert!(capture.payload.lock().expect("capture").is_none());

            bus.publish(Envelope {
                topic: PageMessage::TOPIC.to_string(),
                sender: WEB_ENDPOINT.to_string(),
                correlation: None,
                payload: PageMessage {
                    owner: SETTINGS_ENDPOINT,
                    panel: SETTINGS_PANEL,
                    json: request,
                }
                .encode(),
            });
            bus.dispatch();
            let payload = capture
                .payload
                .lock()
                .expect("capture")
                .clone()
                .expect("response");
            let Command::SendJson(response) =
                Command::decode(&payload).expect("web response command")
            else {
                panic!("expected send-json response");
            };
            let response: Value = serde_json::from_str(response.json).expect("response json");
            assert_eq!(response["requestId"], "request-4");
            assert_eq!(response["ok"], true);
        }
    }
}

#[cfg(feature = "native-ui")]
pub use native::{open_in_native_window, NativeSettingsPresentation};

#[cfg(not(feature = "native-ui"))]
pub fn open_in_native_window(
    _extensions_dir: &Path,
    _selected_extension_id: Option<&str>,
) -> Result<(), UiConfigError> {
    Err(UiConfigError::Window(
        "native UI support is not available in this build".to_string(),
    ))
}

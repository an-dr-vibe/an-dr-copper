use crate::autostart;
#[cfg(test)]
use crate::config_ui_http::{parse_request, write_response, HttpMethod, HttpRequest, HttpResponse};
#[cfg(test)]
use crate::config_ui_service::build_extension_info;
use crate::descriptor::Descriptor;
use crate::host_extensions::HostExtensionRegistry;
use crate::state_store::ExtensionStateStore;
use std::collections::HashSet;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;
use thiserror::Error;

#[path = "config_ui_browser.rs"]
mod browser;
#[path = "config_ui_render.rs"]
mod render;
#[path = "config_ui_server.rs"]
mod server;
#[cfg(test)]
#[path = "config_ui_test_support.rs"]
mod test_support;
#[path = "config_ui_window.rs"]
mod window;

#[cfg(test)]
use server::{
    build_ui_state, find_discoverable_descriptor, parse_json_object, visible_descriptors,
};
pub(crate) use server::{open_extension_config, start_daemon_ui_server};
pub use window::open_in_native_window;
pub(crate) use window::open_url_in_native_window_detached;

#[cfg(test)]
use render::render_html;
#[cfg(test)]
use test_support::{
    core_data_path_for, extension_config_path_for, extension_status_path_for, load_config,
    store_config,
};

const DEFAULT_UI_BIND: &str = "127.0.0.1:0";
pub const DEFAULT_DAEMON_UI_BIND: &str = "127.0.0.1:4766";

#[derive(Debug, Clone)]
pub struct UiOpenOptions {
    pub bind_addr: String,
    pub open_browser: bool,
    pub open_window: bool,
    pub idle_timeout: Duration,
}

impl Default for UiOpenOptions {
    fn default() -> Self {
        Self {
            bind_addr: DEFAULT_UI_BIND.to_string(),
            open_browser: false,
            open_window: true,
            idle_timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Error)]
pub enum UiConfigError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    AutoStart(#[from] autostart::AutoStartError),
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
    #[error("failed to open native window: {0}")]
    Window(String),
}

#[derive(Debug, Clone)]
pub(crate) struct UiServerState {
    pub(crate) selected_extension_id: String,
    pub(crate) descriptors: Vec<Descriptor>,
    pub(crate) discoverable_descriptors: Vec<Descriptor>,
    pub(crate) core_extension_ids: HashSet<String>,
    pub(crate) extension_ids: HashSet<String>,
    pub(crate) user_extensions_dir: PathBuf,
    pub(crate) core_extensions_dir: Option<PathBuf>,
    pub(crate) runtime_extension_roots: Vec<PathBuf>,
    pub(crate) state_store: ExtensionStateStore,
    pub(crate) host_extensions: HostExtensionRegistry,
    pub(crate) auth_token: String,
    pub(crate) origin: String,
    pub(crate) allow_close: bool,
}

pub struct PersistentUiServer {
    pub url: String,
    _thread: JoinHandle<()>,
}

#[cfg(test)]
mod tests {
    include!("config_ui_tests.rs");
}

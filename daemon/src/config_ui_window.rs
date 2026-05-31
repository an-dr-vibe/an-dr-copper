use super::UiConfigError;
#[cfg(all(feature = "native-ui", not(test)))]
use std::process::{Command, Stdio};

#[cfg(all(feature = "native-ui", not(test)))]
pub fn open_in_native_window(url: &str) -> Result<(), UiConfigError> {
    let target_url = tauri::WebviewUrl::External(
        url.parse()
            .map_err(|err| UiConfigError::Window(format!("invalid URL '{url}': {err}")))?,
    );

    tauri::Builder::default()
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(app, "copper-settings", target_url.clone())
                .title("Copper Settings")
                .inner_size(1100.0, 760.0)
                .min_inner_size(720.0, 520.0)
                .resizable(true)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!("tauri.conf.json"))
        .map_err(|err| UiConfigError::Window(err.to_string()))
}

#[cfg(all(feature = "native-ui", not(test)))]
pub(crate) fn open_url_in_native_window_detached(url: &str) -> Result<(), UiConfigError> {
    let exe = std::env::current_exe().map_err(UiConfigError::Io)?;
    Command::new(exe)
        .args(["internal", "native-window", "--url", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| UiConfigError::Window(err.to_string()))
}

#[cfg(any(not(feature = "native-ui"), test))]
pub fn open_in_native_window(_url: &str) -> Result<(), UiConfigError> {
    Err(UiConfigError::Window(
        "native UI support is not available in this build".to_string(),
    ))
}

#[cfg(any(not(feature = "native-ui"), test))]
pub(crate) fn open_url_in_native_window_detached(_url: &str) -> Result<(), UiConfigError> {
    Err(UiConfigError::Window(
        "native UI support is not available in this build".to_string(),
    ))
}

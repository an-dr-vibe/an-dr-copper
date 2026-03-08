use crate::config_ui::{open_extension_config, UiOpenOptions};
use crate::extension::load_runtime_registry;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use thiserror::Error;
use tray_item::{IconSource, TrayItem};

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("tray initialization failed: {0}")]
    Init(String),
}

pub struct TrayController {
    _inner: TrayItem,
}

impl TrayController {
    pub fn initialize(
        running: Arc<AtomicBool>,
        extensions_dir: PathBuf,
    ) -> Result<Self, TrayError> {
        let mut tray = TrayItem::new("Copperd (Running)", default_icon())
            .map_err(|e| TrayError::Init(e.to_string()))?;
        tray.add_label("Daemon is running")
            .map_err(|e| TrayError::Init(e.to_string()))?;

        let ui_extensions_dir = extensions_dir;
        tray.add_menu_item("Open Extension Config", move || {
            let ui_dir = ui_extensions_dir.clone();
            std::thread::spawn(move || {
                let selected_extension = match select_extension_for_config(&ui_dir) {
                    Ok(Some(id)) => id,
                    Ok(None) => {
                        eprintln!(
                            "no extensions found in {} (add extension folders with descriptor.json + main.ts)",
                            ui_dir.display()
                        );
                        return;
                    }
                    Err(err) => {
                        eprintln!("failed to read extensions for config UI: {err}");
                        return;
                    }
                };

                let options = UiOpenOptions {
                    bind_addr: "127.0.0.1:0".to_string(),
                    open_browser: true,
                    idle_timeout: Duration::from_secs(300),
                };
                if let Err(err) = open_extension_config(&ui_dir, &selected_extension, options) {
                    eprintln!("failed to open config UI: {err}");
                }
            });
        })
        .map_err(|e| TrayError::Init(e.to_string()))?;

        let exit_signal = Arc::clone(&running);
        tray.add_menu_item("Exit", move || {
            exit_signal.store(false, Ordering::Relaxed);
        })
        .map_err(|e| TrayError::Init(e.to_string()))?;

        Ok(Self { _inner: tray })
    }
}

fn select_extension_for_config(extensions_dir: &Path) -> Result<Option<String>, TrayError> {
    let registry = load_runtime_registry(extensions_dir).map_err(|err| {
        TrayError::Init(format!(
            "failed loading runtime extension registry from {}: {err}",
            extensions_dir.display()
        ))
    })?;

    if registry.get("desktop-torrent-organizer").is_some() {
        return Ok(Some("desktop-torrent-organizer".to_string()));
    }

    let first = registry.list().next().map(|ext| ext.descriptor.id.clone());
    Ok(first)
}

#[cfg(target_os = "windows")]
fn default_icon() -> IconSource {
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION};
        let icon_handle = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        IconSource::RawIcon(icon_handle as isize)
    }
}

#[cfg(not(target_os = "windows"))]
fn default_icon() -> IconSource {
    IconSource::Data {
        width: 16,
        height: 16,
        data: solid_green_icon_rgba(16, 16),
    }
}

#[cfg(not(target_os = "windows"))]
fn solid_green_icon_rgba(width: usize, height: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * 4);
    for _ in 0..(width * height) {
        data.extend_from_slice(&[37, 178, 82, 255]);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::select_extension_for_config;
    use std::fs;
    use tempfile::tempdir;

    fn write_extension(root: &std::path::Path, id: &str) {
        let ext_dir = root.join(id);
        fs::create_dir_all(&ext_dir).expect("create extension dir");
        fs::write(
            ext_dir.join("descriptor.json"),
            format!(
                r#"{{
                "$schema":"https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id":"{id}",
                "name":"{id}",
                "version":"1.0.0",
                "trigger":"test",
                "actions":[{{"id":"run","label":"Run","script":"return;"}}]
            }}"#
            ),
        )
        .expect("write descriptor");
        fs::write(
            ext_dir.join("main.ts"),
            "export default function(){ return {}; }",
        )
        .expect("write main.ts");
    }

    #[test]
    fn chooses_desktop_torrent_organizer_when_available() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha");
        write_extension(temp.path(), "desktop-torrent-organizer");

        let selected = select_extension_for_config(temp.path()).expect("selection");
        assert_eq!(selected.as_deref(), Some("desktop-torrent-organizer"));
    }

    #[test]
    fn chooses_first_available_extension_when_desktop_extension_missing() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha");
        write_extension(temp.path(), "zeta");

        let selected = select_extension_for_config(temp.path()).expect("selection");
        assert!(
            matches!(
                selected.as_deref(),
                Some("alpha") | Some("desktop-torrent-organizer")
            ),
            "unexpected extension selection: {selected:?}"
        );
    }
}

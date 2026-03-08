use crate::config_ui::open_url_in_browser;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
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

trait TrayOps {
    fn add_label(&mut self, label: &str) -> Result<(), String>;
    fn add_menu_item<F>(&mut self, label: &str, callback: F) -> Result<(), String>
    where
        F: Fn() + Send + Sync + 'static;
}

struct RealTray {
    inner: TrayItem,
}

impl TrayOps for RealTray {
    fn add_label(&mut self, label: &str) -> Result<(), String> {
        self.inner.add_label(label).map_err(|e| e.to_string())
    }

    fn add_menu_item<F>(&mut self, label: &str, callback: F) -> Result<(), String>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.inner
            .add_menu_item(label, callback)
            .map_err(|e| e.to_string())
    }
}

fn configure_tray<T, F>(
    tray: &mut T,
    running: Arc<AtomicBool>,
    ui_url: String,
    open_browser: F,
) -> Result<(), TrayError>
where
    T: TrayOps,
    F: Fn(&str) -> Result<(), crate::config_ui::UiConfigError> + Send + Sync + 'static,
{
    tray.add_label("Daemon is running")
        .map_err(TrayError::Init)?;

    let ui_url_for_menu = ui_url;
    tray.add_menu_item("Open Extension Config", move || {
        if let Err(err) = open_browser(&ui_url_for_menu) {
            eprintln!("failed to open config UI in browser: {err}");
        }
    })
    .map_err(TrayError::Init)?;

    let exit_signal = Arc::clone(&running);
    tray.add_menu_item("Exit", move || {
        exit_signal.store(false, Ordering::Relaxed);
    })
    .map_err(TrayError::Init)?;

    Ok(())
}

impl TrayController {
    pub fn initialize(
        running: Arc<AtomicBool>,
        _extensions_dir: PathBuf,
        ui_url: String,
    ) -> Result<Self, TrayError> {
        let inner = TrayItem::new("Copperd (Running)", default_icon())
            .map_err(|e| TrayError::Init(e.to_string()))?;
        let mut tray = RealTray { inner };
        configure_tray(&mut tray, running, ui_url, open_url_in_browser)?;
        Ok(Self { _inner: tray.inner })
    }
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
    use super::{configure_tray, default_icon, TrayController, TrayOps};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[derive(Default)]
    struct FakeTray {
        labels: Vec<String>,
        items: Vec<(String, Box<dyn Fn() + Send + Sync + 'static>)>,
        fail_label: bool,
        fail_menu: bool,
    }

    impl TrayOps for FakeTray {
        fn add_label(&mut self, label: &str) -> Result<(), String> {
            if self.fail_label {
                return Err("label failed".to_string());
            }
            self.labels.push(label.to_string());
            Ok(())
        }

        fn add_menu_item<F>(&mut self, label: &str, callback: F) -> Result<(), String>
        where
            F: Fn() + Send + Sync + 'static,
        {
            if self.fail_menu {
                return Err("menu failed".to_string());
            }
            self.items.push((label.to_string(), Box::new(callback)));
            Ok(())
        }
    }

    #[test]
    fn default_icon_is_constructible() {
        let _ = default_icon();
    }

    #[test]
    fn configure_tray_registers_actions_and_exit_callback() {
        let mut tray = FakeTray::default();
        let running = Arc::new(AtomicBool::new(true));
        let opened = Arc::new(AtomicBool::new(false));
        let opened_signal = Arc::clone(&opened);

        configure_tray(
            &mut tray,
            Arc::clone(&running),
            "http://127.0.0.1:4766".to_string(),
            move |_url| {
                opened_signal.store(true, Ordering::Relaxed);
                Ok(())
            },
        )
        .expect("configure tray");

        assert_eq!(tray.labels, vec!["Daemon is running".to_string()]);
        assert_eq!(tray.items.len(), 2);

        let open_item = tray
            .items
            .iter()
            .find(|(label, _)| label == "Open Extension Config")
            .expect("open action");
        (open_item.1)();
        assert!(opened.load(Ordering::Relaxed));

        let exit_item = tray
            .items
            .iter()
            .find(|(label, _)| label == "Exit")
            .expect("exit action");
        (exit_item.1)();
        assert!(!running.load(Ordering::Relaxed));
    }

    #[test]
    fn configure_tray_maps_backend_errors() {
        let running = Arc::new(AtomicBool::new(true));
        let mut label_fail = FakeTray {
            fail_label: true,
            ..FakeTray::default()
        };
        let err = configure_tray(
            &mut label_fail,
            Arc::clone(&running),
            "http://127.0.0.1:4766".to_string(),
            |_url| Ok(()),
        )
        .expect_err("label failure");
        assert!(err.to_string().contains("label failed"));

        let mut menu_fail = FakeTray {
            fail_menu: true,
            ..FakeTray::default()
        };
        let err = configure_tray(
            &mut menu_fail,
            Arc::clone(&running),
            "http://127.0.0.1:4766".to_string(),
            |_url| Ok(()),
        )
        .expect_err("menu failure");
        assert!(err.to_string().contains("menu failed"));
    }

    #[test]
    fn initialize_returns_result() {
        let running = Arc::new(AtomicBool::new(true));
        let _ = TrayController::initialize(
            running,
            std::path::PathBuf::from("."),
            "http://127.0.0.1:4766".to_string(),
        );
    }
}

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

impl TrayController {
    pub fn initialize(running: Arc<AtomicBool>) -> Result<Self, TrayError> {
        let mut tray = TrayItem::new("Copperd (Running)", default_icon())
            .map_err(|e| TrayError::Init(e.to_string()))?;
        tray.add_label("Daemon is running")
            .map_err(|e| TrayError::Init(e.to_string()))?;

        let exit_signal = Arc::clone(&running);
        tray.add_menu_item("Exit", move || {
            exit_signal.store(false, Ordering::Relaxed);
        })
        .map_err(|e| TrayError::Init(e.to_string()))?;

        Ok(Self { _inner: tray })
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

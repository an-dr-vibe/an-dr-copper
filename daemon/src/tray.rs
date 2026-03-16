use crate::config_ui::open_url_in_browser;
use crate::logging;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use thiserror::Error;

#[cfg(not(windows))]
use crate::tray_assets;
#[cfg(not(windows))]
use tray_item::{IconSource, TrayItem};

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("tray initialization failed: {0}")]
    Init(String),
}

pub struct TrayController {
    #[cfg(windows)]
    _inner: WindowsTrayHandle,
    #[cfg(not(windows))]
    _inner: TrayItem,
}

#[cfg(not(windows))]
trait TrayOps {
    fn add_label(&mut self, label: &str) -> Result<(), String>;
    fn add_menu_item<F>(&mut self, label: &str, callback: F) -> Result<(), String>
    where
        F: Fn() + Send + Sync + 'static;
}

#[cfg(not(windows))]
struct RealTray {
    inner: TrayItem,
}

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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
    tray.add_menu_item("Open Copper UI", move || {
        if let Err(err) = open_browser(&ui_url_for_menu) {
            logging::error(format!("failed to open config UI in browser: {err}"));
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

#[cfg(not(windows))]
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

#[cfg(windows)]
impl TrayController {
    pub fn initialize(
        running: Arc<AtomicBool>,
        _extensions_dir: PathBuf,
        ui_url: String,
    ) -> Result<Self, TrayError> {
        Ok(Self {
            _inner: WindowsTrayHandle::start(running, ui_url).map_err(TrayError::Init)?,
        })
    }
}

#[cfg(not(windows))]
fn default_icon() -> IconSource {
    match tray_assets::copper_server_icon() {
        Ok(icon) => IconSource::Data {
            width: icon.width as i32,
            height: icon.height as i32,
            data: icon.rgba,
        },
        Err(err) => {
            logging::error(format!("failed to render embedded server tray icon: {err}"));
            IconSource::Data {
                width: 16,
                height: 16,
                data: solid_green_icon_rgba(16, 16),
            }
        }
    }
}

#[cfg(not(windows))]
fn solid_green_icon_rgba(width: usize, height: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * 4);
    for _ in 0..(width * height) {
        data.extend_from_slice(&[37, 178, 82, 255]);
    }
    data
}

#[cfg(windows)]
struct WindowsTrayHandle {
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl WindowsTrayHandle {
    fn start(running: Arc<AtomicBool>, ui_url: String) -> Result<Self, String> {
        let thread = std::thread::Builder::new()
            .name("tray-main".to_string())
            .spawn(move || {
                if let Err(err) = run_windows_tray(running, ui_url) {
                    logging::error(format!("main tray error: {err}"));
                }
            })
            .map_err(|err| err.to_string())?;
        Ok(Self {
            thread: Some(thread),
        })
    }
}

#[cfg(windows)]
impl Drop for WindowsTrayHandle {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use crate::tray_assets;
    use std::mem;
    use std::ptr;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
        DestroyWindow, DispatchMessageW, GetCursorPos, LoadIconW, PeekMessageW, PostQuitMessage,
        RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON,
        IDI_APPLICATION, MF_SEPARATOR, MF_STRING, MSG, PM_REMOVE, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
        TPM_LEFTBUTTON, TPM_RETURNCMD, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP,
        WM_USER, WNDCLASSW,
    };

    const WM_TRAYICON: u32 = WM_USER + 122;
    const CMD_OPEN_UI: u32 = 1001;
    const CMD_EXIT: u32 = 1002;
    const TRAY_LOOP_SLEEP: Duration = Duration::from_millis(20);

    static mut WINDOWS_TRAY_STATE: *mut WindowsTrayState = ptr::null_mut();

    struct WindowsTrayState {
        hwnd: HWND,
        running: Arc<AtomicBool>,
        ui_url: String,
        icon: HICON,
    }

    pub(super) fn run_windows_tray(running: Arc<AtomicBool>, ui_url: String) -> Result<(), String> {
        let class_name = wide("CopperDaemonTray");
        let hmodule = unsafe { GetModuleHandleW(ptr::null()) };
        if hmodule.is_null() {
            return Err("failed to acquire module handle".to_string());
        }

        let mut wnd_class = unsafe { mem::zeroed::<WNDCLASSW>() };
        wnd_class.lpfnWndProc = Some(wnd_proc);
        wnd_class.lpszClassName = class_name.as_ptr();
        unsafe {
            RegisterClassW(&wnd_class);
        }

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("copper-daemon-tray-window").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                hmodule,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("failed to create hidden tray window".to_string());
        }

        let mut state = WindowsTrayState {
            hwnd,
            running,
            ui_url,
            icon: load_tray_icon(),
        };
        unsafe {
            WINDOWS_TRAY_STATE = &mut state;
        }

        add_notify_icon(state.hwnd, state.icon, "Copperd (Running)")?;
        let mut msg = unsafe { mem::zeroed::<MSG>() };

        while state.running.load(Ordering::Relaxed) {
            loop {
                let has_message =
                    unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) };
                if has_message == 0 {
                    break;
                }
                if msg.message == WM_QUIT {
                    state.running.store(false, Ordering::Relaxed);
                    break;
                }
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(TRAY_LOOP_SLEEP);
        }

        remove_notify_icon(state.hwnd).ok();
        unsafe {
            WINDOWS_TRAY_STATE = ptr::null_mut();
            DestroyWindow(state.hwnd);
        }
        if !state.icon.is_null() {
            unsafe {
                DestroyIcon(state.icon);
            }
        }
        Ok(())
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TRAYICON => {
                let event = l_param as u32;
                if event == WM_LBUTTONUP {
                    if let Some(state) = state_mut() {
                        open_ui(state);
                    }
                    return 0;
                }
                if event == WM_RBUTTONUP {
                    if let Some(state) = state_mut() {
                        show_context_menu(state);
                    }
                    return 0;
                }
            }
            WM_CLOSE => {
                if let Some(state) = state_mut() {
                    state.running.store(false, Ordering::Relaxed);
                }
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }

        DefWindowProcW(hwnd, msg, w_param, l_param)
    }

    fn open_ui(state: &WindowsTrayState) {
        if let Err(err) = open_url_in_browser(&state.ui_url) {
            logging::error(format!("failed to open Copper UI in browser: {err}"));
        }
    }

    fn show_context_menu(state: &mut WindowsTrayState) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                CMD_OPEN_UI as usize,
                wide("Open Copper UI").as_ptr(),
            )
        };
        let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) };
        let _ = unsafe { AppendMenuW(menu, MF_STRING, CMD_EXIT as usize, wide("Exit").as_ptr()) };

        let mut point = POINT { x: 0, y: 0 };
        unsafe {
            GetCursorPos(&mut point);
            SetForegroundWindow(state.hwnd);
        }
        let cmd = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_LEFTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                point.x,
                point.y,
                0,
                state.hwnd,
                ptr::null(),
            )
        };

        if cmd > 0 {
            handle_menu_command(state, cmd as u32);
        }

        unsafe {
            DestroyMenu(menu);
        }
    }

    fn handle_menu_command(state: &mut WindowsTrayState, command_id: u32) {
        match command_id {
            CMD_OPEN_UI => open_ui(state),
            CMD_EXIT => state.running.store(false, Ordering::Relaxed),
            _ => {}
        }
    }

    fn add_notify_icon(hwnd: HWND, icon: HICON, tooltip: &str) -> Result<(), String> {
        let mut nid = unsafe { mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = icon;

        let wide_tooltip = wide(tooltip);
        let copy_len = wide_tooltip.len().min(nid.szTip.len()) - 1;
        nid.szTip[..copy_len].copy_from_slice(&wide_tooltip[..copy_len]);

        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
        if added == 0 {
            return Err("failed to add Copper tray icon".to_string());
        }
        Ok(())
    }

    fn remove_notify_icon(hwnd: HWND) -> Result<(), String> {
        let mut nid = unsafe { mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON;

        let removed = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
        if removed == 0 {
            return Err("failed to remove Copper tray icon".to_string());
        }
        Ok(())
    }

    fn load_tray_icon() -> HICON {
        if let Ok(icon) = tray_assets::copper_server_icon() {
            if let Some(hicon) = tray_assets::create_hicon(&icon) {
                return hicon as HICON;
            }
            logging::error("failed to convert embedded server tray icon to HICON".to_string());
        }
        unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) as HICON }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn state_mut() -> Option<&'static mut WindowsTrayState> {
        unsafe { WINDOWS_TRAY_STATE.as_mut() }
    }
}

#[cfg(windows)]
use windows_impl::run_windows_tray;

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::TrayController;
    #[cfg(not(windows))]
    use super::{configure_tray, default_icon, TrayOps};
    #[cfg(not(windows))]
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[cfg(not(windows))]
    #[derive(Default)]
    struct FakeTray {
        labels: Vec<String>,
        items: Vec<(String, Box<dyn Fn() + Send + Sync + 'static>)>,
        fail_label: bool,
        fail_menu: bool,
    }

    #[cfg(not(windows))]
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

    #[cfg(not(windows))]
    #[test]
    fn default_icon_is_constructible() {
        let _ = default_icon();
    }

    #[cfg(not(windows))]
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
            .find(|(label, _)| label == "Open Copper UI")
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

    #[cfg(not(windows))]
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

    #[cfg(not(windows))]
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

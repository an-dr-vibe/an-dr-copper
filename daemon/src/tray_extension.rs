use crate::api::windows_display;
use crate::config_ui::open_url_in_browser;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use thiserror::Error;

const WINDOWS_DISPLAY_EXTENSION_ID: &str = "windows-display-manager";

#[derive(Debug, Clone)]
pub struct AdditionalTrayIconSpec {
    pub extension_id: &'static str,
    pub title: &'static str,
}

#[derive(Debug, Error)]
pub enum AdditionalTrayError {
    #[error("tray icon initialization failed: {0}")]
    Init(String),
}

pub struct AdditionalTrayController {
    specs: Vec<AdditionalTrayIconSpec>,
    #[cfg(windows)]
    _windows_display: Option<WindowsDisplayTrayHandle>,
}

impl AdditionalTrayController {
    pub fn initialize(
        running: Arc<AtomicBool>,
        daemon_ui_url: String,
        windows_display_enabled: bool,
    ) -> Result<Self, AdditionalTrayError> {
        let mut specs = Vec::new();
        #[cfg(windows)]
        let mut windows_display = None;

        if windows_display_enabled {
            specs.push(AdditionalTrayIconSpec {
                extension_id: WINDOWS_DISPLAY_EXTENSION_ID,
                title: "Windows Display Manager",
            });
            #[cfg(windows)]
            {
                windows_display = Some(
                    WindowsDisplayTrayHandle::start(running, daemon_ui_url)
                        .map_err(AdditionalTrayError::Init)?,
                );
            }
        }

        Ok(Self {
            specs,
            #[cfg(windows)]
            _windows_display: windows_display,
        })
    }

    pub fn specs(&self) -> &[AdditionalTrayIconSpec] {
        &self.specs
    }
}

#[cfg(windows)]
struct WindowsDisplayTrayHandle {
    thread: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl WindowsDisplayTrayHandle {
    fn start(running: Arc<AtomicBool>, daemon_ui_url: String) -> Result<Self, String> {
        let thread = std::thread::Builder::new()
            .name("windows-display-tray".to_string())
            .spawn(move || {
                if let Err(err) = run_windows_display_tray(running, daemon_ui_url) {
                    eprintln!("windows display tray error: {err}");
                }
            })
            .map_err(|err| err.to_string())?;
        Ok(Self {
            thread: Some(thread),
        })
    }
}

#[cfg(windows)]
impl Drop for WindowsDisplayTrayHandle {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::mem;
    use std::ptr;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
        DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, LoadIconW, PeekMessageW,
        PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
        CW_USEDEFAULT, HICON, IDI_APPLICATION, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
        MF_UNCHECKED, MSG, PM_REMOVE, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_LEFTBUTTON,
        TPM_RETURNCMD, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP, WM_USER,
        WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    const WM_TRAYICON: u32 = WM_USER + 121;
    const CMD_TOGGLE_TASKBAR: u32 = 1001;
    const CMD_SETTINGS: u32 = 1002;
    const CMD_EXIT: u32 = 1003;
    const CMD_RES_BASE: u32 = 2000;
    const CMD_SCALE_BASE: u32 = 3000;
    const ICON_SIZE: i32 = 16;

    static mut WINDOWS_DISPLAY_STATE: *mut WindowsDisplayTrayState = ptr::null_mut();

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ResolutionPreset {
        width: i32,
        height: i32,
        refresh_rate: i32,
    }

    #[derive(Debug, Clone)]
    struct DisplayStatus {
        taskbar_auto_hide: bool,
        width: i32,
        height: i32,
        refresh_rate: i32,
        available_resolutions: Vec<ResolutionPreset>,
        current_scale: i32,
        available_scales: Vec<i32>,
        system_uses_light_theme: bool,
    }

    impl Default for DisplayStatus {
        fn default() -> Self {
            Self {
                taskbar_auto_hide: false,
                width: 1920,
                height: 1080,
                refresh_rate: 60,
                available_resolutions: default_resolution_presets(),
                current_scale: 100,
                available_scales: vec![100, 125, 150, 175, 200],
                system_uses_light_theme: true,
            }
        }
    }

    struct WindowsDisplayTrayState {
        hwnd: HWND,
        running: Arc<AtomicBool>,
        daemon_ui_url: String,
        data_path: PathBuf,
        status: DisplayStatus,
        resolution_presets: Vec<ResolutionPreset>,
        icon_pinned_dark: HICON,
        icon_unpinned_dark: HICON,
        icon_pinned_light: HICON,
        icon_unpinned_light: HICON,
    }

    impl WindowsDisplayTrayState {
        fn icon_for_status(&self) -> HICON {
            let dark_variant = self.status.system_uses_light_theme;
            match (self.status.taskbar_auto_hide, dark_variant) {
                (false, true) => self.icon_pinned_dark,
                (true, true) => self.icon_unpinned_dark,
                (false, false) => self.icon_pinned_light,
                (true, false) => self.icon_unpinned_light,
            }
        }

        fn tooltip(&self) -> String {
            if self.status.taskbar_auto_hide {
                "Taskbar: Auto-hide".to_string()
            } else {
                "Taskbar: Always visible".to_string()
            }
        }
    }

    pub(super) fn run_windows_display_tray(
        running: Arc<AtomicBool>,
        daemon_ui_url: String,
    ) -> Result<(), String> {
        let data_path = extension_data_path(WINDOWS_DISPLAY_EXTENSION_ID)?;
        let mut state = WindowsDisplayTrayState {
            hwnd: ptr::null_mut(),
            running: Arc::clone(&running),
            daemon_ui_url,
            data_path,
            status: DisplayStatus::default(),
            resolution_presets: DisplayStatus::default().available_resolutions,
            icon_pinned_dark: create_pin_icon(true, true).unwrap_or_else(default_icon),
            icon_unpinned_dark: create_pin_icon(false, true).unwrap_or_else(default_icon),
            icon_pinned_light: create_pin_icon(true, false).unwrap_or_else(default_icon),
            icon_unpinned_light: create_pin_icon(false, false).unwrap_or_else(default_icon),
        };
        refresh_status(&mut state).ok();

        let class_name = wide("CopperWindowsDisplayTray");
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
                wide("copper-windows-display-tray-window").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                0,
                CW_USEDEFAULT,
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

        state.hwnd = hwnd;
        unsafe {
            WINDOWS_DISPLAY_STATE = &mut state;
        }

        add_notify_icon(hwnd, state.icon_for_status(), &state.tooltip())?;
        let mut last_refresh = Instant::now();
        let mut msg = unsafe { mem::zeroed::<MSG>() };

        while running.load(Ordering::Relaxed) {
            loop {
                let has_message =
                    unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) };
                if has_message == 0 {
                    break;
                }
                if msg.message == WM_QUIT {
                    running.store(false, Ordering::Relaxed);
                    break;
                }
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            if last_refresh.elapsed() >= Duration::from_secs(2) {
                if let Some(state) = state_mut() {
                    if refresh_status(state).is_ok() {
                        let _ = modify_notify_icon(
                            state.hwnd,
                            state.icon_for_status(),
                            &state.tooltip(),
                        );
                    }
                }
                last_refresh = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(35));
        }

        remove_notify_icon(hwnd).ok();
        unsafe {
            WINDOWS_DISPLAY_STATE = ptr::null_mut();
            DestroyWindow(hwnd);
        }
        destroy_icons(&state);
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
                        if execute_action(state, "toggle-taskbar-autohide", serde_json::json!({}))
                            .is_ok()
                        {
                            let _ = modify_notify_icon(
                                state.hwnd,
                                state.icon_for_status(),
                                &state.tooltip(),
                            );
                        }
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

    fn show_context_menu(state: &mut WindowsDisplayTrayState) {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        let pinned = !state.status.taskbar_auto_hide;
        let toggle_flags = MF_STRING | if pinned { MF_CHECKED } else { MF_UNCHECKED };
        let _ = unsafe {
            AppendMenuW(
                menu,
                toggle_flags,
                CMD_TOGGLE_TASKBAR as usize,
                wide("Taskbar is pinned").as_ptr(),
            )
        };
        let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) };

        let resolution_menu = unsafe { CreatePopupMenu() };
        if !resolution_menu.is_null() {
            for (index, preset) in state.resolution_presets.iter().enumerate() {
                let checked = preset.width == state.status.width
                    && preset.height == state.status.height
                    && preset.refresh_rate == state.status.refresh_rate;
                let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
                let label = format!(
                    "{} x {} @{}Hz",
                    preset.width, preset.height, preset.refresh_rate
                );
                let cmd_id = CMD_RES_BASE + index as u32;
                let _ = unsafe {
                    AppendMenuW(
                        resolution_menu,
                        flags,
                        cmd_id as usize,
                        wide(&label).as_ptr(),
                    )
                };
            }
            let _ = unsafe {
                AppendMenuW(
                    menu,
                    MF_POPUP | MF_STRING,
                    resolution_menu as usize,
                    wide("Resolution").as_ptr(),
                )
            };
        }

        let scale_menu = unsafe { CreatePopupMenu() };
        if !scale_menu.is_null() {
            for (index, scale) in state.status.available_scales.iter().enumerate() {
                let checked = *scale == state.status.current_scale;
                let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
                let label = format!("{scale}%");
                let cmd_id = CMD_SCALE_BASE + index as u32;
                let _ = unsafe {
                    AppendMenuW(scale_menu, flags, cmd_id as usize, wide(&label).as_ptr())
                };
            }
            let _ = unsafe {
                AppendMenuW(
                    menu,
                    MF_POPUP | MF_STRING,
                    scale_menu as usize,
                    wide("Scale").as_ptr(),
                )
            };
        }

        let _ = unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) };
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                CMD_SETTINGS as usize,
                wide("Settings...").as_ptr(),
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
            let _ = modify_notify_icon(state.hwnd, state.icon_for_status(), &state.tooltip());
        }

        unsafe {
            DestroyMenu(menu);
        }
    }

    fn handle_menu_command(state: &mut WindowsDisplayTrayState, command_id: u32) {
        if command_id == CMD_TOGGLE_TASKBAR {
            let _ = execute_action(state, "toggle-taskbar-autohide", serde_json::json!({}));
            return;
        }
        if command_id == CMD_SETTINGS {
            let settings_url = format!(
                "{}?section=ext:windows-display-manager",
                state.daemon_ui_url
            );
            if let Err(err) = open_url_in_browser(&settings_url) {
                eprintln!("failed to open windows-display settings: {err}");
            }
            return;
        }
        if command_id == CMD_EXIT {
            state.running.store(false, Ordering::Relaxed);
            return;
        }
        if (CMD_RES_BASE..CMD_RES_BASE + state.resolution_presets.len() as u32)
            .contains(&command_id)
        {
            let index = (command_id - CMD_RES_BASE) as usize;
            if let Some(preset) = state.resolution_presets.get(index) {
                let _ = execute_action(
                    state,
                    "set-resolution",
                    serde_json::json!({
                        "resolutionWidth": preset.width,
                        "resolutionHeight": preset.height,
                        "refreshRate": preset.refresh_rate
                    }),
                );
            }
            return;
        }
        if (CMD_SCALE_BASE..CMD_SCALE_BASE + state.status.available_scales.len() as u32)
            .contains(&command_id)
        {
            let index = (command_id - CMD_SCALE_BASE) as usize;
            if let Some(scale) = state.status.available_scales.get(index) {
                let _ = execute_action(
                    state,
                    "set-scale",
                    serde_json::json!({
                        "scalePercent": *scale
                    }),
                );
            }
        }
    }

    fn execute_action(
        state: &mut WindowsDisplayTrayState,
        action_id: &str,
        overrides: Value,
    ) -> Result<(), String> {
        let mut config = load_data_file(&state.data_path);
        merge_object(&mut config, &overrides);
        let result = windows_display::execute_action(action_id, &config)?;
        if let Some(obj) = config.as_object_mut() {
            obj.insert("lastActionId".to_string(), serde_json::json!(action_id));
            obj.insert("lastActionOk".to_string(), serde_json::json!(true));
            obj.insert("lastResult".to_string(), result.clone());
        }
        save_data_file(&state.data_path, &config);
        state.status = parse_status(&result);
        state.resolution_presets = state.status.available_resolutions.clone();
        if action_id == "set-scale"
            && result
                .get("applied")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            if let Some(scale) = result
                .get("requested")
                .and_then(|requested| requested.get("scalePercent"))
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
            {
                state.status.current_scale = scale;
            }
        }
        Ok(())
    }

    fn refresh_status(state: &mut WindowsDisplayTrayState) -> Result<(), String> {
        let config = load_data_file(&state.data_path);
        let result = windows_display::execute_action("status", &config)?;
        state.status = parse_status(&result);
        state.resolution_presets = state.status.available_resolutions.clone();
        Ok(())
    }

    fn parse_status(raw: &Value) -> DisplayStatus {
        let mut status = DisplayStatus::default();
        status.taskbar_auto_hide = raw
            .get("taskbarAutoHide")
            .and_then(Value::as_bool)
            .unwrap_or(status.taskbar_auto_hide);
        status.system_uses_light_theme = raw
            .get("systemUsesLightTheme")
            .and_then(Value::as_bool)
            .unwrap_or(status.system_uses_light_theme);
        if let Some(res) = raw.get("resolution") {
            status.width = res
                .get("width")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(status.width);
            status.height = res
                .get("height")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(status.height);
            status.refresh_rate = res
                .get("refreshRate")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(status.refresh_rate);
            if let Some(values) = res.get("availableModes").and_then(Value::as_array) {
                let parsed = values
                    .iter()
                    .filter_map(parse_resolution_preset)
                    .collect::<Vec<_>>();
                if !parsed.is_empty() {
                    status.available_resolutions = parsed;
                }
            }
        }
        if let Some(scale) = raw.get("scale") {
            status.current_scale = scale
                .get("currentPercent")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(status.current_scale);
            if let Some(values) = scale.get("availablePercentages").and_then(Value::as_array) {
                let parsed = values
                    .iter()
                    .filter_map(Value::as_i64)
                    .filter_map(|v| i32::try_from(v).ok())
                    .collect::<Vec<_>>();
                if !parsed.is_empty() {
                    status.available_scales = parsed;
                }
            }
        }
        status
    }

    fn parse_resolution_preset(value: &Value) -> Option<ResolutionPreset> {
        let width = value
            .get("width")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())?;
        let height = value
            .get("height")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())?;
        let refresh_rate = value
            .get("refreshRate")
            .and_then(Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())?;
        Some(ResolutionPreset {
            width,
            height,
            refresh_rate,
        })
    }

    fn default_resolution_presets() -> Vec<ResolutionPreset> {
        vec![
            ResolutionPreset {
                width: 1920,
                height: 1080,
                refresh_rate: 60,
            },
            ResolutionPreset {
                width: 2560,
                height: 1440,
                refresh_rate: 60,
            },
        ]
    }

    fn load_data_file(path: &PathBuf) -> Value {
        fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}))
    }

    fn save_data_file(path: &PathBuf, value: &Value) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(value) {
            let _ = fs::write(path, raw);
        }
    }

    fn merge_object(target: &mut Value, source: &Value) {
        if let (Some(dst), Some(src)) = (target.as_object_mut(), source.as_object()) {
            for (key, value) in src {
                dst.insert(key.clone(), value.clone());
            }
        }
    }

    fn extension_data_path(extension_id: &str) -> Result<PathBuf, String> {
        let home = dirs::home_dir().ok_or_else(|| "home directory is not available".to_string())?;
        Ok(home
            .join(".Copper")
            .join("extensions")
            .join(extension_id)
            .join("data.json"))
    }

    fn add_notify_icon(hwnd: HWND, icon: HICON, tooltip: &str) -> Result<(), String> {
        let mut nid = unsafe { mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = icon;
        write_tooltip(&mut nid, tooltip);
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
        if ok == 0 {
            return Err("failed to add windows display tray icon".to_string());
        }
        Ok(())
    }

    fn modify_notify_icon(hwnd: HWND, icon: HICON, tooltip: &str) -> Result<(), String> {
        let mut nid = unsafe { mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_TIP;
        nid.hIcon = icon;
        write_tooltip(&mut nid, tooltip);
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
        if ok == 0 {
            return Err("failed to modify windows display tray icon".to_string());
        }
        Ok(())
    }

    fn remove_notify_icon(hwnd: HWND) -> Result<(), String> {
        let mut nid = unsafe { mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        let ok = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
        if ok == 0 {
            return Err("failed to remove windows display tray icon".to_string());
        }
        Ok(())
    }

    fn write_tooltip(nid: &mut NOTIFYICONDATAW, tooltip: &str) {
        let mut wide_tip = wide(tooltip);
        if wide_tip.len() > 128 {
            wide_tip.truncate(128);
            if let Some(last) = wide_tip.last_mut() {
                *last = 0;
            }
        }
        let limit = wide_tip.len().min(nid.szTip.len());
        nid.szTip[..limit].copy_from_slice(&wide_tip[..limit]);
    }

    fn create_pin_icon(pinned: bool, dark_variant: bool) -> Option<HICON> {
        let mut rgba = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
        let color = if dark_variant {
            [25u8, 25u8, 25u8, 255u8]
        } else {
            [236u8, 236u8, 236u8, 255u8]
        };

        if pinned {
            draw_rect(&mut rgba, 5, 2, 11, 4, color);
            draw_rect(&mut rgba, 7, 5, 9, 11, color);
            draw_triangle_down(&mut rgba, 8, 12, 3, color);
        } else {
            draw_rect(&mut rgba, 3, 4, 8, 6, color);
            draw_rect(&mut rgba, 8, 6, 12, 8, color);
            draw_triangle_right(&mut rgba, 12, 9, 3, color);
        }

        create_icon_from_rgba(&rgba, ICON_SIZE, ICON_SIZE)
    }

    fn create_icon_from_rgba(rgba: &[u8], width: i32, height: i32) -> Option<HICON> {
        let pixel_count = (width * height) as usize;
        if rgba.len() != pixel_count * 4 {
            return None;
        }

        let mut xor = vec![0u8; pixel_count * 4];
        let stride = ((width + 31) / 32 * 4) as usize;
        let mut and_mask = vec![0u8; stride * height as usize];

        for y in 0..height {
            for x in 0..width {
                let src_idx = ((y * width + x) * 4) as usize;
                let dst_y = height - 1 - y;
                let dst_idx = ((dst_y * width + x) * 4) as usize;
                let r = rgba[src_idx];
                let g = rgba[src_idx + 1];
                let b = rgba[src_idx + 2];
                let a = rgba[src_idx + 3];
                xor[dst_idx] = b;
                xor[dst_idx + 1] = g;
                xor[dst_idx + 2] = r;
                xor[dst_idx + 3] = a;

                if a == 0 {
                    let row = dst_y as usize;
                    let byte_index = row * stride + (x as usize / 8);
                    let bit = 0x80u8 >> (x as usize % 8);
                    and_mask[byte_index] |= bit;
                }
            }
        }

        let hicon = unsafe {
            CreateIcon(
                ptr::null_mut(),
                width,
                height,
                1,
                32,
                and_mask.as_ptr(),
                xor.as_ptr(),
            )
        };
        if hicon.is_null() {
            None
        } else {
            Some(hicon)
        }
    }

    fn draw_rect(
        rgba: &mut [u8],
        left: i32,
        top: i32,
        right_inclusive: i32,
        bottom_inclusive: i32,
        color: [u8; 4],
    ) {
        for y in top..=bottom_inclusive {
            for x in left..=right_inclusive {
                set_pixel(rgba, x, y, color);
            }
        }
    }

    fn draw_triangle_down(rgba: &mut [u8], center_x: i32, top_y: i32, size: i32, color: [u8; 4]) {
        for row in 0..size {
            let y = top_y + row;
            let span = row;
            for x in (center_x - span)..=(center_x + span) {
                set_pixel(rgba, x, y, color);
            }
        }
    }

    fn draw_triangle_right(rgba: &mut [u8], left_x: i32, top_y: i32, size: i32, color: [u8; 4]) {
        for col in 0..size {
            let x = left_x + col;
            let half = col / 2;
            for y in (top_y - half)..=(top_y + half + 1) {
                set_pixel(rgba, x, y, color);
            }
        }
    }

    fn set_pixel(rgba: &mut [u8], x: i32, y: i32, color: [u8; 4]) {
        if !(0..ICON_SIZE).contains(&x) || !(0..ICON_SIZE).contains(&y) {
            return;
        }
        let idx = ((y * ICON_SIZE + x) * 4) as usize;
        rgba[idx] = color[0];
        rgba[idx + 1] = color[1];
        rgba[idx + 2] = color[2];
        rgba[idx + 3] = color[3];
    }

    fn destroy_icons(state: &WindowsDisplayTrayState) {
        unsafe {
            if !state.icon_pinned_dark.is_null() {
                DestroyIcon(state.icon_pinned_dark);
            }
            if !state.icon_unpinned_dark.is_null() {
                DestroyIcon(state.icon_unpinned_dark);
            }
            if !state.icon_pinned_light.is_null() {
                DestroyIcon(state.icon_pinned_light);
            }
            if !state.icon_unpinned_light.is_null() {
                DestroyIcon(state.icon_unpinned_light);
            }
        }
    }

    fn default_icon() -> HICON {
        unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) as HICON }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn state_mut() -> Option<&'static mut WindowsDisplayTrayState> {
        unsafe { WINDOWS_DISPLAY_STATE.as_mut() }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            default_resolution_presets, merge_object, parse_resolution_preset, parse_status,
        };

        #[test]
        fn parse_status_reads_resolution_scale_theme_and_available_modes() {
            let status = parse_status(&serde_json::json!({
                "taskbarAutoHide": true,
                "systemUsesLightTheme": false,
                "resolution": {
                    "width": 2560,
                    "height": 1440,
                    "refreshRate": 144,
                    "availableModes": [
                        { "width": 1920, "height": 1080, "refreshRate": 60 },
                        { "width": 2560, "height": 1440, "refreshRate": 144 }
                    ]
                },
                "scale": { "currentPercent": 150, "availablePercentages": [100,125,150] }
            }));
            assert!(status.taskbar_auto_hide);
            assert!(!status.system_uses_light_theme);
            assert_eq!(status.width, 2560);
            assert_eq!(status.height, 1440);
            assert_eq!(status.refresh_rate, 144);
            assert_eq!(status.available_resolutions.len(), 2);
            assert_eq!(status.available_resolutions[1].width, 2560);
            assert_eq!(status.current_scale, 150);
            assert_eq!(status.available_scales, vec![100, 125, 150]);
        }

        #[test]
        fn parse_status_falls_back_to_default_resolution_modes() {
            let defaults = default_resolution_presets();
            let status = parse_status(&serde_json::json!({}));
            assert_eq!(status.available_resolutions.len(), defaults.len());
            assert_eq!(status.available_resolutions[0].width, defaults[0].width);
            assert_eq!(status.available_resolutions[0].height, defaults[0].height);
            assert_eq!(
                status.available_resolutions[0].refresh_rate,
                defaults[0].refresh_rate
            );
        }

        #[test]
        fn parse_resolution_preset_reads_valid_entries_only() {
            assert!(parse_resolution_preset(&serde_json::json!({
                "width": "bad",
                "height": 1000,
                "refreshRate": 60
            }))
            .is_none());

            let loaded = parse_resolution_preset(&serde_json::json!({
                "width": 2560,
                "height": 1440,
                "refreshRate": 144
            }))
            .expect("resolution preset");
            assert_eq!(loaded.width, 2560);
            assert_eq!(loaded.refresh_rate, 144);
        }

        #[test]
        fn merge_object_overwrites_keys_from_source() {
            let mut target = serde_json::json!({ "scalePercent": 100, "taskbarAutoHide": false });
            let source = serde_json::json!({ "scalePercent": 150 });
            merge_object(&mut target, &source);
            assert_eq!(
                target.get("scalePercent").and_then(|v| v.as_i64()),
                Some(150)
            );
            assert_eq!(
                target.get("taskbarAutoHide").and_then(|v| v.as_bool()),
                Some(false)
            );
        }
    }
}

#[cfg(windows)]
use windows_impl::run_windows_display_tray;

#[cfg(test)]
mod tests {
    use super::AdditionalTrayController;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn initialize_without_enabled_extensions_creates_empty_controller() {
        let controller = AdditionalTrayController::initialize(
            Arc::new(AtomicBool::new(true)),
            "http://127.0.0.1:4766".to_string(),
            false,
        )
        .expect("controller");
        assert!(controller.specs().is_empty());
    }
}

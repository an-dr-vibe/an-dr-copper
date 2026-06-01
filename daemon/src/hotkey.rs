use crate::daemon::{send_request, IpcRequest};
use crate::logging;
use crate::state_store::ExtensionStateStore;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SAFE_INPUT_KEY_ID: &str = "safe-input-key";
const SAFE_INPUT_ACTION_ID: &str = "type-text";
const HOTKEY_CONFIG_KEY: &str = "triggerKey";

pub struct HotkeyController {
    _thread: JoinHandle<()>,
}

impl HotkeyController {
    pub fn initialize(
        running: Arc<AtomicBool>,
        bind_addr: String,
        state_store: ExtensionStateStore,
    ) -> Result<Option<Self>, String> {
        #[cfg(target_os = "windows")]
        {
            let thread = std::thread::Builder::new()
                .name("copper-hotkeys".to_string())
                .spawn(move || windows_impl::run_hotkey_loop(running, bind_addr, state_store))
                .map_err(|err| err.to_string())?;
            Ok(Some(Self { _thread: thread }))
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (running, bind_addr, state_store);
            Ok(None)
        }
    }
}

fn configured_hotkey(store: &ExtensionStateStore) -> Result<String, std::io::Error> {
    Ok(store
        .load_config(SAFE_INPUT_KEY_ID)?
        .get(HOTKEY_CONFIG_KEY)
        .and_then(Value::as_str)
        .unwrap_or("scroll_lock")
        .trim()
        .to_ascii_lowercase()
        .to_string())
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
    };

    const HOTKEY_ID: i32 = 0xC022;
    const NO_WINDOW: HWND = std::ptr::null_mut();

    pub(super) fn run_hotkey_loop(
        running: Arc<AtomicBool>,
        bind_addr: String,
        state_store: ExtensionStateStore,
    ) {
        let mut registered = RegisteredHotkey::default();
        let mut last_poll = Instant::now() - Duration::from_secs(2);
        while running.load(Ordering::Relaxed) {
            if last_poll.elapsed() >= Duration::from_secs(1) {
                last_poll = Instant::now();
                match configured_hotkey(&state_store) {
                    Ok(combo) => registered.update(combo),
                    Err(err) => logging::error(format!("failed to read hotkey config: {err}")),
                }
            }

            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { PeekMessageW(&mut msg, NO_WINDOW, 0, 0, PM_REMOVE) } != 0 {
                if msg.message == WM_HOTKEY && msg.wParam == HOTKEY_ID as usize {
                    trigger_safe_input(&bind_addr);
                } else {
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        registered.clear();
    }

    #[derive(Default)]
    struct RegisteredHotkey {
        combo: String,
        registered: bool,
    }

    impl RegisteredHotkey {
        fn update(&mut self, combo: String) {
            if combo == self.combo {
                return;
            }
            self.clear();
            self.combo = combo;
            if self.combo.is_empty() {
                return;
            }

            let Some((modifiers, vk)) = parse_hotkey(&self.combo) else {
                logging::error(format!("unsupported hotkey combo '{}'", self.combo));
                return;
            };
            let ok = unsafe { RegisterHotKey(NO_WINDOW, HOTKEY_ID, modifiers, vk) } != 0;
            if ok {
                self.registered = true;
                logging::info(format!("registered Safe Input Key hotkey '{}'", self.combo));
            } else {
                logging::error(format!("failed to register hotkey '{}'", self.combo));
            }
        }

        fn clear(&mut self) {
            if self.registered {
                unsafe {
                    UnregisterHotKey(NO_WINDOW, HOTKEY_ID);
                }
            }
            self.registered = false;
        }
    }

    fn trigger_safe_input(bind_addr: &str) {
        match send_request(
            bind_addr,
            &IpcRequest::Trigger {
                id: SAFE_INPUT_KEY_ID.to_string(),
                action: Some(SAFE_INPUT_ACTION_ID.to_string()),
            },
        ) {
            Ok(response) if response.ok => {}
            Ok(response) => logging::error(format!(
                "Safe Input Key trigger failed: {}",
                response.message
            )),
            Err(err) => logging::error(format!("Safe Input Key trigger request failed: {err}")),
        }
    }

    fn parse_hotkey(combo: &str) -> Option<(u32, u32)> {
        let mut modifiers = 0;
        let mut key = None;
        for part in combo
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            match part {
                "ctrl" | "control" => modifiers |= MOD_CONTROL,
                "alt" => modifiers |= MOD_ALT,
                "shift" => modifiers |= MOD_SHIFT,
                "meta" | "cmd" | "win" => modifiers |= MOD_WIN,
                other => key = key_to_vk(other),
            }
        }
        key.map(|vk| (modifiers, vk))
    }

    fn key_to_vk(key: &str) -> Option<u32> {
        match key {
            "scroll_lock" => Some(0x91),
            "caps_lock" => Some(0x14),
            "num_lock" => Some(0x90),
            "space" => Some(0x20),
            "enter" => Some(0x0D),
            "tab" => Some(0x09),
            "esc" | "escape" => Some(0x1B),
            "backspace" => Some(0x08),
            "delete" => Some(0x2E),
            "insert" => Some(0x2D),
            "home" => Some(0x24),
            "end" => Some(0x23),
            "page_up" => Some(0x21),
            "page_down" => Some(0x22),
            "arrow_left" => Some(0x25),
            "arrow_up" => Some(0x26),
            "arrow_right" => Some(0x27),
            "arrow_down" => Some(0x28),
            key if key.len() == 1 => key.chars().next().map(|ch| ch.to_ascii_uppercase() as u32),
            key if key.starts_with('f') => key[1..]
                .parse::<u32>()
                .ok()
                .filter(|number| (1..=24).contains(number))
                .map(|number| 0x70 + number - 1),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::parse_hotkey;

        #[test]
        fn parses_single_and_modified_hotkeys() {
            assert_eq!(parse_hotkey("scroll_lock"), Some((0, 0x91)));
            assert_eq!(parse_hotkey("ctrl+alt+f12"), Some((0x0002 | 0x0001, 0x7B)));
        }
    }
}

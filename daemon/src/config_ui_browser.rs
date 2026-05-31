use super::UiConfigError;

#[cfg(not(target_os = "windows"))]
use std::process::Command;

pub(super) fn open_in_browser(url: &str) -> Result<(), UiConfigError> {
    #[cfg(target_os = "windows")]
    {
        use std::ptr;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let operation = wide_windows_string("open");
        let target = wide_windows_string(url);
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if result <= 32 {
            return Err(UiConfigError::Browser(format!(
                "ShellExecuteW failed with code {result}"
            )));
        }
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| UiConfigError::Browser(e.to_string()))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .status()
            .map_err(|e| UiConfigError::Browser(e.to_string()))?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn wide_windows_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

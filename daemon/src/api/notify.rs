#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn notify(message: &str) {
    #[cfg(target_os = "windows")]
    notify_windows(message);
    #[cfg(target_os = "macos")]
    notify_macos(message);
    #[cfg(target_os = "linux")]
    notify_linux(message);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let _ = message;
}

#[cfg(target_os = "windows")]
fn notify_windows(message: &str) {
    let msg = message.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms;\
        $n=New-Object System.Windows.Forms.NotifyIcon;\
        $n.Icon=[System.Drawing.SystemIcons]::Information;\
        $n.Visible=$true;\
        $n.ShowBalloonTip(4000,'Copper','{msg}',[System.Windows.Forms.ToolTipIcon]::None);\
        Start-Sleep -Milliseconds 4500;\
        $n.Dispose()"
    );
    let mut command = std::process::Command::new("powershell");
    command.args([
        "-NoProfile",
        "-WindowStyle",
        "Hidden",
        "-NonInteractive",
        "-Command",
        &script,
    ]);
    command.creation_flags(CREATE_NO_WINDOW);
    let _ = command.spawn();
}

#[cfg(target_os = "macos")]
fn notify_macos(message: &str) {
    let escaped = message.replace('"', "\\\"");
    let _ = std::process::Command::new("osascript")
        .args([
            "-e",
            &format!("display notification \"{escaped}\" with title \"Copper\""),
        ])
        .spawn();
}

#[cfg(target_os = "linux")]
fn notify_linux(message: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["Copper", message])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::notify;

    #[test]
    fn notify_does_not_panic() {
        notify("test notification");
    }
}

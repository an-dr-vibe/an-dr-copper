use serde_json::Value;

#[cfg(any(
    all(not(test), target_os = "linux"),
    all(not(test), target_os = "macos")
))]
use std::fs;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::path::{Path, PathBuf};

#[cfg(all(not(test), target_os = "windows"))]
use std::process::Command;

use thiserror::Error;

pub const CORE_AUTO_START_KEY: &str = "autoStart";
#[allow(dead_code)]
const AUTOSTART_NAME: &str = "Copper";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoStartStatus {
    pub enabled: bool,
}

#[derive(Debug, Error)]
pub enum AutoStartError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("home directory is not available")]
    HomeDirUnavailable,
    #[error("current executable path is not available")]
    CurrentExeUnavailable,
    #[error("autostart command failed: {0}")]
    CommandFailed(String),
    #[error("autostart is not supported on this platform")]
    UnsupportedPlatform,
}

pub fn desired_from_core_config(config: &Value) -> bool {
    config
        .get(CORE_AUTO_START_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(not(test))]
pub fn sync_from_core_config(config: &Value) -> Result<AutoStartStatus, AutoStartError> {
    let enabled = desired_from_core_config(config);
    if enabled {
        register_current_exe()?;
    } else {
        unregister()?;
    }
    Ok(AutoStartStatus { enabled })
}

#[cfg(test)]
pub fn sync_from_core_config(config: &Value) -> Result<AutoStartStatus, AutoStartError> {
    Ok(AutoStartStatus {
        enabled: desired_from_core_config(config),
    })
}

#[cfg(all(not(test), target_os = "windows"))]
fn register_current_exe() -> Result<(), AutoStartError> {
    let exe = windows_autostart_exe()?;
    let output = Command::new("reg")
        .args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            AUTOSTART_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &exe,
            "/f",
        ])
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(AutoStartError::CommandFailed(command_output(&output)))
}

#[cfg(all(not(test), target_os = "windows"))]
fn unregister() -> Result<(), AutoStartError> {
    let output = Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            AUTOSTART_NAME,
            "/f",
        ])
        .output()?;
    if output.status.success() || command_output(&output).contains("Unable to find") {
        return Ok(());
    }
    Err(AutoStartError::CommandFailed(command_output(&output)))
}

#[cfg(all(not(test), target_os = "windows"))]
fn windows_autostart_exe() -> Result<String, AutoStartError> {
    let exe = std::env::current_exe().map_err(|_| AutoStartError::CurrentExeUnavailable)?;
    let launch_path = preferred_windows_launch_path(&exe);
    Ok(format!("\"{}\"", launch_path.display()))
}

#[cfg(target_os = "windows")]
fn preferred_windows_launch_path(exe: &Path) -> PathBuf {
    let preferred = exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("copper.exe");
    if preferred.exists() {
        preferred
    } else {
        exe.to_path_buf()
    }
}

#[cfg(all(not(test), target_os = "windows"))]
fn command_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}").trim().to_string()
}

#[cfg(all(not(test), target_os = "linux"))]
fn register_current_exe() -> Result<(), AutoStartError> {
    let home = dirs::home_dir().ok_or(AutoStartError::HomeDirUnavailable)?;
    let path = linux_desktop_entry_path(&home);
    let exe = std::env::current_exe().map_err(|_| AutoStartError::CurrentExeUnavailable)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, linux_desktop_entry(&exe))?;
    Ok(())
}

#[cfg(all(not(test), target_os = "linux"))]
fn unregister() -> Result<(), AutoStartError> {
    let home = dirs::home_dir().ok_or(AutoStartError::HomeDirUnavailable)?;
    let path = linux_desktop_entry_path(&home);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(all(not(test), target_os = "linux"))]
fn linux_desktop_entry_path(home: &Path) -> PathBuf {
    home.join(".config")
        .join("autostart")
        .join(format!("{AUTOSTART_NAME}.desktop"))
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        AUTOSTART_NAME,
        escape_desktop_exec_arg(&exe.display().to_string())
    )
}

#[cfg(target_os = "linux")]
fn escape_desktop_exec_arg(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            ' ' | '\t' | '\n' | '"' | '\'' | '\\' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(all(not(test), target_os = "macos"))]
fn register_current_exe() -> Result<(), AutoStartError> {
    let home = dirs::home_dir().ok_or(AutoStartError::HomeDirUnavailable)?;
    let launch_agents = home.join("Library").join("LaunchAgents");
    let logs_dir = home.join("Library").join("Logs").join("Copper");
    fs::create_dir_all(&launch_agents)?;
    fs::create_dir_all(&logs_dir)?;

    let exe = std::env::current_exe().map_err(|_| AutoStartError::CurrentExeUnavailable)?;
    let plist_path = macos_launch_agent_path(&home);
    let stdout_log = logs_dir.join("Copper.stdout.log");
    let stderr_log = logs_dir.join("Copper.stderr.log");
    fs::write(
        plist_path,
        macos_launch_agent_plist(&exe, &stdout_log, &stderr_log),
    )?;
    Ok(())
}

#[cfg(all(not(test), target_os = "macos"))]
fn unregister() -> Result<(), AutoStartError> {
    let home = dirs::home_dir().ok_or(AutoStartError::HomeDirUnavailable)?;
    let plist_path = macos_launch_agent_path(&home);
    if plist_path.exists() {
        fs::remove_file(plist_path)?;
    }
    Ok(())
}

#[cfg(all(not(test), target_os = "macos"))]
fn macos_launch_agent_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join("dev.copper.Copper.plist")
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_plist(exe: &Path, stdout_log: &Path, stderr_log: &Path) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n",
            "<dict>\n",
            "  <key>Label</key>\n",
            "  <string>dev.copper.Copper</string>\n",
            "  <key>ProgramArguments</key>\n",
            "  <array>\n",
            "    <string>{}</string>\n",
            "  </array>\n",
            "  <key>RunAtLoad</key>\n",
            "  <true/>\n",
            "  <key>WorkingDirectory</key>\n",
            "  <string>{}</string>\n",
            "  <key>StandardOutPath</key>\n",
            "  <string>{}</string>\n",
            "  <key>StandardErrorPath</key>\n",
            "  <string>{}</string>\n",
            "</dict>\n",
            "</plist>\n"
        ),
        xml_escape(&exe.display().to_string()),
        xml_escape(
            &exe.parent()
                .unwrap_or_else(|| Path::new("."))
                .display()
                .to_string()
        ),
        xml_escape(&stdout_log.display().to_string()),
        xml_escape(&stderr_log.display().to_string())
    )
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(all(
    not(test),
    not(any(target_os = "windows", target_os = "linux", target_os = "macos"))
))]
fn register_current_exe() -> Result<(), AutoStartError> {
    Err(AutoStartError::UnsupportedPlatform)
}

#[cfg(all(
    not(test),
    not(any(target_os = "windows", target_os = "linux", target_os = "macos"))
))]
fn unregister() -> Result<(), AutoStartError> {
    Err(AutoStartError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::desired_from_core_config;

    #[test]
    fn desired_from_core_config_defaults_false() {
        assert!(!desired_from_core_config(&serde_json::json!({})));
    }

    #[test]
    fn desired_from_core_config_reads_true() {
        assert!(desired_from_core_config(
            &serde_json::json!({ "autoStart": true })
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_launcher_prefers_gui_binary_when_present() {
        let temp = tempfile::tempdir().expect("tempdir");
        let exe = temp.path().join("copperd.exe");
        let gui = temp.path().join("copper.exe");
        std::fs::write(&exe, b"stub").expect("write copperd");
        std::fs::write(&gui, b"stub").expect("write copper");

        let selected = super::preferred_windows_launch_path(&exe);
        assert_eq!(selected, gui);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_desktop_entry_escapes_spaces() {
        let rendered = super::linux_desktop_entry(std::path::Path::new("/tmp/Copper App/copperd"));
        assert!(rendered.contains("Exec=/tmp/Copper\\ App/copperd"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_launch_agent_includes_label_and_program() {
        let rendered = super::macos_launch_agent_plist(
            std::path::Path::new("/tmp/Copper.app/Contents/MacOS/copperd"),
            std::path::Path::new("/tmp/copper.stdout.log"),
            std::path::Path::new("/tmp/copper.stderr.log"),
        );
        assert!(rendered.contains("dev.copper.Copper"));
        assert!(rendered.contains("/tmp/Copper.app/Contents/MacOS/copperd"));
    }
}

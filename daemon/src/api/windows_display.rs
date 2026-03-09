use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgeRequest {
    action: String,
    taskbar_auto_hide: bool,
    resolution_width: i32,
    resolution_height: i32,
    refresh_rate: i32,
    scale_percent: i32,
}

pub fn execute_action(action_id: &str, config: &Value) -> Result<Value, String> {
    execute_action_with_runner(action_id, config, run_windows_bridge)
}

fn execute_action_with_runner<F>(
    action_id: &str,
    config: &Value,
    runner: F,
) -> Result<Value, String>
where
    F: Fn(&BridgeRequest) -> Result<Value, String>,
{
    let request = BridgeRequest {
        action: action_id.to_string(),
        taskbar_auto_hide: read_bool(config, "taskbarAutoHide", false),
        resolution_width: read_i32(config, "resolutionWidth", 1920, 640, 16_384),
        resolution_height: read_i32(config, "resolutionHeight", 1080, 480, 16_384),
        refresh_rate: read_i32(config, "refreshRate", 60, 1, 480),
        scale_percent: read_i32(config, "scalePercent", 100, 100, 350),
    };

    match action_id {
        "status"
        | "toggle-taskbar-autohide"
        | "set-taskbar-autohide"
        | "set-resolution"
        | "set-scale" => runner(&request),
        other => Err(format!(
            "unsupported windows-display-manager action '{other}'"
        )),
    }
}

fn read_bool(config: &Value, key: &str, default_value: bool) -> bool {
    config
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default_value)
}

fn read_i32(config: &Value, key: &str, default_value: i32, min: i32, max: i32) -> i32 {
    let value = config
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(default_value);
    value.clamp(min, max)
}

#[cfg(target_os = "windows")]
fn run_windows_bridge(request: &BridgeRequest) -> Result<Value, String> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            windows_bridge_script(),
        ])
        .env("COPPER_ACTION", &request.action)
        .env(
            "COPPER_TASKBAR_AUTOHIDE",
            if request.taskbar_auto_hide {
                "true"
            } else {
                "false"
            },
        )
        .env(
            "COPPER_RESOLUTION_WIDTH",
            request.resolution_width.to_string(),
        )
        .env(
            "COPPER_RESOLUTION_HEIGHT",
            request.resolution_height.to_string(),
        )
        .env("COPPER_REFRESH_RATE", request.refresh_rate.to_string())
        .env("COPPER_SCALE_PERCENT", request.scale_percent.to_string())
        .output()
        .map_err(|err| format!("failed to execute windows display bridge: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() { stderr } else { stdout };
        return Err(if details.is_empty() {
            format!(
                "windows display bridge exited with status {}",
                output.status
            )
        } else {
            format!("windows display bridge failed: {details}")
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| "windows display bridge returned empty output".to_string())?;

    let value: Value = serde_json::from_str(json_line.trim())
        .map_err(|err| format!("windows display bridge returned invalid JSON: {err}"))?;
    if value
        .get("ok")
        .and_then(Value::as_bool)
        .map(|v| !v)
        .unwrap_or(false)
    {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("windows display bridge reported failure");
        return Err(error.to_string());
    }
    Ok(value)
}

#[cfg(not(target_os = "windows"))]
fn run_windows_bridge(_request: &BridgeRequest) -> Result<Value, String> {
    Err("windows-display-manager is only supported on Windows".to_string())
}

#[cfg(target_os = "windows")]
fn windows_bridge_script() -> &'static str {
    r#"
$ErrorActionPreference = 'Stop'
$action = $env:COPPER_ACTION
$taskbarAutoHide = ($env:COPPER_TASKBAR_AUTOHIDE -eq 'true')
$resolutionWidth = [int]$env:COPPER_RESOLUTION_WIDTH
$resolutionHeight = [int]$env:COPPER_RESOLUTION_HEIGHT
$refreshRate = [int]$env:COPPER_REFRESH_RATE
$scalePercent = [int]$env:COPPER_SCALE_PERCENT

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32;
public static class CopperWinDisplay {
  [StructLayout(LayoutKind.Sequential)]
  struct APPBARDATA {
    public uint cbSize;
    public IntPtr hWnd;
    public uint uCallbackMessage;
    public uint uEdge;
    public int rcLeft, rcTop, rcRight, rcBottom;
    public IntPtr lParam;
  }
  [DllImport("shell32.dll")]
  static extern uint SHAppBarMessage(uint dwMessage, ref APPBARDATA pData);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
  const uint ABM_GETSTATE = 4;
  const uint ABM_SETSTATE = 10;
  const int ABS_AUTOHIDE = 1;
  const int ABS_ALWAYSONTOP = 2;
  static APPBARDATA MakeAppBarData() {
    APPBARDATA abd = new APPBARDATA();
    abd.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
    abd.hWnd = FindWindow("Shell_TrayWnd", null);
    return abd;
  }
  public static bool IsTaskbarAutoHide() {
    APPBARDATA abd = MakeAppBarData();
    return (SHAppBarMessage(ABM_GETSTATE, ref abd) & ABS_AUTOHIDE) != 0;
  }
  public static bool SetTaskbarAutoHide(bool value) {
    APPBARDATA abd = MakeAppBarData();
    abd.lParam = (IntPtr)(value ? ABS_AUTOHIDE : ABS_ALWAYSONTOP);
    SHAppBarMessage(ABM_SETSTATE, ref abd);
    return IsTaskbarAutoHide() == value;
  }
  public static bool ToggleTaskbarAutoHide() {
    return SetTaskbarAutoHide(!IsTaskbarAutoHide());
  }

  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Ansi)]
  struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
    public int dmFields;
    public int dmPositionX, dmPositionY;
    public int dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public short dmLogPixels;
    public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType;
    public int dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
  }
  [DllImport("user32.dll", CharSet=CharSet.Ansi)]
  static extern bool EnumDisplaySettings(string deviceName, int modeNum, ref DEVMODE devMode);
  [DllImport("user32.dll", CharSet=CharSet.Ansi)]
  static extern int ChangeDisplaySettings(ref DEVMODE devMode, int flags);
  const int ENUM_CURRENT_SETTINGS = -1;
  const int DISP_CHANGE_SUCCESSFUL = 0;
  const int DM_PELSWIDTH = 0x80000;
  const int DM_PELSHEIGHT = 0x100000;
  const int DM_DISPLAYFREQUENCY = 0x400000;
  static DEVMODE CurrentMode() {
    DEVMODE dm = new DEVMODE();
    dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
    EnumDisplaySettings(null, ENUM_CURRENT_SETTINGS, ref dm);
    return dm;
  }
  public static int[] GetResolution() {
    DEVMODE dm = CurrentMode();
    return new int[] { dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency };
  }
  public static bool SetResolution(int width, int height, int refreshRate) {
    DEVMODE dm = CurrentMode();
    dm.dmPelsWidth = width;
    dm.dmPelsHeight = height;
    dm.dmDisplayFrequency = refreshRate;
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    return ChangeDisplaySettings(ref dm, 0) == DISP_CHANGE_SUCCESSFUL;
  }

  [DllImport("user32.dll")]
  static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);
  [DllImport("shcore.dll")]
  static extern int GetDpiForMonitor(IntPtr hmonitor, uint dpiType, out uint dpiX, out uint dpiY);
  const uint MONITOR_DEFAULTTOPRIMARY = 1;
  const uint MDT_EFFECTIVE_DPI = 0;
  public static int GetScalePercent() {
    IntPtr monitor = MonitorFromWindow(IntPtr.Zero, MONITOR_DEFAULTTOPRIMARY);
    uint dpiX, dpiY;
    if (GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, out dpiX, out dpiY) == 0) {
      return (int)Math.Round(dpiX * 100.0 / 96.0);
    }
    return 100;
  }
  public static int[] GetScaleLevels() {
    return new int[] { 100, 125, 150, 175, 200, 225, 250, 300, 350 };
  }
  public static bool SystemUsesLightTheme() {
    object value = Registry.GetValue(
      @"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
      "SystemUsesLightTheme",
      1
    );
    if (value is int) {
      return ((int)value) != 0;
    }
    return true;
  }
  public static bool SetScalePercent(int percent) {
    try {
      int dpi = (int)Math.Round(percent * 96.0 / 100.0);
      RegistryKey key = Registry.CurrentUser.OpenSubKey(@"Control Panel\Desktop", true);
      if (key == null) return false;
      key.SetValue("LogPixels", dpi, RegistryValueKind.DWord);
      key.SetValue("Win8DpiScaling", 1, RegistryValueKind.DWord);
      return true;
    } catch {
      return false;
    }
  }
}
"@ -Language CSharp | Out-Null

function Get-StatusObject {
  $resolution = [CopperWinDisplay]::GetResolution()
  $autoHide = [CopperWinDisplay]::IsTaskbarAutoHide()
  [pscustomobject]@{
    ok = $true
    action = 'status'
    taskbarAutoHide = $autoHide
    taskbarPinned = (-not $autoHide)
    resolution = [pscustomobject]@{
      width = $resolution[0]
      height = $resolution[1]
      refreshRate = $resolution[2]
    }
    scale = [pscustomobject]@{
      currentPercent = [CopperWinDisplay]::GetScalePercent()
      availablePercentages = [CopperWinDisplay]::GetScaleLevels()
    }
    systemUsesLightTheme = [CopperWinDisplay]::SystemUsesLightTheme()
  }
}

$result = switch ($action) {
  'status' {
    Get-StatusObject
    break
  }
  'toggle-taskbar-autohide' {
    $ok = [CopperWinDisplay]::ToggleTaskbarAutoHide()
    $status = Get-StatusObject
    $status.action = 'toggle-taskbar-autohide'
    $status.applied = $ok
    $status
    break
  }
  'set-taskbar-autohide' {
    $ok = [CopperWinDisplay]::SetTaskbarAutoHide($taskbarAutoHide)
    $status = Get-StatusObject
    $status.action = 'set-taskbar-autohide'
    $status.requested = [pscustomobject]@{ taskbarAutoHide = $taskbarAutoHide }
    $status.applied = $ok
    $status
    break
  }
  'set-resolution' {
    $ok = [CopperWinDisplay]::SetResolution($resolutionWidth, $resolutionHeight, $refreshRate)
    $status = Get-StatusObject
    $status.action = 'set-resolution'
    $status.requested = [pscustomobject]@{
      width = $resolutionWidth
      height = $resolutionHeight
      refreshRate = $refreshRate
    }
    $status.applied = $ok
    $status
    break
  }
  'set-scale' {
    $ok = [CopperWinDisplay]::SetScalePercent($scalePercent)
    $status = Get-StatusObject
    $status.action = 'set-scale'
    $status.requested = [pscustomobject]@{ scalePercent = $scalePercent }
    $status.applied = $ok
    $status.requiresSignOut = $true
    $status
    break
  }
  default {
    [pscustomobject]@{
      ok = $false
      error = "unsupported action '$action'"
    }
    break
  }
}

$result | ConvertTo-Json -Compress -Depth 8
"#
}

#[cfg(test)]
mod tests {
    use super::execute_action_with_runner;
    use serde_json::json;

    #[test]
    fn status_action_uses_defaults_and_dispatches_to_runner() {
        let response = execute_action_with_runner("status", &json!({}), |request| {
            assert_eq!(request.action, "status");
            assert_eq!(request.resolution_width, 1920);
            assert_eq!(request.resolution_height, 1080);
            assert_eq!(request.refresh_rate, 60);
            assert_eq!(request.scale_percent, 100);
            assert!(!request.taskbar_auto_hide);
            Ok(json!({ "ok": true, "action": "status" }))
        })
        .expect("status response");

        assert_eq!(response.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            response.get("action").and_then(|v| v.as_str()),
            Some("status")
        );
    }

    #[test]
    fn set_taskbar_autohide_reads_boolean_flag() {
        execute_action_with_runner(
            "set-taskbar-autohide",
            &json!({ "taskbarAutoHide": true }),
            |request| {
                assert!(request.taskbar_auto_hide);
                Ok(json!({ "ok": true }))
            },
        )
        .expect("dispatch");
    }

    #[test]
    fn set_resolution_clamps_invalid_input_range() {
        execute_action_with_runner(
            "set-resolution",
            &json!({
                "resolutionWidth": 50_000,
                "resolutionHeight": 100,
                "refreshRate": 1000
            }),
            |request| {
                assert_eq!(request.resolution_width, 16_384);
                assert_eq!(request.resolution_height, 480);
                assert_eq!(request.refresh_rate, 480);
                Ok(json!({ "ok": true }))
            },
        )
        .expect("dispatch");
    }

    #[test]
    fn set_scale_clamps_to_supported_range() {
        execute_action_with_runner("set-scale", &json!({ "scalePercent": 20 }), |request| {
            assert_eq!(request.scale_percent, 100);
            Ok(json!({ "ok": true }))
        })
        .expect("dispatch");

        execute_action_with_runner("set-scale", &json!({ "scalePercent": 999 }), |request| {
            assert_eq!(request.scale_percent, 350);
            Ok(json!({ "ok": true }))
        })
        .expect("dispatch");
    }

    #[test]
    fn unsupported_action_returns_error() {
        let err =
            execute_action_with_runner("unknown-action", &json!({}), |_request| Ok(json!({})))
                .expect_err("unsupported action must fail");
        assert!(err.contains("unsupported windows-display-manager action"));
    }
}

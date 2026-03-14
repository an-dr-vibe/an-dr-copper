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
    #[cfg(target_os = "windows")]
    {
        execute_action_windows(action_id, config, run_windows_bridge)
    }
    #[cfg(not(target_os = "windows"))]
    execute_action_with_runner(action_id, config, run_windows_bridge)
}

#[cfg(target_os = "windows")]
fn execute_action_windows<F>(action_id: &str, config: &Value, runner: F) -> Result<Value, String>
where
    F: Fn(&BridgeRequest) -> Result<Value, String>,
{
    let request = build_request(action_id, config);
    match action_id {
        "status" => {
            let mut result = status_payload(&request, &runner);
            apply_taskbar_state(&mut result, read_taskbar_auto_hide()?);
            Ok(result)
        }
        "toggle-taskbar-autohide" => {
            let applied = toggle_taskbar_auto_hide()?;
            let mut result = status_payload(&request, &runner);
            apply_taskbar_state(&mut result, read_taskbar_auto_hide()?);
            finalize_taskbar_action(&mut result, action_id, applied, None);
            Ok(result)
        }
        "set-taskbar-autohide" => {
            let applied = set_taskbar_auto_hide(request.taskbar_auto_hide)?;
            let mut result = status_payload(&request, &runner);
            apply_taskbar_state(&mut result, read_taskbar_auto_hide()?);
            finalize_taskbar_action(
                &mut result,
                action_id,
                applied,
                Some(request.taskbar_auto_hide),
            );
            Ok(result)
        }
        _ => {
            let mut result = execute_action_with_runner(action_id, config, runner)?;
            if let Ok(auto_hide) = read_taskbar_auto_hide() {
                apply_taskbar_state(&mut result, auto_hide);
            }
            Ok(result)
        }
    }
}

fn execute_action_with_runner<F>(
    action_id: &str,
    config: &Value,
    runner: F,
) -> Result<Value, String>
where
    F: Fn(&BridgeRequest) -> Result<Value, String>,
{
    let request = build_request(action_id, config);

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

fn build_request(action_id: &str, config: &Value) -> BridgeRequest {
    BridgeRequest {
        action: action_id.to_string(),
        taskbar_auto_hide: read_bool(config, "taskbarAutoHide", false),
        resolution_width: read_i32(config, "resolutionWidth", 1920, 640, 16_384),
        resolution_height: read_i32(config, "resolutionHeight", 1080, 480, 16_384),
        refresh_rate: read_i32(config, "refreshRate", 60, 1, 480),
        scale_percent: read_i32(config, "scalePercent", 100, 100, 350),
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

fn apply_taskbar_state(result: &mut Value, auto_hide: bool) {
    ensure_object(result);
    let map = result
        .as_object_mut()
        .expect("ensure_object should create a JSON object");
    map.insert("ok".to_string(), Value::Bool(true));
    map.insert("taskbarAutoHide".to_string(), Value::Bool(auto_hide));
    map.insert("taskbarPinned".to_string(), Value::Bool(!auto_hide));
}

fn finalize_taskbar_action(
    result: &mut Value,
    action_id: &str,
    applied: bool,
    requested_auto_hide: Option<bool>,
) {
    ensure_object(result);
    let map = result
        .as_object_mut()
        .expect("ensure_object should create a JSON object");
    map.insert("action".to_string(), Value::String(action_id.to_string()));
    map.insert("applied".to_string(), Value::Bool(applied));
    if let Some(value) = requested_auto_hide {
        map.insert(
            "requested".to_string(),
            serde_json::json!({ "taskbarAutoHide": value }),
        );
    }
}

fn ensure_object(value: &mut Value) {
    if !value.is_object() {
        *value = serde_json::json!({});
    }
}

#[cfg(target_os = "windows")]
fn status_payload<F>(request: &BridgeRequest, runner: &F) -> Value
where
    F: Fn(&BridgeRequest) -> Result<Value, String>,
{
    let mut status_request = request.clone();
    status_request.action = "status".to_string();
    runner(&status_request)
        .unwrap_or_else(|_| serde_json::json!({ "ok": true, "action": "status" }))
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct AppBarData {
    cb_size: u32,
    h_wnd: *mut core::ffi::c_void,
    u_callback_message: u32,
    u_edge: u32,
    rc_left: i32,
    rc_top: i32,
    rc_right: i32,
    rc_bottom: i32,
    l_param: isize,
}

#[cfg(target_os = "windows")]
const ABM_GETSTATE: u32 = 4;
#[cfg(target_os = "windows")]
const ABM_SETSTATE: u32 = 10;
#[cfg(target_os = "windows")]
const ABS_AUTOHIDE: u32 = 1;
#[cfg(target_os = "windows")]
const ABS_ALWAYSONTOP: u32 = 2;

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHAppBarMessage(message: u32, data: *mut AppBarData) -> usize;
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> *mut core::ffi::c_void;
}

#[cfg(target_os = "windows")]
fn read_taskbar_auto_hide() -> Result<bool, String> {
    let mut data = taskbar_appbar_data()?;
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) } as u32;
    Ok((state & ABS_AUTOHIDE) != 0)
}

#[cfg(target_os = "windows")]
fn set_taskbar_auto_hide(auto_hide: bool) -> Result<bool, String> {
    let mut data = taskbar_appbar_data()?;
    data.l_param = if auto_hide {
        ABS_AUTOHIDE as isize
    } else {
        ABS_ALWAYSONTOP as isize
    };
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
    Ok(read_taskbar_auto_hide()? == auto_hide)
}

#[cfg(target_os = "windows")]
fn toggle_taskbar_auto_hide() -> Result<bool, String> {
    let current = read_taskbar_auto_hide()?;
    set_taskbar_auto_hide(!current)
}

#[cfg(target_os = "windows")]
fn taskbar_appbar_data() -> Result<AppBarData, String> {
    let shell_tray = wide_null("Shell_TrayWnd");
    let hwnd = unsafe { FindWindowW(shell_tray.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return Err("failed to locate Shell_TrayWnd".to_string());
    }
    Ok(AppBarData {
        cb_size: std::mem::size_of::<AppBarData>() as u32,
        h_wnd: hwnd,
        u_callback_message: 0,
        u_edge: 0,
        rc_left: 0,
        rc_top: 0,
        rc_right: 0,
        rc_bottom: 0,
        l_param: 0,
    })
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
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
using System.Collections.Generic;
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
  public static int[][] GetResolutionModes() {
    List<int[]> modes = new List<int[]>();
    HashSet<string> seen = new HashSet<string>(StringComparer.Ordinal);
    int modeIndex = 0;
    while (true) {
      DEVMODE dm = new DEVMODE();
      dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
      if (!EnumDisplaySettings(null, modeIndex, ref dm)) {
        break;
      }
      string key = dm.dmPelsWidth + "x" + dm.dmPelsHeight + "@" + dm.dmDisplayFrequency;
      if (seen.Add(key)) {
        modes.Add(new int[] { dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency });
      }
      modeIndex += 1;
    }
    if (modes.Count == 0) {
      int[] current = GetResolution();
      modes.Add(new int[] { current[0], current[1], current[2] });
    }
    modes.Sort(delegate(int[] left, int[] right) {
      int width = left[0].CompareTo(right[0]);
      if (width != 0) return width;
      int height = left[1].CompareTo(right[1]);
      if (height != 0) return height;
      return left[2].CompareTo(right[2]);
    });
    return modes.ToArray();
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

  [StructLayout(LayoutKind.Sequential)]
  struct LUID {
    public uint LowPart;
    public int HighPart;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct DISPLAYCONFIG_RATIONAL {
    public uint Numerator;
    public uint Denominator;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct DISPLAYCONFIG_DEVICE_INFO_HEADER {
    public int type;
    public int size;
    public LUID adapterId;
    public uint id;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct DISPLAYCONFIG_PATH_SOURCE_INFO {
    public LUID adapterId;
    public uint id;
    public uint modeInfoIdx;
    public uint statusFlags;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct DISPLAYCONFIG_PATH_TARGET_INFO {
    public LUID adapterId;
    public uint id;
    public uint modeInfoIdx;
    public uint outputTechnology;
    public uint rotation;
    public uint scaling;
    public DISPLAYCONFIG_RATIONAL refreshRate;
    public uint scanLineOrdering;
    [MarshalAs(UnmanagedType.Bool)] public bool targetAvailable;
    public uint statusFlags;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct DISPLAYCONFIG_PATH_INFO {
    public DISPLAYCONFIG_PATH_SOURCE_INFO sourceInfo;
    public DISPLAYCONFIG_PATH_TARGET_INFO targetInfo;
    public uint flags;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct DISPLAYCONFIG_MODE_INFO {
    public uint infoType;
    public uint id;
    public LUID adapterId;
    ulong _u0;
    ulong _u1;
    ulong _u2;
    ulong _u3;
    ulong _u4;
    ulong _u5;
  }
  [DllImport("user32.dll")]
  static extern int GetDisplayConfigBufferSizes(uint flags, out uint pathCount, out uint modeCount);
  [DllImport("user32.dll")]
  static extern int QueryDisplayConfig(
    uint flags,
    ref uint pathCount,
    [Out] DISPLAYCONFIG_PATH_INFO[] paths,
    ref uint modeCount,
    [Out] DISPLAYCONFIG_MODE_INFO[] modes,
    IntPtr currentTopologyId
  );
  [DllImport("user32.dll", EntryPoint = "DisplayConfigGetDeviceInfo")]
  static extern int DisplayConfigGetDeviceInfoRaw(IntPtr packet);
  [DllImport("user32.dll", EntryPoint = "DisplayConfigSetDeviceInfo")]
  static extern int DisplayConfigSetDeviceInfoRaw(IntPtr packet);
  [DllImport("user32.dll")]
  static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);
  [DllImport("shcore.dll")]
  static extern int GetDpiForMonitor(IntPtr hmonitor, uint dpiType, out uint dpiX, out uint dpiY);
  const uint QDC_ONLY_ACTIVE_PATHS = 2;
  const uint MONITOR_DEFAULTTOPRIMARY = 1;
  const uint MDT_EFFECTIVE_DPI = 0;
  const int GET_SIZE = 32;
  const int SET_SIZE = 24;
  static readonly int[] ScaleLevels = new int[] { 100, 125, 150, 175, 200, 225, 250, 300, 350 };
  public static int GetScalePercent() {
    int current;
    int[] available;
    GetScaleInfo(out current, out available);
    return current;
  }
  public static int[] GetScaleLevels() {
    int current;
    int[] available;
    GetScaleInfo(out current, out available);
    return available;
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
    LUID adapterId;
    uint sourceId;
    if (!GetPrimarySourceInfo(out adapterId, out sourceId)) {
      return SetScaleFallback(percent);
    }
    int getType;
    int min;
    int cur;
    int max;
    if (!RawGetDpiScale(adapterId, sourceId, out getType, out min, out cur, out max)) {
      return SetScaleFallback(percent);
    }
    int curIdx = FindClosestIndex(DpiToPercent(GetPrimaryDpi()));
    int recIdx = Clamp(curIdx - cur, 0, ScaleLevels.Length - 1);
    int scaleRel = Clamp(FindClosestIndex(percent) - recIdx, min, max);
    int setType = getType == -4 ? -3 : -4;

    IntPtr buf = Marshal.AllocHGlobal(SET_SIZE);
    try {
      for (int i = 0; i < SET_SIZE; i += 4) Marshal.WriteInt32(buf, i, 0);
      Marshal.WriteInt32(buf, 0, setType);
      Marshal.WriteInt32(buf, 4, SET_SIZE);
      Marshal.WriteInt32(buf, 8, (int)adapterId.LowPart);
      Marshal.WriteInt32(buf, 12, adapterId.HighPart);
      Marshal.WriteInt32(buf, 16, (int)sourceId);
      Marshal.WriteInt32(buf, 20, scaleRel);
      int result = DisplayConfigSetDeviceInfoRaw(buf);
      if (result == 0) {
        return true;
      }
    } finally {
      Marshal.FreeHGlobal(buf);
    }

    return SetScaleFallback(percent);
  }
  static void GetScaleInfo(out int currentPercent, out int[] availablePercentages) {
    currentPercent = GetConfiguredScalePercent();
    availablePercentages = ScaleLevels;

    LUID adapterId;
    uint sourceId;
    if (!GetPrimarySourceInfo(out adapterId, out sourceId)) {
      return;
    }
    int getType;
    int min;
    int cur;
    int max;
    if (!RawGetDpiScale(adapterId, sourceId, out getType, out min, out cur, out max)) {
      return;
    }

    int curIdx = FindClosestIndex(currentPercent);
    int recIdx = Clamp(curIdx - cur, 0, ScaleLevels.Length - 1);
    int minIdx = Math.Max(0, recIdx + min);
    int maxIdx = Math.Min(ScaleLevels.Length - 1, recIdx + max);
    int clampedIdx = Clamp(curIdx, minIdx, maxIdx);
    int length = maxIdx - minIdx + 1;
    int[] levels = new int[length];
    Array.Copy(ScaleLevels, minIdx, levels, 0, length);
    currentPercent = ScaleLevels[clampedIdx];
    availablePercentages = levels;
  }
  static int GetConfiguredScalePercent() {
    try {
      object value = Registry.GetValue(@"HKEY_CURRENT_USER\Control Panel\Desktop", "LogPixels", null);
      if (value is int) {
        return DpiToPercent((int)value);
      }
    } catch {
    }
    return DpiToPercent(GetPrimaryDpi());
  }
  static bool RawGetDpiScale(
    LUID adapterId,
    uint sourceId,
    out int workingType,
    out int min,
    out int cur,
    out int max
  ) {
    int[] types = new int[] { -4, -3 };
    for (int index = 0; index < types.Length; index++) {
      int type = types[index];
      IntPtr buf = Marshal.AllocHGlobal(GET_SIZE);
      try {
        for (int i = 0; i < GET_SIZE; i += 4) Marshal.WriteInt32(buf, i, 0);
        Marshal.WriteInt32(buf, 0, type);
        Marshal.WriteInt32(buf, 4, GET_SIZE);
        Marshal.WriteInt32(buf, 8, (int)adapterId.LowPart);
        Marshal.WriteInt32(buf, 12, adapterId.HighPart);
        Marshal.WriteInt32(buf, 16, (int)sourceId);
        int result = DisplayConfigGetDeviceInfoRaw(buf);
        min = Marshal.ReadInt32(buf, 20);
        cur = Marshal.ReadInt32(buf, 24);
        max = Marshal.ReadInt32(buf, 28);
        if (result == 0) {
          workingType = type;
          return true;
        }
      } finally {
        Marshal.FreeHGlobal(buf);
      }
    }
    workingType = 0;
    min = 0;
    cur = 0;
    max = 0;
    return false;
  }
  static bool SetScaleFallback(int percent) {
    try {
      int dpi = PercentToDpi(percent);
      RegistryKey key = Registry.CurrentUser.OpenSubKey(@"Control Panel\Desktop", true);
      if (key != null) {
        key.SetValue("LogPixels", dpi, RegistryValueKind.DWord);
        key.SetValue("Win8DpiScaling", 1, RegistryValueKind.DWord);
        key.Close();
      }
      System.Diagnostics.Process[] explorer = System.Diagnostics.Process.GetProcessesByName("explorer");
      for (int i = 0; i < explorer.Length; i++) {
        try {
          explorer[i].Kill();
        } catch {
        }
      }
      System.Threading.Thread.Sleep(500);
      System.Diagnostics.Process.Start("explorer.exe");
      return true;
    } catch {
      return false;
    }
  }
  static bool GetPrimarySourceInfo(out LUID adapterId, out uint sourceId) {
    adapterId = new LUID();
    sourceId = 0;
    uint pathCount;
    uint modeCount;
    if (GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, out pathCount, out modeCount) != 0) {
      return false;
    }
    DISPLAYCONFIG_PATH_INFO[] paths = new DISPLAYCONFIG_PATH_INFO[pathCount];
    DISPLAYCONFIG_MODE_INFO[] modes = new DISPLAYCONFIG_MODE_INFO[modeCount];
    if (QueryDisplayConfig(
      QDC_ONLY_ACTIVE_PATHS,
      ref pathCount,
      paths,
      ref modeCount,
      modes,
      IntPtr.Zero
    ) != 0) {
      return false;
    }
    if (paths.Length == 0) {
      return false;
    }
    adapterId = paths[0].sourceInfo.adapterId;
    sourceId = paths[0].sourceInfo.id;
    return true;
  }
  static int GetPrimaryDpi() {
    IntPtr monitor = MonitorFromWindow(IntPtr.Zero, MONITOR_DEFAULTTOPRIMARY);
    uint dpiX;
    uint dpiY;
    if (GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, out dpiX, out dpiY) == 0) {
      return (int)dpiX;
    }
    return 96;
  }
  static int DpiToPercent(int dpi) {
    return (int)Math.Round(dpi * 100.0 / 96.0);
  }
  static int PercentToDpi(int percent) {
    return (int)Math.Round(percent * 96.0 / 100.0);
  }
  static int FindClosestIndex(int percent) {
    int best = 0;
    for (int i = 1; i < ScaleLevels.Length; i++) {
      if (Math.Abs(ScaleLevels[i] - percent) < Math.Abs(ScaleLevels[best] - percent)) {
        best = i;
      }
    }
    return best;
  }
  static int Clamp(int value, int min, int max) {
    if (value < min) return min;
    if (value > max) return max;
    return value;
  }
}
"@ -Language CSharp | Out-Null

function Get-StatusObject {
  $resolution = [CopperWinDisplay]::GetResolution()
  $resolutionModes = @([CopperWinDisplay]::GetResolutionModes() | ForEach-Object {
    [pscustomobject]@{
      width = $_[0]
      height = $_[1]
      refreshRate = $_[2]
    }
  })
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
      availableModes = $resolutionModes
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
    use super::{apply_taskbar_state, execute_action_with_runner, finalize_taskbar_action, Value};
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

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_bridge_script_uses_live_display_config_for_scale() {
        let script = super::windows_bridge_script();
        assert!(script.contains("DisplayConfigSetDeviceInfoRaw"));
        assert!(script.contains("QueryDisplayConfig"));
        assert!(script.contains("GetConfiguredScalePercent"));
        assert!(script.contains("GetResolutionModes"));
        assert!(script.contains("availableModes = $resolutionModes"));
        assert!(!script.contains("$status.requiresSignOut = $true"));
    }

    #[test]
    fn apply_taskbar_state_updates_pinned_flag_from_auto_hide() {
        let mut value = json!({ "action": "status" });
        apply_taskbar_state(&mut value, false);
        assert_eq!(
            value.get("taskbarAutoHide").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            value.get("taskbarPinned").and_then(Value::as_bool),
            Some(true)
        );

        apply_taskbar_state(&mut value, true);
        assert_eq!(
            value.get("taskbarAutoHide").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value.get("taskbarPinned").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn finalize_taskbar_action_adds_requested_payload() {
        let mut value = json!({});
        finalize_taskbar_action(&mut value, "set-taskbar-autohide", true, Some(true));
        assert_eq!(
            value.get("action").and_then(Value::as_str),
            Some("set-taskbar-autohide")
        );
        assert_eq!(value.get("applied").and_then(Value::as_bool), Some(true));
        assert_eq!(
            value
                .get("requested")
                .and_then(|requested| requested.get("taskbarAutoHide"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }
}

//! Platform operations shared by the native GPUI host and the WebView host: clipboard, native
//! dialogs, external links, power blockers and the allowlisted HTTPS transport. Each runs once here,
//! so both renderers answer the same requests the same way. Clipboard, dialogs and power blockers
//! are implemented on Windows, macOS and Linux; safeStorage uses DPAPI on Windows and reports
//! unavailable elsewhere.

use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

/// Power state as `powerMonitor` reports it. Unknown values stay `None` rather than guessing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerSnapshot {
    pub on_battery: Option<bool>,
    pub idle_time: Option<u64>,
    pub thermal_state: String,
    pub locked: Option<bool>,
}

impl PowerSnapshot {
    fn to_json(&self) -> Value {
        json!({
            "onBattery": self.on_battery,
            "idleTime": self.idle_time.unwrap_or(0),
            "thermalState": if self.thermal_state.is_empty() { "unknown" } else { &self.thermal_state },
            "locked": self.locked,
        })
    }
}

#[cfg(target_os = "windows")]
fn platform_power_snapshot() -> PowerSnapshot { windows::power::snapshot() }
#[cfg(target_os = "macos")]
fn platform_power_snapshot() -> PowerSnapshot { macos::power_snapshot() }
#[cfg(target_os = "linux")]
fn platform_power_snapshot() -> PowerSnapshot { linux::power_snapshot() }
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_power_snapshot() -> PowerSnapshot { PowerSnapshot { thermal_state: "unknown".into(), ..PowerSnapshot::default() } }

#[cfg(target_os = "windows")]
fn platform_power_events() -> Vec<String> { windows::power::take_events() }
#[cfg(not(target_os = "windows"))]
fn platform_power_events() -> Vec<String> { Vec::new() }

/// The current power state for the `power.state` host operation.
pub fn power_snapshot() -> Value { platform_power_snapshot().to_json() }

/// Power transition names collected since the last call. Explicit OS notifications (Windows
/// power broadcasts) are merged with state changes the host can observe by polling.
pub fn take_power_events() -> Vec<String> {
    let mut events = platform_power_events();
    let snapshot = platform_power_snapshot();
    static WATCH: OnceLock<Mutex<Option<(Option<bool>, Option<bool>)>>> = OnceLock::new();
    let mut watch = WATCH.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|error| error.into_inner());
    if let Some((previous_battery, previous_lock)) = *watch {
        if let (Some(previous), Some(current)) = (previous_battery, snapshot.on_battery) {
            if previous != current { events.push(if current { "on-battery" } else { "on-ac" }.to_string()); }
        }
        if let (Some(previous), Some(current)) = (previous_lock, snapshot.locked) {
            if previous != current { events.push(if current { "lock-screen" } else { "unlock-screen" }.to_string()); }
        }
    }
    *watch = Some((snapshot.on_battery, snapshot.locked));
    // An explicit Windows notification and the polled diff can describe the same transition.
    let mut unique: Vec<String> = Vec::with_capacity(events.len());
    for event in events {
        if unique.last() != Some(&event) { unique.push(event); }
    }
    unique
}

#[cfg(target_os = "windows")]
fn platform_system_preferences() -> Value { windows::system_preferences() }
#[cfg(target_os = "macos")]
fn platform_system_preferences() -> Value { macos::system_preferences() }
#[cfg(target_os = "linux")]
fn platform_system_preferences() -> Value { linux::system_preferences() }
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_system_preferences() -> Value {
    json!({"darkMode": false, "accentColor": "", "colors": {}, "reduceMotion": false, "invertedColors": false, "highContrast": false, "reduceTransparency": false})
}

/// The current OS appearance and accessibility settings for `systemPreferences`.
pub fn system_preferences() -> Value { platform_system_preferences() }

/// A light/dark palette for platforms whose native colors are not queried, and for renderer-only
/// fallbacks. Keys are the names Electron's `systemPreferences.getColor` accepts.
#[cfg(not(target_os = "windows"))]
fn standard_colors(dark: bool) -> Value {
    let colors: [(&str, &str, &str); 31] = [
        ("3d-dark-shadow", "#696969", "#202020"), ("3d-face", "#f0f0f0", "#2d2d2d"),
        ("3d-highlight", "#ffffff", "#454545"), ("3d-light", "#e3e3e3", "#3a3a3a"),
        ("3d-shadow", "#a0a0a0", "#202020"), ("active-border", "#707070", "#555555"),
        ("active-caption", "#99b4d1", "#202020"), ("active-caption-gradient", "#99b4d1", "#202020"),
        ("app-workspace", "#ababab", "#1e1e1e"), ("button-face", "#f0f0f0", "#2d2d2d"),
        ("button-text", "#000000", "#ffffff"), ("caption-text", "#000000", "#ffffff"),
        ("desktop", "#000000", "#000000"), ("highlight", "#0078d7", "#0078d7"),
        ("highlight-text", "#ffffff", "#ffffff"), ("hotlight", "#0066cc", "#4da3ff"),
        ("inactive-border", "#acacac", "#444444"), ("inactive-caption", "#bfcddb", "#2b2b2b"),
        ("inactive-caption-gradient", "#bfcddb", "#2b2b2b"), ("inactive-caption-text", "#434e54", "#b8b8b8"),
        ("info-background", "#ffffe1", "#2b2b2b"), ("info-text", "#000000", "#ffffff"),
        ("menu", "#f0f0f0", "#202020"), ("menu-highlight", "#0078d7", "#0078d7"),
        ("menubar", "#f0f0f0", "#202020"), ("menu-text", "#000000", "#ffffff"),
        ("scrollbar", "#c8c8c8", "#3a3a3a"), ("window", "#ffffff", "#1e1e1e"),
        ("window-frame", "#646464", "#444444"), ("window-text", "#000000", "#ffffff"),
        // Kept in the list so the count stays obvious if another Electron color is added.
        ("", "", ""),
    ];
    let mut map = serde_json::Map::new();
    for (name, light, dark_color) in colors {
        if name.is_empty() { continue; }
        map.insert(name.to_string(), Value::String(if dark { dark_color } else { light }.to_string()));
    }
    Value::Object(map)
}

#[cfg(not(target_os = "windows"))]
const WINDOWS_ONLY: &str = "This operation is supported on Windows only";

/// Reads a JSON array of strings, ignoring entries that are not strings.
pub fn string_list(value: &Value) -> Vec<String> {
    value.as_array().map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

#[cfg(target_os = "windows")]
pub fn clipboard_read_text() -> Result<String, String> {
    windows::clipboard::read_text()
}

#[cfg(target_os = "macos")]
pub fn clipboard_read_text() -> Result<String, String> {
    macos::clipboard_read_text()
}

#[cfg(target_os = "linux")]
pub fn clipboard_read_text() -> Result<String, String> {
    linux::clipboard_read_text()
}

#[cfg(target_os = "windows")]
pub fn clipboard_write_text(text: &str) -> Result<(), String> {
    windows::clipboard::write_text(text)
}

#[cfg(target_os = "macos")]
pub fn clipboard_write_text(text: &str) -> Result<(), String> {
    macos::clipboard_write_text(text)
}

#[cfg(target_os = "linux")]
pub fn clipboard_write_text(text: &str) -> Result<(), String> {
    linux::clipboard_write_text(text)
}

/// Shows an open dialog. Result: `{ canceled, paths }`.
#[cfg(target_os = "windows")]
pub fn open_file_dialog(args: &Value) -> Result<Value, String> {
    Ok(windows::open_file_dialog(args))
}

#[cfg(not(target_os = "windows"))]
pub fn open_file_dialog(args: &Value) -> Result<Value, String> {
    Ok(dialogs::open_file_dialog(args))
}

/// Shows a save dialog. Result: `{ canceled, path }`.
#[cfg(target_os = "windows")]
pub fn save_file_dialog(args: &Value) -> Result<Value, String> {
    Ok(windows::save_file_dialog(args))
}

#[cfg(not(target_os = "windows"))]
pub fn save_file_dialog(args: &Value) -> Result<Value, String> {
    Ok(dialogs::save_file_dialog(args))
}

/// Shows a message box. Result: `{ response }`, the index of the chosen button.
#[cfg(target_os = "windows")]
pub fn message_dialog(args: &Value) -> Result<Value, String> {
    windows::message_dialog(args)
}

#[cfg(not(target_os = "windows"))]
pub fn message_dialog(args: &Value) -> Result<Value, String> {
    dialogs::message_dialog(args)
}

/// Sets the power blockers. `types` holds `prevent-app-suspension` and/or `prevent-display-sleep`;
/// an empty list clears them.
#[cfg(target_os = "windows")]
pub fn set_power_blockers(types: &[String]) -> Result<(), String> {
    windows::power::set_blockers(types)
}

#[cfg(target_os = "macos")]
pub fn set_power_blockers(types: &[String]) -> Result<(), String> {
    macos::set_power_blockers(types)
}

#[cfg(target_os = "linux")]
pub fn set_power_blockers(types: &[String]) -> Result<(), String> {
    linux::set_power_blockers(types)
}

/// Runs a `safeStorage.*` operation. Values are protected with DPAPI on Windows, so only the current
/// Windows account on this machine can decrypt them. Ciphertext crosses the bridge as base64.
pub fn safe_storage_call(operation: &str, args: &[Value]) -> Result<Value, String> {
    let text_arg = || args.first().and_then(Value::as_str).ok_or_else(|| "Argument 1 must be a string".to_string());
    match operation {
        "safeStorage.isAvailable" => Ok(Value::Bool(cfg!(target_os = "windows"))),
        "safeStorage.encrypt" => Ok(Value::from(base64_encode(&safe_storage_protect(text_arg()?.as_bytes())?))),
        "safeStorage.decrypt" => {
            let plain = safe_storage_unprotect(&base64_decode(text_arg()?)?)?;
            String::from_utf8(plain).map(Value::from).map_err(|_| "Decrypted value is not valid UTF-8".to_string())
        }
        _ => Err(format!("unsupported op: {operation}")),
    }
}

#[cfg(target_os = "windows")]
fn safe_storage_protect(plain: &[u8]) -> Result<Vec<u8>, String> {
    windows::dpapi::protect(plain)
}

#[cfg(target_os = "windows")]
fn safe_storage_unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
    windows::dpapi::unprotect(cipher)
}

#[cfg(not(target_os = "windows"))]
fn safe_storage_protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("safeStorage is supported on Windows only".into())
}

#[cfg(not(target_os = "windows"))]
fn safe_storage_unprotect(_cipher: &[u8]) -> Result<Vec<u8>, String> {
    Err("safeStorage is supported on Windows only".into())
}

pub fn base64_encode(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ABC[((n >> 18) & 63) as usize] as char);
        out.push(ABC[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { ABC[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { ABC[(n & 63) as usize] as char } else { '=' });
    }
    out
}

pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for b in text.bytes() {
        if b == b'=' {
            break;
        }
        let n = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("Invalid base64".into()),
        };
        acc = (acc << 6) | u32::from(n);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Opens an https or mailto link in the system handler. Other schemes and control characters are refused.
pub fn open_external(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("mailto:")) || url.chars().any(char::is_control) {
        return Err("Only https and mailto URLs are allowed".into());
    }
    #[cfg(target_os = "windows")]
    let (command, argv) = ("rundll32", vec!["url.dll,FileProtocolHandler", url]);
    #[cfg(target_os = "macos")]
    let (command, argv) = ("open", vec![url]);
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (command, argv) = ("xdg-open", vec![url]);
    std::process::Command::new(command).args(argv).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Sends one HTTPS request whose host is in `hosts`. Redirects are not followed, and the request
/// and response bodies are capped at 1 MiB. Result: `{ status, headers, body }`.
pub fn https_request(args: &Value, hosts: &[String]) -> Result<Value, String> {
    use std::io::Read;
    let url = args["url"].as_str().ok_or("url must be a string")?;
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().ok_or("URL has no host")?;
    if parsed.scheme() != "https" || !hosts.iter().any(|allowed| allowed.eq_ignore_ascii_case(host)) {
        return Err("PERMISSION_DENIED: Network host is not granted or URL is not HTTPS".into());
    }
    let body = args["body"].as_str().unwrap_or("").to_string();
    if body.len() > 1024 * 1024 {
        return Err("Request body exceeds 1 MiB".into());
    }
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).redirects(0).build();
    let method = args["method"].as_str().unwrap_or("GET").to_string();
    let url = url.to_string();
    let headers_map = args["headers"].clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut request = agent.request(&method, &url);
        if let Some(headers) = headers_map.as_object() {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    request = request.set(key, v);
                }
            }
        }
        let result = match request.send_string(&body) {
            Ok(r) => {
                let status = r.status();
                let mut headers = serde_json::Map::new();
                for key in r.headers_names() {
                    if let Some(value) = r.header(&key) {
                        headers.insert(key, json!(value));
                    }
                }
                let mut bytes = Vec::new();
                let mut reader = r.into_reader();
                let mut chunk = [0u8; 8192];
                loop {
                    if crate::capabilities::current_cancel_token().map(|t| t.load(std::sync::atomic::Ordering::SeqCst)).unwrap_or(false) {
                        return;
                    }
                    match reader.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if bytes.len() + n > 1024 * 1024 {
                                let _ = tx.send(Err("Response body exceeds 1 MiB".to_string()));
                                return;
                            }
                            bytes.extend_from_slice(&chunk[..n]);
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    }
                }
                let _ = tx.send(Ok(json!({ "status": status, "headers": headers, "body": String::from_utf8_lossy(&bytes) })));
            }
            Err(ureq::Error::Status(_, r)) => {
                let status = r.status();
                let mut headers = serde_json::Map::new();
                for key in r.headers_names() {
                    if let Some(value) = r.header(&key) {
                        headers.insert(key, json!(value));
                    }
                }
                let mut bytes = Vec::new();
                let mut reader = r.into_reader();
                let mut chunk = [0u8; 8192];
                loop {
                    if crate::capabilities::current_cancel_token().map(|t| t.load(std::sync::atomic::Ordering::SeqCst)).unwrap_or(false) {
                        return;
                    }
                    match reader.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if bytes.len() + n > 1024 * 1024 {
                                let _ = tx.send(Err("Response body exceeds 1 MiB".to_string()));
                                return;
                            }
                            bytes.extend_from_slice(&chunk[..n]);
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    }
                }
                let _ = tx.send(Ok(json!({ "status": status, "headers": headers, "body": String::from_utf8_lossy(&bytes) })));
            }
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
            }
        };
        let _ = result;
    });
    loop {
        if crate::capabilities::current_cancel_token().map(|t| t.load(std::sync::atomic::Ordering::SeqCst)).unwrap_or(false) {
            return Err("CANCELLED: HTTPS request was cancelled".into());
        }
        match rx.recv_timeout(std::time::Duration::from_millis(10)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("HTTPS request failed".into()),
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{string_list, PowerSnapshot};
    use core::ffi::c_void;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Mutex, OnceLock};
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Graphics::Gdi::{
        GetSysColor, COLOR_3DDKSHADOW, COLOR_3DFACE, COLOR_3DHIGHLIGHT, COLOR_3DLIGHT, COLOR_3DSHADOW,
        COLOR_ACTIVEBORDER, COLOR_ACTIVECAPTION, COLOR_APPWORKSPACE, COLOR_BTNTEXT, COLOR_CAPTIONTEXT,
        COLOR_DESKTOP, COLOR_GRADIENTACTIVECAPTION, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT,
        COLOR_INACTIVEBORDER, COLOR_INACTIVECAPTION, COLOR_INACTIVECAPTIONTEXT, COLOR_INFOBK, COLOR_INFOTEXT,
        COLOR_MENU, COLOR_MENUHILIGHT, COLOR_MENUBAR, COLOR_MENUTEXT, COLOR_SCROLLBAR, COLOR_WINDOW,
        COLOR_WINDOWFRAME, COLOR_WINDOWTEXT,
    };
    use windows_sys::Win32::System::Power::{
        GetSystemPowerStatus, PowerRegisterSuspendResumeNotification, PowerSettingRegisterNotification,
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, POWERBROADCAST_SETTING, SYSTEM_POWER_STATUS,
    };
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, DEVICE_NOTIFY_CALLBACK, PBT_APMPOWERSTATUSCHANGE, PBT_APMRESUMEAUTOMATIC,
        PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE,
        SPI_GETCLIENTAREAANIMATION,
    };

    fn apply_filters(mut dialog: rfd::FileDialog, filters: &Value) -> rfd::FileDialog {
        for filter in filters.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let name = filter["name"].as_str().unwrap_or("Files").to_owned();
            let extensions = string_list(&filter["extensions"]);
            dialog = dialog.add_filter(name, extensions.as_slice());
        }
        dialog
    }

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn registry_dword(subkey: &str, name: &str) -> Option<u32> {
        let subkey = wide(subkey);
        let name = wide(name);
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut kind = 0u32;
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, &mut kind,
                &mut value as *mut u32 as *mut c_void, &mut size)
        };
        (status == 0).then_some(value)
    }

    fn registry_string(subkey: &str, name: &str) -> Option<String> {
        let subkey = wide(subkey);
        let name = wide(name);
        let mut buffer = [0u16; 512];
        let mut size = (buffer.len() * std::mem::size_of::<u16>()) as u32;
        let mut kind = 0u32;
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, &mut kind,
                buffer.as_mut_ptr() as *mut c_void, &mut size)
        };
        if status != 0 { return None; }
        let length = (size as usize / std::mem::size_of::<u16>()).saturating_sub(1).min(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..length]))
    }

    fn colorref(value: u32) -> String {
        let (red, green, blue) = (value & 0xff, (value >> 8) & 0xff, (value >> 16) & 0xff);
        format!("#{red:02x}{green:02x}{blue:02x}")
    }

    fn system_colors() -> Value {
        let colors = [
            ("3d-dark-shadow", COLOR_3DDKSHADOW), ("3d-face", COLOR_3DFACE),
            ("3d-highlight", COLOR_3DHIGHLIGHT), ("3d-light", COLOR_3DLIGHT),
            ("3d-shadow", COLOR_3DSHADOW), ("active-border", COLOR_ACTIVEBORDER),
            ("active-caption", COLOR_ACTIVECAPTION), ("active-caption-gradient", COLOR_GRADIENTACTIVECAPTION),
            ("app-workspace", COLOR_APPWORKSPACE), ("button-face", COLOR_3DFACE),
            ("button-text", COLOR_BTNTEXT), ("caption-text", COLOR_CAPTIONTEXT),
            ("desktop", COLOR_DESKTOP), ("highlight", COLOR_HIGHLIGHT),
            ("highlight-text", COLOR_HIGHLIGHTTEXT), ("hotlight", COLOR_HOTLIGHT),
            ("inactive-border", COLOR_INACTIVEBORDER), ("inactive-caption", COLOR_INACTIVECAPTION),
            ("inactive-caption-gradient", COLOR_INACTIVECAPTION), ("inactive-caption-text", COLOR_INACTIVECAPTIONTEXT),
            ("info-background", COLOR_INFOBK), ("info-text", COLOR_INFOTEXT),
            ("menu", COLOR_MENU), ("menu-highlight", COLOR_MENUHILIGHT),
            ("menubar", COLOR_MENUBAR), ("menu-text", COLOR_MENUTEXT),
            ("scrollbar", COLOR_SCROLLBAR), ("window", COLOR_WINDOW),
            ("window-frame", COLOR_WINDOWFRAME), ("window-text", COLOR_WINDOWTEXT),
        ];
        let mut map = serde_json::Map::new();
        for (name, index) in colors {
            map.insert(name.to_string(), Value::String(colorref(unsafe { GetSysColor(index) })));
        }
        Value::Object(map)
    }

    fn animations_enabled() -> bool {
        let mut enabled: i32 = 1;
        let applied = unsafe {
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut enabled as *mut i32 as *mut c_void, 0)
        };
        applied == 0 || enabled != 0
    }

    pub fn system_preferences() -> Value {
        let dark = registry_dword("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", "AppsUseLightTheme")
            .map(|value| value == 0)
            .unwrap_or(false);
        let accent = registry_dword("Software\\Microsoft\\Windows\\DWM", "ColorizationColor")
            .map(|value| format!("{:02x}{:02x}{:02x}", (value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff))
            .unwrap_or_default();
        let high_contrast = registry_string("Control Panel\\Accessibility\\HighContrast", "Flags")
            .and_then(|value| value.trim().parse::<u32>().ok())
            .map(|flags| flags & 1 != 0)
            .unwrap_or(false);
        let reduce_transparency = registry_dword("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", "EnableTransparency")
            .map(|value| value == 0)
            .unwrap_or(false);
        json!({
            "darkMode": dark,
            "accentColor": accent,
            "colors": system_colors(),
            "reduceMotion": !animations_enabled(),
            "invertedColors": false,
            "highContrast": high_contrast,
            "reduceTransparency": reduce_transparency,
            "platform": "windows",
        })
    }

    pub fn open_file_dialog(args: &Value) -> Value {
        let dialog = apply_filters(rfd::FileDialog::new(), &args["filters"]);
        if args["multiple"].as_bool().unwrap_or(false) {
            match dialog.pick_files() {
                Some(paths) => json!({"canceled": false, "paths": paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>()}),
                None => json!({"canceled": true, "paths": []}),
            }
        } else {
            match dialog.pick_file() {
                Some(path) => json!({"canceled": false, "paths": [path.to_string_lossy().into_owned()]}),
                None => json!({"canceled": true, "paths": []}),
            }
        }
    }

    pub fn save_file_dialog(args: &Value) -> Value {
        let mut dialog = apply_filters(rfd::FileDialog::new(), &args["filters"]);
        if let Some(default) = args["defaultPath"].as_str() {
            let path = std::path::Path::new(default);
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                dialog = dialog.set_directory(parent);
            }
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
            }
        }
        match dialog.save_file() {
            Some(path) => json!({"canceled": false, "path": path.to_string_lossy().into_owned()}),
            None => json!({"canceled": true, "path": null}),
        }
    }

    /// Shows a message with up to three buttons. Closing the dialog counts as choosing the last
    /// button, which is the usual cancel position.
    pub fn message_dialog(args: &Value) -> Result<Value, String> {
        use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
        let labels = match string_list(&args["buttons"]) {
            list if list.is_empty() => vec!["OK".to_owned()],
            list => list,
        };
        let buttons = match labels.as_slice() {
            [a] => MessageButtons::OkCustom(a.clone()),
            [a, b] => MessageButtons::OkCancelCustom(a.clone(), b.clone()),
            [a, b, c] => MessageButtons::YesNoCancelCustom(a.clone(), b.clone(), c.clone()),
            other => return Err(format!("message dialogs support up to three buttons, not {}", other.len())),
        };
        let level = match args["type"].as_str() {
            Some("error") => MessageLevel::Error,
            Some("warning") => MessageLevel::Warning,
            _ => MessageLevel::Info,
        };
        let result = MessageDialog::new()
            .set_title(args["title"].as_str().unwrap_or(""))
            .set_description(args["message"].as_str().unwrap_or(""))
            .set_level(level)
            .set_buttons(buttons)
            .show();
        let response = match result {
            MessageDialogResult::Custom(label) => labels.iter().position(|l| *l == label).unwrap_or(0),
            MessageDialogResult::Ok | MessageDialogResult::Yes => 0,
            MessageDialogResult::No => 1,
            MessageDialogResult::Cancel => labels.len() - 1,
        };
        Ok(json!({"response": response}))
    }

    /// DPAPI (CryptProtectData) keys the ciphertext to the current user account on this machine.
    pub mod dpapi {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

        /// Copies a DPAPI output blob into a Vec and releases the buffer that DPAPI allocated.
        fn take(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
            if blob.pbData.is_null() || blob.cbData == 0 {
                if !blob.pbData.is_null() {
                    unsafe { LocalFree(blob.pbData as _) };
                }
                return Vec::new();
            }
            let bytes = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
            unsafe { LocalFree(blob.pbData as _) };
            bytes
        }

        fn input(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB, String> {
            let len = u32::try_from(data.len()).map_err(|_| "Value is too large".to_string())?;
            Ok(CRYPT_INTEGER_BLOB { cbData: len, pbData: data.as_ptr() as *mut u8 })
        }

        pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
            let data = input(plain)?;
            let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
            let ok = unsafe { CryptProtectData(&data, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
            if ok == 0 {
                return Err(format!("CryptProtectData failed: {}", std::io::Error::last_os_error()));
            }
            Ok(take(out))
        }

        pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
            let data = input(cipher)?;
            let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
            let ok = unsafe { CryptUnprotectData(&data, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
            if ok == 0 {
                return Err("Value cannot be decrypted by this user account".into());
            }
            Ok(take(out))
        }
    }

    /// Power requests are per thread on Windows, so one dedicated thread owns them. Other threads
    /// send it the flags and wait for the result, which keeps the blockers alive for the process.
    pub mod power {
        use super::*;
        use windows_sys::Win32::System::Power::SetThreadExecutionState;

        type Request = (u32, mpsc::Sender<bool>);
        static OWNER: OnceLock<Mutex<mpsc::Sender<Request>>> = OnceLock::new();

        fn owner() -> &'static Mutex<mpsc::Sender<Request>> {
            OWNER.get_or_init(|| {
                let (sender, receiver) = mpsc::channel::<Request>();
                std::thread::spawn(move || {
                    for (flags, reply) in receiver {
                        // A zero result means the system refused the change.
                        let applied = unsafe { SetThreadExecutionState(flags) } != 0;
                        let _ = reply.send(applied);
                    }
                });
                Mutex::new(sender)
            })
        }

        pub fn set_blockers(types: &[String]) -> Result<(), String> {
            // ES_CONTINUOUS makes the state last until another call changes it.
            const ES_CONTINUOUS: u32 = 0x8000_0000;
            const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
            const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
            let mut flags = ES_CONTINUOUS;
            for kind in types {
                match kind.as_str() {
                    "prevent-app-suspension" => flags |= ES_SYSTEM_REQUIRED,
                    "prevent-display-sleep" => flags |= ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED,
                    other => return Err(format!("Unknown power save blocker: {other}")),
                }
            }
            let (reply, answer) = mpsc::channel();
            owner().lock().unwrap_or_else(|error| error.into_inner()).send((flags, reply)).map_err(|e| e.to_string())?;
            match answer.recv() {
                Ok(true) => Ok(()),
                Ok(false) => Err("Unable to change the power state".into()),
                Err(e) => Err(e.to_string()),
            }
        }

        // ---- Live power notifications -------------------------------------------------------
        // Windows delivers suspend/resume, AC/battery and lock/unlock changes through
        // power-setting callbacks. `PowerRegisterSuspendResumeNotification` covers sleep/wake and
        // `PowerSettingRegisterNotification` covers GUID_ACDC_POWER_SOURCE and
        // GUID_SESSION_USER_PRESENCE. Callback registrations avoid a message window and message
        // loop, so they work in both hosts.
        static EVENTS: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
        static LOCKED: AtomicBool = AtomicBool::new(false);
        static REGISTERED: OnceLock<()> = OnceLock::new();

        const GUID_ACDC_POWER_SOURCE: GUID = GUID::from_u128(0x5d3e9a59_e9d5_4b00_a6bd_ff34ff516548);
        const GUID_SESSION_USER_PRESENCE: GUID = GUID::from_u128(0x2b84c20e_ad23_4ddf_93db_05ffbd7efca5);

        fn events() -> &'static Mutex<VecDeque<String>> {
            EVENTS.get_or_init(|| Mutex::new(VecDeque::new()))
        }

        fn push_event(name: &str) {
            events().lock().unwrap_or_else(|error| error.into_inner()).push_back(name.to_string());
        }

        fn same_guid(left: &GUID, right: &GUID) -> bool {
            left.data1 == right.data1 && left.data2 == right.data2 && left.data3 == right.data3 && left.data4 == right.data4
        }

        unsafe extern "system" fn power_callback(_context: *const c_void, kind: u32, setting: *const c_void) -> u32 {
            match kind {
                PBT_APMSUSPEND => push_event("suspend"),
                PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMECRITICAL => push_event("resume"),
                PBT_APMPOWERSTATUSCHANGE => {
                    if let Some(on_battery) = current_on_battery() {
                        push_event(if on_battery { "on-battery" } else { "on-ac" });
                    }
                }
                PBT_POWERSETTINGCHANGE if !setting.is_null() => {
                    // POWERBROADCAST_SETTING carries a trailing DWORD value. The OS owns the
                    // buffer for the duration of this callback.
                    let broadcast = unsafe { &*(setting as *const POWERBROADCAST_SETTING) };
                    if broadcast.DataLength >= std::mem::size_of::<u32>() as u32 {
                        let value = unsafe { std::ptr::read_unaligned(broadcast.Data.as_ptr() as *const u32) };
                        if same_guid(&broadcast.PowerSetting, &GUID_ACDC_POWER_SOURCE) {
                            match value {
                                0 => push_event("on-ac"),
                                1 => push_event("on-battery"),
                                _ => {}
                            }
                        } else if same_guid(&broadcast.PowerSetting, &GUID_SESSION_USER_PRESENCE) {
                            // 0 is user present (unlock); 1 is user absent (lock).
                            let locked = value == 1;
                            LOCKED.store(locked, Ordering::SeqCst);
                            push_event(if locked { "lock-screen" } else { "unlock-screen" });
                        }
                    }
                }
                _ => {}
            }
            0
        }

        fn register_notifications() {
            REGISTERED.get_or_init(|| unsafe {
                // The OS keeps this pointer for the callback registrations, so it must live for
                // the lifetime of the process.
                let parameters = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                    Callback: Some(power_callback),
                    Context: std::ptr::null_mut(),
                }));
                let recipient = parameters as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS as HANDLE;
                let mut handle: *mut c_void = std::ptr::null_mut();
                let _ = PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_CALLBACK, recipient, &mut handle);
                let _ = PowerSettingRegisterNotification(&GUID_ACDC_POWER_SOURCE, DEVICE_NOTIFY_CALLBACK, recipient, &mut handle);
                let _ = PowerSettingRegisterNotification(&GUID_SESSION_USER_PRESENCE, DEVICE_NOTIFY_CALLBACK, recipient, &mut handle);
            });
        }

        fn current_on_battery() -> Option<bool> {
            let mut status = SYSTEM_POWER_STATUS::default();
            if unsafe { GetSystemPowerStatus(&mut status) } == 0 { return None; }
            // BatteryFlag 128 means no system battery; 255 means the state is unknown.
            if status.BatteryFlag == 128 || status.BatteryFlag == 255 { return Some(false); }
            match status.ACLineStatus {
                0 => Some(true),
                1 => Some(false),
                _ => None,
            }
        }

        pub fn snapshot() -> PowerSnapshot {
            register_notifications();
            let idle_time = unsafe {
                let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
                if GetLastInputInfo(&mut info) == 0 {
                    None
                } else {
                    let elapsed = (GetTickCount64() as u32).wrapping_sub(info.dwTime);
                    Some(u64::from(elapsed) / 1000)
                }
            };
            PowerSnapshot {
                on_battery: current_on_battery(),
                idle_time,
                thermal_state: "unknown".into(),
                locked: Some(LOCKED.load(Ordering::SeqCst)),
            }
        }

        pub fn take_events() -> Vec<String> {
            register_notifications();
            events().lock().unwrap_or_else(|error| error.into_inner()).drain(..).collect()
        }
    }

    pub mod clipboard {
        use windows_sys::Win32::System::{
            DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            Ole::CF_UNICODETEXT,
        };

        /// Reads Unicode text from the clipboard. An empty or non-text clipboard reads as an empty string.
        pub fn read_text() -> Result<String, String> {
            unsafe {
                if OpenClipboard(std::ptr::null_mut()) == 0 {
                    return Err("Unable to open the clipboard".into());
                }
                let text = (|| {
                    let handle = GetClipboardData(CF_UNICODETEXT as u32);
                    if handle.is_null() {
                        return Ok(String::new());
                    }
                    let pointer = GlobalLock(handle) as *const u16;
                    if pointer.is_null() {
                        return Err("Unable to read the clipboard text".to_owned());
                    }
                    let mut length = 0usize;
                    while *pointer.add(length) != 0 {
                        length += 1;
                    }
                    let text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length));
                    GlobalUnlock(handle);
                    Ok(text)
                })();
                CloseClipboard();
                text
            }
        }

        /// Replaces the clipboard contents with the given Unicode text.
        pub fn write_text(text: &str) -> Result<(), String> {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * std::mem::size_of::<u16>());
                if memory.is_null() {
                    return Err("Unable to allocate clipboard memory".into());
                }
                let target = GlobalLock(memory) as *mut u16;
                if target.is_null() {
                    return Err("Unable to lock clipboard memory".into());
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
                GlobalUnlock(memory);
                if OpenClipboard(std::ptr::null_mut()) == 0 {
                    return Err("Unable to open the clipboard".into());
                }
                EmptyClipboard();
                let placed = SetClipboardData(CF_UNICODETEXT as u32, memory);
                CloseClipboard();
                if placed.is_null() {
                    return Err("Unable to set the clipboard text".into());
                }
                Ok(())
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod dialogs {
    use super::string_list;
    use serde_json::{json, Value};

    pub fn open_file_dialog(args: &Value) -> Value {
        let mut dialog = rfd::FileDialog::new();
        for filter in args["filters"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let name = filter["name"].as_str().unwrap_or("Files").to_owned();
            let extensions = string_list(&filter["extensions"]);
            dialog = dialog.add_filter(name, extensions.as_slice());
        }
        if args["multiple"].as_bool().unwrap_or(false) {
            match dialog.pick_files() {
                Some(paths) => json!({"canceled": false, "paths": paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>() }),
                None => json!({"canceled": true, "paths": []}),
            }
        } else {
            match dialog.pick_file() {
                Some(path) => json!({"canceled": false, "paths": [path.to_string_lossy().into_owned()]}),
                None => json!({"canceled": true, "paths": []}),
            }
        }
    }

    pub fn save_file_dialog(args: &Value) -> Value {
        let mut dialog = rfd::FileDialog::new();
        for filter in args["filters"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let name = filter["name"].as_str().unwrap_or("Files").to_owned();
            let extensions = string_list(&filter["extensions"]);
            dialog = dialog.add_filter(name, extensions.as_slice());
        }
        if let Some(default) = args["defaultPath"].as_str() {
            let path = std::path::Path::new(default);
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                dialog = dialog.set_directory(parent);
            }
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
            }
        }
        match dialog.save_file() {
            Some(path) => json!({"canceled": false, "path": path.to_string_lossy().into_owned()}),
            None => json!({"canceled": true, "path": null}),
        }
    }

    pub fn message_dialog(args: &Value) -> Result<Value, String> {
        use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
        let labels = match string_list(&args["buttons"]) {
            list if list.is_empty() => vec!["OK".to_owned()],
            list => list,
        };
        let buttons = match labels.as_slice() {
            [a] => MessageButtons::OkCustom(a.clone()),
            [a, b] => MessageButtons::OkCancelCustom(a.clone(), b.clone()),
            [a, b, c] => MessageButtons::YesNoCancelCustom(a.clone(), b.clone(), c.clone()),
            other => return Err(format!("message dialogs support up to three buttons, not {}", other.len())),
        };
        let level = match args["type"].as_str() {
            Some("error") => MessageLevel::Error,
            Some("warning") => MessageLevel::Warning,
            _ => MessageLevel::Info,
        };
        let result = MessageDialog::new()
            .set_title(args["title"].as_str().unwrap_or(""))
            .set_description(args["message"].as_str().unwrap_or(""))
            .set_level(level)
            .set_buttons(buttons)
            .show();
        let response = match result {
            MessageDialogResult::Custom(label) => labels.iter().position(|l| *l == label).unwrap_or(0),
            MessageDialogResult::Ok | MessageDialogResult::Yes => 0,
            MessageDialogResult::No => 1,
            MessageDialogResult::Cancel => labels.len() - 1,
        };
        Ok(json!({"response": response}))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{standard_colors, PowerSnapshot};
    use serde_json::{json, Value};
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    pub fn clipboard_read_text() -> Result<String, String> {
        let out = Command::new("pbpaste").output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub fn clipboard_write_text(text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("pbcopy failed".into());
        }
        Ok(())
    }

    static POWER_CHILD: OnceLock<Mutex<Option<std::process::Child>>> = OnceLock::new();
    fn power_child() -> &'static Mutex<Option<std::process::Child>> {
        POWER_CHILD.get_or_init(|| Mutex::new(None))
    }

    pub fn set_power_blockers(types: &[String]) -> Result<(), String> {
        for kind in types {
            match kind.as_str() {
                "prevent-app-suspension" | "prevent-display-sleep" => {}
                other => return Err(format!("Unknown power save blocker: {other}")),
            }
        }
        if let Some(mut child) = power_child().lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if types.is_empty() {
            return Ok(());
        }
        let mut args: Vec<&str> = Vec::new();
        let mut idle = false;
        let mut display = false;
        for kind in types {
            match kind.as_str() {
                "prevent-app-suspension" => idle = true,
                "prevent-display-sleep" => {
                    idle = true;
                    display = true;
                }
                _ => {}
            }
        }
        if display {
            args.push("-d");
        }
        if idle {
            args.push("-i");
        }
        let child = Command::new("caffeinate").args(&args).spawn().map_err(|e| e.to_string())?;
        *power_child().lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        Ok(())
    }

    fn command_output(command: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(command).args(args).output().ok()?;
        if !output.status.success() { return None; }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn defaults(key: &str) -> Option<String> { command_output("defaults", &["read", "-g", key]) }
    fn defaults_domain(domain: &str, key: &str) -> Option<String> { command_output("defaults", &["read", domain, key]) }
    fn truthy(value: Option<String>) -> bool {
        matches!(value.as_deref(), Some("1") | Some("true") | Some("YES") | Some("yes"))
    }

    fn cpu_speed_limit() -> Option<u32> {
        let text = command_output("pmset", &["-g", "therm"])?;
        text.lines()
            .find(|line| line.contains("CPU_Speed_Limit"))
            .and_then(|line| line.split('=').nth(1))
            .and_then(|value| value.trim().parse::<u32>().ok())
    }

    fn read_power() -> PowerSnapshot {
        let battery = command_output("pmset", &["-g", "batt"]);
        let on_battery = battery.as_deref().and_then(|text| {
            if text.contains("Battery Power") { Some(true) }
            else if text.contains("AC Power") { Some(false) }
            else { None }
        });
        let thermal_state = match cpu_speed_limit() {
            Some(limit) if limit < 50 => "critical",
            Some(limit) if limit < 80 => "serious",
            Some(limit) if limit < 100 => "fair",
            Some(_) => "nominal",
            None => "unknown",
        }.to_string();
        PowerSnapshot { on_battery, idle_time: None, thermal_state, locked: None }
    }

    pub fn power_snapshot() -> PowerSnapshot {
        static CACHE: OnceLock<Mutex<(Instant, PowerSnapshot)>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new((Instant::now() - Duration::from_secs(60), PowerSnapshot::default())));
        let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        if guard.0.elapsed() < Duration::from_secs(2) { return guard.1.clone(); }
        let snapshot = read_power();
        *guard = (Instant::now(), snapshot.clone());
        snapshot
    }

    fn accent_color(dark: bool) -> String {
        let index = defaults("AppleAccentColor").and_then(|value| value.parse::<i32>().ok());
        let color = match index {
            Some(-1) => "8e8e93",
            Some(0) => "ff5257",
            Some(1) => "ff9f0a",
            Some(2) => "ffcc00",
            Some(3) => "28cd41",
            Some(4) => "007aff",
            Some(5) => "af52de",
            Some(6) => "ff2d55",
            _ => if dark { "0a84ff" } else { "007aff" },
        };
        color.to_string()
    }

    pub fn system_preferences() -> Value {
        let dark = matches!(defaults("AppleInterfaceStyle").as_deref(), Some("Dark"));
        json!({
            "darkMode": dark,
            "accentColor": accent_color(dark),
            "colors": standard_colors(dark),
            "reduceMotion": truthy(defaults_domain("com.apple.universalaccess", "reduceMotion")),
            "invertedColors": truthy(defaults_domain("com.apple.universalaccess", "invertColors")),
            "highContrast": truthy(defaults_domain("com.apple.universalaccess", "increaseContrast")),
            "reduceTransparency": truthy(defaults_domain("com.apple.universalaccess", "reduceTransparency")),
            "platform": "macos",
        })
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{standard_colors, PowerSnapshot};
    use serde_json::{json, Value};
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    fn try_read(cmd: &str, args: &[&str]) -> Result<String, String> {
        let out = Command::new(cmd).args(args).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn try_write(cmd: &str, args: &[&str], text: &str) -> Result<(), String> {
        use std::io::Write;
        let mut child = Command::new(cmd).args(args).stdin(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("{cmd} failed"));
        }
        Ok(())
    }

    pub fn clipboard_read_text() -> Result<String, String> {
        if let Ok(s) = try_read("wl-paste", &["--no-newline"]) {
            return Ok(s);
        }
        if let Ok(s) = try_read("xclip", &["-selection", "clipboard", "-o"]) {
            return Ok(s);
        }
        if let Ok(s) = try_read("xsel", &["--clipboard", "--output"]) {
            return Ok(s);
        }
        Err("No clipboard utility found (install wl-clipboard, xclip, or xsel)".into())
    }

    pub fn clipboard_write_text(text: &str) -> Result<(), String> {
        if try_write("wl-copy", &[], text).is_ok() {
            return Ok(());
        }
        if try_write("xclip", &["-selection", "clipboard"], text).is_ok() {
            return Ok(());
        }
        if try_write("xsel", &["--clipboard", "--input"], text).is_ok() {
            return Ok(());
        }
        Err("No clipboard utility found (install wl-clipboard, xclip, or xsel)".into())
    }

    static POWER_CHILD: OnceLock<Mutex<Option<std::process::Child>>> = OnceLock::new();
    fn power_child() -> &'static Mutex<Option<std::process::Child>> {
        POWER_CHILD.get_or_init(|| Mutex::new(None))
    }

    pub fn set_power_blockers(types: &[String]) -> Result<(), String> {
        for kind in types {
            match kind.as_str() {
                "prevent-app-suspension" | "prevent-display-sleep" => {}
                other => return Err(format!("Unknown power save blocker: {other}")),
            }
        }
        if let Some(mut child) = power_child().lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if types.is_empty() {
            return Ok(());
        }
        let child = Command::new("systemd-inhibit")
            .args(["--what=sleep:idle", "--who=TSGPUI", "--why=power blocker", "tail", "-f", "/dev/null"])
            .spawn()
            .map_err(|e| e.to_string())?;
        *power_child().lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        Ok(())
    }

    fn command_output(command: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(command).args(args).output().ok()?;
        if !output.status.success() { return None; }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn upower_on_battery() -> Option<bool> {
        let text = command_output("upower", &["-d"])?;
        for line in text.lines() {
            if let Some(value) = line.trim().strip_prefix("on-battery:") {
                let value = value.trim();
                if value.eq_ignore_ascii_case("yes") { return Some(true); }
                if value.eq_ignore_ascii_case("no") { return Some(false); }
            }
        }
        None
    }

    fn sysfs_on_battery() -> Option<bool> {
        let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
        let mut saw_battery = false;
        let mut discharging = false;
        for entry in entries.flatten() {
            let Ok(kind) = std::fs::read_to_string(entry.path().join("type")) else { continue };
            let kind = kind.trim().to_ascii_lowercase();
            if kind == "mains" || kind == "ac" {
                if let Ok(online) = std::fs::read_to_string(entry.path().join("online")) {
                    return Some(online.trim() != "1");
                }
            } else if kind == "battery" {
                saw_battery = true;
                if let Ok(status) = std::fs::read_to_string(entry.path().join("status")) {
                    if status.trim().eq_ignore_ascii_case("discharging") { discharging = true; }
                }
            }
        }
        saw_battery.then_some(discharging)
    }

    fn locked_hint() -> Option<bool> {
        let text = command_output("loginctl", &["show-session", "self", "-p", "LockedHint"])?;
        let value = text.split('=').nth(1)?.trim();
        Some(value.eq_ignore_ascii_case("yes"))
    }

    fn thermal_state() -> String {
        std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .map(|milli| {
                let celsius = milli / 1000;
                if celsius >= 90 { "critical" } else if celsius >= 80 { "serious" } else if celsius >= 65 { "fair" } else { "nominal" }
            })
            .unwrap_or("unknown")
            .to_string()
    }

    fn read_power() -> PowerSnapshot {
        PowerSnapshot {
            on_battery: upower_on_battery().or_else(sysfs_on_battery),
            idle_time: None,
            thermal_state: thermal_state(),
            locked: locked_hint(),
        }
    }

    pub fn power_snapshot() -> PowerSnapshot {
        static CACHE: OnceLock<Mutex<(Instant, PowerSnapshot)>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new((Instant::now() - Duration::from_secs(60), PowerSnapshot::default())));
        let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        if guard.0.elapsed() < Duration::from_secs(2) { return guard.1.clone(); }
        let snapshot = read_power();
        *guard = (Instant::now(), snapshot.clone());
        snapshot
    }

    fn gsettings(schema: &str, key: &str) -> Option<String> {
        let value = command_output("gsettings", &["get", schema, key])?;
        Some(value.trim().trim_matches('\'').trim_matches('"').to_string())
    }

    fn accent_color() -> String {
        let name = gsettings("org.gnome.desktop.interface", "accent-color").unwrap_or_default();
        match name.to_ascii_lowercase().as_str() {
            "blue" => "3584e4", "teal" => "2190a4", "green" => "3a944a", "yellow" => "e5a50a",
            "orange" => "ed5b00", "red" => "e01b24", "pink" => "d56199", "purple" => "9141ac",
            "slate" => "6f8396", _ => "3584e4",
        }.to_string()
    }

    fn read_system_preferences() -> Value {
        let color_scheme = gsettings("org.gnome.desktop.interface", "color-scheme").unwrap_or_default().to_ascii_lowercase();
        let gtk_theme = gsettings("org.gnome.desktop.interface", "gtk-theme").unwrap_or_default().to_ascii_lowercase();
        let dark = color_scheme.contains("dark") || gtk_theme.contains("dark");
        let animations = gsettings("org.gnome.desktop.interface", "enable-animations").unwrap_or_else(|| "true".into());
        let high_contrast = gsettings("org.gnome.desktop.a11y.interface", "high-contrast").unwrap_or_else(|| "false".into());
        json!({
            "darkMode": dark,
            "accentColor": accent_color(),
            "colors": standard_colors(dark),
            "reduceMotion": animations.eq_ignore_ascii_case("false"),
            "invertedColors": false,
            "highContrast": high_contrast.eq_ignore_ascii_case("true"),
            "reduceTransparency": false,
            "platform": "linux",
        })
    }

    pub fn system_preferences() -> Value {
        static CACHE: OnceLock<Mutex<(Instant, Value)>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new((Instant::now() - Duration::from_secs(60), Value::Null)));
        let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        if guard.0.elapsed() < Duration::from_secs(2) && !guard.1.is_null() { return guard.1.clone(); }
        let values = read_system_preferences();
        *guard = (Instant::now(), values.clone());
        values
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_length() {
        for len in 0..40u8 {
            let bytes: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        }
        assert!(base64_decode("not*base64").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dpapi_round_trips_and_hides_plaintext() {
        let cipher = safe_storage_call("safeStorage.encrypt", &[Value::from("token-123")]).unwrap();
        let cipher = cipher.as_str().unwrap().to_owned();
        assert!(!cipher.contains("token-123"));
        let plain = safe_storage_call("safeStorage.decrypt", &[Value::from(cipher)]).unwrap();
        assert_eq!(plain, Value::from("token-123"));
        assert_eq!(safe_storage_call("safeStorage.isAvailable", &[]).unwrap(), Value::Bool(true));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn dpapi_rejects_garbage_ciphertext() {
        let garbage = base64_encode(b"not dpapi data");
        assert!(safe_storage_call("safeStorage.decrypt", &[Value::from(garbage)]).is_err());
    }
}

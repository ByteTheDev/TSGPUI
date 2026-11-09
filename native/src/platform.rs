//! Platform operations shared by the native GPUI host and the WebView host: clipboard, native
//! dialogs, external links, power blockers and the allowlisted HTTPS transport. Each runs once here,
//! so both renderers answer the same requests the same way. Clipboard, dialogs and power blockers
//! are implemented on Windows; other systems reject them with a clear error.

use serde_json::{json, Value};

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

#[cfg(not(target_os = "windows"))]
pub fn clipboard_read_text() -> Result<String, String> {
    Err(WINDOWS_ONLY.into())
}

#[cfg(target_os = "windows")]
pub fn clipboard_write_text(text: &str) -> Result<(), String> {
    windows::clipboard::write_text(text)
}

#[cfg(not(target_os = "windows"))]
pub fn clipboard_write_text(_text: &str) -> Result<(), String> {
    Err(WINDOWS_ONLY.into())
}

/// Shows an open dialog. Result: `{ canceled, paths }`.
#[cfg(target_os = "windows")]
pub fn open_file_dialog(args: &Value) -> Result<Value, String> {
    Ok(windows::open_file_dialog(args))
}

#[cfg(not(target_os = "windows"))]
pub fn open_file_dialog(_args: &Value) -> Result<Value, String> {
    Err(WINDOWS_ONLY.into())
}

/// Shows a save dialog. Result: `{ canceled, path }`.
#[cfg(target_os = "windows")]
pub fn save_file_dialog(args: &Value) -> Result<Value, String> {
    Ok(windows::save_file_dialog(args))
}

#[cfg(not(target_os = "windows"))]
pub fn save_file_dialog(_args: &Value) -> Result<Value, String> {
    Err(WINDOWS_ONLY.into())
}

/// Shows a message box. Result: `{ response }`, the index of the chosen button.
#[cfg(target_os = "windows")]
pub fn message_dialog(args: &Value) -> Result<Value, String> {
    windows::message_dialog(args)
}

#[cfg(not(target_os = "windows"))]
pub fn message_dialog(_args: &Value) -> Result<Value, String> {
    Err(WINDOWS_ONLY.into())
}

/// Sets the power blockers. `types` holds `prevent-app-suspension` and/or `prevent-display-sleep`;
/// an empty list clears them.
#[cfg(target_os = "windows")]
pub fn set_power_blockers(types: &[String]) -> Result<(), String> {
    windows::power::set_blockers(types)
}

#[cfg(not(target_os = "windows"))]
pub fn set_power_blockers(_types: &[String]) -> Result<(), String> {
    Err("Power save blockers are supported on Windows only".into())
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
    use super::string_list;
    use serde_json::{json, Value};
    use std::sync::{mpsc, Mutex, OnceLock};

    fn apply_filters(mut dialog: rfd::FileDialog, filters: &Value) -> rfd::FileDialog {
        for filter in filters.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let name = filter["name"].as_str().unwrap_or("Files").to_owned();
            let extensions = string_list(&filter["extensions"]);
            dialog = dialog.add_filter(name, extensions.as_slice());
        }
        dialog
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

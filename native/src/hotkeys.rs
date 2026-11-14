//! Process-wide global hotkeys behind the `globalShortcut.*` host operations.
//!
//! Accelerators use Electron's syntax (`CommandOrControl+Shift+P`, `Alt+F4`, `MediaPlayPause`).
//! Registration is best effort and asynchronous: Electron returns `true` optimistically for an
//! accelerator whose syntax is valid, and a binding the OS refuses is reported as a
//! `globalShortcut.failed` event so the shim can drop it and warn.
//!
//! * Windows: `RegisterHotKey` on a dedicated message-loop thread.
//! * macOS: a listen-only `CGEventTap` on a dedicated run-loop thread. macOS requires the app to
//!   have Accessibility permission for this; without it the tap cannot be created.
//! * Linux: `XGrabKey` on the X11 root window, polled on a dedicated thread. Wayland sessions do
//!   not provide global key grabs, so registration fails there.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

/// One parsed accelerator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// Canonical key name: `A`..`Z`, `0`..`9`, `F1`..`F24`, or a lowercase name such as `enter`.
    pub key: String,
}

fn desired() -> &'static Mutex<HashMap<String, Hotkey>> {
    static DESIRED: OnceLock<Mutex<HashMap<String, Hotkey>>> = OnceLock::new();
    DESIRED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A copy of the accelerators the app asked for, for the platform layer to reconcile against.
pub fn desired_snapshot() -> HashMap<String, Hotkey> {
    desired().lock().unwrap_or_else(|error| error.into_inner()).clone()
}

fn failed(accelerator: &str, error: impl Into<String>) {
    crate::tray::push_event("globalShortcut.failed", json!({ "accelerator": accelerator, "error": error.into() }));
}

/// Registers `accelerator`, replacing an existing binding for the same accelerator.
pub fn register(accelerator: &str) -> Result<(), String> {
    let hotkey = parse(accelerator)?;
    desired().lock().unwrap_or_else(|error| error.into_inner()).insert(accelerator.to_owned(), hotkey);
    platform::sync();
    Ok(())
}

pub fn unregister(accelerator: &str) {
    desired().lock().unwrap_or_else(|error| error.into_inner()).remove(accelerator);
    platform::sync();
}

pub fn unregister_all() {
    desired().lock().unwrap_or_else(|error| error.into_inner()).clear();
    platform::sync();
}

pub fn is_registered(accelerator: &str) -> bool {
    desired().lock().unwrap_or_else(|error| error.into_inner()).contains_key(accelerator)
}

/// Runs one `globalShortcut.*` operation. `args` is either the accelerator string or the bridge
/// object `{ accelerator }`.
pub fn call(op: &str, args: &Value) -> Result<Value, String> {
    match op {
        "globalShortcut.register" => {
            register(&accelerator_arg(args)?)?;
            Ok(Value::Bool(true))
        }
        "globalShortcut.unregister" => {
            unregister(&accelerator_arg(args)?);
            Ok(Value::Null)
        }
        "globalShortcut.unregisterAll" => {
            unregister_all();
            Ok(Value::Null)
        }
        "globalShortcut.isRegistered" => Ok(Value::Bool(is_registered(&accelerator_arg(args)?))),
        other => Err(format!("Native operation '{other}' is not available in this build")),
    }
}

fn accelerator_arg(args: &Value) -> Result<String, String> {
    if let Some(accelerator) = args.as_str() {
        return Ok(accelerator.to_owned());
    }
    args.get("accelerator").and_then(Value::as_str).map(str::to_owned).ok_or_else(|| "globalShortcut needs an accelerator string".to_string())
}

/// Parses an Electron accelerator. Modifiers may appear in any order and are
/// case-insensitive; exactly one non-modifier key is required.
pub fn parse(accelerator: &str) -> Result<Hotkey, String> {
    if accelerator.trim().is_empty() {
        return Err("Accelerator must not be empty".into());
    }
    let mut hotkey = Hotkey { ctrl: false, alt: false, shift: false, meta: false, key: String::new() };
    // A trailing "++" means the key is the plus key itself ("Ctrl++").
    let plus_key = accelerator.ends_with("++") || accelerator == "Plus" || accelerator.eq_ignore_ascii_case("plus");
    let body = if accelerator.ends_with("++") {
        &accelerator[..accelerator.len() - 2]
    } else if accelerator.eq_ignore_ascii_case("plus") {
        ""
    } else {
        accelerator
    };
    let mut parts: Vec<&str> = body.split('+').map(str::trim).filter(|part| !part.is_empty()).collect();
    if plus_key {
        parts.push("plus");
    }
    if parts.is_empty() {
        return Err(format!("Accelerator '{accelerator}' has no key"));
    }
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "command" | "cmd" | "meta" | "super" => hotkey.meta = true,
            "commandorcontrol" | "cmdorctrl" | "cmdorcontrol" => {
                if cfg!(target_os = "macos") {
                    hotkey.meta = true;
                } else {
                    hotkey.ctrl = true;
                }
            }
            "control" | "ctrl" => hotkey.ctrl = true,
            "alt" | "option" | "opt" => hotkey.alt = true,
            "shift" => hotkey.shift = true,
            _ => {
                if !hotkey.key.is_empty() {
                    return Err(format!("Accelerator '{accelerator}' has more than one key"));
                }
                hotkey.key = normalized_key(part)?;
            }
        }
    }
    if hotkey.key.is_empty() {
        return Err(format!("Accelerator '{accelerator}' has no key"));
    }
    Ok(hotkey)
}

fn normalized_key(key: &str) -> Result<String, String> {
    let lower = key.to_ascii_lowercase();
    if lower.len() == 1 {
        let character = lower.chars().next().unwrap_or(' ');
        if character.is_ascii_alphanumeric() {
            return Ok(lower.to_ascii_uppercase());
        }
        return Ok(lower);
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|rest| rest.parse::<u8>().ok()) {
        if (1..=24).contains(&number) {
            return Ok(format!("F{number}"));
        }
    }
    if let Some(number) = lower.strip_prefix("num").and_then(|rest| rest.parse::<u8>().ok()) {
        if number <= 9 {
            return Ok(format!("num{number}"));
        }
    }
    let named = match lower.as_str() {
        "space" | "spacebar" => "space",
        "tab" => "tab",
        "enter" | "return" => "enter",
        "escape" | "esc" => "escape",
        "backspace" => "backspace",
        "delete" | "del" => "delete",
        "insert" | "ins" => "insert",
        "home" => "home",
        "end" => "end",
        "pageup" | "pgup" => "pageup",
        "pagedown" | "pgdn" => "pagedown",
        "up" | "arrowup" => "up",
        "down" | "arrowdown" => "down",
        "left" | "arrowleft" => "left",
        "right" | "arrowright" => "right",
        "plus" => "plus",
        "minus" => "minus",
        "printscreen" | "print" => "printscreen",
        "medianexttrack" | "medianext" => "medianext",
        "mediaprevioustrack" | "mediaprevious" => "mediaprevious",
        "mediastop" => "mediastop",
        "mediaplaypause" | "mediaplay" => "mediaplaypause",
        "volumemute" | "mute" => "volumemute",
        "volumedown" => "volumedown",
        "volumeup" => "volumeup",
        "numlock" => "numlock",
        "capslock" => "capslock",
        "scrolllock" => "scrolllock",
        "pause" => "pause",
        _ => return Err(format!("Unsupported accelerator key: {key}")),
    };
    Ok(named.to_owned())
}

#[cfg(target_os = "windows")]
fn windows_virtual_key(key: &str) -> Option<u32> {
    let mut characters = key.chars();
    if key.len() == 1 {
        let character = characters.next()?.to_ascii_uppercase();
        if character.is_ascii_alphanumeric() {
            return Some(character as u32);
        }
        return match character {
            '+' | '=' => Some(0xbb),
            '-' => Some(0xbd),
            ',' => Some(0xbc),
            '.' => Some(0xbe),
            '/' => Some(0xbf),
            ';' => Some(0xba),
            '[' => Some(0xdb),
            '\\' => Some(0xdc),
            ']' => Some(0xdd),
            '\'' => Some(0xde),
            '`' => Some(0xc0),
            _ => None,
        };
    }
    if key == "plus" {
        return Some(0xbb);
    }
    if key == "minus" {
        return Some(0xbd);
    }
    if let Some(number) = key.strip_prefix('F').and_then(|rest| rest.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x6f + number);
        }
    }
    if let Some(number) = key.strip_prefix("num").and_then(|rest| rest.parse::<u32>().ok()) {
        return Some(0x60 + number);
    }
    Some(match key {
        "space" => 0x20,
        "tab" => 0x09,
        "enter" => 0x0d,
        "escape" => 0x1b,
        "backspace" => 0x08,
        "delete" => 0x2e,
        "insert" => 0x2d,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "printscreen" => 0x2c,
        "pause" => 0x13,
        "numlock" => 0x90,
        "capslock" => 0x14,
        "scrolllock" => 0x91,
        "medianext" => 0xb0,
        "mediaprevious" => 0xb1,
        "mediastop" => 0xb2,
        "mediaplaypause" => 0xb3,
        "volumemute" => 0xad,
        "volumedown" => 0xae,
        "volumeup" => 0xaf,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn macos_keycode(key: &str) -> Option<u16> {
    Some(match key {
        "A" => 0, "S" => 1, "D" => 2, "F" => 3, "H" => 4, "G" => 5, "Z" => 6, "X" => 7,
        "C" => 8, "V" => 9, "B" => 11, "Q" => 12, "W" => 13, "E" => 14, "R" => 15,
        "Y" => 16, "T" => 17, "1" => 18, "2" => 19, "3" => 20, "4" => 21, "6" => 22,
        "5" => 23, "=" => 24, "9" => 25, "7" => 26, "-" => 27, "8" => 28, "0" => 29,
        "]" => 30, "O" => 31, "U" => 32, "[" => 33, "I" => 34, "P" => 35, "enter" => 36,
        "L" => 37, "J" => 38, "'" => 39, "K" => 40, ";" => 41, "\\" => 42, "," => 43,
        "/" => 44, "N" => 45, "M" => 46, "." => 47, "tab" => 48, "space" => 49, "`" => 50,
        "backspace" => 51, "escape" => 53, "right" => 124, "left" => 123, "down" => 125,
        "up" => 126, "home" => 115, "end" => 119, "pageup" => 116, "pagedown" => 121,
        "delete" => 117, "F1" => 122, "F2" => 120, "F3" => 99, "F4" => 118, "F5" => 96,
        "F6" => 97, "F7" => 98, "F8" => 100, "F9" => 101, "F10" => 109, "F11" => 103,
        "F12" => 111, "F13" => 105, "F14" => 107, "F15" => 113, "F16" => 106, "F17" => 64,
        "F18" => 79, "F19" => 80, "F20" => 90, "num0" => 82, "num1" => 83, "num2" => 84,
        "num3" => 85, "num4" => 86, "num5" => 87, "num6" => 88, "num7" => 89, "num8" => 91,
        "num9" => 92, "medianext" => 19, "mediaprevious" => 18, "mediaplaypause" => 16,
        "volumemute" => 74, "volumedown" => 73, "volumeup" => 72, "plus" => 69, "minus" => 78,
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
fn x11_keysym(key: &str) -> Option<u32> {
    if key.len() == 1 {
        let character = key.chars().next()?;
        if character.is_ascii_alphanumeric() {
            return Some(character.to_ascii_lowercase() as u32);
        }
        return Some(character as u32);
    }
    if let Some(number) = key.strip_prefix('F').and_then(|rest| rest.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0xffbe + number - 1);
        }
    }
    if let Some(number) = key.strip_prefix("num").and_then(|rest| rest.parse::<u32>().ok()) {
        return Some(0xffb0 + number);
    }
    Some(match key {
        "space" => 0x20,
        "tab" => 0xff09,
        "enter" => 0xff0d,
        "escape" => 0xff1b,
        "backspace" => 0xff08,
        "delete" => 0xffff,
        "insert" => 0xff63,
        "home" => 0xff50,
        "end" => 0xff57,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        "up" => 0xff52,
        "down" => 0xff54,
        "left" => 0xff51,
        "right" => 0xff53,
        "printscreen" => 0xff61,
        "pause" => 0xff13,
        "numlock" => 0xff7f,
        "capslock" => 0xffe5,
        "scrolllock" => 0xff14,
        "plus" => 0x2b,
        "minus" => 0x2d,
        "medianext" => 0x1008ff17,
        "mediaprevious" => 0x1008ff16,
        "mediastop" => 0x1008ff15,
        "mediaplaypause" => 0x1008ff14,
        "volumemute" => 0x1008ff12,
        "volumedown" => 0x1008ff11,
        "volumeup" => 0x1008ff13,
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{
        collections::HashMap,
        ptr::null_mut,
        sync::{
            mpsc::{self, Receiver, Sender},
            Mutex,
        },
        time::Duration,
    };

    use windows_sys::Win32::{
        System::Threading::GetCurrentThreadId,
        UI::{
            Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN},
            WindowsAndMessaging::{GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_HOTKEY},
        },
    };

    use super::{desired_snapshot, failed, windows_virtual_key, Hotkey};

    const WAKE_MESSAGE: u32 = 0x8003; // WM_APP + 3

    enum Cmd {
        Sync,
    }

    struct Service {
        tx: Sender<Cmd>,
        thread_id: u32,
    }

    static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

    fn service() -> Result<(Sender<Cmd>, u32), String> {
        let mut lock = SERVICE.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(service) = lock.as_ref() {
            return Ok((service.tx.clone(), service.thread_id));
        }
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("rendrix-hotkeys".into())
            .spawn(move || run(cmd_rx, ready_tx))
            .map_err(|error| format!("Unable to start the hotkey thread: {error}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(thread_id)) => {
                *lock = Some(Service { tx: cmd_tx.clone(), thread_id });
                Ok((cmd_tx, thread_id))
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err("The hotkey thread did not start".into()),
        }
    }

    pub fn sync() {
        match service() {
            Ok((tx, thread_id)) => {
                let _ = tx.send(Cmd::Sync);
                unsafe { PostThreadMessageW(thread_id, WAKE_MESSAGE, 0, 0) };
            }
            Err(error) => {
                for accelerator in desired_snapshot().keys() {
                    failed(accelerator, error.clone());
                }
            }
        }
    }

    fn run(rx: Receiver<Cmd>, ready: Sender<Result<u32, String>>) {
        let thread_id = unsafe { GetCurrentThreadId() };
        // Create the thread message queue before publishing the thread id. Otherwise a wake-up
        // posted by the first registration can be lost, and that accelerator would not bind until
        // the next mutation.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE) };
        let _ = ready.send(Ok(thread_id));
        let mut registered: HashMap<String, (i32, Hotkey)> = HashMap::new();
        let mut next_id: i32 = 1;
        loop {
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
            if result <= 0 {
                break;
            }
            if message.message == WAKE_MESSAGE {
                while rx.try_recv().is_ok() {}
                reconcile(&mut registered, &mut next_id);
                continue;
            }
            if message.message == WM_HOTKEY {
                let id = message.wParam as i32;
                let accelerator = registered.iter().find(|(_, (registered_id, _))| *registered_id == id).map(|(accelerator, _)| accelerator.clone());
                if let Some(accelerator) = accelerator {
                    crate::tray::push_event("globalShortcut.event", serde_json::json!({ "accelerator": accelerator }));
                }
            }
        }
        for (_, (id, _)) in registered {
            unsafe { UnregisterHotKey(null_mut(), id) };
        }
    }

    fn reconcile(registered: &mut HashMap<String, (i32, Hotkey)>, next_id: &mut i32) {
        let desired = desired_snapshot();
        let stale: Vec<String> = registered.keys().filter(|accelerator| !desired.contains_key(*accelerator)).cloned().collect();
        for accelerator in stale {
            if let Some((id, _)) = registered.remove(&accelerator) {
                unsafe { UnregisterHotKey(null_mut(), id) };
            }
        }
        for (accelerator, hotkey) in desired {
            if registered.contains_key(&accelerator) {
                continue;
            }
            let Some(virtual_key) = windows_virtual_key(&hotkey.key) else {
                failed(&accelerator, format!("Unsupported key: {}", hotkey.key));
                continue;
            };
            let mut modifiers = MOD_NOREPEAT;
            if hotkey.ctrl {
                modifiers |= MOD_CONTROL;
            }
            if hotkey.shift {
                modifiers |= MOD_SHIFT;
            }
            if hotkey.alt {
                modifiers |= MOD_ALT;
            }
            if hotkey.meta {
                modifiers |= MOD_WIN;
            }
            let id = *next_id;
            *next_id += 1;
            if unsafe { RegisterHotKey(null_mut(), id, modifiers, virtual_key) } == 0 {
                failed(&accelerator, "The system refused the hotkey; another application may already own it");
                continue;
            }
            registered.insert(accelerator, (id, hotkey));
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        ffi::c_void,
        sync::{
            atomic::{AtomicPtr, Ordering},
            mpsc, Mutex, OnceLock,
        },
        time::Duration,
    };

    use super::{desired_snapshot, failed, macos_keycode, Hotkey};

    type CFMachPortRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CGEventTapCallBack = unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: CFStringRef;
    }

    const KEY_DOWN: u32 = 10;
    const KEYCODE_FIELD: u32 = 9;
    const FLAG_SHIFT: u64 = 1 << 17;
    const FLAG_CONTROL: u64 = 1 << 18;
    const FLAG_ALT: u64 = 1 << 19;
    const FLAG_COMMAND: u64 = 1 << 20;
    const MODIFIER_MASK: u64 = FLAG_SHIFT | FLAG_CONTROL | FLAG_ALT | FLAG_COMMAND;

    struct Binding {
        accelerator: String,
        keycode: u16,
        flags: u64,
    }

    fn bindings() -> &'static Mutex<Vec<Binding>> {
        static BINDINGS: OnceLock<Mutex<Vec<Binding>>> = OnceLock::new();
        BINDINGS.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn started() -> &'static Mutex<Option<Result<(), String>>> {
        static STARTED: OnceLock<Mutex<Option<Result<(), String>>>> = OnceLock::new();
        STARTED.get_or_init(|| Mutex::new(None))
    }

    static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    pub fn sync() {
        let desired = desired_snapshot();
        let mut list = Vec::new();
        for (accelerator, hotkey) in &desired {
            let Some(keycode) = macos_keycode(&hotkey.key) else {
                failed(accelerator, format!("Unsupported key: {}", hotkey.key));
                continue;
            };
            list.push(Binding { accelerator: accelerator.clone(), keycode, flags: event_flags(hotkey) });
        }
        *bindings().lock().unwrap_or_else(|error| error.into_inner()) = list;

        let mut state = started().lock().unwrap_or_else(|error| error.into_inner());
        if state.is_none() {
            *state = Some(start_tap());
        }
        if let Some(Err(error)) = state.as_ref() {
            let error = error.clone();
            drop(state);
            for accelerator in desired.keys() {
                failed(accelerator, error.clone());
            }
        }
    }

    fn event_flags(hotkey: &Hotkey) -> u64 {
        let mut flags = 0;
        if hotkey.shift {
            flags |= FLAG_SHIFT;
        }
        if hotkey.ctrl {
            flags |= FLAG_CONTROL;
        }
        if hotkey.alt {
            flags |= FLAG_ALT;
        }
        if hotkey.meta {
            flags |= FLAG_COMMAND;
        }
        flags
    }

    fn start_tap() -> Result<(), String> {
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("rendrix-hotkeys".into())
            .spawn(move || unsafe {
                let tap = CGEventTapCreate(1, 0, 1, 1u64 << KEY_DOWN, tap_callback, std::ptr::null_mut());
                if tap.is_null() {
                    let _ = ready_tx.send(Err("The event tap could not be created; grant Accessibility permission to the app".to_string()));
                    return;
                }
                TAP.store(tap, Ordering::SeqCst);
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    let _ = ready_tx.send(Err("The event tap has no run-loop source".to_string()));
                    return;
                }
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                let _ = ready_tx.send(Ok(()));
                CFRunLoopRun();
            })
            .map_err(|error| format!("Unable to start the hotkey thread: {error}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => Err("The event tap did not start".to_string()),
        }
    }

    unsafe extern "C" fn tap_callback(_proxy: CGEventTapProxy, kind: u32, event: CGEventRef, _user: *mut c_void) -> CGEventRef {
        // The system disables a tap that takes too long. Re-enable it instead of losing all
        // shortcuts for the rest of the session.
        if kind == 0xffff_fffe || kind == 0xffff_ffff {
            let tap = TAP.load(Ordering::SeqCst);
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }
        if event.is_null() {
            return event;
        }
        let keycode = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) } as u16;
        let flags = unsafe { CGEventGetFlags(event) } & MODIFIER_MASK;
        let binding = bindings()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|binding| binding.keycode == keycode && binding.flags == flags)
            .map(|binding| binding.accelerator.clone());
        if let Some(accelerator) = binding {
            crate::tray::push_event("globalShortcut.event", serde_json::json!({ "accelerator": accelerator }));
        }
        event
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{
        collections::HashMap,
        sync::{mpsc, Mutex},
        thread,
        time::Duration,
    };

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
    use x11rb::protocol::Event;

    use super::{desired_snapshot, failed, x11_keysym, Hotkey};

    enum Cmd {
        Sync,
    }

    struct Service {
        tx: mpsc::Sender<Cmd>,
    }

    static SERVICE: Mutex<Option<Service>> = Mutex::new(None);

    pub fn sync() {
        let mut lock = SERVICE.lock().unwrap_or_else(|error| error.into_inner());
        if lock.is_none() {
            let (cmd_tx, cmd_rx) = mpsc::channel();
            match std::thread::Builder::new().name("rendrix-hotkeys".into()).spawn(move || run(cmd_rx)) {
                Ok(_) => *lock = Some(Service { tx: cmd_tx }),
                Err(error) => {
                    drop(lock);
                    for accelerator in desired_snapshot().keys() {
                        failed(accelerator, format!("Unable to start the hotkey thread: {error}"));
                    }
                    return;
                }
            }
        }
        if let Some(service) = lock.as_ref() {
            let _ = service.tx.send(Cmd::Sync);
        }
    }

    struct Grab {
        keycode: u8,
        modifiers: u16,
    }

    fn run(rx: mpsc::Receiver<Cmd>) {
        let connect = x11rb::connect(None);
        let (connection, screen) = match connect {
            Ok((connection, screen)) => (connection, screen),
            Err(error) => {
                let message = format!("X11 is not available, so global shortcuts cannot be registered: {error}");
                for accelerator in desired_snapshot().keys() {
                    failed(accelerator, message.clone());
                }
                return;
            }
        };
        let root = connection.setup().roots[screen].root;
        let min_keycode = connection.setup().min_keycode;
        let max_keycode = connection.setup().max_keycode;
        let mut keymap: HashMap<u32, Vec<u8>> = HashMap::new();
        rebuild_keymap(&connection, min_keycode, max_keycode, &mut keymap);
        let mut grabs: Vec<Grab> = Vec::new();
        let mut signatures: HashMap<String, Vec<u32>> = HashMap::new();

        loop {
            while rx.try_recv().is_ok() {}
            let desired = desired_snapshot();
            let signature: HashMap<String, Vec<u32>> = desired
                .iter()
                .map(|(accelerator, hotkey)| (accelerator.clone(), vec![x11_keysym(&hotkey.key).unwrap_or(0), modifiers_of(hotkey) as u32]))
                .collect();
            if signature != signatures {
                ungrab_all(&connection, root, &grabs);
                grabs.clear();
                signatures = signature;
                for (accelerator, hotkey) in &desired {
                    let Some(keysym) = x11_keysym(&hotkey.key) else {
                        failed(accelerator, format!("Unsupported key: {}", hotkey.key));
                        continue;
                    };
                    let Some(keycodes) = keymap.get(&keysym).cloned() else {
                        failed(accelerator, format!("The current keyboard layout has no key for {}", hotkey.key));
                        continue;
                    };
                    let modifiers = modifiers_of(hotkey);
                    let mut registered = false;
                    for keycode in keycodes {
                        if grab_key(&connection, root, keycode, modifiers) {
                            grabs.push(Grab { keycode, modifiers });
                            registered = true;
                        }
                    }
                    if !registered {
                        failed(accelerator, "The system refused the hotkey; another application may already own it");
                    }
                }
                let _ = connection.flush();
            }
            while let Ok(Some(event)) = connection.poll_for_event() {
                match event {
                    Event::KeyPress(event) => {
                        let state = u16::from(event.state) & !(u16::from(ModMask::LOCK) | u16::from(ModMask::M2));
                        if grabs.iter().any(|grab| grab.keycode == event.detail && grab.modifiers == state) {
                            if let Some(accelerator) = desired.iter().find(|(_, hotkey)| {
                                x11_keysym(&hotkey.key).and_then(|keysym| keymap.get(&keysym)).is_some_and(|codes| codes.contains(&event.detail))
                                    && modifiers_of(hotkey) == state
                            }).map(|(accelerator, _)| accelerator.clone()) {
                                crate::tray::push_event("globalShortcut.event", serde_json::json!({ "accelerator": accelerator }));
                            }
                        }
                    }
                    Event::MappingNotify(_) => {
                        keymap.clear();
                        rebuild_keymap(&connection, min_keycode, max_keycode, &mut keymap);
                        ungrab_all(&connection, root, &grabs);
                        grabs.clear();
                        signatures.clear();
                    }
                    _ => {}
                }
            }
            let _ = connection.flush();
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn modifiers_of(hotkey: &Hotkey) -> u16 {
        let mut modifiers = ModMask::from(0u16);
        if hotkey.shift {
            modifiers |= ModMask::SHIFT;
        }
        if hotkey.ctrl {
            modifiers |= ModMask::CONTROL;
        }
        if hotkey.alt {
            modifiers |= ModMask::M1;
        }
        if hotkey.meta {
            modifiers |= ModMask::M4;
        }
        u16::from(modifiers)
    }

    fn rebuild_keymap(connection: &impl Connection, min_keycode: u8, max_keycode: u8, keymap: &mut HashMap<u32, Vec<u8>>) {
        let count = (max_keycode - min_keycode).saturating_add(1);
        let Ok(cookie) = connection.get_keyboard_mapping(min_keycode, count) else { return };
        let Ok(reply) = cookie.reply() else { return };
        let per_keycode = usize::from(reply.keysyms_per_keycode);
        if per_keycode == 0 {
            return;
        }
        for (index, keysym) in reply.keysyms.iter().enumerate() {
            let keycode = min_keycode as usize + index / per_keycode;
            if keycode > max_keycode as usize {
                break;
            }
            keymap.entry(*keysym).or_default().push(keycode as u8);
        }
    }

    fn grab_key(connection: &impl Connection, root: u32, keycode: u8, modifiers: u16) -> bool {
        let mut success = false;
        for extra in [0u16, u16::from(ModMask::LOCK), u16::from(ModMask::M2), u16::from(ModMask::LOCK) | u16::from(ModMask::M2)] {
            let Ok(cookie) = connection.grab_key(true, root, ModMask::from(modifiers | extra), keycode, GrabMode::ASYNC, GrabMode::ASYNC) else {
                continue;
            };
            success |= cookie.check().is_ok();
        }
        success
    }

    fn ungrab_all(connection: &impl Connection, root: u32, grabs: &[Grab]) {
        for grab in grabs {
            for extra in [0u16, u16::from(ModMask::LOCK), u16::from(ModMask::M2), u16::from(ModMask::LOCK) | u16::from(ModMask::M2)] {
                let _ = connection.ungrab_key(grab.keycode, root, ModMask::from(grab.modifiers | extra));
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn sync() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_keys() {
        let hotkey = parse("CommandOrControl+Shift+P").unwrap();
        assert_eq!(hotkey.key, "P");
        assert!(hotkey.shift);
        if cfg!(target_os = "macos") {
            assert!(hotkey.meta && !hotkey.ctrl);
        } else {
            assert!(hotkey.ctrl && !hotkey.meta);
        }
        let function = parse("alt+f4").unwrap();
        assert_eq!(function.key, "F4");
        assert!(function.alt && !function.shift);
        let plus = parse("Ctrl++").unwrap();
        assert_eq!(plus.key, "plus");
        assert!(plus.ctrl);
        assert_eq!(parse("Space").unwrap().key, "space");
        assert_eq!(parse("MediaPlayPause").unwrap().key, "mediaplaypause");
    }

    #[test]
    fn rejects_unknown_or_malformed_accelerators() {
        assert!(parse("").is_err());
        assert!(parse("Ctrl").is_err());
        assert!(parse("Ctrl+Shift").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("Ctrl+DefinitelyNotAKey").is_err());
        assert!(parse("F25").is_err());
    }

    #[test]
    fn platform_key_maps_cover_common_bindings() {
        #[cfg(target_os = "windows")]
        {
            assert_eq!(windows_virtual_key("A"), Some(0x41));
            assert_eq!(windows_virtual_key("F4"), Some(0x73));
            assert!(windows_virtual_key("not-a-key").is_none());
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(x11_keysym("a"), Some(0x61));
            assert_eq!(x11_keysym("F4"), Some(0xffc1));
            assert!(x11_keysym("not-a-key").is_none());
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(macos_keycode("A"), Some(0));
            assert_eq!(macos_keycode("F4"), Some(118));
            assert!(macos_keycode("not-a-key").is_none());
        }
    }
}

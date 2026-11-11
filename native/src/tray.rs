//! OS tray icons behind the `tray.*` host operations.
//!
//! The JS shim owns the Electron-facing object and forwards every mutation here. The platform
//! layer keeps a small amount of state (the icon image handle, tooltip, title and menu model) and
//! reconciles the OS object from it, so operations that arrive out of order from the host worker
//! threads still produce the same final tray:
//!
//! * Windows: `Shell_NotifyIcon` on a message-only window, with a `HMENU` built for right clicks.
//! * macOS: `NSStatusItem` on the main thread, with an `NSMenu` whose items call back through a
//!   small Objective-C target class.
//! * Linux: a `StatusNotifierItem` over D-Bus (`ksni`), including its `com.canonical.dbusmenu`
//!   menu.
//!
//! Events are queued for the host to deliver to the compatibility shim through
//! `__tsgpui_host_event("tray.event", ...)`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};

static EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

type Notifier = Arc<dyn Fn() + Send + Sync>;

static NOTIFIER: Mutex<Option<Notifier>> = Mutex::new(None);

/// Installs a callback the platform layer runs after queueing an event, so a host that waits on an
/// event loop can wake immediately instead of waiting for its next poll tick. Only one host runs per
/// process, so the last installer wins.
pub fn set_event_notifier(notifier: impl Fn() + Send + Sync + 'static) {
    *NOTIFIER.lock().unwrap_or_else(|error| error.into_inner()) = Some(Arc::new(notifier));
}

/// Queues a host event. The runtime and both hosts poll this queue.
pub fn push_event(channel: &str, payload: Value) {
    let mut events = EVENTS.lock().unwrap_or_else(|error| error.into_inner());
    events.push(json!({ "channel": channel, "payload": payload }));
    drop(events);
    let notifier = NOTIFIER.lock().unwrap_or_else(|error| error.into_inner()).clone();
    if let Some(notifier) = notifier {
        notifier();
    }
}

/// Takes the events queued since the last call.
pub fn poll_events() -> Vec<Value> {
    let mut events = EVENTS.lock().unwrap_or_else(|error| error.into_inner());
    std::mem::take(&mut *events)
}

/// Everything the platform layer needs to rebuild one tray icon.
#[derive(Clone, Default)]
pub struct TrayState {
    /// Set by `tray.create`. Later mutations apply to the model before that, so out-of-order host
    /// calls still land in the right order.
    pub created: bool,
    pub icon: Option<usize>,
    pub pressed: Option<usize>,
    pub tooltip: String,
    pub title: String,
    pub menu: Value,
}

fn trays() -> &'static Mutex<HashMap<u64, TrayState>> {
    static TRAYS: OnceLock<Mutex<HashMap<u64, TrayState>>> = OnceLock::new();
    TRAYS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A copy of one tray's model, for the platform layer.
pub fn state_of(id: u64) -> Option<TrayState> {
    trays().lock().unwrap_or_else(|error| error.into_inner()).get(&id).cloned()
}

/// Handles still used by a tray, so the image store does not evict a live icon.
fn live_icons() -> Vec<usize> {
    let map = trays().lock().unwrap_or_else(|error| error.into_inner());
    map.values().flat_map(|state| [state.icon, state.pressed]).flatten().collect()
}

/// Runs one `tray.*` operation. `args` is the bridge object: `{ id, image, toolTip, menu, ... }`.
pub fn call(op: &str, args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_u64).ok_or("Tray operation needs a numeric id")?;
    match op {
        "tray.create" => {
            let image = crate::image_store::decode(&args["image"])?;
            let handle = image.map(|image| crate::image_store::store(image, &live_icons()));
            let mut map = trays().lock().unwrap_or_else(|error| error.into_inner());
            let entry = map.entry(id).or_default();
            entry.created = true;
            if handle.is_some() {
                entry.icon = handle;
            }
            if let Some(tool_tip) = args.get("toolTip").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                entry.tooltip = tool_tip.to_owned();
            }
            if let Some(title) = args.get("title").and_then(Value::as_str).filter(|text| !text.is_empty()) {
                entry.title = title.to_owned();
            }
            if let Some(menu) = args.get("menu") {
                entry.menu = menu.clone();
            }
            drop(map);
            platform::sync(id);
            Ok(json!({ "id": id }))
        }
        "tray.setIcon" => {
            let pressed = args.get("pressed").and_then(Value::as_bool).unwrap_or(false);
            let image = crate::image_store::decode(&args["image"])?;
            let handle = image.map(|image| crate::image_store::store(image, &live_icons()));
            let mut map = trays().lock().unwrap_or_else(|error| error.into_inner());
            let entry = map.entry(id).or_default();
            if pressed {
                entry.pressed = handle;
            } else {
                entry.icon = handle;
            }
            drop(map);
            platform::sync(id);
            Ok(Value::Null)
        }
        "tray.setToolTip" => {
            let tool_tip = args.get("toolTip").and_then(Value::as_str).unwrap_or("");
            let mut map = trays().lock().unwrap_or_else(|error| error.into_inner());
            map.entry(id).or_default().tooltip = tool_tip.to_owned();
            drop(map);
            platform::sync(id);
            Ok(Value::Null)
        }
        "tray.setTitle" => {
            let title = args.get("title").and_then(Value::as_str).unwrap_or("");
            let mut map = trays().lock().unwrap_or_else(|error| error.into_inner());
            map.entry(id).or_default().title = title.to_owned();
            drop(map);
            platform::sync(id);
            Ok(Value::Null)
        }
        "tray.setContextMenu" => {
            let menu = args.get("menu").cloned().unwrap_or(Value::Null);
            let mut map = trays().lock().unwrap_or_else(|error| error.into_inner());
            map.entry(id).or_default().menu = menu;
            drop(map);
            platform::sync(id);
            Ok(Value::Null)
        }
        "tray.popUpContextMenu" => {
            platform::popup(id);
            Ok(Value::Null)
        }
        "tray.destroy" => {
            trays().lock().unwrap_or_else(|error| error.into_inner()).remove(&id);
            platform::remove(id);
            Ok(Value::Null)
        }
        "tray.bounds" => Ok(platform::bounds(id)),
        other => Err(format!("Native operation '{other}' is not available in this build")),
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn tray_error(id: u64, message: impl Into<String>) {
    push_event("tray.error", json!({ "id": id, "error": message.into() }));
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{
        collections::HashMap,
        ffi::c_void,
        mem::size_of,
        ptr::null_mut,
        sync::{
            atomic::{AtomicIsize, AtomicU32, Ordering},
            mpsc::{self, Receiver, Sender},
            Mutex,
        },
        time::Duration,
    };

    use image::DynamicImage;
    use serde_json::{json, Value};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPV5HEADER, BI_BITFIELDS, DIB_RGB_COLORS, HGDIOBJ,
        },
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{
            Shell::{
                Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW,
                GetCursorPos, GetMessageW, LoadIconW, PostMessageW, PostThreadMessageW, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
                TrackPopupMenu, TranslateMessage, ICONINFO, IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_POPUP,
                MF_SEPARATOR, MF_STRING, MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL,
                WM_RBUTTONUP, WNDCLASSW,
            },
        },
    };

    const CALLBACK_MESSAGE: u32 = 0x8001; // WM_APP + 1
    const WAKE_MESSAGE: u32 = 0x8002; // WM_APP + 2

    static HWND_VALUE: AtomicIsize = AtomicIsize::new(0);
    static TASKBAR_MESSAGE: AtomicU32 = AtomicU32::new(0);

    #[derive(Clone, Copy)]
    struct IconSlot {
        handle: isize,
        owned: bool,
    }

    static APPLIED: Mutex<Option<HashMap<u64, IconSlot>>> = Mutex::new(None);

    fn applied() -> &'static Mutex<Option<HashMap<u64, IconSlot>>> {
        &APPLIED
    }

    enum Cmd {
        Sync(u64),
        Remove(u64),
        Popup(u64),
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
            .name("tsgpui-tray".into())
            .spawn(move || run(cmd_rx, ready_tx))
            .map_err(|error| format!("Unable to start the tray thread: {error}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(thread_id)) => {
                *lock = Some(Service { tx: cmd_tx.clone(), thread_id });
                Ok((cmd_tx, thread_id))
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err("The tray message window did not start".into()),
        }
    }

    fn send(command: Cmd) {
        if let Ok((tx, thread_id)) = service() {
            let _ = tx.send(command);
            unsafe { PostThreadMessageW(thread_id, WAKE_MESSAGE, 0, 0) };
        }
    }

    pub fn sync(id: u64) {
        send(Cmd::Sync(id));
    }

    pub fn remove(id: u64) {
        send(Cmd::Remove(id));
    }

    pub fn popup(id: u64) {
        send(Cmd::Popup(id));
    }

    pub fn bounds(id: u64) -> Value {
        let hwnd = HWND_VALUE.load(Ordering::SeqCst);
        if hwnd == 0 {
            return empty_bounds();
        }
        let mut identifier: NOTIFYICONIDENTIFIER = unsafe { std::mem::zeroed() };
        identifier.cbSize = size_of::<NOTIFYICONIDENTIFIER>() as u32;
        identifier.hWnd = hwnd as HWND;
        identifier.uID = id as u32;
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        if unsafe { Shell_NotifyIconGetRect(&identifier, &mut rect) } < 0 {
            return empty_bounds();
        }
        json!({
            "x": rect.left,
            "y": rect.top,
            "width": rect.right - rect.left,
            "height": rect.bottom - rect.top,
        })
    }

    fn empty_bounds() -> Value {
        json!({ "x": 0, "y": 0, "width": 0, "height": 0 })
    }

    fn run(rx: Receiver<Cmd>, ready: Sender<Result<u32, String>>) {
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class_name = wide("TSGPUITrayWindow");
            let window_name = wide("TSGPUI tray");
            let window_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&window_class);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                let _ = ready.send(Err("Unable to create the tray message window".into()));
                return;
            }
            HWND_VALUE.store(hwnd as isize, Ordering::SeqCst);
            TASKBAR_MESSAGE.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Ordering::SeqCst);
            let thread_id = GetCurrentThreadId();
            let _ = ready.send(Ok(thread_id));

            let mut message: MSG = std::mem::zeroed();
            loop {
                let result = GetMessageW(&mut message, null_mut(), 0, 0);
                if result <= 0 {
                    break;
                }
                if message.message == WAKE_MESSAGE {
                    while let Ok(command) = rx.try_recv() {
                        apply(command, hwnd);
                    }
                    continue;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn apply(command: Cmd, hwnd: HWND) {
        match command {
            Cmd::Sync(id) => sync_icon(id, hwnd),
            Cmd::Remove(id) => remove_icon(id, hwnd),
            Cmd::Popup(id) => show_menu(id, hwnd),
        }
    }

    fn notify_data(hwnd: HWND, id: u64, icon: isize, tooltip: &str) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = id as u32;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = CALLBACK_MESSAGE;
        data.hIcon = icon as *mut c_void;
        copy_wide(&mut data.szTip, tooltip);
        data
    }

    fn sync_icon(id: u64, hwnd: HWND) {
        let Some(state) = super::state_of(id) else { return };
        if !state.created {
            return;
        }
        let (icon, owned) = icon_for(state.icon);
        let data = notify_data(hwnd, id, icon as isize, &state.tooltip);
        let existing = applied()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|map| map.contains_key(&id));
        let result = unsafe { Shell_NotifyIconW(if existing { NIM_MODIFY } else { NIM_ADD }, &data) };
        if result == 0 {
            if owned && !existing {
                unsafe { DestroyIcon(icon) };
            }
            super::tray_error(id, "Shell_NotifyIcon could not add or update the tray icon");
            return;
        }
        let previous = {
            let mut lock = applied().lock().unwrap_or_else(|error| error.into_inner());
            lock.get_or_insert_with(HashMap::new).insert(id, IconSlot { handle: icon as isize, owned })
        };
        if let Some(previous) = previous {
            if previous.owned && previous.handle != icon as isize {
                unsafe { DestroyIcon(previous.handle as *mut c_void) };
            }
        }
    }

    fn remove_icon(id: u64, hwnd: HWND) {
        let data = notify_data(hwnd, id, 0, "");
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        let previous = applied().lock().unwrap_or_else(|error| error.into_inner()).as_mut().and_then(|map| map.remove(&id));
        if let Some(slot) = previous {
            if slot.owned {
                unsafe { DestroyIcon(slot.handle as *mut c_void) };
            }
        }
    }

    fn readd_all(hwnd: HWND) {
        let icons: Vec<(u64, isize)> = applied()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|map| map.iter().map(|(id, slot)| (*id, slot.handle)).collect())
            .unwrap_or_default();
        for (id, icon) in icons {
            if let Some(state) = super::state_of(id) {
                if !state.created {
                    continue;
                }
                let data = notify_data(hwnd, id, icon, &state.tooltip);
                unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
            }
        }
    }

    fn icon_for(handle: Option<usize>) -> (*mut c_void, bool) {
        if let Some(image) = handle.and_then(crate::image_store::load) {
            let icon = create_icon(&image);
            if !icon.is_null() {
                return (icon, true);
            }
        }
        // A shared stock icon, so a tray created without an image still appears.
        let icon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        (icon, false)
    }

    fn create_icon(image: &DynamicImage) -> *mut c_void {
        let (width, height, pixels) = crate::image_store::rgba_square(image, 32);
        unsafe {
            let mut header: BITMAPV5HEADER = std::mem::zeroed();
            header.bV5Size = size_of::<BITMAPV5HEADER>() as u32;
            header.bV5Width = width as i32;
            header.bV5Height = -(height as i32);
            header.bV5Planes = 1;
            header.bV5BitCount = 32;
            header.bV5Compression = BI_BITFIELDS;
            header.bV5RedMask = 0x00ff_0000;
            header.bV5GreenMask = 0x0000_ff00;
            header.bV5BlueMask = 0x0000_00ff;
            header.bV5AlphaMask = 0xff00_0000;
            let device = GetDC(null_mut());
            let mut bits: *mut c_void = null_mut();
            let color = CreateDIBSection(device, &header as *const BITMAPV5HEADER as *const BITMAPINFO, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            ReleaseDC(null_mut(), device);
            if color.is_null() || bits.is_null() {
                return null_mut();
            }
            let destination = std::slice::from_raw_parts_mut(bits as *mut u8, pixels.len());
            for (source, target) in pixels.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
                target[0] = source[2];
                target[1] = source[1];
                target[2] = source[0];
                target[3] = source[3];
            }
            let mask = CreateBitmap(width as i32, height as i32, 1, 1, null_mut());
            let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
            let icon = CreateIconIndirect(&info);
            DeleteObject(mask as HGDIOBJ);
            DeleteObject(color as HGDIOBJ);
            icon
        }
    }

    fn show_menu(id: u64, hwnd: HWND) {
        let Some(state) = super::state_of(id) else { return };
        let Some(items) = state.menu.as_array() else { return };
        if items.is_empty() {
            return;
        }
        let menu = build_menu(&state.menu);
        if menu.is_null() {
            return;
        }
        let mut point: POINT = unsafe { std::mem::zeroed() };
        unsafe {
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
            let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, std::ptr::null());
            DestroyMenu(menu);
            PostMessageW(hwnd, WM_NULL, 0, 0);
            if command > 0 {
                super::push_event("tray.event", json!({ "id": id, "event": "menu-click", "itemId": command as u64 }));
            }
        }
    }

    fn build_menu(value: &Value) -> windows_sys::Win32::UI::WindowsAndMessaging::HMENU {
        let Some(items) = value.as_array() else { return null_mut() };
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return menu;
        }
        for item in items {
            append_item(menu, item);
        }
        menu
    }

    fn append_item(menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU, item: &Value) {
        if item.get("visible").and_then(Value::as_bool).unwrap_or(true) == false {
            return;
        }
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("normal");
        if kind == "separator" {
            unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null()) };
            return;
        }
        let enabled = item.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        let label = wide(item.get("label").and_then(Value::as_str).unwrap_or(""));
        let submenu = item.get("submenu").filter(|value| value.is_array());
        if kind == "submenu" || submenu.is_some() {
            if let Some(submenu) = submenu {
                let handle = build_menu(submenu);
                if !handle.is_null() {
                    let mut flags = MF_POPUP;
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    unsafe { AppendMenuW(menu, flags, handle as usize, label.as_ptr()) };
                }
            }
            return;
        }
        let command = item.get("itemId").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut flags = MF_STRING;
        if !enabled {
            flags |= MF_GRAYED;
        }
        if item.get("checked").and_then(Value::as_bool).unwrap_or(false) {
            flags |= MF_CHECKED;
        }
        unsafe { AppendMenuW(menu, flags, command, label.as_ptr()) };
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let taskbar = TASKBAR_MESSAGE.load(Ordering::SeqCst);
        if taskbar != 0 && message == taskbar {
            readd_all(hwnd);
            return 0;
        }
        if message == CALLBACK_MESSAGE {
            let id = wparam as u64;
            let mouse = (lparam as u32) & 0xffff;
            match mouse {
                WM_LBUTTONUP => super::push_event("tray.event", json!({ "id": id, "event": "click" })),
                WM_LBUTTONDBLCLK => super::push_event("tray.event", json!({ "id": id, "event": "double-click" })),
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    super::push_event("tray.event", json!({ "id": id, "event": "right-click" }));
                    show_menu(id, hwnd);
                }
                _ => {}
            }
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn copy_wide(target: &mut [u16], text: &str) {
        let mut length = 0;
        for (index, unit) in text.encode_utf16().enumerate() {
            if index + 1 >= target.len() {
                break;
            }
            target[index] = unit;
            length = index + 1;
        }
        target[length] = 0;
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadOnly};
    use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
    use objc2_foundation::{NSData, NSString};
    use serde_json::{json, Value};

    use super::TrayState;

    struct TargetIvars {
        tray_id: Cell<u64>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "TSGPUITTrayTarget"]
        #[ivars = TargetIvars]
        struct TrayTarget;

        impl TrayTarget {
            #[unsafe(method(trayClicked:))]
            fn tray_clicked(&self, _sender: &AnyObject) {
                super::push_event("tray.event", json!({ "id": self.ivars().tray_id.get(), "event": "click" }));
            }

            #[unsafe(method(menuItemClicked:))]
            fn menu_item_clicked(&self, sender: &NSMenuItem) {
                let item = sender.tag() as u64;
                super::push_event("tray.event", json!({
                    "id": self.ivars().tray_id.get(),
                    "event": "menu-click",
                    "itemId": item,
                }));
            }
        }
    );

    struct MacTray {
        item: Retained<NSStatusItem>,
        target: Retained<TrayTarget>,
    }

    thread_local! {
        static TRAY_MAP: RefCell<HashMap<u64, MacTray>> = RefCell::new(HashMap::new());
    }

    pub fn sync(id: u64) {
        dispatch::Queue::main().exec_async(move || {
            let Some(marker) = objc2::MainThreadMarker::new() else { return };
            let Some(state) = super::state_of(id) else { return };
            if !state.created {
                return;
            }
            TRAY_MAP.with(|map| {
                let mut map = map.borrow_mut();
                let entry = map.entry(id).or_insert_with(|| create_tray(marker, id));
                apply(marker, entry, &state);
            });
        });
    }

    pub fn remove(id: u64) {
        dispatch::Queue::main().exec_async(move || {
            TRAY_MAP.with(|map| {
                if let Some(entry) = map.borrow_mut().remove(&id) {
                    NSStatusBar::systemStatusBar().removeStatusItem(&entry.item);
                }
            });
        });
    }

    pub fn popup(id: u64) {
        // With a menu attached the status item opens it on click; without a menu there is nothing
        // to pop up. `popUpContextMenu` therefore only needs the menu to be current.
        sync(id);
    }

    pub fn bounds(_id: u64) -> Value {
        json!({ "x": 0, "y": 0, "width": 0, "height": 0 })
    }

    fn create_tray(marker: objc2::MainThreadMarker, id: u64) -> MacTray {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let target = TrayTarget::alloc(marker).set_ivars(TargetIvars { tray_id: Cell::new(id) });
        let target: Retained<TrayTarget> = unsafe { msg_send![super(target), init] };
        if let Some(button) = item.button(marker) {
            unsafe {
                button.setTarget(Some(target.as_ref() as &AnyObject));
                button.setAction(Some(sel!(trayClicked:)));
            }
        }
        MacTray { item, target }
    }

    fn apply(marker: objc2::MainThreadMarker, entry: &MacTray, state: &TrayState) {
        let Some(button) = entry.item.button(marker) else { return };
        if let Some(image) = state.icon.and_then(crate::image_store::load) {
            if let Ok(bytes) = crate::image_store::png_bytes(&image) {
                let data = NSData::with_bytes(&bytes);
                if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                    image.setSize(objc2_foundation::NSSize::new(18.0, 18.0));
                    button.setImage(Some(&image));
                }
            }
        } else {
            button.setImage(None);
        }
        if let Some(image) = state.pressed.and_then(crate::image_store::load) {
            if let Ok(bytes) = crate::image_store::png_bytes(&image) {
                let data = NSData::with_bytes(&bytes);
                if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                    image.setSize(objc2_foundation::NSSize::new(18.0, 18.0));
                    button.setAlternateImage(Some(&image));
                }
            }
        } else {
            button.setAlternateImage(None);
        }
        let tooltip = NSString::from_str(&state.tooltip);
        button.setToolTip(if state.tooltip.is_empty() { None } else { Some(&tooltip) });
        button.setTitle(&NSString::from_str(&state.title));
        let menu = state.menu.as_array().filter(|items| !items.is_empty()).map(|items| build_menu(marker, items, &entry.target));
        entry.item.setMenu(menu.as_deref());
    }

    fn build_menu(marker: objc2::MainThreadMarker, items: &[Value], target: &TrayTarget) -> Retained<NSMenu> {
        let menu = NSMenu::new(marker);
        for item in items {
            if item.get("visible").and_then(Value::as_bool).unwrap_or(true) == false {
                continue;
            }
            let kind = item.get("type").and_then(Value::as_str).unwrap_or("normal");
            if kind == "separator" {
                let separator = NSMenuItem::separatorItem(marker);
                menu.addItem(&separator);
                continue;
            }
            let label = NSString::from_str(item.get("label").and_then(Value::as_str).unwrap_or(""));
            let menu_item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(marker), &label, Some(sel!(menuItemClicked:)), &NSString::from_str(""))
            };
            menu_item.setTag(item.get("itemId").and_then(Value::as_u64).unwrap_or(0) as isize);
            menu_item.setEnabled(item.get("enabled").and_then(Value::as_bool).unwrap_or(true));
            menu_item.setHidden(!item.get("visible").and_then(Value::as_bool).unwrap_or(true));
            menu_item.setState(if item.get("checked").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 });
            if let Some(submenu) = item.get("submenu").and_then(Value::as_array) {
                if !submenu.is_empty() {
                    let submenu = build_menu(marker, submenu, target);
                    menu_item.setSubmenu(Some(&submenu));
                }
            }
            let target_object: &AnyObject = unsafe { &*(target as *const TrayTarget as *const AnyObject) };
            unsafe {
                menu_item.setTarget(Some(target_object));
                menu_item.setAction(Some(sel!(menuItemClicked:)));
            }
            menu.addItem(&menu_item);
        }
        menu
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    use ksni::blocking::{Handle, TrayMethods};
    use serde_json::Value;

    use super::TrayState;

    struct LinuxTray {
        id: u64,
        state: TrayState,
    }

    impl ksni::Tray for LinuxTray {
        fn id(&self) -> String {
            format!("tsgpui-{}", self.id)
        }

        fn title(&self) -> String {
            self.state.title.clone()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.state.icon.and_then(crate::image_store::load).map(|image| {
                let (width, height, mut data) = crate::image_store::rgba_square(&image, 32);
                for pixel in data.chunks_exact_mut(4) {
                    pixel.rotate_right(1); // RGBA to ARGB
                }
                vec![ksni::Icon { width: width as i32, height: height as i32, data }]
            }).unwrap_or_default()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                icon_name: String::new(),
                icon_pixmap: Vec::new(),
                title: self.state.title.clone(),
                description: self.state.tooltip.clone(),
            }
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            build_menu(&self.state.menu)
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            super::push_event("tray.event", serde_json::json!({ "id": self.id, "event": "click" }));
        }

        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            super::push_event("tray.event", serde_json::json!({ "id": self.id, "event": "right-click" }));
        }
    }

    fn build_menu(value: &Value) -> Vec<ksni::MenuItem<LinuxTray>> {
        let Some(items) = value.as_array() else { return Vec::new() };
        let mut menu = Vec::new();
        for item in items {
            if item.get("visible").and_then(Value::as_bool).unwrap_or(true) == false {
                continue;
            }
            let kind = item.get("type").and_then(Value::as_str).unwrap_or("normal");
            if kind == "separator" {
                menu.push(ksni::MenuItem::Separator);
                continue;
            }
            let label = item.get("label").and_then(Value::as_str).unwrap_or("").to_owned();
            let enabled = item.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let item_id = item.get("itemId").and_then(Value::as_u64).unwrap_or(0);
            if item.get("submenu").and_then(Value::as_array).is_some() {
                menu.push(ksni::menu::SubMenu {
                    label,
                    enabled,
                    visible: true,
                    submenu: build_menu(item.get("submenu").unwrap_or(&Value::Null)),
                    ..Default::default()
                }.into());
                continue;
            }
            let checked = item.get("checked").and_then(Value::as_bool).unwrap_or(false);
            let activate = Box::new(move |tray: &mut LinuxTray| {
                super::push_event("tray.event", serde_json::json!({ "id": tray.id, "event": "menu-click", "itemId": item_id }))
            });
            if kind == "checkbox" || kind == "radio" {
                menu.push(ksni::menu::CheckmarkItem {
                    label,
                    enabled,
                    visible: true,
                    checked,
                    activate,
                    ..Default::default()
                }.into());
            } else {
                menu.push(ksni::menu::StandardItem {
                    label,
                    enabled,
                    visible: true,
                    activate,
                    ..Default::default()
                }.into());
            }
        }
        menu
    }

    fn handles() -> &'static Mutex<HashMap<u64, Handle<LinuxTray>>> {
        static HANDLES: OnceLock<Mutex<HashMap<u64, Handle<LinuxTray>>>> = OnceLock::new();
        HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn sync(id: u64) {
        let Some(state) = super::state_of(id) else { return };
        if !state.created {
            return;
        }
        let mut map = handles().lock().unwrap_or_else(|error| error.into_inner());
        if let Some(handle) = map.get(&id) {
            let state = state.clone();
            handle.update(move |tray| tray.state = state);
            return;
        }
        let tray = LinuxTray { id, state };
        match tray.spawn() {
            Ok(handle) => {
                map.insert(id, handle);
            }
            Err(error) => super::tray_error(id, format!("Unable to create the StatusNotifierItem: {error}")),
        }
    }

    pub fn remove(id: u64) {
        if let Some(handle) = handles().lock().unwrap_or_else(|error| error.into_inner()).remove(&id) {
            handle.shutdown().wait();
        }
    }

    pub fn popup(_id: u64) {
        // StatusNotifierItem hosts open the menu on their own; there is no request to pop it up.
    }

    pub fn bounds(_id: u64) -> serde_json::Value {
        serde_json::json!({ "x": 0, "y": 0, "width": 0, "height": 0 })
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use serde_json::Value;

    pub fn sync(_id: u64) {}
    pub fn remove(_id: u64) {}
    pub fn popup(_id: u64) {}
    pub fn bounds(_id: u64) -> Value {
        serde_json::json!({ "x": 0, "y": 0, "width": 0, "height": 0 })
    }
}

#![cfg_attr(all(target_os = "windows",not(debug_assertions)),windows_subsystem="windows")]

use anyhow::{Context, Result, bail};
use std::{collections::HashMap, sync::{Arc, RwLock}, time::{Duration, Instant}};
use serde_json::{json, Value};
use tao::{
    dpi::{LogicalPosition, LogicalSize},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Fullscreen, Window, WindowBuilder},
};
use wry::{http::{header::CONTENT_TYPE, Response}, WebViewBuilder};
#[cfg(target_os = "windows")]
use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};

#[allow(dead_code)]
#[path = "webview_package.rs"] mod package;
#[path = "webview_capabilities.rs"] mod capabilities;
mod platform;
mod protocol;
mod webview_bridge;
mod image_store;
mod tray;
mod hotkeys;

/// Events delivered to the UI thread: page requests, and results from the bridge worker thread.
enum UserEvent {
    Request(u64, Value),
    Finished(u64, Value),
    /// A native tray or hotkey event was queued; wake the loop so it can be delivered.
    Wake,
}

struct AppWindow {
    window: Window,
    webview: wry::WebView,
    /// Whether the page subscribed to 'close', so user close requests are sent to it instead of applied.
    close_watched: bool,
    minimized: bool,
    maximized: bool,
    /// The parent this window is modal to. Its input is disabled while this window is open.
    modal_parent: Option<u64>,
}

// Owner and modal support is Windows-only. Other platforms reject parent windows in create_window.
#[cfg(target_os = "windows")]
fn enable_window(window: &Window, enabled: bool) { window.set_enable(enabled); }
#[cfg(not(target_os = "windows"))]
fn enable_window(_window: &Window, _enabled: bool) {}

#[cfg(target_os = "windows")]
fn owner_handle(window: &Window) -> isize { window.hwnd() }
#[cfg(not(target_os = "windows"))]
fn owner_handle(_window: &Window) -> isize { 0 }

#[cfg(target_os = "windows")]
fn with_owner(builder: WindowBuilder, owner: Option<isize>) -> WindowBuilder {
    match owner { Some(handle) => builder.with_owner_window(handle), None => builder }
}
#[cfg(not(target_os = "windows"))]
fn with_owner(builder: WindowBuilder, _owner: Option<isize>) -> WindowBuilder { builder }

/// Window id of the hidden backend window, which runs the app's main-process code when `backendWindow` is "hidden".
const BACKEND_ID: u64 = 0;
/// Path of the backend document in the hidden backend window.
const BACKEND_PATH: &str = "/__tsgpui/backend.html";

/// The page that runs the app's main-process code: the hidden backend window, or else the primary window.
fn backend_id(windows: &HashMap<u64, AppWindow>) -> u64 {
    if windows.contains_key(&BACKEND_ID) { BACKEND_ID } else { 1 }
}

/// Whether a user-visible window remains open. The hidden backend window does not count.
fn user_windows_open(windows: &HashMap<u64, AppWindow>) -> bool {
    windows.keys().any(|id| *id != BACKEND_ID)
}

/// Lists the monitors in device-independent pixels. The primary display is marked.
fn display_list(window: &Window) -> Value {
    let primary = window.primary_monitor().map(|m| { let p = m.position(); (p.x, p.y) });
    let displays: Vec<Value> = window.available_monitors().enumerate().map(|(index, monitor)| {
        let scale = monitor.scale_factor().max(0.01);
        let position = monitor.position();
        let size = monitor.size();
        json!({
            "id": index + 1,
            "x": f64::from(position.x) / scale, "y": f64::from(position.y) / scale,
            "width": f64::from(size.width) / scale, "height": f64::from(size.height) / scale,
            "scaleFactor": scale, "primary": primary == Some((position.x, position.y)),
        })
    }).collect();
    json!({"displays": displays})
}

/// Sends a native window lifecycle event to the page that runs the app's main-process code.
fn notify_main(windows: &HashMap<u64, AppWindow>, id: u64, name: &str, payload: Option<Value>) {
    if let Some(main) = windows.get(&backend_id(windows)) {
        let script = if let Some(payload) = payload {
            format!("globalThis.__tsgpui_window_event?.({},{},{})", id, json!(name), payload)
        } else {
            format!("globalThis.__tsgpui_window_event?.({},{})", id, json!(name))
        };
        let _ = main.webview.evaluate_script(&script);
    }
}

/// Delivers tray clicks, menu choices and global shortcut presses to the compatibility shim.
fn deliver_native_events(windows: &HashMap<u64, AppWindow>) {
    let events = crate::tray::poll_events();
    if events.is_empty() {
        return;
    }
    if let Some(main) = windows.get(&backend_id(windows)) {
        for event in events {
            let script = format!("globalThis.__tsgpui_host_event?.({},{})", event["channel"], event["payload"]);
            let _ = main.webview.evaluate_script(&script);
        }
    }
}

/// The URL that `WebView::load_url` navigates to for a packaged path. Unlike `with_url`, `load_url` does not
/// apply wry's WebView2 custom-protocol workaround, so Windows must use its `http://tsgpui.localhost` form.
/// Navigating to `tsgpui://` directly leaves the page blank because WebView2 does not know that scheme.
fn page_url(path: &str) -> String {
    if cfg!(target_os = "windows") { format!("http://tsgpui.localhost{path}") } else { format!("tsgpui://localhost{path}") }
}

/// Removes a window and re-enables its modal parent once no other modal child remains open.
fn remove_window(windows: &mut HashMap<u64, AppWindow>, id: u64) -> Option<AppWindow> {
    let removed = windows.remove(&id)?;
    if let Some(parent) = removed.modal_parent {
        if !windows.values().any(|w| w.modal_parent == Some(parent)) {
            if let Some(owner) = windows.get(&parent) { enable_window(&owner.window, true); owner.window.set_focus(); }
        }
    }
    Some(removed)
}

/// Creates a window from `window.create`. A parent makes the new window owned by it (Windows only);
/// a modal window also disables its parent until it closes.
fn create_window(windows: &mut HashMap<u64, AppWindow>, target: &tao::event_loop::EventLoopWindowTarget<UserEvent>, page: &Arc<RwLock<package::AppPackage>>, proxy: &EventLoopProxy<UserEvent>, document: &package::AppPackage, id: u64, args: &Value) -> Result<Value, String> {
    if id < 2 || windows.contains_key(&id) { return Err("Invalid or duplicate window ID".into()); }
    let owner = match args["parentWindowId"].as_u64() {
        None => None,
        Some(_) if !cfg!(target_os = "windows") => return Err("Parent windows are supported on Windows only".into()),
        Some(parent) => match windows.get(&parent) {
            Some(owner) => Some((parent, owner_handle(&owner.window))),
            None => return Err("Parent window is not open".into()),
        },
    };
    let modal = args["modal"].as_bool().unwrap_or(false) && owner.is_some();
    let size = LogicalSize::new(args["width"].as_f64().unwrap_or(800.0).clamp(200.0,4096.0), args["height"].as_f64().unwrap_or(600.0).clamp(200.0,4096.0));
    let builder = WindowBuilder::new().with_title(args["title"].as_str().unwrap_or(&document.title)).with_inner_size(size);
    let builder = builder.with_visible(args["show"].as_bool().unwrap_or(true));
    let builder = if args["x"].as_f64().is_some() || args["y"].as_f64().is_some() { builder.with_position(LogicalPosition::new(args["x"].as_f64().unwrap_or(0.0), args["y"].as_f64().unwrap_or(0.0))) } else { builder };
    let builder = if args["fullScreen"].as_bool().unwrap_or(false) { builder.with_fullscreen(Some(Fullscreen::Borderless(None))) } else { builder };
    let builder = with_owner(builder, owner.map(|(_, handle)| handle));
    let window = builder.build(target).map_err(|e| e.to_string())?;
    let url = format!("tsgpui://localhost{}", document.webview_entry.as_deref().unwrap_or("/index.html"));
    let mut app_window = create_webview(window, Arc::clone(page), proxy.clone(), id, &url).map_err(|e| e.to_string())?;
    app_window.modal_parent = if modal { owner.map(|(parent, _)| parent) } else { None };
    if let Some(parent) = app_window.modal_parent {
        if let Some(owner) = windows.get(&parent) { enable_window(&owner.window, false); }
    }
    app_window.window.set_focus();
    windows.insert(id, app_window);
    Ok(Value::Null)
}

fn create_webview(window: Window, page: Arc<RwLock<package::AppPackage>>, proxy: EventLoopProxy<UserEvent>, id: u64, url: &str) -> Result<AppWindow> {
    let (ipc_allowed, hidden_backend) = {
        let doc = page.read().map_err(|_| anyhow::anyhow!("Cannot read application document"))?;
        (doc.permissions.ipc, doc.backend_window.as_deref() == Some("hidden"))
    };
    // Renderer pages send IPC to the host. The page that runs the backend dispatches its own IPC locally.
    let secondary = id >= 2 || (id == 1 && hidden_backend);
    let builder = WebViewBuilder::new()
        .with_custom_protocol("tsgpui".into(), move |_webview_id, request| {
            let response = match page.read() {
                Ok(doc) => {
                    let path = webview_bridge::url_path(request.uri().path()).unwrap_or_default();
                    let entry = doc.webview_entry.as_deref().unwrap_or("/index.html");
                    if path == entry || (doc.webview_format_version < 2 && path == "/index.html") {
                        Response::builder().header(CONTENT_TYPE, "text/html; charset=utf-8").body(doc.webview_html.as_deref().unwrap_or("").as_bytes().to_vec())
                    } else if path == BACKEND_PATH {
                        Response::builder().header(CONTENT_TYPE, "text/html; charset=utf-8").body(doc.webview_main_html.as_deref().unwrap_or("").as_bytes().to_vec())
                    } else if let Some(asset) = doc.webview_files.get(&path) {
                        match webview_bridge::decode(&asset.data) {
                            Ok(bytes) => Response::builder().header(CONTENT_TYPE, &asset.mime).body(bytes),
                            Err(_) => Response::builder().status(500).body(Vec::new())
                        }
                    } else { Response::builder().status(404).body(b"Not found".to_vec()) }
                }
                Err(_) => Response::builder().status(500).body(b"WebView document could not be read".to_vec()),
            };
            response.unwrap().map(Into::into)
        })
        .with_initialization_script(&format!("window.__tsgpui_ipc_allowed={};window.__tsgpui_secondary={};window.__tsgpui_window_id={};{}",ipc_allowed,secondary,id,webview_bridge::SCRIPT))
        .with_ipc_handler(move |request| { if let Ok(value)=serde_json::from_str::<Value>(request.body()) { let _=proxy.send_event(UserEvent::Request(id,value)); } })
        .with_navigation_handler(|url| url.starts_with("tsgpui://localhost/") || url.starts_with("http://tsgpui.localhost/") || url.starts_with("https://tsgpui.localhost/"))
        .with_url(url)
        .with_devtools(cfg!(debug_assertions));
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.gtk_window()).context("Unable to create WebKitGTK view")?
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window).context("Unable to create the system WebView")?;
    Ok(AppWindow { window, webview, close_watched: false, minimized: false, maximized: false, modal_parent: None })
}

fn resolve(webview: &wry::WebView, id: u64, result: &Result<Value, String>) {
    let (ok, value) = match result {
        Ok(value) => (true, value.clone()),
        Err(error) => (false, json!(error)),
    };
    resolve_raw(webview, id, ok, &value);
}

fn resolve_raw(webview: &wry::WebView, id: u64, ok: bool, value: &Value) {
    let _ = webview.evaluate_script(&format!("window.__tsgpui_resolve({id},{ok},{})", value));
}

fn main() -> Result<()> {
    let document = package::load().map_err(anyhow::Error::msg)?;
    if document.renderer != "webview" {
        bail!("This is the WebView host, but the app bundle selects renderer '{}'.", document.renderer);
    }
    let page = Arc::new(RwLock::new(document.clone()));
    let bundle_file = package::bundle_path().map_err(anyhow::Error::msg)?;
    let base = bundle_file.as_ref().and_then(|p|p.parent()).map(std::path::Path::to_path_buf)
        .or_else(||std::env::current_exe().ok().and_then(|p|p.parent().map(std::path::Path::to_path_buf))).unwrap_or_default();
    let mut bundle_modified = bundle_file.as_deref().and_then(|file| std::fs::metadata(file).and_then(|meta| meta.modified()).ok());
    let mut next_power_poll = Instant::now();
    let mut next_state_poll = Instant::now();
    let mut last_displays: Option<Vec<Value>> = None;
    let mut last_preferences: Option<Value> = None;

    #[cfg(target_os = "linux")]
    gtk::init().context("Unable to initialize GTK/WebKitGTK for the WebView renderer")?;

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let worker_proxy = proxy.clone();
    let wake_proxy = proxy.clone();
    crate::tray::set_event_notifier(move || { let _ = wake_proxy.send_event(UserEvent::Wake); });
    let window = WindowBuilder::new()
        .with_title(&document.title)
        .with_inner_size(LogicalSize::new(f64::from(document.width), f64::from(document.height)))
        .build(&event_loop)
        .context("Unable to create the application window")?;

    let primary = create_webview(window, Arc::clone(&page), proxy.clone(), 1, &format!("tsgpui://localhost{}",document.webview_entry.as_deref().unwrap_or("/index.html")))?;
    let mut windows = HashMap::from([(1_u64, primary)]);
    let backend_hidden = document.backend_window.as_deref() == Some("hidden");
    if backend_hidden {
        // The backend runs in an invisible window, so IPC keeps working after the primary window closes.
        let backend_window = WindowBuilder::new().with_title(&document.title).with_visible(false).build(&event_loop).context("Unable to create the backend window")?;
        windows.insert(BACKEND_ID, create_webview(backend_window, Arc::clone(&page), proxy.clone(), BACKEND_ID, &format!("tsgpui://localhost{BACKEND_PATH}"))?);
    }

    // Bridge calls run on one worker thread that owns the session. Dialogs, network requests and processes
    // can block for a long time; running them on the UI thread stops the window from pumping messages.
    let (jobs, job_queue) = std::sync::mpsc::channel::<(u64, Value)>();
    let worker_permissions = document.permissions.clone();
    let worker_base = base.clone();
    let worker_name = document.name.clone();
    let worker_version = document.version.clone();
    std::thread::spawn(move || {
        let mut session = webview_bridge::Session::default();
        for (origin, request) in job_queue {
            let id = request["id"].as_u64().unwrap_or(0);
            let op = request["op"].as_str().unwrap_or("").to_owned();
            let result = webview_bridge::call(&op, &request["args"], &worker_permissions, &worker_base, &worker_name, &worker_version, &mut session);
            let (ok, value) = match result {
                Ok(value) => (true, value),
                Err(error) => (false, json!(error)),
            };
            if worker_proxy.send_event(UserEvent::Finished(origin, json!({"id": id, "ok": ok, "value": value}))).is_err() { break; }
        }
    });
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(450));
        match event {
            Event::UserEvent(UserEvent::Wake) => deliver_native_events(&windows),
            Event::UserEvent(UserEvent::Finished(origin, done)) => {
                if let Some(w) = windows.get(&origin) { resolve_raw(&w.webview, done["id"].as_u64().unwrap_or(0), done["ok"].as_bool().unwrap_or(false), &done["value"]); }
            }
            Event::UserEvent(UserEvent::Request(origin, request)) if request["op"] == "ipc.reply" && origin == backend_id(&windows) => {
                let args = &request["args"];
                if let Some(w) = windows.get(&args["origin"].as_u64().unwrap_or(0)) { resolve_raw(&w.webview, args["requestId"].as_u64().unwrap_or(0), args["ok"].as_bool().unwrap_or(false), &args["value"]); }
            }
            Event::UserEvent(UserEvent::Request(origin, request)) if origin != backend_id(&windows) && (request["op"] == "ipc.invoke" || request["op"] == "ipc.send") => {
                let id = request["id"].as_u64().unwrap_or(0);
                if !document.permissions.ipc { if let Some(w) = windows.get(&origin) { resolve(&w.webview, id, &Err("IPC permission denied".into())); } }
                else if let Some(main) = windows.get(&backend_id(&windows)) {
                    let kind = if request["op"] == "ipc.send" { "send" } else { "invoke" };
                    let args = &request["args"];
                    let script = format!("Promise.resolve().then(()=>globalThis.__tsgpui_ipc_dispatch({},{},{},{})).then(value=>window.ipc.postMessage(JSON.stringify({{op:'ipc.reply',args:{{origin:{},requestId:{},ok:true,value:value??null}}}})),error=>window.ipc.postMessage(JSON.stringify({{op:'ipc.reply',args:{{origin:{},requestId:{},ok:false,value:String(error)}}}})))", json!(kind), args["channel"], args["payload"], origin, origin, id, origin, id);
                    if let Err(error) = main.webview.evaluate_script(&script) { if let Some(w) = windows.get(&origin) { resolve(&w.webview, id, &Err(error.to_string())); } }
                } else if let Some(w) = windows.get(&origin) { resolve(&w.webview, id, &Err("Main window is closed; IPC is unavailable".into())); }
            }
            Event::UserEvent(UserEvent::Request(origin, request)) if request["op"] == "screen.displays" => {
                // Display geometry needs a window handle, so it is answered on the UI thread.
                let id = request["id"].as_u64().unwrap_or(0);
                let result: Result<Value, String> = match windows.get(&1).or_else(|| windows.values().next()) {
                    Some(w) => Ok(display_list(&w.window)),
                    None => Err("No window is open".into()),
                };
                if let Some(w) = windows.get(&origin) { resolve(&w.webview, id, &result); }
            }
            Event::UserEvent(UserEvent::Request(_, request)) if request["op"] == "app.quit" => {
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Request(origin, request)) if origin != backend_id(&windows) && matches!(request["op"].as_str().unwrap_or(""), op if op.starts_with("tray.") || op.starts_with("globalShortcut.")) => {
                // Tray icons and hotkeys are app-level objects. Only the page that runs the
                // backend may create them; a renderer page could otherwise watch keystrokes.
                let id = request["id"].as_u64().unwrap_or(0);
                if let Some(w) = windows.get(&origin) { resolve(&w.webview, id, &Err("Tray and global shortcut operations are only available to the app backend".into())); }
            }
            Event::UserEvent(UserEvent::Request(origin, request)) if !request["op"].as_str().unwrap_or("").starts_with("window.") => {
                // Bridge calls can block for a long time, so they run on the worker thread.
                let _ = jobs.send((origin, request));
            }
            Event::UserEvent(UserEvent::Request(origin, request)) => {
                let id=request["id"].as_u64().unwrap_or(0);
                let op=request["op"].as_str().unwrap_or("");
                let args=&request["args"];
                let mut remove_after: Option<u64> = None;
                let result = {
                    if !document.permissions.window { Err("Window permission denied".into()) } else {
                        let window_id = args["windowId"].as_u64().unwrap_or(origin);
                        match op {
                            "window.create" => create_window(&mut windows, target, &page, &proxy, &document, window_id, args),
                            "window.watchClose" => if let Some(w) = windows.get_mut(&window_id) { w.close_watched = true; Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.send" => if let Some(w) = windows.get(&window_id) { w.webview.evaluate_script(&format!("window.__tsgpui_receive({},{})",args["channel"],args["payload"])).map(|_|Value::Null).map_err(|e|e.to_string()) } else { Err("Window not found".into()) },
                            "window.load" => if let Some(w) = windows.get(&window_id) {
                                // `path` is the packaged path; `url` may instead carry a custom
                                // scheme (`app://...`) that maps onto the same asset map.
                                let url_mapped = args["url"].as_str().and_then(protocol::custom_url_to_path);
                                let path = url_mapped.as_deref().unwrap_or_else(|| args["path"].as_str().unwrap_or(""));
                                if !path.starts_with('/') || path.starts_with("//") || path.contains("..") || path.contains('\\') { Err("Invalid local window path".into()) }
                                else { w.webview.load_url(&page_url(path)).map(|_|Value::Null).map_err(|e|e.to_string()) }
                            } else { Err("Window not found".into()) },
                            // Removal waits until the reply is sent, so a window that closes itself still gets its result.
                            "window.close" => if window_id != BACKEND_ID && windows.contains_key(&window_id) { remove_after = Some(window_id); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.setTitle" | "window.minimize" | "window.maximize" | "window.restore" => if let Some(w) = windows.get(&window_id) {
                                match op { "window.setTitle" => w.window.set_title(args["title"].as_str().unwrap_or("")), "window.minimize" => w.window.set_minimized(true), "window.maximize" => w.window.set_maximized(true), _ => { w.window.set_minimized(false); w.window.set_maximized(false); } };
                                Ok(Value::Null)
                            } else { Err("Window not found".into()) },
                            "window.show" => if let Some(w) = windows.get(&window_id) { w.window.set_visible(true); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.hide" => if let Some(w) = windows.get(&window_id) { w.window.set_visible(false); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.setFullScreen" => if let Some(w) = windows.get(&window_id) { if args["fullScreen"].as_bool().unwrap_or(false) { w.window.set_fullscreen(Some(Fullscreen::Borderless(None))); } else { w.window.set_fullscreen(None); } Ok(Value::Null) } else { Err("Window not found".into()) },
                            // tao has no blur API; blur is tracked locally in BrowserWindow (the window keeps OS focus until another window takes it).
                            "window.focus" => if let Some(w) = windows.get(&window_id) { w.window.set_focus(); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.blur" => if windows.contains_key(&window_id) { Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.setBounds" => if let Some(w) = windows.get(&window_id) { let x = args["x"].as_f64().unwrap_or(0.0); let y = args["y"].as_f64().unwrap_or(0.0); let width = args["width"].as_f64().unwrap_or(800.0).clamp(200.0,4096.0); let height = args["height"].as_f64().unwrap_or(600.0).clamp(200.0,4096.0); w.window.set_outer_position(LogicalPosition::new(x, y)); w.window.set_inner_size(LogicalSize::new(width, height)); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.setPosition" => if let Some(w) = windows.get(&window_id) { let x = args["x"].as_f64().unwrap_or(0.0); let y = args["y"].as_f64().unwrap_or(0.0); w.window.set_outer_position(LogicalPosition::new(x, y)); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.setSize" => if let Some(w) = windows.get(&window_id) { let width = args["width"].as_f64().unwrap_or(800.0).clamp(200.0,4096.0); let height = args["height"].as_f64().unwrap_or(600.0).clamp(200.0,4096.0); w.window.set_inner_size(LogicalSize::new(width, height)); Ok(Value::Null) } else { Err("Window not found".into()) },
                            "window.getBounds" => if let Some(w) = windows.get(&window_id) { let scale = w.window.scale_factor(); let position = w.window.outer_position().map(|p| (p.x as f64 / scale, p.y as f64 / scale)).unwrap_or((0.0, 0.0)); let size = w.window.outer_size(); Ok(json!({"x": position.0, "y": position.1, "width": size.width as f64 / scale, "height": size.height as f64 / scale})) } else { Err("Window not found".into()) },
                            "window.isMaximized" => if let Some(w) = windows.get(&window_id) { Ok(json!(w.window.is_maximized())) } else { Err("Window not found".into()) },
                            "window.isMinimized" => if let Some(w) = windows.get(&window_id) { Ok(json!(w.window.is_minimized())) } else { Err("Window not found".into()) },
                            _ => Err(format!("unsupported op: {op}"))
                        }
                    }
                };
                if let Some(w) = windows.get(&origin) { resolve(&w.webview, id, &result); }
                if let Some(closed) = remove_after {
                    remove_window(&mut windows, closed);
                    if !backend_hidden && !user_windows_open(&windows) { *control_flow = ControlFlow::Exit; }
                }
            }
            Event::WindowEvent { window_id, event, .. } => {
                if let Some(id) = windows.iter().find_map(|(id, w)| (*id != BACKEND_ID && w.window.id() == window_id).then_some(*id)) {
                    match event {
                        // A page that subscribed to 'close' decides: it calls window.close if the close is not prevented.
                        WindowEvent::CloseRequested if windows.get(&id).is_some_and(|w| w.close_watched) => notify_main(&windows, id, "close-requested", None),
                        WindowEvent::CloseRequested => {
                            remove_window(&mut windows, id);
                            if let Some(main) = windows.get(&backend_id(&windows)) { let _ = main.webview.evaluate_script(&format!("globalThis.__tsgpui_window_closed?.({id})")); }
                            if !backend_hidden && !user_windows_open(&windows) { *control_flow = ControlFlow::Exit; }
                        }
                        WindowEvent::Focused(focused) => notify_main(&windows, id, if focused { "focus" } else { "blur" }, None),
                        WindowEvent::Moved(_) => {
                            let payload = windows.get(&id).map(|w| {
                                let scale = w.window.scale_factor();
                                let pos = w.window.outer_position().ok();
                                let size = w.window.outer_size();
                                json!({"x": pos.map(|p| p.x as f64 / scale).unwrap_or(0.0), "y": pos.map(|p| p.y as f64 / scale).unwrap_or(0.0), "width": size.width as f64 / scale, "height": size.height as f64 / scale})
                            });
                            notify_main(&windows, id, "move", payload);
                        }
                        WindowEvent::Resized(_) => {
                            // Minimize, maximize and restore are derived from the window state after each resize.
                            let changes: Vec<(&str, Option<Value>)> = match windows.get_mut(&id) {
                                Some(w) => {
                                    let (minimized, maximized) = (w.window.is_minimized(), w.window.is_maximized());
                                    let (was_minimized, was_maximized) = (w.minimized, w.maximized);
                                    w.minimized = minimized;
                                    w.maximized = maximized;
                                    let scale = w.window.scale_factor();
                                    let pos = w.window.outer_position().ok();
                                    let size = w.window.outer_size();
                                    let bounds = json!({"x": pos.map(|p| p.x as f64 / scale).unwrap_or(0.0), "y": pos.map(|p| p.y as f64 / scale).unwrap_or(0.0), "width": size.width as f64 / scale, "height": size.height as f64 / scale});
                                    let mut changes = Vec::new();
                                    if minimized && !was_minimized { changes.push(("minimize", None)); }
                                    if maximized && !was_maximized && !minimized { changes.push(("maximize", None)); }
                                    if !minimized && !maximized && (was_minimized || was_maximized) { changes.push(("restore", None)); }
                                    changes.push(("resize", Some(bounds)));
                                    changes
                                }
                                None => Vec::new(),
                            };
                            for (name, payload) in changes { notify_main(&windows, id, name, payload); }
                        }
                        _ => {}
                    }
                }
            },
            Event::MainEventsCleared => {
                deliver_native_events(&windows);
                // Power transitions are short-lived, so they are checked twice a second. Theme
                // and monitor changes are slower and only need a periodic snapshot.
                if Instant::now() >= next_power_poll {
                    next_power_poll = Instant::now() + Duration::from_millis(500);
                    if let Some(main) = windows.get(&backend_id(&windows)) {
                        for event in platform::take_power_events() {
                            let script = format!("globalThis.__tsgpui_power_event?.({})", json!(event));
                            let _ = main.webview.evaluate_script(&script);
                        }
                    }
                }
                if Instant::now() >= next_state_poll {
                    next_state_poll = Instant::now() + Duration::from_secs(3);
                    if let Some(main) = windows.get(&backend_id(&windows)) {
                        let preferences = platform::system_preferences();
                        if last_preferences.as_ref() != Some(&preferences) {
                            let first = last_preferences.is_none();
                            last_preferences = Some(preferences);
                            if !first {
                                let _ = main.webview.evaluate_script("globalThis.__tsgpui_system_preferences_event?.('color-changed')");
                            }
                        }
                        if let Some(window) = windows.get(&1).or_else(|| windows.values().next()) {
                            let displays = display_list(&window.window);
                            let next: Vec<Value> = displays["displays"].as_array().cloned().unwrap_or_default();
                            if let Some(previous) = last_displays.replace(next.clone()) {
                                let display_id = |display: &Value| display["id"].as_u64().unwrap_or(0);
                                let previous_ids: std::collections::HashSet<u64> = previous.iter().map(display_id).collect();
                                let next_ids: std::collections::HashSet<u64> = next.iter().map(display_id).collect();
                                for display in &next {
                                    if !previous_ids.contains(&display_id(display)) {
                                        let script = format!("globalThis.__tsgpui_display_event?.('display-added',{},null)", display);
                                        let _ = main.webview.evaluate_script(&script);
                                    }
                                }
                                for display in &previous {
                                    if !next_ids.contains(&display_id(display)) {
                                        let script = format!("globalThis.__tsgpui_display_event?.('display-removed',{},null)", display);
                                        let _ = main.webview.evaluate_script(&script);
                                    }
                                }
                                for display in &next {
                                    if let Some(old) = previous.iter().find(|old| display_id(old) == display_id(display)) {
                                        if old != display {
                                            let details = json!({"bounds": true, "workArea": true, "scaleFactor": old["scaleFactor"] != display["scaleFactor"], "rotation": false});
                                            let script = format!("globalThis.__tsgpui_display_event?.('display-metrics-changed',{},{})", display, details);
                                            let _ = main.webview.evaluate_script(&script);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(file) = bundle_file.as_ref() {
                    let changed = std::fs::metadata(file).and_then(|meta| meta.modified()).ok();
                    if changed.is_some() && changed != bundle_modified {
                        match package::load_bundle(file) {
                            Ok(next) if next.renderer == "webview" => {
                                if next.webview_html.is_some() {
                                    match page.write() {
                                        Ok(mut current) => {
                                            *current = next;
                                            // The backend and the primary page both load the bundle, so both reload.
                                            let reload_ids: &[u64] = if backend_hidden { &[BACKEND_ID, 1] } else { &[1] };
                                            let reloads: Vec<_> = reload_ids.iter().filter_map(|id| windows.get(id)).map(|w| w.webview.evaluate_script("window.location.reload()")).collect();
                                            let mut reloaded = !reloads.is_empty();
                                            for result in reloads { if let Err(error) = result { reloaded = false; eprintln!("TSGPUI WebView reload failed: {error}"); } }
                                            if reloaded { bundle_modified = changed; }
                                        }
                                        Err(error) => eprintln!("TSGPUI WebView reload failed: {error}"),
                                    }
                                }
                            }
                            Ok(_) => eprintln!("TSGPUI WebView reload ignored because the project changed renderers"),
                            Err(error) => eprintln!("TSGPUI WebView reload failed: {error}"),
                        }
                    }
                }
            }
            _ => {}
        }
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    });
}

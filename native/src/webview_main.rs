#![cfg_attr(all(target_os = "windows",not(debug_assertions)),windows_subsystem="windows")]

use anyhow::{Context, Result, bail};
use std::{sync::{Arc, RwLock}, time::{Duration, Instant}};
use serde_json::{json, Value};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{http::{header::CONTENT_TYPE, Response}, WebViewBuilder};

#[allow(dead_code)]
#[path = "webview_package.rs"] mod package;
#[path = "webview_capabilities.rs"] mod capabilities;
mod webview_bridge;

/// Events delivered to the UI thread: page requests, and results from the bridge worker thread.
enum UserEvent {
    Request(Value),
    Finished(Value),
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
    let protocol_page = Arc::clone(&page);
    let bundle_file = package::bundle_path().map_err(anyhow::Error::msg)?;
    let base = bundle_file.as_ref().and_then(|p|p.parent()).map(std::path::Path::to_path_buf)
        .or_else(||std::env::current_exe().ok().and_then(|p|p.parent().map(std::path::Path::to_path_buf))).unwrap_or_default();
    let mut bundle_modified = bundle_file.as_deref().and_then(|file| std::fs::metadata(file).and_then(|meta| meta.modified()).ok());

    #[cfg(target_os = "linux")]
    gtk::init().context("Unable to initialize GTK/WebKitGTK for the WebView renderer")?;

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let worker_proxy = proxy.clone();
    let window = WindowBuilder::new()
        .with_title(&document.title)
        .with_inner_size(LogicalSize::new(f64::from(document.width), f64::from(document.height)))
        .build(&event_loop)
        .context("Unable to create the application window")?;

    let builder = WebViewBuilder::new()
        .with_custom_protocol("tsgpui".into(), move |_webview_id, request| {
            let response = match protocol_page.read() {
                Ok(doc) => {
                    let path = webview_bridge::url_path(request.uri().path()).unwrap_or_default();
                    let entry = doc.webview_entry.as_deref().unwrap_or("/index.html");
                    if path == entry || (doc.webview_format_version < 2 && path == "/index.html") {
                        Response::builder().header(CONTENT_TYPE, "text/html; charset=utf-8")
                            .body(doc.webview_html.as_deref().unwrap_or("").as_bytes().to_vec())
                    } else if let Some(asset) = doc.webview_files.get(&path) {
                        match webview_bridge::decode(&asset.data) {
                            Ok(bytes) => Response::builder().header(CONTENT_TYPE, &asset.mime).body(bytes),
                            Err(_) => Response::builder().status(500).body(Vec::new())
                        }
                    } else { Response::builder().status(404).body(b"Not found".to_vec()) }
                }
                Err(_) => Response::builder()
                    .status(500)
                    .header(CONTENT_TYPE, "text/plain; charset=utf-8")
                    .body(b"WebView document could not be read".to_vec()),
            };
            response.unwrap().map(Into::into)
        })
        .with_initialization_script(&format!("window.__tsgpui_ipc_allowed={};{}",document.permissions.ipc,webview_bridge::SCRIPT))
        .with_ipc_handler(move |request| { if let Ok(value)=serde_json::from_str::<Value>(request.body()) { let _=proxy.send_event(UserEvent::Request(value)); } })
        .with_navigation_handler(|url| url.starts_with("tsgpui://localhost/") || url.starts_with("http://tsgpui.localhost/") || url.starts_with("https://tsgpui.localhost/"))
        .with_url(format!("tsgpui://localhost{}",document.webview_entry.as_deref().unwrap_or("/index.html")))
        .with_devtools(cfg!(debug_assertions));

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.gtk_window()).context("Unable to create WebKitGTK view")?
    };

    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window).context("Unable to create the system WebView")?;

    // Bridge calls run on one worker thread that owns the session. Dialogs, network requests and processes
    // can block for a long time; running them on the UI thread stops the window from pumping messages.
    let (jobs, job_queue) = std::sync::mpsc::channel::<Value>();
    let worker_permissions = document.permissions.clone();
    let worker_base = base.clone();
    let worker_name = document.name.clone();
    let worker_version = document.version.clone();
    std::thread::spawn(move || {
        let mut session = webview_bridge::Session::default();
        for request in job_queue {
            let id = request["id"].as_u64().unwrap_or(0);
            let op = request["op"].as_str().unwrap_or("").to_owned();
            let result = webview_bridge::call(&op, &request["args"], &worker_permissions, &worker_base, &worker_name, &worker_version, &mut session);
            let (ok, value) = match result {
                Ok(value) => (true, value),
                Err(error) => (false, json!(error)),
            };
            if worker_proxy.send_event(UserEvent::Finished(json!({"id": id, "ok": ok, "value": value}))).is_err() { break; }
        }
    });
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(450));
        match event {
            Event::UserEvent(UserEvent::Finished(done)) => {
                resolve_raw(&webview, done["id"].as_u64().unwrap_or(0), done["ok"].as_bool().unwrap_or(false), &done["value"]);
            }
            Event::UserEvent(UserEvent::Request(request)) if !request["op"].as_str().unwrap_or("").starts_with("window.") => {
                // Bridge calls can block for a long time, so they run on the worker thread.
                let _ = jobs.send(request);
            }
            Event::UserEvent(UserEvent::Request(request)) => {
                let id=request["id"].as_u64().unwrap_or(0);
                let op=request["op"].as_str().unwrap_or("");
                let args=&request["args"];
                let result = {
                    if !document.permissions.window { Err("Window permission denied".into()) } else {
                        match op {
                            "window.setTitle" => { window.set_title(args["title"].as_str().unwrap_or("")); Ok(Value::Null) },
                            "window.close" => { *control_flow=ControlFlow::Exit; Ok(Value::Null) },
                            "window.minimize" => { window.set_minimized(true); Ok(Value::Null) },
                            "window.maximize" => { window.set_maximized(true); Ok(Value::Null) },
                            "window.restore" => { window.set_minimized(false); window.set_maximized(false); Ok(Value::Null) },
                            _ => Err(format!("unsupported op: {op}"))
                        }
                    }
                };
                resolve(&webview, id, &result);
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::MainEventsCleared => {
                if let Some(file) = bundle_file.as_ref() {
                    let changed = std::fs::metadata(file).and_then(|meta| meta.modified()).ok();
                    if changed.is_some() && changed != bundle_modified {
                        match package::load_bundle(file) {
                            Ok(next) if next.renderer == "webview" => {
                                if next.webview_html.is_some() {
                                    match page.write() {
                                        Ok(mut current) => {
                                            *current = next;
                                            match webview.evaluate_script("window.location.reload()") {
                                                Ok(()) => bundle_modified = changed,
                                                Err(error) => eprintln!("TSGPUI WebView reload failed: {error}"),
                                            }
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
        let _keep_alive = (&window, &webview);
    });
}

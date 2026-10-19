#![cfg_attr(all(target_os = "windows",not(debug_assertions)),windows_subsystem="windows")]

use anyhow::{Context, Result, bail};
use std::{sync::{Arc, RwLock}, time::{Duration, Instant}};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::{http::{header::CONTENT_TYPE, Response}, WebViewBuilder};

#[allow(dead_code)]
mod package;

fn main() -> Result<()> {
    let document = package::load().map_err(anyhow::Error::msg)?;
    if document.renderer != "webview" {
        bail!("This is the WebView host, but the app bundle selects renderer '{}'.", document.renderer);
    }
    let html = document.webview_html.context("WebView app bundle is missing webview_html")?;
    let page_html = Arc::new(RwLock::new(html));
    let protocol_html = Arc::clone(&page_html);
    let bundle_file = package::bundle_path().map_err(anyhow::Error::msg)?;
    let mut bundle_modified = bundle_file.as_deref().and_then(|file| std::fs::metadata(file).and_then(|meta| meta.modified()).ok());

    #[cfg(target_os = "linux")]
    gtk::init().context("Unable to initialize GTK/WebKitGTK for the WebView renderer")?;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(document.title)
        .with_inner_size(LogicalSize::new(f64::from(document.width), f64::from(document.height)))
        .build(&event_loop)
        .context("Unable to create the application window")?;

    let builder = WebViewBuilder::new()
        .with_custom_protocol("tsgpui".into(), move |_webview_id, _request| {
            let response = match protocol_html.read() {
                Ok(html) => Response::builder()
                    .header(CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(html.as_bytes().to_vec()),
                Err(_) => Response::builder()
                    .status(500)
                    .header(CONTENT_TYPE, "text/plain; charset=utf-8")
                    .body(b"WebView document could not be read".to_vec()),
            };
            response.unwrap().map(Into::into)
        })
        .with_url("tsgpui://localhost/index.html")
        .with_devtools(cfg!(debug_assertions));

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.gtk_window()).context("Unable to create WebKitGTK view")?
    };

    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window).context("Unable to create the system WebView")?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(450));
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::MainEventsCleared => {
                if let Some(file) = bundle_file.as_ref() {
                    let changed = std::fs::metadata(file).and_then(|meta| meta.modified()).ok();
                    if changed.is_some() && changed != bundle_modified {
                        match package::load_bundle(file) {
                            Ok(next) if next.renderer == "webview" => {
                                if let Some(html) = next.webview_html {
                                    match page_html.write() {
                                        Ok(mut current) => {
                                            *current = html;
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

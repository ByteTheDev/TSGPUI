//! The native window driver: winit event loop → `Session`, `Renderer` → wgpu surface.

use crate::bundle::AppBundle;
use crate::session::{KeyInput, Mods, Session, SystemClipboard};
use rendrix_css::Cursor;
use rendrix_gfx::{GpuContext, Renderer};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId};

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct App {
    bundle: AppBundle,
    gpu: Option<Gpu>,
    session: Option<Session>,
    mods: ModifiersState,
    cursor_pos: PhysicalPosition<f64>,
    error: Option<String>,
    start: Instant,
    frames: u64,
    /// Debug aid: `RENDRIX_DUMP_FRAME=<file.png>` writes every presented frame (throttled) to that file.
    dump: Option<(std::path::PathBuf, Instant)>,
    /// `RENDRIX_TRACE_EVENTS=1` prints every window event (debugging input).
    trace: bool,
}

pub fn run(bundle: AppBundle) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("event loop: {e}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App { bundle, gpu: None, session: None, mods: ModifiersState::empty(), cursor_pos: PhysicalPosition::new(0.0, 0.0), error: None, start: Instant::now(), frames: 0, trace: std::env::var_os("RENDRIX_TRACE_EVENTS").is_some(), dump: std::env::var_os("RENDRIX_DUMP_FRAME").map(|p| (std::path::PathBuf::from(p), Instant::now() - Duration::from_secs(1))) };
    event_loop.run_app(&mut app).map_err(|e| format!("event loop: {e}"))?;
    match app.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn mods_of(m: ModifiersState) -> Mods {
    Mods { ctrl: m.control_key(), shift: m.shift_key(), alt: m.alt_key(), meta: m.super_key() }
}

fn dom_key(key: &Key) -> String {
    match key {
        Key::Character(s) => s.to_string(),
        Key::Named(n) => match n {
            NamedKey::Space => " ".into(),
            NamedKey::Control => "Control".into(),
            NamedKey::Super => "Meta".into(),
            other => format!("{other:?}"),
        },
        Key::Dead(_) => "Dead".into(),
        Key::Unidentified(_) => "Unidentified".into(),
    }
}

fn translate_key(ev: &KeyEvent, mods: ModifiersState) -> KeyInput {
    let code = match ev.physical_key {
        PhysicalKey::Code(c) => format!("{c:?}"),
        PhysicalKey::Unidentified(_) => "Unidentified".into(),
    };
    KeyInput { key: dom_key(&ev.logical_key), code, text: ev.text.as_ref().map(|t| t.to_string()), repeat: ev.repeat, mods: mods_of(mods) }
}

fn cursor_icon(c: Cursor) -> CursorIcon {
    match c {
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::Move => CursorIcon::Move,
        Cursor::NotAllowed => CursorIcon::NotAllowed,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::Crosshair => CursorIcon::Crosshair,
        Cursor::Wait => CursorIcon::Wait,
        Cursor::Help => CursorIcon::Help,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::Auto | Cursor::Default => CursorIcon::Default,
    }
}

impl App {
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attrs = WindowAttributes::default().with_title(self.bundle.window_title()).with_inner_size(LogicalSize::new(self.bundle.width as f64, self.bundle.height as f64));
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| format!("create window: {e}"))?);
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::PRIMARY, ..Default::default() });
        let surface = instance.create_surface(window.clone()).map_err(|e| format!("create surface: {e}"))?;
        let ctx = GpuContext::new(Some(&surface), Some(instance))?;
        let caps = surface.get_capabilities(&ctx.adapter);
        // Blend in sRGB space like CSS does: use a non-sRGB surface format.
        let format = caps.formats.iter().find(|f| !f.is_srgb()).copied().unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let alpha_mode = caps.alpha_modes.iter().copied().find(|m| *m == wgpu::CompositeAlphaMode::Opaque).unwrap_or(caps.alpha_modes[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&ctx.device, &config);
        let renderer = Renderer::new(ctx.device, ctx.queue, format);
        let scale = window.scale_factor();
        let (lw, lh) = (size.width as f64 / scale, size.height as f64 / scale);
        eprintln!("[rendrix] adapter: {}  surface: {:?}  scale: {scale}", {
            let i = ctx.adapter.get_info();
            format!("{} ({:?})", i.name, i.backend)
        }, format);
        let session = Session::new(&self.bundle, Box::new(SystemClipboard::new()), (lw as f32, lh as f32, scale as f32))?;
        window.set_ime_allowed(true);
        self.gpu = Some(Gpu { window, surface, config, renderer });
        self.session = Some(session);
        Ok(())
    }

    fn logical(&self, p: PhysicalPosition<f64>) -> (f32, f32) {
        let s = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
        ((p.x / s) as f32, (p.y / s) as f32)
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(gpu), Some(session)) = (self.gpu.as_mut(), self.session.as_mut()) else { return };
        let info = session.frame();
        if info.quit {
            event_loop.exit();
            return;
        }
        if let Some(t) = &info.title {
            gpu.window.set_title(t);
        }
        if let Some((w, h)) = info.resize {
            let _ = gpu.window.request_inner_size(LogicalSize::new(w as f64, h as f64));
        }
        if let Some(c) = info.cursor {
            gpu.window.set_cursor(cursor_icon(c));
        }
        if let Some(r) = session.caret_rect() {
            gpu.window.set_ime_cursor_area(LogicalPosition::new(r[0] as f64, r[1] as f64), LogicalSize::new(r[2] as f64, r[3] as f64));
        }
        let size = gpu.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if info.repainted || self.frames == 0 {
            let frame = match gpu.surface.get_current_texture() {
                Ok(f) => f,
                Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                    gpu.surface.configure(gpu.renderer.device(), &gpu.config);
                    gpu.window.request_redraw();
                    return;
                }
                Err(wgpu::SurfaceError::Timeout) => return,
                Err(e) => {
                    self.error = Some(format!("surface: {e}"));
                    event_loop.exit();
                    return;
                }
            };
            let view = frame.texture.create_view(&Default::default());
            {
                let engine = session.engine.borrow();
                for img in engine.take_image_uploads() {
                    gpu.renderer.upload_image_as(img.id, &img.rgba, img.width, img.height);
                }
                let mut text = engine.text_borrow();
                gpu.renderer.render(engine.display_list(), &mut text, &view, (size.width, size.height), gpu.window.scale_factor() as f32);
            }
            gpu.window.pre_present_notify();
            frame.present();
            self.frames += 1;
            if let Some((path, last)) = self.dump.as_mut() {
                if last.elapsed() > Duration::from_millis(150) {
                    *last = Instant::now();
                    let engine = session.engine.borrow();
                    let mut text = engine.text_borrow();
                    let scale = gpu.window.scale_factor() as f32;
                    let rgba = gpu.renderer.render_to_rgba(engine.display_list(), &mut text, (size.width, size.height), scale);
                    let _ = image::save_buffer(&*path, &rgba, size.width, size.height, image::ColorType::Rgba8);
                }
            }
        }
        if info.continuous {
            gpu.window.request_redraw();
        } else if let Some(w) = info.next_wake_s {
            let now = session.now_s();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_secs_f64((w - now).max(0.0))));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        if let Err(e) = self.init(event_loop) {
            self.error = Some(e);
            event_loop.exit();
            return;
        }
        if let Some(g) = &self.gpu {
            g.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(gpu), Some(session)) = (self.gpu.as_ref(), self.session.as_mut()) else { return };
        let next = session.pump();
        let tick = session.js.tick_info();
        if session.engine.borrow().needs_update() || tick.wants_frame {
            gpu.window.request_redraw();
        } else if tick.pending_io {
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
        } else if let Some(t) = next {
            let now = session.now_s();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_secs_f64((t - now).max(0.0))));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.trace && !matches!(event, WindowEvent::RedrawRequested) {
            eprintln!("[event] {event:?}");
        }
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::RedrawRequested => {
                self.redraw(event_loop);
                return;
            }
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(session)) = (self.gpu.as_mut(), self.session.as_mut()) {
                    if size.width > 0 && size.height > 0 {
                        gpu.config.width = size.width;
                        gpu.config.height = size.height;
                        gpu.surface.configure(gpu.renderer.device(), &gpu.config);
                        let s = gpu.window.scale_factor();
                        session.resize((size.width as f64 / s) as f32, (size.height as f64 / s) as f32, s as f32);
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let (Some(gpu), Some(session)) = (self.gpu.as_ref(), self.session.as_mut()) {
                    let size = gpu.window.inner_size();
                    session.resize((size.width as f64 / scale_factor) as f32, (size.height as f64 / scale_factor) as f32, scale_factor as f32);
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                if let Some(s) = self.session.as_mut() {
                    s.set_mods(mods_of(self.mods));
                }
            }
            WindowEvent::Focused(f) => {
                if let Some(s) = self.session.as_mut() {
                    s.set_window_focused(f);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = position;
                let (x, y) = self.logical(position);
                if let Some(s) = self.session.as_mut() {
                    s.pointer_move(x, y);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(s) = self.session.as_mut() {
                    s.pointer_move(-1.0, -1.0);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(n) => n as u32,
                };
                let (x, y) = self.logical(self.cursor_pos);
                if let Some(s) = self.session.as_mut() {
                    match state {
                        ElementState::Pressed => s.pointer_down(x, y, b),
                        ElementState::Released => s.pointer_up(x, y, b),
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.logical(self.cursor_pos);
                let scale = self.gpu.as_ref().map(|g| g.window.scale_factor()).unwrap_or(1.0);
                // winit: positive y scrolls content down (wheel up); the DOM's deltaY is the opposite.
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => (-dx * 48.0, -dy * 48.0),
                    MouseScrollDelta::PixelDelta(p) => ((-p.x / scale) as f32, (-p.y / scale) as f32),
                };
                if let Some(s) = self.session.as_mut() {
                    s.wheel(x, y, dx, dy);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let k = translate_key(&event, self.mods);
                if let Some(s) = self.session.as_mut() {
                    match event.state {
                        ElementState::Pressed => s.key_down(k),
                        ElementState::Released => s.key_up(k),
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(s) = self.session.as_mut() {
                    s.text_commit(&text);
                }
            }
            WindowEvent::DroppedFile(_) | WindowEvent::HoveredFile(_) => {}
            _ => return,
        }
        if let Some(g) = &self.gpu {
            g.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        let _ = self.start;
    }
}

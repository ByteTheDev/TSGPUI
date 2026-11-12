//! winit event loop + wgpu device/swapchain + frame pacing + DPI. Owner: agent A.
//!
//! Contract (fixed): `pub fn run(doc: &crate::package::AppPackage) -> i32` drives the whole
//! app — creates `backend::App`, opens a window titled `doc.title` at `doc.width x doc.height`,
//! pumps winit events (pointer/key through `input::*`, resize), asks `App::frame` for a
//! `PaintFrame` every vsync (redraw on demand at minimum, continuous is fine for M0), feeds it
//! through `paint::Painter`, and returns 0 on clean exit.
//!
//! All widget coordinates in the rest of the renderer are logical pixels; the surface layer is
//! the only place that knows about device pixels, so it owns the scale factor and divides
//! physical events before they leave this module.

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes};

use crate::gpu::{backend, input, paint};

type SharedWindow = std::sync::Arc<Window>;

/// Async GPU init (instance/adapter/device) is the one blocking call at startup; pollster is the
/// standard way to drain it on a thread that has no async runtime. The surface keeps an owned
/// refcount on the window so `'static` is satisfied.
fn init_gpu(
    window: &SharedWindow,
    instance: &wgpu::Instance,
) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError> {
    instance.create_surface(window.clone())
}

/// Configures the swapchain to the given physical size and returns the format it picked, so
/// pipelines are built against exactly the texture the swapchain presents.
fn configure(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> wgpu::TextureFormat {
    // Zero-sized surfaces are illegal in wgpu (minimized windows report 0x0); clamp instead of
    // skipping so a configure always lands — the frame path guards zero size separately.
    let (width, height) = (width.max(1), height.max(1));
    let caps = surface.get_capabilities(adapter);
    // sRGB avoids double gamma-correcting our linear-space fills; every backend offers one.
    let format = caps
        .formats
        .iter()
        .find(|f| f.is_srgb())
        .copied()
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        // Fifo is the only mode guaranteed everywhere and it maps to vsync, which is what the
        // "every frame" pacing we want actually costs.
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(device, &config);
    format
}

struct WindowState {
    window: SharedWindow,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    painter: paint::Painter,
    /// Device pixels per logical pixel (from winit, always re-read after ScaleFactorChanged).
    scale: f64,
    /// Physical surface size, kept in sync with the last Resized/configure.
    size: (u32, u32),
    /// The configured surface format, for debug render targets that must match the pipelines.
    format: wgpu::TextureFormat,
    /// Last known cursor position in physical pixels. MouseInput carries no position, so
    /// buttons are dispatched against this.
    cursor: (f64, f64),
    debug_pixels_done: bool,
}

impl WindowState {
    fn scale_f32(&self) -> f32 {
        self.scale as f32
    }

    /// Logical (f64) coordinates of a physical position under the current scale.
    fn logical(&self, position: winit::dpi::PhysicalPosition<f64>) -> (f32, f32) {
        (
            (position.x / self.scale) as f32,
            (position.y / self.scale) as f32,
        )
    }

    fn redraw_frame(&mut self, app: &mut backend::App) {
        let (width, height) = self.size;
        if width == 0 || height == 0 {
            return; // minimized; nothing to present
        }
        // Full repaint every frame: M0 does not diff frames, the backend rebuilds the draw
        // list each time and the painter uploads it wholesale.
        let frame = app.frame(self.scale_f32(), (width, height));
        self.painter.set_glyph_atlas(app.atlas());

        // Debug: render the same frame into a copyable texture and dump raw pixels, so the
        // live GPU output can be compared with the window contents directly.
        if std::env::var_os("TSGPU_DEBUG_PIXELS").is_some() && !self.debug_pixels_done {
            self.debug_pixels_done = true;
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tsgpu live debug"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tsgpu live debug") });
            self.painter.paint(&self.device, &mut encoder, &view, &frame);
            self.queue.submit(Some(encoder.finish()));
            let padded = (width * 4 + 255) / 256 * 256;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tsgpu live readback"),
                size: (padded * height) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tsgpu live readback") });
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
                },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
            self.device.poll(wgpu::PollType::Wait).ok();
            if rx.recv().is_ok() {
                let data = slice.get_mapped_range();
                let mut out = Vec::with_capacity((width * height * 4) as usize);
                for row in 0..height {
                    let start = (row * padded) as usize;
                    out.extend_from_slice(&data[start..start + (width * 4) as usize]);
                }
                let _ = std::fs::write(std::env::temp_dir().join("tsgpu-live-pixels.bin"), out);
            }
            readback.unmap();
        }

        let texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                // Swapchain stale after resize; a reconfigure happens in Resized, try next frame.
                self.window.request_redraw();
                return;
            }
            Err(err) => {
                eprintln!("tsgpui-gpu-host: acquire surface failed: {err}");
                return;
            }
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tsgpu frame"),
        });
        self.painter.paint(&self.device, &mut encoder, &view, &frame);
        self.queue.submit([encoder.finish()]);
        texture.present();
    }
}

struct GpuApp {
    /// Taken in `resumed` to build the backend app without cloning the document tree again.
    doc: Option<crate::package::AppPackage>,
    /// Built up-front so the event loop owns the full window spec even before `resumed` runs.
    attrs: WindowAttributes,
    app: Option<backend::App>,
    state: Option<WindowState>,
    /// Set if startup failed; turns a clean event-loop exit into a failure exit code.
    error: Option<String>,
}

impl GpuApp {
    fn build_window_state(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Result<WindowState, String> {
        let window = event_loop
            .create_window(self.attrs.clone())
            .map_err(|err| format!("create window: {err}"))?;
        let window: SharedWindow = std::sync::Arc::new(window);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface =
            init_gpu(&window, &instance).map_err(|err| format!("create surface: {err}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|err| format!("request adapter: {err}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tsgpu device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| format!("request device: {err}"))?;

        let size = window.inner_size();
        let format = configure(&surface, &device, &adapter, size.width, size.height);

        let painter = paint::Painter::new(&device, &queue, format);
        let scale = window.scale_factor();
        Ok(WindowState {
            window,
            surface,
            adapter,
            device,
            queue,
            painter,
            scale,
            size: (size.width, size.height),
            cursor: (0.0, 0.0),
            debug_pixels_done: false,
            format,
        })
    }
}

impl ApplicationHandler for GpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // `resumed` can fire more than once (mobile lifecycle); init only on the first pass.
        if self.state.is_some() {
            return;
        }
        match self.build_window_state(event_loop) {
            Ok(state) => {
                let doc = self.doc.take().expect("resumed ran twice");
                self.app = Some(backend::App::new(doc));
                self.state = Some(state);
            }
            Err(err) => {
                self.error = Some(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let (Some(state), Some(app)) = (self.state.as_mut(), self.app.as_mut()) else {
            return;
        };
        if window_id != state.window.id() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                state.size = (size.width, size.height);
                configure(
                    &state.surface,
                    &state.device,
                    &state.adapter,
                    size.width,
                    size.height,
                );
                state.painter.resize(size.width, size.height, state.scale_f32());
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // The inner-size change that always follows reconfigures the surface; updating
                // scale here means the intervening events already convert correctly.
                state.scale = scale_factor;
                state.painter.resize(state.size.0, state.size.1, state.scale_f32());
            }
            WindowEvent::RedrawRequested => {
                state.redraw_frame(app);
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.cursor = (position.x, position.y);
                let (x, y) = state.logical(position);
                app.pointer(input::PointerEvent::Move { x, y });
            }
            WindowEvent::MouseInput { state: button_state, button, .. } => {
                if button != MouseButton::Left {
                    return;
                }
                let (x, y) = (state.cursor.0, state.cursor.1);
                let (x, y) = ((x / state.scale) as f32, (y / state.scale) as f32);
                let event = match button_state {
                    ElementState::Pressed => input::PointerEvent::Down { x, y },
                    ElementState::Released => input::PointerEvent::Up { x, y },
                };
                app.pointer(event);
            }
            WindowEvent::KeyboardInput { event: key, is_synthetic: false, .. } => {
                // Only presses drive the widget state machines; releases have no meaning yet.
                if key.state != ElementState::Pressed {
                    return;
                }
                if let Some(key_event) = map_key(&key.logical_key) {
                    app.key(key_event);
                }
            }
            // IME commit is how real typed characters reach us on IME platforms and on some
            // layouts that bypass physical keys; the contract takes single chars, so a commit
            // becomes one event per character.
            WindowEvent::Ime(Ime::Commit(text)) => {
                for c in text.chars() {
                    app.key(input::KeyEvent::Character(c));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // M0 paces continuously: request another redraw every time the loop drains. This keeps
        // the frame loop independent of input cadence; on-demand-only redraw arrives with M1
        // when the backend can report dirty state.
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

/// winit logical key → renderer key event. Physical-key matching would break non-QWERTY
/// layouts; `logical_key` is the user-visible meaning of the press.
fn map_key(key: &Key) -> Option<input::KeyEvent> {
    match key {
        Key::Named(NamedKey::Backspace) => Some(input::KeyEvent::Backspace),
        Key::Named(NamedKey::Enter) => Some(input::KeyEvent::Enter),
        Key::Named(NamedKey::Tab) => Some(input::KeyEvent::Tab),
        Key::Named(NamedKey::ArrowLeft) => Some(input::KeyEvent::ArrowLeft),
        Key::Named(NamedKey::ArrowRight) => Some(input::KeyEvent::ArrowRight),
        Key::Named(NamedKey::ArrowUp) => Some(input::KeyEvent::ArrowUp),
        Key::Named(NamedKey::ArrowDown) => Some(input::KeyEvent::ArrowDown),
        Key::Character(text) => text.chars().next().map(input::KeyEvent::Character),
        _ => None,
    }
}

pub fn run(doc: &crate::package::AppPackage) -> i32 {
    let attrs = Window::default_attributes()
        .with_title(doc.title.clone())
        .with_inner_size(LogicalSize::new(doc.width as f64, doc.height as f64))
        .with_resizable(true)
        // "system" keeps the OS chrome; any other titlebar style asks for a frameless window
        // the app decorates itself.
        .with_decorations(doc.window.titlebar == "system");

    let event_loop = match EventLoop::new() {
        Ok(loop_) => loop_,
        Err(err) => {
            eprintln!("tsgpui-gpu-host: event loop: {err}");
            return 1;
        }
    };

    let mut app = GpuApp {
        doc: Some(doc.clone()),
        attrs,
        app: None,
        state: None,
        error: None,
    };
    match event_loop.run_app(&mut app) {
        Ok(()) => i32::from(app.error.is_some()),
        Err(err) => {
            eprintln!("tsgpui-gpu-host: event loop: {err}");
            1
        }
    }
}

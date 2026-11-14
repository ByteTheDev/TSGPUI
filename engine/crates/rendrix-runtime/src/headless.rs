//! Headless driver: runs an application against an offscreen GPU target, optionally replaying a
//! scripted interaction. This exercises the same `Session` input path as the window driver, so it
//! doubles as the end-to-end test harness.

use crate::bundle::AppBundle;
use crate::session::{KeyInput, MemoryClipboard, Mods, Session};
use rendrix_gfx::{GpuContext, Renderer};
use serde_json::Value;
use std::time::{Duration, Instant};

pub struct Headless {
    pub session: Session,
    renderer: Renderer,
    pub size: (u32, u32),
    pub scale: f32,
    pub adapter: String,
}

pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Shot {
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        image::save_buffer(path, &self.rgba, self.width, self.height, image::ColorType::Rgba8).map_err(|e| e.to_string())
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

impl Headless {
    /// `size` is in logical pixels; the target is `size * scale` physical pixels.
    pub fn new(bundle: &AppBundle, size: (u32, u32), scale: f32) -> Result<Headless, String> {
        let ctx = GpuContext::headless()?;
        let adapter = ctx.adapter_name();
        let renderer = Renderer::new(ctx.device, ctx.queue, wgpu::TextureFormat::Rgba8Unorm);
        let session = Session::new(bundle, Box::new(MemoryClipboard::default()), (size.0 as f32, size.1 as f32, scale))?;
        Ok(Headless { session, renderer, size, scale, adapter })
    }

    /// Run frames until the application is idle (no animation, no pending frame request) or `max` elapses.
    pub fn settle(&mut self, max: Duration) {
        let end = Instant::now() + max;
        loop {
            let info = self.session.frame();
            if !info.continuous && !info.repainted {
                let wake = info.next_wake_s.map(|w| w - self.session.now_s());
                match wake {
                    Some(w) if w > 0.0 && w < 0.05 && Instant::now() < end => std::thread::sleep(Duration::from_secs_f64(w.max(0.001))),
                    _ => break,
                }
            } else if info.continuous {
                std::thread::sleep(Duration::from_millis(4));
            }
            if Instant::now() >= end {
                break;
            }
        }
    }

    /// Advance real time by `ms`, running frames as a window would.
    pub fn wait(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.session.frame();
            std::thread::sleep(Duration::from_millis(4));
        }
        self.session.frame();
    }

    /// Render the current frame and read it back.
    pub fn screenshot(&mut self) -> Shot {
        self.session.frame();
        let (w, h) = ((self.size.0 as f32 * self.scale).round() as u32, (self.size.1 as f32 * self.scale).round() as u32);
        let engine = self.session.engine.borrow();
        for img in engine.take_image_uploads() {
            self.renderer.upload_image_as(img.id, &img.rgba, img.width, img.height);
        }
        let mut text = engine.text_borrow();
        let rgba = self.renderer.render_to_rgba(engine.display_list(), &mut text, (w, h), self.scale);
        Shot { width: w, height: h, rgba }
    }

    /// Render the current display list into an offscreen target and wait for the GPU; returns the elapsed time.
    pub fn render_timed(&mut self) -> Duration {
        let (w, h) = ((self.size.0 as f32 * self.scale).round() as u32, (self.size.1 as f32 * self.scale).round() as u32);
        let t = Instant::now();
        let view = self.target_view(w, h);
        let engine = self.session.engine.borrow();
        for img in engine.take_image_uploads() {
            self.renderer.upload_image_as(img.id, &img.rgba, img.width, img.height);
        }
        let mut text = engine.text_borrow();
        self.renderer.render(engine.display_list(), &mut text, &view, (w, h), self.scale);
        let _ = self.renderer.device().poll(wgpu::PollType::Wait);
        t.elapsed()
    }

    fn target_view(&mut self, w: u32, h: u32) -> wgpu::TextureView {
        let tex = self.renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("rendrix bench target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        tex.create_view(&Default::default())
    }

    /// (instances, batches) of the last GPU frame.
    pub fn render_stats(&self) -> (usize, usize) {
        (self.renderer.last_instances, self.renderer.last_batches)
    }

    pub fn click(&mut self, x: f32, y: f32) {
        self.session.pointer_move(x, y);
        self.session.pointer_down(x, y, 0);
        self.session.pointer_up(x, y, 0);
    }

    pub fn click_selector(&mut self, selector: &str) -> Result<(), String> {
        self.session.frame();
        let id = self.session.query(selector).ok_or_else(|| format!("no element matches '{selector}'"))?;
        let (x, y) = self.session.center_of(id).ok_or_else(|| format!("'{selector}' has no box"))?;
        self.click(x, y);
        Ok(())
    }

    pub fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            let s = ch.to_string();
            let code = if ch.is_ascii_alphabetic() { format!("Key{}", ch.to_ascii_uppercase()) } else { String::new() };
            let k = KeyInput { key: s.clone(), code, text: Some(s), repeat: false, mods: Mods::default() };
            self.session.key_down(k.clone());
            self.session.key_up(k);
        }
    }

    pub fn press(&mut self, key: &str, mods: Mods) {
        let k = KeyInput { key: key.to_string(), code: key.to_string(), text: None, repeat: false, mods };
        self.session.key_down(k.clone());
        self.session.key_up(k);
    }

    /// Evaluate a JavaScript expression and render its value as text.
    pub fn eval(&mut self, js: &str) -> Result<String, String> {
        let wrapped = format!("(()=>{{ const v = ({js}); return typeof v === 'string' ? v : JSON.stringify(v); }})()");
        match self.session.js.eval(&wrapped) {
            Ok(rendrix_js::Val::Str(s)) => Ok(s),
            Ok(other) => Ok(format!("{other:?}")),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Replay a list of steps. Returns the number of steps run; the first failure is an error.
    pub fn run_steps(&mut self, steps: &[Value], out_dir: &std::path::Path) -> Result<usize, String> {
        for (i, step) in steps.iter().enumerate() {
            let kind = step["type"].as_str().unwrap_or("");
            let fail = |m: String| format!("step {} ({kind}): {m}", i + 1);
            match kind {
                "wait" => self.wait(step["ms"].as_u64().unwrap_or(50)),
                "settle" => self.settle(Duration::from_millis(step["ms"].as_u64().unwrap_or(2000))),
                "move" => {
                    let (x, y) = self.target(step).map_err(&fail)?;
                    self.session.pointer_move(x, y);
                }
                "click" => {
                    if let Some(sel) = step["selector"].as_str() {
                        self.click_selector(sel).map_err(&fail)?;
                    } else {
                        self.click(step["x"].as_f64().unwrap_or(0.0) as f32, step["y"].as_f64().unwrap_or(0.0) as f32);
                    }
                }
                "type" => self.type_text(step["text"].as_str().unwrap_or("")),
                "key" => {
                    let m = Mods { ctrl: step["ctrl"].as_bool().unwrap_or(false), shift: step["shift"].as_bool().unwrap_or(false), alt: step["alt"].as_bool().unwrap_or(false), meta: false };
                    self.press(step["key"].as_str().unwrap_or(""), m);
                }
                "wheel" => {
                    let (x, y) = self.target(step).unwrap_or((10.0, 10.0));
                    self.session.wheel(x, y, step["dx"].as_f64().unwrap_or(0.0) as f32, step["dy"].as_f64().unwrap_or(0.0) as f32);
                }
                "resize" => {
                    self.size = (step["width"].as_u64().unwrap_or(self.size.0 as u64) as u32, step["height"].as_u64().unwrap_or(self.size.1 as u64) as u32);
                    self.session.resize(self.size.0 as f32, self.size.1 as f32, self.scale);
                }
                "eval" => {
                    let v = self.eval(step["js"].as_str().unwrap_or("undefined")).map_err(&fail)?;
                    println!("[eval] {v}");
                }
                "expect" => {
                    self.session.frame();
                    let js = step["js"].as_str().unwrap_or("undefined");
                    let got = self.eval(js).map_err(&fail)?;
                    let want = step["equals"].as_str().map(str::to_string).unwrap_or_else(|| step["equals"].to_string());
                    if got != want {
                        return Err(fail(format!("`{js}` was {got}, expected {want}")));
                    }
                    println!("[expect] ok: {js} == {want}");
                }
                "screenshot" => {
                    let path = out_dir.join(step["path"].as_str().unwrap_or("screenshot.png"));
                    self.screenshot().save_png(&path).map_err(&fail)?;
                    println!("[screenshot] {}", path.display());
                }
                other => return Err(fail(format!("unknown step type '{other}'"))),
            }
        }
        Ok(steps.len())
    }

    fn target(&mut self, step: &Value) -> Result<(f32, f32), String> {
        if let Some(sel) = step["selector"].as_str() {
            self.session.frame();
            let id = self.session.query(sel).ok_or_else(|| format!("no element matches '{sel}'"))?;
            return self.session.center_of(id).ok_or_else(|| format!("'{sel}' has no box"));
        }
        Ok((step["x"].as_f64().unwrap_or(0.0) as f32, step["y"].as_f64().unwrap_or(0.0) as f32))
    }
}

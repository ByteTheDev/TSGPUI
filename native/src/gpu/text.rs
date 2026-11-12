//! cosmic-text shaping, glyph atlas rasterization, measurement, caret/selection geometry.
//! Owner: agent C.
//!
//! Contract (fixed):
//! - `TextSystem::new() -> Self`
//! - `TextSystem::measure_run(&mut self, run: &types::TextRun) -> (f32, f32)` — wrapped width/height
//! - `TextSystem::shape(&mut self, frame: &mut types::PaintFrame)` — replaces every
//!   `DrawOp::Text` with a `DrawOp::Glyphs` batch (rasterized into the internal atlas)
//! - `TextSystem::atlas(&self) -> &types::GlyphAtlas` — painter uploads when generation changes
//!
//! M0 atlas design (perf TODO at the bottom): every `shape()` re-rasterizes all glyphs into a
//! fresh shelf-packed RGBA8 atlas and bumps `generation`, so `paint` re-uploads it each frame.
//! Simple and always correct; a persistent cache comes with the M1 scroll work.
//!
//! Resolution rules (agreed with the paint owner):
//! - Glyphs rasterize at DEVICE resolution: 1 atlas texel = 1 device pixel, driven by
//!   `frame.scale`. `GlyphInstance.uv`/`size` are atlas texels (device px); `GlyphInstance.pos`
//!   stays in LOGICAL px (paint multiplies it by `frame.scale`).
//! - Coverage lives in the atlas ALPHA channel; RGB is white (paint samples `.a` only).
//! - Atlas `rgba` length is exactly `width * height * 4` bytes. Atlas widths start at 512 and
//!   double, so the row stride (`width * 4`) is always a multiple of 256 — the paint-side
//!   fast-path copy for 256-byte-aligned rows applies.
//! - `caret`/`hit_text` arrive with text editing in M2.

use crate::gpu::types::{Color, DrawOp, GlyphAtlas, GlyphBatch, GlyphInstance, PaintFrame, Rect, TextAlign, TextRun};
use cosmic_text::{Attrs, Buffer, CacheKey, FontSystem, Metrics, Shaping, SwashCache, SwashContent as GlyphContent, SwashImage, Weight};

/// Atlas geometry: start at 512×512 and double up to the 4096 cap. Doubling from 512 keeps the
/// width a multiple of 64, i.e. a 256-byte-aligned row stride (see module docs).
const INITIAL_ATLAS: u32 = 512;
const MAX_ATLAS: u32 = 4096;
/// Line height multiplier applied to every run (matches the layout module's default).
const LINE_HEIGHT_EM: f32 = 1.25;

/// One glyph waiting for atlas space, with everything `GlyphInstance` needs except the UV.
struct GlyphJob {
    cache_key: CacheKey,
    /// Logical-pixel pen (origin) position of the glyph.
    pen: (f32, f32),
    color: Color,
    clip: Vec<Rect>,
}

pub struct TextSystem {
    fonts: FontSystem,
    swash: SwashCache,
    atlas: GlyphAtlas,
    /// Monotonic counter; each atlas change gets the next value (`atlas.generation`).
    generation: u64,
    /// Scratch buffer reused for shaping/measuring runs (re-populated per run).
    scratch: Buffer,
}

impl TextSystem {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            swash: SwashCache::new(),
            atlas: GlyphAtlas { width: INITIAL_ATLAS, height: INITIAL_ATLAS, rgba: Vec::new(), generation: 0 },
            // Starts at 1 so the first committed atlas gets generation 1 — the painter's
            // default is 0 ("nothing uploaded yet"), which must not match.
            generation: 1,
            scratch: Buffer::new_empty(Metrics::new(16.0, 16.0 * LINE_HEIGHT_EM)),
        }
    }

    /// Wrapped width/height of `run` in logical pixels (shaped, never rasterized).
    pub fn measure_run(&mut self, run: &TextRun) -> (f32, f32) {
        if run.text.trim().is_empty() {
            return (0.0, 0.0);
        }
        // Measurement is logical; no raster scale involved.
        let mut sink = Vec::new();
        self.shape_run(run, 1.0, &mut sink)
    }

    /// Shape and rasterize every `DrawOp::Text` in the frame, replacing it in place with a
    /// `DrawOp::Glyphs` batch (same position, so paint order is preserved).
    pub fn shape(&mut self, frame: &mut PaintFrame) {
        let scale = if frame.scale.is_finite() && frame.scale > 0.0 { frame.scale } else { 1.0 };
        let ops = std::mem::take(&mut frame.ops);

        let mut jobs: Vec<GlyphJob> = Vec::new();
        // One range of `jobs` per Text op, in frame order; non-text ops pass through untouched.
        let mut slots: Vec<Option<DrawOp>> = Vec::with_capacity(ops.len());
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for op in ops {
            match op {
                DrawOp::Text(run) => {
                    let start = jobs.len();
                    self.shape_run(&run, scale, &mut jobs);
                    ranges.push((start, jobs.len()));
                    slots.push(None);
                }
                other => slots.push(Some(other)),
            }
        }

        let placed = self.rasterize(scale, &jobs);
        let mut ranges = ranges.into_iter();
        for slot in slots.iter_mut() {
            if slot.is_none() {
                let (start, end) = ranges.next().expect("a range for every Text slot");
                let batch = GlyphBatch { instances: placed[start..end].iter().flatten().cloned().collect() };
                *slot = Some(DrawOp::Glyphs(batch));
            }
        }
        frame.ops = slots.into_iter().flatten().collect();
    }

    pub fn atlas(&self) -> &GlyphAtlas {
        &self.atlas
    }

    /// Shape `run` into the scratch buffer and append one job per glyph (beyond wrapping and
    /// alignment). Returns the wrapped (max line) width and total height in logical px.
    fn shape_run(&mut self, run: &TextRun, scale: f32, jobs: &mut Vec<GlyphJob>) -> (f32, f32) {
        if run.text.trim().is_empty() {
            return (0.0, 0.0);
        }
        let metrics = Metrics::new(run.size, run.size * LINE_HEIGHT_EM);
        let width_opt = (run.max_width > 0.0 && run.max_width.is_finite()).then_some(run.max_width);
        self.scratch.set_text(&mut self.fonts, &run.text, &attrs_for(run), Shaping::Advanced);
        self.scratch.set_metrics_and_size(&mut self.fonts, metrics, width_opt, None);
        self.scratch.shape_until_scroll(&mut self.fonts, true);

        let mut max_line_w = 0.0f32;
        let mut total_h = 0.0f32;
        // `run.y` is the baseline of the first line; subsequent lines step by the line height.
        let mut line_y = run.y;
        for line in self.scratch.layout_runs() {
            let pen_x = run.x + align_offset(line.line_w, run.max_width, run.align);
            for glyph in line.glyphs {
                // Rasterize at device resolution: positions and font size scale by `scale`.
                let physical = glyph.physical((pen_x * scale, line_y * scale), scale);
                jobs.push(GlyphJob {
                    cache_key: physical.cache_key,
                    pen: (pen_x, line_y),
                    color: run.color,
                    clip: run.clip.clone(),
                });
            }
            max_line_w = max_line_w.max(line.line_w);
            total_h += line.line_height;
            line_y += line.line_height;
        }
        (max_line_w, total_h)
    }

    /// Shelf-pack every job's glyph into a fresh atlas, doubling dimensions until everything
    /// fits (or the cap is reached and leftover glyphs are dropped — TODO(M1): overflow page).
    /// Returns one entry per job (None for dropped/empty glyphs).
    fn rasterize(&mut self, scale: f32, jobs: &[GlyphJob]) -> Vec<Option<GlyphInstance>> {
        let mut width = INITIAL_ATLAS;
        let mut height = INITIAL_ATLAS;
        loop {
            // Exact w·h·4 bytes — the painter keeps the old atlas otherwise.
            let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
            let mut cursor = (0u32, 0u32);
            let mut shelf_h = 0u32;
            let mut placed: Vec<Option<GlyphInstance>> = Vec::with_capacity(jobs.len());
            let mut overflow = false;

            for job in jobs {
                let image = self.swash.get_image(&mut self.fonts, job.cache_key);
                let Some(img) = image else {
                    placed.push(None);
                    continue;
                };
                let (w, h) = (img.placement.width, img.placement.height);
                if w == 0 || h == 0 {
                    placed.push(None);
                    continue;
                }
                if cursor.0 + w > width {
                    cursor.0 = 0;
                    cursor.1 += shelf_h;
                    shelf_h = 0;
                }
                if cursor.1 + h > height {
                    overflow = true;
                    break;
                }
                blit(&mut rgba, width, cursor, img);
                // Image placement offsets are device px; convert to logical for `pos`.
                placed.push(Some(GlyphInstance {
                    pos: [job.pen.0 + img.placement.left as f32 / scale, job.pen.1 - img.placement.top as f32 / scale],
                    uv: [cursor.0 as f32, cursor.1 as f32],
                    size: [w as f32, h as f32],
                    color: job.color,
                    clip: job.clip.clone(),
                }));
                cursor.0 += w;
                shelf_h = shelf_h.max(h);
            }

            if !overflow || width >= MAX_ATLAS {
                if placed.iter().any(|p| p.is_some()) {
                    // The atlas was rebuilt from scratch: its bytes changed this frame.
                    self.atlas = GlyphAtlas { width, height, rgba, generation: self.generation };
                    self.generation += 1;
                }
                placed.resize(jobs.len(), None);
                return placed;
            }
            width = (width * 2).min(MAX_ATLAS);
            height = (height * 2).min(MAX_ATLAS);
        }
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Blit one swash glyph image (alpha coverage, or color for emoji) into the atlas. Coverage
/// goes into the ALPHA channel with white RGB; color pixels are copied straight through.
fn blit(atlas: &mut [u8], atlas_width: u32, at: (u32, u32), img: &SwashImage) {
    let (w, h) = (img.placement.width, img.placement.height);
    let (ox, oy) = (at.0 as usize, at.1 as usize);
    let row = atlas_width as usize * 4;
    match img.content {
        GlyphContent::Mask => {
            // One coverage byte per pixel (straight alpha).
            let mut i = 0;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let a = img.data[i];
                    let p = (oy + y) * row + (ox + x) * 4;
                    atlas[p] = 255;
                    atlas[p + 1] = 255;
                    atlas[p + 2] = 255;
                    atlas[p + 3] = a;
                    i += 1;
                }
            }
        }
        GlyphContent::Color => {
            // Straight RGBA, one pixel at a time.
            let mut i = 0;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let p = (oy + y) * row + (ox + x) * 4;
                    atlas[p..p + 4].copy_from_slice(&img.data[i..i + 4]);
                    i += 4;
                }
            }
        }
        // BgRgba / SubpixelMask never occur for our render settings; skip the glyph.
        _ => {}
    }
}

fn attrs_for(run: &TextRun) -> Attrs<'static> {
    // Sans-serif system fallback for M0; bundled custom fonts arrive with font-family (M2).
    Attrs::new().weight(Weight(run.weight))
}

/// Where a line of width `line_w` starts inside a `max_width` box for the run's alignment.
/// TODO(M2): RTL paragraphs anchor on the right edge instead.
fn align_offset(line_w: f32, max_width: f32, align: TextAlign) -> f32 {
    match align {
        TextAlign::Center => ((max_width - line_w) / 2.0).max(0.0),
        TextAlign::Right => (max_width - line_w).max(0.0),
        TextAlign::Left => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::types::{Color, PaintFrame};

    const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

    fn run(text: &str, size: f32, max_width: f32) -> TextRun {
        TextRun {
            node: 1,
            x: 0.0,
            y: size * 0.9,
            max_width,
            text: text.into(),
            size,
            weight: 400,
            color: BLACK,
            align: TextAlign::Left,
            clip: vec![],
        }
    }

    #[test]
    fn measure_single_line() {
        let mut system = TextSystem::new();
        let (w, h) = system.measure_run(&run("Hello", 16.0, 200.0));
        assert!(w > 0.0 && w <= 200.0, "measured width {w}");
        // One line of 16px text with a 1.25 line height is 20 logical px tall.
        assert!((h - 20.0).abs() < 0.01, "measured height {h}");
    }

    #[test]
    fn measure_wraps_at_max_width() {
        let mut system = TextSystem::new();
        let long = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.";
        let (w, h) = system.measure_run(&run(long, 16.0, 100.0));
        assert!(w <= 100.0 + 0.5, "wrapped width {w} exceeds the box");
        assert!(h > 20.0, "long text wraps to more than one line (height {h})");
    }

    #[test]
    fn measure_empty_text() {
        let mut system = TextSystem::new();
        assert_eq!(system.measure_run(&run("", 16.0, 200.0)), (0.0, 0.0));
        assert_eq!(system.measure_run(&run("   ", 16.0, 200.0)), (0.0, 0.0));
    }

    #[test]
    fn shape_replaces_text_with_multi_line_glyphs() {
        let mut system = TextSystem::new();
        let long = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs and then some more words to force wrapping for sure.";
        let mut frame = PaintFrame {
            width: 200,
            height: 400,
            scale: 1.0,
            background: BLACK,
            ops: vec![DrawOp::Text(run(long, 16.0, 120.0))],
        };
        system.shape(&mut frame);

        assert_eq!(frame.ops.len(), 1, "the Text op is replaced by exactly one Glyphs op");
        let DrawOp::Glyphs(batch) = &frame.ops[0] else {
            panic!("expected a Glyphs op");
        };
        assert!(!batch.instances.is_empty(), "glyphs were emitted");

        // More than one line: glyph quads appear at several distinct y positions.
        let mut ys: Vec<f32> = batch.instances.iter().map(|g| g.pos[1]).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ys.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        assert!(ys.len() > 1, "expected wrapped lines, got {} y positions", ys.len());

        // Atlas was rebuilt and carries real coverage in its alpha channel.
        let atlas = system.atlas();
        assert!(atlas.generation >= 1, "generation advanced");
        assert_eq!(atlas.rgba.len(), (atlas.width * atlas.height * 4) as usize, "exact atlas byte length");
        assert!(atlas.rgba.iter().any(|&b| b > 0), "atlas has rasterized coverage");
    }

    #[test]
    fn shape_rasterizes_at_device_resolution() {
        let mut system = TextSystem::new();
        let mut frame = PaintFrame {
            width: 400,
            height: 400,
            scale: 2.0,
            background: BLACK,
            ops: vec![DrawOp::Text(run("Hello", 16.0, 300.0))],
        };
        system.shape(&mut frame);
        let DrawOp::Glyphs(batch) = &frame.ops[0] else {
            panic!("expected a Glyphs op");
        };
        assert!(!batch.instances.is_empty());
        // Glyph quads are sized in atlas texels (device px): at scale 2 a 16px run produces
        // glyphs roughly twice as many device pixels tall.
        let tallest = batch.instances.iter().map(|g| g.size[1]).fold(0.0f32, f32::max);
        assert!(tallest > 20.0, "device-resolution glyph height {tallest} is too small");

        // pos stays logical while uv stays in atlas pixels.
        for glyph in &batch.instances {
            assert!(glyph.pos[0] >= 0.0 && glyph.pos[1] >= 0.0);
            assert!(glyph.uv[0] < system.atlas().width as f32 && glyph.uv[1] < system.atlas().height as f32);
        }
    }

    #[test]
    fn shape_aligns_centered_lines() {
        let mut system = TextSystem::new();
        // First measure the advance width of the line with left alignment.
        let measured = run("Hi", 16.0, 200.0);
        let (line_w, _) = system.measure_run(&measured);

        let mut centered = run("Hi", 16.0, 200.0);
        centered.align = TextAlign::Center;
        let mut frame = PaintFrame {
            width: 200,
            height: 100,
            scale: 1.0,
            background: BLACK,
            ops: vec![DrawOp::Text(centered)],
        };
        system.shape(&mut frame);
        let DrawOp::Glyphs(batch) = &frame.ops[0] else {
            panic!("expected a Glyphs op");
        };
        let left = batch.instances.iter().map(|g| g.pos[0]).fold(f32::MAX, f32::min);
        // The line starts (within glyph-bearing slack) at the centered offset.
        let offset = (200.0 - line_w) / 2.0;
        assert!((left - offset).abs() < 3.0, "centered offset {offset}, ink starts at {left}");
    }

    #[test]
    fn shape_preserves_non_text_ops() {
        let mut system = TextSystem::new();
        let rect = DrawOp::Rect(crate::gpu::types::ShapeRect {
            node: 2,
            rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 },
            radius: 0.0,
            fill: Some(BLACK),
            border: None,
            clip: vec![],
        });
        let mut frame = PaintFrame {
            width: 100,
            height: 100,
            scale: 1.0,
            background: BLACK,
            ops: vec![rect, DrawOp::Text(run("Hi", 16.0, 100.0))],
        };
        system.shape(&mut frame);
        assert_eq!(frame.ops.len(), 2);
        assert!(matches!(frame.ops[0], DrawOp::Rect(_)), "rect op untouched");
        assert!(matches!(frame.ops[1], DrawOp::Glyphs(_)), "text op shaped in place");
    }
}

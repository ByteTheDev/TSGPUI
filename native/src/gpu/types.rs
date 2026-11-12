//! Shared draw-list contract for the tsgpu renderer.
//!
//! `gpu::backend` produces a `PaintFrame` from the document tree; `gpu::paint` turns it into
//! GPU work; `gpu::surface` puts it on screen. Everything in this file is the boundary between
//! those modules — change it only with all module owners' consent (see RENDERER_TSGPU.md).

/// Linear RGBA, 0.0–1.0.
pub type Color = [f32; 4];

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRect {
    /// Document node key this rect came from (0 for decoration rects with no node).
    pub node: u64,
    pub rect: Rect,
    pub radius: f32,
    pub fill: Option<Color>,
    pub border: Option<Border>,
    /// When set, this op is scissored to the intersection of the given rects. The backend
    /// flattens nesting, so paint never tracks a clip stack.
    pub clip: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A single shaped-or-shapeable text run laid out within `max_width`; `text.rs` handles
/// shaping, wrapping and glyph caching; `y` is the baseline of the first line.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Document node key this run came from (0 for decoration runs with no node).
    pub node: u64,
    pub x: f32,
    pub y: f32,
    pub max_width: f32,
    pub text: String,
    pub size: f32,
    pub weight: u16,
    pub color: Color,
    pub align: TextAlign,
    pub clip: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    Rect(ShapeRect),
    Text(TextRun),
    /// Pre-shaped glyphs produced by `text.rs`; `paint.rs` renders them from its atlas texture.
    Glyphs(GlyphBatch),
}

/// One positioned glyph quad sampling the shared atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphInstance {
    /// Top-left corner in logical pixels.
    pub pos: [f32; 2],
    /// Top-left UV and size in atlas pixels.
    pub uv: [f32; 2],
    pub size: [f32; 2],
    pub color: Color,
    pub clip: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GlyphBatch {
    pub instances: Vec<GlyphInstance>,
}

/// CPU-rasterized glyph atlas; `text.rs` grows it, `paint.rs` uploads it when `generation` changes.
#[derive(Debug, Clone, Default)]
pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub generation: u64,
}

/// One frame's complete draw list, in paint order.
#[derive(Debug, Clone, Default)]
pub struct PaintFrame {
    pub width: u32,
    pub height: u32,
    /// Device pixels per logical pixel, from the windowing layer.
    pub scale: f32,
    pub background: Color,
    pub ops: Vec<DrawOp>,
}

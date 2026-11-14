//! Rendrix layout: a layout-engine-neutral box tree ([`LayoutNode`]) and a [`LayoutEngine`] trait.
//!
//! The only implementation today is [`TaffyEngine`]. It keeps its Taffy tree between frames and only
//! touches nodes whose style, children or text content changed, so unchanged subtrees reuse Taffy's
//! layout cache (incremental recalculation).
//!
//! Text is measured through [`TextMeasurer`]; this crate never shapes text itself.

mod taffy_engine;

pub use taffy_engine::TaffyEngine;

use rendrix_css::ComputedStyle;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// One styled span of text inside an inline formatting context.
#[derive(Debug, Clone)]
pub struct InlineRun {
    pub text: String,
    pub style: Arc<ComputedStyle>,
    /// DOM node that produced the run (for hit testing / selection); 0 if synthetic.
    pub node: u64,
    /// A forced line break (`<br>`).
    pub is_break: bool,
}

/// The inline content of a block: runs flowed and wrapped as one paragraph.
#[derive(Debug, Clone)]
pub struct InlineContent {
    pub runs: Vec<InlineRun>,
    /// The block's own style: text-align, white-space, line-height strut.
    pub container: Arc<ComputedStyle>,
}

impl InlineContent {
    /// A hash over everything that affects measurement. Equal keys ⇒ equal measurements.
    pub fn key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for r in &self.runs {
            r.text.hash(&mut h);
            r.is_break.hash(&mut h);
            let s = &r.style;
            s.font_family.hash(&mut h);
            s.font_size.to_bits().hash(&mut h);
            s.font_weight.hash(&mut h);
            (s.font_style as u8).hash(&mut h);
            s.letter_spacing.to_bits().hash(&mut h);
            s.word_spacing.to_bits().hash(&mut h);
            (s.text_transform as u8).hash(&mut h);
            format!("{:?}", s.line_height).hash(&mut h);
        }
        let c = &self.container;
        (c.white_space as u8).hash(&mut h);
        (c.text_align as u8).hash(&mut h);
        c.text_overflow_ellipsis.hash(&mut h);
        format!("{:?}", c.line_height).hash(&mut h);
        c.font_size.to_bits().hash(&mut h);
        h.finish()
    }
}

/// Measures inline content. Implemented by the text system.
pub trait TextMeasurer {
    /// Size of `content` wrapped at `max_width` (`None` = no wrapping constraint / max-content).
    fn measure(&mut self, content: &InlineContent, max_width: Option<f32>) -> (f32, f32);
}

#[derive(Debug, Clone)]
pub enum LayoutKind {
    Box,
    Text(Arc<InlineContent>),
    /// An image-like box with an intrinsic size.
    Replaced { width: f32, height: f32 },
}

#[derive(Debug, Clone)]
pub struct LayoutNode {
    /// Stable identity across frames (DOM node id for element boxes).
    pub id: u64,
    pub style: Arc<ComputedStyle>,
    pub kind: LayoutKind,
    pub children: Vec<LayoutNode>,
}

/// Resolved geometry of one box. `x`/`y` are relative to the parent's border-box origin.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BoxLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Scrollable content extent (≥ box size minus borders when content overflows).
    pub content_width: f32,
    pub content_height: f32,
    /// Resolved border widths [top, right, bottom, left].
    pub border: [f32; 4],
    /// Resolved padding [top, right, bottom, left].
    pub padding: [f32; 4],
}

#[derive(Debug, Default)]
pub struct LayoutResult {
    pub boxes: HashMap<u64, BoxLayout>,
}

pub trait LayoutEngine {
    fn layout(&mut self, root: &LayoutNode, viewport: (f32, f32), measurer: &mut dyn TextMeasurer) -> LayoutResult;
}

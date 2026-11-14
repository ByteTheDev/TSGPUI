//! Rendrix core: the engine that turns a DOM into pixels.
//!
//! `Engine::update` runs the pipeline: stylesheets → restyle → box tree → layout → display list.
//! Interaction (hit testing, scrolling, text editing) works against the geometry of the last frame.
//! The engine knows nothing about windows, GPUs or JavaScript; those layers sit above it.

mod assets;
mod boxtree;
mod edit;
mod paint;
mod style;

pub use assets::{AssetProvider, DirAssets, ImageEntry, MapAssets};
pub use edit::{EditOp, EditResult};
pub use paint::{HitEntry, ScrollInfo};

use assets::Images;
use rendrix_css::animation::mat_apply;
use rendrix_css::{Color, ComputedStyle, Cursor, RuleSet};
use rendrix_dom::{Document, NodeId};
use rendrix_gfx::{DisplayList, TextSystem};
use rendrix_layout::{InlineContent, LayoutEngine, LayoutNode, LayoutResult, TaffyEngine, TextMeasurer};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Adapter so layout can measure text through the engine's shared text system.
struct Measurer<'a>(&'a RefCell<TextSystem>);

impl TextMeasurer for Measurer<'_> {
    fn measure(&mut self, content: &InlineContent, max_width: Option<f32>) -> (f32, f32) {
        self.0.borrow_mut().measure(content, max_width)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct UpdateInfo {
    /// A new display list was produced.
    pub repainted: bool,
    /// Transitions or animations are running: call `update` again next frame.
    pub animating: bool,
    /// Absolute time (engine seconds) at which something time-driven (caret blink) needs a frame.
    pub next_wake: Option<f64>,
}

#[derive(Debug, Default, Clone)]
pub struct EngineStats {
    pub frames: u64,
    pub restyles: u64,
    pub layouts: u64,
    pub full_restyles: u64,
    pub last_layout_micros: u64,
    pub last_style_micros: u64,
    pub last_paint_micros: u64,
}

pub struct Engine {
    pub doc: Document,
    pub(crate) text: RefCell<TextSystem>,
    layout: TaffyEngine,
    pub(crate) assets: Box<dyn AssetProvider>,
    pub(crate) images: RefCell<Images>,
    pub viewport: (f32, f32),
    pub scale: f32,
    pub dark: bool,

    pub(crate) rules: RuleSet,
    pub(crate) rules_dirty: bool,
    pub(crate) rules_media_sig: Option<(u32, u32, u32, bool)>,
    pub(crate) adopted_sheets: Vec<String>,
    pub(crate) loaded_fonts: HashSet<String>,
    pub(crate) styles: Vec<Option<Arc<ComputedStyle>>>,
    pub(crate) pseudo: HashMap<(NodeId, bool), Arc<ComputedStyle>>,
    pub(crate) anim: HashMap<NodeId, style::AnimState>,
    pub now: f64,

    layout_tree: Option<LayoutNode>,
    layout_result: LayoutResult,
    display: DisplayList,
    hits: Vec<HitEntry>,
    scrolls: Vec<ScrollInfo>,
    rects: HashMap<NodeId, [f32; 4]>,
    scroll: RefCell<HashMap<NodeId, (f32, f32)>>,
    input_scroll: RefCell<HashMap<NodeId, f32>>,
    caret_epoch: f64,
    last_caret_visible: bool,
    viewport_changed: bool,
    force_repaint: bool,
    pub(crate) style_changed: bool,
    last_gc: f64,
    freed_log: Vec<NodeId>,
    pub stats: EngineStats,
}

impl Engine {
    pub fn new(assets: Box<dyn AssetProvider>) -> Engine {
        Engine::with_document(Document::new(), assets)
    }

    pub fn with_document(doc: Document, assets: Box<dyn AssetProvider>) -> Engine {
        Engine {
            doc,
            text: RefCell::new(TextSystem::new()),
            layout: TaffyEngine::new(),
            assets,
            images: RefCell::new(Images::default()),
            viewport: (800.0, 600.0),
            scale: 1.0,
            dark: false,
            rules: RuleSet::new(),
            rules_dirty: true,
            rules_media_sig: None,
            adopted_sheets: Vec::new(),
            loaded_fonts: HashSet::new(),
            styles: Vec::new(),
            pseudo: HashMap::new(),
            anim: HashMap::new(),
            now: 0.0,
            layout_tree: None,
            layout_result: LayoutResult::default(),
            display: DisplayList::new(800.0, 600.0, Color::WHITE),
            hits: Vec::new(),
            scrolls: Vec::new(),
            rects: HashMap::new(),
            scroll: RefCell::new(HashMap::new()),
            input_scroll: RefCell::new(HashMap::new()),
            caret_epoch: 0.0,
            last_caret_visible: false,
            viewport_changed: true,
            force_repaint: true,
            style_changed: false,
            last_gc: 0.0,
            freed_log: Vec::new(),
            stats: EngineStats::default(),
        }
    }

    pub fn set_viewport(&mut self, width: f32, height: f32, scale: f32) {
        if (width, height, scale) != (self.viewport.0, self.viewport.1, self.scale) {
            self.viewport = (width, height);
            self.scale = scale;
            self.viewport_changed = true;
        }
    }

    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.rules_dirty = true;
        }
    }

    /// Add a constructed stylesheet (from `adoptedStyleSheets`/framework injection).
    pub fn add_stylesheet(&mut self, css: &str) {
        self.adopted_sheets.push(css.to_string());
        self.rules_dirty = true;
    }

    pub fn text_borrow(&self) -> RefMut<'_, TextSystem> {
        self.text.borrow_mut()
    }

    pub fn text_system(&self) -> Ref<'_, TextSystem> {
        self.text.borrow()
    }

    pub fn load_font(&self, data: Vec<u8>) {
        self.text.borrow_mut().load_font(data);
    }

    pub fn image_entry(&self, url: &str) -> Option<ImageEntry> {
        self.images.borrow_mut().get(url, self.assets.as_ref())
    }

    /// Ids of nodes freed since the last call (so script-side state keyed by id can be dropped).
    pub fn take_freed_for_js(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.freed_log)
    }

    /// Images decoded since the last call; the renderer must upload them under their ids.
    pub fn take_image_uploads(&self) -> Vec<ImageEntry> {
        std::mem::take(&mut self.images.borrow_mut().pending_upload)
    }

    pub fn read_asset(&self, path: &str) -> Option<Vec<u8>> {
        self.assets.read(path)
    }

    pub fn display_list(&self) -> &DisplayList {
        &self.display
    }

    pub fn caret_geometry(&self, content: &InlineContent, width: Option<f32>, byte: usize) -> (f32, f32, f32) {
        self.text.borrow_mut().caret(content, width, byte)
    }

    pub fn input_scroll(&self, id: NodeId, caret_x: f32, width: f32) -> f32 {
        let mut m = self.input_scroll.borrow_mut();
        let cur = m.entry(id).or_insert(0.0);
        if caret_x - *cur > width - 2.0 {
            *cur = caret_x - (width - 2.0);
        }
        if caret_x < *cur {
            *cur = caret_x;
        }
        *cur = cur.max(0.0);
        *cur
    }

    pub fn scroll_offset(&self, id: NodeId, max_x: f32, max_y: f32) -> (f32, f32) {
        let m = self.scroll.borrow();
        let (x, y) = m.get(&id).copied().unwrap_or((0.0, 0.0));
        (x.clamp(0.0, max_x), y.clamp(0.0, max_y))
    }

    pub fn set_scroll(&mut self, id: NodeId, x: f32, y: f32) {
        self.scroll.borrow_mut().insert(id, (x.max(0.0), y.max(0.0)));
        self.force_repaint = true;
    }

    pub fn get_scroll(&self, id: NodeId) -> (f32, f32) {
        let m = self.scroll.borrow().get(&id).copied().unwrap_or((0.0, 0.0));
        let max = self.scrolls.iter().find(|s| s.node == id).map(|s| (s.max_x, s.max_y)).unwrap_or((0.0, 0.0));
        (m.0.clamp(0.0, max.0), m.1.clamp(0.0, max.1))
    }

    /// Scroll extents (scrollWidth, scrollHeight, clientWidth, clientHeight) of an element.
    pub fn scroll_metrics(&self, id: NodeId) -> Option<(f32, f32, f32, f32)> {
        let b = self.layout_result.boxes.get(&(id as u64))?;
        let cw = b.width - b.border[1] - b.border[3];
        let ch = b.height - b.border[0] - b.border[2];
        Some((b.content_width.max(cw), b.content_height.max(ch), cw, ch))
    }

    // ------------------------------------------------------------------------------------
    // The pipeline
    // ------------------------------------------------------------------------------------

    pub fn caret_visible(&self) -> bool {
        ((self.now - self.caret_epoch) * 2.0).floor() as i64 % 2 == 0
    }

    /// Restart caret blinking (call on any editing or focus change).
    pub fn reset_caret(&mut self) {
        self.caret_epoch = self.now;
        self.force_repaint = true;
    }

    fn focused_text_control(&self) -> Option<NodeId> {
        self.doc.focused().filter(|n| self.doc.is_text_control(*n))
    }

    /// DOM, stylesheet or viewport changes waiting to be applied (ignores running animations, which
    /// advance by themselves each frame).
    pub fn has_pending_changes(&self) -> bool {
        let d = &self.doc.dirty;
        d.all_style || d.layout || d.paint || !d.style_roots.is_empty() || !d.state_roots.is_empty() || self.doc.sheets_dirty || self.rules_dirty || self.viewport_changed || self.force_repaint
    }

    /// Does the document need a new frame? Cheap check used by the frame scheduler.
    pub fn needs_update(&self) -> bool {
        let d = &self.doc.dirty;
        d.all_style || d.layout || d.paint || !d.style_roots.is_empty() || !d.state_roots.is_empty() || self.doc.sheets_dirty || self.rules_dirty || self.viewport_changed || self.force_repaint || !self.anim.is_empty()
    }

    pub fn update(&mut self, now: f64) -> UpdateInfo {
        self.now = now;
        self.stats.last_style_micros = 0;
        self.stats.last_layout_micros = 0;
        self.stats.last_paint_micros = 0;
        let t0 = std::time::Instant::now();
        let rules_rebuilt = self.refresh_rules();
        // Sweep detached, unreferenced subtrees at most once a second.
        if now - self.last_gc >= 1.0 {
            self.last_gc = now;
            self.doc.collect_garbage();
        }
        // Nodes freed by the DOM must not leave stale caches behind.
        for id in self.doc.take_freed() {
            self.freed_log.push(id);
            if let Some(s) = self.styles.get_mut(id as usize) {
                *s = None;
            }
            self.anim.remove(&id);
            self.pseudo.remove(&(id, true));
            self.pseudo.remove(&(id, false));
            self.scroll.borrow_mut().remove(&id);
        }
        let dirty = self.doc.take_dirty();
        // Subtrees that were detached lose their cached styles: re-attaching restyles them in context.
        for root in &dirty.detached {
            for n in self.doc.depth_first(*root) {
                if let Some(s) = self.styles.get_mut(n as usize) {
                    *s = None;
                }
                self.anim.remove(&n);
                self.pseudo.remove(&(n, true));
                self.pseudo.remove(&(n, false));
            }
        }
        let all = dirty.all_style || rules_rebuilt || self.viewport_changed;
        let mut restyled = false;
        if all || !dirty.style_roots.is_empty() || !dirty.state_roots.is_empty() {
            restyled = self.restyle(all, dirty.style_roots.clone(), dirty.state_roots.clone());
            self.stats.restyles += 1;
            if all {
                self.stats.full_restyles += 1;
            }
        }
        self.stats.last_style_micros = t0.elapsed().as_micros() as u64;
        let animating = self.tick_motion(now) || !self.anim.is_empty();

        // Caret blink toggles need a repaint without any DOM change.
        let caret_state = self.focused_text_control().is_some() && self.caret_visible();
        let caret_toggled = self.focused_text_control().is_some() && caret_state != self.last_caret_visible;
        self.last_caret_visible = caret_state;

        let needs = restyled || dirty.layout || dirty.paint || self.viewport_changed || self.layout_tree.is_none() || animating || caret_toggled || self.force_repaint;
        if !needs {
            return UpdateInfo { repainted: false, animating: false, next_wake: self.next_wake() };
        }
        let relayout = restyled || dirty.layout || self.viewport_changed || self.layout_tree.is_none() || animating;
        self.viewport_changed = false;
        self.force_repaint = false;

        if relayout {
            let t1 = std::time::Instant::now();
            let tree = boxtree::Builder::new(self).build_root();
            if let Some(mut tree) = tree {
                // The root fills the viewport width.
                let mut s = (*tree.style).clone();
                s.width = rendrix_css::Dim::Px(self.viewport.0);
                tree.style = Arc::new(s);
                self.layout_result = self.layout.layout(&tree, self.viewport, &mut Measurer(&self.text));
                self.layout_tree = Some(tree);
            } else {
                self.layout_tree = None;
                self.layout_result = LayoutResult::default();
            }
            self.stats.layouts += 1;
            self.stats.last_layout_micros = t1.elapsed().as_micros() as u64;
        }

        let t2 = std::time::Instant::now();
        self.paint();
        self.stats.last_paint_micros = t2.elapsed().as_micros() as u64;
        self.stats.frames += 1;
        UpdateInfo { repainted: true, animating, next_wake: self.next_wake() }
    }

    fn next_wake(&self) -> Option<f64> {
        if self.focused_text_control().is_some() {
            let phase = (self.now - self.caret_epoch) * 2.0;
            Some(self.caret_epoch + (phase.floor() + 1.0) / 2.0)
        } else {
            None
        }
    }

    fn paint(&mut self) {
        let Some(tree) = self.layout_tree.take() else {
            self.display = DisplayList::new(self.viewport.0, self.viewport.1, Color::WHITE);
            self.hits.clear();
            return;
        };
        let html = self.doc.document_element().unwrap_or(0);
        let body = self.doc.body();
        let html_bg = self.style_of(html).map(|s| s.background_color).unwrap_or(Color::TRANSPARENT);
        let (bg, skip_body) = if html_bg.a > 0.0 {
            (html_bg, None)
        } else {
            match body.and_then(|b| self.style_of(b).map(|s| (b, s.background_color))) {
                Some((b, c)) if c.a > 0.0 => (c, Some(b)),
                _ => (Color::WHITE, None),
            }
        };
        // An opaque canvas colour: composite translucent backgrounds onto white.
        let bg = if bg.a < 1.0 { Color::WHITE.lerp(Color { a: 1.0, ..bg }, bg.a) } else { bg };
        let list = DisplayList::new(self.viewport.0, self.viewport.1, bg);
        let mut painter = paint::Painter::new(self, &self.layout_result, list, html);
        painter.set_skip_body_background(skip_body);
        painter.paint_root(&tree);
        let paint::Painter { list, hits, scrolls, .. } = painter;
        let mut rects = HashMap::new();
        for h in &hits {
            rects.entry(h.node).or_insert(h.rect);
        }
        self.display = list;
        self.hits = hits;
        self.scrolls = scrolls;
        self.rects = rects;
        self.layout_tree = Some(tree);
    }

    /// Advance the engine clock without producing a frame (so style/geometry reads from script see
    /// transitions and animations at the current time).
    pub fn set_time(&mut self, now: f64) {
        if now > self.now {
            self.now = now;
        }
    }

    /// Bring geometry up to date after DOM changes (forced synchronous layout, like `offsetWidth`).
    pub fn ensure_layout(&mut self) {
        if self.needs_update() {
            let now = self.now;
            self.update(now);
        }
    }

    // ------------------------------------------------------------------------------------
    // Geometry queries
    // ------------------------------------------------------------------------------------

    /// Border-box rectangle of an element in viewport coordinates (x, y, w, h); transforms are applied
    /// to the returned bounding box.
    pub fn element_rect(&self, id: NodeId) -> Option<[f32; 4]> {
        let r = *self.rects.get(&id)?;
        // Apply the cumulative transform recorded for hit testing, if any.
        let h = self.hits.iter().find(|h| h.node == id)?;
        match h.inv.and_then(rendrix_css::animation::mat_invert) {
            Some(m) => {
                let cs = [mat_apply(m, r[0], r[1]), mat_apply(m, r[0] + r[2], r[1]), mat_apply(m, r[0], r[1] + r[3]), mat_apply(m, r[0] + r[2], r[1] + r[3])];
                let x0 = cs.iter().map(|c| c.0).fold(f32::MAX, f32::min);
                let x1 = cs.iter().map(|c| c.0).fold(f32::MIN, f32::max);
                let y0 = cs.iter().map(|c| c.1).fold(f32::MAX, f32::min);
                let y1 = cs.iter().map(|c| c.1).fold(f32::MIN, f32::max);
                Some([x0, y0, x1 - x0, y1 - y0])
            }
            None => Some(r),
        }
    }

    /// Untransformed layout size and offset within the offset parent chain (offsetWidth/Height).
    pub fn layout_size(&self, id: NodeId) -> Option<(f32, f32)> {
        self.layout_result.boxes.get(&(id as u64)).map(|b| (b.width, b.height))
    }

    /// Topmost element under a viewport point, honouring clips, transforms, z-order and `pointer-events`.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        for h in self.hits.iter().rev() {
            if !h.pointer_events {
                continue;
            }
            let c = h.clip;
            if x < c[0] || y < c[1] || x >= c[0] + c[2] || y >= c[1] + c[3] {
                continue;
            }
            let (lx, ly) = match h.inv {
                Some(m) => mat_apply(m, x, y),
                None => (x, y),
            };
            let r = h.rect;
            if lx >= r[0] && lx < r[0] + r[2] && ly >= r[1] && ly < r[1] + r[3] {
                return Some(h.node);
            }
        }
        None
    }

    pub fn cursor_at(&self, x: f32, y: f32) -> Cursor {
        let Some(mut n) = self.hit_test(x, y) else { return Cursor::Default };
        loop {
            if let Some(s) = self.style_of(n) {
                if s.cursor != Cursor::Auto {
                    return s.cursor;
                }
                if self.doc.is_text_control(n) {
                    return Cursor::Text;
                }
                if matches!(self.doc.tag(n), Some("a")) && self.doc.has_attr(n, "href") {
                    return Cursor::Pointer;
                }
            }
            match self.doc.parent(n) {
                Some(p) if self.doc.element(p).is_some() => n = p,
                _ => return Cursor::Default,
            }
        }
    }

    /// Apply a wheel delta at a point to the innermost scrollable container that can move. Returns whether anything scrolled.
    pub fn scroll_by(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        let candidates: Vec<ScrollInfo> = self.scrolls.iter().rev().copied().collect();
        // Prefer containers under the pointer, innermost (latest painted) first; the root is the fallback.
        for s in candidates {
            let inside = x >= s.clip[0] && y >= s.clip[1] && x < s.clip[0] + s.clip[2] && y < s.clip[1] + s.clip[3] && x >= s.rect[0] && y >= s.rect[1] && x < s.rect[0] + s.rect[2] && y < s.rect[1] + s.rect[3];
            if !inside {
                continue;
            }
            let (cx, cy) = self.get_scroll(s.node);
            let nx = (cx + dx).clamp(0.0, s.max_x);
            let ny = (cy + dy).clamp(0.0, s.max_y);
            if (nx, ny) != (cx, cy) {
                self.scroll.borrow_mut().insert(s.node, (nx, ny));
                self.force_repaint = true;
                return true;
            }
        }
        false
    }

    pub fn content_of(&self, id: NodeId) -> Option<Arc<InlineContent>> {
        fn find(n: &LayoutNode, id: u64) -> Option<Arc<InlineContent>> {
            if n.id == id {
                if let rendrix_layout::LayoutKind::Text(c) = &n.kind {
                    return Some(c.clone());
                }
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        self.layout_tree.as_ref().and_then(|t| find(t, id as u64))
    }

    /// Content-box origin and width of a text control, for converting pointer positions to text offsets.
    pub fn text_origin(&self, id: NodeId) -> Option<(f32, f32, f32)> {
        let b = self.layout_result.boxes.get(&(id as u64))?;
        let r = self.rects.get(&id)?;
        let x = r[0] + b.border[3] + b.padding[3];
        let y = r[1] + b.border[0] + b.padding[0];
        let w = b.width - b.border[1] - b.border[3] - b.padding[1] - b.padding[3];
        Some((x, y, w))
    }

    /// Byte offset in a text control's value nearest to a viewport point.
    pub fn text_offset_at(&self, id: NodeId, x: f32, y: f32) -> Option<usize> {
        let content = self.content_of(id)?;
        let (ox, oy, w) = self.text_origin(id)?;
        let single = self.doc.tag(id) == Some("input");
        let scroll = if single { self.input_scroll.borrow().get(&id).copied().unwrap_or(0.0) } else { 0.0 };
        let off = self.text.borrow_mut().hit(&content, if single { None } else { Some(w) }, x - ox + scroll, y - oy);
        // The paragraph for an empty field is the placeholder; its offsets are not value offsets.
        Some(off.min(self.doc.value(id).len()))
    }

    // ------------------------------------------------------------------------------------
    // Style queries
    // ------------------------------------------------------------------------------------

    /// `getComputedStyle(el).getPropertyValue(name)`.
    pub fn computed_value(&self, id: NodeId, name: &str) -> String {
        let Some(s) = self.style_of(id) else { return String::new() };
        let px = |v: f32| format!("{}px", (v * 1000.0).round() / 1000.0);
        let dim = |d: rendrix_css::Dim| match d {
            rendrix_css::Dim::Auto => "auto".to_string(),
            rendrix_css::Dim::Px(v) => px(v),
            rendrix_css::Dim::Percent(p) => format!("{p}%"),
        };
        let color = |c: Color| {
            let (r, g, b) = ((c.r * 255.0).round(), (c.g * 255.0).round(), (c.b * 255.0).round());
            if c.a >= 1.0 {
                format!("rgb({r}, {g}, {b})")
            } else {
                format!("rgba({r}, {g}, {b}, {})", (c.a * 1000.0).round() / 1000.0)
            }
        };
        if let Some(v) = name.strip_prefix("--") {
            return s.custom.get(&format!("--{v}")).cloned().unwrap_or_default();
        }
        let box_size = self.layout_result.boxes.get(&(id as u64));
        match name {
            "display" => format!("{:?}", s.display).to_lowercase().replace("inlineblock", "inline-block").replace("inlineflex", "inline-flex").replace("inlinegrid", "inline-grid"),
            "position" => format!("{:?}", s.position).to_lowercase(),
            "color" => color(s.color),
            "background-color" => color(s.background_color),
            "opacity" => format!("{}", s.opacity),
            "visibility" => format!("{:?}", s.visibility).to_lowercase(),
            "font-size" => px(s.font_size),
            "font-weight" => s.font_weight.to_string(),
            "font-family" => s.font_family.join(", "),
            "line-height" => match s.line_height {
                rendrix_css::LineHeight::Normal => "normal".into(),
                rendrix_css::LineHeight::Number(n) => px(n * s.font_size),
                rendrix_css::LineHeight::Px(p) => px(p),
            },
            "width" => box_size.map(|b| px(b.width - if s.box_sizing == rendrix_css::BoxSizing::ContentBox { b.padding[1] + b.padding[3] + b.border[1] + b.border[3] } else { 0.0 })).unwrap_or_else(|| dim(s.width)),
            "height" => box_size.map(|b| px(b.height - if s.box_sizing == rendrix_css::BoxSizing::ContentBox { b.padding[0] + b.padding[2] + b.border[0] + b.border[2] } else { 0.0 })).unwrap_or_else(|| dim(s.height)),
            "margin-top" => dim(s.margin[0]),
            "margin-right" => dim(s.margin[1]),
            "margin-bottom" => dim(s.margin[2]),
            "margin-left" => dim(s.margin[3]),
            "padding-top" => dim(s.padding[0]),
            "padding-right" => dim(s.padding[1]),
            "padding-bottom" => dim(s.padding[2]),
            "padding-left" => dim(s.padding[3]),
            "top" => dim(s.inset[0]),
            "right" => dim(s.inset[1]),
            "bottom" => dim(s.inset[2]),
            "left" => dim(s.inset[3]),
            "border-top-width" => px(s.border_width[0]),
            "border-right-width" => px(s.border_width[1]),
            "border-bottom-width" => px(s.border_width[2]),
            "border-left-width" => px(s.border_width[3]),
            "z-index" => s.z_index.map(|z| z.to_string()).unwrap_or_else(|| "auto".into()),
            "overflow-x" => format!("{:?}", s.overflow_x).to_lowercase(),
            "overflow-y" => format!("{:?}", s.overflow_y).to_lowercase(),
            "cursor" => format!("{:?}", s.cursor).to_lowercase(),
            "pointer-events" => format!("{:?}", s.pointer_events).to_lowercase(),
            "text-align" => format!("{:?}", s.text_align).to_lowercase(),
            "flex-direction" => format!("{:?}", s.flex_direction).to_lowercase(),
            "transform" => {
                if s.transform.is_empty() {
                    "none".into()
                } else {
                    let m = rendrix_css::animation::transform_matrix(&s.transform, s.transform_origin, 0.0, 0.0);
                    format!("matrix({}, {}, {}, {}, {}, {})", m[0], m[1], m[2], m[3], m[4], m[5])
                }
            }
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests;

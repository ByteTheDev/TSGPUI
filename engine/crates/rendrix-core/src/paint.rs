//! Painting: layout boxes → display list, plus the hit-test list built in the same traversal.

use crate::boxtree::ANON;
use crate::Engine;
use rendrix_css::animation::{mat_apply, mat_invert, mat_mul, transform_matrix, Matrix, IDENTITY};
use rendrix_css::{BackgroundImage, BackgroundRepeat, BackgroundSize, BorderStyle, Color, ComputedStyle, Dim, ObjectFit, Overflow, PointerEvents, Visibility};
use rendrix_dom::NodeId;
use rendrix_gfx::{DisplayList, Fill, ImageItem, Item, RectItem, ShadowItem, TextItem};
use rendrix_layout::{BoxLayout, LayoutKind, LayoutNode, LayoutResult};

#[derive(Debug, Clone)]
pub struct HitEntry {
    pub node: NodeId,
    /// Rectangle in the local (pre-transform) coordinate space of this entry.
    pub rect: [f32; 4],
    /// Inverse of the cumulative transform, `None` for identity.
    pub inv: Option<Matrix>,
    /// Axis-aligned clip in viewport space.
    pub clip: [f32; 4],
    pub pointer_events: bool,
}

/// Scroll geometry of one scroll container, used to clamp and to hit-test wheel events.
#[derive(Debug, Clone, Copy)]
pub struct ScrollInfo {
    pub node: NodeId,
    pub rect: [f32; 4],
    pub clip: [f32; 4],
    pub max_x: f32,
    pub max_y: f32,
}

pub(crate) struct Painter<'a> {
    pub engine: &'a Engine,
    pub res: &'a LayoutResult,
    pub list: DisplayList,
    pub hits: Vec<HitEntry>,
    pub scrolls: Vec<ScrollInfo>,
    xf: Vec<Matrix>,
    clips: Vec<[f32; 4]>,
    skip_body_bg: Option<NodeId>,
    root: NodeId,
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (x0, y0) = (a[0].max(b[0]), a[1].max(b[1]));
    let (x1, y1) = ((a[0] + a[2]).min(b[0] + b[2]), (a[1] + a[3]).min(b[1] + b[3]));
    [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
}

fn aabb(m: Matrix, r: [f32; 4]) -> [f32; 4] {
    let cs = [mat_apply(m, r[0], r[1]), mat_apply(m, r[0] + r[2], r[1]), mat_apply(m, r[0], r[1] + r[3]), mat_apply(m, r[0] + r[2], r[1] + r[3])];
    let x0 = cs.iter().map(|c| c.0).fold(f32::MAX, f32::min);
    let x1 = cs.iter().map(|c| c.0).fold(f32::MIN, f32::max);
    let y0 = cs.iter().map(|c| c.1).fold(f32::MAX, f32::min);
    let y1 = cs.iter().map(|c| c.1).fold(f32::MIN, f32::max);
    [x0, y0, x1 - x0, y1 - y0]
}

fn resolve_radii(s: &ComputedStyle, w: f32, h: f32) -> [f32; 4] {
    let r = |d: Dim| match d {
        Dim::Px(v) => v,
        Dim::Percent(p) => p / 100.0 * w.min(h),
        Dim::Auto => 0.0,
    };
    [r(s.border_radius[0]), r(s.border_radius[1]), r(s.border_radius[2]), r(s.border_radius[3])]
}

impl<'a> Painter<'a> {
    pub fn new(engine: &'a Engine, res: &'a LayoutResult, list: DisplayList, root: NodeId) -> Self {
        let (list_w, list_h) = (list.width, list.height);
        Painter { engine, res, list, hits: Vec::new(), scrolls: Vec::new(), xf: vec![IDENTITY], clips: vec![[0.0, 0.0, list_w, list_h]], skip_body_bg: None, root }
    }

    pub fn set_skip_body_background(&mut self, body: Option<NodeId>) {
        self.skip_body_bg = body;
    }

    fn cur_xf(&self) -> Matrix {
        *self.xf.last().unwrap()
    }

    fn cur_clip(&self) -> [f32; 4] {
        *self.clips.last().unwrap()
    }

    fn add_hit(&mut self, node: NodeId, rect: [f32; 4], style: &ComputedStyle) {
        let m = self.cur_xf();
        let inv = if m == IDENTITY { None } else { mat_invert(m) };
        self.hits.push(HitEntry { node, rect, inv, clip: self.cur_clip(), pointer_events: style.pointer_events != PointerEvents::None });
    }

    fn color_visible(c: Color) -> bool {
        c.a > 0.0
    }

    pub fn paint_root(&mut self, node: &LayoutNode) {
        self.paint_node(node, (0.0, 0.0));
    }

    fn paint_node(&mut self, node: &LayoutNode, origin: (f32, f32)) {
        let Some(b) = self.res.boxes.get(&node.id).copied() else { return };
        let style = node.style.clone();
        let (x, y) = (origin.0 + b.x, origin.1 + b.y);
        let rect = [x, y, b.width, b.height];
        let is_real = node.id & ANON == 0;

        // Cull boxes whose whole footprint (including scrollable overflow, shadows and outlines) is
        // outside the current clip: nothing below them can be visible or hit.
        if node.id != self.root as u64 {
            let reach = style.box_shadow.iter().map(|s| s.blur + s.spread.abs() + s.dx.abs().max(s.dy.abs())).fold(0.0f32, f32::max) + style.outline_width + style.outline_offset.abs() + 2.0;
            let (ew, eh) = (b.width.max(b.content_width + b.border[3]), b.height.max(b.content_height + b.border[0]));
            let ext = [x - reach, y - reach, ew + 2.0 * reach, eh + 2.0 * reach];
            let screen = aabb(self.cur_xf(), if style.has_transform() { transformed_bounds(&style, ext, rect) } else { ext });
            let clip = self.cur_clip();
            if screen[0] >= clip[0] + clip[2] || screen[1] >= clip[1] + clip[3] || screen[0] + screen[2] <= clip[0] || screen[1] + screen[3] <= clip[1] {
                return;
            }
        }

        let mut pops: Vec<Item> = Vec::new();
        let mut popped_xf = false;
        if style.has_transform() {
            let m = transform_matrix(&style.transform, style.transform_origin, b.width, b.height);
            let full = mat_mul(mat_mul([1.0, 0.0, 0.0, 1.0, x, y], m), [1.0, 0.0, 0.0, 1.0, -x, -y]);
            self.list.push(Item::PushTransform(full));
            let cum = mat_mul(self.cur_xf(), full);
            self.xf.push(cum);
            popped_xf = true;
            pops.push(Item::PopTransform);
        }
        if style.opacity < 1.0 {
            self.list.push(Item::PushOpacity(style.opacity));
            pops.push(Item::PopOpacity);
        }

        let visible = style.visibility == Visibility::Visible;
        if visible {
            self.paint_self(node, &style, &b, rect);
        }
        if is_real {
            self.add_hit(node.id as NodeId, rect, &style);
        }
        self.paint_content(node, &style, &b, rect, visible);
        self.paint_children(node, &style, &b, rect, origin);

        if visible && style.outline_width > 0.0 && !style.outline_color.is_transparent() {
            let o = style.outline_offset + style.outline_width / 2.0;
            let r = [x - o - style.outline_width / 2.0, y - o - style.outline_width / 2.0, b.width + 2.0 * o + style.outline_width, b.height + 2.0 * o + style.outline_width];
            let radii = resolve_radii(&style, b.width, b.height).map(|v| v + o + style.outline_width / 2.0);
            self.list.push(Item::Rect(RectItem { rect: r, radii, fill: Fill::None, border: Some(([style.outline_width; 4], style.outline_color)) }));
        }

        for p in pops.into_iter().rev() {
            self.list.push(p);
        }
        if popped_xf {
            self.xf.pop();
        }
    }

    fn paint_self(&mut self, node: &LayoutNode, style: &ComputedStyle, b: &BoxLayout, rect: [f32; 4]) {
        let id = node.id;
        let radii = resolve_radii(style, b.width, b.height);
        // Outer shadows, painted back-to-front.
        for sh in style.box_shadow.iter().rev().filter(|s| !s.inset) {
            if Self::color_visible(sh.color) {
                self.list.push(Item::Shadow(ShadowItem {
                    rect: [rect[0] + sh.dx - sh.spread, rect[1] + sh.dy - sh.spread, rect[2] + 2.0 * sh.spread, rect[3] + 2.0 * sh.spread],
                    radii: radii.map(|r| (r + sh.spread).max(0.0)),
                    blur: sh.blur,
                    color: sh.color,
                    inset: false,
                    hole: [0.0; 4],
                }));
            }
        }
        // Canvas background propagation: the root/body background is painted by the viewport.
        let propagated = id == self.root as u64 || Some(id as NodeId) == self.skip_body_bg;
        let has_border = style.border_width.iter().any(|w| *w > 0.0) && style.border_style.iter().any(|s| !matches!(s, BorderStyle::None | BorderStyle::Hidden));
        let border = if has_border {
            // Per-side colors are not modelled by the shader; use the first visible side's color.
            let c = (0..4).find(|i| style.border_width[*i] > 0.0).map(|i| style.border_color[i]).unwrap_or(style.color);
            Some((style.border_width, c))
        } else {
            None
        };
        if !propagated {
            let fill = if Self::color_visible(style.background_color) { Fill::Solid(style.background_color) } else { Fill::None };
            if !matches!(fill, Fill::None) || border.is_some() {
                self.list.push(Item::Rect(RectItem { rect, radii, fill, border }));
            }
            let (px, py, pw, ph) = (rect[0] + style.border_width[3], rect[1] + style.border_width[0], rect[2] - style.border_width[1] - style.border_width[3], rect[3] - style.border_width[0] - style.border_width[2]);
            for img in &style.background_images {
                match img {
                    BackgroundImage::LinearGradient { angle, stops } => {
                        self.list.push(Item::Rect(RectItem { rect: [px, py, pw, ph], radii, fill: Fill::Linear { angle: *angle, stops: gradient_stops(stops) }, border: None }));
                    }
                    BackgroundImage::RadialGradient { stops } => {
                        self.list.push(Item::Rect(RectItem { rect: [px, py, pw, ph], radii, fill: Fill::Radial { stops: gradient_stops(stops) }, border: None }));
                    }
                    BackgroundImage::Url(url) => self.paint_background_image(url, style, [px, py, pw, ph], radii),
                }
            }
        }
        for sh in style.box_shadow.iter().filter(|s| s.inset) {
            if Self::color_visible(sh.color) {
                let inner = [rect[0] + style.border_width[3], rect[1] + style.border_width[0], rect[2] - style.border_width[1] - style.border_width[3], rect[3] - style.border_width[0] - style.border_width[2]];
                self.list.push(Item::Shadow(ShadowItem {
                    rect: inner,
                    radii,
                    blur: sh.blur.max(0.5),
                    color: sh.color,
                    inset: true,
                    hole: [inner[0] + sh.dx + sh.spread, inner[1] + sh.dy + sh.spread, inner[2] - 2.0 * sh.spread, inner[3] - 2.0 * sh.spread],
                }));
            }
        }
    }

    fn paint_background_image(&mut self, url: &str, style: &ComputedStyle, area: [f32; 4], radii: [f32; 4]) {
        let Some(img) = self.engine.image_entry(url) else { return };
        let (iw, ih) = (img.width as f32, img.height as f32);
        if iw <= 0.0 || ih <= 0.0 || area[2] <= 0.0 || area[3] <= 0.0 {
            return;
        }
        let (tw, th) = match style.background_size {
            BackgroundSize::Auto => (iw, ih),
            BackgroundSize::Cover => {
                let k = (area[2] / iw).max(area[3] / ih);
                (iw * k, ih * k)
            }
            BackgroundSize::Contain => {
                let k = (area[2] / iw).min(area[3] / ih);
                (iw * k, ih * k)
            }
            BackgroundSize::Explicit(w, h) => {
                let rw = w.resolve(area[2]);
                let rh = h.resolve(area[3]);
                match (rw, rh) {
                    (Some(w), Some(h)) => (w, h),
                    (Some(w), None) => (w, w * ih / iw),
                    (None, Some(h)) => (h * iw / ih, h),
                    (None, None) => (iw, ih),
                }
            }
        };
        let cover_like = matches!(style.background_size, BackgroundSize::Cover);
        let (ox, oy) = if cover_like { (area[0] + (area[2] - tw) / 2.0, area[1] + (area[3] - th) / 2.0) } else { (area[0], area[1]) };
        self.list.push(Item::PushClip { rect: area, radius: radii[0] });
        let (rx, ry) = match style.background_repeat {
            BackgroundRepeat::Repeat => (true, true),
            BackgroundRepeat::RepeatX => (true, false),
            BackgroundRepeat::RepeatY => (false, true),
            BackgroundRepeat::NoRepeat => (false, false),
        };
        let nx = if rx { ((area[2] / tw).ceil() as i32 + 1).clamp(1, 32) } else { 1 };
        let ny = if ry { ((area[3] / th).ceil() as i32 + 1).clamp(1, 32) } else { 1 };
        for j in 0..ny {
            for i in 0..nx {
                self.list.push(Item::Image(ImageItem { image: img.id, rect: [ox + i as f32 * tw, oy + j as f32 * th, tw, th], radii: [0.0; 4], uv: [0.0, 0.0, 1.0, 1.0] }));
            }
        }
        self.list.push(Item::PopClip);
    }

    fn paint_content(&mut self, node: &LayoutNode, style: &ComputedStyle, b: &BoxLayout, rect: [f32; 4], visible: bool) {
        let engine = self.engine;
        let content_x = rect[0] + b.border[3] + b.padding[3];
        let content_y = rect[1] + b.border[0] + b.padding[0];
        let content_w = (b.width - b.border[1] - b.border[3] - b.padding[1] - b.padding[3]).max(0.0);
        let content_h = (b.height - b.border[0] - b.border[2] - b.padding[0] - b.padding[2]).max(0.0);
        match &node.kind {
            LayoutKind::Text(content) => {
                let is_real = node.id & ANON == 0;
                let dom = if is_real { Some(node.id as NodeId) } else { None };
                let control = dom.filter(|d| engine.doc.is_text_control(*d) || engine.doc.tag(*d) == Some("textarea"));
                let mut tx = content_x;
                let mut clipped = false;
                if let Some(c) = control {
                    self.list.push(Item::PushClip { rect: [content_x, content_y, content_w, content_h], radius: 0.0 });
                    clipped = true;
                    let focused = engine.doc.element(c).is_some_and(|e| e.flags.focus);
                    if focused && engine.doc.tag(c) == Some("input") {
                        // Keep the caret inside the visible field.
                        let sel = engine.doc.element(c).unwrap().form.selection_end;
                        let (cx, _, _) = engine.caret_geometry(content, None, sel);
                        tx -= engine.input_scroll(c, cx, content_w);
                    }
                }
                let width = if control.is_some() && engine.doc.tag(control.unwrap()) == Some("input") { 1.0e5 } else { content_w };
                if visible {
                    self.list.push(Item::Text(TextItem { content: content.clone(), x: tx, y: content_y, width }));
                }
                // Text selection + caret for focused text controls.
                if let (Some(c), true) = (control, visible) {
                    let el = engine.doc.element(c).unwrap();
                    if el.flags.focus {
                        let (s, e) = (el.form.selection_start, el.form.selection_end);
                        let w = if engine.doc.tag(c) == Some("input") { None } else { Some(content_w) };
                        // Selection and caret are drawn under/over the text in paragraph space.
                        let mut ts = engine.text_borrow();
                        if s != e {
                            for r in ts.selection_rects(content, w, s, e) {
                                self.list.push(Item::Rect(RectItem { rect: [tx + r[0], content_y + r[1], r[2], r[3]], radii: [0.0; 4], fill: Fill::Solid(Color::rgba(0.10, 0.45, 0.91, 0.30)), border: None }));
                            }
                        }
                        if engine.caret_visible() && s == e {
                            let (cx, cy, ch) = ts.caret(content, w, e);
                            let col = style.caret_color.unwrap_or(style.color);
                            self.list.push(Item::Rect(RectItem { rect: [tx + cx, content_y + cy, 1.0, ch], radii: [0.0; 4], fill: Fill::Solid(col), border: None }));
                        }
                    }
                }
                if clipped {
                    self.list.push(Item::PopClip);
                }
                // Hit regions for inline elements nested in the paragraph.
                let para = engine.text_borrow().paragraph(content, Some(width));
                for seg in &para.segments {
                    let Some(run) = content.runs.get(seg.run) else { continue };
                    if run.node != node.id && run.node & ANON == 0 {
                        let r = [tx + seg.x, content_y + seg.line_top, seg.w, seg.line_height];
                        self.add_hit(run.node as NodeId, r, &run.style);
                    }
                }
            }
            LayoutKind::Replaced { .. } => {
                if !visible {
                    return;
                }
                let tag = node.id as NodeId;
                if engine.doc.tag(tag) == Some("img") {
                    if let Some(img) = engine.doc.get_attr(tag, "src").and_then(|s| engine.image_entry(s)) {
                        let (iw, ih) = (img.width as f32, img.height as f32);
                        let (dw, dh) = (content_w, content_h);
                        if iw > 0.0 && ih > 0.0 && dw > 0.0 && dh > 0.0 {
                            let (mut r, mut uv) = ([content_x, content_y, dw, dh], [0.0, 0.0, 1.0, 1.0]);
                            match style.object_fit {
                                ObjectFit::Fill => {}
                                ObjectFit::Contain | ObjectFit::ScaleDown => {
                                    let mut k = (dw / iw).min(dh / ih);
                                    if style.object_fit == ObjectFit::ScaleDown {
                                        k = k.min(1.0);
                                    }
                                    let (w, h) = (iw * k, ih * k);
                                    r = [content_x + (dw - w) / 2.0, content_y + (dh - h) / 2.0, w, h];
                                }
                                ObjectFit::None => {
                                    let (w, h) = (iw.min(dw), ih.min(dh));
                                    r = [content_x + (dw - w) / 2.0, content_y + (dh - h) / 2.0, w, h];
                                    uv = [(iw - w) / 2.0 / iw, (ih - h) / 2.0 / ih, w / iw, h / ih];
                                }
                                ObjectFit::Cover => {
                                    let k = (dw / iw).max(dh / ih);
                                    let (vw, vh) = (dw / k, dh / k);
                                    uv = [(iw - vw) / 2.0 / iw, (ih - vh) / 2.0 / ih, vw / iw, vh / ih];
                                }
                            }
                            let radii = resolve_radii(style, b.width, b.height);
                            self.list.push(Item::Image(ImageItem { image: img.id, rect: r, radii, uv }));
                        }
                    }
                }
            }
            LayoutKind::Box => {
                if !visible || node.id & ANON != 0 {
                    return;
                }
                let id = node.id as NodeId;
                // Checkbox / radio marks.
                if engine.doc.tag(id) == Some("input") && engine.doc.is_checked(id) {
                    let ty = engine.doc.get_attr(id, "type").unwrap_or("").to_ascii_lowercase();
                    let white = Fill::Solid(Color::WHITE);
                    if ty == "checkbox" {
                        // A check built from two rotated bars.
                        let (cx, cy, s) = (content_x + content_w / 2.0, content_y + content_h / 2.0, content_w.min(content_h));
                        let bar = |angle: f32, ox: f32, oy: f32, len: f32| {
                            let m = mat_mul([1.0, 0.0, 0.0, 1.0, cx + ox, cy + oy], transform_matrix(&[rendrix_css::TransformOp::Rotate(angle)], (Dim::Px(0.0), Dim::Px(0.0)), 0.0, 0.0));
                            (m, len)
                        };
                        let thick = (s * 0.16).max(1.5);
                        for (m, len) in [bar(0.785, -s * 0.12, s * 0.02, s * 0.26), bar(-0.785, s * 0.10, s * 0.08, s * 0.5)] {
                            self.list.push(Item::PushTransform(m));
                            self.list.push(Item::Rect(RectItem { rect: [0.0, -thick / 2.0, len, thick], radii: [thick / 2.0; 4], fill: white.clone(), border: None }));
                            self.list.push(Item::PopTransform);
                        }
                    } else if ty == "radio" {
                        let d = content_w.min(content_h) * 0.4;
                        self.list.push(Item::Rect(RectItem { rect: [content_x + (content_w - d) / 2.0, content_y + (content_h - d) / 2.0, d, d], radii: [d / 2.0; 4], fill: white, border: None }));
                    }
                }
            }
        }
    }

    fn paint_children(&mut self, node: &LayoutNode, style: &ComputedStyle, b: &BoxLayout, rect: [f32; 4], _origin: (f32, f32)) {
        if node.children.is_empty() {
            return;
        }
        let engine = self.engine;
        let clips = style.clips_overflow() && node.id & ANON == 0 || node.id == self.root as u64;
        let is_root = node.id == self.root as u64;
        let mut child_origin = (rect[0], rect[1]);
        let mut pushed_clip = false;
        if clips && !is_root {
            let inner = [rect[0] + b.border[3], rect[1] + b.border[0], rect[2] - b.border[1] - b.border[3], rect[3] - b.border[0] - b.border[2]];
            let r = resolve_radii(style, b.width, b.height);
            self.list.push(Item::PushClip { rect: inner, radius: r[0].max(0.0) });
            let c = intersect(self.cur_clip(), aabb(self.cur_xf(), inner));
            self.clips.push(c);
            pushed_clip = true;
            let scrollable = matches!(style.overflow_y, Overflow::Scroll | Overflow::Auto) || matches!(style.overflow_x, Overflow::Scroll | Overflow::Auto);
            if scrollable {
                let max_y = (b.content_height - inner[3]).max(0.0);
                let max_x = (b.content_width - inner[2]).max(0.0);
                let (sx, sy) = engine.scroll_offset(node.id as NodeId, max_x, max_y);
                child_origin = (rect[0] - sx, rect[1] - sy);
                self.scrolls.push(ScrollInfo { node: node.id as NodeId, rect: inner, clip: self.cur_clip(), max_x, max_y });
            }
        }
        if is_root {
            let max_y = (b.content_height.max(b.height) - self.list.height).max(0.0);
            let (sx, sy) = engine.scroll_offset(node.id as NodeId, 0.0, max_y);
            child_origin = (rect[0] - sx, rect[1] - sy);
            self.scrolls.push(ScrollInfo { node: node.id as NodeId, rect: [0.0, 0.0, self.list.width, self.list.height], clip: [0.0, 0.0, self.list.width, self.list.height], max_x: 0.0, max_y });
        }

        // Stacking: negative z, in-flow, positioned/z=0, positive z.
        let mut order: Vec<(i32, i32, usize)> = Vec::with_capacity(node.children.len());
        for (i, c) in node.children.iter().enumerate() {
            let cs = &c.style;
            let positioned = cs.position != rendrix_css::Position::Static || cs.creates_stacking_context();
            let z = if positioned { cs.z_index.unwrap_or(0) } else { 0 };
            let group = if !positioned { 1 } else if z < 0 { 0 } else if z == 0 { 2 } else { 3 };
            order.push((group, if group == 0 || group == 3 { z } else { 0 }, i));
        }
        order.sort();
        for (_, _, i) in order {
            self.paint_node(&node.children[i], child_origin);
        }

        if pushed_clip {
            // Overlay scrollbar thumb.
            let scrollable_y = matches!(style.overflow_y, Overflow::Scroll | Overflow::Auto);
            if scrollable_y {
                let inner = [rect[0] + b.border[3], rect[1] + b.border[0], rect[2] - b.border[1] - b.border[3], rect[3] - b.border[0] - b.border[2]];
                let max_y = (b.content_height - inner[3]).max(0.0);
                if max_y > 0.5 {
                    let (_, sy) = engine.scroll_offset(node.id as NodeId, 0.0, max_y);
                    let track = inner[3] - 4.0;
                    let thumb = (inner[3] / b.content_height * track).clamp(24.0, track);
                    let ty = inner[1] + 2.0 + (sy / max_y) * (track - thumb);
                    self.list.push(Item::Rect(RectItem { rect: [inner[0] + inner[2] - 8.0, ty, 6.0, thumb], radii: [3.0; 4], fill: Fill::Solid(Color::rgba(0.5, 0.5, 0.5, 0.55)), border: None }));
                }
            }
            self.list.push(Item::PopClip);
            self.clips.pop();
        }
    }
}

/// Bounds of a box after its own transform, expressed in the parent's coordinate space.
fn transformed_bounds(style: &ComputedStyle, ext: [f32; 4], rect: [f32; 4]) -> [f32; 4] {
    let m = transform_matrix(&style.transform, style.transform_origin, rect[2], rect[3]);
    let full = mat_mul(mat_mul([1.0, 0.0, 0.0, 1.0, rect[0], rect[1]], m), [1.0, 0.0, 0.0, 1.0, -rect[0], -rect[1]]);
    aabb(full, ext)
}

fn gradient_stops(stops: &[rendrix_css::GradientStop]) -> Vec<(Color, f32)> {
    let n = stops.len();
    stops
        .iter()
        .enumerate()
        .map(|(i, s)| (s.color, s.pos.unwrap_or(if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 })))
        .collect()
}

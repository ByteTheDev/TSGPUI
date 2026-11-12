//! Document Ã¢â€ â€™ layout Ã¢â€ â€™ paint glue; owns the frame cache and re-layouts on state change.
//! Owner: agent D.
//!
//! Contract (fixed):
//! - `App::new(doc: crate::package::AppPackage) -> Self`
//! - `App::frame(&mut self, scale: f32, size: (u32, u32)) -> types::PaintFrame` Ã¢â‚¬â€ full repaint:
//!   layout::measure on doc.root, text runs for visible text nodes, clips flattened down the tree.
//! - `App::pointer(&mut self, event: input::PointerEvent)` Ã¢â‚¬â€ hit test via input::hit_test,
//!   hover/active state, dispatch `data-onclick` actions (M0: log to stdout is acceptable;
//!   QuickJS backend wiring via the existing runtime bridge is M1)
//! - `App::key(&mut self, event: input::KeyEvent)`
//!
//! M0 notes: `frame()` rebuilds the whole draw list on every call (surface repaints every vsync),
//! so state changes are picked up without an explicit dirty flag. Hit testing uses a cached
//! PRE-shaping copy of the last frame Ã¢â‚¬â€ shaping replaces `Text` ops with `Glyphs` batches that
//! carry no node key.

use crate::gpu::{input, layout, text, types};
use crate::package::Node;
use std::collections::{BTreeMap, HashMap};
use types::{Border, Color, DrawOp, PaintFrame, Rect, ShapeRect, TextAlign, TextRun};

/// Neutral fallback window background when the document declares none.
const DEFAULT_BACKGROUND: Color = [0.95, 0.95, 0.95, 1.0];
/// Default text style (used when layout's text_styles map has no entry for a node; layout's
/// resolved values take precedence in `resolve_text_style` below).
const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_FONT_WEIGHT: u16 = 400;
const DEFAULT_TEXT_COLOR: Color = [0.0, 0.0, 0.0, 1.0];
/// Baseline approximation inside a text node's rect: padding-top plus ~0.9em (ascent + half of
/// the leading of a ~1.2 line-height box). Exact metrics come from text.rs measurement in M2.
const BASELINE_EM: f32 = 0.9;

pub struct App {
    doc: crate::package::AppPackage,
    text: text::TextSystem,
    /// Node key Ã¢â€ â€™ tag, flattened at construction (the doc tree is immutable in M0).
    tags: HashMap<u64, String>,
    /// Node key Ã¢â€ â€™ action name for `data-onclick`.
    onclick: HashMap<u64, String>,
    // --- interaction state (M0) ---
    hovered: Option<u64>,
    active: Option<u64>,
    focus: Option<u64>,
    /// Node pressed but not yet released (for click = press+release on the same node).
    pressed: Option<u64>,
    /// Checkbox checked state, toggled on click; painted into the tree next frame.
    checked: HashMap<u64, bool>,
    /// Text values for input/textarea nodes (M2 editing; M0 forwards key characters here).
    values: HashMap<u64, String>,
    /// Last produced frame BEFORE glyph shaping, so hit tests still see `Text` ops with node keys.
    hit_frame: Option<PaintFrame>,
}

impl App {
    pub fn new(doc: crate::package::AppPackage) -> Self {
        let mut tags = HashMap::new();
        let mut onclick = HashMap::new();
        collect_nodes(&doc.root, &mut tags, &mut onclick);
        Self {
            doc,
            text: text::TextSystem::new(),
            tags,
            onclick,
            hovered: None,
            active: None,
            focus: None,
            pressed: None,
            checked: HashMap::new(),
            values: HashMap::new(),
            hit_frame: None,
        }
    }

    pub fn frame(&mut self, scale: f32, size: (u32, u32)) -> PaintFrame {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let logical = (size.0 as f32 / scale, size.1 as f32 / scale);
        let output = layout::measure(&self.doc.root, logical, scale, &mut self.text);

        let mut frame = PaintFrame {
            width: size.0,
            height: size.1,
            scale,
            background: self.background_color(),
            ops: Vec::new(),
        };
        let root = &self.doc.root;
        walk(root, &[], &output, &self.state_flags(), &mut frame.ops);

        // Cache the pre-shape list for input hit testing, then let text.rs replace Text ops
        // with shaped Glyph batches for painting.
        self.hit_frame = Some(frame.clone());
        self.text.shape(&mut frame);
        frame
    }

    /// The current glyph atlas for `paint::Painter::set_glyph_atlas` (used by surface.rs).
    pub fn atlas(&self) -> &crate::gpu::types::GlyphAtlas {
        self.text.atlas()
    }

    pub fn pointer(&mut self, event: input::PointerEvent) {
        match event {
            input::PointerEvent::Move { x, y } => {
                self.hovered = self.hit(x, y);
            }
            input::PointerEvent::Down { x, y } => {
                let hit = self.hit(x, y);
                self.pressed = hit;
                if let Some(key) = hit {
                    match self.tags.get(&key).map(String::as_str) {
                        Some("button" | "checkbox") => self.active = Some(key),
                        Some("input" | "textarea") => {
                            self.active = Some(key);
                            self.focus = Some(key);
                        }
                        _ => {}
                    }
                }
            }
            input::PointerEvent::Up { x, y } => {
                let hit = self.hit(x, y);
                let pressed = self.pressed.take();
                self.active = None;
                // A click is press + release on the same node.
                if let Some(key) = pressed.filter(|k| Some(*k) == hit) {
                    if self.is_checkbox(key) {
                        let checked = self.checked.entry(key).or_insert(false);
                        *checked = !*checked;
                        // M0: state map only; the checkmark decoration paints next frame.
                    }
                    if let Some(action) = self.onclick.get(&key).cloned() {
                        self.dispatch_action(&action);
                    }
                }
            }
        }
    }

    pub fn key(&mut self, event: input::KeyEvent) {
        // M0: forward characters into the focused input/textarea's value. Caret, selection,
        // clipboard and navigation keys arrive with the M2 text-editing state.
        let Some(key) = self.focus else { return };
        let editable = matches!(
            self.tags.get(&key).map(String::as_str),
            Some("input") | Some("textarea")
        );
        if !editable {
            return;
        }
        let value = self.values.entry(key).or_default();
        match event {
            input::KeyEvent::Character(c) => value.push(c),
            input::KeyEvent::Backspace => {
                value.pop();
            }
            _ => {}
        }
    }

    /// Dispatch seam. M0 logs the action; M1 routes it through the existing QuickJS runtime
    /// bridge so backend actions mutate app state, which re-drives `frame()`.
    // TODO(M1): wire to the QuickJS backend bridge (rquickjs) instead of printing.
    fn dispatch_action(&mut self, action: &str) {
        println!("[tsgpu] action {action}");
    }

    fn hit(&self, x: f32, y: f32) -> Option<u64> {
        self.hit_frame.as_ref().and_then(|frame| input::hit_test(frame, x, y))
    }

    fn is_checkbox(&self, key: u64) -> bool {
        match self.tags.get(&key).map(String::as_str) {
            Some("checkbox") => true,
            Some("input") => {
                // The compiler may resolve <input type="checkbox"> to tag "input".
                find_node(&self.doc.root, key)
                    .map(|node| node.attrs.get("type").map(String::as_str) == Some("checkbox"))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Which interaction states currently apply Ã¢â‚¬â€ merged into styles during paint.
    fn state_flags(&self) -> StateFlags<'_> {
        StateFlags {
            hovered: self.hovered,
            active: self.active,
            focus: self.focus,
            checked: &self.checked,
            values: &self.values,
        }
    }

    /// Document background: the root node stands in for `html, body` in the compiled IR.
    fn background_color(&self) -> Color {
        style_color(&self.doc.root.style, &["background-color", "background"])
            .unwrap_or(DEFAULT_BACKGROUND)
    }
}

/// Per-frame interaction flags passed down the paint walk.
struct StateFlags<'a> {
    hovered: Option<u64>,
    active: Option<u64>,
    focus: Option<u64>,
    checked: &'a HashMap<u64, bool>,
    values: &'a HashMap<u64, String>,
}

fn collect_nodes(node: &Node, tags: &mut HashMap<u64, String>, onclick: &mut HashMap<u64, String>) {
    tags.insert(node.key, node.tag.clone());
    if let Some(action) = node.attrs.get("data-onclick") {
        onclick.insert(node.key, action.clone());
    }
    for child in &node.children {
        collect_nodes(child, tags, onclick);
    }
}

/// Depth-first search for a node by key.
fn find_node<'a>(node: &'a Node, key: u64) -> Option<&'a Node> {
    if node.key == key {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_node(child, key))
}

/// Resolved paint style for one node: document declarations with interaction-state overrides
/// (hover/active/focus) applied on top, parsed from the CSS subset into concrete values.
#[derive(Debug, Clone)]
struct NodeStyle {
    fill: Option<Color>,
    radius: f32,
    border: Option<Border>,
    opacity: f32,
    /// CSS box order: top, right, bottom, left.
    padding: [f32; 4],
    overflow_hidden: bool,
    font_size: f32,
    font_weight: u16,
    color: Color,
    align: TextAlign,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            fill: None,
            radius: 0.0,
            border: None,
            opacity: 1.0,
            padding: [0.0; 4],
            overflow_hidden: false,
            font_size: DEFAULT_FONT_SIZE,
            font_weight: DEFAULT_FONT_WEIGHT,
            color: DEFAULT_TEXT_COLOR,
            align: TextAlign::Left,
        }
    }
}

impl NodeStyle {
    /// Overlay one property map (base style or a state variant) onto the resolved style.
    fn apply(&mut self, map: &BTreeMap<String, String>) {
        if let Some(c) = style_color(map, &["background-color", "background"]) {
            self.fill = Some(c);
        }
        if let Some(r) = map.get("border-radius").and_then(|v| parse_length(v)) {
            self.radius = r;
        }
        if let Some(border) = parse_border(map) {
            self.border = Some(border);
        }
        if let Some(o) = map.get("opacity").and_then(|v| v.trim().parse::<f32>().ok()) {
            self.opacity = o.clamp(0.0, 1.0);
        }
        self.padding = parse_padding(map);
        if map.get("overflow").map(String::as_str) == Some("hidden") {
            self.overflow_hidden = true;
        }
        if let Some(s) = map.get("font-size").and_then(|v| parse_length(v)) {
            self.font_size = s;
        }
        if let Some(w) = map.get("font-weight").and_then(|v| parse_weight(v)) {
            self.font_weight = w;
        }
        if let Some(c) = style_color(map, &["color"]) {
            self.color = c;
        }
        if let Some(a) = map.get("text-align").and_then(|v| parse_align(v)) {
            self.align = a;
        }
    }

    fn resolve(node: &Node, flags: &StateFlags) -> Self {
        let mut style = Self::default();
        style.apply(&node.style);
        // Later overrides win, matching CSS state-rule precedence for M0.
        if flags.hovered == Some(node.key) {
            if let Some(variant) = node.states.get("hover") {
                style.apply(variant);
            }
        }
        if flags.active == Some(node.key) {
            if let Some(variant) = node.states.get("active") {
                style.apply(variant);
            }
        }
        if flags.focus == Some(node.key) {
            if let Some(variant) = node.states.get("focus") {
                style.apply(variant);
            }
        }
        style
    }
}

/// First parseable color among the given property names, trying the whole value and then its
/// first token (shorthand values like `background: #fff url(...)` still resolve).
fn style_color(map: &BTreeMap<String, String>, names: &[&str]) -> Option<Color> {
    for name in names {
        if let Some(value) = map.get(*name) {
            if let Some(color) = parse_color(value) {
                return Some(color);
            }
            if let Some(token) = value.split_whitespace().next() {
                if let Some(color) = parse_color(token) {
                    return Some(color);
                }
            }
        }
    }
    None
}

/// `border-width` + `border-color`, falling back to the `border` shorthand's first/last tokens.
fn parse_border(map: &BTreeMap<String, String>) -> Option<Border> {
    let width = map.get("border-width").and_then(|v| parse_length(v));
    let color = style_color(map, &["border-color"]);
    if let Some(shorthand) = map.get("border") {
        let tokens: Vec<&str> = shorthand.split_whitespace().collect();
        let short_width = tokens.first().and_then(|t| parse_length(t));
        let short_color = tokens.iter().rev().find_map(|t| parse_color(t));
        return Some(Border {
            width: width.or(short_width).unwrap_or(1.0),
            color: color.or(short_color).unwrap_or(DEFAULT_TEXT_COLOR),
        });
    }
    if width.is_some() || color.is_some() {
        return Some(Border {
            width: width.unwrap_or(1.0),
            color: color.unwrap_or(DEFAULT_TEXT_COLOR),
        });
    }
    None
}

/// CSS padding shorthand plus per-side overrides into [top, right, bottom, left].
fn parse_padding(map: &BTreeMap<String, String>) -> [f32; 4] {
    let mut padding = map
        .get("padding")
        .and_then(|v| parse_box(v))
        .unwrap_or([0.0; 4]);
    for (name, index) in [
        ("padding-top", 0),
        ("padding-right", 1),
        ("padding-bottom", 2),
        ("padding-left", 3),
    ] {
        if let Some(v) = map.get(name).and_then(|v| parse_length(v)) {
            padding[index] = v;
        }
    }
    padding
}

fn parse_box(value: &str) -> Option<[f32; 4]> {
    let sides: Vec<f32> = value.split_whitespace().filter_map(parse_length).collect();
    let [t, r, b, l] = match sides.as_slice() {
        [a] => [*a, *a, *a, *a],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    };
    Some([t, r, b, l])
}

/// Lengths in px (the M0 CSS subset); percentages and other units resolve to None for now.
fn parse_length(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.ends_with('%') {
        return None;
    }
    let digits = value.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    digits.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

fn parse_weight(value: &str) -> Option<u16> {
    match value.trim() {
        "bold" => Some(700),
        "normal" => Some(400),
        v => v.parse::<u16>().ok().filter(|w| (100..=900).contains(w)),
    }
}

fn parse_align(value: &str) -> Option<TextAlign> {
    match value.trim() {
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        "left" => Some(TextAlign::Left),
        _ => None,
    }
}

/// CSS color parser for the M0 subset: hex (#rgb/#rgba/#rrggbb/#rrggbbaa), rgb()/rgba(),
/// and a small named-color table.
fn parse_color(value: &str) -> Option<Color> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(hex) = value.strip_prefix('#') {
        let mut digits = Vec::with_capacity(hex.len());
        for c in hex.chars() {
            digits.push(c.to_digit(16)? as f32);
        }
        let (r, g, b, a) = match digits.len() {
            3 => (
                scale_short(digits[0]),
                scale_short(digits[1]),
                scale_short(digits[2]),
                1.0,
            ),
            4 => (
                scale_short(digits[0]),
                scale_short(digits[1]),
                scale_short(digits[2]),
                scale_short(digits[3]),
            ),
            6 => (
                scale_pair(digits[0], digits[1]),
                scale_pair(digits[2], digits[3]),
                scale_pair(digits[4], digits[5]),
                1.0,
            ),
            8 => (
                scale_pair(digits[0], digits[1]),
                scale_pair(digits[2], digits[3]),
                scale_pair(digits[4], digits[5]),
                scale_pair(digits[6], digits[7]),
            ),
            _ => return None,
        };
        return Some([r, g, b, a]);
    }
    let lowered = value.to_ascii_lowercase();
    if lowered == "transparent" {
        return Some([0.0, 0.0, 0.0, 0.0]);
    }
    if let Some(inner) = lowered
        .strip_prefix("rgba(")
        .or_else(|| lowered.strip_prefix("rgb("))
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let channel = |s: &str| -> Option<f32> {
            if let Some(pct) = s.strip_suffix('%') {
                pct.parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0))
            } else {
                s.parse::<f32>().ok().map(|v| (v / 255.0).clamp(0.0, 1.0))
            }
        };
        // Alpha is 0Ã¢â‚¬â€œ1 (or a percentage), never divided by 255.
        let alpha = |s: &str| -> Option<f32> {
            if let Some(pct) = s.strip_suffix('%') {
                pct.parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0))
            } else {
                s.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
            }
        };
        if parts.len() == 3 {
            return Some([
                channel(parts[0])?,
                channel(parts[1])?,
                channel(parts[2])?,
                1.0,
            ]);
        }
        if parts.len() == 4 {
            return Some([
                channel(parts[0])?,
                channel(parts[1])?,
                channel(parts[2])?,
                alpha(parts[3])?,
            ]);
        }
        return None;
    }
    named_color(&lowered)
}

fn scale_short(d: f32) -> f32 {
    (d * 17.0) / 255.0
}

fn scale_pair(high: f32, low: f32) -> f32 {
    (high * 16.0 + low) / 255.0
}

fn named_color(name: &str) -> Option<Color> {
    let (r, g, b) = match name {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "magenta" => (255, 0, 255),
        "cyan" => (0, 255, 255),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "pink" => (255, 192, 203),
        _ => return None,
    };
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
}

fn with_opacity(color: Color, opacity: f32) -> Color {
    [color[0], color[1], color[2], color[3] * opacity]
}

/// Visible text content of a node: its own text, or for inputs the current value.
/// Empty containers contribute no text op. Checked checkboxes get a checkmark prefix so the
/// toggled state is visible in M0 (a real checkmark decoration arrives with M1 state painting).
fn text_content(node: &Node, flags: &StateFlags) -> String {
    match node.tag.as_str() {
        "input" | "textarea" => {
            if let Some(value) = flags.values.get(&node.key) {
                if !value.is_empty() {
                    return value.clone();
                }
            }
            // Unedited inputs fall back to the declared value; M2 adds placeholder styling.
            node.attrs.get("value").cloned().unwrap_or_default()
        }
        "checkbox" => {
            let base = node.text.clone();
            if flags.checked.get(&node.key).copied().unwrap_or(false) {
                format!("Ã¢Å“â€œ {base}")
            } else {
                base
            }
        }
        _ => node.text.clone(),
    }
}

/// Recursive paint walk in tree order (parents before children = back to front).
/// `clip` is the flattened list of ancestor rects that set `overflow: hidden`.
fn walk(node: &Node, clip: &[Rect], output: &layout::LayoutOutput, flags: &StateFlags, ops: &mut Vec<DrawOp>) {
    let Some(rect) = output.rects.get(&node.key).copied() else {
        // Layout gave this node no box (e.g. display:none); nothing to paint for it or below.
        return;
    };
    let style = NodeStyle::resolve(node, flags);

    ops.push(DrawOp::Rect(ShapeRect {
        node: node.key,
        rect,
        radius: style.radius,
        fill: style.fill.map(|c| with_opacity(c, style.opacity)),
        border: style
            .border
            .map(|b| Border { width: b.width, color: with_opacity(b.color, style.opacity) }),
        clip: clip.to_vec(),
    }));

    let content = text_content(node, flags);
    if !content.is_empty() {
        // Text is content of the node, so it is also clipped by the node's own overflow rect.
        let mut text_clip = clip.to_vec();
        if style.overflow_hidden {
            text_clip.push(rect);
        }
        // The content box starts inside the node's border and padding; `x`/`max_width` use our
        // own padding parse (layout does not expose per-side padding), while the baseline comes
        // from layout's estimate when available.
        let border_w = style.border.as_ref().map(|b| b.width).unwrap_or(0.0);
        let left = border_w + style.padding[3];
        let right = border_w + style.padding[1];
        let (size, weight, color, align) = resolve_text_style(node, &style, flags, output);
        let baseline = output.baselines.get(&node.key).copied().unwrap_or_else(|| {
            rect.y + style.padding[0] + size * BASELINE_EM
        });
        ops.push(DrawOp::Text(TextRun {
            node: node.key,
            x: rect.x + left,
            y: baseline,
            max_width: (rect.w - left - right).max(0.0),
            text: content,
            size,
            weight,
            color: with_opacity(color, style.opacity),
            align,
            clip: text_clip,
        }));
    }

    let mut child_clip = clip.to_vec();
    if style.overflow_hidden {
        child_clip.push(rect);
    }
    for child in &node.children {
        walk(child, &child_clip, output, flags, ops);
    }
}

/// Text style for a `TextRun`: layout's resolved per-node style when it has one (it understands
/// rem/em and inheritance plans), falling back to the backend's own parse, with hover/active/
/// focus state overrides applied last (layout ignores interaction states by design for M0).
fn resolve_text_style(
    node: &Node,
    style: &NodeStyle,
    flags: &StateFlags,
    output: &layout::LayoutOutput,
) -> (f32, u16, Color, TextAlign) {
    let mut text = match output.text_styles.get(&node.key) {
        Some(ts) => (ts.size, ts.weight, ts.color, ts.align),
        None => (style.font_size, style.font_weight, style.color, style.align),
    };
    for (state, applies) in [
        ("hover", flags.hovered == Some(node.key)),
        ("active", flags.active == Some(node.key)),
        ("focus", flags.focus == Some(node.key)),
    ] {
        if applies {
            if let Some(variant) = node.states.get(state) {
                apply_text_overrides(variant, &mut text);
            }
        }
    }
    text
}

fn apply_text_overrides(map: &BTreeMap<String, String>, text: &mut (f32, u16, Color, TextAlign)) {
    if let Some(size) = map.get("font-size").and_then(|v| parse_length(v)) {
        text.0 = size;
    }
    if let Some(weight) = map.get("font-weight").and_then(|v| parse_weight(v)) {
        text.1 = weight;
    }
    if let Some(color) = style_color(map, &["color"]) {
        text.2 = color;
    }
    if let Some(align) = map.get("text-align").and_then(|v| parse_align(v)) {
        text.3 = align;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::AppPackage;

    fn doc(style: &str) -> AppPackage {
        let json = format!(
            r#"{{"format":"tsgpu-1","name":"app","title":"App","width":400,"height":300,
                "backend_js":"","root":{{"tag":"div","key":1,"style":{style}}}}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_color("#090b14"), Some([9.0 / 255.0, 11.0 / 255.0, 20.0 / 255.0, 1.0]));
        assert_eq!(parse_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_color("#0000"), Some([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(parse_color("#7288ffff").unwrap()[3], 1.0);
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn parses_function_and_named_colors() {
        assert_eq!(parse_color("rgb(255, 136, 0)"), Some([1.0, 136.0 / 255.0, 0.0, 1.0]));
        assert_eq!(parse_color("rgba(0, 0, 0, 0.5)").unwrap()[3], 0.5);
        assert_eq!(parse_color("rgb(50%, 100%, 0%)"), Some([0.5, 1.0, 0.0, 1.0]));
        assert_eq!(parse_color("White"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_color("transparent"), Some([0.0, 0.0, 0.0, 0.0]));
    }

    #[test]
    fn parses_lengths_boxes_and_weights() {
        assert_eq!(parse_length("48px"), Some(48.0));
        assert_eq!(parse_length("13.5"), Some(13.5));
        assert_eq!(parse_length("50%"), None);
        assert_eq!(parse_box("36px"), Some([36.0; 4]));
        assert_eq!(parse_box("4px 8px"), Some([4.0, 8.0, 4.0, 8.0]));
        assert_eq!(parse_box("1px 2px 3px"), Some([1.0, 2.0, 3.0, 2.0]));
        assert_eq!(parse_box("1px 2px 3px 4px"), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(parse_weight("700"), Some(700));
        assert_eq!(parse_weight("bold"), Some(700));
        assert_eq!(parse_weight("normal"), Some(400));
        assert_eq!(parse_weight("9000"), None);
    }

    #[test]
    fn border_shorthand_falls_back_to_tokens() {
        let mut map = BTreeMap::new();
        map.insert("border".to_string(), "1px solid #ff0000".to_string());
        let border = parse_border(&map).unwrap();
        assert_eq!(border.width, 1.0);
        assert_eq!(border.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn frame_uses_document_background_and_layout_rects() {
        let mut app = App::new(doc(r##"{"background":"#090b14"}"##));
        let frame = app.frame(1.0, (400, 300));
        assert_eq!(frame.background, [9.0 / 255.0, 11.0 / 255.0, 20.0 / 255.0, 1.0]);
        assert_eq!(frame.width, 400);
        assert_eq!(frame.height, 300);
        // Layout places the root at the viewport; it paints exactly one rect (the document
        // background color as its fill) and has no text.
        assert_eq!(frame.ops.len(), 1);
        match &frame.ops[0] {
            DrawOp::Rect(shape) => {
                assert_eq!(shape.node, 1);
                assert_eq!(shape.fill, Some([9.0 / 255.0, 11.0 / 255.0, 20.0 / 255.0, 1.0]));
            }
            _ => panic!("expected a rect op for the root node"),
        }
        // The pre-shape frame is cached for hit testing.
        assert!(app.hit_frame.is_some());
    }

    #[test]
    fn frame_emits_text_ops_from_layout_text_styles() {
        // A nested text node styled by the document; layout resolves its box and text style.
        let json = r##"{"format":"tsgpu-1","name":"app","title":"App","width":400,"height":300,
            "backend_js":"",
            "root":{"tag":"div","key":1,"children":[
                {"tag":"h1","key":2,"text":"TSGPUI","style":{"font-size":"48px","color":"#ffffff"}}
            ]}}"##;
        let mut app = App::new(serde_json::from_str::<AppPackage>(json).unwrap());
        let frame = app.frame(1.0, (400, 300));
        // Shaping replaces Text ops with Glyphs batches in the returned frame, so the TextRun
        // styling is asserted on the cached pre-shape copy used for hit testing.
        let text_ops: Vec<&TextRun> = app
            .hit_frame
            .as_ref()
            .unwrap()
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Text(run) => Some(run),
                _ => None,
            })
            .collect();
        assert_eq!(text_ops.len(), 1);
        let run = text_ops[0];
        assert_eq!(run.node, 2);
        assert_eq!(run.text, "TSGPUI");
        assert_eq!(run.size, 48.0);
        assert_eq!(run.color, [1.0, 1.0, 1.0, 1.0]);
        // The baseline comes from layout's estimate inside the node's box.
        assert!(run.y > 0.0 && run.max_width > 0.0);
        // And the final frame carries the shaped glyph batch (one instance per character).
        assert!(frame.ops.iter().any(|op| matches!(
            op,
            DrawOp::Glyphs(batch) if batch.instances.len() == "TSGPUI".len()
        )));
    }

    #[test]
    fn pointer_and_key_are_safe_before_first_frame() {
        let mut app = App::new(doc("{}"));
        app.pointer(input::PointerEvent::Move { x: 10.0, y: 10.0 });
        app.pointer(input::PointerEvent::Down { x: 10.0, y: 10.0 });
        app.pointer(input::PointerEvent::Up { x: 10.0, y: 10.0 });
        app.key(input::KeyEvent::Character('x'));
        assert_eq!(app.hovered, None);
        assert_eq!(app.active, None);
        assert_eq!(app.values.len(), 0);
    }

    #[test]
    fn atlas_comes_from_the_text_system() {
        let app = App::new(doc("{}"));
        let atlas = app.atlas();
        assert_eq!(atlas.generation, 0);
    }
}

#[cfg(test)]
mod diagnostic {
    use super::*;

    #[test]
    fn diagnostic_counter_bundle_rects_and_glyphs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/counter/dist/tsgpu-counter.tgp");
        let json = std::fs::read_to_string(&path).expect("counter bundle");
        let doc: crate::package::AppPackage = serde_json::from_str(&json).expect("parse bundle");
        let mut app = App::new(doc);
        let frame = app.frame(1.0, (900, 600));
        if let Some(hit) = &app.hit_frame {
            for op in hit.ops.iter() {
                if let DrawOp::Text(run) = op {
                    println!("textrun node={} x={:.1} y={:.1} mw={:.1} size={:.1} text={:?}", run.node, run.x, run.y, run.max_width, run.size, run.text.chars().take(12).collect::<String>());
                }
            }
        }
        let text_ops = frame.ops.iter().filter(|op| matches!(op, DrawOp::Text(_))).count();
        let rect_ops = frame.ops.iter().filter(|op| matches!(op, DrawOp::Rect(_))).count();
        let mut glyph_instances = 0usize;
        for op in &frame.ops {
            if let DrawOp::Glyphs(batch) = op { glyph_instances += batch.instances.len(); }
        }
        println!("rect_ops={rect_ops} text_ops={text_ops} glyph_instances={glyph_instances}"); let atlas = app.atlas(); println!("atlas: {}x{} gen={} bytes={}", atlas.width, atlas.height, atlas.generation, atlas.rgba.len()); let mut batch_i = 0usize; for op in frame.ops.iter() { if let DrawOp::Glyphs(batch) = op { if batch_i < 8 { if let Some(first) = batch.instances.first() { println!("batch {batch_i}: n={} first pos=({:.1},{:.1}) size=({:.1},{:.1})", batch.instances.len(), first.pos[0], first.pos[1], first.size[0], first.size[1]); } } batch_i += 1; } }
        for (key, rect) in frame.ops.iter().filter_map(|op| match op {
            DrawOp::Rect(s) if s.node == 2 || s.node == 3 || s.node == 4 || s.node == 19 => Some((s.node, s.rect)),
            _ => None,
        }) {
            println!("node {key}: x={:.1} y={:.1} w={:.1} h={:.1}", rect.x, rect.y, rect.w, rect.h);
        }
    }
}

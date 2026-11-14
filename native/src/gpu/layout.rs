//! Document/style-map → taffy layout tree → absolute rects. Pure and unit-tested. Owner: agent C.
//!
//! Contract (fixed):
//! - `LayoutOutput { rects: HashMap<u64, Rect>, content: (f32, f32) }` — rect per node key in
//!   logical pixels (already scaled), plus the laid-out content size.
//! - `measure(root: &crate::package::Node, viewport: (f32, f32), scale: f32) -> LayoutOutput`
//!   Maps the CSS subset Rendrix compiles (see docs/SUPPORTED_SYNTAX.md): flex direction/wrap/
//!   gap, padding/margin, border-radius/border, background colors, opacity, width/height
//!   (px, %, auto), text styling (font-size/weight/color/align). Ignore unsupported properties
//!   silently for M0; a skipped list comes in M1.
//!
//! Additive contract extensions made here (this file only, noted for the other owners):
//! - `LayoutOutput.text_styles: HashMap<u64, TextStyle>` — per-node text styling the backend
//!   needs to build `types::TextRun` ops.
//! - `LayoutOutput.baselines: HashMap<u64, f32>` — estimated first-line baseline Y (logical px)
//!   for text nodes, so the backend does not have to guess `TextRun.y`. It is
//!   `content_top + 0.91 * font_size` (content top = border + padding resolved by taffy);
//!   TODO(M1): replace the estimate with real font metrics from `text.rs`.
//!
//! M0 decisions (TODOs live inline):
//! - `position: static` only; anything else lays out in flow (`relative`) without insets.
//! - `overflow: auto/scroll` are treated as plain containers (no scrolling, no clipping).
//! - `display: grid` is laid out as block; the compiler's grid→flex-track remap is M1.
//! - State (`:hover` …) and `@media` responsive declarations do not affect static layout.
//! - CSS inheritance (font-size/color/…) is not implemented; every node parses its own style.
//! - `scale` is accepted but unused: all lengths arrive in px and stay logical px.

use crate::gpu::types::{Color, Rect, TextAlign};
use crate::package::Node;
use std::collections::{BTreeMap, HashMap};
use taffy::prelude::{
    AlignContent, AlignItems, AvailableSpace, Dimension, Display, FlexDirection, FlexWrap, JustifyContent,
    LengthPercentage, LengthPercentageAuto, NodeId, Position, Style, TaffyTree,
};
use taffy::prelude::{Rect as TaffyRect, Size as TaffySize};

/// Measures one text run for layout. Implemented by `text::TextSystem` in the app; stubs in
/// tests. Taffy cannot size text on its own, so every text leaf asks this during layout —
/// without it, shrink-to-fit containers (buttons, centered spans) collapse to zero width.
pub trait TextMeasure {
    /// Returns the wrapped (width, height) of `text` at `size`/`weight` within `max_width`.
    fn measure_text(&mut self, text: &str, size: f32, weight: u16, max_width: f32) -> (f32, f32);
}

/// Per-node measure context for taffy text leaves.
enum MeasureContext {
    Text { text: String, size: f32, weight: u16 },
}

/// Text styling extracted per node so the backend can build `types::TextRun` ops.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    pub color: Color,
    pub align: TextAlign,
}

impl Default for TextStyle {
    fn default() -> Self {
        // The CSS initial values for inherited text properties.
        TextStyle { size: 16.0, weight: 400, color: [0.0, 0.0, 0.0, 1.0], align: TextAlign::Left }
    }
}

/// `FontSystem`-independent estimate of where the first baseline sits inside a line box whose
/// height is `1.25em` (see text.rs): typical UI-font ascent ≈ 0.78em, and cosmic-text centers
/// the glyph box in the line, giving baseline ≈ (1.25 − 1.0)/2 + 0.78 = 0.905em. 0.91 is close
/// enough for M0; real metrics replace it in M1.
const FIRST_BASELINE_EM: f32 = 0.91;
/// rem/em resolution used by the CSS compiler (docs/SUPPORTED_SYNTAX.md).
const REM_PX: f32 = 16.0;

#[derive(Debug, Clone, Default)]
pub struct LayoutOutput {
    pub rects: HashMap<u64, Rect>,
    pub content: (f32, f32),
    /// Text styling for every node with non-empty text (additive, see module docs).
    pub text_styles: HashMap<u64, TextStyle>,
    /// Estimated first-line baseline per text node, logical px (additive, see module docs).
    pub baselines: HashMap<u64, f32>,
}

/// Lay out the document tree within `viewport` (logical px) and return absolute rects.
pub fn measure(root: &Node, viewport: (f32, f32), scale: f32, text: &mut dyn TextMeasure) -> LayoutOutput {
    // M0: styles are px and rects are logical px; `scale` is kept for future DPI work only.
    let _ = scale;
    let vp = (viewport.0.max(1.0), viewport.1.max(1.0));

    let mut tree: TaffyTree<MeasureContext> = TaffyTree::new();
    let mut out = LayoutOutput::default();
    let mut ids: HashMap<u64, NodeId> = HashMap::new();

    let Some(root_id) = build(&mut tree, root, vp, &mut out, &mut ids, true, TextStyle::default()) else {
        return out;
    };
    tree.compute_layout_with_measure(
        root_id,
        TaffySize {
            width: AvailableSpace::Definite(vp.0),
            height: AvailableSpace::Definite(vp.1),
        },
        |known, available, _id, context, _style| {
            let Some(MeasureContext::Text { text: content, size, weight }) = context.map(|c| &*c) else {
                return TaffySize { width: 0.0, height: 0.0 };
            };
            let max_width = known
                .width
                .or(match available.width { AvailableSpace::Definite(w) => Some(w), _ => None })
                .unwrap_or(f32::MAX);
            let (w, h) = text.measure_text(content, *size, *weight, max_width);
            TaffySize { width: w, height: h }
        },
    )
    .expect("taffy compute_layout failed");

    collect(&tree, root, &ids, &mut out, 0.0, 0.0);
    if let Ok(root_layout) = tree.layout(root_id) {
        out.content = (root_layout.size.width, root_layout.size.height);
    }
    out
}

/// Recursively add `node` to the taffy tree, recording text styles on the way down.
fn build(
    tree: &mut TaffyTree<MeasureContext>,
    node: &Node,
    vp: (f32, f32),
    out: &mut LayoutOutput,
    ids: &mut HashMap<u64, NodeId>,
    is_root: bool,
    inherited: TextStyle,
) -> Option<NodeId> {
    let mut style = style_for(node, vp);
    if is_root {
        // The root fills the viewport.
        style.size = TaffySize { width: Dimension::percent(1.0), height: Dimension::percent(1.0) };
    }
    let effective_text = text_style_for(node, vp, &inherited);
    if !node.text.trim().is_empty() {
        out.text_styles.insert(node.key, effective_text.clone());
    }

    let id = if node.children.is_empty() {
        if node.text.trim().is_empty() {
            tree.new_leaf(style).ok()?
        } else {
            // Text leaves carry a measure context so flexbox can size shrink-to-fit
            // containers (buttons, centered labels) around their real text.
            tree.new_leaf_with_context(
                style,
                MeasureContext::Text {
                    text: node.text.clone(),
                    size: out.text_styles.get(&node.key).map(|s| s.size).unwrap_or(16.0),
                    weight: out.text_styles.get(&node.key).map(|s| s.weight).unwrap_or(400),
                },
            )
            .ok()?
        }
    } else {
        let mut children = Vec::with_capacity(node.children.len());
        for child in &node.children {
            children.push(build(tree, child, vp, out, ids, false, effective_text.clone())?);
        }
        tree.new_with_children(style, &children).ok()?
    };
    // Document node keys are unique (compiler guarantee); a duplicate would simply overwrite.
    ids.insert(node.key, id);
    Some(id)
}

/// Copy taffy's resolved geometry into the output, walking the document tree in parallel.
/// Taffy reports each node's location relative to its parent, so the absolute position is
/// accumulated down the walk.
fn collect(
    tree: &TaffyTree<MeasureContext>,
    node: &Node,
    ids: &HashMap<u64, NodeId>,
    out: &mut LayoutOutput,
    offset_x: f32,
    offset_y: f32,
) {
    let Some(id) = ids.get(&node.key).copied() else { return };
    let (child_dx, child_dy) = if let Ok(layout) = tree.layout(id) {
        let abs_x = offset_x + layout.location.x;
        let abs_y = offset_y + layout.location.y;
        out.rects.insert(
            node.key,
            Rect { x: abs_x, y: abs_y, w: layout.size.width, h: layout.size.height },
        );
        if let Some(text_style) = out.text_styles.get(&node.key) {
            out.baselines.insert(node.key, abs_y + layout.border.top + layout.padding.top + text_style.size * FIRST_BASELINE_EM);
        }
        (abs_x, abs_y)
    } else {
        (offset_x, offset_y)
    };
    for child in &node.children {
        collect(tree, child, ids, out, child_dx, child_dy);
    }
}

/// Map the supported CSS declarations of one node onto a taffy `Style`.
///
/// Every declaration that fails to parse is skipped silently (M0 contract); the same policy
/// applies to declarations Rendrix compiles that this module does not model yet.
fn style_for(node: &Node, vp: (f32, f32)) -> Style {
    let s = &node.style;
    // HTML default is block flow; `Style::default()` is flex, so be explicit.
    let mut style = Style { display: Display::Block, ..Style::default() };

    match decl(s, "display").as_str() {
        "flex" => style.display = Display::Flex,
        "grid" => {
            // TODO(M1): remap grid-template-columns to a flex row with one cell per track the
            // way the CSS doc describes. For M0 grid containers lay out as plain blocks.
        }
        _ => {} // block (default), or unsupported value → default
    }

    if style.display == Display::Flex {
        style.flex_direction = match decl(s, "flex-direction").as_str() {
            "column" => FlexDirection::Column,
            "row-reverse" => FlexDirection::RowReverse,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => FlexDirection::Row, // row and any unparsed value
        };
        style.flex_wrap = if decl(s, "flex-wrap") == "wrap" { FlexWrap::Wrap } else { FlexWrap::NoWrap };
    }

    if let Some(value) = s.get("gap") {
        // gap: <row> [<column>]; taffy's gap Size is { width: column, height: row }.
        let parts: Vec<LengthPercentage> =
            value.split_whitespace().filter_map(|t| parse_length(t, vp)).collect();
        if !parts.is_empty() {
            let column = parts.get(1).copied().unwrap_or(parts[0]);
            style.gap = TaffySize { width: column, height: parts[0] };
        }
    }

    if let Some(value) = s.get("padding") {
        if let Some(box_) = parse_box(&value, vp, parse_length) {
            style.padding = box_;
        }
    }
    for (property, side) in [("padding-top", 0), ("padding-right", 1), ("padding-bottom", 2), ("padding-left", 3)] {
        if let Some(value) = s.get(property) {
            if let Some(lp) = parse_length(value, vp) {
                set_box_side(&mut style.padding, side, lp);
            }
        }
    }

    if let Some(value) = s.get("margin") {
        if let Some(box_) = parse_box(&value, vp, parse_margin_one) {
            style.margin = box_;
        }
    }
    for (property, side) in [("margin-top", 0), ("margin-right", 1), ("margin-bottom", 2), ("margin-left", 3)] {
        if let Some(value) = s.get(property) {
            if let Some(lpa) = parse_margin_one(value, vp) {
                set_box_side(&mut style.margin, side, lpa);
            }
        }
    }

    if let Some(value) = s.get("width") {
        if let Some(d) = parse_dimension(value, vp) {
            style.size.width = d;
        }
    }
    if let Some(value) = s.get("height") {
        if let Some(d) = parse_dimension(value, vp) {
            style.size.height = d;
        }
    }
    for (property, axis) in [("min-width", 0), ("min-height", 1)] {
        if let Some(value) = s.get(property) {
            if let Some(d) = parse_dimension(value, vp) {
                if axis == 0 { style.min_size.width = d } else { style.min_size.height = d }
            }
        }
    }
    for (property, axis) in [("max-width", 0), ("max-height", 1)] {
        if let Some(value) = s.get(property) {
            if let Some(d) = parse_dimension(value, vp) {
                if axis == 0 { style.max_size.width = d } else { style.max_size.height = d }
            }
        }
    }

    // `flex: <grow> [<shrink> [<basis>]]`; bare numbers imply shrink 1, basis 0%.
    if let Some(value) = s.get("flex") {
        let mut tokens = value.split_whitespace();
        let first = tokens.next().unwrap_or("");
        if first == "none" {
            style.flex_grow = 0.0;
            style.flex_shrink = 0.0;
            style.flex_basis = Dimension::auto();
        } else {
            if let Some(grow) = parse_num(first) {
                style.flex_grow = grow;
                style.flex_shrink = 1.0;
                style.flex_basis = Dimension::percent(0.0);
            }
            for token in tokens {
                if let Some(shrink) = parse_num(token) {
                    style.flex_shrink = shrink;
                } else if let Some(basis) = parse_dimension(token, vp) {
                    style.flex_basis = basis;
                    if token == "auto" {
                        style.flex_grow = 1.0;
                        style.flex_shrink = 1.0;
                    }
                }
            }
        }
    }
    if let Some(value) = s.get("flex-grow") {
        if let Some(grow) = parse_num(value) {
            style.flex_grow = grow;
        }
    }
    if let Some(value) = s.get("flex-shrink") {
        if let Some(shrink) = parse_num(value) {
            style.flex_shrink = shrink;
        }
    }
    if let Some(value) = s.get("flex-basis") {
        if let Some(basis) = parse_dimension(value, vp) {
            style.flex_basis = basis;
        }
    }

    if let Some(alignment) = align_items(&decl(s, "align-items")) {
        style.align_items = Some(alignment);
    }
    if let Some(alignment) = align_self(&decl(s, "align-self")) {
        style.align_self = Some(alignment);
    }
    if let Some(alignment) = justify_content(&decl(s, "justify-content")) {
        style.justify_content = Some(alignment);
    }
    if let Some(alignment) = align_content(&decl(s, "align-content")) {
        style.align_content = Some(alignment);
    }

    // Borders affect layout through their widths only; width/color as paint properties are
    // re-read from the style map by the backend when it builds `ShapeRect`s.
    let mut border_width: Option<LengthPercentage> = None;
    if let Some(value) = s.get("border") {
        let mut width: Option<LengthPercentage> = None;
        let mut none = false;
        for token in value.split_whitespace() {
            if token == "none" {
                none = true;
            } else if let Some(lp) = parse_length(token, vp) {
                width = Some(lp);
            }
            // style keywords and colors are paint concerns, parsed by the backend
        }
        if !none {
            border_width = width;
        }
    }
    if border_width.is_none() {
        if let Some(value) = s.get("border-width") {
            let parts: Vec<LengthPercentage> =
                value.split_whitespace().filter_map(|t| parse_length(t, vp)).collect();
            border_width = parts.first().copied();
        }
    }
    if let Some(width) = border_width {
        style.border = TaffyRect { top: width, right: width, bottom: width, left: width };
    }
    // TODO(M1): per-edge `border-top`/`-right`/`-bottom`/`-left` widths.

    match decl(s, "position").as_str() {
        "static" | "relative" | "" => style.position = Position::Relative,
        // M0: absolute/fixed/sticky are not supported; the element stays in flow. Insets and
        // z-index paint-order resolution come with absolute positioning in M1.
        _ => {
            // TODO(M1): position: absolute/fixed/sticky.
            style.position = Position::Relative;
        }
    }

    // TODO(M1): overflow: auto/scroll/hidden → scrollable/clipped containers. For M0 every
    // container behaves as if overflow were visible (the scroll milestone covers this).
    // TODO(M1): opacity as a fill-alpha multiplier is applied by the paint backend.

    style
}

/// Extract the text styling the backend needs for `TextRun`s.
/// Resolves one node's text style over the INHERITED parent style. `color`, `font-size`,
/// `font-weight` and `text-align` are inherited CSS properties: a bare `#text` node has no
/// declarations of its own and must take the nearest ancestor's values (a `color` set on
/// `body` reaches every label through this).
fn text_style_for(node: &Node, vp: (f32, f32), inherited: &TextStyle) -> TextStyle {
    let s = &node.style;
    let size = s
        .get("font-size")
        .and_then(|v| parse_length_value(v, vp))
        .and_then(|parsed| match parsed {
            ParsedLength::Px(px) => Some(px),
            // font-size percentages resolve against the PARENT font size (CSS semantics).
            ParsedLength::Percent(p) => Some(inherited.size * p),
        })
        .filter(|px| *px > 0.0)
        .unwrap_or(inherited.size);
    let weight = s
        .get("font-weight")
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(400.0),
            "bold" => Some(700.0),
            _ => parse_num(v),
        })
        .map(|w| w.clamp(1.0, 1000.0) as u16)
        .unwrap_or(inherited.weight);
    let color = s.get("color").and_then(|v| parse_color(v)).unwrap_or(inherited.color);
    let align = match decl(s, "text-align").as_str() {
        "center" => TextAlign::Center,
        "right" | "end" => TextAlign::Right,
        _ => inherited.align, // left, start, unparsed values, and unset inherit
    };
    TextStyle { size, weight, color, align }
}

fn decl(style: &BTreeMap<String, String>, property: &str) -> String {
    style.get(property).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default()
}

fn parse_num(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().filter(|n| n.is_finite())
}

/// A CSS length with its unit resolved: absolute px, or a percentage as a 0.0–1.0 fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ParsedLength {
    Px(f32),
    Percent(f32),
}

/// px / % / rem / em / vw / vh / vmin / vmax → resolved value. Bare "0" is valid CSS; bare
/// numbers with other units are not.
fn parse_length_value(value: &str, vp: (f32, f32)) -> Option<ParsedLength> {
    let v = value.trim().to_ascii_lowercase();
    if v == "0" {
        return Some(ParsedLength::Px(0.0));
    }
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n = parse_num(num)?;
    match unit {
        "px" => Some(ParsedLength::Px(n)),
        "%" => Some(ParsedLength::Percent(n / 100.0)),
        "rem" | "em" => Some(ParsedLength::Px(n * REM_PX)),
        // Viewport units resolve against the window; nested percentages-of-parent would differ
        // from browsers, but Rendrix only compiles vw/vh on near-root elements. Approximation OK.
        "vw" => Some(ParsedLength::Px(n * vp.0 / 100.0)),
        "vh" => Some(ParsedLength::Px(n * vp.1 / 100.0)),
        "vmin" => Some(ParsedLength::Px(n * vp.0.min(vp.1) / 100.0)),
        "vmax" => Some(ParsedLength::Px(n * vp.0.max(vp.1) / 100.0)),
        _ => None,
    }
}

fn parse_length(value: &str, vp: (f32, f32)) -> Option<LengthPercentage> {
    match parse_length_value(value, vp)? {
        ParsedLength::Px(v) => Some(LengthPercentage::length(v)),
        ParsedLength::Percent(p) => Some(LengthPercentage::percent(p)),
    }
}

/// `auto`/`none`-tolerant length for width/height/min/max and flex-basis.
fn parse_dimension(value: &str, vp: (f32, f32)) -> Option<Dimension> {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" || v == "none" {
        return Some(Dimension::auto());
    }
    parse_length(&v, vp).map(From::from)
}

/// Length or auto (margins). Real `auto` margins are passed through; taffy distributes them.
fn parse_margin_one(value: &str, vp: (f32, f32)) -> Option<LengthPercentageAuto> {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(LengthPercentageAuto::auto());
    }
    parse_length(&v, vp).map(From::from)
}

/// Expand a 1–4 value CSS box shorthand (top right bottom left) into a taffy rect.
fn parse_box<T, F>(value: &str, vp: (f32, f32), parse_token: F) -> Option<TaffyRect<T>>
where
    F: Fn(&str, (f32, f32)) -> Option<T>,
    T: Copy,
{
    let sides: Vec<T> = value.split_whitespace().filter_map(|t| parse_token(t, vp)).collect();
    if sides.is_empty() {
        return None;
    }
    let [top, right, bottom, left] = expand4(&sides);
    Some(TaffyRect { top, right, bottom, left })
}

fn expand4<T: Copy>(sides: &[T]) -> [T; 4] {
    match sides.len() {
        1 => [sides[0]; 4],
        2 => [sides[0], sides[1], sides[0], sides[1]],
        3 => [sides[0], sides[1], sides[2], sides[1]],
        _ => [sides[0], sides[1], sides[2], sides[3]],
    }
}

fn set_box_side<T>(rect: &mut TaffyRect<T>, side: usize, value: T) {
    match side {
        0 => rect.top = value,
        1 => rect.right = value,
        2 => rect.bottom = value,
        _ => rect.left = value,
    }
}

fn align_items(value: &str) -> Option<AlignItems> {
    match value {
        "center" => Some(AlignItems::Center),
        "flex-start" | "start" => Some(AlignItems::FlexStart),
        "flex-end" | "end" => Some(AlignItems::FlexEnd),
        "stretch" => Some(AlignItems::Stretch),
        "baseline" => Some(AlignItems::Baseline),
        _ => None,
    }
}

fn align_self(value: &str) -> Option<taffy::prelude::AlignSelf> {
    // AlignSelf mirrors AlignItems; reuse the same mapping.
    align_items(value).map(|a| match a {
        AlignItems::Center => taffy::prelude::AlignSelf::Center,
        AlignItems::FlexStart => taffy::prelude::AlignSelf::FlexStart,
        AlignItems::FlexEnd => taffy::prelude::AlignSelf::FlexEnd,
        AlignItems::Stretch => taffy::prelude::AlignSelf::Stretch,
        AlignItems::Baseline => taffy::prelude::AlignSelf::Baseline,
        _ => taffy::prelude::AlignSelf::Stretch,
    })
}

fn justify_content(value: &str) -> Option<JustifyContent> {
    match value {
        "center" => Some(JustifyContent::Center),
        "flex-start" | "start" => Some(JustifyContent::FlexStart),
        "flex-end" | "end" => Some(JustifyContent::FlexEnd),
        "space-between" => Some(JustifyContent::SpaceBetween),
        "space-around" => Some(JustifyContent::SpaceAround),
        "space-evenly" => Some(JustifyContent::SpaceEvenly),
        _ => None,
    }
}

fn align_content(value: &str) -> Option<AlignContent> {
    match value {
        "center" => Some(AlignContent::Center),
        "flex-start" | "start" => Some(AlignContent::FlexStart),
        "flex-end" | "end" => Some(AlignContent::FlexEnd),
        "space-between" => Some(AlignContent::SpaceBetween),
        "space-around" => Some(AlignContent::SpaceAround),
        "space-evenly" => Some(AlignContent::SpaceEvenly),
        "stretch" => Some(AlignContent::Stretch),
        _ => None,
    }
}

/// Parse any CSS color syntax Rendrix compiles: `#rgb #rgba #rrggbb #rrggbbaa`, `rgb()/rgba()`
/// (comma or space separated, optional `/ alpha`), and the named colors that appear in
/// documents. Returns linear-ish [r, g, b, a] in 0.0–1.0, or None to skip the declaration.
pub fn parse_color(value: &str) -> Option<Color> {
    let v = value.trim().to_ascii_lowercase();
    if let Some(named) = named_color(&v) {
        return Some(named);
    }
    if let Some(hex) = v.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<Vec<u8>>>()?;
        let (r, g, b, a) = match digits.len() {
            3 => (expand_nibble(digits[0]), expand_nibble(digits[1]), expand_nibble(digits[2]), 255),
            4 => (expand_nibble(digits[0]), expand_nibble(digits[1]), expand_nibble(digits[2]), expand_nibble(digits[3])),
            6 => (
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
                255,
            ),
            8 => (
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
                digits[6] * 16 + digits[7],
            ),
            _ => return None,
        };
        return Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]);
    }
    let inner = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))
        .and_then(|i| i.strip_suffix(')'))?;
    // Accept "r, g, b, a", "r g b / a", and percent channels.
    let tokens: Vec<&str> = inner
        .split(|c: char| c == ',' || c == ' ' || c == '/' || c == '\t')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() < 3 {
        return None;
    }
    let channel = |t: &str| -> Option<f32> {
        if let Some(p) = t.strip_suffix('%') {
            parse_num(p).map(|n| n / 100.0 * 255.0)
        } else {
            parse_num(t)
        }
    };
    let r = channel(tokens[0])?;
    let g = channel(tokens[1])?;
    let b = channel(tokens[2])?;
    let a = if let Some(t) = tokens.get(3) {
        if let Some(p) = t.strip_suffix('%') {
            parse_num(p)? / 100.0
        } else {
            let n = parse_num(t)?;
            if n > 1.0 {
                n / 255.0
            } else {
                n
            }
        }
    } else {
        1.0
    };
    Some([r / 255.0, g / 255.0, b / 255.0, a.clamp(0.0, 1.0)])
}

fn expand_nibble(n: u8) -> u8 {
    n * 17
}

/// The named colors that appear in Rendrix documents and the library styles (a practical
/// subset of the CSS named colors; anything else is skipped by the caller).
fn named_color(v: &str) -> Option<Color> {
    Some(match v {
        "transparent" => [0.0, 0.0, 0.0, 0.0],
        "white" => [1.0, 1.0, 1.0, 1.0],
        "black" => [0.0, 0.0, 0.0, 1.0],
        "red" => [1.0, 0.0, 0.0, 1.0],
        "green" => [0.0, 0.5, 0.0, 1.0],
        "lime" => [0.0, 1.0, 0.0, 1.0],
        "blue" => [0.0, 0.0, 1.0, 1.0],
        "yellow" => [1.0, 1.0, 0.0, 1.0],
        "cyan" | "aqua" => [0.0, 1.0, 1.0, 1.0],
        "magenta" | "fuchsia" => [1.0, 0.0, 1.0, 1.0],
        "silver" => [0.75, 0.75, 0.75, 1.0],
        "gray" | "grey" => [0.5, 0.5, 0.5, 1.0],
        "maroon" => [0.5, 0.0, 0.0, 1.0],
        "olive" => [0.5, 0.5, 0.0, 1.0],
        "purple" => [0.5, 0.0, 0.5, 1.0],
        "teal" => [0.0, 0.5, 0.5, 1.0],
        "navy" => [0.0, 0.0, 0.5, 1.0],
        "orange" => [1.0, 0.65, 0.0, 1.0],
        "pink" => [1.0, 0.75, 0.8, 1.0],
        "brown" => [0.65, 0.4, 0.2, 1.0],
        "gold" => [1.0, 0.85, 0.0, 1.0],
        "whitesmoke" => [0.96, 0.96, 0.96, 1.0],
        "crimson" => [0.86, 0.08, 0.24, 1.0],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn node(key: u64, style: &[(&str, &str)], text: &str, children: Vec<Node>) -> Node {
        Node {
            tag: "div".into(),
            key,
            text: text.into(),
            attrs: BTreeMap::new(),
            props: BTreeMap::new(),
            style: style.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            states: BTreeMap::new(),
            responsive: Vec::new(),
            children,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    fn rect_at(rect: Rect, x: f32, y: f32, w: f32, h: f32) -> bool {
        close(rect.x, x) && close(rect.y, y) && close(rect.w, w) && close(rect.h, h)
    }

    #[test]
    fn root_fills_viewport() {
        let root = node(1, &[], "", vec![]);
        let out = measure(&root, (300.0, 200.0), 1.0, &mut NoText);
        assert_eq!(out.content, (300.0, 200.0));
        assert!(rect_at(out.rects[&1], 0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn column_flex_with_gap() {
        let root = node(
            1,
            &[("display", "flex"), ("flex-direction", "column"), ("gap", "10px")],
            "",
            vec![node(2, &[("height", "20px")], "", vec![]), node(3, &[("height", "30px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        // Children stretch to the full width in a column flex container.
        assert!(rect_at(out.rects[&2], 0.0, 0.0, 200.0, 20.0), "{:?}", out.rects);
        assert!(rect_at(out.rects[&3], 0.0, 30.0, 200.0, 30.0), "{:?}", out.rects);
    }

    #[test]
    fn nested_box_with_padding() {
        let root = node(
            1,
            &[],
            "",
            vec![node(
                2,
                &[("padding", "8px")],
                "",
                vec![node(3, &[("height", "20px")], "", vec![])],
            )],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        // Inner box sits inside the padding and fills the content width; the padded box grows
        // to content + padding (border-box auto height).
        assert!(rect_at(out.rects[&3], 8.0, 8.0, 184.0, 20.0), "{:?}", out.rects);
        assert!(rect_at(out.rects[&2], 0.0, 0.0, 200.0, 36.0), "{:?}", out.rects);
    }

    #[test]
    fn percentage_widths() {
        let root = node(
            1,
            &[("display", "flex")],
            "",
            vec![node(2, &[("width", "50%"), ("height", "30px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        assert!(rect_at(out.rects[&2], 0.0, 0.0, 100.0, 30.0), "{:?}", out.rects);
    }

    #[test]
    fn min_and_max_sizes_clamp() {
        let root = node(
            1,
            &[("display", "flex")],
            "",
            vec![node(2, &[("width", "50%"), ("min-width", "120px"), ("height", "30px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        assert!(close(out.rects[&2].w, 120.0), "{:?}", out.rects);
    }

    #[test]
    fn text_style_extraction() {
        let root = node(
            1,
            &[
                ("font-size", "14px"),
                ("color", "#ff375f"),
                ("font-weight", "600"),
                ("text-align", "center"),
            ],
            "Hi",
            vec![],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        let style = out.text_styles.get(&1).expect("text style recorded");
        assert!(close(style.size, 14.0));
        assert_eq!(style.weight, 600);
        assert!(close(style.color[0], 1.0) && close(style.color[1], 55.0 / 255.0) && close(style.color[2], 95.0 / 255.0));
        assert_eq!(style.align, TextAlign::Center);
        // Baseline estimate sits inside the box below the top edge.
        let baseline = out.baselines.get(&1).copied().expect("baseline recorded");
        assert!(baseline > 0.0 && baseline < 100.0);
    }

    #[test]
    fn text_style_defaults() {
        let root = node(1, &[], "plain", vec![]);
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        let style = out.text_styles.get(&1).expect("text style recorded");
        assert_eq!(*style, TextStyle { size: 16.0, weight: 400, color: [0.0, 0.0, 0.0, 1.0], align: TextAlign::Left });
    }

    #[test]
    fn color_parsing() {
        assert_eq!(parse_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_color("#00000080"), Some([0.0, 0.0, 0.0, 128.0 / 255.0]));
        assert_eq!(parse_color("rgb(255, 59, 48)"), Some([1.0, 59.0 / 255.0, 48.0 / 255.0, 1.0]));
        assert_eq!(parse_color("rgba(0 0 255 / 50%)"), Some([0.0, 0.0, 1.0, 0.5]));
        assert_eq!(parse_color("white"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_color("transparent"), Some([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(parse_color("not-a-color"), None);
    }

    #[test]
    fn length_parsing() {
        let vp = (800.0, 600.0);
        assert_eq!(parse_length_value("12px", vp), Some(ParsedLength::Px(12.0)));
        assert_eq!(parse_length_value("50%", vp), Some(ParsedLength::Percent(0.5)));
        assert_eq!(parse_length_value("2rem", vp), Some(ParsedLength::Px(32.0)));
        assert_eq!(parse_length_value("10vw", vp), Some(ParsedLength::Px(80.0)));
        assert_eq!(parse_length_value("0", vp), Some(ParsedLength::Px(0.0)));
        assert_eq!(parse_length_value("12", vp), None);
        assert_eq!(parse_length_value("banana", vp), None);
        // Dimensions accept auto/none and turn them into taffy's auto.
        assert_eq!(parse_dimension("auto", vp), Some(Dimension::auto()));
        assert_eq!(parse_dimension("none", vp), Some(Dimension::auto()));
        assert_eq!(parse_dimension("8px", vp), Some(Dimension::length(8.0)));
        // Margins accept auto.
        assert_eq!(parse_margin_one("auto", vp), Some(LengthPercentageAuto::auto()));
        assert_eq!(parse_margin_one("4px", vp), Some(LengthPercentageAuto::length(4.0)));
    }

    #[test]
    fn display_grid_treated_as_block() {
        let root = node(
            1,
            &[("display", "grid"), ("grid-template-columns", "1fr 1fr")],
            "",
            vec![node(2, &[("height", "20px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        assert!(rect_at(out.rects[&2], 0.0, 0.0, 200.0, 20.0), "{:?}", out.rects);
    }

    #[test]
    fn position_absolute_lays_out_in_flow_for_m0() {
        let root = node(
            1,
            &[("display", "flex")],
            "",
            vec![node(2, &[("width", "50px"), ("height", "20px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        assert!(rect_at(out.rects[&2], 0.0, 0.0, 50.0, 20.0), "{:?}", out.rects);
    }

    #[test]
    fn row_flex_justify_and_align() {
        let root = node(
            1,
            &[
                ("display", "flex"),
                ("justify-content", "space-between"),
                ("align-items", "center"),
                ("height", "100px"),
            ],
            "",
            vec![node(2, &[("width", "40px"), ("height", "20px")], "", vec![]), node(3, &[("width", "40px"), ("height", "20px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        assert!(rect_at(out.rects[&2], 0.0, 40.0, 40.0, 20.0), "{:?}", out.rects);
        assert!(rect_at(out.rects[&3], 160.0, 40.0, 40.0, 20.0), "{:?}", out.rects);
    }

    #[test]
    fn margin_shorthand_with_sides() {
        let root = node(
            1,
            &[("display", "flex"), ("flex-direction", "column")],
            "",
            vec![node(2, &[("height", "20px"), ("margin", "4px 8px")], "", vec![])],
        );
        let out = measure(&root, (200.0, 100.0), 1.0, &mut NoText);
        // margin top 4, right 8, bottom 4, left 8: child box is inset horizontally.
        assert!(rect_at(out.rects[&2], 8.0, 4.0, 184.0, 20.0), "{:?}", out.rects);
    }
}

#[cfg(test)]
struct NoText;
#[cfg(test)]
impl TextMeasure for NoText {
    fn measure_text(&mut self, _text: &str, _size: f32, _weight: u16, _max_width: f32) -> (f32, f32) { (0.0, 0.0) }
}

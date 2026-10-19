use std::{cell::RefCell, collections::{BTreeMap, HashMap}, io::Read, ops::Range, path::{Path, PathBuf}, rc::Rc, sync::Arc, time::{Duration, Instant, SystemTime}};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures::io::AsyncReadExt as _;
use gpui::{
    AbsoluteLength, App, AppContext, AnyElement, Application, Bounds, ClipboardItem, Context, Div,
    ElementInputHandler, EntityInputHandler, ExternalPaths, FocusHandle, FontWeight, Hsla, Image, ImageFormat,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Point,
    Position, PromptLevel, Rgba, ScrollWheelEvent, ShapedLine, Stateful, TextAlign, TitlebarOptions,
    UTF16Selection, UniformListScrollHandle, Window, WindowBackgroundAppearance, WindowBounds,
    WindowControlArea, WindowOptions, WrappedLine, canvas, div, fill, img, http_client, prelude::*,
    point, px, rgba, size, uniform_list,
};
use serde_json::Value;

use crate::{
    network::NativeHttpClient,
    package::{self, AppPackage, Node},
    runtime::JsBackend,
    menus::TSGPUIAction,
    style as styles,
    text_input::{self, Selection},
};

pub struct AppView {
    document: AppPackage,
    backend: JsBackend,
    bundle_file: Option<PathBuf>,
    local_values: HashMap<u64, Value>,
    cursor_offsets: HashMap<u64, usize>,
    selected_all: std::collections::HashSet<u64>,
    validation_errors: HashMap<u64, String>,
    // Single-line <input> fields, keyed by node key plus repeat index.
    fields: HashMap<String, FieldRef>,
    focus_handles: HashMap<String, FocusHandle>,
    geometry: HashMap<String, Rc<RefCell<FieldGeometry>>>,
    selections: HashMap<String, Selection>,
    edit_history: HashMap<String, FieldHistory>,
    // IME composition: the field and the value range that holds the uncommitted text.
    marked: Option<(String, Range<usize>)>,
    composition_undo: Option<(String, String, Selection)>,
    focused_field: Option<String>,
    caret_visible: bool,
    caret_blink_running: bool,
    dragging_field: Option<String>,
    active_context_menu: Option<(String, Point<Pixels>)>,
    // Scroll position of each data-virtual list, keyed by node key plus repeat suffix.
    scroll_handles: HashMap<String, UniformListScrollHandle>,
    // Multi-line <textarea> fields. Their scroll offset must survive frames, so the geometry does too.
    areas: HashMap<String, Rc<RefCell<AreaGeometry>>>,
}

// Geometry of a multi-line field, written from prepaint. Each logical line is shaped on its own, so a
// line break in the value is always a real break, and each line wraps to the width of the field.
#[derive(Default)]
struct AreaGeometry {
    bounds: Option<Bounds<Pixels>>,
    lines: Vec<AreaLine>,
    placeholder: Option<ShapedLine>,
    line_height: Pixels,
    padding: Pixels,
    content_height: Pixels,
    // Vertical scroll offset into the text. It persists between frames.
    scroll: Pixels,
    // Set by keyboard movement and edits, so the next frame brings the caret into view.
    follow_caret: bool,
}

struct AreaLine {
    // Byte offset of the line in the value, and its length without the line break.
    start: usize,
    len: usize,
    // Vertical position of the line in the text, before scrolling.
    top: Pixels,
    // None for an empty line, which has nothing to shape.
    layout: Option<WrappedLine>,
}

impl AreaGeometry {
    fn line_for_index(&self, index: usize) -> usize {
        self.lines.partition_point(|line| line.start <= index).saturating_sub(1)
    }

    // Position of a byte index relative to the top-left of the text, before scrolling.
    fn point_for_index(&self, index: usize) -> Point<Pixels> {
        let Some(line) = self.lines.get(self.line_for_index(index)) else { return point(px(0.0), px(0.0)); };
        let Some(layout) = &line.layout else { return point(px(0.0), line.top); };
        let local = index.saturating_sub(line.start).min(line.len);
        match layout.position_for_index(local, self.line_height) {
            Some(at) => point(at.x, line.top + at.y),
            None => point(layout.width(), line.top),
        }
    }

    // The byte index nearest a point relative to the top-left of the text, before scrolling.
    fn index_for_point(&self, at: Point<Pixels>) -> usize {
        let index = self.lines.partition_point(|line| line.top <= at.y).saturating_sub(1);
        let Some(line) = self.lines.get(index) else { return 0; };
        let local = match &line.layout {
            Some(layout) => layout
                .closest_index_for_position(point(at.x, at.y - line.top), self.line_height)
                .unwrap_or_else(|closest| closest)
                .min(line.len),
            None => 0,
        };
        line.start + local
    }

    fn visible_height(&self) -> Pixels {
        self.bounds.map_or(px(0.0), |bounds| bounds.size.height) - self.padding * 2.0
    }

    fn max_scroll(&self) -> Pixels {
        (self.content_height - self.visible_height()).max(px(0.0))
    }
}

// What an input needs at runtime about its element. It is written from prepaint, which is the
// only point where the element's inherited text style is known.
#[derive(Default)]
struct FieldGeometry {
    bounds: Option<Bounds<Pixels>>,
    line: Option<ShapedLine>,
    masked: bool,
    padding: Pixels,
    line_height: Pixels,
}

#[derive(Clone)]
struct FieldRef {
    node_key: u64,
    binding: Option<String>,
    action: Option<String>,
    input_name: Option<String>,
    number: bool,
    // A <textarea>: line breaks are kept, and Up/Down, Home/End, and Enter act on lines.
    multiline: bool,
    // The value as the last render showed it. Used when neither state nor a local holds one.
    rendered_value: String,
}

#[derive(Clone)]
struct EditSnapshot {
    value: String,
    selection: Selection,
}

#[derive(Default)]
struct FieldHistory {
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    last_edit_at: Option<Instant>,
    last_edit_caret: Option<usize>,
    last_edit_kind: Option<EditKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind { Insert, Delete, Replace }

fn font_weight_for(weight: &str) -> FontWeight {
    match weight {
        "bold" | "700" => FontWeight::BOLD,
        "600" => FontWeight::SEMIBOLD,
        "500" => FontWeight::MEDIUM,
        _ => FontWeight::NORMAL,
    }
}

fn px_value(style: &BTreeMap<String, String>, key: &str) -> Option<f32> {
    style.get(key)?.strip_suffix("px")?.parse::<f32>().ok()
}

// Styles beyond the base layout set: paint, shadows, gradients, borders, flex, positioning,
// and text. It is generic over `Styled`, so the same code styles an element and its hover,
// active, focus, and disabled refinements, which are `StyleRefinement` values.
// `current_color` is the element's text color, used for borders that name no color.
fn apply_extended<S: Styled>(mut target: S, style: &BTreeMap<String, String>, current_color: Option<Rgba>) -> S {
    if let Some(background) = style.get("background").and_then(|value| styles::background(value)) {
        target = target.bg(background);
    }
    if let Some(color) = style.get("color").and_then(|value| styles::color(value)) {
        target = target.text_color(color);
    }
    if let Some(opacity) = style.get("opacity").and_then(|value| value.parse::<f32>().ok()) {
        target = target.opacity(opacity);
    }
    if let Some(shadows) = style.get("box-shadow").and_then(|value| styles::shadows(value)) {
        target = target.shadow(shadows);
    }
    let font_px = px_value(style, "font-size").unwrap_or(16.0);
    if let Some(height) = style.get("line-height").and_then(|value| styles::line_height(value, font_px)) {
        target = target.line_height(height);
    }
    if style.get("font-style").is_some_and(|value| value == "italic") {
        target = target.italic();
    }
    if style.get("white-space").is_some_and(|value| value == "nowrap") {
        target = target.whitespace_nowrap();
    }
    if style.get("text-overflow").is_some_and(|value| value == "ellipsis") {
        target = target.text_ellipsis();
    }
    target = match style.get("cursor").map(String::as_str) {
        Some("pointer") => target.cursor_pointer(),
        Some("text") => target.cursor_text(),
        Some("not-allowed") => target.cursor_not_allowed(),
        Some("grab") => target.cursor_grab(),
        Some("move") => target.cursor_move(),
        Some("crosshair") => target.cursor_crosshair(),
        Some("default") => target.cursor_default(),
        _ => target,
    };

    let refinement = target.style();
    if let Some(width) = style.get("min-width").and_then(|value| styles::length(value)) { refinement.min_size.width = Some(width); }
    if let Some(height) = style.get("min-height").and_then(|value| styles::length(value)) { refinement.min_size.height = Some(height); }
    if let Some(width) = style.get("max-width").and_then(|value| styles::length(value)) { refinement.max_size.width = Some(width); }
    if let Some(height) = style.get("max-height").and_then(|value| styles::length(value)) { refinement.max_size.height = Some(height); }
    if let Some(grow) = style.get("flex-grow").and_then(|value| value.parse::<f32>().ok()) { refinement.flex_grow = Some(grow); }
    if let Some(shrink) = style.get("flex-shrink").and_then(|value| value.parse::<f32>().ok()) { refinement.flex_shrink = Some(shrink); }
    if let Some(basis) = style.get("flex-basis").and_then(|value| styles::length(value)) { refinement.flex_basis = Some(basis); }
    match style.get("position").map(String::as_str) {
        Some("absolute") => refinement.position = Some(Position::Absolute),
        Some("relative") => refinement.position = Some(Position::Relative),
        _ => {}
    }
    if let Some(top) = style.get("top").and_then(|value| styles::length(value)) { refinement.inset.top = Some(top); }
    if let Some(right) = style.get("right").and_then(|value| styles::length(value)) { refinement.inset.right = Some(right); }
    if let Some(bottom) = style.get("bottom").and_then(|value| styles::length(value)) { refinement.inset.bottom = Some(bottom); }
    if let Some(left) = style.get("left").and_then(|value| styles::length(value)) { refinement.inset.left = Some(left); }
    if let Some(value) = style.get("padding-top").and_then(|value| styles::definite(value)) { refinement.padding.top = Some(value); }
    if let Some(value) = style.get("padding-right").and_then(|value| styles::definite(value)) { refinement.padding.right = Some(value); }
    if let Some(value) = style.get("padding-bottom").and_then(|value| styles::definite(value)) { refinement.padding.bottom = Some(value); }
    if let Some(value) = style.get("padding-left").and_then(|value| styles::definite(value)) { refinement.padding.left = Some(value); }
    if let Some(value) = style.get("margin-top").and_then(|value| styles::length(value)) { refinement.margin.top = Some(value); }
    if let Some(value) = style.get("margin-right").and_then(|value| styles::length(value)) { refinement.margin.right = Some(value); }
    if let Some(value) = style.get("margin-bottom").and_then(|value| styles::length(value)) { refinement.margin.bottom = Some(value); }
    if let Some(value) = style.get("margin-left").and_then(|value| styles::length(value)) { refinement.margin.left = Some(value); }
    let border_width = style.get("border-width").and_then(|value| styles::px_value(value));
    let border_hidden = style.get("border-style").is_some_and(|value| value == "none");
    let border_px = if border_hidden { Some(0.0) } else { border_width };
    let side_widths = ["top", "right", "bottom", "left"].map(|side| {
        let width = style.get(&format!("border-{side}-width")).and_then(|value| styles::px_value(value));
        let hidden = style.get(&format!("border-{side}-style")).is_some_and(|value| value == "none");
        if hidden { Some(0.0) } else { width }
    });
    let mut widths = [border_px; 4];
    for (index, side_width) in side_widths.into_iter().enumerate() {
        if side_width.is_some() { widths[index] = side_width; }
    }
    for (target, width) in [&mut refinement.border_widths.top, &mut refinement.border_widths.right, &mut refinement.border_widths.bottom, &mut refinement.border_widths.left].into_iter().zip(widths) {
        if let Some(width) = width { *target = Some(AbsoluteLength::Pixels(px(width))); }
    }
    if let Some(color) = style.get("border-color").and_then(|value| styles::color(value)).or(current_color) {
        if widths.iter().any(|width| width.is_some_and(|width| width > 0.0)) {
            refinement.border_color = Some(Hsla::from(color));
        }
    }
    target
}

fn media_matches(query: &str, width: gpui::Pixels, height: gpui::Pixels) -> bool {
    let normalized = query.to_ascii_lowercase();
    normalized.split("and").all(|condition| {
        let Some((edge_axis, raw_value)) = condition.trim().trim_matches(['(', ')']).split_once(':') else {
            return false;
        };
        let Some(raw_value) = raw_value.trim().strip_suffix("px") else { return false; };
        let Ok(value) = raw_value.parse::<f32>() else { return false; };
        let Some((edge, axis)) = edge_axis.trim().split_once('-') else { return false; };
        let actual = match axis.trim() {
            "width" => width,
            "height" => height,
            _ => return false,
        };
        match edge.trim() {
            "min" => actual >= px(value),
            "max" => actual <= px(value),
            _ => false,
        }
    })
}

fn computed_style(node: &Node, window: &Window) -> BTreeMap<String, String> {
    let mut style = node.style.clone();
    let bounds = window.bounds();
    let mut responsive: Vec<_> = node.responsive.iter().collect();
    responsive.sort_by_key(|rule| (rule.specificity, rule.order));
    for rule in responsive {
        if media_matches(&rule.media, bounds.size.width, bounds.size.height) {
            style.extend(rule.declarations.clone());
        }
    }
    style
}

fn lookup_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |current, part| match current {
        Value::Array(items) => part.parse::<usize>().ok().and_then(|index| items.get(index)),
        Value::Object(object) => object.get(part),
        _ => None,
    })
}

fn resolved_binding(path: &str, local_paths: &BTreeMap<String, String>) -> String {
    let mut pieces = path.splitn(2, '.');
    let head = pieces.next().unwrap_or_default();
    match (local_paths.get(head), pieces.next()) {
        (Some(base), Some(tail)) => format!("{base}.{tail}"),
        (Some(base), None) => base.clone(),
        _ => path.to_string(),
    }
}

fn as_input_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn printable(value: Option<&Value>) -> String {
    value.map(as_input_string).unwrap_or_default()
}

fn text_value(text: &str, state: &Value, locals: &BTreeMap<String, Value>) -> String {
    let mut rendered = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("{{") {
        rendered.push_str(&remaining[..start]);
        let after = &remaining[start + 2..];
        if let Some(end) = after.find("}}") {
            let path = after[..end].trim();
            let mut parts = path.splitn(2, '.');
            let first = parts.next().unwrap_or_default();
            let value = locals.get(first).and_then(|local| {
                parts.next().map_or(Some(local), |tail| lookup_path(local, tail))
            }).or_else(|| lookup_path(state, path));
            rendered.push_str(&printable(value));
            remaining = &after[end + 2..];
        } else {
            rendered.push_str(&remaining[start..]);
            return rendered;
        }
    }
    rendered.push_str(remaining);
    rendered
}

fn node_text(node: &Node) -> String {
    if node.tag == "#text" { return node.text.clone(); }
    node.children.iter().map(node_text).collect::<Vec<_>>().join(" ")
}

fn image_format(format: &str) -> Option<ImageFormat> {
    match format {
        "png" => Some(ImageFormat::Png),
        "jpeg" | "jpg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        "bmp" => Some(ImageFormat::Bmp),
        "svg" => Some(ImageFormat::Svg),
        _ => None,
    }
}

fn shortcut_matches(shortcut: &str, event: &gpui::KeyDownEvent) -> bool {
    let parts: Vec<_> = shortcut.to_ascii_lowercase().split('-').map(str::to_owned).collect();
    let Some(key) = parts.last() else { return false; };
    let modifiers = event.keystroke.modifiers;
    let wants_control = parts.iter().any(|part| part == "ctrl");
    let wants_command = parts.iter().any(|part| part == "cmd");
    let wants_alt = parts.iter().any(|part| part == "alt");
    let wants_shift = parts.iter().any(|part| part == "shift");
    let wants_mod = parts.iter().any(|part| part == "mod");
    let expected_control = wants_control || wants_mod && !cfg!(target_os = "macos");
    let expected_platform = wants_command || wants_mod && cfg!(target_os = "macos");
    if modifiers.control != expected_control || modifiers.platform != expected_platform || modifiers.alt != wants_alt || modifiers.shift != wants_shift { return false; }
    let actual = event.keystroke.key_char.as_deref().unwrap_or(&event.keystroke.key);
    actual.eq_ignore_ascii_case(key)
}

fn find_shortcut<'a>(node: &'a Node, event: &gpui::KeyDownEvent) -> Option<&'a Node> {
    if matches!(node.tag.as_str(), "button" | "menuitem") && !node.attrs.contains_key("disabled")
        && node.attrs.get("data-shortcut").is_some_and(|shortcut| shortcut_matches(shortcut, event)) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_shortcut(child, event))
}

fn form_is_valid(node: &Node, state: &Value, local_values: &HashMap<u64, Value>) -> bool {
    fn visit(
        node: &Node,
        state: &Value,
        local_values: &HashMap<u64, Value>,
        locals: &BTreeMap<String, Value>,
        local_paths: &BTreeMap<String, String>,
        skip_repeat: bool,
    ) -> bool {
        if !skip_repeat {
            if let Some(expression) = node.attrs.get("data-repeat") {
                let mut pieces = expression.split_whitespace();
                let array_path = pieces.next().unwrap_or_default();
                let alias = if pieces.next() == Some("as") { pieces.next().unwrap_or("item") } else { "item" };
                let state_path = resolved_binding(array_path, local_paths);
                let items = lookup_path(state, &state_path).and_then(Value::as_array).cloned().unwrap_or_default();
                return items.into_iter().enumerate().all(|(index, item)| {
                    let mut scoped = locals.clone();
                    scoped.insert(alias.to_string(), item);
                    let mut scoped_paths = local_paths.clone();
                    scoped_paths.insert(alias.to_string(), format!("{state_path}.{index}"));
                    visit(node, state, local_values, &scoped, &scoped_paths, true)
                });
            }
        }

        let binding = node.attrs.get("data-bind");
        let bound_value = binding.and_then(|path| {
            locals.get(path.split('.').next().unwrap_or_default())
                .and_then(|local| path.split_once('.').and_then(|(_, tail)| lookup_path(local, tail)))
                .or_else(|| lookup_path(state, &resolved_binding(path, local_paths)))
        });
        let default_value = if node.tag == "input" && node.attrs.get("type").is_some_and(|kind| kind == "checkbox") {
            node.attrs.contains_key("checked").then_some(Value::Bool(true))
        } else if node.tag == "select" {
            node.children.iter().find(|option| option.attrs.contains_key("selected"))
                .or_else(|| node.children.iter().find(|option| option.tag == "option"))
                .map(|option| Value::String(option.attrs.get("value").cloned().unwrap_or_else(|| node_text(option))))
        } else {
            node.attrs.get("value").cloned().map(Value::String)
        };
        let raw = local_values.get(&node.key).cloned().or_else(|| bound_value.cloned()).or(default_value);
        if node.attrs.contains_key("required") && ["input", "textarea", "select"].contains(&node.tag.as_str()) {
            if node.tag == "input" && node.attrs.get("type").is_some_and(|kind| kind == "checkbox") {
                if !raw.as_ref().is_some_and(|value| value == &Value::Bool(true)) { return false; }
            } else if raw.as_ref().map(as_input_string).unwrap_or_default().trim().is_empty() {
                return false;
            }
        }
        if node.tag == "input" && node.attrs.get("type").is_some_and(|kind| kind == "number") {
            let raw_text = raw.as_ref().map(as_input_string).unwrap_or_default();
            if !raw_text.trim().is_empty() {
                if let Ok(number) = raw_text.parse::<f64>() {
                    let min = node.attrs.get("min").and_then(|value| value.parse::<f64>().ok());
                    let max = node.attrs.get("max").and_then(|value| value.parse::<f64>().ok());
                    if min.is_some_and(|min| number < min) || max.is_some_and(|max| number > max) { return false; }
                    if let Some(step) = node.attrs.get("step").and_then(|value| value.parse::<f64>().ok()) {
                        let base = min.unwrap_or(0.0);
                        if ((number - base) / step - ((number - base) / step).round()).abs() > 0.000001 { return false; }
                    }
                } else { return false; }
            } else if node.attrs.contains_key("required") {
                return false;
            }
        }
        if node.tag == "input" && node.attrs.get("type").is_some_and(|kind| kind == "email") {
            let email = raw.as_ref().map(as_input_string).unwrap_or_default();
            if !email.is_empty() && !email.split_once('@').is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.')) { return false; }
        }
        node.children.iter().all(|child| visit(child, state, local_values, locals, local_paths, false))
    }
    visit(node, state, local_values, &BTreeMap::new(), &BTreeMap::new(), false)
}

impl AppView {
    pub fn new(document: AppPackage, backend: JsBackend, bundle_file: Option<PathBuf>) -> Self {
        Self {
            document, backend, bundle_file, local_values: HashMap::new(), cursor_offsets: HashMap::new(),
            selected_all: std::collections::HashSet::new(), validation_errors: HashMap::new(),
            fields: HashMap::new(), focus_handles: HashMap::new(), geometry: HashMap::new(),
            selections: HashMap::new(), edit_history: HashMap::new(), marked: None, focused_field: None,
            composition_undo: None, caret_visible: true, caret_blink_running: false, dragging_field: None,
            active_context_menu: None,
            scroll_handles: HashMap::new(), areas: HashMap::new(),
        }
    }

    fn start_hot_reload(&self, cx: &mut Context<Self>) {
        let Some(bundle_file) = self.bundle_file.clone() else { return; };
        let mut last_stamp = bundle_stamp(&bundle_file);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                let stamp = bundle_stamp(&bundle_file);
                if stamp == last_stamp { continue; }
                last_stamp = stamp;
                let document = match package::load_bundle(&bundle_file) {
                    Ok(document) => document,
                    Err(error) => { eprintln!("TSGPUI hot reload: {error}"); continue; }
                };
                if this.update(cx, |view, cx| {
                    // The new script starts fresh, then keeps the values the running app has changed.
                    // A failed reload leaves the running document and backend in place.
                    let backend = match JsBackend::reload(&document.backend_js, &view.backend) {
                        Ok(backend) => backend,
                        Err(error) => { eprintln!("TSGPUI hot reload: {error}"); return; }
                    };
                    view.document = document;
                    view.backend = backend;
                    // Node keys follow document order, so per-node UI state (typed text, caret,
                    // selection, focus) is reset rather than matched to nodes that may have moved.
                    view.local_values.clear();
                    view.cursor_offsets.clear();
                    view.selected_all.clear();
                    view.validation_errors.clear();
                    view.fields.clear();
                    view.geometry.clear();
                    view.selections.clear();
                    view.edit_history.clear();
                    view.marked = None;
                    view.composition_undo = None;
                    view.focused_field = None;
                    view.caret_visible = true;
                    view.dragging_field = None;
                    view.active_context_menu = None;
                    view.scroll_handles.clear();
                    view.areas.clear();
                    cx.notify();
                }).is_err() {
                    break;
                }
            }
        }).detach();
    }

    fn start_caret_blink(&mut self, cx: &mut Context<Self>) {
        if self.caret_blink_running { return; }
        self.caret_blink_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(530)).await;
                let keep_running = this.update(cx, |view, cx| {
                    if view.focused_field.is_some() {
                        view.caret_visible = !view.caret_visible;
                        cx.notify();
                        true
                    } else {
                        view.caret_visible = true;
                        view.caret_blink_running = false;
                        cx.notify();
                        false
                    }
                }).unwrap_or(false);
                if !keep_running { break; }
            }
        }).detach();
    }

    fn render_node(
        &mut self,
        node: &Node,
        cx: &mut Context<Self>,
        window: &mut Window,
        locals: &BTreeMap<String, Value>,
        local_paths: &BTreeMap<String, String>,
        inherited_submit: Option<String>,
        inherited_form: Option<Node>,
        repeat_suffix: &str,
        skip_repeat: bool,
    ) -> AnyElement {
        if matches!(node.tag.as_str(), "menu" | "menuitem" | "hr") {
            return div().into_any_element();
        }
        let mut scoped_locals = locals.clone();
        if !node.props.is_empty() {
            scoped_locals.insert("props".into(), Value::Object(node.props.iter().map(|(key, value)| (key.clone(), Value::String(value.clone()))).collect()));
        }
        let locals = &scoped_locals;
        if !skip_repeat {
            if let Some(expression) = node.attrs.get("data-repeat") {
                let mut pieces = expression.split_whitespace();
                let array_path = pieces.next().unwrap_or_default();
                let alias = if pieces.next() == Some("as") { pieces.next().unwrap_or("item") } else { "item" };
                let resolved_array_path = resolved_binding(array_path, local_paths);
                if let Some(row_height) = node.attrs.get("data-virtual").and_then(|value| value.parse::<f32>().ok()) {
                    return self.render_virtual_rows(node, row_height, alias, &resolved_array_path, locals, local_paths, inherited_submit, inherited_form, repeat_suffix, cx);
                }
                let items = lookup_path(self.backend.state(), &resolved_array_path).and_then(Value::as_array).cloned().unwrap_or_default();
                let mut wrapper = div().flex().flex_col();
                for (index, item) in items.into_iter().enumerate() {
                    let mut scoped = locals.clone();
                    scoped.insert(alias.to_string(), item);
                    let mut scoped_paths = local_paths.clone();
                    scoped_paths.insert(alias.to_string(), format!("{resolved_array_path}.{index}"));
                    let suffix = format!("{repeat_suffix}:{index}");
                    wrapper = wrapper.child(self.render_node(node, cx, window, &scoped, &scoped_paths, inherited_submit.clone(), inherited_form.clone(), &suffix, true));
                }
                return wrapper.into_any_element();
            }
        }

        if node.tag == "img" {
            if let Some(asset) = node.attrs.get("src").and_then(|path| self.document.assets.get(path)) {
                if let (Some(format), Ok(bytes)) = (image_format(&asset.format), STANDARD.decode(&asset.data)) {
                    let image = Arc::new(Image::from_bytes(format, bytes));
                    let mut image_element = img(image);
                    let style = computed_style(node, window);
                    if let Some(width) = px_value(&style, "width") { image_element = image_element.w(px(width)); }
                    if let Some(height) = px_value(&style, "height") { image_element = image_element.h(px(height)); }
                    return image_element.into_any_element();
                }
            }
            return div().child(node.attrs.get("alt").cloned().unwrap_or_default()).into_any_element();
        }

        let state = self.backend.state().clone();
        let style = computed_style(node, window);
        let is_form = node.tag == "form";
        let enabled = !node.attrs.contains_key("disabled");
        let form_action = if is_form { node.attrs.get("data-submit").cloned().or(inherited_submit) } else { inherited_submit };
        let form_node = if is_form { Some(node.clone()) } else { inherited_form };
        let element_id: gpui::SharedString = format!("tsgpui:{}{}", node.key, repeat_suffix).into();
        let mut element = div().id(element_id);
        match style.get("display").map(String::as_str) {
            Some("block") => element = element.block(),
            Some("grid") => {
                element = element.grid();
                if let Some(columns) = style.get("grid-template-columns") {
                    element = element.grid_cols(columns.split_whitespace().count().clamp(1, 12) as u16);
                }
                if let Some(rows) = style.get("grid-template-rows") {
                    element = element.grid_rows(rows.split_whitespace().count().clamp(1, 12) as u16);
                }
            }
            _ => element = element.flex().flex_col(),
        }
        if node.tag == "body" { element = element.size_full(); }
        if let Some(color) = style.get("color").and_then(|value| styles::color(value)) { element = element.text_color(color); }
        if let Some(value) = style.get("width").and_then(|value| styles::length(value)) { element = element.w(value); }
        if let Some(value) = style.get("height").and_then(|value| styles::length(value)) { element = element.h(value); }
        if let Some(value) = style.get("padding").and_then(|value| styles::definite(value)) { element = element.p(value); }
        if let Some(value) = style.get("margin").and_then(|value| styles::length(value)) { element = element.m(value); }
        if let Some(value) = px_value(&style, "gap") { element = element.gap(px(value)); }
        let column_gap = px_value(&style, "column-gap");
        let row_gap = px_value(&style, "row-gap");
        if column_gap.is_some() || row_gap.is_some() {
            let gap = &mut element.style().gap;
            if let Some(value) = column_gap { gap.width = Some(px(value).into()); }
            if let Some(value) = row_gap { gap.height = Some(px(value).into()); }
        }
        if let Some(value) = px_value(&style, "border-radius") { element = element.rounded(px(value)); }
        if let Some(value) = px_value(&style, "font-size") { element = element.text_size(px(value)); }
        if let Some(family) = style.get("font-family") { element = element.font_family(family.trim_matches(['\"', '\'']).to_string()); }
        if let Some(value) = style.get("opacity").and_then(|value| value.parse::<f32>().ok()) { element = element.opacity(value); }
        if let Some(weight) = style.get("font-weight") {
            element = element.font_weight(font_weight_for(weight));
        }
        if style.get("flex-direction").is_some_and(|value| value == "row") { element = element.flex_row(); }
        match style.get("overflow").map(String::as_str) {
            Some("scroll" | "auto") => element = element.overflow_scroll(),
            Some("hidden") => element = element.overflow_hidden(),
            _ => {},
        }
        match style.get("align-items").map(String::as_str) {
            Some("center") => element = element.items_center(),
            Some("flex-start") => element = element.items_start(),
            Some("flex-end") => element = element.items_end(),
            _ => {},
        }
        match style.get("justify-content").map(String::as_str) {
            Some("center") => element = element.justify_center(),
            Some("flex-start") => element = element.justify_start(),
            Some("flex-end") => element = element.justify_end(),
            Some("space-between") => element = element.justify_between(),
            Some("space-around") => element = element.justify_around(),
            _ => {},
        }
        match style.get("text-align").map(String::as_str) {
            Some("center") => element = element.text_center(),
            Some("right") => element = element.text_right(),
            _ => {},
        }
        if enabled {
            if let Some(index) = node.attrs.get("tabindex").and_then(|value| value.parse::<isize>().ok()) {
                element = if index < 0 { element.focusable() } else { element.tab_index(index) };
            } else if ["button", "input", "textarea", "select"].contains(&node.tag.as_str()) {
                element = element.tab_index(0);
            }
        }
        if node.tag == "button" { element = element.cursor_pointer(); }
        // Pseudo-class styles compile into node.states. :hover, :active and :focus are GPUI refinements,
        // so they apply only while the pointer or focus is on this element.
        let current_color = style.get("color").and_then(|value| styles::color(value));
        element = apply_extended(element, &style, current_color);
        if !enabled {
            if let Some(map) = node.states.get("disabled") { element = apply_extended(element, map, current_color); }
        }
        if let Some(map) = node.states.get("hover").cloned() { element = element.hover(move |refined| apply_extended(refined, &map, current_color)); }
        if let Some(map) = node.states.get("active").cloned() { element = element.active(move |refined| apply_extended(refined, &map, current_color)); }
        if let Some(map) = node.states.get("focus").cloned() { element = element.focus(move |refined| apply_extended(refined, &map, current_color)); }
        // With a unified title bar the window asks the OS for these areas: dragging the bar moves the
        // window, and the window buttons close, minimize, or maximize it without a round trip through JS.
        if node.attrs.contains_key("data-window-drag") { element = element.window_control_area(WindowControlArea::Drag); }
        match node.attrs.get("data-window-control").map(String::as_str) {
            Some("minimize") => element = element.window_control_area(WindowControlArea::Min),
            Some("maximize") => element = element.window_control_area(WindowControlArea::Max),
            Some("close") => element = element.window_control_area(WindowControlArea::Close),
            _ => {}
        }

        if enabled && (node.tag == "button" || node.tag == "input" && node.attrs.get("type").is_some_and(|kind| kind == "checkbox")) {
            if let Some(action) = node.attrs.get("data-onclick").filter(|_| !node.attrs.contains_key("data-confirm")) {
                let action = action.clone();
                element = element.on_click(cx.listener(move |this, _, _, cx| {
                    match this.backend.invoke_with_event(&action, serde_json::json!({"type":"click"})) {
                        Ok(()) => cx.notify(),
                        Err(error) => eprintln!("TSGPUI: {error}"),
                    }
                }));
            }
            if node.attrs.get("type").is_some_and(|kind| kind == "checkbox") {
                let key = node.key;
                let binding = node.attrs.get("data-bind").map(|path| resolved_binding(path, local_paths));
                let action = node.attrs.get("data-oninput").cloned();
                let initially_checked = node.attrs.contains_key("checked");
                let input_name = node.attrs.get("name").cloned();
                element = element.on_click(cx.listener(move |this, _, _, cx| {
                    let previous = binding.as_ref().and_then(|key| lookup_path(this.backend.state(), key))
                        .cloned().or_else(|| this.local_values.get(&key).cloned())
                        .unwrap_or_else(|| Value::Bool(initially_checked));
                    let checked = !previous.as_bool().unwrap_or(false);
                    if let Some(binding) = &binding {
                        if let Err(error) = this.backend.set_state(binding, Value::Bool(checked)) { eprintln!("TSGPUI: {error}"); }
                    } else {
                        this.local_values.insert(key, Value::Bool(checked));
                    }
                    if let Some(action) = &action {
                        let event = serde_json::json!({"type":"input","checked":checked,"value":checked,"name":input_name});
                        if let Err(error) = this.backend.invoke_with_event(action, event) { eprintln!("TSGPUI: {error}"); }
                    }
                    cx.notify();
                }));
            }
        }

        if node.tag == "select" {
            let key = node.key;
            let binding = node.attrs.get("data-bind").map(|path| resolved_binding(path, local_paths));
            let options: Vec<(String, String)> = node.children.iter().filter(|child| child.tag == "option")
                .map(|option| (option.attrs.get("value").cloned().unwrap_or_else(|| node_text(option)), node_text(option)))
                .collect();
            let default_selected = node.children.iter().find(|option| option.attrs.contains_key("selected"))
                .map(|option| option.attrs.get("value").cloned().unwrap_or_else(|| node_text(option)))
                .or_else(|| options.first().map(|option| option.0.clone()));
            let selected = node.attrs.get("data-bind").and_then(|path| {
                locals.get(path.split('.').next().unwrap_or_default())
                    .and_then(|local| path.split_once('.').and_then(|(_, tail)| lookup_path(local, tail)))
                    .or_else(|| binding.as_ref().and_then(|path| lookup_path(&state, path)))
            }).map(as_input_string)
                .or_else(|| self.local_values.get(&key).map(as_input_string))
                .or(default_selected.clone()).unwrap_or_default();
            let label = options.iter().find(|option| option.0 == selected).map(|option| option.1.clone()).unwrap_or(selected);
            element = element.cursor_pointer().child(label);
            let action = node.attrs.get("data-oninput").cloned();
            if enabled { element = element.on_click(cx.listener(move |this, _, _, cx| {
                if options.is_empty() { return; }
                let current = binding.as_ref().and_then(|path| lookup_path(this.backend.state(), path)).map(as_input_string)
                    .or_else(|| this.local_values.get(&key).map(as_input_string)).unwrap_or_default();
                let next = options.iter().position(|option| option.0 == current).map_or(0, |index| (index + 1) % options.len());
                let value = options[next].0.clone();
                if let Some(binding) = &binding {
                    if let Err(error) = this.backend.set_state(binding, Value::String(value.clone())) { eprintln!("TSGPUI: {error}"); }
                } else { this.local_values.insert(key, Value::String(value.clone())); }
                if let Some(action) = &action {
                    if let Err(error) = this.backend.invoke_with_event(action, serde_json::json!({"type":"input","value":value})) { eprintln!("TSGPUI: {error}"); }
                }
                cx.notify();
            })); }
            if enabled {
                let key = node.key;
                let binding = node.attrs.get("data-bind").map(|path| resolved_binding(path, local_paths));
                let key_action = node.attrs.get("data-oninput").cloned();
                let key_options = node.children.iter().filter(|child| child.tag == "option")
                    .map(|option| option.attrs.get("value").cloned().unwrap_or_else(|| node_text(option))).collect::<Vec<_>>();
                element = element.on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                    if !event.keystroke.key.eq_ignore_ascii_case("down") && !event.keystroke.key.eq_ignore_ascii_case("up") { return; }
                    if key_options.is_empty() { return; }
                    let current = binding.as_ref().and_then(|path| lookup_path(this.backend.state(), path)).map(as_input_string)
                        .or_else(|| this.local_values.get(&key).map(as_input_string)).unwrap_or_default();
                    let old = key_options.iter().position(|option| option == &current).unwrap_or(0);
                    let next = if event.keystroke.key.eq_ignore_ascii_case("down") { (old + 1) % key_options.len() } else { (old + key_options.len() - 1) % key_options.len() };
                    let value = key_options[next].clone();
                    if let Some(binding) = &binding {
                        if let Err(error) = this.backend.set_state(binding, Value::String(value.clone())) { eprintln!("TSGPUI: {error}"); }
                    } else { this.local_values.insert(key, Value::String(value.clone())); }
                    if let Some(action) = &key_action {
                    if let Err(error) = this.backend.invoke_with_event(action, serde_json::json!({"type":"input","value":value})) { eprintln!("TSGPUI: {error}"); }
                    }
                    cx.notify();
                }));
            }
            return element.into_any_element();
        }

        if node.tag == "textarea" {
            return self.render_multi_line_input(node, element, &style, locals, local_paths, enabled, repeat_suffix, cx, window);
        }
        if node.tag == "input" && node.attrs.get("type").map(String::as_str) != Some("checkbox") {
            return self.render_single_line_input(node, element, &style, locals, local_paths, enabled, repeat_suffix, cx, window);
        }

        if let Some(copy_key) = node.attrs.get("data-clipboard-copy").filter(|_| enabled) {
            let copy_key = copy_key.clone();
            element = element.on_click(cx.listener(move |this, _, _, cx| {
                let value = lookup_path(this.backend.state(), &copy_key).map(as_input_string).unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(value));
            }));
        }
        if let Some(paste_key) = node.attrs.get("data-clipboard-paste").filter(|_| enabled) {
            let paste_key = paste_key.clone();
            element = element.on_click(cx.listener(move |this, _, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    if let Err(error) = this.backend.set_state(&paste_key, Value::String(text)) { eprintln!("TSGPUI: {error}"); }
                    cx.notify();
                }
            }));
        }
        if let (Some(request_url), Some(target_key)) = (node.attrs.get("data-fetch").filter(|_| enabled), node.attrs.get("data-fetch-into")) {
            let request_url = request_url.clone();
            let target_key = target_key.clone();
            let allowed_hosts = self.document.permissions.network_hosts.clone();
            element = element.on_click(cx.listener(move |_this, _, _, cx| {
                let parsed = match http_client::Url::parse(&request_url) {
                    Ok(url) => url,
                    Err(error) => { eprintln!("TSGPUI network request: invalid URL: {error}"); return; }
                };
                let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
                if parsed.scheme() != "https" || parsed.port_or_known_default() != Some(443) || !allowed_hosts.iter().any(|allowed| allowed == &host) {
                    eprintln!("TSGPUI network request: host is not permitted: {host}");
                    return;
                }
                let target_key = target_key.clone();
                let client = cx.http_client();
                cx.spawn(async move |this, cx| {
                    let result = async {
                        let response = client.get(parsed.as_str(), http_client::AsyncBody::empty(), false).await?;
                        if !response.status().is_success() { anyhow::bail!("HTTP status {}", response.status()); }
                        let body = response.into_body();
                        let mut bytes = Vec::new();
                        body.take(1024 * 1024 + 1).read_to_end(&mut bytes).await?;
                        if bytes.len() > 1024 * 1024 { anyhow::bail!("response body exceeds 1 MiB"); }
                        String::from_utf8(bytes).map_err(Into::into)
                    }.await;
                    match result {
                        Ok(text) => { let _ = this.update(cx, |this, cx| {
                            if let Err(error) = this.backend.set_state(&target_key, Value::String(text)) { eprintln!("TSGPUI network request: {error}"); }
                            cx.notify();
                        }); }
                        Err(error) => eprintln!("TSGPUI network request: {error}"),
                    }
                }).detach();
            }));
        }
        if let Some(target_key) = node.attrs.get("data-file-open").filter(|_| enabled) {
            let target_key = target_key.clone();
            let read_key = node.attrs.get("data-file-read").cloned();
            element = element.on_click(cx.listener(move |_this, _, _window, cx| {
                let target_key = target_key.clone();
                let read_key = read_key.clone();
                let receiver = cx.prompt_for_paths(PathPromptOptions {
                    files: true, directories: false, multiple: false,
                    prompt: Some("Choose a file".into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = receiver.await {
                        if let Some(path) = paths.first() {
                            let path = path.display().to_string();
                            let contents = if read_key.is_some() {
                                match std::fs::File::open(&path) {
                                    Ok(file) => {
                                        let mut bytes = Vec::new();
                                        match file.take(1024 * 1024 + 1).read_to_end(&mut bytes) {
                                            Ok(_) if bytes.len() <= 1024 * 1024 => match String::from_utf8(bytes) {
                                                Ok(text) => Some(text),
                                                Err(_) => { eprintln!("TSGPUI file picker: selected file is not UTF-8 text"); None }
                                            },
                                            Ok(_) => { eprintln!("TSGPUI file picker: text files must be at most 1 MiB"); None }
                                            Err(error) => { eprintln!("TSGPUI file picker: {error}"); None }
                                        }
                                    }
                                    Err(error) => { eprintln!("TSGPUI file picker: {error}"); None }
                                }
                            } else { None };
                            let _ = this.update(cx, |this, cx| {
                                if let Err(error) = this.backend.set_state(&target_key, Value::String(path)) { eprintln!("TSGPUI: {error}"); }
                                if let (Some(key), Some(contents)) = (&read_key, contents) {
                                    if let Err(error) = this.backend.set_state(key, Value::String(contents)) { eprintln!("TSGPUI: {error}"); }
                                }
                                cx.notify();
                            });
                        }
                    }
                }).detach();
            }));
        }
        if let Some(source_key) = node.attrs.get("data-file-save").filter(|_| enabled) {
            let source_key = source_key.clone();
            let suggested_name = node.attrs.get("title").cloned().unwrap_or_else(|| "document.txt".into());
            element = element.on_click(cx.listener(move |this, _, _window, cx| {
                let source_key = source_key.clone();
                let contents = lookup_path(this.backend.state(), &source_key).map(as_input_string).unwrap_or_default();
                if contents.len() > 1024 * 1024 {
                    eprintln!("TSGPUI file picker: text files must be at most 1 MiB");
                    return;
                }
                let suggested_name = suggested_name.clone();
                let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let receiver = cx.prompt_for_new_path(Path::new(&directory), Some(&suggested_name));
                cx.spawn(async move |_this, _cx| {
                    if let Ok(Ok(Some(path))) = receiver.await {
                        if let Err(error) = std::fs::write(path, contents.as_bytes()) {
                            eprintln!("TSGPUI file picker: save failed: {error}");
                        }
                    }
                }).detach();
            }));
        }
        if let Some(action) = node.attrs.get("data-confirm").filter(|_| enabled) {
            let action = action.clone();
            let message = node.attrs.get("title").cloned().unwrap_or_else(|| node_text(node));
            element = element.on_click(cx.listener(move |_this, _, window, cx| {
                let action = action.clone();
                let answer = window.prompt(PromptLevel::Warning, &message, None, &["Continue", "Cancel"], cx);
                cx.spawn(async move |this, cx| {
                    if answer.await == Ok(0) {
                        let _ = this.update(cx, |this, cx| {
                            match this.backend.invoke_with_event(&action, serde_json::json!({"type":"confirm"})) {
                                Ok(()) => cx.notify(),
                                Err(error) => eprintln!("TSGPUI: {error}"),
                            }
                        });
                    }
                }).detach();
            }));
        }

        if node.key == self.document.root.key {
            element = element.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("escape") && this.active_context_menu.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key.eq_ignore_ascii_case("tab") {
                    if event.keystroke.modifiers.shift { window.focus_prev(); } else { window.focus_next(); }
                    cx.stop_propagation();
                    return;
                }
                if let Some(button) = find_shortcut(&this.document.root, event) {
                    if let Some(action) = button.attrs.get("data-onclick") {
                        if let Err(error) = this.backend.invoke_with_event(action, serde_json::json!({"type":"shortcut","key":event.keystroke.key})) { eprintln!("TSGPUI: {error}"); }
                        cx.notify();
                        cx.stop_propagation();
                    }
                }
            }));
            element = element.on_action(cx.listener(|this, action: &TSGPUIAction, _, cx| {
                if let Err(error) = this.backend.invoke_with_event(&action.action, serde_json::json!({"type":"menu"})) {
                    eprintln!("TSGPUI menu action: {error}");
                }
                cx.notify();
            }));
        }
        if let Some(menu_id) = node.attrs.get("data-contextmenu") {
            let menu_id = menu_id.clone();
            element = element.on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.active_context_menu = Some((menu_id.clone(), event.position));
                cx.notify();
            }));
        }
        if let Some(action) = node.attrs.get("data-ondrop") {
            let action = action.clone();
            element = element.on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                let files: Vec<_> = paths.paths().iter().map(|path| {
                    serde_json::json!({
                        "path": path.to_string_lossy(),
                        "name": path.file_name().unwrap_or_default().to_string_lossy()
                    })
                }).collect();
                if let Err(error) = this.backend.invoke_with_event(&action, serde_json::json!({"type":"drop","files":files})) {
                    eprintln!("TSGPUI file drop action: {error}");
                }
                cx.notify();
            }));
        }

        if let Some(binding) = node.attrs.get("data-bind") {
            let value = locals.get(binding.split('.').next().unwrap_or_default())
                .and_then(|local| binding.split_once('.').and_then(|(_, path)| lookup_path(local, path)))
                .or_else(|| lookup_path(&state, binding));
            element = element.child(printable(value));
        } else if node.tag == "#text" {
            element = element.child(text_value(&node.text, &state, locals));
        } else if node.attrs.get("type").is_some_and(|kind| kind == "checkbox") {
            let checked = node.attrs.get("data-bind").and_then(|key| {
                locals.get(key.split('.').next().unwrap_or_default())
                    .and_then(|local| key.split_once('.').and_then(|(_, tail)| lookup_path(local, tail)))
                    .or_else(|| lookup_path(&state, &resolved_binding(key, local_paths)))
            }).and_then(Value::as_bool)
                .or_else(|| self.local_values.get(&node.key).and_then(Value::as_bool))
                .unwrap_or_else(|| node.attrs.contains_key("checked"));
            let label = node.attrs.get("aria-label").map(|label| format!("{} {label}", if checked { "[x]" } else { "[ ]" }))
                .unwrap_or_else(|| if checked { "[x]".into() } else { "[ ]".into() });
            element = element.child(label);
        } else {
            for child in &node.children {
                element = element.child(self.render_node(child, cx, window, locals, local_paths, form_action.clone(), form_node.clone(), repeat_suffix, false));
            }
            if node.children.is_empty() && node.tag == "button" {
                if let Some(label) = node.attrs.get("aria-label") { element = element.child(label.clone()); }
            }
            if is_form {
                if let Some(message) = self.validation_errors.get(&node.key) { element = element.child(div().child(message.clone())); }
            }
        }
        if node.tag == "form" {
            let key = node.key;
            let active_form = node.clone();
            let submit_action = form_action.clone();
            element = element.on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("enter") {
                    if form_is_valid(&active_form, this.backend.state(), &this.local_values) {
                        this.validation_errors.remove(&key);
                        if let Some(action) = &submit_action {
                            if let Err(error) = this.backend.invoke_with_event(action, serde_json::json!({"type":"submit","submit":true})) { eprintln!("TSGPUI: {error}"); }
                        }
                    } else { this.validation_errors.insert(key, "Check the required fields and values.".into()); }
                    cx.notify();
                }
            }));
        }
        if enabled && node.tag == "button" && node.attrs.get("type").is_some_and(|kind| kind == "submit") {
            if let Some(action) = form_action.clone() {
                let active_form = form_node.clone().unwrap_or_else(|| self.document.root.clone());
                element = element.on_click(cx.listener(move |this, _, _, cx| {
                    if form_is_valid(&active_form, this.backend.state(), &this.local_values) {
                        this.validation_errors.clear();
                        if let Err(error) = this.backend.invoke_with_event(&action, serde_json::json!({"type":"submit","submit":true})) { eprintln!("TSGPUI: {error}"); }
                    } else { this.validation_errors.insert(active_form.key, "Check the required fields and values.".into()); }
                    cx.notify();
                }));
            }
        }
        if node.key == self.document.root.key {
            if let Some(overlay) = self.context_menu_overlay(node, cx) { element = element.child(overlay); }
        }
        element.into_any_element()
    }
}

impl AppView {
    fn context_menu_overlay(&self, root: &Node, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (menu_id, at) = self.active_context_menu.clone()?;
        let menu = crate::menus::context_menu(root, &menu_id)?;
        fn flatten(node: &Node, depth: usize, out: &mut Vec<(Node, usize)>) {
            for child in &node.children {
                if child.tag == "hr" {
                    out.push((child.clone(), depth));
                } else if child.tag == "menuitem" {
                    out.push((child.clone(), depth));
                    if child.children.iter().any(|grandchild| grandchild.tag == "menuitem") {
                        flatten(child, depth + 1, out);
                    }
                }
            }
        }
        let mut items = Vec::new();
        flatten(menu, 0, &mut items);
        let mut popup = div().absolute().left(at.x).top(at.y)
            .min_w(px(190.0)).p(px(4.0)).bg(rgba(0x252525ff))
            .border_1().border_color(rgba(0x555555ff)).rounded_md().shadow_lg();
        for (index, (item, depth)) in items.iter().enumerate() {
            if item.tag == "hr" {
                popup = popup.child(div().h(px(1.0)).my(px(3.0)).bg(rgba(0x555555ff)));
            } else if item.tag == "menuitem" {
                let title = crate::menus::text_label(item);
                let disabled = item.attrs.contains_key("disabled");
                let action = item.attrs.get("data-onclick").cloned();
                let mut row = div().id(("context-menu-row", index)).pl(px(10.0 + *depth as f32 * 14.0)).pr(px(10.0)).py(px(6.0)).rounded_sm()
                    .text_color(if disabled { rgba(0x888888ff) } else { rgba(0xf3f3f3ff) })
                    .child(title);
                if !disabled {
                    if let Some(action) = action {
                        row = row.on_click(cx.listener(move |this, _, _, cx| {
                            this.active_context_menu = None;
                            if let Err(error) = this.backend.invoke_with_event(&action, serde_json::json!({"type":"contextmenu"})) {
                                eprintln!("TSGPUI context menu action: {error}");
                            }
                            cx.notify();
                        }));
                    }
                }
                popup = popup.child(row);
            }
        }
        let mut overlay = div().absolute().top_0().left_0().size_full();
        overlay = overlay.on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.active_context_menu = None;
                cx.notify();
            }));
        popup = popup.on_mouse_down(MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
        }));
        overlay = overlay.child(popup);
        Some(overlay.into_any_element())
    }

    // A data-virtual list. The repeated element is the row, and each row must have the height given
    // by data-virtual, so GPUI's uniform list can place rows without measuring them. Only rows near
    // the viewport are built, on each frame, from the current state.
    #[allow(clippy::too_many_arguments)]
    fn render_virtual_rows(
        &mut self,
        node: &Node,
        row_height: f32,
        alias: &str,
        array_path: &str,
        locals: &BTreeMap<String, Value>,
        local_paths: &BTreeMap<String, String>,
        inherited_submit: Option<String>,
        inherited_form: Option<Node>,
        repeat_suffix: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let list_key = format!("{}{}", node.key, repeat_suffix);
        let scroll = self.scroll_handles.entry(list_key.clone()).or_insert_with(UniformListScrollHandle::new).clone();
        let count = lookup_path(self.backend.state(), array_path).and_then(Value::as_array).map_or(0, Vec::len);
        // Rows are the repeated element with a fixed height. Height rules from media queries would
        // break the uniform height, so they are removed from the row copy.
        let mut row = node.clone();
        row.style.insert("height".into(), format!("{row_height}px"));
        row.style.insert("flex-shrink".into(), "0".into());
        // Rows fill the list's width, as they would in a column, unless the row sets its own width.
        row.style.entry("width".into()).or_insert_with(|| "100%".into());
        for rule in &mut row.responsive {
            for key in ["height", "min-height", "max-height"] { rule.declarations.remove(key); }
        }
        let alias = alias.to_string();
        let array_path = array_path.to_string();
        let locals = locals.clone();
        let local_paths = local_paths.clone();
        let repeat_suffix = repeat_suffix.to_string();
        let rows = cx.processor(move |this: &mut AppView, range: Range<usize>, window: &mut Window, cx: &mut Context<AppView>| {
            range.map(|index| {
                let item = lookup_path(this.backend.state(), &array_path).and_then(Value::as_array).and_then(|items| items.get(index)).cloned();
                let Some(item) = item else { return div().h(px(row_height)).into_any_element(); };
                let mut scoped = locals.clone();
                scoped.insert(alias.clone(), item);
                let mut scoped_paths = local_paths.clone();
                scoped_paths.insert(alias.clone(), format!("{array_path}.{index}"));
                let suffix = format!("{repeat_suffix}:{index}");
                this.render_node(&row, cx, window, &scoped, &scoped_paths, inherited_submit.clone(), inherited_form.clone(), &suffix, true)
            }).collect()
        });
        let list_id: gpui::SharedString = format!("tsgpui-list:{list_key}").into();
        uniform_list(list_id, count, rows)
            .track_scroll(scroll)
            .w_full()
            .flex_grow()
            .min_h_0()
            .into_any_element()
    }

    // Multi-line <textarea>. As with the single-line field, the canvas does the text work. Prepaint
    // shapes each logical line to the field's width, and paint draws the selection, text, and caret
    // from that layout, so the caret is always where the text is. The element clips the text and
    // scrolls it: the wheel and the caret move the offset, which is kept in AreaGeometry.
    #[allow(clippy::too_many_arguments)]
    fn render_multi_line_input(
        &mut self,
        node: &Node,
        mut element: Stateful<Div>,
        style: &BTreeMap<String, String>,
        locals: &BTreeMap<String, Value>,
        local_paths: &BTreeMap<String, String>,
        enabled: bool,
        repeat_suffix: &str,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) -> AnyElement {
        let key = node.key;
        let field_key = format!("{}{}", node.key, repeat_suffix);
        let binding = node.attrs.get("data-bind").map(|path| resolved_binding(path, local_paths));
        let initial_value = node.attrs.get("value").cloned().unwrap_or_else(|| node_text(node));
        let displayed = node.attrs.get("data-bind").and_then(|path| {
            locals.get(path.split('.').next().unwrap_or_default())
                .and_then(|local| path.split_once('.').and_then(|(_, tail)| lookup_path(local, tail)))
                .or_else(|| binding.as_ref().and_then(|path| lookup_path(self.backend.state(), path)))
        }).map(as_input_string)
            .or_else(|| self.local_values.get(&key).map(as_input_string))
            .unwrap_or(initial_value);
        let placeholder = node.attrs.get("placeholder").cloned().unwrap_or_default();
        self.fields.insert(field_key.clone(), FieldRef {
            node_key: key,
            binding,
            action: node.attrs.get("data-oninput").cloned(),
            input_name: node.attrs.get("name").cloned(),
            number: false,
            multiline: true,
            rendered_value: displayed.clone(),
        });
        // A textarea without a height shows about three lines, rather than collapsing to nothing.
        if !style.contains_key("height") && !style.contains_key("min-height") && !style.contains_key("flex-grow") {
            element = element.h(px(96.0));
        }
        element = element.overflow_hidden().cursor_text();
        if !enabled {
            return element.child(displayed).into_any_element();
        }

        let focus = self.focus_handles.entry(field_key.clone()).or_insert_with(|| cx.focus_handle()).clone();
        let focused = focus.is_focused(window);
        if focused {
            self.focused_field = Some(field_key.clone());
            self.start_caret_blink(cx);
        }
        let selection = self.selection_in(&field_key, &displayed);
        let area = self.areas.entry(field_key.clone()).or_insert_with(|| Rc::new(RefCell::new(AreaGeometry::default()))).clone();
        let padding = px_value(style, "padding").unwrap_or(0.0);
        let font_size = px_value(style, "font-size");
        let weight = style.get("font-weight").cloned();
        let font_family = style.get("font-family").cloned();
        let caret_color = style.get("color").and_then(|value| styles::color(value)).unwrap_or(rgba(0xffffffff));
        let placeholder_color = rgba(0x8e8e93ff);
        let selection_color = rgba(0x3b82f640);
        let view = cx.entity();
        let layout_area = area.clone();
        let layout_value = displayed.clone();
        let layout_placeholder = placeholder.clone();
        let layout_caret = selection.head;
        let paint_area = area.clone();
        let paint_focus = focus.clone();
        let paint_selection = selection;
        let paint_caret_visible = self.caret_visible;
        let overlay = canvas(
            move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                let mut text_style = window.text_style();
                if let Some(size) = font_size {
                    text_style.font_size = px(size).into();
                }
                if let Some(weight) = &weight {
                    text_style.font_weight = font_weight_for(weight);
                }
                if let Some(family) = &font_family { text_style.font_family = family.trim_matches(['\"', '\'']).to_owned().into(); }
                let rem = window.rem_size();
                let font_px = text_style.font_size.to_pixels(rem);
                let line_height = text_style.line_height_in_pixels(rem);
                let wrap_width = (bounds.size.width - px(padding) * 2.0).max(px(1.0));
                let mut lines = Vec::new();
                let mut top = px(0.0);
                let mut start = 0;
                for segment in layout_value.split('\n') {
                    let layout = if segment.is_empty() {
                        None
                    } else {
                        let run = text_style.to_run(segment.len());
                        window.text_system()
                            .shape_text(segment.to_owned().into(), font_px, &[run], Some(wrap_width), None)
                            .ok()
                            .and_then(|wrapped| wrapped.into_iter().next())
                    };
                    let height = layout.as_ref().map_or(line_height, |wrapped| wrapped.size(line_height).height);
                    lines.push(AreaLine { start, len: segment.len(), top, layout });
                    top += height;
                    start += segment.len() + 1;
                }
                let placeholder = (layout_value.is_empty() && !layout_placeholder.is_empty()).then(|| {
                    let mut run = text_style.to_run(layout_placeholder.len());
                    run.color = placeholder_color.into();
                    window.text_system().shape_line(layout_placeholder.clone().into(), font_px, &[run], None)
                });
                let mut geometry = layout_area.borrow_mut();
                geometry.bounds = Some(bounds);
                geometry.lines = lines;
                geometry.placeholder = placeholder;
                geometry.line_height = line_height;
                geometry.padding = px(padding);
                geometry.content_height = top;
                if geometry.follow_caret {
                    geometry.follow_caret = false;
                    let caret = geometry.point_for_index(layout_caret);
                    let visible = geometry.visible_height();
                    if caret.y < geometry.scroll {
                        geometry.scroll = caret.y;
                    } else if caret.y + line_height > geometry.scroll + visible {
                        geometry.scroll = caret.y + line_height - visible;
                    }
                }
                geometry.scroll = geometry.scroll.max(px(0.0)).min(geometry.max_scroll());
            },
            move |bounds: Bounds<Pixels>, _: (), window: &mut Window, cx: &mut App| {
                let geometry = paint_area.borrow();
                let origin = point(bounds.origin.x + px(padding), bounds.origin.y + px(padding) - geometry.scroll);
                let line_height = geometry.line_height;
                if focused && !paint_selection.is_empty() {
                    // A selection is painted row by row. Rows between the ends span the full wrap width.
                    let range = paint_selection.range();
                    let right = bounds.size.width - px(padding) * 2.0;
                    let break_width = line_height * 0.5;
                    let mut paint_row = |x: Pixels, y: Pixels, width: Pixels| {
                        if width > px(0.0) {
                            window.paint_quad(fill(Bounds::new(point(origin.x + x, origin.y + y), size(width, line_height)), selection_color));
                        }
                    };
                    for line in &geometry.lines {
                        let line_end = line.start + line.len;
                        if line_end < range.start || line.start > range.end { continue; }
                        let from = range.start.max(line.start);
                        let to = range.end.min(line_end);
                        let breaks = range.end > line_end;
                        let first = geometry.point_for_index(from);
                        let last = geometry.point_for_index(to);
                        if (first.y - last.y).abs() < px(0.5) {
                            let extra = if breaks { break_width } else { px(0.0) };
                            paint_row(first.x, first.y, last.x - first.x + extra);
                        } else {
                            paint_row(first.x, first.y, right - first.x);
                            let mut row = first.y + line_height;
                            while row < last.y {
                                paint_row(px(0.0), row, right);
                                row += line_height;
                            }
                            let extra = if breaks { break_width } else { px(0.0) };
                            paint_row(px(0.0), last.y, last.x + extra);
                        }
                    }
                }
                if let Some(placeholder) = &geometry.placeholder {
                    let _ = placeholder.paint(origin, line_height, window, cx);
                }
                for line in &geometry.lines {
                    if let Some(layout) = &line.layout {
                        let _ = layout.paint(point(origin.x, origin.y + line.top), line_height, TextAlign::Left, Some(bounds), window, cx);
                    }
                }
                if focused && paint_caret_visible {
                    let caret = geometry.point_for_index(paint_selection.head);
                    window.paint_quad(fill(Bounds::new(point(origin.x + caret.x, origin.y + caret.y), size(px(1.5), line_height)), caret_color));
                }
                window.handle_input(&paint_focus, ElementInputHandler::new(bounds, view), cx);
            },
        ).absolute().top_0().left_0().size_full();

        element = element.track_focus(&focus).child(overlay);
        if enabled {
            element = element
                .on_key_down(cx.listener({
                    let field_key = field_key.clone();
                    move |this, event: &gpui::KeyDownEvent, _, cx| {
                        if this.handle_field_key(&field_key, event, cx) {
                            cx.stop_propagation();
                        }
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener({
                    let field_key = field_key.clone();
                    let focus = focus.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        window.focus(&focus);
                        this.dragging_field = Some(field_key.clone());
                        this.click_field(&field_key, event.position, event.modifiers.shift, event.click_count);
                        cx.notify();
                    }
                }))
                .on_mouse_move(cx.listener({
                    let field_key = field_key.clone();
                    move |this, event: &MouseMoveEvent, _, cx| {
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.dragging_field = None;
                        } else if this.dragging_field.as_deref() == Some(field_key.as_str()) {
                            this.click_field(&field_key, event.position, true, 1);
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.dragging_field = None;
                }))
                .on_scroll_wheel(cx.listener({
                    let field_key = field_key.clone();
                    move |this, event: &ScrollWheelEvent, _, cx| {
                        if this.scroll_area(&field_key, event.delta.pixel_delta(px(20.0)).y) {
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }
                }));
        }
        element.into_any_element()
    }

    // Moves a textarea's scroll offset by a wheel delta. Returns true when the text moved, so the
    // wheel does not also scroll an enclosing list.
    fn scroll_area(&mut self, field_key: &str, delta_y: Pixels) -> bool {
        let Some(area) = self.areas.get(field_key) else { return false; };
        let mut area = area.borrow_mut();
        let before = area.scroll;
        area.scroll = (area.scroll - delta_y).max(px(0.0)).min(area.max_scroll());
        area.scroll != before
    }

    // The index one visual row up or down from `index`, keeping the caret's horizontal position.
    fn vertical_index(&self, field_key: &str, value: &str, index: usize, down: bool) -> usize {
        let Some(area) = self.areas.get(field_key) else { return index; };
        let area = area.borrow();
        let caret = area.point_for_index(index);
        let row_middle = if down { caret.y + area.line_height * 1.5 } else { caret.y - area.line_height * 0.5 };
        if row_middle < px(0.0) {
            0
        } else if row_middle > area.content_height {
            value.len()
        } else {
            text_input::clamp_to_boundary(value, area.index_for_point(point(caret.x, row_middle)))
        }
    }

    // Single-line <input> fields. The text is an ordinary child; the caret and selection are
    // painted by a canvas that shapes the text with the element's inherited style, so positions
    // match what GPUI draws. Printable text arrives through the EntityInputHandler, which is what
    // makes IME work; key listeners handle movement and editing keys.
    #[allow(clippy::too_many_arguments)]
    fn render_single_line_input(
        &mut self,
        node: &Node,
        mut element: Stateful<Div>,
        style: &BTreeMap<String, String>,
        locals: &BTreeMap<String, Value>,
        local_paths: &BTreeMap<String, String>,
        enabled: bool,
        repeat_suffix: &str,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) -> AnyElement {
        let key = node.key;
        let field_key = format!("{}{}", node.key, repeat_suffix);
        let binding = node.attrs.get("data-bind").map(|path| resolved_binding(path, local_paths));
        let input_type = node.attrs.get("type").cloned().unwrap_or_else(|| "text".into());
        let masked = input_type == "password";
        let displayed = node.attrs.get("data-bind").and_then(|path| {
            locals.get(path.split('.').next().unwrap_or_default())
                .and_then(|local| path.split_once('.').and_then(|(_, tail)| lookup_path(local, tail)))
                .or_else(|| binding.as_ref().and_then(|path| lookup_path(self.backend.state(), path)))
        }).map(as_input_string)
            .or_else(|| self.local_values.get(&key).map(as_input_string))
            .or_else(|| node.attrs.get("value").cloned())
            .unwrap_or_default();
        let placeholder = node.attrs.get("placeholder").cloned().unwrap_or_default();
        let shown = if masked {
            "*".repeat(displayed.chars().count())
        } else if displayed.is_empty() && !placeholder.is_empty() {
            placeholder
        } else {
            displayed.clone()
        };
        self.fields.insert(field_key.clone(), FieldRef {
            node_key: key,
            binding,
            action: node.attrs.get("data-oninput").cloned(),
            input_name: node.attrs.get("name").cloned(),
            number: input_type == "number",
            multiline: false,
            rendered_value: displayed.clone(),
        });
        if !enabled {
            return element.child(shown).into_any_element();
        }

        let focus = self.focus_handles.entry(field_key.clone()).or_insert_with(|| cx.focus_handle()).clone();
        let focused = focus.is_focused(window);
        if focused {
            self.focused_field = Some(field_key.clone());
            self.start_caret_blink(cx);
        }
        let selection = self.selection_in(&field_key, &displayed);
        let geometry = Rc::new(RefCell::new(FieldGeometry::default()));
        self.geometry.insert(field_key.clone(), geometry.clone());
        let padding = px_value(style, "padding").unwrap_or(0.0);
        let font_size = px_value(style, "font-size");
        let weight = style.get("font-weight").cloned();
        let font_family = style.get("font-family").cloned();
        let caret_color = style.get("color").and_then(|value| styles::color(value)).unwrap_or(rgba(0xffffffff));
        let view = cx.entity();
        let layout_geometry = geometry.clone();
        let layout_text = shown.clone();
        let paint_value = displayed.clone();
        let paint_caret_visible = self.caret_visible;
        let paint_focus = focus.clone();
        let overlay = canvas(
            move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                let mut text_style = window.text_style();
                if let Some(size) = font_size {
                    text_style.font_size = px(size).into();
                }
                if let Some(weight) = &weight {
                    text_style.font_weight = font_weight_for(weight);
                }
                if let Some(family) = &font_family { text_style.font_family = family.trim_matches(['\"', '\'']).to_owned().into(); }
                let rem = window.rem_size();
                let font_px = text_style.font_size.to_pixels(rem);
                let line_height = text_style.line_height_in_pixels(rem);
                let run = text_style.to_run(layout_text.len());
                let line = window.text_system().shape_line(layout_text.clone().into(), font_px, &[run], None);
                *layout_geometry.borrow_mut() = FieldGeometry {
                    bounds: Some(bounds),
                    line: Some(line.clone()),
                    masked,
                    padding: px(padding),
                    line_height,
                };
                (line, line_height)
            },
            move |bounds: Bounds<Pixels>, (line, line_height): (ShapedLine, Pixels), window: &mut Window, cx: &mut App| {
                if focused && paint_caret_visible {
                    let origin_x = bounds.origin.x + px(padding);
                    let origin_y = bounds.origin.y + px(padding);
                    let x_at = |index: usize| origin_x + line.x_for_index(text_input::display_offset(&paint_value, index, masked));
                    if !selection.is_empty() {
                        let range = selection.range();
                        let left = x_at(range.start);
                        let right = x_at(range.end);
                        window.paint_quad(fill(Bounds::new(point(left, origin_y), size(right - left, line_height)), rgba(0x3b82f640)));
                    }
                    window.paint_quad(fill(Bounds::new(point(x_at(selection.head), origin_y), size(px(1.5), line_height)), caret_color));
                }
                window.handle_input(&paint_focus, ElementInputHandler::new(bounds, view), cx);
            },
        ).absolute().top_0().left_0().size_full();

        element = element
            .track_focus(&focus)
            .child(shown)
            .child(overlay)
            .on_key_down(cx.listener({
                let field_key = field_key.clone();
                move |this, event: &gpui::KeyDownEvent, _, cx| {
                    if this.handle_field_key(&field_key, event, cx) {
                        cx.stop_propagation();
                    }
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener({
                let field_key = field_key.clone();
                let focus = focus.clone();
                move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus);
                    this.dragging_field = Some(field_key.clone());
                    this.click_field(&field_key, event.position, event.modifiers.shift, event.click_count);
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener({
                let field_key = field_key.clone();
                move |this, event: &MouseMoveEvent, _, cx| {
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.dragging_field = None;
                    } else if this.dragging_field.as_deref() == Some(field_key.as_str()) {
                        this.click_field(&field_key, event.position, true, 1);
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, _| {
                this.dragging_field = None;
            }));
        element.into_any_element()
    }

    fn field_value(&self, field: &FieldRef) -> String {
        field.binding.as_ref().and_then(|path| lookup_path(self.backend.state(), path)).map(as_input_string)
            .or_else(|| self.local_values.get(&field.node_key).map(as_input_string))
            .unwrap_or_else(|| field.rendered_value.clone())
    }

    // Stored selections can outlive a value that changed under them, so they are clamped on read.
    // Without a stored selection the caret sits at the end, as before.
    fn selection_in(&self, field_key: &str, value: &str) -> Selection {
        self.selections.get(field_key).copied().unwrap_or_else(|| Selection::caret(value.len())).clamped(value)
    }

    fn index_at(&self, field_key: &str, value: &str, position: Point<Pixels>) -> Option<usize> {
        if self.fields.get(field_key).is_some_and(|field| field.multiline) {
            let area = self.areas.get(field_key)?.borrow();
            let bounds = area.bounds?;
            let at = point(position.x - bounds.origin.x - area.padding, position.y - bounds.origin.y - area.padding + area.scroll);
            return Some(text_input::clamp_to_boundary(value, area.index_for_point(at)));
        }
        let geometry = self.geometry.get(field_key)?.borrow();
        let bounds = geometry.bounds?;
        let line = geometry.line.as_ref()?;
        let x = position.x - bounds.origin.x - geometry.padding;
        Some(text_input::value_offset(value, line.closest_index_for_x(x), geometry.masked))
    }

    // A click places the caret; shift extends from the anchor; a double click selects the word.
    fn click_field(&mut self, field_key: &str, position: Point<Pixels>, extend: bool, clicks: usize) {
        let Some(field) = self.fields.get(field_key).cloned() else { return; };
        let value = self.field_value(&field);
        let Some(index) = self.index_at(field_key, &value, position) else { return; };
        if clicks >= 2 {
            let word = text_input::word_range(&value, index);
            self.selections.insert(field_key.to_string(), Selection { anchor: word.start, head: word.end });
            return;
        }
        let anchor = if extend { self.selection_in(field_key, &value).anchor } else { index };
        self.selections.insert(field_key.to_string(), Selection { anchor, head: index });
    }

    // Navigation and editing keys. Printable keys are left alone so they reach the input handler.
    // Returns true when the key was consumed.
    fn handle_field_key(&mut self, field_key: &str, event: &gpui::KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(field) = self.fields.get(field_key).cloned() else { return false; };
        let value = self.field_value(&field);
        let selection = self.selection_in(field_key, &value);
        let modifiers = event.keystroke.modifiers;
        let secondary = modifiers.control || modifiers.platform;
        let key_name = event.keystroke.key.to_ascii_lowercase();
        let head = selection.head;
        let range = selection.range();
        let mut moved: Option<Selection> = None;
        let mut edit: Option<(Range<usize>, String)> = None;
        match key_name.as_str() {
            "z" if secondary => {
                if modifiers.shift { self.redo_field(field_key); } else { self.undo_field(field_key); }
                cx.notify();
                return true;
            }
            "y" if secondary => {
                self.redo_field(field_key);
                cx.notify();
                return true;
            }
            "left" | "right" if secondary => {
                let target = if key_name == "left" {
                    text_input::previous_word_boundary(&value, head)
                } else {
                    text_input::next_word_boundary(&value, head)
                };
                moved = Some(if modifiers.shift { Selection { anchor: selection.anchor, head: target } } else { Selection::caret(target) });
            }
            "left" | "right" | "home" | "end" if !secondary => {
                let target = match key_name.as_str() {
                    "left" if !modifiers.shift && !selection.is_empty() => range.start,
                    "left" => text_input::previous_boundary(&value, head),
                    "right" if !modifiers.shift && !selection.is_empty() => range.end,
                    "right" => text_input::next_boundary(&value, head),
                    "home" if field.multiline => text_input::line_start(&value, head),
                    "home" => 0,
                    "end" if field.multiline => text_input::line_end(&value, head),
                    _ => value.len(),
                };
                moved = Some(if modifiers.shift { Selection { anchor: selection.anchor, head: target } } else { Selection::caret(target) });
            }
            "home" | "end" if secondary => {
                let target = if key_name == "home" { 0 } else { value.len() };
                moved = Some(if modifiers.shift { Selection { anchor: selection.anchor, head: target } } else { Selection::caret(target) });
            }
            "up" | "down" if field.multiline && !secondary => {
                let target = self.vertical_index(field_key, &value, head, key_name == "down");
                moved = Some(if modifiers.shift { Selection { anchor: selection.anchor, head: target } } else { Selection::caret(target) });
            }
            "enter" if field.multiline && !secondary => edit = Some((range.clone(), "\n".to_string())),
            "a" if secondary => moved = Some(Selection { anchor: 0, head: value.len() }),
            "c" if secondary => {
                if !selection.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(value[range.clone()].to_string()));
                }
                moved = Some(selection);
            }
            "x" if secondary && !selection.is_empty() => {
                cx.write_to_clipboard(ClipboardItem::new_string(value[range.clone()].to_string()));
                edit = Some((range.clone(), String::new()));
            }
            "v" if secondary => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    edit = Some((range.clone(), text));
                }
            }
            "backspace" | "delete" if secondary => {
                let target = if !selection.is_empty() {
                    range.clone()
                } else if key_name == "backspace" {
                    text_input::previous_word_boundary(&value, head)..head
                } else {
                    head..text_input::next_word_boundary(&value, head)
                };
                if !target.is_empty() { edit = Some((target, String::new())); }
            }
            "backspace" | "delete" => {
                let target = if !selection.is_empty() {
                    range.clone()
                } else if key_name == "backspace" {
                    text_input::previous_boundary(&value, head)..head
                } else {
                    head..text_input::next_boundary(&value, head)
                };
                if !target.is_empty() {
                    edit = Some((target, String::new()));
                }
            }
            _ => return false,
        }
        match edit {
            Some((range, text)) => self.edit_field(field_key, range, &text),
            None => {
                self.selections.insert(field_key.to_string(), moved.unwrap_or(selection));
                if field.multiline { self.follow_caret(field_key); }
            }
        }
        cx.notify();
        true
    }

    // Replaces `range` of a field's value with `text`, then commits the change.
    fn edit_field(&mut self, field_key: &str, range: Range<usize>, text: &str) {
        let Some(field) = self.fields.get(field_key).cloned() else { return; };
        let value = self.field_value(&field);
        let text = if field.multiline { text_input::normalize_newlines(text) } else { text_input::single_line(text) };
        let (updated, caret) = text_input::replace_range(&value, range.clone(), &text);
        if updated != value {
            self.record_edit(field_key, &value, self.selection_in(field_key, &value), caret, range, &text);
        }
        self.composition_undo = None;
        self.marked = None;
        self.selections.insert(field_key.to_string(), Selection::caret(caret));
        if field.multiline { self.follow_caret(field_key); }
        if updated != value {
            self.commit_field(&field, updated);
        }
    }

    fn record_edit(&mut self, field_key: &str, before: &str, selection: Selection, caret: usize, range: Range<usize>, inserted: &str) {
        const HISTORY_LIMIT: usize = 100;
        let kind = if range.is_empty() && !inserted.is_empty() { EditKind::Insert }
            else if inserted.is_empty() { EditKind::Delete }
            else { EditKind::Replace };
        let now = Instant::now();
        let history = self.edit_history.entry(field_key.to_string()).or_default();
        let can_coalesce = inserted != "\n" && matches!(kind, EditKind::Insert | EditKind::Delete)
            && history.last_edit_kind == Some(kind)
            && history.last_edit_at.is_some_and(|at| now.duration_since(at) < Duration::from_millis(900))
            && history.last_edit_caret == Some(selection.head);
        if !can_coalesce {
            history.undo.push(EditSnapshot { value: before.to_string(), selection });
            if history.undo.len() > HISTORY_LIMIT { history.undo.remove(0); }
        }
        history.redo.clear();
        history.last_edit_at = Some(now);
        history.last_edit_caret = Some(caret);
        history.last_edit_kind = Some(kind);
    }

    fn undo_field(&mut self, field_key: &str) {
        let Some(field) = self.fields.get(field_key).cloned() else { return; };
        let current = self.field_value(&field);
        let selection = self.selection_in(field_key, &current);
        let Some(history) = self.edit_history.get_mut(field_key) else { return; };
        let Some(snapshot) = history.undo.pop() else { return; };
        history.redo.push(EditSnapshot { value: current, selection });
        history.last_edit_at = None;
        history.last_edit_kind = None;
        self.selections.insert(field_key.to_string(), snapshot.selection);
        if field.multiline { self.follow_caret(field_key); }
        self.commit_field(&field, snapshot.value);
    }

    fn finish_composition(&mut self, field_key: &str) {
        let Some((key, value, selection)) = self.composition_undo.take() else { return; };
        if key != field_key { return; }
        let Some(field) = self.fields.get(field_key) else { return; };
        let current = self.field_value(field);
        if current == value { return; }
        let history = self.edit_history.entry(field_key.to_string()).or_default();
        history.undo.push(EditSnapshot { value, selection });
        if history.undo.len() > 100 { history.undo.remove(0); }
        history.redo.clear();
        history.last_edit_at = None;
        history.last_edit_kind = None;
    }

    fn redo_field(&mut self, field_key: &str) {
        let Some(field) = self.fields.get(field_key).cloned() else { return; };
        let current = self.field_value(&field);
        let selection = self.selection_in(field_key, &current);
        let Some(history) = self.edit_history.get_mut(field_key) else { return; };
        let Some(snapshot) = history.redo.pop() else { return; };
        history.undo.push(EditSnapshot { value: current, selection });
        history.last_edit_at = None;
        history.last_edit_kind = None;
        self.selections.insert(field_key.to_string(), snapshot.selection);
        if field.multiline { self.follow_caret(field_key); }
        self.commit_field(&field, snapshot.value);
    }

    // The next frame scrolls a textarea so its caret is visible.
    fn follow_caret(&self, field_key: &str) {
        if let Some(area) = self.areas.get(field_key) {
            area.borrow_mut().follow_caret = true;
        }
    }

    fn commit_field(&mut self, field: &FieldRef, value: String) {
        let state_value = if field.number {
            value.parse::<f64>().ok().and_then(serde_json::Number::from_f64)
                .map(Value::Number).unwrap_or_else(|| Value::String(value.clone()))
        } else {
            Value::String(value.clone())
        };
        if let Some(binding) = &field.binding {
            if let Err(error) = self.backend.set_state(binding, state_value.clone()) { eprintln!("TSGPUI: {error}"); }
        } else {
            self.local_values.insert(field.node_key, state_value.clone());
        }
        if let Some(action) = &field.action {
            if let Err(error) = self.backend.invoke_with_event(action, serde_json::json!({"type":"input","value":state_value,"name":field.input_name})) { eprintln!("TSGPUI: {error}"); }
        }
    }

    fn focused_field_ref(&self) -> Option<(String, FieldRef)> {
        let key = self.focused_field.clone()?;
        let field = self.fields.get(&key)?.clone();
        Some((key, field))
    }

    // The value range an input operation targets: the given UTF-16 range, else the marked
    // composition, else the selection.
    fn target_range(&self, field_key: &str, value: &str, range_utf16: Option<Range<usize>>) -> Range<usize> {
        match range_utf16 {
            Some(range) => text_input::utf16_to_byte(value, range.start)..text_input::utf16_to_byte(value, range.end),
            None => self.marked.as_ref()
                .filter(|(marked_key, _)| marked_key == field_key)
                .map(|(_, range)| range.clone())
                .unwrap_or_else(|| self.selection_in(field_key, value).range()),
        }
    }
}

impl EntityInputHandler for AppView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (_, field) = self.focused_field_ref()?;
        let value = self.field_value(&field);
        let start = text_input::utf16_to_byte(&value, range.start);
        let end = text_input::utf16_to_byte(&value, range.end).max(start);
        *adjusted_range = Some(text_input::byte_to_utf16(&value, start)..text_input::byte_to_utf16(&value, end));
        Some(value[start..end].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (key, field) = self.focused_field_ref()?;
        let value = self.field_value(&field);
        let selection = self.selection_in(&key, &value);
        let range = selection.range();
        Some(UTF16Selection {
            range: text_input::byte_to_utf16(&value, range.start)..text_input::byte_to_utf16(&value, range.end),
            reversed: selection.reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let (key, field) = self.focused_field_ref()?;
        let (marked_key, marked) = self.marked.as_ref()?;
        if *marked_key != key {
            return None;
        }
        let value = self.field_value(&field);
        Some(text_input::byte_to_utf16(&value, marked.start)..text_input::byte_to_utf16(&value, marked.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        let key = self.marked.as_ref().map(|(key, _)| key.clone())
            .or_else(|| self.composition_undo.as_ref().map(|(key, _, _)| key.clone()));
        if let Some(key) = key { self.finish_composition(&key); }
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let Some((key, field)) = self.focused_field_ref() else { return; };
        let value = self.field_value(&field);
        let range = self.target_range(&key, &value, range_utf16);
        self.edit_field(&key, range, text);
        cx.notify();
    }

    // Composition text is written into the value as it is typed, so the backend always sees what
    // the user sees. The marked range is what the next composition step replaces.
    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((key, field)) = self.focused_field_ref() else { return; };
        let value = self.field_value(&field);
        if self.composition_undo.is_none() {
            self.composition_undo = Some((key.clone(), value.clone(), self.selection_in(&key, &value)));
        }
        let range = self.target_range(&key, &value, range_utf16);
        let (updated, _) = text_input::replace_range(&value, range.clone(), new_text);
        let mark_start = text_input::clamp_to_boundary(&value, range.start);
        let mark_end = mark_start + new_text.len();
        let selection = match new_selected_range {
            Some(within) => Selection {
                anchor: mark_start + text_input::utf16_to_byte(new_text, within.start),
                head: mark_start + text_input::utf16_to_byte(new_text, within.end),
            },
            None => Selection::caret(mark_end),
        };
        self.marked = if new_text.is_empty() { None } else { Some((key.clone(), mark_start..mark_end)) };
        self.selections.insert(key, selection);
        if updated != value {
            self.commit_field(&field, updated);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (key, field) = self.focused_field_ref()?;
        let value = self.field_value(&field);
        let start = text_input::utf16_to_byte(&value, range_utf16.start);
        if field.multiline {
            // The candidate window opens at the start of the range, on its visual row.
            let area = self.areas.get(&key)?.borrow();
            let caret = area.point_for_index(start);
            let origin = point(element_bounds.origin.x + area.padding + caret.x, element_bounds.origin.y + area.padding + caret.y - area.scroll);
            return Some(Bounds::new(origin, size(px(1.5), area.line_height)));
        }
        let geometry = self.geometry.get(&key)?.borrow();
        let line = geometry.line.as_ref()?;
        let end = text_input::utf16_to_byte(&value, range_utf16.end).max(start);
        let x_start = line.x_for_index(text_input::display_offset(&value, start, geometry.masked));
        let x_end = line.x_for_index(text_input::display_offset(&value, end, geometry.masked));
        Some(Bounds::new(
            point(element_bounds.origin.x + geometry.padding + x_start, element_bounds.origin.y + geometry.padding),
            size(x_end - x_start, geometry.line_height),
        ))
    }

    fn character_index_for_point(&mut self, position: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let (key, field) = self.focused_field_ref()?;
        let value = self.field_value(&field);
        let index = self.index_at(&key, &value, position)?;
        Some(text_input::byte_to_utf16(&value, index))
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.document.title);
        self.focused_field = None;
        let root = self.document.root.clone();
        self.render_node(&root, cx, window, &BTreeMap::new(), &BTreeMap::new(), None, None, "", false)
    }
}

fn bundle_stamp(path: &std::path::Path) -> Option<(u64, SystemTime)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

pub fn run(doc: AppPackage, backend: JsBackend, bundle_file: Option<PathBuf>) {
    Application::new().with_http_client(Arc::new(NativeHttpClient::new())).run(move |cx: &mut App| {
        crate::menus::set_application_menus(&doc.root, cx);
        let fonts = doc.fonts.values().filter_map(|encoded| {
            STANDARD.decode(encoded).ok().map(std::borrow::Cow::Owned)
        }).collect();
        if let Err(error) = cx.text_system().add_fonts(fonts) {
            eprintln!("TSGPUI font load error: {error}");
        }
        let bounds = Bounds::centered(None, size(px(doc.width as f32), px(doc.height as f32)), cx);
        let mut options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: if doc.window.backdrop == "blur" { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque },
            ..Default::default()
        };
        if doc.window.titlebar == "unified" {
            options.titlebar = Some(TitlebarOptions { appears_transparent: true, ..Default::default() });
        }
        cx.open_window(options, move |_, cx| cx.new(|cx| {
            let view = AppView::new(doc, backend, bundle_file);
            view.start_hot_reload(cx);
            view
        }))
            .expect("Failed to open native GPUI window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;
    use serde_json::json;

    fn style(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn px_value_reads_only_px_lengths() {
        let s = style(&[("width", "12px"), ("height", "3em"), ("gap", "abcpx")]);
        assert_eq!(px_value(&s, "width"), Some(12.0));
        assert_eq!(px_value(&s, "height"), None);
        assert_eq!(px_value(&s, "gap"), None);
        assert_eq!(px_value(&s, "missing"), None);
    }

    #[test]
    fn media_queries_compare_window_size() {
        let (w, h) = (px(800.0), px(600.0));
        assert!(media_matches("(min-width: 600px)", w, h));
        assert!(!media_matches("(max-width: 500px)", w, h));
        assert!(media_matches("(min-width: 400px) and (max-height: 700px)", w, h));
        assert!(!media_matches("(min-width: 400px) and (max-width: 500px)", w, h));
    }

    #[test]
    fn malformed_media_queries_never_match() {
        let (w, h) = (px(800.0), px(600.0));
        assert!(!media_matches("screen", w, h));
        assert!(!media_matches("(color: 600px)", w, h));
        assert!(!media_matches("(min-depth: 600px)", w, h));
        assert!(!media_matches("(min-width: wide)", w, h));
        assert!(!media_matches("(min-width: 600em)", w, h));
    }

    #[test]
    fn lookup_path_walks_objects_and_arrays() {
        let state = json!({ "a": { "b": [ { "c": 1 } ] } });
        assert_eq!(lookup_path(&state, "a.b.0.c"), Some(&json!(1)));
        assert_eq!(lookup_path(&state, "a.x"), None);
        assert_eq!(lookup_path(&state, "a.b.5"), None);
        assert_eq!(lookup_path(&state, "a.b.c"), None);
    }

    #[test]
    fn resolved_binding_expands_loop_locals() {
        let locals = BTreeMap::from([("item".to_string(), "items.2".to_string())]);
        assert_eq!(resolved_binding("item.name", &locals), "items.2.name");
        assert_eq!(resolved_binding("item", &locals), "items.2");
        assert_eq!(resolved_binding("other.x", &locals), "other.x");
    }

    #[test]
    fn text_interpolation_uses_state_and_locals() {
        let state = json!({ "user": { "name": "ada" } });
        let locals = BTreeMap::from([("item".to_string(), json!({ "name": "local" }))]);
        assert_eq!(text_value("Hi {{user.name}}!", &state, &BTreeMap::new()), "Hi ada!");
        assert_eq!(text_value("{{item.name}}", &state, &locals), "local");
        assert_eq!(text_value("[{{missing}}]", &state, &BTreeMap::new()), "[]");
        assert_eq!(text_value("open {{oops", &state, &BTreeMap::new()), "open {{oops");
    }

    #[test]
    fn input_strings_render_null_as_empty() {
        assert_eq!(as_input_string(&json!(null)), "");
        assert_eq!(as_input_string(&json!(5)), "5");
        assert_eq!(as_input_string(&json!("text")), "text");
    }
}

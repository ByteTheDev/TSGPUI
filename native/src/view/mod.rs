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
    runtime::{JsBackend, UiCommand},
    menus::TSGPUIAction,
    style as styles,
    text_input::{self, Selection},
};


mod bindings;
mod layout;
mod inputs;
mod overlays;
mod rendering;

use bindings::*;
use layout::*;

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
    // Open <select> popup: the choices, the current value, and where the popup was requested.
    active_select: Option<SelectPopup>,
    // Scroll position of each data-virtual list, keyed by node key plus repeat suffix.
    scroll_handles: HashMap<String, UniformListScrollHandle>,
    // Multi-line <textarea> fields. Their scroll offset must survive frames, so the geometry does too.
    areas: HashMap<String, Rc<RefCell<AreaGeometry>>>,
    // Development overlay. The error is kept separately so a failed reload can show it in-window.
    build_error: Option<String>,
    panel_open: bool,
    panel_tab: PanelTab,
    inspector: bool,
    selected_node: Option<NodeSnapshot>,
    diagnostics: Vec<String>,
    dev: bool,
    persist_path: Option<PathBuf>,
    // Set by the backend's `window.setTitle`. Until then the document's title is shown.
    title_override: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelTab { Problems, Inspector }

#[derive(Clone)]
struct NodeSnapshot {
    tag: String,
    key: u64,
    id: Option<String>,
    class: Option<String>,
    attrs: BTreeMap<String, String>,
    style: BTreeMap<String, String>,
    states: BTreeMap<String, BTreeMap<String, String>>,
}

impl NodeSnapshot {
    fn from_node(node: &Node, style: &BTreeMap<String, String>) -> Self {
        Self {
            tag: node.tag.clone(),
            key: node.key,
            id: node.attrs.get("id").cloned(),
            class: node.attrs.get("class").cloned(),
            attrs: node.attrs.clone(),
            style: style.clone(),
            states: node.states.clone(),
        }
    }

    fn title(&self) -> String {
        let mut title = self.tag.clone();
        if let Some(id) = &self.id { title.push('#'); title.push_str(id); }
        if let Some(class) = &self.class {
            for name in class.split_whitespace() { title.push('.'); title.push_str(name); }
        }
        title
    }
}

#[derive(Clone)]
struct SelectPopup {
    at: Point<Pixels>,
    options: Vec<(String, String)>,
    selected: String,
    binding: Option<String>,
    action: Option<String>,
    local_key: u64,
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

impl AppView {
    pub fn new(document: AppPackage, mut backend: JsBackend, bundle_file: Option<PathBuf>) -> Self {
        // Opt-in persistent state: merge a previous run into the fresh defaults. Bad or oversized
        // files are ignored rather than preventing the app from starting.
        let persist_path = if document.persist { package::state_file(&document.name) } else { None };
        if let Some(path) = &persist_path {
            if let Ok(bytes) = std::fs::read(path) {
                if bytes.len() as u64 <= package::MAX_STATE_BYTES {
                    match serde_json::from_slice::<Value>(&bytes) {
                        Ok(saved) => backend.merge_saved(saved),
                        Err(error) => eprintln!("TSGPUI persisted state: ignoring invalid JSON: {error}"),
                    }
                }
            }
        }
        let diagnostics = document.diagnostics.clone();
        let dev = document.dev;
        Self {
            document, backend, bundle_file, local_values: HashMap::new(), cursor_offsets: HashMap::new(),
            selected_all: std::collections::HashSet::new(), validation_errors: HashMap::new(),
            fields: HashMap::new(), focus_handles: HashMap::new(), geometry: HashMap::new(),
            selections: HashMap::new(), edit_history: HashMap::new(), marked: None, focused_field: None,
            composition_undo: None, caret_visible: true, caret_blink_running: false, dragging_field: None,
            active_context_menu: None, active_select: None,
            scroll_handles: HashMap::new(), areas: HashMap::new(),
            build_error: None, panel_open: false, panel_tab: PanelTab::Problems, inspector: false,
            selected_node: None, diagnostics, dev, persist_path, title_override: None,
        }
    }

    fn start_hot_reload(&self, cx: &mut Context<Self>) {
        let Some(bundle_file) = self.bundle_file.clone() else { return; };
        // The CLI cannot write a bundle when compilation fails, so it leaves a sidecar next to
        // the last good bundle. The window shows that message and keeps running the old build.
        let error_file = bundle_file.with_extension("tgp.error");
        let mut last_stamp = bundle_stamp(&bundle_file);
        let mut last_error: Option<String> = None;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                // Compare the text, not just a timestamp: a second error can have the same length
                // and mtime resolution as the first.
                let error_text = match std::fs::read(&error_file) {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).trim().to_string();
                        Some(if text.is_empty() { "Build failed. Check the terminal for details.".to_string() } else { format!("Build failed: {text}") })
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => Some(format!("Build failed: could not read the build error: {error}")),
                };
                if error_text != last_error {
                    match &error_text {
                        Some(message) => eprintln!("TSGPUI hot reload: {message}"),
                        None if last_error.is_some() => eprintln!("TSGPUI hot reload: build error cleared"),
                        None => {}
                    }
                    last_error = error_text.clone();
                    let opened = error_text.is_some();
                    let result = this.update(cx, |view, cx| {
                        view.build_error = error_text;
                        if opened {
                            view.panel_open = true;
                            view.panel_tab = PanelTab::Problems;
                        }
                        cx.notify();
                    });
                    if result.is_err() { break; }
                }
                let stamp = bundle_stamp(&bundle_file);
                if stamp == last_stamp { continue; }
                last_stamp = stamp;
                let document = match package::load_bundle(&bundle_file) {
                    Ok(document) => document,
                    Err(error) => {
                        let message = format!("Build failed: {error}");
                        eprintln!("TSGPUI hot reload: {message}");
                        let _ = this.update(cx, |view, cx| {
                            view.build_error = Some(message);
                            view.panel_open = true;
                            view.panel_tab = PanelTab::Problems;
                            cx.notify();
                        });
                        continue;
                    }
                };
                if this.update(cx, |view, cx| {
                    // The new script starts fresh, then keeps the values the running app has changed.
                    // A failed reload leaves the running document and backend in place.
                    let backend = match JsBackend::reload(&document.backend_js, &view.backend) {
                        Ok(backend) => backend,
                        Err(error) => {
                            view.build_error = Some(format!("Build failed: {error}"));
                            view.panel_open = true;
                            view.panel_tab = PanelTab::Problems;
                            cx.notify();
                            return;
                        }
                    };
                    view.build_error = None;
                    view.diagnostics = document.diagnostics.clone();
                    view.dev = document.dev;
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
                    view.active_select = None;
                    view.selected_node = None;
                    view.scroll_handles.clear();
                    view.areas.clear();
                    cx.notify();
                }).is_err() {
                    break;
                }
            }
        }).detach();
    }

    fn start_persistence(&self, cx: &mut Context<Self>) {
        let Some(path) = self.persist_path.clone() else { return; };
        cx.spawn(async move |this, cx| {
            let mut last = String::new();
            let mut reported_size = false;
            loop {
                cx.background_executor().timer(Duration::from_millis(400)).await;
                let result = this.update(cx, |view, _| serde_json::to_string(view.backend.state()).ok());
                let Ok(Some(json)) = result else {
                    if result.is_err() { break; }
                    continue;
                };
                if json == last { continue; }
                if json.len() as u64 > package::MAX_STATE_BYTES {
                    if !reported_size {
                        eprintln!("TSGPUI persisted state: state exceeds {} bytes; not saving", package::MAX_STATE_BYTES);
                        reported_size = true;
                    }
                    continue;
                }
                match write_state_file(&path, json.as_bytes()) {
                    Ok(()) => { last = json; reported_size = false; }
                    Err(error) => eprintln!("TSGPUI persisted state: {error}"),
                }
            }
        }).detach();
    }

    // Runs the backend's pending work on the UI thread. Host results are delivered here, and window
    // requests are applied to this window. The view redraws only when something changed.
    fn start_backend_pump(&self, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let result = this.update_in(cx, |view, window, cx| {
                    let pump = view.backend.pump();
                    for error in &pump.errors {
                        eprintln!("TSGPUI: {error}");
                    }
                    let commands = view.backend.take_ui_commands();
                    let changed = pump.changed || !commands.is_empty();
                    for command in commands {
                        view.apply_ui_command(window, command);
                    }
                    if changed {
                        cx.notify();
                    }
                });
                if result.is_err() { break; }
            }
        }).detach();
    }

    fn apply_ui_command(&mut self, window: &mut Window, command: UiCommand) {
        match command {
            UiCommand::Minimize => window.minimize_window(),
            // GPUI's zoom toggles, so only call it when the window is in the other state.
            UiCommand::Maximize => if !window.is_maximized() { window.zoom_window() },
            UiCommand::Restore => if window.is_maximized() { window.zoom_window() },
            UiCommand::Close => window.remove_window(),
            UiCommand::SetTitle(title) => self.title_override = Some(title),
        }
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

}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(self.title_override.as_deref().unwrap_or(&self.document.title));
        self.focused_field = None;
        let root = self.document.root.clone();
        let content = self.render_node(&root, cx, window, &BTreeMap::new(), &BTreeMap::new(), None, None, "", false);
        let mut stack = div().size_full().child(content);
        if let Some(overlay) = self.select_overlay(window, cx) { stack = stack.child(overlay); }
        if self.panel_open { stack = stack.child(self.dev_overlay(cx)); }
        stack
    }
}

fn bundle_stamp(path: &std::path::Path) -> Option<(u64, SystemTime)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

// Write through a sibling temp file so a crash never leaves a half-written state file.
fn write_state_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, bytes)?;
    if std::fs::rename(&temp, path).is_ok() { return Ok(()); }
    // Windows does not replace an existing file with rename.
    if path.exists() { std::fs::remove_file(path)?; }
    std::fs::rename(&temp, path)
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
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| {
                let view = AppView::new(doc, backend, bundle_file);
                view.start_hot_reload(cx);
                view.start_persistence(cx);
                view
            });
            view.update(cx, |view, cx| view.start_backend_pump(window, cx));
            view
        })
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

    #[test]
    fn viewport_units_resolve_against_the_window() {
        let mut s = style(&[("width", "50vw"), ("height", "25vh"), ("gap", "10vmin"), ("margin", "4vmax"), ("color", "#ff0000ff")]);
        resolve_viewport_units(&mut s, px(800.0), px(600.0));
        assert_eq!(s["width"], "400px");
        assert_eq!(s["height"], "150px");
        assert_eq!(s["gap"], "60px");
        assert_eq!(s["margin"], "32px");
        assert_eq!(s["color"], "#ff0000ff");
    }

    #[test]
    fn grid_tracks_are_parsed_and_mixed_tracks_emulated() {
        assert_eq!(parse_grid_tracks("240px 1fr 2.5fr auto"), Some(vec![
            GridTrack::Px(240.0), GridTrack::Fr(1.0), GridTrack::Fr(2.5), GridTrack::Auto,
        ]));
        assert_eq!(parse_grid_tracks("1fr 1fr"), Some(vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0)]));
        assert_eq!(parse_grid_tracks("repeat(2, 1fr)"), None);
        assert_eq!(parse_grid_tracks("1fr -2px"), None);
        assert!(!grid_needs_emulation(&[GridTrack::Fr(1.0), GridTrack::Fr(2.0)]));
        assert!(grid_needs_emulation(&[GridTrack::Px(200.0), GridTrack::Fr(1.0)]));
        assert!(grid_needs_emulation(&[GridTrack::Auto, GridTrack::Auto]));
    }
}

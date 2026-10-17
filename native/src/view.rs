use std::{collections::{BTreeMap, HashMap}, io::Read, path::{Path, PathBuf}, sync::Arc, time::{Duration, SystemTime}};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures::io::AsyncReadExt as _;
use gpui::{
    App, AppContext, AnyElement, Application, Bounds, ClipboardItem, Context, FontWeight, Image,
    ImageFormat, PathPromptOptions, PromptLevel, Window, WindowBounds, WindowOptions, div, img,
    http_client,
    prelude::*, px, rgb, size,
};
use serde_json::Value;

use crate::{network::NativeHttpClient, package::{self, AppPackage, Node}, runtime::JsBackend};

pub struct AppView {
    document: AppPackage,
    backend: JsBackend,
    bundle_file: Option<PathBuf>,
    local_values: HashMap<u64, Value>,
    cursor_offsets: HashMap<u64, usize>,
    selected_all: std::collections::HashSet<u64>,
    validation_errors: HashMap<u64, String>,
}

fn px_value(style: &BTreeMap<String, String>, key: &str) -> Option<f32> {
    style.get(key)?.strip_suffix("px")?.parse::<f32>().ok()
}

fn hex_color(s: &str) -> Option<u32> {
    let value = s.strip_prefix('#')?;
    if value.len() == 6 {
        u32::from_str_radix(value, 16).ok()
    } else if value.len() == 3 {
        let chars: Vec<char> = value.chars().collect();
        let full = format!("{0}{0}{1}{1}{2}{2}", chars[0], chars[1], chars[2]);
        u32::from_str_radix(&full, 16).ok()
    } else {
        None
    }
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
    if !node.attrs.contains_key("disabled") && node.attrs.get("data-shortcut").is_some_and(|shortcut| shortcut_matches(shortcut, event)) {
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
        Self { document, backend, bundle_file, local_values: HashMap::new(), cursor_offsets: HashMap::new(), selected_all: std::collections::HashSet::new(), validation_errors: HashMap::new() }
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
                let backend = match JsBackend::new(&document.backend_js) {
                    Ok(backend) => backend,
                    Err(error) => { eprintln!("TSGPUI hot reload: {error}"); continue; }
                };
                if this.update(cx, |view, cx| {
                    view.document = document;
                    view.backend = backend;
                    view.local_values.clear();
                    view.cursor_offsets.clear();
                    view.selected_all.clear();
                    view.validation_errors.clear();
                    cx.notify();
                }).is_err() {
                    break;
                }
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
        if let Some(color) = style.get("background").and_then(|value| hex_color(value)) { element = element.bg(rgb(color)); }
        if let Some(color) = style.get("color").and_then(|value| hex_color(value)) { element = element.text_color(rgb(color)); }
        if let Some(value) = px_value(&style, "width") { element = element.w(px(value)); }
        if let Some(value) = px_value(&style, "height") { element = element.h(px(value)); }
        if let Some(value) = px_value(&style, "padding") { element = element.p(px(value)); }
        if let Some(value) = px_value(&style, "margin") { element = element.m(px(value)); }
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
        if let Some(value) = style.get("opacity").and_then(|value| value.parse::<f32>().ok()) { element = element.opacity(value); }
        if let Some(weight) = style.get("font-weight") {
            let font_weight = match weight.as_str() {
                "bold" | "700" => FontWeight::BOLD,
                "600" => FontWeight::SEMIBOLD,
                "500" => FontWeight::MEDIUM,
                _ => FontWeight::NORMAL,
            };
            element = element.font_weight(font_weight);
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

        if ["input", "textarea"].contains(&node.tag.as_str()) && node.attrs.get("type").map(String::as_str) != Some("checkbox") {
            let key = node.key;
            let binding = node.attrs.get("data-bind").map(|path| resolved_binding(path, local_paths));
            let action = node.attrs.get("data-oninput").cloned();
            let input_tag = node.tag.clone();
            let is_textarea = input_tag == "textarea";
            let input_type = node.attrs.get("type").cloned().unwrap_or_else(|| "text".into());
            let initial_value = node.attrs.get("value").cloned().or_else(|| is_textarea.then(|| node_text(node))).unwrap_or_default();
            let input_name = node.attrs.get("name").cloned();
            let displayed = node.attrs.get("data-bind").and_then(|path| {
                locals.get(path.split('.').next().unwrap_or_default())
                    .and_then(|local| path.split_once('.').and_then(|(_, tail)| lookup_path(local, tail)))
                    .or_else(|| binding.as_ref().and_then(|path| lookup_path(&state, path)))
            }).map(as_input_string)
                .or_else(|| self.local_values.get(&key).map(as_input_string))
                .or_else(|| Some(initial_value.clone())).unwrap_or_default();
            let placeholder = node.attrs.get("placeholder").cloned().unwrap_or_default();
            let text = if node.attrs.get("type").is_some_and(|kind| kind == "password") {
                "*".repeat(displayed.chars().count())
            } else if displayed.is_empty() && !placeholder.is_empty() { placeholder } else { displayed };
            if enabled { element = element.tab_index(0); }
            element = element.child(text);
            if enabled { element = element.on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                let mut value = binding.as_ref().and_then(|path| lookup_path(this.backend.state(), path)).map(as_input_string)
                    .or_else(|| this.local_values.get(&key).map(as_input_string))
                    .or_else(|| Some(initial_value.clone())).unwrap_or_default();
                let previous = value.clone();
                let mut characters: Vec<char> = value.chars().collect();
                let mut cursor = this.cursor_offsets.get(&key).copied().unwrap_or(characters.len()).min(characters.len());
                let key_name = event.keystroke.key.to_ascii_lowercase();
                if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform) && key_name == "a" {
                    this.selected_all.insert(key);
                    cursor = characters.len();
                } else if key_name == "left" {
                    cursor = cursor.saturating_sub(1);
                    this.selected_all.remove(&key);
                } else if key_name == "right" {
                    cursor = (cursor + 1).min(characters.len());
                    this.selected_all.remove(&key);
                } else if key_name == "home" {
                    cursor = 0;
                    this.selected_all.remove(&key);
                } else if key_name == "end" {
                    cursor = characters.len();
                    this.selected_all.remove(&key);
                } else if matches!(key_name.as_str(), "backspace" | "delete") {
                    if this.selected_all.remove(&key) {
                        characters.clear();
                        cursor = 0;
                    } else if key_name == "backspace" && cursor > 0 {
                        characters.remove(cursor - 1);
                        cursor -= 1;
                    } else if key_name == "delete" && cursor < characters.len() {
                        characters.remove(cursor);
                    }
                } else if key_name == "enter" && is_textarea {
                    if this.selected_all.remove(&key) { characters.clear(); cursor = 0; }
                    characters.insert(cursor, '\n');
                    cursor += 1;
                } else if let Some(chars) = event.keystroke.key_char.as_ref() {
                    if !event.keystroke.modifiers.control && !event.keystroke.modifiers.platform && !event.keystroke.modifiers.alt {
                        if this.selected_all.remove(&key) { characters.clear(); cursor = 0; }
                        for character in chars.chars() {
                            characters.insert(cursor, character);
                            cursor += 1;
                        }
                    }
                }
                value = characters.into_iter().collect();
                this.cursor_offsets.insert(key, cursor);
                if value != previous {
                    if is_textarea && key_name == "enter" { cx.stop_propagation(); }
                    let state_value = if input_type == "number" {
                        value.parse::<f64>().ok().and_then(serde_json::Number::from_f64)
                            .map(Value::Number).unwrap_or_else(|| Value::String(value.clone()))
                    } else { Value::String(value.clone()) };
                    if let Some(binding) = &binding {
                        if let Err(error) = this.backend.set_state(binding, state_value.clone()) { eprintln!("TSGPUI: {error}"); }
                    } else { this.local_values.insert(key, state_value.clone()); }
                    if let Some(action) = &action {
                        if let Err(error) = this.backend.invoke_with_event(action, serde_json::json!({"type":"input","value":state_value,"name":input_name})) { eprintln!("TSGPUI: {error}"); }
                    }
                }
                cx.notify();
            })); }
            return element.into_any_element();
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
        element.into_any_element()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.document.title);
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
        let bounds = Bounds::centered(None, size(px(doc.width as f32), px(doc.height as f32)), cx);
        cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        }, move |_, cx| cx.new(|cx| {
            let view = AppView::new(doc, backend, bundle_file);
            view.start_hot_reload(cx);
            view
        }))
            .expect("Failed to open native GPUI window");
        cx.activate(true);
    });
}

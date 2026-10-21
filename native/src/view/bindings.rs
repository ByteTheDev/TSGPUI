use super::*;

pub(super) fn lookup_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |current, part| match current {
        Value::Array(items) => part.parse::<usize>().ok().and_then(|index| items.get(index)),
        Value::Object(object) => object.get(part),
        _ => None,
    })
}

pub(super) fn resolved_binding(path: &str, local_paths: &BTreeMap<String, String>) -> String {
    let mut pieces = path.splitn(2, '.');
    let head = pieces.next().unwrap_or_default();
    match (local_paths.get(head), pieces.next()) {
        (Some(base), Some(tail)) => format!("{base}.{tail}"),
        (Some(base), None) => base.clone(),
        _ => path.to_string(),
    }
}

pub(super) fn as_input_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub(super) fn printable(value: Option<&Value>) -> String {
    value.map(as_input_string).unwrap_or_default()
}

pub(super) fn text_value(text: &str, state: &Value, locals: &BTreeMap<String, Value>) -> String {
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

pub(super) fn node_text(node: &Node) -> String {
    if node.tag == "#text" { return node.text.clone(); }
    node.children.iter().map(node_text).collect::<Vec<_>>().join(" ")
}

pub(super) fn image_format(format: &str) -> Option<ImageFormat> {
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

pub(super) fn shortcut_matches(shortcut: &str, event: &gpui::KeyDownEvent) -> bool {
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

pub(super) fn find_shortcut<'a>(node: &'a Node, event: &gpui::KeyDownEvent) -> Option<&'a Node> {
    if matches!(node.tag.as_str(), "button" | "menuitem") && !node.attrs.contains_key("disabled")
        && node.attrs.get("data-shortcut").is_some_and(|shortcut| shortcut_matches(shortcut, event)) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_shortcut(child, event))
}

pub(super) fn form_is_valid(node: &Node, state: &Value, local_values: &HashMap<u64, Value>) -> bool {
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


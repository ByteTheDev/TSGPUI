use super::*;

impl AppView {
    pub(super) fn render_node(
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
        let grid_tracks = grid_tracks_for(node, &style);
        let is_form = node.tag == "form";
        let disabled = node.attrs.contains_key("disabled");
        // While the inspector is active, app controls behave like plain boxes so a click selects
        // the element instead of invoking it.
        let enabled = !disabled && !self.inspector;
        let form_action = if is_form { node.attrs.get("data-submit").cloned().or(inherited_submit) } else { inherited_submit };
        let form_node = if is_form { Some(node.clone()) } else { inherited_form };
        let element_id: gpui::SharedString = format!("tsgpui:{}{}", node.key, repeat_suffix).into();
        let mut element = div().id(element_id);
        match style.get("display").map(String::as_str) {
            Some("block") => element = element.block(),
            Some("grid") => {
                if grid_tracks.is_some() {
                    element = element.flex().flex_col();
                } else {
                    element = element.grid();
                    if let Some(columns) = style.get("grid-template-columns") {
                        element = element.grid_cols(columns.split_whitespace().count().clamp(1, 12) as u16);
                    }
                    if let Some(rows) = style.get("grid-template-rows") {
                        element = element.grid_rows(rows.split_whitespace().count().clamp(1, 12) as u16);
                    }
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
        if disabled {
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

        // Development inspector: select the element under the pointer. The listener runs before the
        // app's own handlers (which are disabled above while `inspector` is true).
        if self.selected_node.as_ref().is_some_and(|snapshot| snapshot.key == node.key) {
            element = element.border_1().border_color(rgba(0x3b82f6ff));
        }
        if self.inspector {
            let snapshot = NodeSnapshot::from_node(node, &style);
            element = element.on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.selected_node = Some(snapshot.clone());
                this.panel_open = true;
                this.panel_tab = PanelTab::Inspector;
                cx.stop_propagation();
                cx.notify();
            }));
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
            let label = options.iter().find(|option| option.0 == selected).map(|option| option.1.clone()).unwrap_or_else(|| selected.clone());
            element = element.cursor_pointer().child(label);
            let action = node.attrs.get("data-oninput").cloned();
            if enabled {
                let popup_options = options.clone();
                let popup_selected = selected.clone();
                let popup_binding = binding.clone();
                let popup_action = action.clone();
                element = element.on_mouse_down(MouseButton::Left, cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    if popup_options.is_empty() { return; }
                    this.active_select = Some(SelectPopup {
                        at: event.position,
                        options: popup_options.clone(),
                        selected: popup_selected.clone(),
                        binding: popup_binding.clone(),
                        action: popup_action.clone(),
                        local_key: key,
                    });
                    this.active_context_menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }));
            }
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
                    this.active_select = None;
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
            let method = node.attrs.get("data-fetch-method").map(|value| value.to_ascii_uppercase()).unwrap_or_else(|| "GET".into());
            let body_key = node.attrs.get("data-fetch-body").cloned();
            let content_type = node.attrs.get("data-fetch-content-type").cloned();
            let status_key = node.attrs.get("data-fetch-status-into").cloned();
            let error_key = node.attrs.get("data-fetch-error-into").cloned();
            let allowed_hosts = self.document.permissions.network_hosts.clone();
            element = element.on_click(cx.listener(move |this, _, _, cx| {
                let parsed = match http_client::Url::parse(&request_url) {
                    Ok(url) => url,
                    Err(error) => { eprintln!("TSGPUI network request: invalid URL: {error}"); return; }
                };
                let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
                if parsed.scheme() != "https" || parsed.port_or_known_default() != Some(443) || !allowed_hosts.iter().any(|allowed| allowed == &host) {
                    eprintln!("TSGPUI network request: host is not permitted: {host}");
                    return;
                }
                // A POST body is a state key. Strings are sent as text; other values are serialized
                // as JSON when the request declares a JSON content type.
                let body_text = body_key.as_ref().map(|key| {
                    let value = lookup_path(this.backend.state(), key).cloned().unwrap_or(Value::Null);
                    let json = content_type.as_deref().is_some_and(|kind| kind.to_ascii_lowercase().starts_with("application/json"));
                    if json && !value.is_string() { serde_json::to_string(&value).unwrap_or_default() } else { as_input_string(&value) }
                }).unwrap_or_default();
                if body_text.len() > 1024 * 1024 {
                    eprintln!("TSGPUI network request: request body exceeds 1 MiB");
                    return;
                }
                let target_key = target_key.clone();
                let status_key = status_key.clone();
                let error_key = error_key.clone();
                let content_type = content_type.clone();
                let method = method.clone();
                let client = cx.http_client();
                cx.spawn(async move |this, cx| {
                    let result: Result<(u16, String), (Option<u16>, anyhow::Error)> = async {
                        let request_method = match method.as_str() {
                            "POST" => http_client::Method::POST,
                            _ => http_client::Method::GET,
                        };
                        let mut builder = http_client::Request::builder().method(request_method).uri(parsed.as_str());
                        if let Some(content_type) = &content_type { builder = builder.header("Content-Type", content_type.as_str()); }
                        let request = builder.body(http_client::AsyncBody::from(body_text)).map_err(|error| (None, error.into()))?;
                        let response = client.send(request).await.map_err(|error| (None, error))?;
                        let status = response.status().as_u16();
                        if !response.status().is_success() { return Err((Some(status), anyhow::anyhow!("HTTP status {status}"))); }
                        let body = response.into_body();
                        let mut bytes = Vec::new();
                        body.take(1024 * 1024 + 1).read_to_end(&mut bytes).await.map_err(|error| (Some(status), error.into()))?;
                        if bytes.len() > 1024 * 1024 { return Err((Some(status), anyhow::anyhow!("response body exceeds 1 MiB"))); }
                        let text = String::from_utf8(bytes).map_err(|error| (Some(status), error.into()))?;
                        Ok((status, text))
                    }.await;
                    match result {
                        Ok((status, text)) => { let _ = this.update(cx, |this, cx| {
                            if let Err(error) = this.backend.set_state(&target_key, Value::String(text)) { eprintln!("TSGPUI network request: {error}"); }
                            if let Some(key) = &status_key {
                                if let Err(error) = this.backend.set_state(key, Value::from(status)) { eprintln!("TSGPUI network request: {error}"); }
                            }
                            if let Some(key) = &error_key {
                                if let Err(error) = this.backend.set_state(key, Value::String(String::new())) { eprintln!("TSGPUI network request: {error}"); }
                            }
                            cx.notify();
                        }); }
                        Err((status, error)) => {
                            eprintln!("TSGPUI network request: {error}");
                            let _ = this.update(cx, |this, cx| {
                                if let Some(key) = &status_key {
                                    if let Some(status) = status {
                                        if let Err(error) = this.backend.set_state(key, Value::from(status)) { eprintln!("TSGPUI network request: {error}"); }
                                    }
                                }
                                if let Some(key) = &error_key {
                                    if let Err(error) = this.backend.set_state(key, Value::String(error.to_string())) { eprintln!("TSGPUI network request: {error}"); }
                                }
                                cx.notify();
                            });
                        }
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
                let keystroke = &event.keystroke;
                if keystroke.key.eq_ignore_ascii_case("f12")
                    || (keystroke.key.eq_ignore_ascii_case("i") && keystroke.modifiers.control && keystroke.modifiers.shift) {
                    this.panel_open = !this.panel_open;
                    if !this.panel_open { this.inspector = false; }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if keystroke.key.eq_ignore_ascii_case("escape") {
                    let closed_popup = this.active_select.take().is_some();
                    let closed_menu = this.active_context_menu.take().is_some();
                    if closed_popup || closed_menu {
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                    if this.panel_open {
                        this.panel_open = false;
                        this.inspector = false;
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                }
                if keystroke.key.eq_ignore_ascii_case("tab") {
                    if keystroke.modifiers.shift { window.focus_prev(); } else { window.focus_next(); }
                    cx.stop_propagation();
                    return;
                }
                if let Some(button) = find_shortcut(&this.document.root, event) {
                    if !this.inspector {
                        if let Some(action) = button.attrs.get("data-onclick") {
                            if let Err(error) = this.backend.invoke_with_event(action, serde_json::json!({"type":"shortcut","key":event.keystroke.key})) { eprintln!("TSGPUI: {error}"); }
                            cx.notify();
                            cx.stop_propagation();
                        }
                    }
                }
            }));
            element = element.on_action(cx.listener(|this, action: &TSGPUIAction, _, cx| {
                if this.inspector { return; }
                if let Err(error) = this.backend.invoke_with_event(&action.action, serde_json::json!({"type":"menu"})) {
                    eprintln!("TSGPUI menu action: {error}");
                }
                cx.notify();
            }));
        }
        if let Some(menu_id) = node.attrs.get("data-contextmenu").filter(|_| enabled) {
            let menu_id = menu_id.clone();
            element = element.on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.active_select = None;
                this.active_context_menu = Some((menu_id.clone(), event.position));
                cx.notify();
            }));
        }
        if let Some(action) = node.attrs.get("data-ondrop").filter(|_| enabled) {
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
            if let Some(tracks) = grid_tracks.as_deref() {
                element = self.render_grid_rows(node, element, tracks, locals, local_paths, form_action.clone(), form_node.clone(), repeat_suffix, cx, window);
            } else {
                // Paint order for absolutely positioned siblings follows z-index. Negative layers go
                // behind normal flow, non-negative layers above it. This is not a full stacking context.
                let mut negative = Vec::new();
                let mut flow = Vec::new();
                let mut positive = Vec::new();
                for child in &node.children {
                    let child_style = computed_style(child, window);
                    if child_style.get("position").is_some_and(|value| value == "absolute") {
                        let z = child_style.get("z-index").and_then(|value| value.parse::<i32>().ok()).unwrap_or(0);
                        if z < 0 { negative.push((z, child)); } else { positive.push((z, child)); }
                    } else {
                        flow.push(child);
                    }
                }
                negative.sort_by_key(|(z, _)| *z);
                positive.sort_by_key(|(z, _)| *z);
                for (_, child) in negative {
                    element = element.child(self.render_node(child, cx, window, locals, local_paths, form_action.clone(), form_node.clone(), repeat_suffix, false));
                }
                for child in flow {
                    element = element.child(self.render_node(child, cx, window, locals, local_paths, form_action.clone(), form_node.clone(), repeat_suffix, false));
                }
                for (_, child) in positive {
                    element = element.child(self.render_node(child, cx, window, locals, local_paths, form_action.clone(), form_node.clone(), repeat_suffix, false));
                }
            }
            if node.children.is_empty() && node.tag == "button" {
                if let Some(label) = node.attrs.get("aria-label") { element = element.child(label.clone()); }
            }
            if is_form {
                if let Some(message) = self.validation_errors.get(&node.key) { element = element.child(div().child(message.clone())); }
            }
        }
        if node.tag == "form" && !self.inspector {
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

    // Fixed and mixed grid templates are emulated with flex rows. Each row contains one cell per
    // column track: px tracks are fixed, fr tracks share the remainder, and auto tracks size to
    // their content. This keeps the common "sidebar + content" layout from collapsing to equal
    // columns, without pretending to implement full grid placement.
    #[allow(clippy::too_many_arguments)]
    fn render_grid_rows(
        &mut self,
        node: &Node,
        element: Stateful<Div>,
        tracks: &[GridTrack],
        locals: &BTreeMap<String, Value>,
        local_paths: &BTreeMap<String, String>,
        form_action: Option<String>,
        form_node: Option<Node>,
        repeat_suffix: &str,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) -> Stateful<Div> {
        let style = computed_style(node, window);
        let column_gap = px_value(&style, "column-gap").or_else(|| px_value(&style, "gap")).unwrap_or(0.0);
        let row_gap = px_value(&style, "row-gap").or_else(|| px_value(&style, "gap")).unwrap_or(0.0);
        let row_tracks = style.get("grid-template-rows").and_then(|value| parse_grid_tracks(value)).unwrap_or_default();
        let items: Vec<&Node> = node.children.iter()
            .filter(|child| !(child.tag == "#text" && child.text.trim().is_empty()))
            .collect();
        let mut rows = div().flex().flex_col().w_full();
        if row_gap > 0.0 { rows = rows.gap(px(row_gap)); }
        for (row_index, chunk) in items.chunks(tracks.len()).enumerate() {
            let mut row = div().flex().flex_row().w_full();
            if column_gap > 0.0 { row = row.gap(px(column_gap)); }
            if let Some(GridTrack::Px(height)) = row_tracks.get(row_index) { row = row.h(px(*height)); }
            for (column, child) in chunk.iter().enumerate() {
                let mut cell = div();
                match tracks[column] {
                    GridTrack::Px(width) => { cell = cell.w(px(width)).flex_shrink_0(); }
                    GridTrack::Fr(grow) => {
                        { cell.style().flex_grow = Some(grow); }
                        cell = cell.flex_basis(px(0.0)).min_w(px(0.0));
                    }
                    GridTrack::Auto => { cell = cell.flex_shrink_0(); }
                }
                cell = cell.child(self.render_node(child, cx, window, locals, local_paths, form_action.clone(), form_node.clone(), repeat_suffix, false));
                row = row.child(cell);
            }
            rows = rows.child(row);
        }
        element.child(rows)
    }
}

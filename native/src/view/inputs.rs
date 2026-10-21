use super::*;

impl AppView {
    pub(super) fn render_multi_line_input(
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
    pub(super) fn render_single_line_input(
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

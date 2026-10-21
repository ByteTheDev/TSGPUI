use super::*;

impl AppView {
    pub(super) fn context_menu_overlay(&self, root: &Node, cx: &mut Context<Self>) -> Option<AnyElement> {
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

    fn commit_select_value(&mut self, value: &str, binding: Option<&str>, action: Option<&str>, local_key: u64) {
        if let Some(binding) = binding {
            if let Err(error) = self.backend.set_state(binding, Value::String(value.to_string())) {
                eprintln!("TSGPUI: {error}");
            }
        } else {
            self.local_values.insert(local_key, Value::String(value.to_string()));
        }
        if let Some(action) = action {
            if let Err(error) = self.backend.invoke_with_event(action, serde_json::json!({"type":"input","value":value})) {
                eprintln!("TSGPUI: {error}");
            }
        }
        self.active_select = None;
    }

    // Native dropdown for <select>. GPUI has no menu primitive, so the options are a positioned
    // overlay with a full-window dismiss layer, like the context menu.
    pub(super) fn select_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let popup = self.active_select.clone()?;
        let bounds = window.bounds();
        let left = f32::from(popup.at.x).min((f32::from(bounds.size.width) - 200.0).max(0.0));
        let top = (f32::from(popup.at.y) + 2.0).min((f32::from(bounds.size.height) - 290.0).max(0.0));
        let mut menu = div().id("tsgpui-select-menu").absolute().left(px(left)).top(px(top))
            .min_w(px(180.0)).max_w(px(320.0)).max_h(px(280.0)).overflow_scroll()
            .p(px(4.0)).bg(rgba(0x252525ff)).border_1().border_color(rgba(0x555555ff))
            .rounded_md().shadow_lg();
        for (index, (value, label)) in popup.options.iter().enumerate() {
            let selected = value == &popup.selected;
            let mut row = div().id(("select-option", index))
                .px(px(8.0)).py(px(6.0)).rounded_sm().cursor_pointer()
                .text_size(px(13.0))
                .text_color(if selected { rgba(0x93c5fdff) } else { rgba(0xf3f3f3ff) })
                .child(format!("{} {}", if selected { "✓" } else { " " }, label));
            if selected { row = row.bg(rgba(0x1e3a5f80)); }
            let value = value.clone();
            let binding = popup.binding.clone();
            let action = popup.action.clone();
            let local_key = popup.local_key;
            row = row.on_click(cx.listener(move |this, _, _, cx| {
                this.commit_select_value(&value, binding.as_deref(), action.as_deref(), local_key);
                cx.notify();
            }));
            menu = menu.child(row);
        }
        let mut overlay = div().absolute().top_0().left_0().size_full();
        overlay = overlay.on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.active_select = None;
            cx.notify();
        }));
        menu = menu.on_mouse_down(MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
        }));
        Some(overlay.child(menu).into_any_element())
    }

    // The development panel shares one overlay for build problems and element inspection.
    pub(super) fn dev_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div().id("tsgpui-dev-panel").absolute().top(px(10.0)).right(px(10.0))
            .w(px(380.0)).max_h(px(480.0)).flex().flex_col().overflow_hidden()
            .bg(rgba(0x18181bff)).border_1().border_color(rgba(0x52525bff)).rounded_lg().shadow_lg()
            .text_size(px(12.0)).text_color(rgba(0xe4e4e7ff));

        let mut problems_tab = div().id("dev-tab-problems").px(px(8.0)).py(px(3.0)).rounded_sm().cursor_pointer().child("Problems");
        if self.panel_tab == PanelTab::Problems { problems_tab = problems_tab.bg(rgba(0x3f3f46ff)); }
        problems_tab = problems_tab.on_click(cx.listener(|this, _, _, cx| {
            this.panel_tab = PanelTab::Problems;
            cx.notify();
        }));
        let mut inspector_tab = div().id("dev-tab-inspector").px(px(8.0)).py(px(3.0)).rounded_sm().cursor_pointer()
            .child(if self.inspector { "Inspector: on" } else { "Inspector: off" });
        if self.panel_tab == PanelTab::Inspector { inspector_tab = inspector_tab.bg(rgba(0x3f3f46ff)); }
        inspector_tab = inspector_tab.on_click(cx.listener(|this, _, _, cx| {
            this.inspector = !this.inspector;
            if this.inspector { this.panel_tab = PanelTab::Inspector; }
            cx.notify();
        }));
        let close = div().id("dev-close").px(px(7.0)).py(px(1.0)).rounded_sm().cursor_pointer()
            .text_color(rgba(0xa1a1aaff)).child("×")
            .on_click(cx.listener(|this, _, _, cx| {
                this.panel_open = false;
                this.inspector = false;
                this.selected_node = None;
                cx.notify();
            }));
        let header = div().flex().flex_row().items_center().gap(px(6.0)).px(px(8.0)).py(px(6.0))
            .child(problems_tab).child(inspector_tab).child(div().flex_grow()).child(close);
        panel = panel.child(header).child(div().h(px(1.0)).bg(rgba(0x3f3f46ff)));

        if let Some(error) = &self.build_error {
            panel = panel.child(div().p(px(8.0)).bg(rgba(0x7f1d1dff)).text_color(rgba(0xfecacaff))
                .child("Build failed. The last successful build is still running.").child(error.clone()));
        }

        let mut body = div().id("dev-body").flex().flex_col().gap(px(3.0)).p(px(8.0))
            .flex_grow().min_h_0().overflow_scroll();
        match self.panel_tab {
            PanelTab::Problems => {
                if self.dev {
                    body = body.child(div().text_color(rgba(0xa1a1aaff)).child("Hot reload is on: save a source file to rebuild."));
                }
                if self.diagnostics.is_empty() {
                    body = body.child(div().text_color(rgba(0xa1a1aaff)).child("No compile warnings."));
                } else {
                    for warning in &self.diagnostics {
                        body = body.child(div().py(px(3.0)).text_color(rgba(0xfbbf24ff)).child(warning.clone()));
                    }
                }
            }
            PanelTab::Inspector => {
                if let Some(selected) = &self.selected_node {
                    body = body.child(div().text_color(rgba(0x93c5fdff)).child(selected.title()));
                    body = body.child(div().text_color(rgba(0xa1a1aaff)).child(format!("node key {}", selected.key)));
                    if !selected.attrs.is_empty() {
                        body = body.child(div().pt(px(4.0)).text_color(rgba(0xd4d4d8ff)).child("Attributes"));
                        for (key, value) in &selected.attrs {
                            body = body.child(div().pl(px(6.0)).child(format!("{key}=\"{value}\"")));
                        }
                    }
                    body = body.child(div().pt(px(4.0)).text_color(rgba(0xd4d4d8ff)).child("Computed style"));
                    for (key, value) in &selected.style {
                        body = body.child(div().pl(px(6.0)).child(format!("{key}: {value}")));
                    }
                    if !selected.states.is_empty() {
                        body = body.child(div().pt(px(4.0)).text_color(rgba(0xd4d4d8ff)).child("State styles"));
                        for (state, declarations) in &selected.states {
                            body = body.child(div().pl(px(6.0)).text_color(rgba(0xfbbf24ff)).child(format!(":{state}")));
                            for (key, value) in declarations {
                                body = body.child(div().pl(px(14.0)).child(format!("{key}: {value}")));
                            }
                        }
                    }
                } else {
                    body = body.child(div().text_color(rgba(0xa1a1aaff))
                        .child("Turn the inspector on, then click an element to see the styles that applied. Declarations the compiler skipped are listed in Problems."));
                }
            }
        }
        panel = panel.child(body).child(div().px(px(8.0)).py(px(4.0)).text_color(rgba(0x71717aff))
            .child("F12 or Ctrl+Shift+I to close · Esc closes the panel"));
        panel.on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.stop_propagation())).into_any_element()
    }
}

//! A running application: engine + JavaScript runtime + the input state machine.
//!
//! `Session` is platform-independent. The window driver and the headless driver both feed it
//! pointer, wheel, keyboard and text events and call [`Session::frame`]; it turns them into DOM
//! events, default actions (focus, click activation, caret placement, text editing, Tab traversal)
//! and engine updates.

use crate::bundle::AppBundle;
use rendrix_core::{EditOp, Engine};
use rendrix_css::Cursor;
use rendrix_dom::{NodeId, NodeKind};
use rendrix_js::{JsRuntime, WindowCommand};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// A keyboard event in DOM terms.
#[derive(Debug, Clone, Default)]
pub struct KeyInput {
    /// `KeyboardEvent.key` (`"a"`, `"Enter"`, `"ArrowLeft"`, …).
    pub key: String,
    /// `KeyboardEvent.code` (`"KeyA"`, `"Enter"`, …).
    pub code: String,
    /// Text the key produced, if any.
    pub text: Option<String>,
    pub repeat: bool,
    pub mods: Mods,
}

pub trait Clipboard {
    fn get(&mut self) -> String;
    fn set(&mut self, text: &str);
}

#[derive(Default)]
pub struct MemoryClipboard(pub String);
impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> String {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = text.to_string();
    }
}

pub struct SystemClipboard(Option<arboard::Clipboard>);
impl SystemClipboard {
    pub fn new() -> Self {
        SystemClipboard(arboard::Clipboard::new().ok())
    }
}
impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}
impl Clipboard for SystemClipboard {
    fn get(&mut self) -> String {
        self.0.as_mut().and_then(|c| c.get_text().ok()).unwrap_or_default()
    }
    fn set(&mut self, text: &str) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.set_text(text.to_string());
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FrameInfo {
    /// The engine produced a new display list.
    pub repainted: bool,
    /// Another frame is needed right away (animations, rAF).
    pub continuous: bool,
    /// Runtime-clock seconds at which something time-driven needs attention.
    pub next_wake_s: Option<f64>,
    pub cursor: Option<Cursor>,
    pub quit: bool,
    pub title: Option<String>,
    pub resize: Option<(f32, f32)>,
}

struct ClickState {
    time: Instant,
    pos: (f32, f32),
    count: u32,
}

#[derive(Default)]
struct InputState {
    pos: (f32, f32),
    hover: Option<NodeId>,
    pressed: Option<NodeId>,
    buttons: u32,
    mods: Mods,
    last_click: Option<ClickState>,
    /// Text-control drag selection: (control, anchor byte offset).
    drag: Option<(NodeId, usize)>,
    /// A text control with edits not yet announced with a `change` event.
    edit_dirty: Option<NodeId>,
    space_down_on: Option<NodeId>,
    window_focused: bool,
}

pub struct Session {
    pub engine: Rc<RefCell<Engine>>,
    pub js: JsRuntime,
    input: InputState,
    clipboard: Box<dyn Clipboard>,
    last_cursor: Option<Cursor>,
    title: String,
    pub bundle_name: String,
}

fn normalize(p: &str) -> String {
    p.trim_start_matches("./").trim_start_matches('/').split(['?', '#']).next().unwrap_or("").to_string()
}

impl Session {
    /// Parse the bundle's document, install the runtime and run its scripts in document order.
    pub fn new(bundle: &AppBundle, clipboard: Box<dyn Clipboard>, viewport: (f32, f32, f32)) -> Result<Session, String> {
        let doc = rendrix_dom::Document::parse(&bundle.html);
        let mut engine = Engine::with_document(doc, bundle.asset_provider());
        engine.set_viewport(viewport.0, viewport.1, viewport.2);
        let engine = Rc::new(RefCell::new(engine));
        let mut js = JsRuntime::new(engine.clone(), &bundle.name, bundle.permissions()).map_err(|e| e.to_string())?;
        for (path, src) in &bundle.modules {
            js.add_module(path, src.clone());
        }
        let mut session = Session { engine, js, input: InputState { window_focused: true, ..Default::default() }, clipboard, last_cursor: None, title: bundle.window_title(), bundle_name: bundle.name.clone() };
        session.run_document_scripts(bundle)?;
        session.js.boot();
        Ok(session)
    }

    fn run_document_scripts(&mut self, bundle: &AppBundle) -> Result<(), String> {
        // Collect first: scripts may mutate the document while they run.
        let scripts: Vec<(Option<String>, String, bool)> = {
            let e = self.engine.borrow();
            let doc = &e.doc;
            doc.get_elements_by_tag_name(doc.root(), "script")
                .into_iter()
                .map(|n| (doc.get_attr(n, "src").map(str::to_string), doc.text_content(n), doc.get_attr(n, "type").is_some_and(|t| t.eq_ignore_ascii_case("module"))))
                .collect()
        };
        let mut inline = 0;
        for (src, text, is_module) in scripts {
            match (src, is_module) {
                (Some(src), true) => {
                    let key = normalize(&src);
                    let name = [key.clone(), format!("{key}.js")].into_iter().find(|k| bundle.modules.contains_key(k)).ok_or_else(|| format!("module script '{src}' is not in the bundle"))?;
                    self.js.run_module(&name).map_err(|e| format!("{src}: {e}"))?;
                }
                (None, true) => {
                    inline += 1;
                    let name = format!("inline-{inline}.js");
                    self.js.add_module(&name, text);
                    self.js.run_module(&name).map_err(|e| format!("{name}: {e}"))?;
                }
                (Some(src), false) => {
                    let key = normalize(&src);
                    let code = bundle.modules.get(&key).cloned().or_else(|| self.engine.borrow().read_asset(&key).map(|b| String::from_utf8_lossy(&b).into_owned())).ok_or_else(|| format!("script '{src}' is not in the bundle"))?;
                    self.js.run_script(&src, &code).map_err(|e| format!("{src}: {e}"))?;
                }
                (None, false) => {
                    if !text.trim().is_empty() {
                        self.js.run_script("inline-script", &text).map_err(|e| format!("inline script: {e}"))?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn now_s(&self) -> f64 {
        self.js.host.borrow().now_ms() / 1000.0
    }

    pub fn resize(&mut self, width: f32, height: f32, scale: f32) {
        self.engine.borrow_mut().set_viewport(width, height, scale);
        self.js.viewport_changed();
    }

    // ------------------------------------------------------------------------------------
    // Event plumbing
    // ------------------------------------------------------------------------------------

    fn base_init(&self, extra: Value) -> Value {
        let m = self.input.mods;
        let mut v = json!({
            "clientX": self.input.pos.0, "clientY": self.input.pos.1, "pageX": self.input.pos.0, "pageY": self.input.pos.1, "screenX": self.input.pos.0, "screenY": self.input.pos.1,
            "buttons": self.input.buttons, "ctrlKey": m.ctrl, "shiftKey": m.shift, "altKey": m.alt, "metaKey": m.meta,
        });
        if let (Value::Object(base), Value::Object(more)) = (&mut v, extra) {
            base.extend(more);
        }
        v
    }

    fn dispatch(&mut self, ty: &str, target: Option<NodeId>, extra: Value) -> bool {
        let init = self.base_init(extra);
        self.js.dispatch(ty, target, &init)
    }

    fn elements_on_path(&self, id: Option<NodeId>) -> Vec<NodeId> {
        let e = self.engine.borrow();
        match id {
            Some(n) => e.doc.event_path(n).into_iter().filter(|x| matches!(e.doc.node(*x).kind, NodeKind::Element(_))).collect(),
            None => vec![],
        }
    }

    fn set_hover(&mut self, new: Option<NodeId>) {
        let old = self.input.hover;
        if old == new {
            return;
        }
        let (old_path, new_path) = (self.elements_on_path(old), self.elements_on_path(new));
        let rel_new = new.map(|n| json!({ "relatedId": n })).unwrap_or(json!({}));
        let rel_old = old.map(|n| json!({ "relatedId": n })).unwrap_or(json!({}));
        if let Some(o) = old {
            self.dispatch("pointerout", Some(o), rel_new.clone());
            self.dispatch("mouseout", Some(o), rel_new.clone());
            for n in old_path.iter().filter(|n| !new_path.contains(n)) {
                self.dispatch("pointerleave", Some(*n), rel_new.clone());
                self.dispatch("mouseleave", Some(*n), rel_new.clone());
            }
        }
        self.engine.borrow_mut().doc.set_hover_chain(new);
        self.input.hover = new;
        if let Some(n) = new {
            self.dispatch("pointerover", Some(n), rel_old.clone());
            self.dispatch("mouseover", Some(n), rel_old.clone());
            for e in new_path.iter().rev().filter(|e| !old_path.contains(e)) {
                self.dispatch("pointerenter", Some(*e), rel_old.clone());
                self.dispatch("mouseenter", Some(*e), rel_old.clone());
            }
        }
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.input.pos = (x, y);
        let target = self.engine.borrow().hit_test(x, y);
        self.set_hover(target);
        if let Some(t) = target {
            self.dispatch("pointermove", Some(t), json!({ "pointerType": "mouse" }));
            self.dispatch("mousemove", Some(t), json!({}));
        }
        if let Some((c, anchor)) = self.input.drag {
            let off = self.engine.borrow().text_offset_at(c, x, y);
            if let Some(off) = off {
                self.engine.borrow_mut().doc.set_selection(c, anchor, off);
                self.engine.borrow_mut().reset_caret();
            }
        }
    }

    fn focus_target(&self, mut n: NodeId) -> Option<NodeId> {
        let e = self.engine.borrow();
        loop {
            if e.doc.is_focusable(n) {
                return Some(n);
            }
            match e.doc.parent(n) {
                Some(p) if e.doc.element(p).is_some() => n = p,
                _ => return None,
            }
        }
    }

    fn move_focus(&mut self, to: Option<NodeId>) {
        let current = self.engine.borrow().doc.focused();
        if current == to {
            return;
        }
        if let Some(dirty) = self.input.edit_dirty.take() {
            if Some(dirty) != to {
                self.js.fire_change(dirty);
            }
        }
        self.js.set_focus(to);
    }

    pub fn pointer_down(&mut self, x: f32, y: f32, button: u32) {
        self.input.pos = (x, y);
        let target = self.engine.borrow().hit_test(x, y);
        self.set_hover(target);
        let bit = match button {
            0 => 1,
            1 => 4,
            2 => 2,
            _ => 0,
        };
        self.input.buttons |= bit;
        let Some(t) = target else {
            self.move_focus(None);
            return;
        };
        if button == 0 {
            self.engine.borrow_mut().doc.set_active_chain(Some(t));
            self.input.pressed = Some(t);
        }
        let ok_p = self.dispatch("pointerdown", Some(t), json!({ "button": button, "pointerType": "mouse" }));
        let ok_m = self.dispatch("mousedown", Some(t), json!({ "button": button }));
        if !(ok_p && ok_m) || button != 0 {
            return;
        }
        // Click counting for double/triple click selection.
        let now = Instant::now();
        let count = match &self.input.last_click {
            Some(c) if now.duration_since(c.time).as_millis() < 450 && (c.pos.0 - x).abs() < 5.0 && (c.pos.1 - y).abs() < 5.0 => c.count + 1,
            _ => 1,
        };
        self.input.last_click = Some(ClickState { time: now, pos: (x, y), count });
        let f = self.focus_target(t);
        self.move_focus(f);
        if let Some(f) = f {
            let is_text = self.engine.borrow().doc.is_text_control(f);
            if is_text {
                let off = self.engine.borrow().text_offset_at(f, x, y).unwrap_or(0);
                let value = self.engine.borrow().doc.value(f);
                let (a, b) = match count {
                    1 => (off, off),
                    2 => word_bounds(&value, off),
                    _ => (0, value.len()),
                };
                self.engine.borrow_mut().doc.set_selection(f, a, b);
                self.engine.borrow_mut().reset_caret();
                self.input.drag = if count == 1 { Some((f, off)) } else { None };
            }
        }
    }

    pub fn pointer_up(&mut self, x: f32, y: f32, button: u32) {
        self.input.pos = (x, y);
        let target = self.engine.borrow().hit_test(x, y);
        let bit = match button {
            0 => 1,
            1 => 4,
            2 => 2,
            _ => 0,
        };
        self.input.buttons &= !bit;
        self.input.drag = None;
        if let Some(t) = target {
            self.dispatch("pointerup", Some(t), json!({ "button": button, "pointerType": "mouse" }));
            self.dispatch("mouseup", Some(t), json!({ "button": button }));
        }
        if button == 0 {
            self.engine.borrow_mut().doc.set_active_chain(None);
            let pressed = self.input.pressed.take();
            if let (Some(p), Some(t)) = (pressed, target) {
                // `click` goes to the nearest common ancestor of the press and release targets.
                let common = {
                    let e = self.engine.borrow();
                    let pa = e.doc.event_path(p);
                    e.doc.event_path(t).into_iter().find(|n| pa.contains(n) && e.doc.element(*n).is_some())
                };
                if let Some(c) = common {
                    let count = self.input.last_click.as_ref().map(|c| c.count).unwrap_or(1);
                    self.dispatch("click", Some(c), json!({ "button": 0, "detail": count, "pointerType": "mouse" }));
                    if count == 2 {
                        self.dispatch("dblclick", Some(c), json!({ "button": 0, "detail": 2 }));
                    }
                }
            }
        } else if button == 2 {
            if let Some(t) = target {
                self.dispatch("contextmenu", Some(t), json!({ "button": 2 }));
            }
        } else if button == 1 {
            if let Some(t) = target {
                self.dispatch("auxclick", Some(t), json!({ "button": 1 }));
            }
        }
    }

    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) {
        self.input.pos = (x, y);
        let target = self.engine.borrow().hit_test(x, y).or_else(|| self.engine.borrow().doc.body());
        let allowed = self.dispatch("wheel", target, json!({ "deltaX": dx, "deltaY": dy, "deltaMode": 0 }));
        if allowed {
            let scrolled = self.engine.borrow_mut().scroll_by(x, y, dx, dy);
            if scrolled {
                let t = self.engine.borrow().hit_test(x, y);
                if let Some(t) = t {
                    // Scroll changed what is under the pointer.
                    self.set_hover(Some(t));
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // Keyboard
    // ------------------------------------------------------------------------------------

    pub fn set_mods(&mut self, mods: Mods) {
        self.input.mods = mods;
    }

    fn key_init(&self, k: &KeyInput) -> Value {
        self.base_init(json!({ "key": k.key, "code": k.code, "repeat": k.repeat, "ctrlKey": k.mods.ctrl, "shiftKey": k.mods.shift, "altKey": k.mods.alt, "metaKey": k.mods.meta }))
    }

    fn key_target(&self) -> Option<NodeId> {
        let e = self.engine.borrow();
        e.doc.focused().or_else(|| e.doc.body())
    }

    pub fn key_down(&mut self, k: KeyInput) {
        self.input.mods = k.mods;
        let target = self.key_target();
        let init = self.key_init(&k);
        let allowed = self.js.dispatch("keydown", target, &init);
        if !allowed {
            return;
        }
        self.default_key_action(&k, target);
    }

    pub fn key_up(&mut self, k: KeyInput) {
        self.input.mods = k.mods;
        let target = self.key_target();
        let init = self.key_init(&k);
        let allowed = self.js.dispatch("keyup", target, &init);
        if k.key == " " && allowed {
            if let (Some(t), Some(down)) = (target, self.input.space_down_on.take()) {
                if t == down && self.is_click_on_space(t) {
                    self.dispatch("click", Some(t), json!({ "detail": 0 }));
                }
            }
        }
    }

    fn is_click_on_space(&self, n: NodeId) -> bool {
        let e = self.engine.borrow();
        match e.doc.tag(n) {
            Some("button") => true,
            Some("input") => matches!(e.doc.get_attr(n, "type").map(|t| t.to_ascii_lowercase()).as_deref(), Some("checkbox" | "radio" | "button" | "submit" | "reset")),
            _ => false,
        }
    }

    fn focusables(&self) -> Vec<NodeId> {
        let e = self.engine.borrow();
        let mut positive: Vec<(i32, NodeId)> = Vec::new();
        let mut zero: Vec<NodeId> = Vec::new();
        for n in e.doc.depth_first(e.doc.root()) {
            if !e.doc.is_focusable(n) || e.element_rect(n).is_none() {
                continue;
            }
            match e.doc.get_attr(n, "tabindex").and_then(|t| t.trim().parse::<i32>().ok()) {
                Some(t) if t > 0 => positive.push((t, n)),
                Some(t) if t < 0 => {}
                _ => zero.push(n),
            }
        }
        positive.sort_by_key(|(t, _)| *t);
        positive.into_iter().map(|(_, n)| n).chain(zero).collect()
    }

    fn default_key_action(&mut self, k: &KeyInput, target: Option<NodeId>) {
        let m = k.mods;
        if k.key == "Tab" && !m.ctrl && !m.alt {
            let list = self.focusables();
            if list.is_empty() {
                return;
            }
            let cur = self.engine.borrow().doc.focused();
            let pos = cur.and_then(|c| list.iter().position(|n| *n == c));
            let next = match (pos, m.shift) {
                (Some(i), false) => list[(i + 1) % list.len()],
                (Some(i), true) => list[(i + list.len() - 1) % list.len()],
                (None, false) => list[0],
                (None, true) => list[list.len() - 1],
            };
            self.move_focus(Some(next));
            return;
        }
        let Some(t) = target else { return };
        let (is_text, tag) = {
            let e = self.engine.borrow();
            (e.doc.is_text_control(t) && e.doc.element(t).is_some_and(|el| el.flags.focus), e.doc.tag(t).map(str::to_string))
        };
        if is_text {
            self.text_key(k, t, tag.as_deref() == Some("textarea"));
            return;
        }
        // Activation keys on buttons, checkboxes, radios and links.
        match (k.key.as_str(), tag.as_deref()) {
            ("Enter", Some("button" | "a")) => {
                self.dispatch("click", Some(t), json!({ "detail": 0 }));
            }
            ("Enter", Some("input")) => {
                let ty = self.engine.borrow().doc.get_attr(t, "type").map(|x| x.to_ascii_lowercase());
                if matches!(ty.as_deref(), Some("button" | "submit" | "reset" | "checkbox" | "radio")) {
                    self.dispatch("click", Some(t), json!({ "detail": 0 }));
                }
            }
            (" ", Some(_)) if self.is_click_on_space(t) => self.input.space_down_on = Some(t),
            _ => {}
        }
        // Keyboard scrolling of the document / focused scroller.
        let (vw, vh) = self.engine.borrow().viewport;
        let step = match k.key.as_str() {
            "ArrowDown" => Some(40.0),
            "ArrowUp" => Some(-40.0),
            "PageDown" => Some(vh * 0.9),
            "PageUp" => Some(-vh * 0.9),
            " " if !self.is_click_on_space(t) => Some(if m.shift { -vh * 0.9 } else { vh * 0.9 }),
            "Home" => Some(-1.0e9),
            "End" => Some(1.0e9),
            _ => None,
        };
        if let Some(dy) = step {
            self.engine.borrow_mut().scroll_by(vw / 2.0, vh / 2.0, 0.0, dy);
        }
    }

    fn text_key(&mut self, k: &KeyInput, id: NodeId, multiline: bool) {
        let m = k.mods;
        let cmd = m.ctrl || m.meta;
        let word = m.ctrl || m.alt;
        let mut value_change: Option<(&str, Option<String>)> = None;
        let op = match k.key.as_str() {
            "Backspace" => Some((EditOp::Backspace { word }, "deleteContentBackward")),
            "Delete" => Some((EditOp::Delete { word }, "deleteContentForward")),
            "ArrowLeft" => Some((EditOp::Left { shift: m.shift, word }, "")),
            "ArrowRight" => Some((EditOp::Right { shift: m.shift, word }, "")),
            "ArrowUp" => Some((EditOp::Up { shift: m.shift }, "")),
            "ArrowDown" => Some((EditOp::Down { shift: m.shift }, "")),
            "Home" => Some((EditOp::Home { shift: m.shift }, "")),
            "End" => Some((EditOp::End { shift: m.shift }, "")),
            "a" | "A" if cmd => Some((EditOp::SelectAll, "")),
            "Enter" if multiline => Some((EditOp::Insert("\n".into()), "insertLineBreak")),
            _ => None,
        };
        if let Some((op, input_type)) = op {
            let inserted = if let EditOp::Insert(s) = &op { Some(s.clone()) } else { None };
            let r = self.engine.borrow_mut().edit(id, op);
            if r.value_changed {
                value_change = Some((input_type, inserted));
            }
        } else if k.key == "Enter" {
            // Single-line field: commit, then implicit form submission.
            if let Some(d) = self.input.edit_dirty.take() {
                self.js.fire_change(d);
            }
            self.js.submit_form(id);
        } else if cmd && matches!(k.key.as_str(), "c" | "C" | "x" | "X") {
            let (text, cut) = {
                let e = self.engine.borrow();
                let el = e.doc.element(id).unwrap();
                let v = e.doc.value(id);
                let (a, b) = (el.form.selection_start.min(el.form.selection_end), el.form.selection_start.max(el.form.selection_end));
                (v.get(a..b).unwrap_or("").to_string(), matches!(k.key.as_str(), "x" | "X"))
            };
            if !text.is_empty() {
                self.clipboard.set(&text);
                self.js.host.borrow_mut().clipboard = text;
                if cut {
                    let r = self.engine.borrow_mut().edit(id, EditOp::Backspace { word: false });
                    if r.value_changed {
                        value_change = Some(("deleteByCut", None));
                    }
                }
            }
        } else if cmd && matches!(k.key.as_str(), "v" | "V") {
            let text = self.clipboard.get();
            if !text.is_empty() {
                let r = self.engine.borrow_mut().edit(id, EditOp::Insert(text.clone()));
                if r.value_changed {
                    value_change = Some(("insertFromPaste", Some(text)));
                }
            }
        } else if let Some(text) = k.text.as_ref().filter(|t| !cmd && !t.chars().any(|c| c.is_control())) {
            let r = self.engine.borrow_mut().edit(id, EditOp::Insert(text.clone()));
            if r.value_changed {
                value_change = Some(("insertText", Some(text.clone())));
            }
        }
        if let Some((ty, data)) = value_change {
            self.input.edit_dirty = Some(id);
            self.js.fire_input(id, ty, data.as_deref());
        }
    }

    /// Committed text from an input method.
    pub fn text_commit(&mut self, text: &str) {
        let Some(id) = self.engine.borrow().doc.focused().filter(|n| self.engine.borrow().doc.is_text_control(*n)) else { return };
        let r = self.engine.borrow_mut().edit(id, EditOp::Insert(text.to_string()));
        if r.value_changed {
            self.input.edit_dirty = Some(id);
            self.js.fire_input(id, "insertCompositionText", Some(text));
        }
    }

    /// Caret rectangle of the focused text control in viewport coordinates (for IME placement).
    pub fn caret_rect(&self) -> Option<[f32; 4]> {
        let e = self.engine.borrow();
        let id = e.doc.focused().filter(|n| e.doc.is_text_control(*n))?;
        let content = e.content_of(id)?;
        let (ox, oy, w) = e.text_origin(id)?;
        let single = e.doc.tag(id) == Some("input");
        let sel = e.doc.element(id)?.form.selection_end;
        let (cx, cy, ch) = e.caret_geometry(&content, if single { None } else { Some(w) }, sel);
        Some([ox + cx, oy + cy, 1.0, ch])
    }

    pub fn set_window_focused(&mut self, focused: bool) {
        self.input.window_focused = focused;
        if !focused {
            self.input.buttons = 0;
            self.input.drag = None;
            self.engine.borrow_mut().doc.set_active_chain(None);
        }
        self.engine.borrow_mut().reset_caret();
    }

    // ------------------------------------------------------------------------------------
    // Frames
    // ------------------------------------------------------------------------------------

    /// Run timers/fetches without producing a frame. Returns when the next timer is due (runtime seconds).
    pub fn pump(&mut self) -> Option<f64> {
        self.js.tick().next_timer_ms.map(|ms| ms / 1000.0)
    }

    /// Produce the next frame: timers, `requestAnimationFrame`, style, layout.
    pub fn frame(&mut self) -> FrameInfo {
        let tick = self.js.tick();
        let now = self.now_s();
        if tick.wants_frame {
            self.js.run_animation_frame(now * 1000.0);
        }
        let mut info = self.engine.borrow_mut().update(now);
        self.js.after_layout();
        if self.engine.borrow().has_pending_changes() {
            let mut e = self.engine.borrow_mut();
            let second = e.update(now);
            info.repainted |= second.repainted;
            info.animating |= second.animating;
            info.next_wake = second.next_wake.or(info.next_wake);
        }
        let mut out = FrameInfo { repainted: info.repainted, ..Default::default() };
        let tick = self.js.tick_info();
        out.continuous = info.animating || tick.wants_frame || self.engine.borrow().needs_update();
        let timer = tick.next_timer_ms.map(|ms| ms / 1000.0);
        out.next_wake_s = match (timer, info.next_wake) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if tick.pending_io {
            out.next_wake_s = Some(out.next_wake_s.map_or(now + 0.016, |w| w.min(now + 0.016)));
        }
        // Cursor from the element under the pointer.
        let c = self.engine.borrow().cursor_at(self.input.pos.0, self.input.pos.1);
        if self.last_cursor != Some(c) {
            self.last_cursor = Some(c);
            out.cursor = Some(c);
        }
        for cmd in self.js.take_window_commands() {
            match cmd {
                WindowCommand::SetTitle(t) => {
                    self.title = t.clone();
                    out.title = Some(t);
                }
                WindowCommand::Resize(w, h) => out.resize = Some((w, h)),
                WindowCommand::Close | WindowCommand::Quit => out.quit = true,
            }
        }
        out
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn display_list(&self) -> std::cell::Ref<'_, Engine> {
        self.engine.borrow()
    }

    /// First element matching a selector (for scripted drivers).
    pub fn query(&self, selector: &str) -> Option<NodeId> {
        let e = self.engine.borrow();
        e.doc.query_selector(e.doc.root(), selector).ok().flatten()
    }

    /// Centre of an element in viewport coordinates.
    pub fn center_of(&self, id: NodeId) -> Option<(f32, f32)> {
        self.engine.borrow().element_rect(id).map(|r| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0))
    }
}

fn word_bounds(s: &str, at: usize) -> (usize, usize) {
    let at = at.min(s.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = at;
    while start > 0 {
        let prev = s[..start].chars().next_back().unwrap();
        if !is_word(prev) {
            break;
        }
        start -= prev.len_utf8();
    }
    let mut end = at;
    while let Some(c) = s[end..].chars().next() {
        if !is_word(c) {
            break;
        }
        end += c.len_utf8();
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_selection_bounds() {
        assert_eq!(word_bounds("hello big world", 7), (6, 9));
        assert_eq!(word_bounds("hello", 0), (0, 5));
        assert_eq!(word_bounds("a  b", 2), (2, 2));
    }
}

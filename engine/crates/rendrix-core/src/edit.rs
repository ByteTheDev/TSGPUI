//! Text editing for `<input>` and `<textarea>`: caret movement, selection and insertion on UTF-8 values.

use crate::Engine;
use rendrix_dom::NodeId;

#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    Insert(String),
    Backspace { word: bool },
    Delete { word: bool },
    Left { shift: bool, word: bool },
    Right { shift: bool, word: bool },
    Up { shift: bool },
    Down { shift: bool },
    Home { shift: bool },
    End { shift: bool },
    SelectAll,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct EditResult {
    pub value_changed: bool,
    pub selection_changed: bool,
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].char_indices().next_back().map(|(p, _)| p).unwrap_or(0)
}

fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(s.len())
}

fn word_left(s: &str, mut i: usize) -> usize {
    while i > 0 && s[..i].chars().next_back().is_some_and(|c| c.is_whitespace()) {
        i = prev_boundary(s, i);
    }
    while i > 0 && s[..i].chars().next_back().is_some_and(|c| !c.is_whitespace()) {
        i = prev_boundary(s, i);
    }
    i
}

fn word_right(s: &str, mut i: usize) -> usize {
    while i < s.len() && s[i..].chars().next().is_some_and(|c| !c.is_whitespace()) {
        i = next_boundary(s, i);
    }
    while i < s.len() && s[i..].chars().next().is_some_and(|c| c.is_whitespace()) {
        i = next_boundary(s, i);
    }
    i
}

fn line_start(s: &str, i: usize) -> usize {
    s[..i].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

fn line_end(s: &str, i: usize) -> usize {
    s[i..].find('\n').map(|p| i + p).unwrap_or(s.len())
}

impl Engine {
    /// Apply an editing operation to a text control. The caller dispatches `input`/`change` events
    /// when `value_changed` is set.
    pub fn edit(&mut self, id: NodeId, op: EditOp) -> EditResult {
        let Some(el) = self.doc.element(id) else { return EditResult::default() };
        let multiline = el.tag == "textarea";
        let readonly = self.doc.has_attr(id, "readonly") || self.doc.has_attr(id, "disabled");
        let max_len = self.doc.get_attr(id, "maxlength").and_then(|v| v.parse::<usize>().ok());
        let value = self.doc.value(id);
        let (mut a, mut b) = {
            let f = &self.doc.element(id).unwrap().form;
            let len = value.len();
            (f.selection_start.min(len), f.selection_end.min(len))
        };
        // `a` is the anchor, `b` the moving end (caret). Normalise to char boundaries.
        while !value.is_char_boundary(a) {
            a -= 1;
        }
        while !value.is_char_boundary(b) {
            b -= 1;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        let mut new_value: Option<String> = None;
        let (mut na, mut nb) = (a, b);

        let move_caret = |target: usize, shift: bool, a: usize| -> (usize, usize) { if shift { (a, target) } else { (target, target) } };

        match op {
            EditOp::Insert(text) if !readonly => {
                let mut text = if multiline { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.replace(['\r', '\n'], " ") };
                if let Some(m) = max_len {
                    let room = m.saturating_sub(value[..lo].chars().count() + value[hi..].chars().count());
                    text = text.chars().take(room).collect();
                }
                if !text.is_empty() || lo != hi {
                    let mut v = String::with_capacity(value.len() + text.len());
                    v.push_str(&value[..lo]);
                    v.push_str(&text);
                    v.push_str(&value[hi..]);
                    na = lo + text.len();
                    nb = na;
                    new_value = Some(v);
                }
            }
            EditOp::Backspace { word } if !readonly => {
                let (s, e) = if lo != hi {
                    (lo, hi)
                } else if lo > 0 {
                    (if word { word_left(&value, lo) } else { prev_boundary(&value, lo) }, lo)
                } else {
                    (0, 0)
                };
                if s != e {
                    new_value = Some(format!("{}{}", &value[..s], &value[e..]));
                    na = s;
                    nb = s;
                }
            }
            EditOp::Delete { word } if !readonly => {
                let (s, e) = if lo != hi {
                    (lo, hi)
                } else if hi < value.len() {
                    (lo, if word { word_right(&value, hi) } else { next_boundary(&value, hi) })
                } else {
                    (0, 0)
                };
                if s != e {
                    new_value = Some(format!("{}{}", &value[..s], &value[e..]));
                    na = s;
                    nb = s;
                }
            }
            EditOp::Left { shift, word } => {
                let t = if !shift && lo != hi { lo } else if word { word_left(&value, b) } else { prev_boundary(&value, b) };
                (na, nb) = move_caret(t, shift, a);
            }
            EditOp::Right { shift, word } => {
                let t = if !shift && lo != hi { hi } else if word { word_right(&value, b) } else { next_boundary(&value, b) };
                (na, nb) = move_caret(t, shift, a);
            }
            EditOp::Home { shift } => {
                let t = if multiline { line_start(&value, b) } else { 0 };
                (na, nb) = move_caret(t, shift, a);
            }
            EditOp::End { shift } => {
                let t = if multiline { line_end(&value, b) } else { value.len() };
                (na, nb) = move_caret(t, shift, a);
            }
            EditOp::Up { shift } | EditOp::Down { shift } => {
                let up = matches!(op, EditOp::Up { .. });
                let t = if multiline { self.vertical_target(id, b, up) } else if up { 0 } else { value.len() };
                (na, nb) = move_caret(t, shift, a);
            }
            EditOp::SelectAll => {
                na = 0;
                nb = value.len();
            }
            _ => {}
        }

        let value_changed = new_value.is_some();
        if let Some(v) = new_value {
            self.doc.set_form_value(id, &v);
        }
        let selection_changed = (na, nb) != (a, b);
        // set_selection stores start/end; keep the anchor as start and caret as end.
        self.doc.set_selection(id, na, nb);
        if value_changed || selection_changed {
            self.reset_caret();
        }
        EditResult { value_changed, selection_changed }
    }

    /// Byte offset one visual line above/below `byte` in a textarea, keeping the x position.
    fn vertical_target(&self, id: NodeId, byte: usize, up: bool) -> usize {
        let Some(content) = self.content_of(id) else { return byte };
        let Some((_, _, w)) = self.text_origin(id) else { return byte };
        let mut ts = self.text.borrow_mut();
        let (x, y, h) = ts.caret(&content, Some(w), byte);
        let ty = if up { y - h * 0.5 } else { y + h * 1.5 };
        if ty < 0.0 {
            return 0;
        }
        let value_len = self.doc.value(id).len();
        ts.hit(&content, Some(w), x, ty).min(value_len)
    }
}

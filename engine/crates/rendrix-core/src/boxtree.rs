//! Box generation: DOM + computed styles → the layout-engine-neutral box tree.
//!
//! This is where CSS visual formatting model rules live: block vs inline vs flex/grid containers,
//! anonymous boxes, inline formatting contexts (runs of text sharing one paragraph), whitespace
//! collapsing, replaced elements, form controls, generated content and containing blocks for
//! absolutely positioned boxes.

use crate::Engine;
use rendrix_css::{Align, ComputedStyle, Dim, Display, TextAlign, WhiteSpace};
use rendrix_dom::{NodeId, NodeKind};
use rendrix_layout::{InlineContent, InlineRun, LayoutKind, LayoutNode};
use std::sync::Arc;

pub const ANON: u64 = 1 << 40;

pub(crate) fn anon_id(parent: u64, idx: u64) -> u64 {
    ANON | (parent << 8) | (idx & 0xff)
}

enum Item {
    Run(InlineRun),
    Atomic(LayoutNode),
    Block(LayoutNode),
}

pub(crate) struct Builder<'a> {
    pub engine: &'a Engine,
    /// Collection points for absolutely positioned descendants, innermost positioned ancestor last.
    abs_stack: Vec<Vec<LayoutNode>>,
}

fn is_text_input(engine: &Engine, id: NodeId) -> bool {
    engine.doc.is_text_control(id)
}

impl<'a> Builder<'a> {
    pub fn new(engine: &'a Engine) -> Self {
        Builder { engine, abs_stack: vec![Vec::new()] }
    }

    pub fn build_root(mut self) -> Option<LayoutNode> {
        let html = self.engine.doc.document_element()?;
        let mut node = self.build_box(html)?;
        // Fixed / viewport-level absolutes hang off the root.
        let rest = self.abs_stack.pop().unwrap_or_default();
        node.children.extend(rest);
        Some(node)
    }

    fn style(&self, id: NodeId) -> Option<Arc<ComputedStyle>> {
        self.engine.style_of(id)
    }

    fn anon_block_style(&self, parent: &ComputedStyle) -> Arc<ComputedStyle> {
        let mut s = ComputedStyle::inherit_from(parent);
        s.display = Display::Block;
        Arc::new(s)
    }

    fn is_replaced(tag: &str) -> bool {
        matches!(tag, "img" | "canvas" | "video" | "svg" | "iframe" | "object" | "embed")
    }

    fn is_box_like(s: &ComputedStyle) -> bool {
        !s.background_color.is_transparent()
            || !s.background_images.is_empty()
            || s.border_width.iter().any(|w| *w > 0.0)
            || s.padding.iter().any(|p| *p != Dim::Px(0.0))
            || s.margin.iter().any(|m| *m != Dim::Px(0.0))
            || s.width != Dim::Auto
            || s.height != Dim::Auto
            || !s.box_shadow.is_empty()
            || s.has_transform()
            || s.opacity < 1.0
            || s.position != rendrix_css::Position::Static
    }

    fn has_block_child(&self, id: NodeId) -> bool {
        self.engine.doc.child_elements(id).any(|c| match self.style(c) {
            Some(s) => match s.display {
                Display::Block | Display::Flex | Display::Grid => true,
                Display::Inline => self.has_block_child(c),
                _ => false,
            },
            None => false,
        })
    }

    /// Build the box for element `id`, or `None` if it generates none (`display: none`).
    pub fn build_box(&mut self, id: NodeId) -> Option<LayoutNode> {
        let engine = self.engine;
        let mut style = self.style(id)?;
        if style.display == Display::None {
            return None;
        }
        let tag = engine.doc.tag(id)?.to_string();
        let lid = id as u64;

        // --- replaced elements ---
        if Self::is_replaced(&tag) {
            let attr_px = |name: &str| engine.doc.get_attr(id, name).and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok());
            let (mut w, mut h) = match tag.as_str() {
                "img" => {
                    let natural = engine.doc.get_attr(id, "src").and_then(|src| engine.image_entry(src));
                    natural.map(|e| (e.width as f32, e.height as f32)).unwrap_or((0.0, 0.0))
                }
                "canvas" | "svg" | "iframe" | "video" | "object" | "embed" => (300.0, 150.0),
                _ => (0.0, 0.0),
            };
            if let Some(a) = attr_px("width") {
                w = a;
            }
            if let Some(a) = attr_px("height") {
                h = a;
            }
            // Explicit CSS sizes win; Replaced measurement preserves aspect ratio for the missing side.
            let mut s = (*style).clone();
            if s.width == Dim::Auto && s.height == Dim::Auto && (attr_px("width").is_some() || attr_px("height").is_some()) {
                if attr_px("width").is_some() {
                    s.width = Dim::Px(w);
                }
                if attr_px("height").is_some() {
                    s.height = Dim::Px(h);
                }
            }
            return Some(LayoutNode { id: lid, style: Arc::new(s), kind: LayoutKind::Replaced { width: w, height: h }, children: vec![] });
        }

        let positioned = style.position != rendrix_css::Position::Static || style.has_transform();
        let is_host = positioned;
        if is_host {
            self.abs_stack.push(Vec::new());
        }

        let (mut kind, mut children) = if let Some(content) = self.control_content(id, &tag, &style) {
            (LayoutKind::Text(Arc::new(content)), vec![])
        } else if matches!(tag.as_str(), "input") && !is_text_input(engine, id) {
            (LayoutKind::Box, vec![]) // checkbox, radio, hidden …
        } else {
            let flex = matches!(style.display, Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid);
            let mut items = Vec::new();
            self.collect(id, &style, flex, &mut items);
            self.arrange(id, &style, flex, items)
        };

        if is_host {
            children.extend(self.abs_stack.pop().unwrap_or_default());
        }
        // A folded text leaf cannot also own children (absolutely positioned descendants): split it.
        if matches!(kind, LayoutKind::Text(_)) && !children.is_empty() {
            if let LayoutKind::Text(content) = std::mem::replace(&mut kind, LayoutKind::Box) {
                let leaf_style = self.anon_block_style(&style);
                let content = Arc::new(InlineContent { runs: content.runs.clone(), container: style.clone() });
                children.insert(0, LayoutNode { id: anon_id(lid, 0), style: leaf_style, kind: LayoutKind::Text(content), children: vec![] });
            }
        }
        // Text controls and buttons keep their own style on the leaf.
        let _ = &mut style;
        Some(LayoutNode { id: lid, style, kind, children })
    }

    /// Content of form controls that render their own text (inputs, textareas, selects).
    fn control_content(&self, id: NodeId, tag: &str, style: &Arc<ComputedStyle>) -> Option<InlineContent> {
        let doc = &self.engine.doc;
        match tag {
            "input" if is_text_input(self.engine, id) => {
                let ty = doc.get_attr(id, "type").unwrap_or("text").to_ascii_lowercase();
                let value = doc.value(id);
                let mut container = (**style).clone();
                container.white_space = WhiteSpace::Pre;
                container.text_align = if matches!(style.text_align, TextAlign::Start) { TextAlign::Left } else { style.text_align };
                let container = Arc::new(container);
                let (text, run_style) = if value.is_empty() {
                    let mut ph = (*container).clone();
                    ph.color = rendrix_css::Color::rgba(0.46, 0.46, 0.46, 1.0);
                    (doc.get_attr(id, "placeholder").unwrap_or("").to_string(), Arc::new(ph))
                } else if ty == "password" {
                    ("•".repeat(value.chars().count()), container.clone())
                } else {
                    (value, container.clone())
                };
                Some(InlineContent { runs: vec![InlineRun { text, style: run_style, node: id as u64, is_break: false }], container })
            }
            "input" if matches!(doc.get_attr(id, "type").map(|t| t.to_ascii_lowercase()).as_deref(), Some("button" | "submit" | "reset")) => {
                let label = doc.get_attr(id, "value").unwrap_or(match doc.get_attr(id, "type") {
                    Some("reset") => "Reset",
                    Some("button") => "",
                    _ => "Submit",
                });
                let mut c = (**style).clone();
                c.white_space = WhiteSpace::Pre;
                let c = Arc::new(c);
                Some(InlineContent { runs: vec![InlineRun { text: label.to_string(), style: c.clone(), node: id as u64, is_break: false }], container: c })
            }
            "textarea" => {
                let value = doc.value(id);
                let (text, rs) = if value.is_empty() {
                    let mut ph = (**style).clone();
                    ph.color = rendrix_css::Color::rgba(0.46, 0.46, 0.46, 1.0);
                    (doc.get_attr(id, "placeholder").unwrap_or("").to_string(), Arc::new(ph))
                } else {
                    (value, style.clone())
                };
                Some(InlineContent { runs: vec![InlineRun { text, style: rs, node: id as u64, is_break: false }], container: style.clone() })
            }
            "select" => {
                let opts = doc.get_elements_by_tag_name(id, "option");
                let chosen = opts.iter().find(|o| doc.has_attr(**o, "selected")).or(opts.first());
                let label = chosen.map(|o| doc.text_content(*o).trim().to_string()).unwrap_or_default();
                let mut c = (**style).clone();
                c.white_space = WhiteSpace::Pre;
                let c = Arc::new(c);
                Some(InlineContent { runs: vec![InlineRun { text: format!("{label}  \u{25be}"), style: c.clone(), node: id as u64, is_break: false }], container: c })
            }
            _ => None,
        }
    }

    fn push_generated(&mut self, parent: NodeId, before: bool, flex: bool, items: &mut Vec<Item>) {
        let Some(ps) = self.engine.pseudo_style_of(parent, before) else { return };
        let Some(text) = ps.content.clone() else { return };
        if ps.display == Display::None {
            return;
        }
        let run = InlineRun { text, style: ps.clone(), node: parent as u64, is_break: false };
        if !flex && matches!(ps.display, Display::Inline) {
            items.push(Item::Run(run));
        } else {
            let content = Arc::new(InlineContent { runs: vec![run], container: ps.clone() });
            let id = anon_id(parent as u64, if before { 250 } else { 251 });
            items.push(Item::Block(LayoutNode { id, style: ps, kind: LayoutKind::Text(content), children: vec![] }));
        }
    }

    fn collect(&mut self, parent: NodeId, parent_style: &Arc<ComputedStyle>, flex: bool, items: &mut Vec<Item>) {
        let engine = self.engine;
        let doc = &engine.doc;
        self.push_generated(parent, true, flex, items);
        // List marker.
        if doc.tag(parent) == Some("li") && !parent_style.list_style_none {
            if let Some(list) = doc.parent(parent) {
                let marker = match doc.tag(list) {
                    Some("ol") => {
                        let start: i32 = doc.get_attr(list, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
                        let idx = doc.child_elements(list).take_while(|c| *c != parent).filter(|c| doc.tag(*c) == Some("li")).count() as i32;
                        Some(format!("{}. ", start + idx))
                    }
                    Some("ul") | Some("menu") => Some("\u{2022} ".to_string()),
                    _ => None,
                };
                if let Some(m) = marker {
                    items.push(Item::Run(InlineRun { text: m, style: parent_style.clone(), node: parent as u64, is_break: false }));
                }
            }
        }
        let kids: Vec<NodeId> = doc.children(parent).collect();
        for c in kids {
            match &doc.node(c).kind {
                NodeKind::Text(t) => {
                    items.push(Item::Run(InlineRun { text: t.clone(), style: parent_style.clone(), node: parent as u64, is_break: false }));
                }
                NodeKind::Element(e) => {
                    let Some(cs) = self.style(c) else { continue };
                    match cs.display {
                        Display::None => continue,
                        Display::Contents => {
                            self.collect(c, &cs, flex, items);
                            continue;
                        }
                        _ => {}
                    }
                    if cs.is_out_of_flow() {
                        if let Some(n) = self.build_box(c) {
                            let idx = if cs.position == rendrix_css::Position::Fixed { 0 } else { self.abs_stack.len() - 1 };
                            self.abs_stack[idx].push(n);
                        }
                        continue;
                    }
                    if e.tag == "br" {
                        items.push(Item::Run(InlineRun { text: String::new(), style: parent_style.clone(), node: parent as u64, is_break: true }));
                        continue;
                    }
                    if flex {
                        if let Some(n) = self.build_box(c) {
                            items.push(Item::Block(n));
                        }
                        continue;
                    }
                    let tag = e.tag.as_str();
                    let inline_flow = cs.display == Display::Inline && !Self::is_replaced(tag) && !Self::is_box_like(&cs) && !self.has_block_child(c) && !matches!(tag, "input" | "button" | "select" | "textarea");
                    if inline_flow {
                        self.collect(c, &cs, false, items);
                    } else if matches!(cs.display, Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid) {
                        if let Some(n) = self.build_box(c) {
                            items.push(Item::Atomic(n));
                        }
                    } else if let Some(n) = self.build_box(c) {
                        items.push(Item::Block(n));
                    }
                }
                _ => {}
            }
        }
        self.push_generated(parent, false, flex, items);
    }

    fn text_leaf(&self, parent: u64, idx: u64, style: &Arc<ComputedStyle>, runs: Vec<InlineRun>) -> LayoutNode {
        let leaf_style = self.anon_block_style(style);
        let content = Arc::new(InlineContent { runs, container: leaf_style.clone() });
        LayoutNode { id: anon_id(parent, idx), style: leaf_style, kind: LayoutKind::Text(content), children: vec![] }
    }

    fn arrange(&mut self, id: NodeId, style: &Arc<ComputedStyle>, flex: bool, items: Vec<Item>) -> (LayoutKind, Vec<LayoutNode>) {
        let lid = id as u64;
        let mut children: Vec<LayoutNode> = Vec::new();
        let mut anon_idx = 1u64;

        if flex {
            let mut buf: Vec<InlineRun> = Vec::new();
            let flush = |buf: &mut Vec<InlineRun>, children: &mut Vec<LayoutNode>, anon_idx: &mut u64, this: &Self| {
                let runs = collapse(std::mem::take(buf));
                if runs.iter().any(|r| r.is_break || !r.text.trim().is_empty()) {
                    children.push(this.text_leaf(lid, *anon_idx, style, runs));
                    *anon_idx += 1;
                }
            };
            for it in items {
                match it {
                    Item::Run(r) => buf.push(r),
                    Item::Atomic(n) | Item::Block(n) => {
                        flush(&mut buf, &mut children, &mut anon_idx, self);
                        children.push(n);
                    }
                }
            }
            flush(&mut buf, &mut children, &mut anon_idx, self);
            return (LayoutKind::Box, children);
        }

        let has_block = items.iter().any(|i| matches!(i, Item::Block(_)));
        let has_atomic = items.iter().any(|i| matches!(i, Item::Atomic(_)));
        if !has_block && !has_atomic {
            let runs: Vec<InlineRun> = items.into_iter().filter_map(|i| if let Item::Run(r) = i { Some(r) } else { None }).collect();
            let runs = collapse(runs);
            if runs.is_empty() || runs.iter().all(|r| !r.is_break && r.text.trim().is_empty() && r.style.white_space != WhiteSpace::Pre && r.style.white_space != WhiteSpace::PreWrap) {
                return (LayoutKind::Box, vec![]);
            }
            return (LayoutKind::Text(Arc::new(InlineContent { runs, container: style.clone() })), vec![]);
        }

        // Mixed content: group consecutive inline-level items into anonymous boxes.
        let mut group: Vec<Item> = Vec::new();
        let flush_group = |group: &mut Vec<Item>, children: &mut Vec<LayoutNode>, anon_idx: &mut u64, this: &Self| {
            if group.is_empty() {
                return;
            }
            let g = std::mem::take(group);
            let atomic = g.iter().any(|i| matches!(i, Item::Atomic(_)));
            if !atomic {
                let runs: Vec<InlineRun> = g.into_iter().filter_map(|i| if let Item::Run(r) = i { Some(r) } else { None }).collect();
                let runs = collapse(runs);
                if runs.iter().any(|r| r.is_break || !r.text.trim().is_empty()) {
                    children.push(this.text_leaf(lid, *anon_idx, style, runs));
                    *anon_idx += 1;
                }
                return;
            }
            // Inline-level atomic boxes (buttons, inputs, images, inline-blocks) flow like a wrapping row.
            let mut row_style = ComputedStyle::inherit_from(style);
            row_style.display = Display::Flex;
            row_style.flex_wrap = rendrix_css::FlexWrap::Wrap;
            row_style.align_items = Align::Center;
            row_style.justify_content = match style.text_align {
                TextAlign::Center => Align::Center,
                TextAlign::Right | TextAlign::End => Align::FlexEnd,
                _ => Align::FlexStart,
            };
            row_style.column_gap = Dim::Px(style.font_size * 0.28);
            let row_id = anon_id(lid, *anon_idx);
            *anon_idx += 1;
            let row_style = Arc::new(row_style);
            let mut kids: Vec<LayoutNode> = Vec::new();
            let mut run_buf: Vec<InlineRun> = Vec::new();
            let mut sub = 0u64;
            let mut flush_runs = |buf: &mut Vec<InlineRun>, kids: &mut Vec<LayoutNode>| {
                let runs = collapse(std::mem::take(buf));
                if runs.iter().any(|r| r.is_break || !r.text.trim().is_empty()) {
                    sub += 1;
                    let ls = this.anon_block_style(style);
                    let content = Arc::new(InlineContent { runs, container: ls.clone() });
                    kids.push(LayoutNode { id: anon_id(row_id, sub), style: ls, kind: LayoutKind::Text(content), children: vec![] });
                }
            };
            for it in g {
                match it {
                    Item::Run(r) => run_buf.push(r),
                    Item::Atomic(n) | Item::Block(n) => {
                        flush_runs(&mut run_buf, &mut kids);
                        kids.push(n);
                    }
                }
            }
            flush_runs(&mut run_buf, &mut kids);
            children.push(LayoutNode { id: row_id, style: row_style, kind: LayoutKind::Box, children: kids });
        };
        for it in items {
            match it {
                Item::Block(n) => {
                    flush_group(&mut group, &mut children, &mut anon_idx, self);
                    children.push(n);
                }
                other => group.push(other),
            }
        }
        flush_group(&mut group, &mut children, &mut anon_idx, self);
        (LayoutKind::Box, children)
    }
}

/// CSS white-space processing across a sequence of runs in one inline formatting context.
pub fn collapse(runs: Vec<InlineRun>) -> Vec<InlineRun> {
    let mut out: Vec<InlineRun> = Vec::with_capacity(runs.len());
    let mut prev_space = true; // leading whitespace at line start is removed
    for mut r in runs {
        if r.is_break {
            prev_space = true;
            // Trailing collapsible space before a forced break is removed.
            if let Some(last) = out.last_mut() {
                if !last.is_break && collapsible(last.style.white_space) && last.text.ends_with(' ') {
                    last.text.pop();
                }
            }
            out.push(r);
            continue;
        }
        match r.style.white_space {
            WhiteSpace::Pre | WhiteSpace::PreWrap => {
                prev_space = r.text.ends_with(['\n', ' ']);
                if !r.text.is_empty() {
                    out.push(r);
                }
            }
            ws => {
                let keep_nl = ws == WhiteSpace::PreLine;
                let mut s = String::with_capacity(r.text.len());
                for ch in r.text.chars() {
                    let is_ws = matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{c}');
                    if keep_nl && ch == '\n' {
                        s.push('\n');
                        prev_space = true;
                    } else if is_ws {
                        if !prev_space {
                            s.push(' ');
                            prev_space = true;
                        }
                    } else {
                        s.push(ch);
                        prev_space = false;
                    }
                }
                if !s.is_empty() {
                    r.text = s;
                    out.push(r);
                }
            }
        }
    }
    while let Some(last) = out.last_mut() {
        if !last.is_break && collapsible(last.style.white_space) && last.text.ends_with(' ') {
            last.text.pop();
            if last.text.is_empty() {
                out.pop();
                continue;
            }
        }
        break;
    }
    out
}

fn collapsible(ws: WhiteSpace) -> bool {
    matches!(ws, WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &str) -> InlineRun {
        InlineRun { text: t.into(), style: Arc::new(ComputedStyle::default()), node: 1, is_break: false }
    }

    fn text(runs: &[InlineRun]) -> String {
        runs.iter().map(|r| if r.is_break { "|".to_string() } else { r.text.clone() }).collect()
    }

    #[test]
    fn collapses_whitespace_across_runs() {
        let out = collapse(vec![run("  hello \n  "), run(" world  "), run(" !")]);
        assert_eq!(text(&out), "hello world !");
    }

    #[test]
    fn trims_edges_and_breaks() {
        let br = InlineRun { text: String::new(), style: Arc::new(ComputedStyle::default()), node: 1, is_break: true };
        let out = collapse(vec![run("a  "), br, run("   b  ")]);
        assert_eq!(text(&out), "a|b");
        assert!(collapse(vec![run("   \n  ")]).is_empty());
    }

    #[test]
    fn preformatted_text_is_kept() {
        let mut s = ComputedStyle::default();
        s.white_space = WhiteSpace::Pre;
        let r = InlineRun { text: "  a\n  b ".into(), style: Arc::new(s), node: 1, is_break: false };
        assert_eq!(text(&collapse(vec![r])), "  a\n  b ");
    }

    #[test]
    fn nbsp_is_not_collapsed() {
        assert_eq!(text(&collapse(vec![run("a\u{a0}\u{a0}b")])), "a\u{a0}\u{a0}b");
    }
}

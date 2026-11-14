//! Rendrix DOM: an arena-backed document tree with attributes, mutation tracking, selector queries,
//! form-control state, an HTML parser/serializer and dirty tracking for the style/layout pipeline.
//!
//! Event *propagation* order is computed here ([`Document::event_path`]); listener storage and
//! invocation live in the JS binding layer, so the DOM stays engine-agnostic.

mod html;
mod query;

pub use html::{decode_entities, escape_attr, escape_text};
pub use query::ElRef;

use rendrix_css::{parse_declarations, Declaration};
use std::collections::HashSet;

pub type NodeId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum DomError {
    HierarchyRequest(&'static str),
    NotFound,
    InvalidNode,
    Syntax(String),
}

impl std::fmt::Display for DomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomError::HierarchyRequest(m) => write!(f, "HierarchyRequestError: {m}"),
            DomError::NotFound => write!(f, "NotFoundError: the node is not a child of this node"),
            DomError::InvalidNode => write!(f, "InvalidNodeTypeError"),
            DomError::Syntax(m) => write!(f, "SyntaxError: {m}"),
        }
    }
}
impl std::error::Error for DomError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Element = 1,
    Text = 3,
    Comment = 8,
    Document = 9,
    Fragment = 11,
}

#[derive(Debug, Clone, Default)]
pub struct FormState {
    /// Current value of an input/textarea/select (separate from the `value` attribute once edited).
    pub value: String,
    pub checked: bool,
    /// Caret/selection in UTF-8 byte offsets into `value`.
    pub selection_start: usize,
    pub selection_end: usize,
    pub value_dirty: bool,
    pub checked_dirty: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ElementFlags {
    pub hover: bool,
    pub active: bool,
    pub focus: bool,
    pub focus_within: bool,
}

#[derive(Debug, Clone)]
pub struct ElementData {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub inline_style: Vec<Declaration>,
    pub flags: ElementFlags,
    pub form: FormState,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Document,
    Fragment,
    Element(ElementData),
    Text(String),
    Comment(String),
    Free,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
    /// Number of live JS wrappers (see [`Document::retain`]).
    pub refs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MutationKind {
    ChildList,
    Attributes,
    CharacterData,
}

#[derive(Debug, Clone)]
pub struct MutationRecord {
    pub kind: MutationKind,
    pub target: NodeId,
    /// Ancestors of `target` at the time of the mutation (nearest first), for `subtree` matching.
    pub ancestors: Vec<NodeId>,
    pub added: Vec<NodeId>,
    pub removed: Vec<NodeId>,
    pub previous_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub attribute_name: Option<String>,
    pub old_value: Option<String>,
}

/// What changed since the pipeline last ran.
#[derive(Debug, Default, Clone)]
pub struct Dirty {
    /// Subtrees whose style must be recomputed.
    pub style_roots: Vec<NodeId>,
    /// Elements whose interaction state (hover/active/focus) changed. Their own style is recomputed;
    /// descendants only if the style changed (or the stylesheet has non-local state selectors).
    pub state_roots: Vec<NodeId>,
    /// Roots of subtrees that were detached; cached styles for them are stale if they are re-attached.
    pub detached: Vec<NodeId>,
    /// Every style is stale (stylesheet or viewport change).
    pub all_style: bool,
    /// Layout must rerun even if no style changed (text edits, scroll content size, etc.).
    pub layout: bool,
    /// Repaint needed without relayout (state that only changes paint is still restyled; this flags raw redraws).
    pub paint: bool,
}

pub struct Document {
    nodes: Vec<Node>,
    free: Vec<NodeId>,
    root: NodeId,
    pub dirty: Dirty,
    in_dirty_list: HashSet<NodeId>,
    in_state_list: HashSet<NodeId>,
    /// Bumped on every structural/attribute/text mutation.
    pub version: u64,
    record_mutations: bool,
    mutations: Vec<MutationRecord>,
    freed: Vec<NodeId>,
    gc_pending: bool,
    /// A `<style>`/`<link>` element or its content changed; stylesheets must be re-collected.
    pub sheets_dirty: bool,
    /// Bumped whenever a node leaves its parent (lets wrapper caches know to re-check connectivity).
    pub detach_epoch: u64,
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

impl Document {
    /// A document with `<html><head></head><body></body></html>`.
    pub fn new() -> Document {
        let mut d = Document::empty();
        let html = d.create_element("html");
        let head = d.create_element("head");
        let body = d.create_element("body");
        d.append_child(d.root, html).unwrap();
        d.append_child(html, head).unwrap();
        d.append_child(html, body).unwrap();
        d.dirty = Dirty { all_style: true, layout: true, paint: true, ..Dirty::default() };
        d
    }

    /// A document node only (used by the parser).
    pub fn empty() -> Document {
        let root_node = Node { kind: NodeKind::Document, parent: None, first_child: None, last_child: None, prev: None, next: None, refs: 1 };
        Document {
            nodes: vec![root_node],
            free: Vec::new(),
            root: 0,
            dirty: Dirty::default(),
            in_dirty_list: HashSet::new(),
            in_state_list: HashSet::new(),
            version: 0,
            record_mutations: false,
            mutations: Vec::new(),
            freed: Vec::new(),
            gc_pending: false,
            sheets_dirty: true,
            detach_epoch: 0,
        }
    }

    pub fn parse(source: &str) -> Document {
        html::parse_document(source)
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn is_alive(&self, id: NodeId) -> bool {
        self.nodes.get(id as usize).is_some_and(|n| !matches!(n.kind, NodeKind::Free))
    }

    /// Upper bound (exclusive) on live node ids.
    pub fn capacity(&self) -> usize {
        self.nodes.len()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len() - self.free.len()
    }

    fn alloc(&mut self, kind: NodeKind) -> NodeId {
        let node = Node { kind, parent: None, first_child: None, last_child: None, prev: None, next: None, refs: 0 };
        if let Some(id) = self.free.pop() {
            self.nodes[id as usize] = node;
            id
        } else {
            self.nodes.push(node);
            (self.nodes.len() - 1) as NodeId
        }
    }

    // ----- creation -----

    pub fn create_element(&mut self, tag: &str) -> NodeId {
        let tag = tag.to_ascii_lowercase();
        let mut form = FormState::default();
        if tag == "input" {
            form.value = String::new();
        }
        self.alloc(NodeKind::Element(ElementData {
            tag,
            attrs: Vec::new(),
            id: None,
            classes: Vec::new(),
            inline_style: Vec::new(),
            flags: ElementFlags::default(),
            form,
        }))
    }

    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.alloc(NodeKind::Text(text.to_string()))
    }

    pub fn create_comment(&mut self, text: &str) -> NodeId {
        self.alloc(NodeKind::Comment(text.to_string()))
    }

    pub fn create_fragment(&mut self) -> NodeId {
        self.alloc(NodeKind::Fragment)
    }

    // ----- accessors -----

    pub fn node_type(&self, id: NodeId) -> NodeType {
        match self.node(id).kind {
            NodeKind::Element(_) => NodeType::Element,
            NodeKind::Text(_) => NodeType::Text,
            NodeKind::Comment(_) => NodeType::Comment,
            NodeKind::Document => NodeType::Document,
            NodeKind::Fragment => NodeType::Fragment,
            NodeKind::Free => NodeType::Comment,
        }
    }

    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        match &self.node(id).kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut ElementData> {
        match &mut self.nodes[id as usize].kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn tag(&self, id: NodeId) -> Option<&str> {
        self.element(id).map(|e| e.tag.as_str())
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).first_child
    }
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).last_child
    }
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).next
    }
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).prev
    }

    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children { doc: self, next: self.node(id).first_child }
    }

    pub fn child_elements(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id).filter(|c| matches!(self.node(*c).kind, NodeKind::Element(_)))
    }

    pub fn document_element(&self) -> Option<NodeId> {
        self.child_elements(self.root).next()
    }

    pub fn head(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.child_elements(html).find(|c| self.tag(*c) == Some("head"))
    }

    pub fn body(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.child_elements(html).find(|c| self.tag(*c) == Some("body"))
    }

    pub fn is_connected(&self, id: NodeId) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == self.root {
                return true;
            }
            cur = self.node(c).parent;
        }
        false
    }

    pub fn contains(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            match self.node(node).parent {
                Some(p) => node = p,
                None => return false,
            }
        }
    }

    pub fn depth_first(&self, start: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            out.push(n);
            let mut kids: Vec<NodeId> = self.children(n).collect();
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    /// Target → root ancestors, the order events bubble through.
    pub fn event_path(&self, target: NodeId) -> Vec<NodeId> {
        let mut path = vec![target];
        let mut cur = self.node(target).parent;
        while let Some(p) = cur {
            path.push(p);
            cur = self.node(p).parent;
        }
        path
    }

    // ----- text -----

    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).kind {
            NodeKind::Text(t) | NodeKind::Comment(t) => Some(t),
            _ => None,
        }
    }

    pub fn set_text(&mut self, id: NodeId, value: &str) {
        let old = match &mut self.nodes[id as usize].kind {
            NodeKind::Text(t) | NodeKind::Comment(t) => std::mem::replace(t, value.to_string()),
            _ => return,
        };
        self.record(MutationRecord {
            kind: MutationKind::CharacterData,
            ancestors: Vec::new(),
            target: id,
            added: vec![],
            removed: vec![],
            previous_sibling: None,
            next_sibling: None,
            attribute_name: None,
            old_value: Some(old),
        });
        self.touch(id, true);
    }

    pub fn text_content(&self, id: NodeId) -> String {
        match &self.node(id).kind {
            NodeKind::Text(t) | NodeKind::Comment(t) => t.clone(),
            NodeKind::Element(_) | NodeKind::Fragment | NodeKind::Document => {
                let mut s = String::new();
                for n in self.depth_first(id) {
                    if let NodeKind::Text(t) = &self.node(n).kind {
                        s.push_str(t);
                    }
                }
                s
            }
            NodeKind::Free => String::new(),
        }
    }

    pub fn set_text_content(&mut self, id: NodeId, value: &str) {
        match self.node(id).kind {
            NodeKind::Text(_) | NodeKind::Comment(_) => self.set_text(id, value),
            _ => {
                self.remove_all_children(id);
                if !value.is_empty() {
                    let t = self.create_text(value);
                    let _ = self.append_child(id, t);
                }
            }
        }
    }

    // ----- tree mutation -----

    fn check_insert(&self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        match self.node(parent).kind {
            NodeKind::Element(_) | NodeKind::Fragment | NodeKind::Document => {}
            _ => return Err(DomError::HierarchyRequest("parent cannot have children")),
        }
        if matches!(self.node(child).kind, NodeKind::Document) {
            return Err(DomError::HierarchyRequest("a Document cannot be inserted"));
        }
        if self.contains(child, parent) {
            return Err(DomError::HierarchyRequest("the new child contains the parent"));
        }
        Ok(())
    }

    /// Detach `id` from its parent without recording a mutation or marking dirty.
    fn unlink(&mut self, id: NodeId) {
        self.detach_epoch += 1;
        self.dirty.detached.push(id);
        let (parent, prev, next) = {
            let n = self.node(id);
            (n.parent, n.prev, n.next)
        };
        if let Some(p) = prev {
            self.nodes[p as usize].next = next;
        } else if let Some(par) = parent {
            self.nodes[par as usize].first_child = next;
        }
        if let Some(nx) = next {
            self.nodes[nx as usize].prev = prev;
        } else if let Some(par) = parent {
            self.nodes[par as usize].last_child = prev;
        }
        let n = &mut self.nodes[id as usize];
        n.parent = None;
        n.prev = None;
        n.next = None;
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<NodeId, DomError> {
        self.insert_before(parent, child, None)
    }

    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) -> Result<NodeId, DomError> {
        self.check_insert(parent, child)?;
        if let Some(r) = reference {
            if self.node(r).parent != Some(parent) {
                return Err(DomError::NotFound);
            }
            if r == child {
                return Ok(child);
            }
        }
        if matches!(self.node(child).kind, NodeKind::Fragment) {
            let kids: Vec<NodeId> = self.children(child).collect();
            for k in &kids {
                self.unlink(*k);
            }
            self.nodes[child as usize].first_child = None;
            self.nodes[child as usize].last_child = None;
            for k in &kids {
                self.link_before(parent, *k, reference);
            }
            self.after_children_changed(parent, kids, vec![], reference);
            return Ok(child);
        }
        if let Some(old_parent) = self.node(child).parent {
            let prev = self.node(child).prev;
            let next = self.node(child).next;
            self.unlink(child);
            self.record(MutationRecord {
                kind: MutationKind::ChildList,
                ancestors: Vec::new(),
                target: old_parent,
                added: vec![],
                removed: vec![child],
                previous_sibling: prev,
                next_sibling: next,
                attribute_name: None,
                old_value: None,
            });
            self.mark_child_list_dirty(old_parent);
        }
        self.link_before(parent, child, reference);
        self.after_children_changed(parent, vec![child], vec![], reference);
        Ok(child)
    }

    fn link_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        let prev = match reference {
            Some(r) => self.node(r).prev,
            None => self.node(parent).last_child,
        };
        {
            let n = &mut self.nodes[child as usize];
            n.parent = Some(parent);
            n.prev = prev;
            n.next = reference;
        }
        match prev {
            Some(p) => self.nodes[p as usize].next = Some(child),
            None => self.nodes[parent as usize].first_child = Some(child),
        }
        match reference {
            Some(r) => self.nodes[r as usize].prev = Some(child),
            None => self.nodes[parent as usize].last_child = Some(child),
        }
    }

    fn after_children_changed(&mut self, parent: NodeId, added: Vec<NodeId>, removed: Vec<NodeId>, reference: Option<NodeId>) {
        self.note_sheet_change(parent, &added);
        let prev = added.first().and_then(|a| self.node(*a).prev);
        self.record(MutationRecord {
            kind: MutationKind::ChildList,
            ancestors: Vec::new(),
            target: parent,
            added,
            removed,
            previous_sibling: prev,
            next_sibling: reference,
            attribute_name: None,
            old_value: None,
        });
        self.mark_child_list_dirty(parent);
        self.dirty.layout = true;
        self.version += 1;
    }

    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<NodeId, DomError> {
        if self.node(child).parent != Some(parent) {
            return Err(DomError::NotFound);
        }
        let prev = self.node(child).prev;
        let next = self.node(child).next;
        self.unlink(child);
        self.note_sheet_change(parent, &[child]);
        self.record(MutationRecord {
            kind: MutationKind::ChildList,
            ancestors: Vec::new(),
            target: parent,
            added: vec![],
            removed: vec![child],
            previous_sibling: prev,
            next_sibling: next,
            attribute_name: None,
            old_value: None,
        });
        self.mark_child_list_dirty(parent);
        self.dirty.layout = true;
        self.version += 1;
        self.gc_pending = true;
        Ok(child)
    }

    pub fn remove(&mut self, id: NodeId) {
        if let Some(p) = self.node(id).parent {
            let _ = self.remove_child(p, id);
        }
    }

    pub fn replace_child(&mut self, parent: NodeId, new: NodeId, old: NodeId) -> Result<NodeId, DomError> {
        if self.node(old).parent != Some(parent) {
            return Err(DomError::NotFound);
        }
        let next = self.node(old).next;
        self.insert_before(parent, new, Some(old))?;
        let _ = next;
        self.remove_child(parent, old)
    }

    pub fn remove_all_children(&mut self, id: NodeId) {
        let kids: Vec<NodeId> = self.children(id).collect();
        for k in kids {
            let _ = self.remove_child(id, k);
        }
    }

    /// Clone a node (and its subtree when `deep`). The clone is detached.
    pub fn clone_node(&mut self, id: NodeId, deep: bool) -> NodeId {
        let kind = self.node(id).kind.clone();
        let copy = match kind {
            NodeKind::Element(mut e) => {
                e.flags = ElementFlags::default();
                self.alloc(NodeKind::Element(e))
            }
            NodeKind::Document | NodeKind::Free => self.create_fragment(),
            k => self.alloc(k),
        };
        if deep {
            let kids: Vec<NodeId> = self.children(id).collect();
            for k in kids {
                let c = self.clone_node(k, true);
                let _ = self.append_child(copy, c);
            }
        }
        copy
    }

    // ----- attributes -----

    pub fn get_attr(&self, id: NodeId, name: &str) -> Option<&str> {
        let e = self.element(id)?;
        let name = name.to_ascii_lowercase();
        e.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    pub fn has_attr(&self, id: NodeId, name: &str) -> bool {
        self.get_attr(id, name).is_some()
    }

    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        let Some(e) = self.element_mut(id) else { return };
        let old = e.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone());
        if old.as_deref() == Some(value) {
            return;
        }
        match e.attrs.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value.to_string(),
            None => e.attrs.push((name.clone(), value.to_string())),
        }
        Self::sync_cached_attr(e, &name, Some(value));
        self.record(MutationRecord {
            kind: MutationKind::Attributes,
            ancestors: Vec::new(),
            target: id,
            added: vec![],
            removed: vec![],
            previous_sibling: None,
            next_sibling: None,
            attribute_name: Some(name),
            old_value: old,
        });
        self.touch(id, false);
    }

    pub fn remove_attr(&mut self, id: NodeId, name: &str) {
        let name = name.to_ascii_lowercase();
        let Some(e) = self.element_mut(id) else { return };
        let Some(pos) = e.attrs.iter().position(|(k, _)| *k == name) else { return };
        let (_, old) = e.attrs.remove(pos);
        Self::sync_cached_attr(e, &name, None);
        self.record(MutationRecord {
            kind: MutationKind::Attributes,
            ancestors: Vec::new(),
            target: id,
            added: vec![],
            removed: vec![],
            previous_sibling: None,
            next_sibling: None,
            attribute_name: Some(name),
            old_value: Some(old),
        });
        self.touch(id, false);
    }

    fn sync_cached_attr(e: &mut ElementData, name: &str, value: Option<&str>) {
        match name {
            "id" => e.id = value.map(str::to_string),
            "class" => e.classes = value.map(|v| v.split_whitespace().map(str::to_string).collect()).unwrap_or_default(),
            "style" => e.inline_style = value.map(parse_declarations).unwrap_or_default(),
            "value" if !e.form.value_dirty => {
                e.form.value = value.unwrap_or("").to_string();
                e.form.selection_start = e.form.value.len();
                e.form.selection_end = e.form.value.len();
            }
            "checked" if !e.form.checked_dirty => e.form.checked = value.is_some(),
            _ => {}
        }
    }

    pub fn class_list(&self, id: NodeId) -> Vec<String> {
        self.element(id).map(|e| e.classes.clone()).unwrap_or_default()
    }

    pub fn toggle_class(&mut self, id: NodeId, class: &str, force: Option<bool>) -> bool {
        let mut classes = self.class_list(id);
        let has = classes.iter().any(|c| c == class);
        let want = force.unwrap_or(!has);
        if want && !has {
            classes.push(class.to_string());
        } else if !want && has {
            classes.retain(|c| c != class);
        }
        if want != has {
            self.set_attr(id, "class", &classes.join(" "));
        }
        want
    }

    // ----- form state -----

    pub fn form_value(&self, id: NodeId) -> String {
        self.element(id).map(|e| e.form.value.clone()).unwrap_or_default()
    }

    pub fn set_form_value(&mut self, id: NodeId, value: &str) {
        let Some(e) = self.element_mut(id) else { return };
        e.form.value = value.to_string();
        e.form.value_dirty = true;
        e.form.selection_start = value.len();
        e.form.selection_end = value.len();
        self.touch(id, true);
    }

    pub fn set_selection(&mut self, id: NodeId, start: usize, end: usize) {
        if let Some(e) = self.element_mut(id) {
            let len = e.form.value.len();
            e.form.selection_start = start.min(len);
            e.form.selection_end = end.min(len);
        }
        self.dirty.paint = true;
    }

    /// The `value` of a form control: edited value, else the default from markup.
    pub fn value(&self, id: NodeId) -> String {
        match self.tag(id) {
            Some("textarea") => {
                let e = self.element(id).unwrap();
                if e.form.value_dirty { e.form.value.clone() } else { self.text_content(id) }
            }
            Some("select") => {
                let opts = self.get_elements_by_tag_name(id, "option");
                let chosen = opts.iter().find(|o| self.has_attr(**o, "selected")).or(opts.first());
                match chosen {
                    Some(o) => self.get_attr(*o, "value").map(str::to_string).unwrap_or_else(|| self.text_content(*o)),
                    None => String::new(),
                }
            }
            _ => self.form_value(id),
        }
    }

    pub fn is_checked(&self, id: NodeId) -> bool {
        self.element(id).is_some_and(|e| e.form.checked)
    }

    pub fn set_checked(&mut self, id: NodeId, checked: bool) {
        let Some(e) = self.element_mut(id) else { return };
        e.form.checked = checked;
        e.form.checked_dirty = true;
        self.touch(id, false);
    }

    // ----- interaction state -----

    pub fn set_hover_chain(&mut self, new_target: Option<NodeId>) {
        let mut want: HashSet<NodeId> = HashSet::new();
        if let Some(t) = new_target {
            for n in self.event_path(t) {
                want.insert(n);
            }
        }
        let all: Vec<NodeId> = self.depth_first(self.root);
        for n in all {
            let is = want.contains(&n);
            let was = self.element(n).is_some_and(|e| e.flags.hover);
            if is != was {
                if let Some(e) = self.element_mut(n) {
                    e.flags.hover = is;
                }
                self.mark_state_dirty(n);
            }
        }
    }

    pub fn set_active_chain(&mut self, target: Option<NodeId>) {
        let want: HashSet<NodeId> = target.map(|t| self.event_path(t).into_iter().collect()).unwrap_or_default();
        for n in self.depth_first(self.root) {
            let is = want.contains(&n);
            let was = self.element(n).is_some_and(|e| e.flags.active);
            if is != was {
                if let Some(e) = self.element_mut(n) {
                    e.flags.active = is;
                }
                self.mark_state_dirty(n);
            }
        }
    }

    pub fn set_focus(&mut self, target: Option<NodeId>) {
        let want_within: HashSet<NodeId> = target.map(|t| self.event_path(t).into_iter().collect()).unwrap_or_default();
        for n in self.depth_first(self.root) {
            let (is_focus, is_within) = (Some(n) == target, want_within.contains(&n) && Some(n) != target);
            let Some(e) = self.element(n) else { continue };
            if e.flags.focus != is_focus || e.flags.focus_within != is_within {
                if let Some(e) = self.element_mut(n) {
                    e.flags.focus = is_focus;
                    e.flags.focus_within = is_within;
                }
                self.mark_state_dirty(n);
            }
        }
    }

    pub fn focused(&self) -> Option<NodeId> {
        self.depth_first(self.root).into_iter().find(|n| self.element(*n).is_some_and(|e| e.flags.focus))
    }

    /// Can this element receive keyboard focus?
    pub fn is_focusable(&self, id: NodeId) -> bool {
        let Some(e) = self.element(id) else { return false };
        if self.has_attr(id, "disabled") {
            return false;
        }
        if let Some(t) = self.get_attr(id, "tabindex") {
            return t.trim().parse::<i32>().map(|n| n >= 0).unwrap_or(false);
        }
        matches!(e.tag.as_str(), "button" | "input" | "select" | "textarea") && self.get_attr(id, "type") != Some("hidden")
            || (e.tag == "a" && self.has_attr(id, "href"))
    }

    pub fn is_text_control(&self, id: NodeId) -> bool {
        match self.tag(id) {
            Some("textarea") => true,
            Some("input") => !matches!(
                self.get_attr(id, "type").unwrap_or("text").to_ascii_lowercase().as_str(),
                "checkbox" | "radio" | "button" | "submit" | "reset" | "image" | "file" | "range" | "color" | "hidden"
            ),
            _ => false,
        }
    }

    // ----- dirty tracking -----

    pub fn mark_style_dirty(&mut self, id: NodeId) {
        if self.dirty.all_style {
            return;
        }
        if self.in_dirty_list.insert(id) {
            self.dirty.style_roots.push(id);
        }
        self.version += 1;
    }

    /// A child list changed: the parent's own style and (if structural selectors exist) its children's
    /// styles may change; the engine decides how deep to go.
    pub fn mark_child_list_dirty(&mut self, id: NodeId) {
        self.mark_state_dirty(id);
    }

    pub fn mark_state_dirty(&mut self, id: NodeId) {
        if self.dirty.all_style {
            return;
        }
        if self.in_state_list.insert(id) {
            self.dirty.state_roots.push(id);
        }
        self.version += 1;
    }

    pub fn mark_all_style_dirty(&mut self) {
        self.dirty.all_style = true;
        self.dirty.layout = true;
        self.dirty.paint = true;
        self.version += 1;
    }

    fn note_sheet_change(&mut self, parent: NodeId, nodes: &[NodeId]) {
        let is_sheet = |d: &Document, n: NodeId| matches!(d.tag(n), Some("style") | Some("link"));
        if is_sheet(self, parent) || nodes.iter().any(|n| is_sheet(self, *n) || self.depth_first(*n).iter().any(|m| is_sheet(self, *m))) {
            self.sheets_dirty = true;
        }
    }

    fn touch(&mut self, id: NodeId, layout_only: bool) {
        if matches!(self.tag(id), Some("style") | Some("link")) || self.node(id).parent.is_some_and(|p| self.tag(p) == Some("style")) {
            self.sheets_dirty = true;
        }
        if layout_only {
            self.dirty.layout = true;
            self.dirty.paint = true;
            self.version += 1;
            // Text changes can alter inherited-style-independent layout only, but pseudo/`:empty` matters rarely.
            if let Some(p) = self.node(id).parent {
                self.mark_style_dirty(p);
            }
        } else {
            self.mark_style_dirty(id);
            self.dirty.layout = true;
        }
    }

    pub fn take_dirty(&mut self) -> Dirty {
        self.in_dirty_list.clear();
        self.in_state_list.clear();
        std::mem::take(&mut self.dirty)
    }

    // ----- mutation observation -----

    pub fn set_record_mutations(&mut self, on: bool) {
        self.record_mutations = on;
        if !on {
            self.mutations.clear();
        }
    }

    fn record(&mut self, mut r: MutationRecord) {
        if self.record_mutations {
            let mut cur = self.node(r.target).parent;
            while let Some(p) = cur {
                r.ancestors.push(p);
                cur = self.node(p).parent;
            }
            self.mutations.push(r);
        }
    }

    pub fn take_mutations(&mut self) -> Vec<MutationRecord> {
        std::mem::take(&mut self.mutations)
    }

    // ----- memory management -----

    /// A JS wrapper for `id` exists.
    pub fn retain(&mut self, id: NodeId) {
        self.nodes[id as usize].refs += 1;
    }

    /// A JS wrapper for `id` was collected.
    pub fn release(&mut self, id: NodeId) {
        if let Some(n) = self.nodes.get_mut(id as usize) {
            n.refs = n.refs.saturating_sub(1);
        }
        self.gc_pending = true;
    }

    /// Free detached subtrees that no JS wrapper can reach. Returns freed ids (caches must drop them).
    pub fn collect_garbage(&mut self) -> Vec<NodeId> {
        if !self.gc_pending {
            return Vec::new();
        }
        self.gc_pending = false;
        let mut reachable = vec![false; self.nodes.len()];
        for n in self.depth_first(self.root) {
            reachable[n as usize] = true;
        }
        let mut freed = Vec::new();
        for id in 0..self.nodes.len() as NodeId {
            let node = &self.nodes[id as usize];
            if reachable[id as usize] || matches!(node.kind, NodeKind::Free) || node.parent.is_some() {
                continue;
            }
            // `id` is the root of a detached tree.
            let tree = self.depth_first(id);
            if tree.iter().all(|n| self.nodes[*n as usize].refs == 0) {
                for n in tree {
                    self.nodes[n as usize] = Node { kind: NodeKind::Free, parent: None, first_child: None, last_child: None, prev: None, next: None, refs: 0 };
                    self.free.push(n);
                    freed.push(n);
                }
            }
        }
        self.freed.extend(freed.iter().copied());
        freed
    }

    pub fn take_freed(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.freed)
    }

    // ----- html -----

    pub fn inner_html(&self, id: NodeId) -> String {
        html::serialize_children(self, id)
    }

    pub fn outer_html(&self, id: NodeId) -> String {
        html::serialize_node(self, id)
    }

    pub fn set_inner_html(&mut self, id: NodeId, source: &str) {
        self.remove_all_children(id);
        let ctx = self.tag(id).map(str::to_string);
        let frag = html::parse_fragment_into(self, source, ctx.as_deref());
        let _ = self.append_child(id, frag);
    }

    /// Parse markup into a detached fragment node.
    pub fn parse_fragment(&mut self, source: &str) -> NodeId {
        html::parse_fragment_into(self, source, None)
    }
}

pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.doc.node(cur).next;
        Some(cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(d: &mut Document, tag: &str) -> NodeId {
        d.create_element(tag)
    }

    #[test]
    fn tree_ops_and_links() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let (a, b, c) = (el(&mut d, "a"), el(&mut d, "b"), el(&mut d, "i"));
        d.append_child(body, a).unwrap();
        d.append_child(body, c).unwrap();
        d.insert_before(body, b, Some(c)).unwrap();
        assert_eq!(d.children(body).collect::<Vec<_>>(), vec![a, b, c]);
        assert_eq!(d.prev_sibling(c), Some(b));
        d.remove_child(body, b).unwrap();
        assert_eq!(d.children(body).collect::<Vec<_>>(), vec![a, c]);
        assert_eq!(d.next_sibling(a), Some(c));
        // moving re-parents
        d.append_child(body, a).unwrap();
        assert_eq!(d.children(body).collect::<Vec<_>>(), vec![c, a]);
    }

    #[test]
    fn rejects_cycles_and_bad_references() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let (p, ch) = (el(&mut d, "div"), el(&mut d, "div"));
        d.append_child(body, p).unwrap();
        d.append_child(p, ch).unwrap();
        assert!(matches!(d.append_child(ch, p), Err(DomError::HierarchyRequest(_))));
        assert!(matches!(d.append_child(p, p), Err(DomError::HierarchyRequest(_))));
        let stranger = el(&mut d, "x");
        assert_eq!(d.insert_before(p, stranger, Some(body)), Err(DomError::NotFound));
        let t = d.create_text("t");
        assert!(d.append_child(t, ch).is_err());
    }

    #[test]
    fn fragments_move_their_children() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let f = d.create_fragment();
        let (a, b) = (el(&mut d, "a"), el(&mut d, "b"));
        d.append_child(f, a).unwrap();
        d.append_child(f, b).unwrap();
        d.append_child(body, f).unwrap();
        assert_eq!(d.children(body).collect::<Vec<_>>(), vec![a, b]);
        assert!(d.first_child(f).is_none());
    }

    #[test]
    fn attributes_update_cached_fields_and_dirty() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let e = el(&mut d, "div");
        d.append_child(body, e).unwrap();
        d.take_dirty();
        d.set_attr(e, "ID", "x");
        d.set_attr(e, "class", "a  b");
        d.set_attr(e, "style", "color: red; margin: 1px");
        let data = d.element(e).unwrap();
        assert_eq!(data.id.as_deref(), Some("x"));
        assert_eq!(data.classes, vec!["a", "b"]);
        assert_eq!(data.inline_style.len(), 2);
        assert_eq!(d.dirty.style_roots, vec![e]);
        d.remove_attr(e, "class");
        assert!(d.class_list(e).is_empty());
        assert!(d.toggle_class(e, "on", None));
        assert!(!d.toggle_class(e, "on", None));
    }

    #[test]
    fn mutation_records() {
        let mut d = Document::new();
        d.set_record_mutations(true);
        let body = d.body().unwrap();
        let e = el(&mut d, "p");
        d.append_child(body, e).unwrap();
        d.set_attr(e, "title", "t");
        d.set_attr(e, "title", "u");
        let t = d.create_text("hi");
        d.append_child(e, t).unwrap();
        d.set_text(t, "yo");
        let m = d.take_mutations();
        assert_eq!(m.len(), 5);
        assert_eq!(m[2].old_value.as_deref(), Some("t"));
        assert_eq!(m[4].old_value.as_deref(), Some("hi"));
        assert!(matches!(m[0].kind, MutationKind::ChildList) && m[0].added == vec![e]);
    }

    #[test]
    fn text_content_roundtrip() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        d.set_inner_html(body, "<p>a<b>b</b></p>c");
        assert_eq!(d.text_content(body), "abc");
        d.set_text_content(body, "x");
        assert_eq!(d.children(body).count(), 1);
        assert_eq!(d.text_content(body), "x");
    }

    #[test]
    fn event_path_runs_target_to_root() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let e = el(&mut d, "div");
        d.append_child(body, e).unwrap();
        let path = d.event_path(e);
        assert_eq!(path.first(), Some(&e));
        assert_eq!(path[1], body);
        assert_eq!(*path.last().unwrap(), d.root());
    }

    #[test]
    fn hover_chain_marks_ancestors() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let (a, b) = (el(&mut d, "div"), el(&mut d, "div"));
        d.append_child(body, a).unwrap();
        d.append_child(a, b).unwrap();
        d.set_hover_chain(Some(b));
        assert!(d.element(a).unwrap().flags.hover && d.element(b).unwrap().flags.hover);
        d.set_hover_chain(None);
        assert!(!d.element(a).unwrap().flags.hover);
    }

    #[test]
    fn garbage_collection_frees_only_unreferenced_detached_trees() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        let (kept, dropped) = (el(&mut d, "div"), el(&mut d, "div"));
        let child = el(&mut d, "span");
        d.append_child(body, kept).unwrap();
        d.append_child(body, dropped).unwrap();
        d.append_child(dropped, child).unwrap();
        d.retain(child); // a JS wrapper still points into the detached subtree
        d.remove_child(body, dropped).unwrap();
        assert!(d.collect_garbage().is_empty(), "referenced descendant keeps the subtree alive");
        d.release(child);
        let freed = d.collect_garbage();
        assert_eq!(freed.len(), 2);
        assert!(!d.is_alive(dropped) && d.is_alive(kept));
        let reused = el(&mut d, "p");
        assert!(freed.contains(&reused), "freed slots are reused");
    }

    #[test]
    fn clone_deep_copies_attrs_not_flags() {
        let mut d = Document::new();
        let body = d.body().unwrap();
        d.set_inner_html(body, "<div class=\"x\" id=\"q\"><b>t</b></div>");
        let div = d.first_child(body).unwrap();
        d.set_hover_chain(Some(div));
        let copy = d.clone_node(div, true);
        assert_eq!(d.outer_html(copy), d.outer_html(div));
        assert!(!d.element(copy).unwrap().flags.hover);
        assert!(d.parent(copy).is_none());
    }
}

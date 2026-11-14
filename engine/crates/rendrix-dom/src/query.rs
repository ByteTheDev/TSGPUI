//! Selector-based queries over the DOM, built on `rendrix_css::SelectorElement`.

use crate::{Document, NodeId, NodeKind};
use rendrix_css::{ElementState, Selector, SelectorElement};

/// A borrowed handle to an element, usable with the CSS selector engine.
#[derive(Clone, Copy)]
pub struct ElRef<'a> {
    pub doc: &'a Document,
    pub id: NodeId,
}

impl<'a> ElRef<'a> {
    pub fn new(doc: &'a Document, id: NodeId) -> Self {
        ElRef { doc, id }
    }

    fn data(&self) -> &'a crate::ElementData {
        self.doc.element(self.id).expect("ElRef must point at an element")
    }

    fn sibling(&self, mut cur: Option<NodeId>, forward: bool) -> Option<ElRef<'a>> {
        while let Some(n) = cur {
            if matches!(self.doc.node(n).kind, NodeKind::Element(_)) {
                return Some(ElRef { doc: self.doc, id: n });
            }
            cur = if forward { self.doc.next_sibling(n) } else { self.doc.prev_sibling(n) };
        }
        None
    }
}

impl SelectorElement for ElRef<'_> {
    fn tag(&self) -> &str {
        &self.data().tag
    }
    fn id(&self) -> Option<&str> {
        self.data().id.as_deref()
    }
    fn has_class(&self, class: &str) -> bool {
        self.data().classes.iter().any(|c| c == class)
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.doc.get_attr(self.id, name)
    }
    fn parent_element(&self) -> Option<Self> {
        let p = self.doc.parent(self.id)?;
        matches!(self.doc.node(p).kind, NodeKind::Element(_)).then_some(ElRef { doc: self.doc, id: p })
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(self.doc.prev_sibling(self.id), false)
    }
    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(self.doc.next_sibling(self.id), true)
    }
    fn has_state(&self, state: ElementState) -> bool {
        let d = self.data();
        match state {
            ElementState::Hover => d.flags.hover,
            ElementState::Active => d.flags.active,
            ElementState::Focus => d.flags.focus,
            ElementState::FocusWithin => d.flags.focus_within,
            ElementState::Disabled => self.doc.has_attr(self.id, "disabled"),
            ElementState::Checked => {
                d.form.checked || (d.tag == "option" && self.doc.has_attr(self.id, "selected"))
            }
        }
    }
    fn is_root(&self) -> bool {
        self.doc.parent(self.id).is_some_and(|p| matches!(self.doc.node(p).kind, NodeKind::Document))
    }
    fn is_empty(&self) -> bool {
        self.doc.children(self.id).all(|c| match &self.doc.node(c).kind {
            NodeKind::Text(t) => t.is_empty(),
            NodeKind::Comment(_) => true,
            _ => false,
        })
    }
}

impl Document {
    pub fn matches(&self, el: NodeId, selector: &str) -> Result<bool, crate::DomError> {
        let list = parse_selector_list(selector)?;
        if self.element(el).is_none() {
            return Ok(false);
        }
        let r = ElRef::new(self, el);
        Ok(list.iter().any(|s| s.matches(&r)))
    }

    pub fn closest(&self, el: NodeId, selector: &str) -> Result<Option<NodeId>, crate::DomError> {
        let list = parse_selector_list(selector)?;
        let mut cur = Some(el);
        while let Some(n) = cur {
            if self.element(n).is_some() && list.iter().any(|s| s.matches(&ElRef::new(self, n))) {
                return Ok(Some(n));
            }
            cur = self.parent(n);
        }
        Ok(None)
    }

    /// First descendant of `root` (excluding `root`) matching `selector`, in document order.
    pub fn query_selector(&self, root: NodeId, selector: &str) -> Result<Option<NodeId>, crate::DomError> {
        Ok(self.query_selector_all(root, selector)?.into_iter().next())
    }

    pub fn query_selector_all(&self, root: NodeId, selector: &str) -> Result<Vec<NodeId>, crate::DomError> {
        let list = parse_selector_list(selector)?;
        let mut out = Vec::new();
        for n in self.depth_first(root) {
            if n != root && self.element(n).is_some() && list.iter().any(|s| s.matches(&ElRef::new(self, n))) {
                out.push(n);
            }
        }
        Ok(out)
    }

    pub fn get_element_by_id(&self, id: &str) -> Option<NodeId> {
        self.depth_first(self.root()).into_iter().find(|n| self.element(*n).is_some_and(|e| e.id.as_deref() == Some(id)))
    }

    pub fn get_elements_by_tag_name(&self, root: NodeId, tag: &str) -> Vec<NodeId> {
        let tag = tag.to_ascii_lowercase();
        self.depth_first(root).into_iter().filter(|n| *n != root && self.element(*n).is_some_and(|e| tag == "*" || e.tag == tag)).collect()
    }

    pub fn get_elements_by_class_name(&self, root: NodeId, classes: &str) -> Vec<NodeId> {
        let want: Vec<&str> = classes.split_whitespace().collect();
        self.depth_first(root)
            .into_iter()
            .filter(|n| *n != root && self.element(*n).is_some_and(|e| !want.is_empty() && want.iter().all(|c| e.classes.iter().any(|x| x == c))))
            .collect()
    }
}

fn parse_selector_list(s: &str) -> Result<Vec<Selector>, crate::DomError> {
    let parts = rendrix_css::selector::split_top_level(s, ',');
    let mut out = Vec::new();
    for p in &parts {
        match Selector::parse(p.trim()) {
            Some(sel) => out.push(sel),
            None => return Err(crate::DomError::Syntax(format!("'{s}' is not a valid selector"))),
        }
    }
    if out.is_empty() {
        return Err(crate::DomError::Syntax(format!("'{s}' is not a valid selector")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        let mut d = Document::new();
        let body = d.body().unwrap();
        d.set_inner_html(
            body,
            r#"<div id="app" class="shell"><ul class="list"><li class="item a">1</li><li class="item">2</li><li class="item b">3</li></ul>
               <button disabled>x</button><input type="checkbox" checked></div>"#,
        );
        d
    }

    #[test]
    fn selectors_and_document_order() {
        let d = doc();
        let root = d.root();
        let all = d.query_selector_all(root, ".item").unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(d.text_content(all[2]), "3");
        assert_eq!(d.query_selector_all(root, "li:nth-child(odd)").unwrap().len(), 2);
        assert_eq!(d.query_selector_all(root, "#app > ul .a, .b").unwrap().len(), 2);
        assert_eq!(d.query_selector_all(root, ".item + .item").unwrap().len(), 2);
        assert_eq!(d.query_selector_all(root, "li:not(.a):not(.b)").unwrap().len(), 1);
        assert_eq!(d.query_selector_all(root, "button:disabled, input:checked").unwrap().len(), 2);
        assert!(d.query_selector(root, "section").unwrap().is_none());
    }

    #[test]
    fn matches_and_closest() {
        let d = doc();
        let li = d.query_selector(d.root(), ".b").unwrap().unwrap();
        assert!(d.matches(li, "ul > li.item").unwrap());
        assert_eq!(d.closest(li, ".shell").unwrap(), d.get_element_by_id("app"));
        assert_eq!(d.closest(li, "section").unwrap(), None);
    }

    #[test]
    fn invalid_selectors_error() {
        let d = doc();
        assert!(matches!(d.query_selector(d.root(), "li >"), Err(crate::DomError::Syntax(_))));
        assert!(d.query_selector(d.root(), "").is_err());
    }

    #[test]
    fn by_tag_and_class() {
        let d = doc();
        assert_eq!(d.get_elements_by_tag_name(d.root(), "li").len(), 3);
        assert_eq!(d.get_elements_by_class_name(d.root(), "item b").len(), 1);
    }
}

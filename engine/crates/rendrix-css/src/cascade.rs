//! The cascade: rule indexing, matching, ordering by origin/importance/specificity/source order,
//! custom property resolution, and computed style construction.

use crate::parser::{Declaration, FontFace, Keyframes, MediaContext, Rule, StyleRule, Stylesheet};
use crate::properties::{apply_declaration, StyleContext};
use crate::selector::{PseudoElement, Selector, SelectorElement};
use crate::style::{ComputedStyle, Display, Position};
use crate::values::substitute_vars;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent,
    Author,
}

#[derive(Debug)]
struct IndexedRule {
    selector: Selector,
    declarations: Arc<[Declaration]>,
    origin: Origin,
    order: u32,
}

#[derive(Debug, Default)]
pub struct RuleSet {
    rules: Vec<IndexedRule>,
    by_id: HashMap<String, Vec<u32>>,
    by_class: HashMap<String, Vec<u32>>,
    by_tag: HashMap<String, Vec<u32>>,
    universal: Vec<u32>,
    pub keyframes: HashMap<String, Keyframes>,
    pub font_faces: Vec<FontFace>,
    next_order: u32,
    /// Some selector tests hover/active/focus on a compound other than its subject (e.g. `.a:hover .b`),
    /// so a state change can restyle descendants or siblings of the element that changed.
    pub state_deps_nonlocal: bool,
    /// Some selector depends on sibling position or siblings (`:nth-child`, `:first-child`, `+`, `~`, `:empty`).
    pub structural_deps: bool,
}

impl RuleSet {
    pub fn new() -> Self {
        RuleSet::default()
    }

    /// Add every rule of `sheet` that applies under `media`.
    pub fn add_stylesheet(&mut self, sheet: &Stylesheet, origin: Origin, media: &MediaContext) {
        self.add_rules(&sheet.rules, origin, media);
    }

    fn add_rules(&mut self, rules: &[Rule], origin: Origin, media: &MediaContext) {
        for rule in rules {
            match rule {
                Rule::Style(r) => self.add_style_rule(r, origin),
                Rule::Media(q, inner) => {
                    if q.matches(media) {
                        self.add_rules(inner, origin, media);
                    }
                }
                Rule::Keyframes(k) => {
                    self.keyframes.insert(k.name.clone(), k.clone());
                }
                Rule::FontFace(f) => self.font_faces.push(f.clone()),
            }
        }
    }

    fn add_style_rule(&mut self, r: &StyleRule, origin: Origin) {
        let decls: Arc<[Declaration]> = r.declarations.clone().into();
        for sel in &r.selectors {
            let state = |c: &crate::selector::Compound| {
                fn has(p: &crate::selector::PseudoClass) -> bool {
                    use crate::selector::PseudoClass::*;
                    match p {
                        Hover | Active | Focus | FocusVisible | FocusWithin | Checked => true,
                        Not(l) | Is(l) | Where(l) => l.iter().any(|s| s.compounds.iter().any(|c| c.pseudos.iter().any(has))),
                        _ => false,
                    }
                }
                c.pseudos.iter().any(has)
            };
            let structural = {
                fn has(p: &crate::selector::PseudoClass) -> bool {
                    use crate::selector::PseudoClass::*;
                    match p {
                        FirstChild | LastChild | OnlyChild | NthChild(..) | NthLastChild(..) | NthOfType(..) | FirstOfType | LastOfType | Empty => true,
                        Not(l) | Is(l) | Where(l) => l.iter().any(|s| s.compounds.iter().any(|c| c.pseudos.iter().any(has))),
                        _ => false,
                    }
                }
                sel.compounds.iter().any(|c| c.pseudos.iter().any(has))
                    || sel.combinators.iter().any(|c| matches!(c, crate::selector::Combinator::NextSibling | crate::selector::Combinator::SubsequentSibling))
            };
            if structural {
                self.structural_deps = true;
            }
            if sel.compounds.len() > 1 && sel.compounds[..sel.compounds.len() - 1].iter().any(state) {
                self.state_deps_nonlocal = true;
            }
            let idx = self.rules.len() as u32;
            let subject = sel.compounds.last().unwrap();
            if let Some(id) = &subject.id {
                self.by_id.entry(id.clone()).or_default().push(idx);
            } else if let Some(c) = subject.classes.first() {
                self.by_class.entry(c.clone()).or_default().push(idx);
            } else if let Some(t) = &subject.tag {
                self.by_tag.entry(t.clone()).or_default().push(idx);
            } else {
                self.universal.push(idx);
            }
            self.rules.push(IndexedRule { selector: sel.clone(), declarations: decls.clone(), origin, order: self.next_order });
            self.next_order += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Rules matching `el` (for `pseudo`, the originating element of that pseudo-element), sorted by
    /// ascending cascade precedence: (origin, specificity, source order).
    fn matching<E: SelectorElement>(&self, el: &E, pseudo: Option<PseudoElement>) -> Vec<&IndexedRule> {
        let mut candidates: Vec<u32> = Vec::new();
        if let Some(id) = el.id() {
            if let Some(v) = self.by_id.get(id) {
                candidates.extend_from_slice(v);
            }
        }
        // Class buckets: probe each class the element could carry via a class attribute scan.
        if let Some(class_attr) = el.attr("class") {
            for c in class_attr.split_whitespace() {
                if let Some(v) = self.by_class.get(c) {
                    candidates.extend_from_slice(v);
                }
            }
        }
        if let Some(v) = self.by_tag.get(el.tag()) {
            candidates.extend_from_slice(v);
        }
        candidates.extend_from_slice(&self.universal);
        candidates.sort_unstable();
        candidates.dedup();
        let mut hits: Vec<&IndexedRule> = candidates
            .into_iter()
            .map(|i| &self.rules[i as usize])
            .filter(|r| match pseudo {
                None => r.selector.matches(el),
                Some(p) => r.selector.pseudo_element == Some(p) && r.selector.matches_subject(el),
            })
            .collect();
        hits.sort_by_key(|r| (r.origin, r.selector.specificity, r.order));
        hits
    }
}

/// Compute the style of one element.
///
/// * `inline` are declarations from the `style` attribute (highest author normal precedence).
/// * `parent` is the parent's computed style (`None` for the root).
pub fn compute_style<E: SelectorElement>(
    el: &E,
    rules: &RuleSet,
    inline: &[Declaration],
    parent: Option<&ComputedStyle>,
    sc: &StyleContext,
    pseudo: Option<PseudoElement>,
) -> ComputedStyle {
    let default_parent = ComputedStyle::default();
    let parent_style = parent.unwrap_or(&default_parent);

    // Gather declarations with their cascade tier.
    // Tier order (ascending): UA normal, author normal, author important, UA important.
    let mut tiers: [Vec<&Declaration>; 4] = Default::default();
    for r in rules.matching(el, pseudo) {
        for d in r.declarations.iter() {
            let tier = match (r.origin, d.important) {
                (Origin::UserAgent, false) => 0,
                (Origin::Author, false) => 1,
                (Origin::Author, true) => 2,
                (Origin::UserAgent, true) => 3,
            };
            tiers[tier].push(d);
        }
    }
    if pseudo.is_none() {
        for d in inline {
            tiers[if d.important { 2 } else { 1 }].push(d);
        }
    }
    let ordered: Vec<&Declaration> = tiers.into_iter().flatten().collect();

    // Custom properties (inherited): start from the parent's, then apply in cascade order.
    let mut custom: BTreeMap<String, String> = (*parent_style.custom).clone();
    let mut any_custom = false;
    for d in &ordered {
        if d.name.starts_with("--") {
            custom.insert(d.name.clone(), d.value.clone());
            any_custom = true;
        }
    }

    let mut style = ComputedStyle::inherit_from(parent_style);
    if any_custom {
        style.custom = Arc::new(custom.clone());
    }
    // Un-inherit nothing else: inherit_from already gave initial values for non-inherited props.

    let value_of = |d: &Declaration| -> Option<String> {
        if d.value.contains("var(") {
            substitute_vars(&d.value, &custom)
        } else {
            Some(d.value.clone())
        }
    };

    // Pass 1: properties others depend on (font-size → em, color → currentcolor).
    for d in &ordered {
        if d.name.starts_with("--") {
            continue;
        }
        if matches!(d.name.as_str(), "font-size" | "font" | "color") {
            if let Some(v) = value_of(d) {
                apply_declaration(&mut style, parent_style, &d.name, &v, sc);
            }
        }
    }
    // Pass 2: everything else, in cascade order.
    for d in &ordered {
        if d.name.starts_with("--") || matches!(d.name.as_str(), "font-size" | "font" | "color") {
            continue;
        }
        if let Some(v) = value_of(d) {
            apply_declaration(&mut style, parent_style, &d.name, &v, sc);
        }
    }

    // Used-value adjustments that CSS applies at computed-value time.
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.display = match style.display {
            Display::Inline | Display::InlineBlock => Display::Block,
            Display::InlineFlex => Display::Flex,
            Display::InlineGrid => Display::Grid,
            d => d,
        };
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::selector::ElementState;
    use crate::values::Dim;

    /// A tiny element tree for testing the cascade in isolation.
    #[derive(Clone)]
    struct El {
        tree: std::rc::Rc<Vec<N>>,
        idx: usize,
    }
    struct N {
        tag: &'static str,
        id: Option<&'static str>,
        class: Option<&'static str>,
        parent: Option<usize>,
        hover: bool,
    }
    impl SelectorElement for El {
        fn tag(&self) -> &str {
            self.tree[self.idx].tag
        }
        fn id(&self) -> Option<&str> {
            self.tree[self.idx].id
        }
        fn has_class(&self, c: &str) -> bool {
            self.tree[self.idx].class.is_some_and(|cl| cl.split_whitespace().any(|x| x == c))
        }
        fn attr(&self, name: &str) -> Option<&str> {
            match name {
                "class" => self.tree[self.idx].class,
                "id" => self.tree[self.idx].id,
                _ => None,
            }
        }
        fn parent_element(&self) -> Option<Self> {
            self.tree[self.idx].parent.map(|p| El { tree: self.tree.clone(), idx: p })
        }
        fn prev_sibling_element(&self) -> Option<Self> {
            None
        }
        fn next_sibling_element(&self) -> Option<Self> {
            None
        }
        fn has_state(&self, s: ElementState) -> bool {
            s == ElementState::Hover && self.tree[self.idx].hover
        }
        fn is_root(&self) -> bool {
            self.tree[self.idx].parent.is_none()
        }
        fn is_empty(&self) -> bool {
            true
        }
    }

    fn tree(hover: bool) -> (El, El) {
        let t = std::rc::Rc::new(vec![
            N { tag: "div", id: Some("app"), class: None, parent: None, hover: false },
            N { tag: "span", id: None, class: Some("a b"), parent: Some(0), hover },
        ]);
        (El { tree: t.clone(), idx: 0 }, El { tree: t, idx: 1 })
    }

    fn rules(css: &str) -> RuleSet {
        let mut rs = RuleSet::new();
        rs.add_stylesheet(&Stylesheet::parse(css), Origin::Author, &MediaContext::default());
        rs
    }

    fn compute(el: &El, rs: &RuleSet, parent: Option<&ComputedStyle>) -> ComputedStyle {
        compute_style(el, rs, &[], parent, &StyleContext::default(), None)
    }

    #[test]
    fn specificity_beats_order() {
        let (_, span) = tree(false);
        let rs = rules("#app span { color: red } span.a { color: blue } span { color: green }");
        let s = compute(&span, &rs, None);
        assert_eq!(s.color, Color::from_u8(255, 0, 0, 1.0));
    }

    #[test]
    fn later_wins_on_tie_and_important_wins() {
        let (_, span) = tree(false);
        let rs = rules(".a { color: red } .b { color: blue } .a { margin: 1px !important } span { margin: 9px }");
        let s = compute(&span, &rs, None);
        assert_eq!(s.color, Color::from_u8(0, 0, 255, 1.0));
        assert_eq!(s.margin[0], Dim::Px(1.0));
    }

    #[test]
    fn inline_beats_rules_but_not_important() {
        let (_, span) = tree(false);
        let rs = rules("#app span { color: red; width: 10px !important }");
        let inline = crate::parser::parse_declarations("color: blue; width: 99px");
        let s = compute_style(&span, &rs, &inline, None, &StyleContext::default(), None);
        assert_eq!(s.color, Color::from_u8(0, 0, 255, 1.0));
        assert_eq!(s.width, Dim::Px(10.0));
    }

    #[test]
    fn inheritance_and_non_inherited() {
        let (div, span) = tree(false);
        let rs = rules("#app { color: red; padding: 10px }");
        let ds = compute(&div, &rs, None);
        let ss = compute(&span, &rs, Some(&ds));
        assert_eq!(ss.color, ds.color, "color inherits");
        assert_eq!(ss.padding[0], Dim::Px(0.0), "padding does not");
    }

    #[test]
    fn custom_properties_inherit_and_resolve() {
        let (div, span) = tree(false);
        let rs = rules("#app { --gap: 12px; --pad: var(--gap) } span { margin: var(--gap); padding: var(--pad, 1px); width: var(--none, 5px) }");
        let ds = compute(&div, &rs, None);
        let ss = compute(&span, &rs, Some(&ds));
        assert_eq!(ss.margin[0], Dim::Px(12.0));
        assert_eq!(ss.padding[0], Dim::Px(12.0));
        assert_eq!(ss.width, Dim::Px(5.0));
    }

    #[test]
    fn state_pseudo_classes_and_media() {
        let (_, span) = tree(true);
        let mut rs = RuleSet::new();
        let css = "span { color: red } span:hover { color: blue } @media (max-width: 100px) { span { color: green } }";
        rs.add_stylesheet(&Stylesheet::parse(css), Origin::Author, &MediaContext::default());
        assert_eq!(compute(&span, &rs, None).color, Color::from_u8(0, 0, 255, 1.0));
        let (_, span) = tree(false);
        assert_eq!(compute(&span, &rs, None).color, Color::from_u8(255, 0, 0, 1.0));
        let mut small = RuleSet::new();
        small.add_stylesheet(&Stylesheet::parse(css), Origin::Author, &MediaContext { width: 50.0, ..Default::default() });
        assert_eq!(compute(&span, &small, None).color, Color::from_u8(0, 128, 0, 1.0));
    }

    #[test]
    fn user_agent_loses_to_author() {
        let (_, span) = tree(false);
        let mut rs = RuleSet::new();
        rs.add_stylesheet(&Stylesheet::parse("span { display: block; color: red }"), Origin::UserAgent, &MediaContext::default());
        rs.add_stylesheet(&Stylesheet::parse("span { color: blue }"), Origin::Author, &MediaContext::default());
        let s = compute(&span, &rs, None);
        assert_eq!(s.display, Display::Block);
        assert_eq!(s.color, Color::from_u8(0, 0, 255, 1.0));
    }

    #[test]
    fn absolute_blockifies() {
        let (_, span) = tree(false);
        let rs = rules("span { position: absolute }");
        assert_eq!(compute(&span, &rs, None).display, Display::Block);
    }
}

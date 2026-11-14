//! Style resolution for the DOM: stylesheet collection, restyling, transitions and keyframe animations.

use crate::Engine;
use rendrix_css::animation::{interpolate_property, property_differs, sample_animation, ANIMATABLE};
use rendrix_css::{compute_style, ComputedStyle, MediaContext, Origin, PseudoElement, RuleSet, StyleContext, Stylesheet, TimingFunction};
use rendrix_dom::{ElRef, NodeId, NodeKind};
use std::sync::Arc;

pub struct ActiveTransition {
    pub prop: &'static str,
    /// The effective (possibly mid-transition) style when the transition started.
    pub from: Arc<ComputedStyle>,
    pub start: f64,
    pub duration: f32,
    pub delay: f32,
    pub timing: TimingFunction,
}

pub struct ActiveAnimation {
    pub name: String,
    pub spec_index: usize,
    pub start: f64,
}

#[derive(Default)]
pub struct AnimState {
    pub transitions: Vec<ActiveTransition>,
    pub animations: Vec<ActiveAnimation>,
}

impl Engine {
    pub fn media_context(&self) -> MediaContext {
        MediaContext { width: self.viewport.0, height: self.viewport.1, dpr: self.scale, dark: self.dark, reduced_motion: false }
    }

    pub fn style_context(&self) -> StyleContext {
        StyleContext { viewport_w: self.viewport.0, viewport_h: self.viewport.1, root_font_size: 16.0 }
    }

    /// Re-collect `<style>` and `<link rel=stylesheet>` sheets when they or the media context changed.
    /// Returns true when the rule set was rebuilt (all styles are then stale).
    pub(crate) fn refresh_rules(&mut self) -> bool {
        let media = self.media_context();
        let media_sig = (media.width.to_bits(), media.height.to_bits(), media.dpr.to_bits(), media.dark);
        if !self.doc.sheets_dirty && !self.rules_dirty && self.rules_media_sig == Some(media_sig) {
            return false;
        }
        self.doc.sheets_dirty = false;
        self.rules_dirty = false;
        self.rules_media_sig = Some(media_sig);
        let mut rules = RuleSet::new();
        let ua = Stylesheet::parse(rendrix_css::ua::USER_AGENT_CSS);
        rules.add_stylesheet(&ua, Origin::UserAgent, &media);
        let root = self.doc.root();
        for n in self.doc.depth_first(root) {
            match self.doc.tag(n) {
                Some("style") => {
                    let css = self.doc.text_content(n);
                    rules.add_stylesheet(&Stylesheet::parse(&css), Origin::Author, &media);
                }
                Some("link") => {
                    let is_sheet = self.doc.get_attr(n, "rel").is_some_and(|r| r.eq_ignore_ascii_case("stylesheet"));
                    if is_sheet {
                        if let Some(href) = self.doc.get_attr(n, "href") {
                            if let Some(bytes) = self.assets.read(href) {
                                let css = String::from_utf8_lossy(&bytes).into_owned();
                                rules.add_stylesheet(&Stylesheet::parse(&css), Origin::Author, &media);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for sheet in &self.adopted_sheets {
            rules.add_stylesheet(&Stylesheet::parse(sheet), Origin::Author, &media);
        }
        // Register @font-face fonts that point at bundled assets.
        let faces = rules.font_faces.clone();
        for f in faces {
            if !self.loaded_fonts.insert(f.src.clone()) {
                continue;
            }
            if let Some(url) = extract_url(&f.src) {
                if let Some(bytes) = self.assets.read(&url) {
                    self.text.borrow_mut().load_font(bytes);
                }
            }
        }
        self.rules = rules;
        true
    }

    /// Recompute styles for dirty subtrees (or everything).
    /// Returns whether any computed style actually changed.
    pub(crate) fn restyle(&mut self, all: bool, roots: Vec<NodeId>, state_roots: Vec<NodeId>) -> bool {
        self.style_changed = all;
        let sc = self.style_context();
        let cap = self.doc_capacity();
        if self.styles.len() < cap {
            self.styles.resize(cap + 64, None);
        }
        let start_nodes: Vec<NodeId> = if all {
            vec![self.doc.root()]
        } else {
            let mut v = roots;
            v.retain(|n| self.doc.is_alive(*n) && self.doc.is_connected(*n));
            // Drop roots nested inside other dirty roots.
            let set: std::collections::HashSet<NodeId> = v.iter().copied().collect();
            v.into_iter()
                .filter(|n| {
                    let mut p = self.doc.parent(*n);
                    while let Some(q) = p {
                        if set.contains(&q) {
                            return false;
                        }
                        p = self.doc.parent(q);
                    }
                    true
                })
                .collect()
        };
        let covered: std::collections::HashSet<NodeId> = start_nodes.iter().flat_map(|n| self.doc.depth_first(*n)).collect();
        for start in start_nodes {
            let parent_style = self.doc.parent(start).and_then(|p| self.styles.get(p as usize).cloned().flatten());
            self.restyle_subtree(start, parent_style, &sc, false);
        }
        // Interaction-state changes: restyle the element itself; descend only where something changed.
        if !all {
            for n in state_roots {
                if covered.contains(&n) || !self.doc.is_alive(n) || self.doc.element(n).is_none() || !self.doc.is_connected(n) {
                    continue;
                }
                let parent_style = self.doc.parent(n).and_then(|p| self.styles.get(p as usize).cloned().flatten());
                self.restyle_subtree(n, parent_style, &sc, !(self.rules.state_deps_nonlocal || self.rules.structural_deps));
            }
        }
        self.style_changed
    }

    fn restyle_subtree(&mut self, id: NodeId, parent_style: Option<Arc<ComputedStyle>>, sc: &StyleContext, shallow: bool) {
        match &self.doc.node(id).kind {
            NodeKind::Document => {
                let kids: Vec<NodeId> = self.doc.children(id).collect();
                for k in kids {
                    self.restyle_subtree(k, None, sc, shallow);
                }
            }
            NodeKind::Element(_) => {
                let new = {
                    let el = ElRef::new(&self.doc, id);
                    let inline = self.doc.element(id).unwrap().inline_style.clone();
                    Arc::new(compute_style(&el, &self.rules, &inline, parent_style.as_deref(), sc, None))
                };
                for (which, key) in [(PseudoElement::Before, true), (PseudoElement::After, false)] {
                    let fresh = self.pseudo_style(id, which, &new, sc);
                    let prev = self.pseudo.get(&(id, key)).cloned();
                    if fresh.as_deref() != prev.as_deref() {
                        self.style_changed = true;
                    }
                    match fresh {
                        Some(s) => {
                            self.pseudo.insert((id, key), s);
                        }
                        None => {
                            self.pseudo.remove(&(id, key));
                        }
                    }
                }
                let old = self.styles[id as usize].take();
                let unchanged = old.as_ref().is_some_and(|o| **o == *new);
                if !unchanged {
                    self.style_changed = true;
                }
                self.update_motion(id, old.as_ref(), &new);
                // Keep the old Arc when nothing changed so cached comparisons stay cheap.
                let kept = if unchanged { old.unwrap() } else { new };
                self.styles[id as usize] = Some(kept.clone());
                let kids: Vec<NodeId> = self.doc.children(id).collect();
                for k in kids {
                    if shallow && unchanged && self.styles.get(k as usize).is_some_and(|s| s.is_some()) {
                        continue;
                    }
                    self.restyle_subtree(k, Some(kept.clone()), sc, shallow);
                }
            }
            _ => {}
        }
    }

    fn pseudo_style(&self, id: NodeId, which: PseudoElement, parent: &Arc<ComputedStyle>, sc: &StyleContext) -> Option<Arc<ComputedStyle>> {
        let el = ElRef::new(&self.doc, id);
        let s = compute_style(&el, &self.rules, &[], Some(parent), sc, Some(which));
        s.content.is_some().then(|| Arc::new(s))
    }

    /// Start transitions / animations implied by the change from `old` to `new`.
    fn update_motion(&mut self, id: NodeId, old: Option<&Arc<ComputedStyle>>, new: &Arc<ComputedStyle>) {
        let has_any = !new.transitions.is_empty() || !new.animations.is_empty() || self.anim.contains_key(&id);
        if !has_any {
            return;
        }
        let now = self.now;
        // Effective style before this change, so interrupted transitions restart from where they were.
        let before_effective = old.map(|o| self.effective_from(id, o, now));
        let has_keyframes: Vec<bool> = new.animations.iter().map(|s| self.rules.keyframes.contains_key(&s.name)).collect();
        let state = self.anim.entry(id).or_default();

        if let (Some(old), Some(before)) = (old, before_effective) {
            for t in &new.transitions {
                if t.duration <= 0.0 && t.delay <= 0.0 {
                    continue;
                }
                for prop in ANIMATABLE {
                    if t.property != "all" && t.property != *prop {
                        continue;
                    }
                    if property_differs(prop, old, new) {
                        state.transitions.retain(|x| x.prop != *prop);
                        state.transitions.push(ActiveTransition { prop, from: before.clone(), start: now, duration: t.duration, delay: t.delay, timing: t.timing.clone() });
                    }
                }
            }
        }
        // Keyframe animations: start new names, drop removed ones.
        state.animations.retain(|a| new.animations.iter().any(|s| s.name == a.name));
        for (i, spec) in new.animations.iter().enumerate() {
            if !state.animations.iter().any(|a| a.name == spec.name) && has_keyframes[i] {
                state.animations.push(ActiveAnimation { name: spec.name.clone(), spec_index: i, start: now });
            }
        }
        for a in state.animations.iter_mut() {
            if let Some(i) = new.animations.iter().position(|s| s.name == a.name) {
                a.spec_index = i;
            }
        }
        if state.transitions.is_empty() && state.animations.is_empty() {
            self.anim.remove(&id);
        }
    }

    /// The style as displayed at time `now` (base style plus transitions and animations).
    pub(crate) fn effective_from(&self, id: NodeId, base: &Arc<ComputedStyle>, now: f64) -> Arc<ComputedStyle> {
        let Some(state) = self.anim.get(&id) else { return base.clone() };
        let mut out = (**base).clone();
        for t in &state.transitions {
            let elapsed = (now - t.start) as f32 - t.delay;
            if elapsed < 0.0 {
                interpolate_property(t.prop, &t.from, base, 0.0, &mut out);
                continue;
            }
            let p = if t.duration <= 0.0 { 1.0 } else { (elapsed / t.duration).clamp(0.0, 1.0) };
            if p < 1.0 {
                interpolate_property(t.prop, &t.from, base, t.timing.eval(p), &mut out);
            }
        }
        let parent_style = self.doc.parent(id).and_then(|p| self.styles.get(p as usize).cloned().flatten());
        let default_parent = ComputedStyle::default();
        let sc = self.style_context();
        for a in &state.animations {
            let (Some(kf), Some(spec)) = (self.rules.keyframes.get(&a.name), base.animations.get(a.spec_index)) else { continue };
            let elapsed = (now - a.start) as f32;
            if let Some(sample) = sample_animation(&out, parent_style.as_deref().unwrap_or(&default_parent), kf, spec, elapsed, &sc) {
                out = sample.style;
            }
        }
        Arc::new(out)
    }

    /// Drop finished motion and report whether anything is still moving.
    pub(crate) fn tick_motion(&mut self, now: f64) -> bool {
        let mut done_nodes = Vec::new();
        let mut any = false;
        for (id, st) in self.anim.iter_mut() {
            st.transitions.retain(|t| (now - t.start) as f32 - t.delay < t.duration);
            let base = self.styles.get(*id as usize).cloned().flatten();
            st.animations.retain(|a| {
                let Some(spec) = base.as_ref().and_then(|b| b.animations.get(a.spec_index)) else { return false };
                let finished = !spec.iterations.is_infinite() && spec.duration > 0.0 && (now - a.start) as f32 - spec.delay >= spec.duration * spec.iterations;
                !finished || matches!(spec.fill, rendrix_css::FillMode::Forwards | rendrix_css::FillMode::Both)
            });
            let running_anim = st.animations.iter().any(|a| {
                base.as_ref().and_then(|b| b.animations.get(a.spec_index)).is_some_and(|s| s.iterations.is_infinite() || (now - a.start) as f32 - s.delay < s.duration * s.iterations)
            });
            if !st.transitions.is_empty() || running_anim {
                any = true;
            }
            if st.transitions.is_empty() && st.animations.is_empty() {
                done_nodes.push(*id);
            }
        }
        for id in done_nodes {
            self.anim.remove(&id);
        }
        any
    }

    /// Effective style of an element at the engine's current time.
    pub fn style_of(&self, id: NodeId) -> Option<Arc<ComputedStyle>> {
        let base = self.styles.get(id as usize)?.as_ref()?;
        if self.anim.contains_key(&id) {
            Some(self.effective_from(id, base, self.now))
        } else {
            Some(base.clone())
        }
    }

    pub fn pseudo_style_of(&self, id: NodeId, before: bool) -> Option<Arc<ComputedStyle>> {
        self.pseudo.get(&(id, before)).cloned()
    }

    pub(crate) fn doc_capacity(&self) -> usize {
        self.doc.capacity()
    }
}

fn extract_url(src: &str) -> Option<String> {
    let start = src.find("url(")? + 4;
    let rest = &src[start..];
    let end = rest.find(')')?;
    Some(rest[..end].trim().trim_matches(|c| c == '"' || c == '\'').to_string())
}

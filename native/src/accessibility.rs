//! Accessibility metadata for the native renderer.
//!
//! GPUI 0.2.2 does not expose an accessibility tree API: there are no role,
//! label, state, or live-region primitives on elements or windows. The HTML
//! `aria-*` and `role` attributes are therefore parsed and carried as metadata
//! only. They are not announced by assistive technology in the native renderer;
//! the WebView renderer keeps the platform browser's accessibility behavior.
//!
//! TODO(accessibility): GPUI 0.2.2 has no accessibility surface. When a future
//! GPUI version exposes role/label/value/state APIs, map [`AriaMetadata`] and
//! [`implicit_role`] onto them here and call the mapping from `rendering.rs`.
//! Until then [`apply`] is intentionally a no-op so callers can be wired up
//! without changing behavior.

use crate::package::Node;

/// Accessibility metadata extracted from an element's attributes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AriaMetadata {
    pub role: Option<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub hidden: bool,
    pub live: Option<String>,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub value: Option<String>,
}

impl AriaMetadata {
    pub fn from_node(node: &Node) -> Self {
        Self {
            role: node.attrs.get("role").cloned(),
            label: node.attrs.get("aria-label").cloned(),
            description: node
                .attrs
                .get("aria-describedby")
                .cloned()
                .or_else(|| node.attrs.get("aria-description").cloned()),
            hidden: node
                .attrs
                .get("aria-hidden")
                .map(|value| value.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            live: node.attrs.get("aria-live").cloned(),
            checked: node
                .attrs
                .get("aria-checked")
                .and_then(|value| match value.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                }),
            disabled: node.attrs.contains_key("disabled")
                || node
                    .attrs
                    .get("aria-disabled")
                    .map(|value| value.eq_ignore_ascii_case("true"))
                    .unwrap_or(false),
            value: node.attrs.get("aria-valuenow").cloned(),
        }
    }

    /// The effective role: an explicit `role` attribute, or the implicit role for
    /// the element's tag when the author did not provide one.
    pub fn effective_role(&self, node: &Node) -> Option<String> {
        self.role
            .clone()
            .or_else(|| implicit_role(&node.tag, node.attrs.get("type").map(String::as_str)).map(str::to_string))
    }

    /// A human-readable label, falling back to `title` and `alt` when no
    /// `aria-label` was authored.
    pub fn effective_label(&self, node: &Node) -> Option<String> {
        self.label
            .clone()
            .or_else(|| node.attrs.get("title").cloned())
            .or_else(|| node.attrs.get("alt").cloned())
    }
}

/// Implicit ARIA role for a tag, following the common HTML mapping.
pub fn implicit_role(tag: &str, input_type: Option<&str>) -> Option<&'static str> {
    match tag {
        "a" => Some("link"),
        "article" => Some("article"),
        "aside" => Some("complementary"),
        "button" => Some("button"),
        "footer" => Some("contentinfo"),
        "form" => Some("form"),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some("heading"),
        "header" => Some("banner"),
        "img" => Some("img"),
        "input" => match input_type {
            Some("button" | "submit" | "reset") => Some("button"),
            Some("checkbox") => Some("checkbox"),
            Some("radio") => Some("radio"),
            Some("range") => Some("slider"),
            Some("search") => Some("searchbox"),
            _ => Some("textbox"),
        },
        "li" => Some("listitem"),
        "main" => Some("main"),
        "nav" => Some("navigation"),
        "ol" | "ul" => Some("list"),
        "option" => Some("option"),
        "select" => Some("combobox"),
        "section" => Some("region"),
        "table" => Some("table"),
        "td" => Some("cell"),
        "textarea" => Some("textbox"),
        "th" => Some("columnheader"),
        "tr" => Some("row"),
        _ => None,
    }
}

/// No-op placeholder for the future native mapping. Kept so rendering code can
/// call it today without changing behavior.
pub fn apply(_metadata: &AriaMetadata) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::Node;
    use std::collections::BTreeMap;

    fn node(tag: &str, attrs: &[(&str, &str)]) -> Node {
        Node {
            key: 1,
            tag: tag.to_string(),
            text: String::new(),
            attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            style: BTreeMap::new(),
            states: BTreeMap::new(),
            props: BTreeMap::new(),
            children: Vec::new(),
            responsive: Vec::new(),
        }
    }

    #[test]
    fn extracts_aria_attributes() {
        let node = node("button", &[("aria-label", "Save"), ("aria-hidden", "true"), ("disabled", "")]);
        let meta = AriaMetadata::from_node(&node);
        assert_eq!(meta.label.as_deref(), Some("Save"));
        assert!(meta.hidden);
        assert!(meta.disabled);
        assert_eq!(meta.effective_role(&node).as_deref(), Some("button"));
    }

    #[test]
    fn implicit_roles_cover_common_controls() {
        assert_eq!(implicit_role("input", Some("checkbox")), Some("checkbox"));
        assert_eq!(implicit_role("nav", None), Some("navigation"));
        assert_eq!(implicit_role("div", None), None);
    }

    #[test]
    fn effective_label_falls_back_to_title_and_alt() {
        let node = node("img", &[("title", "Logo"), ("alt", "A logo")]);
        let meta = AriaMetadata::from_node(&node);
        assert_eq!(meta.effective_label(&node).as_deref(), Some("Logo"));
    }
}

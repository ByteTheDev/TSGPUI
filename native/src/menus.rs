use gpui::{Action, App, Menu, MenuItem, SharedString};

use crate::package::Node;

#[derive(Clone, PartialEq, Action)]
#[action(name = "TSGPUIAction", no_json)]
pub struct TSGPUIAction {
    pub action: String,
}

fn label(node: &Node) -> String {
    node.children.iter().filter(|child| child.tag == "#text")
        .map(|child| child.text.as_str()).collect::<Vec<_>>().join(" ").trim().to_string()
}

fn menu_items(node: &Node) -> Vec<MenuItem> {
    node.children.iter().filter_map(|child| match child.tag.as_str() {
        "hr" => Some(MenuItem::separator()),
        "menuitem" => {
            if child.attrs.contains_key("disabled") { return None; }
            let title: SharedString = label(child).into();
            if child.children.iter().any(|grandchild| grandchild.tag == "menuitem") {
                Some(MenuItem::submenu(Menu { name: title, items: menu_items(child) }))
            } else {
                child.attrs.get("data-onclick").map(|action| MenuItem::action(
                    title,
                    TSGPUIAction { action: action.clone() },
                ))
            }
        }
        _ => None,
    }).collect()
}

fn collect(node: &Node, menus: &mut Vec<Menu>) {
    if let Some(name) = node.attrs.get("data-app-menu") {
        menus.push(Menu { name: name.clone().into(), items: menu_items(node) });
    }
    for child in &node.children { collect(child, menus); }
}

pub fn set_application_menus(root: &Node, cx: &mut App) {
    let mut menus = Vec::new();
    collect(root, &mut menus);
    if !menus.is_empty() { cx.set_menus(menus); }
}

pub fn context_menu<'a>(root: &'a Node, id: &str) -> Option<&'a Node> {
    if root.tag == "menu" && root.attrs.get("id").is_some_and(|menu_id| menu_id == id) { return Some(root); }
    root.children.iter().find_map(|child| context_menu(child, id))
}

pub fn text_label(node: &Node) -> String { label(node) }

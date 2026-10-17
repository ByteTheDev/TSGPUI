use gpui::{App,AnyElement,Application,Bounds,Context,FontWeight,Window,WindowBounds,WindowOptions,div,prelude::*,px,rgb,size};
use crate::{package::{AppPackage,Node},runtime::JsBackend};

pub struct AppView {
    document: AppPackage,
    backend: JsBackend,
}

fn px_value(style: &std::collections::BTreeMap<String,String>, key:&str)->Option<f32>{
    style.get(key)?.strip_suffix("px")?.parse::<f32>().ok()
}
fn hex_color(s:&str)->Option<u32>{
    let value=s.strip_prefix('#')?;
    if value.len()==6 {u32::from_str_radix(value,16).ok()}
    else if value.len()==3 {let chars:Vec<char>=value.chars().collect();let full=format!("{0}{0}{1}{1}{2}{2}",chars[0],chars[1],chars[2]);u32::from_str_radix(&full,16).ok()}
    else {None}
}
fn text_value(text:&str,state:&serde_json::Value)->String{
    // Literal text interpolation: "Count: {{count}}".
    let mut rendered=String::new();
    let mut remaining=text;
    while let Some(start)=remaining.find("{{") {
        rendered.push_str(&remaining[..start]);
        let after=&remaining[start+2..];
        if let Some(end)=after.find("}}") {
            let key=after[..end].trim();
            let val=state.get(key);
            let printable=match val {
                Some(serde_json::Value::String(s))=>s.clone(),
                Some(serde_json::Value::Null)|None=>String::new(),
                Some(v)=>v.to_string(),
            };
            rendered.push_str(&printable);
            remaining=&after[end+2..];
        } else {rendered.push_str(&remaining[start..]);return rendered;}
    }
    rendered.push_str(remaining);
    rendered
}

impl AppView {
    pub fn new(document:AppPackage,backend:JsBackend)->Self{Self{document,backend}}

    fn render_node(node:&Node,state:&serde_json::Value,cx:&mut Context<Self>)->AnyElement{
        let style=&node.style;
        let mut element=div().id(("tsgpui", node.key)).flex().flex_col();
        if node.tag=="body" {element=element.size_full();}
        if let Some(color)=style.get("background").and_then(|s|hex_color(s)) {element=element.bg(rgb(color));}
        if let Some(color)=style.get("color").and_then(|s|hex_color(s)) {element=element.text_color(rgb(color));}
        if let Some(v)=px_value(style,"width") {element=element.w(px(v));}
        if let Some(v)=px_value(style,"height") {element=element.h(px(v));}
        if let Some(v)=px_value(style,"padding") {element=element.p(px(v));}
        if let Some(v)=px_value(style,"margin") {element=element.m(px(v));}
        if let Some(v)=px_value(style,"gap") {element=element.gap(px(v));}
        if let Some(v)=px_value(style,"border-radius") {element=element.rounded(px(v));}
        if let Some(v)=px_value(style,"font-size") {element=element.text_size(px(v));}
        if let Some(weight)=style.get("font-weight") {
            let fw=match weight.as_str(){"bold"|"700"=>FontWeight::BOLD,"600"=>FontWeight::SEMIBOLD,"500"=>FontWeight::MEDIUM,_=>FontWeight::NORMAL};
            element=element.font_weight(fw);
        }
        if style.get("flex-direction").is_some_and(|x|x=="row") {element=element.flex_row();}
        match style.get("align-items").map(String::as_str) {
            Some("center")=>element=element.items_center(),
            Some("flex-start")=>element=element.items_start(),
            Some("flex-end")=>element=element.items_end(),
            Some("stretch")=>{}, // flex default: GPUI has no items_stretch in 0.2.2
            _=>{}
        }
        match style.get("justify-content").map(String::as_str) {
            Some("center")=>element=element.justify_center(),
            Some("flex-start")=>element=element.justify_start(),
            Some("flex-end")=>element=element.justify_end(),
            Some("space-between")=>element=element.justify_between(),
            Some("space-around")=>element=element.justify_around(),
            _=>{}
        }
        if node.tag=="button" {
            element=element.cursor_pointer();
            if let Some(action)=node.attrs.get("data-onclick") {
                let action=action.clone();
                element=element.on_click(cx.listener(move |this,_,_,cx| {
                    match this.backend.invoke(&action) {
                        Ok(())=>cx.notify(),
                        Err(e)=>eprintln!("TSGPUI: {e}"),
                    }
                }));
            }
        }
        if let Some(binding)=node.attrs.get("data-bind") {
            let rendered=state.get(binding).map(|value| match value {
                serde_json::Value::String(s)=>s.clone(),
                serde_json::Value::Null=>String::new(),
                other=>other.to_string(),
            }).unwrap_or_default();
            element=element.child(rendered);
        } else if node.tag=="#text" {
            element=element.child(text_value(&node.text,state));
        } else {
            for child in &node.children {
                element=element.child(Self::render_node(child,state,cx));
            }
        }
        element.into_any_element()
    }
}
impl Render for AppView {
    fn render(&mut self,_window:&mut Window,cx:&mut Context<Self>)->impl IntoElement {
        Self::render_node(&self.document.root,self.backend.state(),cx)
    }
}

pub fn run(doc:AppPackage, backend:JsBackend) {
    Application::new().run(move |cx:&mut App| {
        let bounds=Bounds::centered(None,size(px(doc.width as f32),px(doc.height as f32)),cx);
        cx.open_window(WindowOptions{
            window_bounds:Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },move |_,cx| cx.new(|_|AppView::new(doc,backend)))
            .expect("Failed to open native GPUI window");
        cx.activate(true);
    });
}

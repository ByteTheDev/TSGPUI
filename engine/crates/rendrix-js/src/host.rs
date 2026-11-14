//! Native operations behind `__native(op, ...args)`: DOM access, timers, storage, networking, console, window control.

use crate::engine_trait::{Host, Val};
use rendrix_core::Engine;
use rendrix_dom::{MutationKind, NodeId};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct Permissions {
    pub network: bool,
    /// Directories scripts may read from / write to. Empty = no access.
    pub fs_read: Vec<PathBuf>,
    pub fs_write: Vec<PathBuf>,
}

impl Permissions {
    pub fn allow_all() -> Permissions {
        Permissions { network: true, fs_read: vec![PathBuf::from("/")], fs_write: vec![] }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WindowCommand {
    SetTitle(String),
    Close,
    Resize(f32, f32),
    Quit,
}

pub struct FetchResult {
    pub id: u32,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Storage {
    local: BTreeMap<String, String>,
    session: BTreeMap<String, String>,
    path: Option<PathBuf>,
}

impl Storage {
    fn area(&mut self, name: &str) -> &mut BTreeMap<String, String> {
        if name == "session" {
            &mut self.session
        } else {
            &mut self.local
        }
    }
    fn persist(&self) {
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, serde_json::to_string(&self.local).unwrap_or_default());
        }
    }
}

pub struct DomHost {
    pub engine: Rc<RefCell<Engine>>,
    start: Instant,
    pub timers: BTreeMap<u32, (f64, Option<f64>)>,
    pub raf_requested: bool,
    pub console: Vec<(String, String)>,
    pub echo_console: bool,
    storage: Storage,
    fetch_tx: Sender<FetchResult>,
    fetch_rx: Receiver<FetchResult>,
    pub pending_fetches: usize,
    pub immediate_fetches: Vec<FetchResult>,
    pub permissions: Permissions,
    pub window_commands: Vec<WindowCommand>,
    pub clipboard: String,
    pub quit_requested: bool,
    pub app_name: String,
}

pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn id(args: &[Val], i: usize) -> Result<NodeId, String> {
    args.get(i).and_then(|v| v.as_f64()).map(|n| n as NodeId).ok_or_else(|| format!("argument {i} must be a node id"))
}

fn opt_id(args: &[Val], i: usize) -> Option<NodeId> {
    match args.get(i) {
        Some(Val::Num(n)) => Some(*n as NodeId),
        _ => None,
    }
}

fn s(args: &[Val], i: usize) -> String {
    match args.get(i) {
        Some(Val::Str(s)) => s.clone(),
        Some(Val::Num(n)) => n.to_string(),
        Some(Val::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn num(args: &[Val], i: usize) -> f64 {
    args.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

fn ids(v: Vec<NodeId>) -> Val {
    Val::List(v.into_iter().map(|i| Val::Num(i as f64)).collect())
}

fn rect_val(r: Option<[f32; 4]>) -> Val {
    match r {
        Some(r) => Val::List(r.iter().map(|v| Val::Num(*v as f64)).collect()),
        None => Val::Null,
    }
}

impl DomHost {
    pub fn new(engine: Rc<RefCell<Engine>>, app_name: &str, permissions: Permissions) -> DomHost {
        let (fetch_tx, fetch_rx) = channel();
        let mut storage = Storage::default();
        // localStorage lives in the per-user data directory, keyed by app name.
        let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_DATA_HOME")).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
        if let Some(base) = base {
            let safe: String = app_name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let p = base.join("Rendrix").join(safe).join("localStorage.json");
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(m) = serde_json::from_str(&text) {
                    storage.local = m;
                }
            }
            storage.path = Some(p);
        }
        DomHost {
            engine,
            start: Instant::now(),
            timers: BTreeMap::new(),
            raf_requested: false,
            console: Vec::new(),
            echo_console: true,
            storage,
            fetch_tx,
            fetch_rx,
            pending_fetches: 0,
            immediate_fetches: Vec::new(),
            permissions,
            window_commands: Vec::new(),
            clipboard: String::new(),
            quit_requested: false,
            app_name: app_name.to_string(),
        }
    }

    /// Disable persistence of localStorage (tests).
    pub fn disable_storage_persistence(&mut self) {
        self.storage.path = None;
    }

    pub fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    pub fn take_console(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.console)
    }

    /// Timers whose deadline has passed; one-shot timers are removed, intervals re-armed.
    pub fn take_due_timers(&mut self, now_ms: f64) -> Vec<u32> {
        let due: Vec<u32> = self.timers.iter().filter(|(_, (t, _))| *t <= now_ms).map(|(i, _)| *i).collect();
        for i in &due {
            if let Some((t, interval)) = self.timers.get(i).copied() {
                match interval {
                    Some(iv) => {
                        self.timers.insert(*i, ((t + iv).max(now_ms + 1.0), Some(iv)));
                    }
                    None => {
                        self.timers.remove(i);
                    }
                }
            }
        }
        due
    }

    pub fn next_timer_ms(&self) -> Option<f64> {
        self.timers.values().map(|(t, _)| *t).fold(None, |a: Option<f64>, t| Some(a.map_or(t, |x| x.min(t))))
    }

    pub fn poll_fetches(&mut self) -> Vec<FetchResult> {
        let mut out = std::mem::take(&mut self.immediate_fetches);
        while let Ok(r) = self.fetch_rx.try_recv() {
            self.pending_fetches = self.pending_fetches.saturating_sub(1);
            out.push(r);
        }
        out
    }

    /// A uniform f64 in [0, 1) from the operating system's CSPRNG (backs `crypto.getRandomValues`).
    fn rand(&mut self) -> f64 {
        let mut buf = [0u8; 8];
        if getrandom::getrandom(&mut buf).is_err() {
            // Never fall back to a predictable source for crypto: fail loudly instead.
            panic!("the operating system random number generator is unavailable");
        }
        (u64::from_le_bytes(buf) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn start_fetch(&mut self, fid: u32, url: String, method: String, headers: Vec<(String, String)>, body: Option<Vec<u8>>) {
        let done = |id, status: u16, text: &str, headers: Vec<(String, String)>, body: Vec<u8>| FetchResult { id, status, status_text: text.to_string(), headers, body, error: None };
        let fail = |id, msg: String| FetchResult { id, status: 0, status_text: String::new(), headers: vec![], body: vec![], error: Some(msg) };
        if let Some(rest) = url.strip_prefix("data:") {
            let r = match rest.split_once(',') {
                Some((meta, data)) => {
                    let bytes = if meta.ends_with(";base64") { base64_decode(data) } else { Some(percent_decode(data)) };
                    match bytes {
                        Some(b) => done(fid, 200, "OK", vec![("content-type".into(), meta.trim_end_matches(";base64").to_string())], b),
                        None => fail(fid, "invalid data: URL".into()),
                    }
                }
                None => fail(fid, "invalid data: URL".into()),
            };
            self.immediate_fetches.push(r);
            return;
        }
        if !url.contains("://") {
            // Relative URL → bundled asset.
            let bytes = self.engine.borrow().read_asset(&url);
            let r = match bytes {
                Some(b) => done(fid, 200, "OK", vec![("content-type".into(), guess_mime(&url).into())], b),
                None => done(fid, 404, "Not Found", vec![], vec![]),
            };
            self.immediate_fetches.push(r);
            return;
        }
        if !self.permissions.network {
            self.immediate_fetches.push(fail(fid, "network access is not permitted for this app".into()));
            return;
        }
        self.pending_fetches += 1;
        let tx = self.fetch_tx.clone();
        std::thread::spawn(move || {
            let mut req = ureq::request(&method, &url).timeout(std::time::Duration::from_secs(60));
            for (k, v) in &headers {
                req = req.set(k, v);
            }
            let resp = match body {
                Some(b) => req.send_bytes(&b),
                None => req.call(),
            };
            let result = match resp {
                Ok(r) | Err(ureq::Error::Status(_, r)) => {
                    let status = r.status();
                    let text = r.status_text().to_string();
                    let hdrs: Vec<(String, String)> = r.headers_names().into_iter().filter_map(|n| r.header(&n).map(|v| (n, v.to_string()))).collect();
                    let mut buf = Vec::new();
                    use std::io::Read;
                    match r.into_reader().take(64 * 1024 * 1024).read_to_end(&mut buf) {
                        Ok(_) => FetchResult { id: fid, status, status_text: text, headers: hdrs, body: buf, error: None },
                        Err(e) => FetchResult { id: fid, status: 0, status_text: String::new(), headers: vec![], body: vec![], error: Some(e.to_string()) },
                    }
                }
                Err(e) => FetchResult { id: fid, status: 0, status_text: String::new(), headers: vec![], body: vec![], error: Some(e.to_string()) },
            };
            let _ = tx.send(result);
        });
    }

    fn fs_allowed(list: &[PathBuf], path: &str) -> Option<PathBuf> {
        let p = std::fs::canonicalize(path).ok().or_else(|| {
            // For writes to new files: canonicalise the parent.
            let pb = PathBuf::from(path);
            pb.parent().and_then(|d| std::fs::canonicalize(d).ok()).map(|d| d.join(pb.file_name().unwrap_or_default()))
        })?;
        list.iter().any(|root| std::fs::canonicalize(root).map(|r| p.starts_with(r)).unwrap_or(false)).then_some(p)
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn guess_mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "txt" => "text/plain",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

impl Host for DomHost {
    fn call(&mut self, op: &str, a: &[Val]) -> Result<Val, String> {
        let engine = self.engine.clone();
        // Most ops touch the DOM; borrow lazily per arm to keep geometry ops able to run layout.
        macro_rules! doc {
            () => {
                engine.borrow_mut()
            };
        }
        Ok(match op {
            // ---- creation ----
            "create_element" => Val::Num(doc!().doc.create_element(&s(a, 0)) as f64),
            "create_text" => Val::Num(doc!().doc.create_text(&s(a, 0)) as f64),
            "create_comment" => Val::Num(doc!().doc.create_comment(&s(a, 0)) as f64),
            "create_fragment" => Val::Num(doc!().doc.create_fragment() as f64),
            "clone_node" => Val::Num(doc!().doc.clone_node(id(a, 0)?, a.get(1).is_some_and(|v| v.as_bool())) as f64),
            // ---- tree ----
            "append_child" => {
                doc!().doc.append_child(id(a, 0)?, id(a, 1)?).map_err(|e| e.to_string())?;
                Val::Undefined
            }
            "insert_before" => {
                doc!().doc.insert_before(id(a, 0)?, id(a, 1)?, opt_id(a, 2)).map_err(|e| e.to_string())?;
                Val::Undefined
            }
            "remove_child" => {
                doc!().doc.remove_child(id(a, 0)?, id(a, 1)?).map_err(|e| e.to_string())?;
                Val::Undefined
            }
            "replace_child" => {
                doc!().doc.replace_child(id(a, 0)?, id(a, 1)?, id(a, 2)?).map_err(|e| e.to_string())?;
                Val::Undefined
            }
            "parent" => Val::from_opt_id(doc!().doc.parent(id(a, 0)?)),
            "first_child" => Val::from_opt_id(doc!().doc.first_child(id(a, 0)?)),
            "last_child" => Val::from_opt_id(doc!().doc.last_child(id(a, 0)?)),
            "next_sibling" => Val::from_opt_id(doc!().doc.next_sibling(id(a, 0)?)),
            "prev_sibling" => Val::from_opt_id(doc!().doc.prev_sibling(id(a, 0)?)),
            "children" => {
                let e = doc!();
                ids(e.doc.children(id(a, 0)?).collect())
            }
            "contains" => Val::Bool(doc!().doc.contains(id(a, 0)?, id(a, 1)?)),
            "is_connected" => Val::Bool(doc!().doc.is_connected(id(a, 0)?)),
            "event_path" => ids(doc!().doc.event_path(id(a, 0)?)),
            "document_element" => Val::from_opt_id(doc!().doc.document_element()),
            "head" => Val::from_opt_id(doc!().doc.head()),
            "body" => Val::from_opt_id(doc!().doc.body()),
            "document_root" => Val::Num(doc!().doc.root() as f64),
            // ---- node info ----
            "node_type" => Val::Num(doc!().doc.node_type(id(a, 0)?) as u8 as f64),
            "tag" => doc!().doc.tag(id(a, 0)?).map(Val::from).unwrap_or(Val::Null),
            "text_data" => doc!().doc.text(id(a, 0)?).map(Val::from).unwrap_or(Val::Null),
            "set_text_data" => {
                doc!().doc.set_text(id(a, 0)?, &s(a, 1));
                Val::Undefined
            }
            "text_content" => Val::Str(doc!().doc.text_content(id(a, 0)?)),
            "set_text_content" => {
                doc!().doc.set_text_content(id(a, 0)?, &s(a, 1));
                Val::Undefined
            }
            "inner_html" => Val::Str(doc!().doc.inner_html(id(a, 0)?)),
            "outer_html" => Val::Str(doc!().doc.outer_html(id(a, 0)?)),
            "set_inner_html" => {
                doc!().doc.set_inner_html(id(a, 0)?, &s(a, 1));
                Val::Undefined
            }
            "parse_fragment" => Val::Num(doc!().doc.parse_fragment(&s(a, 0)) as f64),
            // ---- attributes ----
            "get_attr" => doc!().doc.get_attr(id(a, 0)?, &s(a, 1)).map(Val::from).unwrap_or(Val::Null),
            "set_attr" => {
                doc!().doc.set_attr(id(a, 0)?, &s(a, 1), &s(a, 2));
                Val::Undefined
            }
            "remove_attr" => {
                doc!().doc.remove_attr(id(a, 0)?, &s(a, 1));
                Val::Undefined
            }
            "attrs" => {
                let e = doc!();
                match e.doc.element(id(a, 0)?) {
                    Some(el) => Val::List(el.attrs.iter().flat_map(|(k, v)| [Val::Str(k.clone()), Val::Str(v.clone())]).collect()),
                    None => Val::List(vec![]),
                }
            }
            // ---- queries ----
            "query" => {
                let e = doc!();
                Val::from_opt_id(e.doc.query_selector(id(a, 0)?, &s(a, 1)).map_err(|e| e.to_string())?)
            }
            "query_all" => {
                let e = doc!();
                ids(e.doc.query_selector_all(id(a, 0)?, &s(a, 1)).map_err(|e| e.to_string())?)
            }
            "matches" => Val::Bool(doc!().doc.matches(id(a, 0)?, &s(a, 1)).map_err(|e| e.to_string())?),
            "closest" => Val::from_opt_id(doc!().doc.closest(id(a, 0)?, &s(a, 1)).map_err(|e| e.to_string())?),
            "by_id" => Val::from_opt_id(doc!().doc.get_element_by_id(&s(a, 0))),
            "by_tag" => ids(doc!().doc.get_elements_by_tag_name(id(a, 0)?, &s(a, 1))),
            "by_class" => ids(doc!().doc.get_elements_by_class_name(id(a, 0)?, &s(a, 1))),
            // ---- form state ----
            "value" => Val::Str(doc!().doc.value(id(a, 0)?)),
            "set_value" => {
                doc!().doc.set_form_value(id(a, 0)?, &s(a, 1));
                Val::Undefined
            }
            "checked" => Val::Bool(doc!().doc.is_checked(id(a, 0)?)),
            "set_checked" => {
                doc!().doc.set_checked(id(a, 0)?, a.get(1).is_some_and(|v| v.as_bool()));
                Val::Undefined
            }
            "selection" => {
                let e = doc!();
                match e.doc.element(id(a, 0)?) {
                    Some(el) => {
                        let (x, y) = (el.form.selection_start, el.form.selection_end);
                        let v = e.doc.value(id(a, 0)?);
                        // UTF-16 offsets for JS.
                        let u16 = |b: usize| v[..b.min(v.len())].encode_utf16().count() as f64;
                        Val::List(vec![Val::Num(u16(x.min(y))), Val::Num(u16(x.max(y)))])
                    }
                    None => Val::Null,
                }
            }
            "set_selection" => {
                let mut e = doc!();
                let v = e.doc.value(id(a, 0)?);
                let to_byte = |u: usize| {
                    let mut count = 0;
                    for (b, c) in v.char_indices() {
                        if count >= u {
                            return b;
                        }
                        count += c.len_utf16();
                    }
                    v.len()
                };
                let (x, y) = (to_byte(num(a, 1) as usize), to_byte(num(a, 2) as usize));
                e.doc.set_selection(id(a, 0)?, x, y);
                e.reset_caret();
                Val::Undefined
            }
            // ---- focus ----
            "focus" => {
                let mut e = doc!();
                let n = id(a, 0)?;
                let prev = e.doc.focused();
                if e.doc.is_focusable(n) {
                    e.doc.set_focus(Some(n));
                    e.reset_caret();
                }
                Val::from_opt_id(prev)
            }
            "blur" => {
                let mut e = doc!();
                let n = id(a, 0)?;
                if e.doc.focused() == Some(n) {
                    e.doc.set_focus(None);
                }
                Val::Undefined
            }
            "focused" => Val::from_opt_id(doc!().doc.focused()),
            "is_focusable" => Val::Bool(doc!().doc.is_focusable(id(a, 0)?)),
            "is_text_control" => Val::Bool(doc!().doc.is_text_control(id(a, 0)?)),
            // ---- geometry & style ----
            "rect" => {
                let mut e = doc!();
                e.set_time(self.start.elapsed().as_secs_f64());
                e.ensure_layout();
                rect_val(e.element_rect(id(a, 0)?))
            }
            "layout_size" => {
                let mut e = doc!();
                e.set_time(self.start.elapsed().as_secs_f64());
                e.ensure_layout();
                match e.layout_size(id(a, 0)?) {
                    Some((w, h)) => Val::List(vec![Val::Num(w as f64), Val::Num(h as f64)]),
                    None => Val::Null,
                }
            }
            "scroll_metrics" => {
                let mut e = doc!();
                e.set_time(self.start.elapsed().as_secs_f64());
                e.ensure_layout();
                match e.scroll_metrics(id(a, 0)?) {
                    Some((sw, sh, cw, ch)) => Val::List([sw, sh, cw, ch].iter().map(|v| Val::Num(*v as f64)).collect()),
                    None => Val::Null,
                }
            }
            "get_scroll" => {
                let mut e = doc!();
                e.set_time(self.start.elapsed().as_secs_f64());
                e.ensure_layout();
                let (x, y) = e.get_scroll(id(a, 0)?);
                Val::List(vec![Val::Num(x as f64), Val::Num(y as f64)])
            }
            "set_scroll" => {
                let mut e = doc!();
                e.ensure_layout();
                let n = id(a, 0)?;
                let (cx, cy) = e.get_scroll(n);
                let x = if matches!(a.get(1), Some(Val::Num(_))) { num(a, 1) as f32 } else { cx };
                let y = if matches!(a.get(2), Some(Val::Num(_))) { num(a, 2) as f32 } else { cy };
                e.set_scroll(n, x, y);
                Val::Undefined
            }
            "computed" => {
                let mut e = doc!();
                e.set_time(self.start.elapsed().as_secs_f64());
                e.ensure_layout();
                Val::Str(e.computed_value(id(a, 0)?, &s(a, 1)))
            }
            "hit_test" => {
                let mut e = doc!();
                e.set_time(self.start.elapsed().as_secs_f64());
                e.ensure_layout();
                Val::from_opt_id(e.hit_test(num(a, 0) as f32, num(a, 1) as f32))
            }
            "viewport" => {
                let e = doc!();
                Val::List(vec![Val::Num(e.viewport.0 as f64), Val::Num(e.viewport.1 as f64), Val::Num(e.scale as f64)])
            }
            "media_matches" => {
                let e = doc!();
                let q = rendrix_css::parser::MediaQueryList::parse(&s(a, 0));
                Val::Bool(q.matches(&e.media_context()))
            }
            "image_info" => match doc!().image_entry(&s(a, 0)) {
                Some(i) => Val::List(vec![Val::Num(i.width as f64), Val::Num(i.height as f64)]),
                None => Val::Null,
            },
            "add_stylesheet" => {
                doc!().add_stylesheet(&s(a, 0));
                Val::Undefined
            }
            // ---- memory ----
            "detach_epoch" => Val::Num(doc!().doc.detach_epoch as f64),
            "take_freed" => {
                let mut e = doc!();
                // Sweep now so script-side state for unreachable nodes is dropped promptly.
                e.doc.collect_garbage();
                let freed = e.doc.take_freed();
                let mut all = e.take_freed_for_js();
                all.extend(freed);
                ids(all)
            }
            "retain" => {
                doc!().doc.retain(id(a, 0)?);
                Val::Undefined
            }
            "release" => {
                doc!().doc.release(id(a, 0)?);
                Val::Undefined
            }
            // ---- mutation observation ----
            "observe_mutations" => {
                doc!().doc.set_record_mutations(a.first().is_some_and(|v| v.as_bool()));
                Val::Undefined
            }
            "take_mutations" => {
                let recs = doc!().doc.take_mutations();
                Val::List(
                    recs.into_iter()
                        .map(|r| {
                            Val::List(vec![
                                Val::Str(match r.kind {
                                    MutationKind::ChildList => "childList",
                                    MutationKind::Attributes => "attributes",
                                    MutationKind::CharacterData => "characterData",
                                }
                                .into()),
                                Val::Num(r.target as f64),
                                ids(r.added),
                                ids(r.removed),
                                Val::from_opt_id(r.previous_sibling),
                                Val::from_opt_id(r.next_sibling),
                                r.attribute_name.map(Val::Str).unwrap_or(Val::Null),
                                r.old_value.map(Val::Str).unwrap_or(Val::Null),
                                ids(r.ancestors),
                            ])
                        })
                        .collect(),
                )
            }
            // ---- time ----
            "now" => Val::Num(self.now_ms()),
            "timer_set" => {
                let (tid, delay) = (num(a, 0) as u32, num(a, 1).max(0.0));
                let repeat = a.get(2).is_some_and(|v| v.as_bool());
                self.timers.insert(tid, (self.now_ms() + delay, repeat.then_some(delay.max(1.0))));
                Val::Undefined
            }
            "timer_clear" => {
                self.timers.remove(&(num(a, 0) as u32));
                Val::Undefined
            }
            "raf_request" => {
                self.raf_requested = true;
                Val::Undefined
            }
            "random" => Val::Num(self.rand()),
            // ---- console ----
            "console" => {
                let (level, msg) = (s(a, 0), s(a, 1));
                if self.echo_console {
                    if matches!(level.as_str(), "error" | "warn") {
                        eprintln!("[{level}] {msg}");
                    } else {
                        println!("[{level}] {msg}");
                    }
                }
                self.console.push((level, msg));
                if self.console.len() > 2000 {
                    self.console.drain(..500);
                }
                Val::Undefined
            }
            // ---- storage ----
            "storage_get" => self.storage.area(&s(a, 0)).get(&s(a, 1)).cloned().map(Val::Str).unwrap_or(Val::Null),
            "storage_set" => {
                let area = s(a, 0);
                self.storage.area(&area).insert(s(a, 1), s(a, 2));
                if area != "session" {
                    self.storage.persist();
                }
                Val::Undefined
            }
            "storage_remove" => {
                let area = s(a, 0);
                self.storage.area(&area).remove(&s(a, 1));
                if area != "session" {
                    self.storage.persist();
                }
                Val::Undefined
            }
            "storage_clear" => {
                let area = s(a, 0);
                self.storage.area(&area).clear();
                if area != "session" {
                    self.storage.persist();
                }
                Val::Undefined
            }
            "storage_keys" => Val::List(self.storage.area(&s(a, 0)).keys().cloned().map(Val::Str).collect()),
            // ---- network ----
            "fetch_start" => {
                let headers: Vec<(String, String)> = match a.get(3) {
                    Some(Val::List(l)) => l.chunks(2).filter_map(|c| Some((c.first()?.as_str()?.to_string(), c.get(1)?.as_str()?.to_string()))).collect(),
                    _ => vec![],
                };
                let body = a.get(4).and_then(|v| v.as_str()).and_then(base64_decode);
                self.start_fetch(num(a, 0) as u32, s(a, 1), s(a, 2).to_uppercase(), headers, body);
                Val::Undefined
            }
            "base64_encode_str" => Val::Str(base64_encode(s(a, 0).as_bytes())),
            // ---- clipboard ----
            "clipboard_read" => Val::Str(self.clipboard.clone()),
            "clipboard_write" => {
                self.clipboard = s(a, 0);
                Val::Undefined
            }
            // ---- window ----
            "window_title" => {
                self.window_commands.push(WindowCommand::SetTitle(s(a, 0)));
                Val::Undefined
            }
            "window_close" => {
                self.window_commands.push(WindowCommand::Close);
                Val::Undefined
            }
            "window_resize" => {
                self.window_commands.push(WindowCommand::Resize(num(a, 0) as f32, num(a, 1) as f32));
                Val::Undefined
            }
            "quit" => {
                self.quit_requested = true;
                self.window_commands.push(WindowCommand::Quit);
                Val::Undefined
            }
            // ---- filesystem (permission-gated) ----
            "fs_read_text" => {
                let p = Self::fs_allowed(&self.permissions.fs_read, &s(a, 0)).ok_or_else(|| "filesystem read is not permitted for this path".to_string())?;
                Val::Str(std::fs::read_to_string(p).map_err(|e| e.to_string())?)
            }
            "fs_write_text" => {
                let p = Self::fs_allowed(&self.permissions.fs_write, &s(a, 0)).ok_or_else(|| "filesystem write is not permitted for this path".to_string())?;
                std::fs::write(p, s(a, 1)).map_err(|e| e.to_string())?;
                Val::Undefined
            }
            "fs_exists" => Val::Bool(Self::fs_allowed(&self.permissions.fs_read, &s(a, 0)).is_some_and(|p| p.exists())),
            "fs_read_dir" => {
                let p = Self::fs_allowed(&self.permissions.fs_read, &s(a, 0)).ok_or_else(|| "filesystem read is not permitted for this path".to_string())?;
                let mut names: Vec<String> = std::fs::read_dir(p).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                names.sort();
                Val::List(names.into_iter().map(Val::Str).collect())
            }
            other => return Err(format!("unknown native operation '{other}'")),
        })
    }
}

#[allow(dead_code)]
type _Unused = HashMap<(), ()>;

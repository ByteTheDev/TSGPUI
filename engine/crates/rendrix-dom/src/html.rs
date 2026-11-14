//! A tolerant HTML parser and the matching serializer.
//!
//! This is not a full HTML5 tree builder (no foster parenting, no adoption agency); it covers the
//! practical subset applications use: void elements, raw-text elements, implied end tags for
//! `p`/`li`/`option`/`dt`/`dd`, head/body routing and entity decoding.

use crate::{Document, NodeId, NodeKind};

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
const RAW_TEXT: &[&str] = &["script", "style"];
const RCDATA: &[&str] = &["textarea", "title"];
const HEAD_ONLY: &[&str] = &["title", "meta", "link", "style", "script", "base", "noscript", "template"];
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "details", "div", "dl", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3",
    "h4", "h5", "h6", "header", "hr", "main", "menu", "nav", "ol", "p", "pre", "section", "table", "ul",
];

pub fn is_void(tag: &str) -> bool {
    VOID.contains(&tag)
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if let Some(semi) = after.find(';').filter(|&i| i <= 10) {
            let name = &after[..semi];
            let decoded = if let Some(num) = name.strip_prefix('#') {
                let code = if let Some(hex) = num.strip_prefix(['x', 'X']) { u32::from_str_radix(hex, 16).ok() } else { num.parse().ok() };
                code.and_then(char::from_u32)
            } else {
                named_entity(name)
            };
            if let Some(c) = decoded {
                out.push(c);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn named_entity(n: &str) -> Option<char> {
    Some(match n {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "bull" => '•',
        "middot" => '·',
        "times" => '×',
        "divide" => '÷',
        "laquo" => '«',
        "raquo" => '»',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "deg" => '°',
        "plusmn" => '±',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "minus" => '−',
        "check" => '✓',
        _ => return None,
    })
}

pub fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\u{a0}', "&nbsp;")
}

pub fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('\u{a0}', "&nbsp;")
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

enum Token {
    Start { tag: String, attrs: Vec<(String, String)>, self_closing: bool },
    End(String),
    Text(String),
    Comment(String),
    Eof,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn next(&mut self, raw_for: Option<&str>) -> Token {
        if self.pos >= self.src.len() {
            return Token::Eof;
        }
        if let Some(tag) = raw_for {
            // Raw text / RCDATA: consume until the matching end tag.
            let lower = self.rest().to_ascii_lowercase();
            let close = format!("</{tag}");
            let end = lower.find(&close).unwrap_or(lower.len());
            if end > 0 {
                let text = self.rest()[..end].to_string();
                self.pos += end;
                return Token::Text(text);
            }
            // At the closing tag (or EOF): fall through to normal tokenising so the End token is produced.
            if self.pos >= self.src.len() {
                return Token::Eof;
            }
        }
        let rest = self.rest();
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            self.pos += end;
            return Token::Text(rest[..end].to_string());
        }
        if rest.starts_with("<!--") {
            let end = rest[4..].find("-->").map(|i| i + 4);
            let (body, adv) = match end {
                Some(e) => (&rest[4..e], e + 3),
                None => (&rest[4..], rest.len()),
            };
            self.pos += adv;
            return Token::Comment(body.to_string());
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            self.pos += end;
            return self.next(None);
        }
        if let Some(after) = rest.strip_prefix("</") {
            if after.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                let end = after.find('>').unwrap_or(after.len());
                let name = after[..end].split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                self.pos += 2 + (end + 1).min(after.len());
                return Token::End(name);
            }
        }
        if rest[1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            return self.start_tag();
        }
        // Stray '<' is text.
        self.pos += 1;
        Token::Text("<".to_string())
    }

    fn start_tag(&mut self) -> Token {
        let bytes = self.src.as_bytes();
        let mut i = self.pos + 1;
        let name_start = i;
        while i < bytes.len() && !(bytes[i].is_ascii_whitespace() || bytes[i] == b'>' || bytes[i] == b'/') {
            i += 1;
        }
        let tag = self.src[name_start..i].to_ascii_lowercase();
        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut self_closing = false;
        loop {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'>' => {
                    i += 1;
                    break;
                }
                b'/' => {
                    if bytes.get(i + 1) == Some(&b'>') {
                        self_closing = true;
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                _ => {
                    let ns = i;
                    while i < bytes.len() && !(bytes[i].is_ascii_whitespace() || matches!(bytes[i], b'=' | b'>' | b'/')) {
                        i += 1;
                    }
                    if i == ns {
                        i += 1;
                        continue;
                    }
                    let name = self.src[ns..i].to_ascii_lowercase();
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    let mut value = String::new();
                    if i < bytes.len() && bytes[i] == b'=' {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                            let q = bytes[i];
                            i += 1;
                            let vs = i;
                            while i < bytes.len() && bytes[i] != q {
                                i += 1;
                            }
                            value = decode_entities(&self.src[vs..i]);
                            i = (i + 1).min(bytes.len());
                        } else {
                            let vs = i;
                            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                                i += 1;
                            }
                            value = decode_entities(&self.src[vs..i]);
                        }
                    }
                    if !attrs.iter().any(|(k, _)| *k == name) {
                        attrs.push((name, value));
                    }
                }
            }
        }
        self.pos = i;
        Token::Start { tag, attrs, self_closing }
    }
}

struct Builder<'a> {
    doc: &'a mut Document,
    stack: Vec<NodeId>,
    /// Document mode routes content into head/body.
    html: Option<NodeId>,
    head: Option<NodeId>,
    body: Option<NodeId>,
    in_head: bool,
}

impl Builder<'_> {
    fn current(&self) -> NodeId {
        *self.stack.last().unwrap()
    }

    fn tag_of(&self, id: NodeId) -> &str {
        self.doc.tag(id).unwrap_or("")
    }

    fn open_idx(&self, tag: &str) -> Option<usize> {
        self.stack.iter().rposition(|n| self.tag_of(*n) == tag)
    }

    fn has_in_scope(&self, tag: &str, stop: &[&str]) -> Option<usize> {
        for (i, n) in self.stack.iter().enumerate().rev() {
            let t = self.tag_of(*n);
            if t == tag {
                return Some(i);
            }
            if stop.contains(&t) {
                return None;
            }
        }
        None
    }

    fn switch_to_body(&mut self) {
        if let (true, Some(html)) = (self.in_head, self.html) {
            self.in_head = false;
            let body = match self.body {
                Some(b) => b,
                None => {
                    let b = self.doc.create_element("body");
                    let _ = self.doc.append_child(html, b);
                    self.body = Some(b);
                    b
                }
            };
            self.stack = vec![html, body];
        }
    }

    fn start(&mut self, tag: String, attrs: Vec<(String, String)>, self_closing: bool) {
        if self.html.is_some() {
            match tag.as_str() {
                "html" => {
                    let h = self.html.unwrap();
                    for (k, v) in attrs {
                        self.doc.set_attr(h, &k, &v);
                    }
                    return;
                }
                "head" => {
                    if let Some(h) = self.head {
                        self.in_head = true;
                        self.stack = vec![self.html.unwrap(), h];
                    }
                    return;
                }
                "body" => {
                    self.in_head = true;
                    self.switch_to_body();
                    let b = self.body.unwrap();
                    for (k, v) in attrs {
                        self.doc.set_attr(b, &k, &v);
                    }
                    return;
                }
                t if self.in_head && !HEAD_ONLY.contains(&t) => self.switch_to_body(),
                _ => {}
            }
        }
        // Implied end tags.
        if CLOSES_P.contains(&tag.as_str()) {
            if let Some(i) = self.has_in_scope("p", &["button", "td", "th", "table", "html", "body"]) {
                self.stack.truncate(i);
            }
        }
        match tag.as_str() {
            "li" => {
                if let Some(i) = self.has_in_scope("li", &["ul", "ol", "html", "body"]) {
                    self.stack.truncate(i);
                }
            }
            "dt" | "dd" => {
                if let Some(i) = self.stack.iter().rposition(|n| matches!(self.tag_of(*n), "dt" | "dd")) {
                    if !self.stack[i..].iter().any(|n| self.tag_of(*n) == "dl") {
                        self.stack.truncate(i);
                    }
                }
            }
            "option" => {
                if self.tag_of(self.current()) == "option" {
                    self.stack.pop();
                }
            }
            _ => {}
        }
        let el = self.doc.create_element(&tag);
        for (k, v) in attrs {
            self.doc.set_attr(el, &k, &v);
        }
        let parent = self.current();
        let _ = self.doc.append_child(parent, el);
        if !(is_void(&tag) || self_closing) {
            self.stack.push(el);
        }
    }

    fn end(&mut self, tag: &str) {
        if self.html.is_some() && matches!(tag, "html" | "body" | "head") {
            if tag == "head" {
                self.switch_to_body_keep_head_closed();
            }
            return;
        }
        if let Some(i) = self.open_idx(tag) {
            if i > 0 {
                self.stack.truncate(i);
            }
        }
    }

    fn switch_to_body_keep_head_closed(&mut self) {
        if let Some(html) = self.html {
            // After </head>, further content belongs to body.
            self.in_head = true;
            let _ = html;
            self.switch_to_body();
        }
    }

    fn text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if self.in_head && !text.trim().is_empty() {
            self.switch_to_body();
        }
        if self.in_head {
            // Whitespace between head elements is kept out of the DOM, as browsers do.
            return;
        }
        let text = decode_entities(&text);
        let parent = self.current();
        if let Some(last) = self.doc.last_child(parent) {
            if let NodeKind::Text(_) = self.doc.node(last).kind {
                let merged = format!("{}{}", self.doc.text(last).unwrap_or(""), text);
                self.doc.set_text(last, &merged);
                return;
            }
        }
        let t = self.doc.create_text(&text);
        let _ = self.doc.append_child(parent, t);
    }
}

fn run(builder: &mut Builder, source: &str) {
    let mut p = Parser { src: source, pos: 0 };
    let mut raw_for: Option<String> = None;
    loop {
        let tok = p.next(raw_for.as_deref());
        match tok {
            Token::Eof => break,
            Token::Text(t) => {
                if raw_for.is_some() {
                    // Raw text content: RCDATA decodes entities, script/style do not.
                    let cur = builder.current();
                    let decoded = if RCDATA.contains(&builder.tag_of(cur)) { decode_entities(&t) } else { t };
                    if !decoded.is_empty() {
                        let n = builder.doc.create_text(&decoded);
                        let _ = builder.doc.append_child(cur, n);
                    }
                } else {
                    builder.text(t);
                }
            }
            Token::Comment(c) => {
                if !builder.in_head {
                    let n = builder.doc.create_comment(&c);
                    let parent = builder.current();
                    let _ = builder.doc.append_child(parent, n);
                }
            }
            Token::Start { tag, attrs, self_closing } => {
                let raw = (RAW_TEXT.contains(&tag.as_str()) || RCDATA.contains(&tag.as_str())) && !self_closing;
                let before = builder.stack.len();
                builder.start(tag.clone(), attrs, self_closing);
                if raw && builder.stack.len() > before {
                    raw_for = Some(tag);
                }
            }
            Token::End(tag) => {
                if raw_for.as_deref() == Some(tag.as_str()) {
                    raw_for = None;
                }
                builder.end(&tag);
            }
        }
        // `raw_for` is reset on the End token above; a stale value cannot outlive its element.
        if raw_for.is_some() && p.rest().is_empty() {
            raw_for = None;
        }
    }
}

pub fn parse_document(source: &str) -> Document {
    let mut doc = Document::new();
    // Start from the skeleton: html > head + body.
    let html = doc.document_element().unwrap();
    let head = doc.head();
    let body = doc.body();
    // Rebuild empty so parsed content owns the structure.
    let mut builder = Builder { doc: &mut doc, stack: vec![html, head.unwrap()], html: Some(html), head, body, in_head: true };
    run(&mut builder, source);
    // If no content ever left head and there is no body content, the empty body from the skeleton remains.
    doc.dirty = crate::Dirty { all_style: true, layout: true, paint: true, ..Default::default() };
    doc
}

pub fn parse_fragment_into(doc: &mut Document, source: &str, context: Option<&str>) -> NodeId {
    let frag = doc.create_fragment();
    let mut builder = Builder { doc, stack: vec![frag], html: None, head: None, body: None, in_head: false };
    let _ = context;
    run(&mut builder, source);
    frag
}

// ---------------------------------------------------------------------------------------------
// serialization
// ---------------------------------------------------------------------------------------------

pub fn serialize_node(doc: &Document, id: NodeId) -> String {
    let mut s = String::new();
    write_node(doc, id, &mut s);
    s
}

pub fn serialize_children(doc: &Document, id: NodeId) -> String {
    let mut s = String::new();
    let raw = doc.tag(id).is_some_and(|t| RAW_TEXT.contains(&t));
    for c in doc.children(id) {
        if raw {
            if let Some(t) = doc.text(c) {
                s.push_str(t);
                continue;
            }
        }
        write_node(doc, c, &mut s);
    }
    s
}

fn write_node(doc: &Document, id: NodeId, out: &mut String) {
    match &doc.node(id).kind {
        NodeKind::Text(t) => out.push_str(&escape_text(t)),
        NodeKind::Comment(c) => {
            out.push_str("<!--");
            out.push_str(c);
            out.push_str("-->");
        }
        NodeKind::Element(e) => {
            out.push('<');
            out.push_str(&e.tag);
            for (k, v) in &e.attrs {
                out.push(' ');
                out.push_str(k);
                if !v.is_empty() {
                    out.push_str("=\"");
                    out.push_str(&escape_attr(v));
                    out.push('"');
                }
            }
            out.push('>');
            if !is_void(&e.tag) {
                out.push_str(&serialize_children(doc, id));
                out.push_str("</");
                out.push_str(&e.tag);
                out.push('>');
            }
        }
        NodeKind::Fragment | NodeKind::Document => out.push_str(&serialize_children(doc, id)),
        NodeKind::Free => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(src: &str) -> (Document, NodeId) {
        let mut d = Document::new();
        let f = d.parse_fragment(src);
        (d, f)
    }

    #[test]
    fn roundtrip_basic() {
        let (d, f) = frag(r#"<div id="a" class="x y"><p>Hi <b>there</b></p><img src="i.png"><br></div>"#);
        assert_eq!(serialize_children(&d, f), r#"<div id="a" class="x y"><p>Hi <b>there</b></p><img src="i.png"><br></div>"#);
    }

    #[test]
    fn entities_decode_and_escape() {
        let (d, f) = frag("<p title=\"a &amp; b\">1 &lt; 2 &#65;&#x42; &copy;</p>");
        let p = d.first_child(f).unwrap();
        assert_eq!(d.get_attr(p, "title"), Some("a & b"));
        assert_eq!(d.text_content(p), "1 < 2 AB ©");
        assert!(serialize_children(&d, f).contains("1 &lt; 2"));
        assert_eq!(decode_entities("&bogus; &amp"), "&bogus; &amp");
    }

    #[test]
    fn implied_end_tags() {
        let (d, f) = frag("<ul><li>a<li>b</ul><p>one<p>two<div>x</div>");
        let ul = d.first_child(f).unwrap();
        assert_eq!(d.child_elements(ul).count(), 2);
        let ps = d.child_elements(f).filter(|n| d.tag(*n) == Some("p")).count();
        assert_eq!(ps, 2);
    }

    #[test]
    fn raw_text_elements_keep_markup() {
        let (d, f) = frag("<style>a > b { color: red }</style><script>if (a<b && c>d) {}</script><textarea>&lt;x&gt;<b></textarea>");
        let kids: Vec<_> = d.child_elements(f).collect();
        assert_eq!(d.text_content(kids[0]), "a > b { color: red }");
        assert_eq!(d.text_content(kids[1]), "if (a<b && c>d) {}");
        assert_eq!(d.text_content(kids[2]), "<x><b>");
        assert_eq!(d.child_elements(kids[2]).count(), 0);
    }

    #[test]
    fn mismatched_end_tags_are_tolerated() {
        let (d, f) = frag("<div><span>a</div>b</span>c");
        assert_eq!(d.text_content(f), "abc");
        let (d, f) = frag("a</b>c <3 d");
        assert_eq!(d.text_content(f), "ac <3 d");
    }

    #[test]
    fn document_structure() {
        let d = Document::parse(
            "<!doctype html><html lang=en><head><title>T</title><style>p{}</style></head><body class=\"app\"><div id=r>x</div><script src=a.js></script></body></html>",
        );
        let html = d.document_element().unwrap();
        assert_eq!(d.get_attr(html, "lang"), Some("en"));
        let head = d.head().unwrap();
        let tags: Vec<_> = d.child_elements(head).map(|n| d.tag(n).unwrap().to_string()).collect();
        assert_eq!(tags, vec!["title", "style"]);
        let body = d.body().unwrap();
        assert_eq!(d.get_attr(body, "class"), Some("app"));
        assert_eq!(d.child_elements(body).count(), 2);
    }

    #[test]
    fn document_without_html_or_body_tags() {
        let d = Document::parse("<title>x</title><div>hi</div><p>yo</p>");
        assert_eq!(d.child_elements(d.head().unwrap()).count(), 1);
        assert_eq!(d.child_elements(d.body().unwrap()).count(), 2);
        let d = Document::parse("just text");
        assert_eq!(d.text_content(d.body().unwrap()), "just text");
    }

    #[test]
    fn self_closing_and_boolean_attrs() {
        let (d, f) = frag("<div/><input disabled checked value=x><my-el a=1 b='2'/>");
        assert_eq!(d.child_elements(f).count(), 3);
        let input = d.child_elements(f).nth(1).unwrap();
        assert!(d.has_attr(input, "disabled"));
        assert_eq!(d.get_attr(input, "value"), Some("x"));
        assert!(d.element(input).unwrap().form.checked);
    }
}

//! Stylesheet parser: rules, declarations, `@media`, `@keyframes`, `@font-face`, `@supports`, `@layer`.
//! Error recovery follows the CSS spec's spirit: an invalid rule or declaration is dropped, the rest survives.

use crate::selector::{split_top_level, Selector};

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    /// Lower-cased property name; custom properties (`--x`) keep their case.
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Debug, Clone)]
pub struct StyleRule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone)]
pub struct Keyframes {
    pub name: String,
    /// Sorted by offset (0.0..=1.0).
    pub frames: Vec<(f32, Vec<Declaration>)>,
}

#[derive(Debug, Clone)]
pub struct FontFace {
    pub family: String,
    pub src: String,
    pub weight: Option<u16>,
    pub italic: bool,
}

#[derive(Debug, Clone)]
pub enum Rule {
    Style(StyleRule),
    Media(MediaQueryList, Vec<Rule>),
    Keyframes(Keyframes),
    FontFace(FontFace),
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

impl Stylesheet {
    pub fn parse(source: &str) -> Stylesheet {
        let clean = strip_comments(source);
        Stylesheet { rules: parse_rules(&clean) }
    }
}

pub fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut last = 0;
    while i < b.len() {
        if let Some(q) = quote {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if b[i] == b'"' || b[i] == b'\'' {
            quote = Some(b[i]);
            i += 1;
        } else if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            out.push_str(&s[last..i]);
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            last = i.min(b.len());
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last.min(s.len())..]);
    out
}

/// Find the index of the `}` matching the `{` at `open`.
fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0;
    let mut quote: Option<u8> = None;
    let mut i = open;
    while i < b.len() {
        if let Some(q) = quote {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == q {
                quote = None;
            }
        } else {
            match b[i] {
                b'"' | b'\'' => quote = Some(b[i]),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn parse_rules(s: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let trimmed = rest.trim_start();
        i += rest.len() - trimmed.len();
        if i >= s.len() {
            break;
        }
        let rest = &s[i..];
        // At-rule without a block, e.g. @import / @charset.
        let brace = find_unquoted(rest, '{');
        let semi = find_unquoted(rest, ';');
        if rest.starts_with('@') {
            match (brace, semi) {
                (_, Some(sp)) if brace.is_none_or(|b| sp < b) => {
                    i += sp + 1;
                    continue;
                }
                (Some(bp), _) => {
                    let open = i + bp;
                    let Some(close) = matching_brace(s, open) else { break };
                    let prelude = s[i..open].trim();
                    let body = &s[open + 1..close];
                    parse_at_rule(prelude, body, &mut rules);
                    i = close + 1;
                    continue;
                }
                _ => break,
            }
        }
        let Some(bp) = brace else { break };
        let open = i + bp;
        let Some(close) = matching_brace(s, open) else { break };
        let selectors = Selector::parse_list(&s[i..open]);
        let declarations = parse_declarations(&s[open + 1..close]);
        if !selectors.is_empty() {
            rules.push(Rule::Style(StyleRule { selectors, declarations }));
        }
        i = close + 1;
    }
    rules
}

fn find_unquoted(s: &str, target: char) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == target && depth <= 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_at_rule(prelude: &str, body: &str, out: &mut Vec<Rule>) {
    let (name, args) = match prelude.find(|c: char| c.is_whitespace() || c == '(') {
        Some(p) => (&prelude[1..p], prelude[p..].trim()),
        None => (&prelude[1..], ""),
    };
    match name.to_ascii_lowercase().as_str() {
        "media" => out.push(Rule::Media(MediaQueryList::parse(args), parse_rules(body))),
        "supports" | "layer" | "container" if !body.trim().is_empty() => out.extend(parse_rules(body)),
        "keyframes" | "-webkit-keyframes" => {
            let mut frames = Vec::new();
            let mut i = 0;
            while let Some(bp) = find_unquoted(&body[i..], '{') {
                let open = i + bp;
                let Some(close) = matching_brace(body, open) else { break };
                let decls = parse_declarations(&body[open + 1..close]);
                for sel in body[i..open].split(',') {
                    let off = match sel.trim().to_ascii_lowercase().as_str() {
                        "from" => Some(0.0),
                        "to" => Some(1.0),
                        t => t.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()).map(|v| v / 100.0),
                    };
                    if let Some(o) = off {
                        frames.push((o.clamp(0.0, 1.0), decls.clone()));
                    }
                }
                i = close + 1;
            }
            frames.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            out.push(Rule::Keyframes(Keyframes { name: args.trim_matches(|c| c == '"' || c == '\'').to_string(), frames }));
        }
        "font-face" => {
            let decls = parse_declarations(body);
            let get = |n: &str| decls.iter().find(|d| d.name == n).map(|d| d.value.clone());
            if let (Some(family), Some(src)) = (get("font-family"), get("src")) {
                out.push(Rule::FontFace(FontFace {
                    family: family.trim_matches(|c| c == '"' || c == '\'').to_string(),
                    src,
                    weight: get("font-weight").and_then(|w| match w.as_str() {
                        "bold" => Some(700),
                        "normal" => Some(400),
                        n => n.parse().ok(),
                    }),
                    italic: get("font-style").is_some_and(|s| s == "italic" || s == "oblique"),
                }));
            }
        }
        _ => {}
    }
}

pub fn parse_declarations(body: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    for part in split_top_level(body, ';') {
        let Some(colon) = part.find(':') else { continue };
        let name = part[..colon].trim();
        let mut value = part[colon + 1..].trim().to_string();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let mut important = false;
        let lower = value.to_ascii_lowercase();
        if let Some(p) = lower.rfind("!important") {
            if lower[p + 10..].trim().is_empty() {
                important = true;
                value = value[..p].trim().to_string();
            }
        }
        let name = if name.starts_with("--") { name.to_string() } else { name.to_ascii_lowercase() };
        out.push(Declaration { name, value, important });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Media queries
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct MediaContext {
    pub width: f32,
    pub height: f32,
    pub dpr: f32,
    pub dark: bool,
    pub reduced_motion: bool,
}

impl Default for MediaContext {
    fn default() -> Self {
        MediaContext { width: 800.0, height: 600.0, dpr: 1.0, dark: false, reduced_motion: false }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MediaQueryList {
    queries: Vec<MediaQuery>,
}

#[derive(Debug, Clone)]
struct MediaQuery {
    negated: bool,
    media_type: String,
    features: Vec<(String, Option<String>)>,
}

impl MediaQueryList {
    pub fn parse(s: &str) -> Self {
        let mut queries = Vec::new();
        for q in s.split(',') {
            let mut q = q.trim().to_ascii_lowercase();
            if q.is_empty() {
                continue;
            }
            let mut negated = false;
            if let Some(r) = q.strip_prefix("not ") {
                negated = true;
                q = r.to_string();
            }
            if let Some(r) = q.strip_prefix("only ") {
                q = r.to_string();
            }
            let mut media_type = "all".to_string();
            let mut features = Vec::new();
            for tok in q.split(" and ") {
                let tok = tok.trim();
                if let Some(inner) = tok.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
                    match inner.split_once(':') {
                        Some((k, v)) => features.push((k.trim().to_string(), Some(v.trim().to_string()))),
                        None => features.push((inner.trim().to_string(), None)),
                    }
                } else if !tok.is_empty() {
                    media_type = tok.to_string();
                }
            }
            queries.push(MediaQuery { negated, media_type, features });
        }
        MediaQueryList { queries }
    }

    pub fn matches(&self, cx: &MediaContext) -> bool {
        if self.queries.is_empty() {
            return true;
        }
        self.queries.iter().any(|q| {
            let type_ok = matches!(q.media_type.as_str(), "all" | "screen");
            let ok = type_ok && q.features.iter().all(|(k, v)| feature_matches(k, v.as_deref(), cx));
            ok != q.negated
        })
    }
}

fn px(v: &str, cx: &MediaContext) -> Option<f32> {
    let _ = cx;
    if let Some(n) = v.strip_suffix("px") {
        n.trim().parse().ok()
    } else if let Some(n) = v.strip_suffix("em").or_else(|| v.strip_suffix("rem")) {
        n.trim().parse::<f32>().ok().map(|x| x * 16.0)
    } else {
        v.parse().ok()
    }
}

fn feature_matches(k: &str, v: Option<&str>, cx: &MediaContext) -> bool {
    match (k, v) {
        ("min-width", Some(v)) => px(v, cx).is_some_and(|x| cx.width >= x),
        ("max-width", Some(v)) => px(v, cx).is_some_and(|x| cx.width <= x),
        ("width", Some(v)) => px(v, cx).is_some_and(|x| (cx.width - x).abs() < 0.5),
        ("min-height", Some(v)) => px(v, cx).is_some_and(|x| cx.height >= x),
        ("max-height", Some(v)) => px(v, cx).is_some_and(|x| cx.height <= x),
        ("height", Some(v)) => px(v, cx).is_some_and(|x| (cx.height - x).abs() < 0.5),
        ("orientation", Some("landscape")) => cx.width >= cx.height,
        ("orientation", Some("portrait")) => cx.width < cx.height,
        ("prefers-color-scheme", Some("dark")) => cx.dark,
        ("prefers-color-scheme", Some("light")) => !cx.dark,
        ("prefers-reduced-motion", Some("reduce")) => cx.reduced_motion,
        ("prefers-reduced-motion", Some("no-preference")) => !cx.reduced_motion,
        ("min-resolution", Some(v)) | ("-webkit-min-device-pixel-ratio", Some(v)) => {
            let n = v.trim_end_matches("dppx").trim_end_matches('x').parse::<f32>().unwrap_or(f32::MAX);
            cx.dpr >= n
        }
        ("hover", Some("hover")) | ("pointer", Some("fine")) => true,
        ("color", None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules_and_recovers() {
        let css = r#"
            /* hi */ .a, #b > c { color: red; margin : 4px !important; ; bogus }
            @import url(x.css);
            @media (min-width: 600px) and (max-width: 900px) { .m { display:none } }
            @keyframes spin { from { opacity: 0 } 50% { opacity: .5 } to { opacity: 1 } }
            .broken { color: blue
        "#;
        let sheet = Stylesheet::parse(css);
        let styles = sheet.rules.iter().filter(|r| matches!(r, Rule::Style(_))).count();
        assert_eq!(styles, 1, "unterminated trailing rule is dropped");
        match &sheet.rules[0] {
            Rule::Style(r) => {
                assert_eq!(r.selectors.len(), 2);
                assert_eq!(r.declarations.len(), 2);
                assert!(r.declarations[1].important);
                assert_eq!(r.declarations[1].value, "4px");
            }
            _ => panic!(),
        }
        let kf = sheet.rules.iter().find_map(|r| if let Rule::Keyframes(k) = r { Some(k) } else { None }).unwrap();
        assert_eq!(kf.frames.len(), 3);
        assert_eq!(kf.frames[1].0, 0.5);
    }

    #[test]
    fn media_evaluation() {
        let q = MediaQueryList::parse("screen and (min-width: 600px) and (max-width: 900px), print");
        let mut cx = MediaContext::default();
        cx.width = 700.0;
        assert!(q.matches(&cx));
        cx.width = 1000.0;
        assert!(!q.matches(&cx));
        assert!(MediaQueryList::parse("(prefers-color-scheme: dark)").matches(&MediaContext { dark: true, ..cx }));
        assert!(!MediaQueryList::parse("not screen").matches(&cx));
    }

    #[test]
    fn declarations_keep_custom_property_case() {
        let d = parse_declarations("--Brand-Color: #fff; COLOR: red");
        assert_eq!(d[0].name, "--Brand-Color");
        assert_eq!(d[1].name, "color");
    }

    #[test]
    fn semicolons_inside_url_survive() {
        let d = parse_declarations("background: url(\"data:image/png;base64,AAA\"); color: red");
        assert_eq!(d.len(), 2);
    }
}

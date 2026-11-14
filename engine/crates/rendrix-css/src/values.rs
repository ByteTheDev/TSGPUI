//! Value-level parsing helpers: lengths, `calc()` on absolute operands, tokenising and `var()` substitution.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dim {
    Auto,
    Px(f32),
    Percent(f32),
}

impl Dim {
    pub fn px(self) -> Option<f32> {
        match self {
            Dim::Px(v) => Some(v),
            _ => None,
        }
    }
    pub fn resolve(self, basis: f32) -> Option<f32> {
        match self {
            Dim::Auto => None,
            Dim::Px(v) => Some(v),
            Dim::Percent(p) => Some(basis * p / 100.0),
        }
    }
}

/// What lengths resolve against while computing a style.
#[derive(Debug, Clone, Copy)]
pub struct LengthContext {
    pub font_size: f32,
    pub root_font_size: f32,
    pub viewport_w: f32,
    pub viewport_h: f32,
}

impl LengthContext {
    pub fn new(font_size: f32, root_font_size: f32, vw: f32, vh: f32) -> Self {
        LengthContext { font_size, root_font_size, viewport_w: vw, viewport_h: vh }
    }
}

/// Split a value on top-level whitespace (respecting parentheses and quotes).
pub fn split_ws(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Split on top-level commas.
pub fn split_commas(s: &str) -> Vec<String> {
    crate::selector::split_top_level(s, ',').into_iter().map(|p| p.trim().to_string()).collect()
}

/// Parse a number with an optional unit. Returns (value, lowercase unit).
pub fn split_number(s: &str) -> Option<(f32, String)> {
    let s = s.trim();
    let end = s
        .char_indices()
        .find(|&(i, c)| {
            !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && i == 0) || ((c == 'e' || c == 'E') && false))
        })
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    let num: f32 = s[..end].parse().ok()?;
    Some((num, s[end..].to_ascii_lowercase()))
}

pub fn parse_length(s: &str, cx: &LengthContext) -> Option<Dim> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(Dim::Auto);
    }
    if let Some(inner) = strip_func(s, "calc") {
        return calc(inner, cx);
    }
    if let Some((n, unit)) = split_number(s) {
        return match unit.as_str() {
            "" if n == 0.0 => Some(Dim::Px(0.0)),
            "%" => Some(Dim::Percent(n)),
            u => abs_unit(n, u, cx).map(Dim::Px),
        };
    }
    None
}

fn abs_unit(n: f32, unit: &str, cx: &LengthContext) -> Option<f32> {
    Some(match unit {
        "px" => n,
        "em" => n * cx.font_size,
        "rem" => n * cx.root_font_size,
        "ch" => n * cx.font_size * 0.5,
        "ex" => n * cx.font_size * 0.5,
        "vw" => n * cx.viewport_w / 100.0,
        "vh" => n * cx.viewport_h / 100.0,
        "vmin" => n * cx.viewport_w.min(cx.viewport_h) / 100.0,
        "vmax" => n * cx.viewport_w.max(cx.viewport_h) / 100.0,
        "pt" => n * 96.0 / 72.0,
        "pc" => n * 16.0,
        "in" => n * 96.0,
        "cm" => n * 96.0 / 2.54,
        "mm" => n * 96.0 / 25.4,
        "q" => n * 96.0 / 101.6,
        _ => return None,
    })
}

/// Parse a non-negative absolute length (no percent, no auto).
pub fn parse_px(s: &str, cx: &LengthContext) -> Option<f32> {
    match parse_length(s, cx)? {
        Dim::Px(v) => Some(v),
        _ => None,
    }
}

pub fn strip_func<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    let lower_ok = s.len() > name.len() + 2
        && s[..name.len()].eq_ignore_ascii_case(name)
        && s.as_bytes()[name.len()] == b'('
        && s.ends_with(')');
    if lower_ok {
        Some(&s[name.len() + 1..s.len() - 1])
    } else {
        None
    }
}

/// `calc()` is evaluated when all operands share a kind (all absolute lengths, or all percentages).
/// Mixed `100% - 20px` cannot be represented in `Dim` and returns `None` (documented limitation).
fn calc(expr: &str, cx: &LengthContext) -> Option<Dim> {
    #[derive(Clone, Copy)]
    enum V {
        Abs(f32),
        Pct(f32),
        Num(f32),
    }
    fn tokenize(e: &str) -> Vec<String> {
        let mut toks = Vec::new();
        let mut cur = String::new();
        let mut depth = 0;
        let chars: Vec<char> = e.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            match c {
                '(' => {
                    depth += 1;
                    cur.push(c);
                }
                ')' => {
                    depth -= 1;
                    cur.push(c);
                }
                '+' | '-' | '*' | '/' if depth == 0 => {
                    // '-'/'+' directly after an operator or at start is a sign; operators in calc need whitespace.
                    let prev_ws = i > 0 && chars[i - 1].is_whitespace();
                    let next_ws = i + 1 < chars.len() && chars[i + 1].is_whitespace();
                    if (c == '*' || c == '/') || (prev_ws && next_ws) {
                        if !cur.trim().is_empty() {
                            toks.push(cur.trim().to_string());
                        }
                        cur.clear();
                        toks.push(c.to_string());
                    } else {
                        cur.push(c);
                    }
                }
                c if c.is_whitespace() && depth == 0 => {
                    if !cur.trim().is_empty() {
                        toks.push(cur.trim().to_string());
                    }
                    cur.clear();
                }
                c => cur.push(c),
            }
        }
        if !cur.trim().is_empty() {
            toks.push(cur.trim().to_string());
        }
        toks
    }
    fn operand(t: &str, cx: &LengthContext) -> Option<V> {
        if let Some(inner) = strip_func(t, "calc").or_else(|| t.strip_prefix('(').and_then(|x| x.strip_suffix(')'))) {
            return match calc(inner, cx)? {
                Dim::Px(v) => Some(V::Abs(v)),
                Dim::Percent(p) => Some(V::Pct(p)),
                Dim::Auto => None,
            };
        }
        let (n, unit) = split_number(t)?;
        Some(match unit.as_str() {
            "" => V::Num(n),
            "%" => V::Pct(n),
            u => V::Abs(abs_unit(n, u, cx)?),
        })
    }
    fn combine(a: V, b: V, op: &str) -> Option<V> {
        use V::*;
        Some(match (a, b, op) {
            (Abs(x), Abs(y), "+") => Abs(x + y),
            (Abs(x), Abs(y), "-") => Abs(x - y),
            (Pct(x), Pct(y), "+") => Pct(x + y),
            (Pct(x), Pct(y), "-") => Pct(x - y),
            (Num(x), Num(y), "+") => Num(x + y),
            (Num(x), Num(y), "-") => Num(x - y),
            (Abs(x), Num(y), "*") | (Num(y), Abs(x), "*") => Abs(x * y),
            (Pct(x), Num(y), "*") | (Num(y), Pct(x), "*") => Pct(x * y),
            (Num(x), Num(y), "*") => Num(x * y),
            (Abs(x), Num(y), "/") if y != 0.0 => Abs(x / y),
            (Pct(x), Num(y), "/") if y != 0.0 => Pct(x / y),
            (Num(x), Num(y), "/") if y != 0.0 => Num(x / y),
            _ => return None,
        })
    }
    let toks = tokenize(expr);
    if toks.is_empty() {
        return None;
    }
    // First pass: * and /.
    let mut terms: Vec<V> = Vec::new();
    let mut ops: Vec<String> = Vec::new();
    let mut i = 0;
    let mut cur = operand(&toks[0], cx)?;
    i += 1;
    while i < toks.len() {
        let op = toks[i].as_str();
        let rhs = operand(toks.get(i + 1)?, cx)?;
        match op {
            "*" | "/" => cur = combine(cur, rhs, op)?,
            "+" | "-" => {
                terms.push(cur);
                ops.push(op.to_string());
                cur = rhs;
            }
            _ => return None,
        }
        i += 2;
    }
    terms.push(cur);
    let mut acc = terms[0];
    for (k, op) in ops.iter().enumerate() {
        acc = combine(acc, terms[k + 1], op)?;
    }
    match acc {
        V::Abs(v) => Some(Dim::Px(v)),
        V::Pct(p) => Some(Dim::Percent(p)),
        V::Num(v) if v == 0.0 => Some(Dim::Px(0.0)),
        V::Num(_) => None,
    }
}

/// Parse a time value (`200ms`, `0.3s`) to seconds.
pub fn parse_time(s: &str) -> Option<f32> {
    let (n, u) = split_number(s)?;
    match u.as_str() {
        "s" => Some(n),
        "ms" => Some(n / 1000.0),
        _ => None,
    }
}

/// Resolve all `var(--x, fallback)` references. Returns `None` if a variable is undefined with no fallback.
pub fn substitute_vars(value: &str, vars: &BTreeMap<String, String>) -> Option<String> {
    substitute_depth(value, vars, 0)
}

fn substitute_depth(value: &str, vars: &BTreeMap<String, String>, depth: u32) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let mut out = String::new();
    let mut rest = value;
    while let Some(pos) = rest.find("var(") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 4..];
        let mut level = 1;
        let mut end = None;
        for (i, c) in after.char_indices() {
            match c {
                '(' => level += 1,
                ')' => {
                    level -= 1;
                    if level == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end?;
        let inner = &after[..end];
        let (name, fallback) = match inner.find(',') {
            Some(c) => (inner[..c].trim(), Some(inner[c + 1..].trim())),
            None => (inner.trim(), None),
        };
        let resolved = match vars.get(name) {
            Some(v) => substitute_depth(v, vars, depth + 1)?,
            None => substitute_depth(fallback?, vars, depth + 1)?,
        };
        out.push_str(&resolved);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx() -> LengthContext {
        LengthContext::new(20.0, 16.0, 1000.0, 500.0)
    }

    #[test]
    fn units() {
        assert_eq!(parse_length("2em", &cx()), Some(Dim::Px(40.0)));
        assert_eq!(parse_length("2rem", &cx()), Some(Dim::Px(32.0)));
        assert_eq!(parse_length("10vw", &cx()), Some(Dim::Px(100.0)));
        assert_eq!(parse_length("50%", &cx()), Some(Dim::Percent(50.0)));
        assert_eq!(parse_length("0", &cx()), Some(Dim::Px(0.0)));
        assert_eq!(parse_length("-4px", &cx()), Some(Dim::Px(-4.0)));
        assert_eq!(parse_length("auto", &cx()), Some(Dim::Auto));
        assert_eq!(parse_length("12", &cx()), None, "unitless non-zero is invalid");
        assert_eq!(parse_length("1in", &cx()), Some(Dim::Px(96.0)));
    }

    #[test]
    fn calc_same_kind() {
        assert_eq!(parse_length("calc(10px + 2em)", &cx()), Some(Dim::Px(50.0)));
        assert_eq!(parse_length("calc(100% / 4)", &cx()), Some(Dim::Percent(25.0)));
        assert_eq!(parse_length("calc(2 * (3px + 1px))", &cx()), Some(Dim::Px(8.0)));
        assert_eq!(parse_length("calc(100% - 20px)", &cx()), None);
    }

    #[test]
    fn vars() {
        let mut v = BTreeMap::new();
        v.insert("--a".to_string(), "10px".to_string());
        v.insert("--b".to_string(), "var(--a)".to_string());
        assert_eq!(substitute_vars("var(--b) var(--c, 3px)", &v).unwrap(), "10px 3px");
        assert!(substitute_vars("var(--missing)", &v).is_none());
        v.insert("--loop".to_string(), "var(--loop)".to_string());
        assert!(substitute_vars("var(--loop)", &v).is_none());
    }

    #[test]
    fn whitespace_splitting() {
        assert_eq!(split_ws("1px solid rgb(1, 2, 3)"), vec!["1px", "solid", "rgb(1, 2, 3)"]);
    }
}

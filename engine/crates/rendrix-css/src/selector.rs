//! Selector AST, parsing, specificity and right-to-left matching.
//!
//! Matching is generic over [`SelectorElement`], so the DOM (or any tree) can be matched without
//! this crate knowing about it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementState {
    Hover,
    Active,
    Focus,
    FocusWithin,
    Disabled,
    Checked,
}

pub trait SelectorElement: Sized + Clone {
    fn tag(&self) -> &str;
    fn id(&self) -> Option<&str>;
    fn has_class(&self, class: &str) -> bool;
    fn attr(&self, name: &str) -> Option<&str>;
    fn parent_element(&self) -> Option<Self>;
    fn prev_sibling_element(&self) -> Option<Self>;
    fn next_sibling_element(&self) -> Option<Self>;
    fn has_state(&self, state: ElementState) -> bool;
    fn is_root(&self) -> bool;
    fn is_empty(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttrOp {
    Exists,
    Equals,
    Includes,  // ~=
    DashMatch, // |=
    Prefix,    // ^=
    Suffix,    // $=
    Substring, // *=
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttrSelector {
    pub name: String,
    pub op: AttrOp,
    pub value: String,
    pub case_insensitive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PseudoClass {
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
    Disabled,
    Enabled,
    Checked,
    Root,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    NthChild(i32, i32),
    NthLastChild(i32, i32),
    NthOfType(i32, i32),
    Not(Vec<Selector>),
    Is(Vec<Selector>),
    Where(Vec<Selector>),
    /// Recognised but never matching (e.g. `:visited`, `:link` is treated as `a[href]`).
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseudoElement {
    Before,
    After,
    Placeholder,
    Selection,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSelector>,
    pub pseudos: Vec<PseudoClass>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// Leftmost compound first; the last compound is the subject.
    pub compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` and `compounds[i + 1]`.
    pub combinators: Vec<Combinator>,
    pub pseudo_element: Option<PseudoElement>,
    pub specificity: (u32, u32, u32),
}

impl Selector {
    pub fn parse_list(input: &str) -> Vec<Selector> {
        split_top_level(input, ',')
            .iter()
            .filter_map(|s| Selector::parse(s.trim()))
            .collect()
    }

    pub fn parse(input: &str) -> Option<Selector> {
        let chars: Vec<char> = input.trim().chars().collect();
        if chars.is_empty() {
            return None;
        }
        let mut i = 0;
        let mut compounds = Vec::new();
        let mut combinators = Vec::new();
        let mut pseudo_element = None;
        let mut spec = (0u32, 0u32, 0u32);
        loop {
            let (compound, pe, next, s) = parse_compound(&chars, i)?;
            if pe.is_some() {
                pseudo_element = pe;
            }
            spec.0 += s.0;
            spec.1 += s.1;
            spec.2 += s.2;
            compounds.push(compound);
            i = next;
            // combinator
            let mut saw_space = false;
            while i < chars.len() && chars[i].is_whitespace() {
                saw_space = true;
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let comb = match chars[i] {
                '>' => {
                    i += 1;
                    Combinator::Child
                }
                '+' => {
                    i += 1;
                    Combinator::NextSibling
                }
                '~' => {
                    i += 1;
                    Combinator::SubsequentSibling
                }
                _ if saw_space => Combinator::Descendant,
                _ => return None,
            };
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i >= chars.len() {
                return None;
            }
            combinators.push(comb);
        }
        Some(Selector { compounds, combinators, pseudo_element, specificity: spec })
    }

    pub fn matches<E: SelectorElement>(&self, el: &E) -> bool {
        if self.pseudo_element.is_some() {
            return false;
        }
        self.matches_subject(el)
    }

    /// Match ignoring the pseudo-element (used to find the originating element of `::before`).
    pub fn matches_subject<E: SelectorElement>(&self, el: &E) -> bool {
        self.match_from(self.compounds.len() - 1, el)
    }

    fn match_from<E: SelectorElement>(&self, idx: usize, el: &E) -> bool {
        if !compound_matches(&self.compounds[idx], el) {
            return false;
        }
        if idx == 0 {
            return true;
        }
        match self.combinators[idx - 1] {
            Combinator::Child => el.parent_element().is_some_and(|p| self.match_from(idx - 1, &p)),
            Combinator::Descendant => {
                let mut cur = el.parent_element();
                while let Some(p) = cur {
                    if self.match_from(idx - 1, &p) {
                        return true;
                    }
                    cur = p.parent_element();
                }
                false
            }
            Combinator::NextSibling => el.prev_sibling_element().is_some_and(|p| self.match_from(idx - 1, &p)),
            Combinator::SubsequentSibling => {
                let mut cur = el.prev_sibling_element();
                while let Some(p) = cur {
                    if self.match_from(idx - 1, &p) {
                        return true;
                    }
                    cur = p.prev_sibling_element();
                }
                false
            }
        }
    }
}

fn compound_matches<E: SelectorElement>(c: &Compound, el: &E) -> bool {
    if let Some(tag) = &c.tag {
        if !el.tag().eq_ignore_ascii_case(tag) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if el.id() != Some(id.as_str()) {
            return false;
        }
    }
    if !c.classes.iter().all(|cl| el.has_class(cl)) {
        return false;
    }
    for a in &c.attrs {
        let Some(v) = el.attr(&a.name) else { return false };
        let (v, want) = if a.case_insensitive {
            (v.to_ascii_lowercase(), a.value.to_ascii_lowercase())
        } else {
            (v.to_string(), a.value.clone())
        };
        let ok = match a.op {
            AttrOp::Exists => true,
            AttrOp::Equals => v == want,
            AttrOp::Includes => v.split_whitespace().any(|w| w == want),
            AttrOp::DashMatch => v == want || v.starts_with(&format!("{want}-")),
            AttrOp::Prefix => !want.is_empty() && v.starts_with(&want),
            AttrOp::Suffix => !want.is_empty() && v.ends_with(&want),
            AttrOp::Substring => !want.is_empty() && v.contains(&want),
        };
        if !ok {
            return false;
        }
    }
    c.pseudos.iter().all(|p| pseudo_matches(p, el))
}

fn sibling_position<E: SelectorElement>(el: &E, of_type: bool, from_end: bool) -> i32 {
    let mut n = 1;
    let mut cur = if from_end { el.next_sibling_element() } else { el.prev_sibling_element() };
    while let Some(s) = cur {
        if !of_type || s.tag().eq_ignore_ascii_case(el.tag()) {
            n += 1;
        }
        cur = if from_end { s.next_sibling_element() } else { s.prev_sibling_element() };
    }
    n
}

fn nth_matches(a: i32, b: i32, pos: i32) -> bool {
    if a == 0 {
        return pos == b;
    }
    let diff = pos - b;
    diff % a == 0 && diff / a >= 0
}

fn pseudo_matches<E: SelectorElement>(p: &PseudoClass, el: &E) -> bool {
    match p {
        PseudoClass::Hover => el.has_state(ElementState::Hover),
        PseudoClass::Active => el.has_state(ElementState::Active),
        PseudoClass::Focus | PseudoClass::FocusVisible => el.has_state(ElementState::Focus),
        PseudoClass::FocusWithin => el.has_state(ElementState::FocusWithin) || el.has_state(ElementState::Focus),
        PseudoClass::Disabled => el.has_state(ElementState::Disabled),
        PseudoClass::Enabled => {
            matches!(el.tag(), "button" | "input" | "select" | "textarea") && !el.has_state(ElementState::Disabled)
        }
        PseudoClass::Checked => el.has_state(ElementState::Checked),
        PseudoClass::Root => el.is_root(),
        PseudoClass::Empty => el.is_empty(),
        PseudoClass::FirstChild => el.prev_sibling_element().is_none(),
        PseudoClass::LastChild => el.next_sibling_element().is_none(),
        PseudoClass::OnlyChild => el.prev_sibling_element().is_none() && el.next_sibling_element().is_none(),
        PseudoClass::FirstOfType => sibling_position(el, true, false) == 1,
        PseudoClass::LastOfType => sibling_position(el, true, true) == 1,
        PseudoClass::NthChild(a, b) => nth_matches(*a, *b, sibling_position(el, false, false)),
        PseudoClass::NthLastChild(a, b) => nth_matches(*a, *b, sibling_position(el, false, true)),
        PseudoClass::NthOfType(a, b) => nth_matches(*a, *b, sibling_position(el, true, false)),
        PseudoClass::Not(list) => !list.iter().any(|s| s.matches(el)),
        PseudoClass::Is(list) | PseudoClass::Where(list) => list.iter().any(|s| s.matches(el)),
        PseudoClass::Never => false,
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

fn read_ident(chars: &[char], mut i: usize) -> (String, usize) {
    let mut s = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            s.push(chars[i + 1]);
            i += 2;
        } else if is_ident_char(c) {
            s.push(c);
            i += 1;
        } else {
            break;
        }
    }
    (s, i)
}

type CompoundParse = (Compound, Option<PseudoElement>, usize, (u32, u32, u32));

fn parse_compound(chars: &[char], mut i: usize) -> Option<CompoundParse> {
    let mut c = Compound::default();
    let mut pe = None;
    let mut spec = (0u32, 0u32, 0u32);
    let start = i;
    while i < chars.len() {
        match chars[i] {
            '*' => i += 1,
            '#' => {
                let (name, n) = read_ident(chars, i + 1);
                if name.is_empty() {
                    return None;
                }
                c.id = Some(name);
                spec.0 += 1;
                i = n;
            }
            '.' => {
                let (name, n) = read_ident(chars, i + 1);
                if name.is_empty() {
                    return None;
                }
                c.classes.push(name);
                spec.1 += 1;
                i = n;
            }
            '[' => {
                let close = chars[i..].iter().position(|&ch| ch == ']')? + i;
                let body: String = chars[i + 1..close].iter().collect();
                c.attrs.push(parse_attr(&body)?);
                spec.1 += 1;
                i = close + 1;
            }
            ':' => {
                let double = chars.get(i + 1) == Some(&':');
                let (name, mut n) = read_ident(chars, i + if double { 2 } else { 1 });
                let name = name.to_ascii_lowercase();
                let mut arg: Option<String> = None;
                if chars.get(n) == Some(&'(') {
                    let mut depth = 0;
                    let mut j = n;
                    while j < chars.len() {
                        if chars[j] == '(' {
                            depth += 1;
                        } else if chars[j] == ')' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        j += 1;
                    }
                    if j >= chars.len() {
                        return None;
                    }
                    arg = Some(chars[n + 1..j].iter().collect());
                    n = j + 1;
                }
                i = n;
                if double || matches!(name.as_str(), "before" | "after" | "first-line" | "first-letter") {
                    pe = Some(match name.as_str() {
                        "before" => PseudoElement::Before,
                        "after" => PseudoElement::After,
                        "placeholder" => PseudoElement::Placeholder,
                        "selection" => PseudoElement::Selection,
                        _ => PseudoElement::Other,
                    });
                    spec.2 += 1;
                    continue;
                }
                let pc = match (name.as_str(), arg) {
                    ("hover", None) => PseudoClass::Hover,
                    ("active", None) => PseudoClass::Active,
                    ("focus", None) => PseudoClass::Focus,
                    ("focus-visible", None) => PseudoClass::FocusVisible,
                    ("focus-within", None) => PseudoClass::FocusWithin,
                    ("disabled", None) => PseudoClass::Disabled,
                    ("enabled", None) => PseudoClass::Enabled,
                    ("checked", None) => PseudoClass::Checked,
                    ("root", None) => PseudoClass::Root,
                    ("empty", None) => PseudoClass::Empty,
                    ("first-child", None) => PseudoClass::FirstChild,
                    ("last-child", None) => PseudoClass::LastChild,
                    ("only-child", None) => PseudoClass::OnlyChild,
                    ("first-of-type", None) => PseudoClass::FirstOfType,
                    ("last-of-type", None) => PseudoClass::LastOfType,
                    ("nth-child", Some(a)) => {
                        let (x, y) = parse_nth(&a)?;
                        PseudoClass::NthChild(x, y)
                    }
                    ("nth-last-child", Some(a)) => {
                        let (x, y) = parse_nth(&a)?;
                        PseudoClass::NthLastChild(x, y)
                    }
                    ("nth-of-type", Some(a)) => {
                        let (x, y) = parse_nth(&a)?;
                        PseudoClass::NthOfType(x, y)
                    }
                    ("not", Some(a)) => {
                        let list = Selector::parse_list(&a);
                        let m = list.iter().map(|s| s.specificity).max().unwrap_or_default();
                        spec = (spec.0 + m.0, spec.1 + m.1, spec.2 + m.2);
                        c.pseudos.push(PseudoClass::Not(list));
                        continue;
                    }
                    ("is", Some(a)) | ("matches", Some(a)) => {
                        let list = Selector::parse_list(&a);
                        let m = list.iter().map(|s| s.specificity).max().unwrap_or_default();
                        spec = (spec.0 + m.0, spec.1 + m.1, spec.2 + m.2);
                        c.pseudos.push(PseudoClass::Is(list));
                        continue;
                    }
                    ("where", Some(a)) => {
                        c.pseudos.push(PseudoClass::Where(Selector::parse_list(&a)));
                        continue;
                    }
                    ("link", None) | ("any-link", None) => {
                        c.attrs.push(AttrSelector {
                            name: "href".into(),
                            op: AttrOp::Exists,
                            value: String::new(),
                            case_insensitive: false,
                        });
                        spec.1 += 1;
                        continue;
                    }
                    _ => PseudoClass::Never,
                };
                spec.1 += 1;
                c.pseudos.push(pc);
            }
            ch if is_ident_char(ch) || ch == '\\' => {
                let (name, n) = read_ident(chars, i);
                c.tag = Some(name.to_ascii_lowercase());
                spec.2 += 1;
                i = n;
            }
            _ => break,
        }
    }
    if i == start {
        return None;
    }
    Some((c, pe, i, spec))
}

fn parse_attr(body: &str) -> Option<AttrSelector> {
    let body = body.trim();
    let ops = ["~=", "|=", "^=", "$=", "*=", "="];
    for op in ops {
        if let Some(pos) = body.find(op) {
            let name = body[..pos].trim().to_string();
            let mut value = body[pos + op.len()..].trim().to_string();
            let mut ci = false;
            if let Some(stripped) = value.strip_suffix(" i").or_else(|| value.strip_suffix(" I")) {
                value = stripped.trim().to_string();
                ci = true;
            }
            let value = value.trim_matches(|c| c == '"' || c == '\'').to_string();
            let op = match op {
                "~=" => AttrOp::Includes,
                "|=" => AttrOp::DashMatch,
                "^=" => AttrOp::Prefix,
                "$=" => AttrOp::Suffix,
                "*=" => AttrOp::Substring,
                _ => AttrOp::Equals,
            };
            return Some(AttrSelector { name: name.to_ascii_lowercase(), op, value, case_insensitive: ci });
        }
    }
    if body.is_empty() {
        None
    } else {
        Some(AttrSelector { name: body.to_ascii_lowercase(), op: AttrOp::Exists, value: String::new(), case_insensitive: false })
    }
}

/// Parse the `an+b` microsyntax.
pub fn parse_nth(arg: &str) -> Option<(i32, i32)> {
    let s: String = arg.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some(npos) = s.find('n') {
        let a = match &s[..npos] {
            "" | "+" => 1,
            "-" => -1,
            t => t.parse().ok()?,
        };
        let rest = &s[npos + 1..];
        let b = if rest.is_empty() { 0 } else { rest.parse().ok()? };
        Some((a, b))
    } else {
        Some((0, s.parse().ok()?))
    }
}

/// Split on `sep` ignoring separators inside (), [] and quotes.
pub fn split_top_level(input: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for c in input.chars() {
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
            '(' | '[' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                cur.push(c);
            }
            c if c == sep && depth == 0 => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specificity() {
        let s = Selector::parse("div.a#b:hover > span::before").unwrap();
        assert_eq!(s.specificity, (1, 2, 3));
        assert_eq!(s.pseudo_element, Some(PseudoElement::Before));
        assert_eq!(Selector::parse(":not(.a, #b)").unwrap().specificity, (1, 0, 0));
        assert_eq!(Selector::parse(":where(#a)").unwrap().specificity, (0, 0, 0));
    }

    #[test]
    fn nth() {
        assert_eq!(parse_nth("2n+1"), Some((2, 1)));
        assert_eq!(parse_nth("odd"), Some((2, 1)));
        assert_eq!(parse_nth("-n+3"), Some((-1, 3)));
        assert_eq!(parse_nth("4"), Some((0, 4)));
        assert!(nth_matches(2, 1, 3) && !nth_matches(2, 1, 4));
        assert!(nth_matches(-1, 3, 2) && !nth_matches(-1, 3, 4));
    }

    #[test]
    fn list_splitting_respects_parens() {
        assert_eq!(Selector::parse_list("a:not(.x, .y), b > c").len(), 2);
    }

    #[test]
    fn attribute_forms() {
        let s = Selector::parse("input[type=\"text\" i][data-x^=ab]").unwrap();
        assert_eq!(s.compounds[0].attrs.len(), 2);
        assert!(s.compounds[0].attrs[0].case_insensitive);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Selector::parse("> a").is_none());
        assert!(Selector::parse("a >").is_none());
        assert!(Selector::parse("").is_none());
    }
}

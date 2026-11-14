//! Property parsing and application: turns `name: value` into fields of [`ComputedStyle`].
//! Shorthands are expanded here; this is table-driven by property name, not by element type.

use crate::color::Color;
use crate::style::*;
use crate::values::*;

#[derive(Debug, Clone, Copy)]
pub struct StyleContext {
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub root_font_size: f32,
}

impl Default for StyleContext {
    fn default() -> Self {
        StyleContext { viewport_w: 800.0, viewport_h: 600.0, root_font_size: 16.0 }
    }
}

/// Properties whose values are resolved first because other lengths (`em`) depend on them.
pub fn is_font_size_property(name: &str) -> bool {
    matches!(name, "font-size" | "font")
}

pub fn is_inherited_property(name: &str) -> bool {
    matches!(
        name,
        "color"
            | "font-family"
            | "font-size"
            | "font-weight"
            | "font-style"
            | "line-height"
            | "letter-spacing"
            | "word-spacing"
            | "text-align"
            | "text-transform"
            | "white-space"
            | "visibility"
            | "cursor"
            | "caret-color"
            | "pointer-events"
            | "user-select"
            | "list-style"
            | "list-style-type"
    )
}

/// Apply one declaration. Returns `false` if the property is unknown or the value is invalid
/// (the declaration is then ignored, as CSS requires).
pub fn apply_declaration(style: &mut ComputedStyle, parent: &ComputedStyle, name: &str, value: &str, sc: &StyleContext) -> bool {
    let value = value.trim();
    match value.to_ascii_lowercase().as_str() {
        "inherit" => return copy_property(style, parent, name),
        "initial" => return copy_property(style, &ComputedStyle::default(), name),
        "unset" => {
            return if is_inherited_property(name) {
                copy_property(style, parent, name)
            } else {
                copy_property(style, &ComputedStyle::default(), name)
            }
        }
        _ => {}
    }
    // Lengths on font-size resolve against the parent's font size; everything else against our own.
    let own_fs = style.font_size;
    let lcx_own = LengthContext::new(own_fs, sc.root_font_size, sc.viewport_w, sc.viewport_h);
    let lcx_parent = LengthContext::new(parent.font_size, sc.root_font_size, sc.viewport_w, sc.viewport_h);
    apply_inner(style, parent, name, value, &lcx_own, &lcx_parent).is_some()
}

fn copy_property(dst: &mut ComputedStyle, src: &ComputedStyle, name: &str) -> bool {
    macro_rules! cp {
        ($($f:ident),+) => {{ $(dst.$f = src.$f.clone();)+ true }};
    }
    match name {
        "display" => cp!(display),
        "position" => cp!(position),
        "top" => {
            dst.inset[0] = src.inset[0];
            true
        }
        "right" => {
            dst.inset[1] = src.inset[1];
            true
        }
        "bottom" => {
            dst.inset[2] = src.inset[2];
            true
        }
        "left" => {
            dst.inset[3] = src.inset[3];
            true
        }
        "z-index" => cp!(z_index),
        "overflow-x" => cp!(overflow_x),
        "overflow-y" => cp!(overflow_y),
        "overflow" => cp!(overflow_x, overflow_y),
        "box-sizing" => cp!(box_sizing),
        "visibility" => cp!(visibility),
        "opacity" => cp!(opacity),
        "pointer-events" => cp!(pointer_events),
        "cursor" => cp!(cursor),
        "user-select" => cp!(user_select),
        "width" => cp!(width),
        "height" => cp!(height),
        "min-width" => cp!(min_width),
        "min-height" => cp!(min_height),
        "max-width" => cp!(max_width),
        "max-height" => cp!(max_height),
        "aspect-ratio" => cp!(aspect_ratio),
        "margin" => cp!(margin),
        "padding" => cp!(padding),
        "border-width" => cp!(border_width),
        "border-style" => cp!(border_style),
        "border-color" => cp!(border_color),
        "border" => cp!(border_width, border_style, border_color),
        "border-radius" => cp!(border_radius),
        "flex-direction" => cp!(flex_direction),
        "flex-wrap" => cp!(flex_wrap),
        "flex-grow" => cp!(flex_grow),
        "flex-shrink" => cp!(flex_shrink),
        "flex-basis" => cp!(flex_basis),
        "flex" => cp!(flex_grow, flex_shrink, flex_basis),
        "order" => cp!(order),
        "align-items" => cp!(align_items),
        "align-self" => cp!(align_self),
        "align-content" => cp!(align_content),
        "justify-content" => cp!(justify_content),
        "justify-items" => cp!(justify_items),
        "justify-self" => cp!(justify_self),
        "row-gap" => cp!(row_gap),
        "column-gap" => cp!(column_gap),
        "gap" => cp!(row_gap, column_gap),
        "color" => cp!(color),
        "background-color" => cp!(background_color),
        "background-image" => cp!(background_images),
        "background-size" => cp!(background_size),
        "background-repeat" => cp!(background_repeat),
        "background" => cp!(background_color, background_images, background_size, background_repeat),
        "box-shadow" => cp!(box_shadow),
        "transform" => cp!(transform),
        "transform-origin" => cp!(transform_origin),
        "object-fit" => cp!(object_fit),
        "content" => cp!(content),
        "font-family" => cp!(font_family),
        "font-size" => cp!(font_size),
        "font-weight" => cp!(font_weight),
        "font-style" => cp!(font_style),
        "line-height" => cp!(line_height),
        "letter-spacing" => cp!(letter_spacing),
        "word-spacing" => cp!(word_spacing),
        "text-align" => cp!(text_align),
        "text-decoration" | "text-decoration-line" => cp!(text_decoration),
        "text-transform" => cp!(text_transform),
        "white-space" => cp!(white_space),
        "text-overflow" => cp!(text_overflow_ellipsis),
        "caret-color" => cp!(caret_color),
        "list-style" | "list-style-type" => cp!(list_style_none),
        "transition" => cp!(transitions),
        "animation" => cp!(animations),
        "grid-template-columns" => cp!(grid_template_columns),
        "grid-template-rows" => cp!(grid_template_rows),
        "grid-auto-columns" => cp!(grid_auto_columns),
        "grid-auto-rows" => cp!(grid_auto_rows),
        "grid-auto-flow" => cp!(grid_auto_flow),
        "grid-column" => cp!(grid_column),
        "grid-row" => cp!(grid_row),
        _ => false,
    }
}

fn box4<T: Copy>(vals: &[T]) -> Option<[T; 4]> {
    match vals.len() {
        1 => Some([vals[0]; 4]),
        2 => Some([vals[0], vals[1], vals[0], vals[1]]),
        3 => Some([vals[0], vals[1], vals[2], vals[1]]),
        4 => Some([vals[0], vals[1], vals[2], vals[3]]),
        _ => None,
    }
}

fn dims(value: &str, cx: &LengthContext, allow_auto: bool, allow_negative: bool) -> Option<Vec<Dim>> {
    let parts = split_ws(value);
    let mut out = Vec::new();
    for p in &parts {
        let d = parse_length(p, cx)?;
        if matches!(d, Dim::Auto) && !allow_auto {
            return None;
        }
        if !allow_negative {
            match d {
                Dim::Px(v) | Dim::Percent(v) if v < 0.0 => return None,
                _ => {}
            }
        }
        out.push(d);
    }
    Some(out)
}

fn parse_border_width(s: &str, cx: &LengthContext) -> Option<f32> {
    match s.to_ascii_lowercase().as_str() {
        "thin" => Some(1.0),
        "medium" => Some(3.0),
        "thick" => Some(5.0),
        _ => parse_px(s, cx).filter(|v| *v >= 0.0),
    }
}

fn parse_number(s: &str) -> Option<f32> {
    let (n, u) = split_number(s)?;
    if u.is_empty() {
        Some(n)
    } else {
        None
    }
}

fn parse_angle(s: &str) -> Option<f32> {
    let (n, u) = split_number(s)?;
    match u.as_str() {
        "deg" | "" => Some(n.to_radians()),
        "rad" => Some(n),
        "grad" => Some(n * std::f32::consts::PI / 200.0),
        "turn" => Some(n * std::f32::consts::TAU),
        _ => None,
    }
}

fn apply_inner(
    st: &mut ComputedStyle,
    parent: &ComputedStyle,
    name: &str,
    v: &str,
    cx: &LengthContext,
    pcx: &LengthContext,
) -> Option<()> {
    match name {
        "display" => st.display = Display::parse(v)?,
        "position" => st.position = Position::parse(v)?,
        "top" | "right" | "bottom" | "left" => {
            let i = ["top", "right", "bottom", "left"].iter().position(|n| *n == name)?;
            st.inset[i] = parse_length(v, cx)?;
        }
        "inset" => {
            let d = dims(v, cx, true, true)?;
            st.inset = box4(&d)?;
        }
        "z-index" => {
            st.z_index = if v.eq_ignore_ascii_case("auto") { None } else { Some(v.parse::<i32>().ok()?) };
        }
        "overflow" => {
            let parts: Vec<_> = split_ws(v).iter().map(|p| Overflow::parse(p)).collect::<Option<_>>()?;
            match parts.len() {
                1 => {
                    st.overflow_x = parts[0];
                    st.overflow_y = parts[0];
                }
                2 => {
                    st.overflow_x = parts[0];
                    st.overflow_y = parts[1];
                }
                _ => return None,
            }
        }
        "overflow-x" => st.overflow_x = Overflow::parse(v)?,
        "overflow-y" => st.overflow_y = Overflow::parse(v)?,
        "box-sizing" => st.box_sizing = BoxSizing::parse(v)?,
        "visibility" => st.visibility = Visibility::parse(v)?,
        "opacity" => {
            let n = if let Some(p) = v.strip_suffix('%') { p.trim().parse::<f32>().ok()? / 100.0 } else { parse_number(v)? };
            st.opacity = n.clamp(0.0, 1.0);
        }
        "pointer-events" => st.pointer_events = PointerEvents::parse(v)?,
        "cursor" => {
            // The first recognised keyword in the fallback list wins.
            st.cursor = split_commas(v).iter().find_map(|c| Cursor::parse(c))?;
        }
        "user-select" => st.user_select = UserSelect::parse(v)?,
        "width" => st.width = non_negative(parse_length(v, cx)?)?,
        "height" => st.height = non_negative(parse_length(v, cx)?)?,
        "min-width" => st.min_width = non_negative(parse_length(v, cx)?)?,
        "min-height" => st.min_height = non_negative(parse_length(v, cx)?)?,
        "max-width" => st.max_width = if v.eq_ignore_ascii_case("none") { Dim::Auto } else { non_negative(parse_length(v, cx)?)? },
        "max-height" => st.max_height = if v.eq_ignore_ascii_case("none") { Dim::Auto } else { non_negative(parse_length(v, cx)?)? },
        "aspect-ratio" => {
            st.aspect_ratio = if v.eq_ignore_ascii_case("auto") {
                None
            } else if let Some((a, b)) = v.split_once('/') {
                let (a, b) = (parse_number(a.trim())?, parse_number(b.trim())?);
                if b == 0.0 {
                    return None;
                }
                Some(a / b)
            } else {
                Some(parse_number(v)?)
            };
        }
        "margin" => st.margin = box4(&dims(v, cx, true, true)?)?,
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            let i = ["top", "right", "bottom", "left"].iter().position(|n| name.ends_with(n))?;
            st.margin[i] = parse_length(v, cx)?;
        }
        "padding" => st.padding = box4(&dims(v, cx, false, false)?)?,
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            let i = ["top", "right", "bottom", "left"].iter().position(|n| name.ends_with(n))?;
            st.padding[i] = non_negative(parse_length(v, cx)?).filter(|d| !matches!(d, Dim::Auto))?;
        }
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let sides: Vec<usize> = match name {
                "border" => vec![0, 1, 2, 3],
                "border-top" => vec![0],
                "border-right" => vec![1],
                "border-bottom" => vec![2],
                _ => vec![3],
            };
            let (mut w, mut s, mut c) = (None, None, None);
            for tok in split_ws(v) {
                if let Some(x) = BorderStyle::parse(&tok) {
                    s = Some(x);
                } else if let Some(x) = parse_border_width(&tok, cx) {
                    w = Some(x);
                } else if let Some(x) = Color::parse(&tok, st.color) {
                    c = Some(x);
                } else {
                    return None;
                }
            }
            // Per spec, omitted style resets to none; omitted width to medium; omitted color to currentcolor.
            let style = s.unwrap_or(BorderStyle::None);
            let width = if matches!(style, BorderStyle::None | BorderStyle::Hidden) { 0.0 } else { w.unwrap_or(3.0) };
            let color = c.unwrap_or(st.color);
            for i in sides {
                st.border_width[i] = width;
                st.border_style[i] = style;
                st.border_color[i] = color;
            }
        }
        "border-width" => {
            let parts: Vec<f32> = split_ws(v).iter().map(|p| parse_border_width(p, cx)).collect::<Option<_>>()?;
            let w = box4(&parts)?;
            for i in 0..4 {
                st.border_width[i] = if matches!(st.border_style[i], BorderStyle::None | BorderStyle::Hidden) { w[i] } else { w[i] };
            }
        }
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            let i = side_index(name)?;
            st.border_width[i] = parse_border_width(v, cx)?;
        }
        "border-style" => {
            let parts: Vec<BorderStyle> = split_ws(v).iter().map(|p| BorderStyle::parse(p)).collect::<Option<_>>()?;
            st.border_style = box4(&parts)?;
        }
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => {
            st.border_style[side_index(name)?] = BorderStyle::parse(v)?;
        }
        "border-color" => {
            let cur = st.color;
            let parts: Vec<Color> = split_ws(v).iter().map(|p| Color::parse(p, cur)).collect::<Option<_>>()?;
            st.border_color = box4(&parts)?;
        }
        "border-top-color" | "border-right-color" | "border-bottom-color" | "border-left-color" => {
            st.border_color[side_index(name)?] = Color::parse(v, st.color)?;
        }
        "border-radius" => {
            let first = v.split('/').next()?;
            let d = dims(first, cx, false, false)?;
            // Corner order for a 4-value list: TL TR BR BL; box4 maps 2/3-value forms correctly.
            st.border_radius = box4(&d)?;
        }
        "border-top-left-radius" | "border-top-right-radius" | "border-bottom-right-radius" | "border-bottom-left-radius" => {
            let i = match name {
                "border-top-left-radius" => 0,
                "border-top-right-radius" => 1,
                "border-bottom-right-radius" => 2,
                _ => 3,
            };
            st.border_radius[i] = non_negative(parse_length(split_ws(v).first()?, cx)?)?;
        }
        "outline" => {
            let (mut w, mut c) = (None, None);
            let mut visible = true;
            for tok in split_ws(v) {
                if let Some(s) = BorderStyle::parse(&tok) {
                    visible = !matches!(s, BorderStyle::None | BorderStyle::Hidden);
                } else if let Some(x) = parse_border_width(&tok, cx) {
                    w = Some(x);
                } else if let Some(x) = Color::parse(&tok, st.color) {
                    c = Some(x);
                } else {
                    return None;
                }
            }
            st.outline_width = if visible { w.unwrap_or(3.0) } else { 0.0 };
            st.outline_color = c.unwrap_or(st.color);
        }
        "outline-width" => st.outline_width = parse_border_width(v, cx)?,
        "outline-color" => st.outline_color = Color::parse(v, st.color)?,
        "outline-offset" => st.outline_offset = parse_px(v, cx)?,

        // ----- flex / alignment -----
        "flex-direction" => st.flex_direction = FlexDirection::parse(v)?,
        "flex-wrap" => st.flex_wrap = FlexWrap::parse(v)?,
        "flex-flow" => {
            for tok in split_ws(v) {
                if let Some(d) = FlexDirection::parse(&tok) {
                    st.flex_direction = d;
                } else if let Some(w) = FlexWrap::parse(&tok) {
                    st.flex_wrap = w;
                } else {
                    return None;
                }
            }
        }
        "flex-grow" => st.flex_grow = parse_number(v).filter(|n| *n >= 0.0)?,
        "flex-shrink" => st.flex_shrink = parse_number(v).filter(|n| *n >= 0.0)?,
        "flex-basis" => st.flex_basis = if v.eq_ignore_ascii_case("content") { Dim::Auto } else { parse_length(v, cx)? },
        "flex" => {
            let lower = v.to_ascii_lowercase();
            match lower.as_str() {
                "none" => {
                    st.flex_grow = 0.0;
                    st.flex_shrink = 0.0;
                    st.flex_basis = Dim::Auto;
                }
                "auto" => {
                    st.flex_grow = 1.0;
                    st.flex_shrink = 1.0;
                    st.flex_basis = Dim::Auto;
                }
                "initial" => {
                    st.flex_grow = 0.0;
                    st.flex_shrink = 1.0;
                    st.flex_basis = Dim::Auto;
                }
                _ => {
                    let toks = split_ws(v);
                    let mut nums = Vec::new();
                    let mut basis = None;
                    for t in &toks {
                        if let Some(n) = parse_number(t) {
                            nums.push(n);
                        } else if let Some(d) = parse_length(t, cx) {
                            basis = Some(d);
                        } else {
                            return None;
                        }
                    }
                    if nums.len() > 2 || nums.iter().any(|n| *n < 0.0) {
                        return None;
                    }
                    st.flex_grow = nums.first().copied().unwrap_or(1.0);
                    st.flex_shrink = nums.get(1).copied().unwrap_or(1.0);
                    // `flex: 1` means basis 0%, not auto.
                    st.flex_basis = basis.unwrap_or(Dim::Percent(0.0));
                }
            }
        }
        "order" => st.order = v.parse().ok()?,
        "align-items" => st.align_items = Align::parse(v)?,
        "align-self" => st.align_self = Align::parse(v)?,
        "align-content" => st.align_content = Align::parse(v)?,
        "justify-content" => st.justify_content = Align::parse(v)?,
        "justify-items" => st.justify_items = Align::parse(v)?,
        "justify-self" => st.justify_self = Align::parse(v)?,
        "place-items" => {
            let p = split_ws(v);
            st.align_items = Align::parse(p.first()?)?;
            st.justify_items = Align::parse(p.get(1).unwrap_or(&p[0]))?;
        }
        "place-content" => {
            let p = split_ws(v);
            st.align_content = Align::parse(p.first()?)?;
            st.justify_content = Align::parse(p.get(1).unwrap_or(&p[0]))?;
        }
        "place-self" => {
            let p = split_ws(v);
            st.align_self = Align::parse(p.first()?)?;
            st.justify_self = Align::parse(p.get(1).unwrap_or(&p[0]))?;
        }
        "gap" | "grid-gap" => {
            let d = dims(v, cx, false, false)?;
            match d.len() {
                1 => {
                    st.row_gap = d[0];
                    st.column_gap = d[0];
                }
                2 => {
                    st.row_gap = d[0];
                    st.column_gap = d[1];
                }
                _ => return None,
            }
        }
        "row-gap" | "grid-row-gap" => st.row_gap = non_negative(parse_length(v, cx)?)?,
        "column-gap" | "grid-column-gap" => st.column_gap = non_negative(parse_length(v, cx)?)?,

        // ----- grid -----
        "grid-template-columns" => st.grid_template_columns = parse_track_list(v, cx)?,
        "grid-template-rows" => st.grid_template_rows = parse_track_list(v, cx)?,
        "grid-template" => {
            let (rows, cols) = v.split_once('/')?;
            st.grid_template_rows = parse_track_list(rows, cx)?;
            st.grid_template_columns = parse_track_list(cols, cx)?;
        }
        "grid-auto-columns" => st.grid_auto_columns = split_ws(v).iter().map(|t| parse_track(t, cx)).collect::<Option<_>>()?,
        "grid-auto-rows" => st.grid_auto_rows = split_ws(v).iter().map(|t| parse_track(t, cx)).collect::<Option<_>>()?,
        "grid-auto-flow" => st.grid_auto_flow = GridAutoFlow::parse(v)?,
        "grid-column" => st.grid_column = parse_grid_placement(v)?,
        "grid-row" => st.grid_row = parse_grid_placement(v)?,
        "grid-column-start" => st.grid_column.0 = parse_grid_line(v)?,
        "grid-column-end" => st.grid_column.1 = parse_grid_line(v)?,
        "grid-row-start" => st.grid_row.0 = parse_grid_line(v)?,
        "grid-row-end" => st.grid_row.1 = parse_grid_line(v)?,

        // ----- paint -----
        "color" => st.color = Color::parse(v, parent.color)?,
        "caret-color" => st.caret_color = if v.eq_ignore_ascii_case("auto") { None } else { Some(Color::parse(v, st.color)?) },
        "background-color" => st.background_color = Color::parse(v, st.color)?,
        "background-image" => st.background_images = parse_background_images(v, st.color)?,
        "background-size" => st.background_size = parse_bg_size(v, cx)?,
        "background-repeat" => st.background_repeat = BackgroundRepeat::parse(v)?,
        "background" => {
            let layers = split_commas(v);
            let mut images = Vec::new();
            let mut color = Color::TRANSPARENT;
            let mut size = BackgroundSize::Auto;
            let mut repeat = BackgroundRepeat::Repeat;
            for (li, layer) in layers.iter().enumerate() {
                let mut size_tokens = Vec::new();
                for tok in split_ws(layer) {
                    if let Some(img) = parse_background_image(&tok, st.color) {
                        images.push(img);
                    } else if let Some(r) = BackgroundRepeat::parse(&tok) {
                        repeat = r;
                    } else if tok == "/" {
                        continue;
                    } else if let Some(c) = Color::parse(&tok, st.color) {
                        if li == layers.len() - 1 {
                            color = c;
                        }
                    } else if matches!(tok.to_ascii_lowercase().as_str(), "cover" | "contain") {
                        size_tokens.push(tok);
                    } else if matches!(tok.to_ascii_lowercase().as_str(), "center" | "top" | "bottom" | "left" | "right" | "none" | "scroll" | "fixed" | "local" | "border-box" | "padding-box" | "content-box") || parse_length(&tok, cx).is_some() {
                        // Position / attachment / box keywords: accepted, position not yet modelled.
                    } else {
                        return None;
                    }
                }
                if let Some(s) = size_tokens.first() {
                    size = parse_bg_size(s, cx)?;
                }
            }
            st.background_color = color;
            st.background_images = images;
            st.background_size = size;
            st.background_repeat = repeat;
        }
        "box-shadow" => st.box_shadow = parse_shadows(v, st.color, cx)?,
        "transform" => st.transform = parse_transform(v, cx)?,
        "transform-origin" => {
            let p = split_ws(v);
            let axis = |t: &str, horizontal: bool| -> Option<Dim> {
                Some(match t.to_ascii_lowercase().as_str() {
                    "left" if horizontal => Dim::Percent(0.0),
                    "right" if horizontal => Dim::Percent(100.0),
                    "top" if !horizontal => Dim::Percent(0.0),
                    "bottom" if !horizontal => Dim::Percent(100.0),
                    "center" => Dim::Percent(50.0),
                    _ => parse_length(t, cx)?,
                })
            };
            st.transform_origin = (axis(p.first()?, true)?, axis(p.get(1).map(|s| s.as_str()).unwrap_or("center"), false)?);
        }
        "object-fit" => st.object_fit = ObjectFit::parse(v)?,
        "content" => {
            let t = v.trim();
            st.content = if matches!(t.to_ascii_lowercase().as_str(), "none" | "normal") {
                None
            } else if (t.starts_with('"') && t.ends_with('"') || t.starts_with('\'') && t.ends_with('\'')) && t.len() >= 2 {
                Some(t[1..t.len() - 1].to_string())
            } else {
                return None;
            };
        }

        // ----- text -----
        "font-family" => {
            let fams: Vec<String> = split_commas(v)
                .iter()
                .map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
                .filter(|f| !f.is_empty())
                .collect();
            if fams.is_empty() {
                return None;
            }
            st.font_family = fams.into();
        }
        "font-size" => st.font_size = parse_font_size(v, pcx, parent.font_size)?,
        "font-weight" => st.font_weight = parse_font_weight(v, parent.font_weight)?,
        "font-style" => st.font_style = FontStyle::parse(v)?,
        "font" => {
            // [style] [weight] size[/line-height] family
            let toks = split_ws(v);
            let mut i = 0;
            let mut style = FontStyle::Normal;
            let mut weight = 400;
            while i < toks.len() {
                let t = &toks[i];
                if let Some(s) = FontStyle::parse(t).filter(|s| *s != FontStyle::Normal) {
                    style = s;
                } else if let Some(w) = parse_font_weight(t, parent.font_weight).filter(|_| t.as_str() != "normal") {
                    weight = w;
                } else if matches!(t.to_ascii_lowercase().as_str(), "normal" | "small-caps") {
                } else {
                    break;
                }
                i += 1;
            }
            let size_tok = toks.get(i)?;
            let (size_s, lh) = match size_tok.split_once('/') {
                Some((a, b)) => (a.to_string(), Some(b.to_string())),
                None => (size_tok.clone(), None),
            };
            let size = parse_font_size(&size_s, pcx, parent.font_size)?;
            let family_str = toks[i + 1..].join(" ");
            if family_str.is_empty() {
                return None;
            }
            st.font_style = style;
            st.font_weight = weight;
            st.font_size = size;
            if let Some(lh) = lh {
                st.line_height = parse_line_height(&lh, &LengthContext { font_size: size, ..*cx })?;
            }
            let fams: Vec<String> = split_commas(&family_str).iter().map(|f| f.trim_matches(|c| c == '"' || c == '\'').to_string()).collect();
            st.font_family = fams.into();
        }
        "line-height" => st.line_height = parse_line_height(v, cx)?,
        "letter-spacing" => st.letter_spacing = if v.eq_ignore_ascii_case("normal") { 0.0 } else { parse_px(v, cx)? },
        "word-spacing" => st.word_spacing = if v.eq_ignore_ascii_case("normal") { 0.0 } else { parse_px(v, cx)? },
        "text-align" => st.text_align = TextAlign::parse(v)?,
        "text-decoration" | "text-decoration-line" => {
            let mut d = TextDecoration::default();
            for t in split_ws(v) {
                match t.to_ascii_lowercase().as_str() {
                    "underline" => d.underline = true,
                    "line-through" => d.line_through = true,
                    "overline" => d.overline = true,
                    "none" => {}
                    // style / color / thickness parts of the shorthand are accepted but unused.
                    "solid" | "dotted" | "dashed" | "wavy" | "double" => {}
                    t if Color::parse(t, st.color).is_some() || parse_length(t, cx).is_some() => {}
                    _ => return None,
                }
            }
            st.text_decoration = d;
        }
        "text-transform" => st.text_transform = TextTransform::parse(v)?,
        "white-space" => st.white_space = WhiteSpace::parse(v)?,
        "list-style" | "list-style-type" => st.list_style_none = v.split_whitespace().any(|t| t.eq_ignore_ascii_case("none")),
        "text-overflow" => st.text_overflow_ellipsis = matches!(v.to_ascii_lowercase().as_str(), "ellipsis"),

        // ----- motion -----
        "transition" => st.transitions = parse_transitions(v)?,
        "transition-property" | "transition-duration" | "transition-timing-function" | "transition-delay" => {
            let list = split_commas(v);
            if st.transitions.is_empty() {
                st.transitions = vec![Transition { property: "all".into(), duration: 0.0, delay: 0.0, timing: TimingFunction::ease() }];
            }
            while st.transitions.len() < list.len() {
                let last = st.transitions.last().cloned()?;
                st.transitions.push(last);
            }
            for (i, item) in list.iter().enumerate() {
                let t = &mut st.transitions[i];
                match name {
                    "transition-property" => t.property = item.to_ascii_lowercase(),
                    "transition-duration" => t.duration = parse_time(item)?,
                    "transition-delay" => t.delay = parse_time(item)?,
                    _ => t.timing = parse_timing(item)?,
                }
            }
        }
        "animation" => st.animations = parse_animations(v)?,
        "animation-name" => {
            let names: Vec<String> = split_commas(v);
            if names.len() == 1 && names[0].eq_ignore_ascii_case("none") {
                st.animations.clear();
            } else {
                for (i, n) in names.iter().enumerate() {
                    if i >= st.animations.len() {
                        st.animations.push(AnimationSpec {
                            name: n.clone(),
                            duration: 0.0,
                            delay: 0.0,
                            timing: TimingFunction::ease(),
                            iterations: 1.0,
                            direction: AnimationDirection::Normal,
                            fill: FillMode::None,
                        });
                    } else {
                        st.animations[i].name = n.clone();
                    }
                }
            }
        }
        "animation-duration" | "animation-delay" | "animation-timing-function" | "animation-iteration-count" | "animation-direction" | "animation-fill-mode" => {
            for (i, item) in split_commas(v).iter().enumerate() {
                let a = st.animations.get_mut(i)?;
                match name {
                    "animation-duration" => a.duration = parse_time(item)?,
                    "animation-delay" => a.delay = parse_time(item)?,
                    "animation-timing-function" => a.timing = parse_timing(item)?,
                    "animation-iteration-count" => {
                        a.iterations = if item.eq_ignore_ascii_case("infinite") { f32::INFINITY } else { parse_number(item)? }
                    }
                    "animation-direction" => a.direction = parse_direction(item)?,
                    _ => a.fill = parse_fill(item)?,
                }
            }
        }
        _ => return None,
    }
    Some(())
}

fn non_negative(d: Dim) -> Option<Dim> {
    match d {
        Dim::Px(v) | Dim::Percent(v) if v < 0.0 => None,
        d => Some(d),
    }
}

fn side_index(name: &str) -> Option<usize> {
    ["top", "right", "bottom", "left"].iter().position(|s| name.contains(&format!("-{s}")))
}

fn parse_font_size(v: &str, pcx: &LengthContext, parent_size: f32) -> Option<f32> {
    let lower = v.to_ascii_lowercase();
    let kw = match lower.as_str() {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "smaller" => Some(parent_size / 1.2),
        "larger" => Some(parent_size * 1.2),
        _ => None,
    };
    if kw.is_some() {
        return kw;
    }
    match parse_length(v, pcx)? {
        Dim::Px(p) if p >= 0.0 => Some(p),
        Dim::Percent(p) if p >= 0.0 => Some(parent_size * p / 100.0),
        _ => None,
    }
}

fn parse_font_weight(v: &str, parent: u16) -> Option<u16> {
    match v.to_ascii_lowercase().as_str() {
        "normal" => Some(400),
        "bold" => Some(700),
        "bolder" => Some(if parent < 400 { 400 } else if parent < 600 { 700 } else { 900 }),
        "lighter" => Some(if parent > 750 { 700 } else if parent > 550 { 400 } else { 100 }),
        n => n.parse::<u16>().ok().filter(|w| (1..=1000).contains(w)),
    }
}

fn parse_line_height(v: &str, cx: &LengthContext) -> Option<LineHeight> {
    if v.eq_ignore_ascii_case("normal") {
        return Some(LineHeight::Normal);
    }
    if let Some(n) = parse_number(v) {
        return (n >= 0.0).then_some(LineHeight::Number(n));
    }
    match parse_length(v, cx)? {
        Dim::Px(p) if p >= 0.0 => Some(LineHeight::Px(p)),
        Dim::Percent(p) if p >= 0.0 => Some(LineHeight::Px(cx.font_size * p / 100.0)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// grid
// ---------------------------------------------------------------------------------------------

fn parse_track(t: &str, cx: &LengthContext) -> Option<TrackSize> {
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "auto" => return Some(TrackSize::Auto),
        "min-content" => return Some(TrackSize::MinContent),
        "max-content" => return Some(TrackSize::MaxContent),
        _ => {}
    }
    if let Some(inner) = strip_func(t, "minmax") {
        let parts = split_commas(inner);
        if parts.len() != 2 {
            return None;
        }
        return Some(TrackSize::MinMax(Box::new(parse_track(&parts[0], cx)?), Box::new(parse_track(&parts[1], cx)?)));
    }
    if let Some(inner) = strip_func(t, "fit-content") {
        return match parse_length(inner, cx)? {
            Dim::Px(p) => Some(TrackSize::FitContent(p)),
            _ => None,
        };
    }
    if let Some(n) = lower.strip_suffix("fr") {
        return n.trim().parse::<f32>().ok().filter(|n| *n >= 0.0).map(TrackSize::Fr);
    }
    match parse_length(t, cx)? {
        Dim::Px(p) => Some(TrackSize::Px(p)),
        Dim::Percent(p) => Some(TrackSize::Percent(p)),
        Dim::Auto => Some(TrackSize::Auto),
    }
}

fn parse_track_list(v: &str, cx: &LengthContext) -> Option<Vec<TrackItem>> {
    if v.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for tok in split_ws(v) {
        if tok.starts_with('[') && tok.ends_with(']') {
            continue; // named lines are accepted and ignored
        }
        if let Some(inner) = strip_func(&tok, "repeat") {
            let (count, rest) = inner.split_once(',')?;
            let count = match count.trim().to_ascii_lowercase().as_str() {
                "auto-fill" => RepeatCount::AutoFill,
                "auto-fit" => RepeatCount::AutoFit,
                n => RepeatCount::Count(n.parse().ok().filter(|n| *n > 0)?),
            };
            let tracks: Vec<TrackSize> = split_ws(rest).iter().filter(|t| !t.starts_with('[')).map(|t| parse_track(t, cx)).collect::<Option<_>>()?;
            if tracks.is_empty() {
                return None;
            }
            out.push(TrackItem::Repeat(count, tracks));
        } else {
            out.push(TrackItem::Single(parse_track(&tok, cx)?));
        }
    }
    Some(out)
}

fn parse_grid_line(v: &str) -> Option<GridLine> {
    let v = v.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(GridLine::Auto);
    }
    if let Some(n) = v.strip_prefix("span") {
        return n.trim().parse::<u16>().ok().filter(|n| *n > 0).map(GridLine::Span);
    }
    v.parse::<i16>().ok().filter(|n| *n != 0).map(GridLine::Line)
}

fn parse_grid_placement(v: &str) -> Option<(GridLine, GridLine)> {
    match v.split_once('/') {
        Some((a, b)) => Some((parse_grid_line(a)?, parse_grid_line(b)?)),
        None => {
            let a = parse_grid_line(v)?;
            Some((a, GridLine::Auto))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// backgrounds, shadows, transforms
// ---------------------------------------------------------------------------------------------

fn parse_bg_size(v: &str, cx: &LengthContext) -> Option<BackgroundSize> {
    let lower = v.to_ascii_lowercase();
    match lower.as_str() {
        "auto" => Some(BackgroundSize::Auto),
        "cover" => Some(BackgroundSize::Cover),
        "contain" => Some(BackgroundSize::Contain),
        _ => {
            let p = split_ws(v);
            let a = parse_length(p.first()?, cx)?;
            let b = p.get(1).map(|s| parse_length(s, cx)).unwrap_or(Some(Dim::Auto))?;
            Some(BackgroundSize::Explicit(a, b))
        }
    }
}

fn parse_background_images(v: &str, cur: Color) -> Option<Vec<BackgroundImage>> {
    if v.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    split_commas(v).iter().map(|l| parse_background_image(l, cur)).collect()
}

fn parse_background_image(v: &str, cur: Color) -> Option<BackgroundImage> {
    let v = v.trim();
    if let Some(inner) = strip_func(v, "url") {
        return Some(BackgroundImage::Url(inner.trim().trim_matches(|c| c == '"' || c == '\'').to_string()));
    }
    for (fname, linear) in [("linear-gradient", true), ("repeating-linear-gradient", true), ("radial-gradient", false)] {
        if let Some(inner) = strip_func(v, fname) {
            let mut parts = split_commas(inner);
            let mut angle = std::f32::consts::PI; // default: to bottom
            if let Some(first) = parts.first() {
                let l = first.to_ascii_lowercase();
                if linear && l.starts_with("to ") {
                    angle = match l[3..].trim() {
                        "top" => 0.0,
                        "right" => std::f32::consts::FRAC_PI_2,
                        "bottom" => std::f32::consts::PI,
                        "left" => 3.0 * std::f32::consts::FRAC_PI_2,
                        "top right" | "right top" => std::f32::consts::FRAC_PI_4,
                        "bottom right" | "right bottom" => 3.0 * std::f32::consts::FRAC_PI_4,
                        "bottom left" | "left bottom" => 5.0 * std::f32::consts::FRAC_PI_4,
                        "top left" | "left top" => 7.0 * std::f32::consts::FRAC_PI_4,
                        _ => return None,
                    };
                    parts.remove(0);
                } else if linear && parse_angle(first).is_some() && split_number(first).is_some_and(|(_, u)| !u.is_empty()) {
                    angle = parse_angle(first)?;
                    parts.remove(0);
                } else if !linear && (l.starts_with("circle") || l.starts_with("ellipse") || l.starts_with("closest") || l.starts_with("farthest")) {
                    parts.remove(0);
                }
            }
            let mut stops = Vec::new();
            for p in parts {
                let toks = split_ws(&p);
                let color = Color::parse(toks.first()?, cur)?;
                let pos = toks.get(1).and_then(|t| t.strip_suffix('%')).and_then(|n| n.trim().parse::<f32>().ok()).map(|n| n / 100.0);
                stops.push(GradientStop { color, pos });
            }
            if stops.len() < 2 {
                return None;
            }
            return Some(if linear { BackgroundImage::LinearGradient { angle, stops } } else { BackgroundImage::RadialGradient { stops } });
        }
    }
    None
}

fn parse_shadows(v: &str, cur: Color, cx: &LengthContext) -> Option<Vec<Shadow>> {
    if v.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for layer in split_commas(v) {
        let mut nums = Vec::new();
        let mut color = None;
        let mut inset = false;
        for t in split_ws(&layer) {
            if t.eq_ignore_ascii_case("inset") {
                inset = true;
            } else if let Some(px) = parse_px(&t, cx) {
                nums.push(px);
            } else if let Some(c) = Color::parse(&t, cur) {
                color = Some(c);
            } else {
                return None;
            }
        }
        if nums.len() < 2 || nums.len() > 4 {
            return None;
        }
        out.push(Shadow {
            dx: nums[0],
            dy: nums[1],
            blur: nums.get(2).copied().unwrap_or(0.0).max(0.0),
            spread: nums.get(3).copied().unwrap_or(0.0),
            color: color.unwrap_or(cur),
            inset,
        });
    }
    Some(out)
}

fn parse_transform(v: &str, cx: &LengthContext) -> Option<Vec<TransformOp>> {
    if v.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for f in split_ws(v) {
        let open = f.find('(')?;
        let name = f[..open].to_ascii_lowercase();
        let inner = f[open + 1..].strip_suffix(')')?;
        let args = split_commas(inner);
        let num = |i: usize| args.get(i).and_then(|a| parse_number(a));
        let len = |i: usize| args.get(i).and_then(|a| parse_length(a, cx));
        out.push(match name.as_str() {
            "translate" => TransformOp::Translate(len(0)?, len(1).unwrap_or(Dim::Px(0.0))),
            "translatex" => TransformOp::Translate(len(0)?, Dim::Px(0.0)),
            "translatey" => TransformOp::Translate(Dim::Px(0.0), len(0)?),
            "scale" => {
                let x = num(0)?;
                TransformOp::Scale(x, num(1).unwrap_or(x))
            }
            "scalex" => TransformOp::Scale(num(0)?, 1.0),
            "scaley" => TransformOp::Scale(1.0, num(0)?),
            "rotate" | "rotatez" => TransformOp::Rotate(parse_angle(args.first()?)?),
            "skew" => TransformOp::Skew(parse_angle(args.first()?)?, args.get(1).and_then(|a| parse_angle(a)).unwrap_or(0.0)),
            "skewx" => TransformOp::Skew(parse_angle(args.first()?)?, 0.0),
            "skewy" => TransformOp::Skew(0.0, parse_angle(args.first()?)?),
            "matrix" => {
                if args.len() != 6 {
                    return None;
                }
                let mut m = [0.0; 6];
                for (i, a) in args.iter().enumerate() {
                    m[i] = parse_number(a)?;
                }
                TransformOp::Matrix(m)
            }
            _ => return None,
        });
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// motion
// ---------------------------------------------------------------------------------------------

pub fn parse_timing(v: &str) -> Option<TimingFunction> {
    let v = v.trim();
    let kind = match v.to_ascii_lowercase().as_str() {
        "ease" => TimingKind::CubicBezier(0.25, 0.1, 0.25, 1.0),
        "ease-in" => TimingKind::CubicBezier(0.42, 0.0, 1.0, 1.0),
        "ease-out" => TimingKind::CubicBezier(0.0, 0.0, 0.58, 1.0),
        "ease-in-out" => TimingKind::CubicBezier(0.42, 0.0, 0.58, 1.0),
        "linear" => TimingKind::Linear,
        "step-start" => TimingKind::Steps(1, true),
        "step-end" => TimingKind::Steps(1, false),
        _ => {
            if let Some(inner) = strip_func(v, "cubic-bezier") {
                let n: Vec<f32> = split_commas(inner).iter().map(|x| x.parse().ok()).collect::<Option<_>>()?;
                if n.len() != 4 {
                    return None;
                }
                TimingKind::CubicBezier(n[0], n[1], n[2], n[3])
            } else if let Some(inner) = strip_func(v, "steps") {
                let p = split_commas(inner);
                let n: u32 = p.first()?.parse().ok().filter(|n| *n > 0)?;
                TimingKind::Steps(n, p.get(1).map(|s| s.eq_ignore_ascii_case("start") || s == "jump-start").unwrap_or(false))
            } else {
                return None;
            }
        }
    };
    Some(TimingFunction { kind })
}

fn parse_direction(s: &str) -> Option<AnimationDirection> {
    match s.to_ascii_lowercase().as_str() {
        "normal" => Some(AnimationDirection::Normal),
        "reverse" => Some(AnimationDirection::Reverse),
        "alternate" => Some(AnimationDirection::Alternate),
        "alternate-reverse" => Some(AnimationDirection::AlternateReverse),
        _ => None,
    }
}

fn parse_fill(s: &str) -> Option<FillMode> {
    match s.to_ascii_lowercase().as_str() {
        "none" => Some(FillMode::None),
        "forwards" => Some(FillMode::Forwards),
        "backwards" => Some(FillMode::Backwards),
        "both" => Some(FillMode::Both),
        _ => None,
    }
}

fn parse_transitions(v: &str) -> Option<Vec<Transition>> {
    if v.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for item in split_commas(v) {
        let mut t = Transition { property: "all".into(), duration: 0.0, delay: 0.0, timing: TimingFunction::ease() };
        let mut times = 0;
        for tok in split_ws(&item) {
            if let Some(time) = parse_time(&tok) {
                if times == 0 {
                    t.duration = time;
                } else {
                    t.delay = time;
                }
                times += 1;
            } else if let Some(tf) = parse_timing(&tok) {
                t.timing = tf;
            } else {
                t.property = tok.to_ascii_lowercase();
            }
        }
        out.push(t);
    }
    Some(out)
}

fn parse_animations(v: &str) -> Option<Vec<AnimationSpec>> {
    if v.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for item in split_commas(v) {
        let mut a = AnimationSpec {
            name: String::new(),
            duration: 0.0,
            delay: 0.0,
            timing: TimingFunction::ease(),
            iterations: 1.0,
            direction: AnimationDirection::Normal,
            fill: FillMode::None,
        };
        let mut times = 0;
        for tok in split_ws(&item) {
            if let Some(time) = parse_time(&tok) {
                if times == 0 {
                    a.duration = time;
                } else {
                    a.delay = time;
                }
                times += 1;
            } else if let Some(tf) = parse_timing(&tok) {
                a.timing = tf;
            } else if tok.eq_ignore_ascii_case("infinite") {
                a.iterations = f32::INFINITY;
            } else if let Some(n) = parse_number(&tok) {
                a.iterations = n;
            } else if let Some(d) = parse_direction(&tok) {
                a.direction = d;
            } else if let Some(f) = parse_fill(&tok).filter(|_| !tok.eq_ignore_ascii_case("none")) {
                a.fill = f;
            } else if matches!(tok.to_ascii_lowercase().as_str(), "running" | "paused") {
            } else {
                a.name = tok;
            }
        }
        if a.name.is_empty() {
            return None;
        }
        out.push(a);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(decl: &[(&str, &str)]) -> ComputedStyle {
        let parent = ComputedStyle::default();
        let mut st = ComputedStyle::inherit_from(&parent);
        let sc = StyleContext::default();
        for (n, v) in decl {
            apply_declaration(&mut st, &parent, n, v, &sc);
        }
        st
    }

    #[test]
    fn box_shorthands() {
        let s = apply(&[("margin", "1px 2px 3px"), ("padding", "4px 8px")]);
        assert_eq!(s.margin, [Dim::Px(1.0), Dim::Px(2.0), Dim::Px(3.0), Dim::Px(2.0)]);
        assert_eq!(s.padding, [Dim::Px(4.0), Dim::Px(8.0), Dim::Px(4.0), Dim::Px(8.0)]);
    }

    #[test]
    fn border_shorthand_and_longhand_override() {
        let s = apply(&[("border", "2px solid red"), ("border-left-color", "blue")]);
        assert_eq!(s.border_width, [2.0; 4]);
        assert_eq!(s.border_color[0], Color::from_u8(255, 0, 0, 1.0));
        assert_eq!(s.border_color[3], Color::from_u8(0, 0, 255, 1.0));
        let none = apply(&[("border", "2px none red")]);
        assert_eq!(none.border_width, [0.0; 4]);
    }

    #[test]
    fn flex_shorthand_forms() {
        assert_eq!((apply(&[("flex", "1")]).flex_grow, apply(&[("flex", "1")]).flex_shrink), (1.0, 1.0));
        assert_eq!(apply(&[("flex", "1")]).flex_basis, Dim::Percent(0.0));
        let s = apply(&[("flex", "2 0 100px")]);
        assert_eq!((s.flex_grow, s.flex_shrink, s.flex_basis), (2.0, 0.0, Dim::Px(100.0)));
        assert_eq!(apply(&[("flex", "none")]).flex_shrink, 0.0);
    }

    #[test]
    fn invalid_values_are_ignored() {
        let s = apply(&[("width", "100px"), ("width", "banana"), ("display", "flex"), ("display", "blorp")]);
        assert_eq!(s.width, Dim::Px(100.0));
        assert_eq!(s.display, Display::Flex);
    }

    #[test]
    fn em_uses_own_font_size_but_font_size_uses_parent() {
        let mut parent = ComputedStyle::default();
        parent.font_size = 10.0;
        let mut st = ComputedStyle::inherit_from(&parent);
        let sc = StyleContext::default();
        apply_declaration(&mut st, &parent, "font-size", "2em", &sc);
        assert_eq!(st.font_size, 20.0);
        apply_declaration(&mut st, &parent, "padding", "1em", &sc);
        assert_eq!(st.padding[0], Dim::Px(20.0));
    }

    #[test]
    fn inherit_and_initial_keywords() {
        let mut parent = ComputedStyle::default();
        parent.background_color = Color::WHITE;
        let mut st = ComputedStyle::inherit_from(&parent);
        let sc = StyleContext::default();
        assert!(apply_declaration(&mut st, &parent, "background-color", "inherit", &sc));
        assert_eq!(st.background_color, Color::WHITE);
        assert!(apply_declaration(&mut st, &parent, "background-color", "initial", &sc));
        assert_eq!(st.background_color, Color::TRANSPARENT);
    }

    #[test]
    fn grid_tracks() {
        let s = apply(&[("grid-template-columns", "repeat(2, 1fr) 100px minmax(10px, auto)")]);
        assert_eq!(s.grid_template_columns.len(), 3);
        let s = apply(&[("grid-template-columns", "repeat(auto-fill, minmax(120px, 1fr))")]);
        assert!(matches!(s.grid_template_columns[0], TrackItem::Repeat(RepeatCount::AutoFill, _)));
    }

    #[test]
    fn shadows_gradients_transforms() {
        let s = apply(&[
            ("box-shadow", "0 2px 8px rgba(0,0,0,.5), inset 0 0 0 1px #fff"),
            ("background", "linear-gradient(to right, #000, #fff 50%) no-repeat"),
            ("transform", "translate(10px, 5%) rotate(45deg) scale(2)"),
        ]);
        assert_eq!(s.box_shadow.len(), 2);
        assert!(s.box_shadow[1].inset);
        assert!(matches!(s.background_images[0], BackgroundImage::LinearGradient { .. }));
        assert_eq!(s.transform.len(), 3);
    }

    #[test]
    fn transitions_and_animations() {
        let s = apply(&[("transition", "opacity .2s ease-in 50ms, color 1s"), ("animation", "spin 2s linear infinite alternate")]);
        assert_eq!(s.transitions.len(), 2);
        assert_eq!(s.transitions[0].delay, 0.05);
        assert_eq!(s.animations[0].name, "spin");
        assert!(s.animations[0].iterations.is_infinite());
        assert_eq!(s.animations[0].direction, AnimationDirection::Alternate);
    }

    #[test]
    fn font_shorthand() {
        let s = apply(&[("font", "italic bold 14px/20px Inter, sans-serif")]);
        assert_eq!((s.font_size, s.font_weight), (14.0, 700));
        assert_eq!(s.line_height, LineHeight::Px(20.0));
        assert_eq!(s.font_family.len(), 2);
    }
}

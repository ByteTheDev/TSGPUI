use super::*;

pub(super) fn font_weight_for(weight: &str) -> FontWeight {
    match weight {
        "bold" | "700" => FontWeight::BOLD,
        "600" => FontWeight::SEMIBOLD,
        "500" => FontWeight::MEDIUM,
        _ => FontWeight::NORMAL,
    }
}

pub(super) fn px_value(style: &BTreeMap<String, String>, key: &str) -> Option<f32> {
    style.get(key)?.strip_suffix("px")?.parse::<f32>().ok()
}

// Styles beyond the base layout set: paint, shadows, gradients, borders, flex, positioning,
// and text. It is generic over `Styled`, so the same code styles an element and its hover,
// active, focus, and disabled refinements, which are `StyleRefinement` values.
// `current_color` is the element's text color, used for borders that name no color.
pub(super) fn apply_extended<S: Styled>(mut target: S, style: &BTreeMap<String, String>, current_color: Option<Rgba>) -> S {
    if let Some(background) = style.get("background").and_then(|value| styles::background(value)) {
        target = target.bg(background);
    }
    if let Some(color) = style.get("color").and_then(|value| styles::color(value)) {
        target = target.text_color(color);
    }
    if let Some(opacity) = style.get("opacity").and_then(|value| value.parse::<f32>().ok()) {
        target = target.opacity(opacity);
    }
    if let Some(shadows) = style.get("box-shadow").and_then(|value| styles::shadows(value)) {
        target = target.shadow(shadows);
    }
    let font_px = px_value(style, "font-size").unwrap_or(16.0);
    if let Some(height) = style.get("line-height").and_then(|value| styles::line_height(value, font_px)) {
        target = target.line_height(height);
    }
    if style.get("font-style").is_some_and(|value| value == "italic") {
        target = target.italic();
    }
    if style.get("white-space").is_some_and(|value| value == "nowrap") {
        target = target.whitespace_nowrap();
    }
    if style.get("text-overflow").is_some_and(|value| value == "ellipsis") {
        target = target.text_ellipsis();
    }
    target = match style.get("cursor").map(String::as_str) {
        Some("pointer") => target.cursor_pointer(),
        Some("text") => target.cursor_text(),
        Some("not-allowed") => target.cursor_not_allowed(),
        Some("grab") => target.cursor_grab(),
        Some("move") => target.cursor_move(),
        Some("crosshair") => target.cursor_crosshair(),
        Some("default") => target.cursor_default(),
        _ => target,
    };

    let refinement = target.style();
    if let Some(width) = style.get("min-width").and_then(|value| styles::length(value)) { refinement.min_size.width = Some(width); }
    if let Some(height) = style.get("min-height").and_then(|value| styles::length(value)) { refinement.min_size.height = Some(height); }
    if let Some(width) = style.get("max-width").and_then(|value| styles::length(value)) { refinement.max_size.width = Some(width); }
    if let Some(height) = style.get("max-height").and_then(|value| styles::length(value)) { refinement.max_size.height = Some(height); }
    if let Some(grow) = style.get("flex-grow").and_then(|value| value.parse::<f32>().ok()) { refinement.flex_grow = Some(grow); }
    if let Some(shrink) = style.get("flex-shrink").and_then(|value| value.parse::<f32>().ok()) { refinement.flex_shrink = Some(shrink); }
    if let Some(basis) = style.get("flex-basis").and_then(|value| styles::length(value)) { refinement.flex_basis = Some(basis); }
    match style.get("position").map(String::as_str) {
        Some("absolute") => refinement.position = Some(Position::Absolute),
        Some("relative") => refinement.position = Some(Position::Relative),
        _ => {}
    }
    if let Some(top) = style.get("top").and_then(|value| styles::length(value)) { refinement.inset.top = Some(top); }
    if let Some(right) = style.get("right").and_then(|value| styles::length(value)) { refinement.inset.right = Some(right); }
    if let Some(bottom) = style.get("bottom").and_then(|value| styles::length(value)) { refinement.inset.bottom = Some(bottom); }
    if let Some(left) = style.get("left").and_then(|value| styles::length(value)) { refinement.inset.left = Some(left); }
    if let Some(value) = style.get("padding-top").and_then(|value| styles::definite(value)) { refinement.padding.top = Some(value); }
    if let Some(value) = style.get("padding-right").and_then(|value| styles::definite(value)) { refinement.padding.right = Some(value); }
    if let Some(value) = style.get("padding-bottom").and_then(|value| styles::definite(value)) { refinement.padding.bottom = Some(value); }
    if let Some(value) = style.get("padding-left").and_then(|value| styles::definite(value)) { refinement.padding.left = Some(value); }
    if let Some(value) = style.get("margin-top").and_then(|value| styles::length(value)) { refinement.margin.top = Some(value); }
    if let Some(value) = style.get("margin-right").and_then(|value| styles::length(value)) { refinement.margin.right = Some(value); }
    if let Some(value) = style.get("margin-bottom").and_then(|value| styles::length(value)) { refinement.margin.bottom = Some(value); }
    if let Some(value) = style.get("margin-left").and_then(|value| styles::length(value)) { refinement.margin.left = Some(value); }
    let border_width = style.get("border-width").and_then(|value| styles::px_value(value));
    let border_hidden = style.get("border-style").is_some_and(|value| value == "none");
    let border_px = if border_hidden { Some(0.0) } else { border_width };
    let side_widths = ["top", "right", "bottom", "left"].map(|side| {
        let width = style.get(&format!("border-{side}-width")).and_then(|value| styles::px_value(value));
        let hidden = style.get(&format!("border-{side}-style")).is_some_and(|value| value == "none");
        if hidden { Some(0.0) } else { width }
    });
    let mut widths = [border_px; 4];
    for (index, side_width) in side_widths.into_iter().enumerate() {
        if side_width.is_some() { widths[index] = side_width; }
    }
    for (target, width) in [&mut refinement.border_widths.top, &mut refinement.border_widths.right, &mut refinement.border_widths.bottom, &mut refinement.border_widths.left].into_iter().zip(widths) {
        if let Some(width) = width { *target = Some(AbsoluteLength::Pixels(px(width))); }
    }
    if let Some(color) = style.get("border-color").and_then(|value| styles::color(value)).or(current_color) {
        if widths.iter().any(|width| width.is_some_and(|width| width > 0.0)) {
            refinement.border_color = Some(Hsla::from(color));
        }
    }
    target
}

pub(super) fn media_matches(query: &str, width: gpui::Pixels, height: gpui::Pixels) -> bool {
    let normalized = query.to_ascii_lowercase();
    normalized.split("and").all(|condition| {
        let Some((edge_axis, raw_value)) = condition.trim().trim_matches(['(', ')']).split_once(':') else {
            return false;
        };
        let Some(raw_value) = raw_value.trim().strip_suffix("px") else { return false; };
        let Ok(value) = raw_value.parse::<f32>() else { return false; };
        let Some((edge, axis)) = edge_axis.trim().split_once('-') else { return false; };
        let actual = match axis.trim() {
            "width" => width,
            "height" => height,
            _ => return false,
        };
        match edge.trim() {
            "min" => actual >= px(value),
            "max" => actual <= px(value),
            _ => false,
        }
    })
}

pub(super) fn computed_style(node: &Node, window: &Window) -> BTreeMap<String, String> {
    let mut style = node.style.clone();
    let bounds = window.bounds();
    let mut responsive: Vec<_> = node.responsive.iter().collect();
    responsive.sort_by_key(|rule| (rule.specificity, rule.order));
    for rule in responsive {
        if media_matches(&rule.media, bounds.size.width, bounds.size.height) {
            style.extend(rule.declarations.clone());
        }
    }
    resolve_viewport_units(&mut style, bounds.size.width, bounds.size.height);
    style
}

// The compiler keeps vw/vh/vmin/vmax symbolic because only the host knows the window size.
pub(super) fn viewport_length(value: &str, width: f32, height: f32) -> Option<f32> {
    let (number, base) = if let Some(number) = value.strip_suffix("vmin") {
        (number, width.min(height))
    } else if let Some(number) = value.strip_suffix("vmax") {
        (number, width.max(height))
    } else if let Some(number) = value.strip_suffix("vw") {
        (number, width)
    } else if let Some(number) = value.strip_suffix("vh") {
        (number, height)
    } else {
        return None;
    };
    let amount = number.parse::<f32>().ok()?;
    amount.is_finite().then_some(amount * base / 100.0)
}

pub(super) fn resolve_viewport_units(style: &mut BTreeMap<String, String>, width: gpui::Pixels, height: gpui::Pixels) {
    let (width, height) = (f32::from(width), f32::from(height));
    for value in style.values_mut() {
        if let Some(px) = viewport_length(value, width, height) {
            *value = format!("{px}px");
        }
    }
}

// Grid tracks are compiled as "Nfr", "Npx", or "auto" tokens. GPUI's grid only supports equal
// tracks, so fixed/mixed templates are laid out as flex rows with one cell per track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum GridTrack { Fr(f32), Px(f32), Auto }

pub(super) fn parse_grid_tracks(value: &str) -> Option<Vec<GridTrack>> {
    let mut tracks = Vec::new();
    for token in value.split_whitespace() {
        if token == "auto" {
            tracks.push(GridTrack::Auto);
        } else if let Some(number) = token.strip_suffix("fr") {
            let amount = number.parse::<f32>().ok()?;
            if !amount.is_finite() || amount <= 0.0 { return None; }
            tracks.push(GridTrack::Fr(amount));
        } else if let Some(number) = token.strip_suffix("px") {
            let amount = number.parse::<f32>().ok()?;
            if !amount.is_finite() || amount < 0.0 { return None; }
            tracks.push(GridTrack::Px(amount));
        } else {
            return None;
        }
    }
    (!tracks.is_empty()).then_some(tracks)
}

pub(super) fn grid_needs_emulation(tracks: &[GridTrack]) -> bool {
    tracks.iter().any(|track| !matches!(track, GridTrack::Fr(_)))
}

pub(super) fn grid_tracks_for(node: &Node, style: &BTreeMap<String, String>) -> Option<Vec<GridTrack>> {
    if style.get("display").map(String::as_str) != Some("grid") { return None; }
    let tracks = parse_grid_tracks(style.get("grid-template-columns")?)?;
    if !grid_needs_emulation(&tracks) { return None; }
    // Repeated, virtualized, absolute, or text items need the ordinary flow renderer.
    let unsupported = node.children.iter().any(|child| {
        child.attrs.contains_key("data-repeat")
            || child.attrs.contains_key("data-virtual")
            || child.style.get("position").is_some_and(|position| position == "absolute")
            || (child.tag == "#text" && !child.text.trim().is_empty())
    });
    (!unsupported).then_some(tracks)
}


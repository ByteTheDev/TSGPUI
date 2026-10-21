//! Parsers for the canonical style values written by the CSS compiler (packages/cli/src/css.mjs).
//! The compiler normalizes web CSS, so these accept only the canonical forms. Anything else is None.

use gpui::{
    AbsoluteLength, Background, BoxShadow, DefiniteLength, Hsla, Length, LinearColorStop, Rgba,
    linear_gradient, point, px, rgba,
};

/// "auto", "12px" or "50%".
pub fn length(value: &str) -> Option<Length> {
    if value == "auto" {
        return Some(Length::Auto);
    }
    definite(value).map(Length::Definite)
}

/// "12px" or "50%".
pub fn definite(value: &str) -> Option<DefiniteLength> {
    if let Some(percent) = value.strip_suffix('%') {
        return percent.parse::<f32>().ok().map(|amount| gpui::relative(amount / 100.0));
    }
    px_value(value).map(|amount| DefiniteLength::Absolute(AbsoluteLength::Pixels(px(amount))))
}

/// "12px" becomes 12.0.
pub fn px_value(value: &str) -> Option<f32> {
    value.strip_suffix("px")?.parse::<f32>().ok().filter(|amount| amount.is_finite())
}

/// "#RRGGBB" or "#RRGGBBAA".
pub fn color(value: &str) -> Option<Rgba> {
    let digits = value.strip_prefix('#')?;
    if !digits.is_ascii() {
        return None;
    }
    let packed = match digits.len() {
        6 => (u32::from_str_radix(digits, 16).ok()? << 8) | 0xff,
        8 => u32::from_str_radix(digits, 16).ok()?,
        _ => return None,
    };
    Some(rgba(packed))
}

/// A color, or "linear-gradient(<deg>,<color>,<color>)".
pub fn background(value: &str) -> Option<Background> {
    if let Some(args) = value.strip_prefix("linear-gradient(").and_then(|rest| rest.strip_suffix(')')) {
        let (angle, stops) = args.split_once(',')?;
        let angle = angle.strip_suffix("deg")?.parse::<f32>().ok()?;
        let (from, to) = stops.split_once(',')?;
        let from: Hsla = color(from)?.into();
        let to: Hsla = color(to)?.into();
        return Some(linear_gradient(
            angle,
            LinearColorStop { color: from, percentage: 0.0 },
            LinearColorStop { color: to, percentage: 1.0 },
        ));
    }
    let solid: Hsla = color(value)?.into();
    Some(solid.into())
}

/// "x y blur spread color" entries joined by ", ".
pub fn shadows(value: &str) -> Option<Vec<BoxShadow>> {
    value
        .split(", ")
        .map(|entry| {
            let parts: Vec<&str> = entry.split(' ').collect();
            if parts.len() != 5 {
                return None;
            }
            let color: Hsla = color(parts[4])?.into();
            Some(BoxShadow {
                color,
                offset: point(px(px_value(parts[0])?), px(px_value(parts[1])?)),
                blur_radius: px(px_value(parts[2])?),
                spread_radius: px(px_value(parts[3])?),
            })
        })
        .collect()
}

/// "<n>px", or a unitless multiplier of the element's font size.
pub fn line_height(value: &str, font_px: f32) -> Option<DefiniteLength> {
    if value.ends_with("px") {
        return px_value(value).map(|amount| DefiniteLength::Absolute(AbsoluteLength::Pixels(px(amount))));
    }
    let multiplier = value.parse::<f32>().ok()?;
    Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(px(multiplier * font_px))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_parse_px_percent_and_auto() {
        assert!(matches!(length("12px"), Some(Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(v)))) if v == px(12.0)));
        assert!(matches!(length("50%"), Some(Length::Definite(DefiniteLength::Fraction(f))) if (f - 0.5).abs() < 1e-6));
        assert!(matches!(length("auto"), Some(Length::Auto)));
        assert!(length("12em").is_none());
        assert!(length("-x").is_none());
    }

    #[test]
    fn px_value_requires_the_px_suffix() {
        assert_eq!(px_value("8px"), Some(8.0));
        assert_eq!(px_value("8"), None);
        assert_eq!(px_value("8%"), None);
    }

    #[test]
    fn colors_parse_six_and_eight_digit_hex() {
        let opaque = color("#ff8000").unwrap();
        assert!((opaque.r - 1.0).abs() < 1e-6 && (opaque.g - 0.5019608).abs() < 1e-5 && opaque.b == 0.0);
        assert!((opaque.a - 1.0).abs() < 1e-6);
        let translucent = color("#00000080").unwrap();
        assert!((translucent.a - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn colors_reject_names_and_short_hex() {
        assert!(color("red").is_none());
        assert!(color("#abc").is_none());
        assert!(color("#éééé").is_none());
    }

    #[test]
    fn backgrounds_accept_colors_and_two_stop_gradients() {
        assert!(background("#112233").is_some());
        assert!(background("linear-gradient(90deg,#ffffff,#000000)").is_some());
        assert!(background("linear-gradient(#ffffff,#000000)").is_none());
        assert!(background("url(x.png)").is_none());
    }

    #[test]
    fn shadows_parse_each_entry() {
        let parsed = shadows("0px 2px 8px 0px #00000033, 0px 1px 0px 0px #ffffff").unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].offset, point(px(0.0), px(2.0)));
        assert_eq!(parsed[0].blur_radius, px(8.0));
        assert_eq!(parsed[1].spread_radius, px(0.0));
        assert!(shadows("0px 2px red").is_none());
    }

    #[test]
    fn line_height_is_px_or_a_multiplier_of_font_size() {
        assert!(matches!(line_height("20px", 16.0), Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(v))) if v == px(20.0)));
        assert!(matches!(line_height("1.5", 16.0), Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(v))) if v == px(24.0)));
    }
}

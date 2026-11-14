//! CSS colors. Stored as non-premultiplied sRGB floats in 0..=1.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    pub fn from_u8(r: u8, g: u8, b: u8, a: f32) -> Self {
        Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn with_alpha_mul(self, k: f32) -> Color {
        Color { a: self.a * k, ..self }
    }

    pub fn lerp(self, other: Color, t: f32) -> Color {
        // Interpolate in premultiplied space like CSS Color 4 does for sRGB.
        let pa = self.a;
        let pb = other.a;
        let a = pa + (pb - pa) * t;
        if a <= 0.0 {
            return Color::TRANSPARENT;
        }
        let mix = |x: f32, y: f32| (x * pa + (y * pb - x * pa) * t) / a;
        Color { r: mix(self.r, other.r), g: mix(self.g, other.g), b: mix(self.b, other.b), a }
    }

    pub fn is_transparent(self) -> bool {
        self.a <= 0.0
    }

    /// Parse any supported CSS color. `current` resolves `currentcolor`.
    pub fn parse(input: &str, current: Color) -> Option<Color> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        let lower = s.to_ascii_lowercase();
        if lower == "currentcolor" {
            return Some(current);
        }
        if lower == "transparent" {
            return Some(Color::TRANSPARENT);
        }
        if let Some(hex) = lower.strip_prefix('#') {
            return parse_hex(hex);
        }
        if let Some(open) = lower.find('(') {
            let name = lower[..open].trim();
            let close = lower.rfind(')')?;
            let args = &lower[open + 1..close];
            return match name {
                "rgb" | "rgba" => parse_rgb(args),
                "hsl" | "hsla" => parse_hsl(args),
                _ => None,
            };
        }
        named(&lower)
    }
}

fn parse_hex(h: &str) -> Option<Color> {
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let d = |i: usize| u8::from_str_radix(&h[i..i + 1].repeat(2), 16).ok();
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        3 => Some(Color::from_u8(d(0)?, d(1)?, d(2)?, 1.0)),
        4 => Some(Color::from_u8(d(0)?, d(1)?, d(2)?, d(3)? as f32 / 255.0)),
        6 => Some(Color::from_u8(p(0)?, p(2)?, p(4)?, 1.0)),
        8 => Some(Color::from_u8(p(0)?, p(2)?, p(4)?, p(6)? as f32 / 255.0)),
        _ => None,
    }
}

fn split_args(args: &str) -> Vec<String> {
    args.replace('/', " / ")
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty() && *p != "/")
        .map(|p| p.to_string())
        .collect()
}

fn component(p: &str, scale: f32) -> Option<f32> {
    if let Some(n) = p.strip_suffix('%') {
        n.parse::<f32>().ok().map(|v| v / 100.0)
    } else {
        p.parse::<f32>().ok().map(|v| v / scale)
    }
}

fn alpha(p: Option<&String>) -> Option<f32> {
    match p {
        None => Some(1.0),
        Some(p) => component(p, 1.0).map(|a| a.clamp(0.0, 1.0)),
    }
}

fn parse_rgb(args: &str) -> Option<Color> {
    let parts = split_args(args);
    if parts.len() < 3 {
        return None;
    }
    let r = component(&parts[0], 255.0)?.clamp(0.0, 1.0);
    let g = component(&parts[1], 255.0)?.clamp(0.0, 1.0);
    let b = component(&parts[2], 255.0)?.clamp(0.0, 1.0);
    Some(Color { r, g, b, a: alpha(parts.get(3))? })
}

fn parse_hsl(args: &str) -> Option<Color> {
    let parts = split_args(args);
    if parts.len() < 3 {
        return None;
    }
    let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?.rem_euclid(360.0) / 360.0;
    let s = component(&parts[1], 100.0)?.clamp(0.0, 1.0);
    let l = component(&parts[2], 100.0)?.clamp(0.0, 1.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Some(Color { r: hue(h + 1.0 / 3.0), g: hue(h), b: hue(h - 1.0 / 3.0), a: alpha(parts.get(3))? })
}

fn named(name: &str) -> Option<Color> {
    let (r, g, b) = match name {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "maroon" => (128, 0, 0),
        "olive" => (128, 128, 0),
        "lime" => (0, 255, 0),
        "teal" => (0, 128, 128),
        "navy" => (0, 0, 128),
        "purple" => (128, 0, 128),
        "orange" => (255, 165, 0),
        "pink" => (255, 192, 203),
        "brown" => (165, 42, 42),
        "gold" => (255, 215, 0),
        "indigo" => (75, 0, 130),
        "violet" => (238, 130, 238),
        "coral" => (255, 127, 80),
        "crimson" => (220, 20, 60),
        "tomato" => (255, 99, 71),
        "salmon" => (250, 128, 114),
        "khaki" => (240, 230, 140),
        "tan" => (210, 180, 140),
        "beige" => (245, 245, 220),
        "ivory" => (255, 255, 240),
        "lavender" => (230, 230, 250),
        "turquoise" => (64, 224, 208),
        "skyblue" => (135, 206, 235),
        "steelblue" => (70, 130, 180),
        "royalblue" => (65, 105, 225),
        "dodgerblue" => (30, 144, 255),
        "slategray" | "slategrey" => (112, 128, 144),
        "dimgray" | "dimgrey" => (105, 105, 105),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "gainsboro" => (220, 220, 220),
        "whitesmoke" => (245, 245, 245),
        "rebeccapurple" => (102, 51, 153),
        "darkgreen" => (0, 100, 0),
        "darkblue" => (0, 0, 139),
        "darkred" => (139, 0, 0),
        "limegreen" => (50, 205, 50),
        "seagreen" => (46, 139, 87),
        "forestgreen" => (34, 139, 34),
        "hotpink" => (255, 105, 180),
        "deeppink" => (255, 20, 147),
        "orangered" => (255, 69, 0),
        "chocolate" => (210, 105, 30),
        "firebrick" => (178, 34, 34),
        "plum" => (221, 160, 221),
        "orchid" => (218, 112, 214),
        "mediumpurple" => (147, 112, 219),
        "midnightblue" => (25, 25, 112),
        _ => return None,
    };
    Some(Color::from_u8(r, g, b, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_forms() {
        assert_eq!(Color::parse("#fff", Color::BLACK), Some(Color::WHITE));
        assert_eq!(Color::parse("#ff0000", Color::BLACK), Some(Color::from_u8(255, 0, 0, 1.0)));
        let c = Color::parse("#00000080", Color::BLACK).unwrap();
        assert!((c.a - 128.0 / 255.0).abs() < 1e-4);
    }

    #[test]
    fn functional_forms() {
        let c = Color::parse("rgba(255, 0, 0, 0.5)", Color::BLACK).unwrap();
        assert_eq!((c.r, c.a), (1.0, 0.5));
        let c = Color::parse("rgb(0 128 255 / 50%)", Color::BLACK).unwrap();
        assert!((c.b - 1.0).abs() < 1e-6 && (c.a - 0.5).abs() < 1e-6);
        let c = Color::parse("hsl(120, 100%, 50%)", Color::BLACK).unwrap();
        assert!(c.g > 0.99 && c.r < 0.01);
    }

    #[test]
    fn keywords() {
        let cur = Color::from_u8(1, 2, 3, 1.0);
        assert_eq!(Color::parse("currentColor", cur), Some(cur));
        assert_eq!(Color::parse("TRANSPARENT", cur), Some(Color::TRANSPARENT));
        assert_eq!(Color::parse("nope", cur), None);
    }
}

//! Timing functions, property interpolation, 2D transform matrices and keyframe sampling.
//! Pure functions over [`ComputedStyle`]; the engine owns clocks and animation state.

use crate::color::Color;
use crate::parser::{Declaration, Keyframes};
use crate::properties::{apply_declaration, StyleContext};
use crate::style::*;
use crate::values::Dim;

// ---------------------------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------------------------

impl TimingFunction {
    /// Map linear progress `x` in 0..=1 to eased progress.
    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self.kind {
            TimingKind::Linear => x,
            TimingKind::Steps(n, jump_start) => {
                let n = n as f32;
                let step = if jump_start { (x * n).ceil() } else { (x * n).floor() };
                (step / n).clamp(0.0, 1.0)
            }
            TimingKind::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, x),
        }
    }
}

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    // Solve bezier_x(t) = x by bisection (robust, 24 iterations ≈ 6e-8 precision), then evaluate y.
    let bez = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if bez(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) * 0.5)
}

// ---------------------------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------------------------

/// 2D affine matrix `[a b c d e f]` mapping (x, y) → (a·x + c·y + e, b·x + d·y + f).
pub type Matrix = [f32; 6];
pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

pub fn mat_mul(m: Matrix, n: Matrix) -> Matrix {
    // result = m · n (apply n first, then m).
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

pub fn mat_apply(m: Matrix, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

pub fn mat_invert(m: Matrix) -> Option<Matrix> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let a = m[3] * inv;
    let b = -m[1] * inv;
    let c = -m[2] * inv;
    let d = m[0] * inv;
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}

/// Build the matrix for a style's `transform` around its `transform-origin`, for a box of `w` × `h`.
pub fn transform_matrix(ops: &[TransformOp], origin: (Dim, Dim), w: f32, h: f32) -> Matrix {
    if ops.is_empty() {
        return IDENTITY;
    }
    let mut m = IDENTITY;
    for op in ops {
        let o = match *op {
            TransformOp::Translate(x, y) => [1.0, 0.0, 0.0, 1.0, x.resolve(w).unwrap_or(0.0), y.resolve(h).unwrap_or(0.0)],
            TransformOp::Scale(x, y) => [x, 0.0, 0.0, y, 0.0, 0.0],
            TransformOp::Rotate(a) => {
                let (s, c) = a.sin_cos();
                [c, s, -s, c, 0.0, 0.0]
            }
            TransformOp::Skew(x, y) => [1.0, y.tan(), x.tan(), 1.0, 0.0, 0.0],
            TransformOp::Matrix(mm) => mm,
        };
        m = mat_mul(m, o);
    }
    let ox = origin.0.resolve(w).unwrap_or(w / 2.0);
    let oy = origin.1.resolve(h).unwrap_or(h / 2.0);
    // translate(origin) · m · translate(-origin)
    mat_mul(mat_mul([1.0, 0.0, 0.0, 1.0, ox, oy], m), [1.0, 0.0, 0.0, 1.0, -ox, -oy])
}

// ---------------------------------------------------------------------------------------------
// Interpolation
// ---------------------------------------------------------------------------------------------

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_dim(a: Dim, b: Dim, t: f32) -> Dim {
    match (a, b) {
        (Dim::Px(x), Dim::Px(y)) => Dim::Px(lerp(x, y, t)),
        (Dim::Percent(x), Dim::Percent(y)) => Dim::Percent(lerp(x, y, t)),
        // Mixed units / auto are discrete in this engine (no calc() mixing), flipping at the midpoint.
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

fn lerp_transform(a: &[TransformOp], b: &[TransformOp], t: f32) -> Vec<TransformOp> {
    let ident = |op: &TransformOp| match op {
        TransformOp::Translate(..) => TransformOp::Translate(Dim::Px(0.0), Dim::Px(0.0)),
        TransformOp::Scale(..) => TransformOp::Scale(1.0, 1.0),
        TransformOp::Rotate(_) => TransformOp::Rotate(0.0),
        TransformOp::Skew(..) => TransformOp::Skew(0.0, 0.0),
        TransformOp::Matrix(_) => TransformOp::Matrix(IDENTITY),
    };
    let len = a.len().max(b.len());
    let mut out = Vec::new();
    for i in 0..len {
        let (x, y) = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => (*x, *y),
            (Some(x), None) => (*x, ident(x)),
            (None, Some(y)) => (ident(y), *y),
            _ => unreachable!(),
        };
        out.push(match (x, y) {
            (TransformOp::Translate(ax, ay), TransformOp::Translate(bx, by)) => TransformOp::Translate(lerp_dim(ax, bx, t), lerp_dim(ay, by, t)),
            (TransformOp::Scale(ax, ay), TransformOp::Scale(bx, by)) => TransformOp::Scale(lerp(ax, bx, t), lerp(ay, by, t)),
            (TransformOp::Rotate(p), TransformOp::Rotate(q)) => TransformOp::Rotate(lerp(p, q, t)),
            (TransformOp::Skew(ax, ay), TransformOp::Skew(bx, by)) => TransformOp::Skew(lerp(ax, bx, t), lerp(ay, by, t)),
            (TransformOp::Matrix(p), TransformOp::Matrix(q)) => {
                let mut m = [0.0; 6];
                for k in 0..6 {
                    m[k] = lerp(p[k], q[k], t);
                }
                TransformOp::Matrix(m)
            }
            // Different function kinds at the same position are not interpolable: flip the whole list.
            _ => return if t < 0.5 { a.to_vec() } else { b.to_vec() },
        });
    }
    out
}

/// Property names that can be transitioned or keyframed by this engine.
pub const ANIMATABLE: &[&str] = &[
    "opacity",
    "color",
    "background-color",
    "border-color",
    "border-radius",
    "border-width",
    "outline-color",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "margin",
    "padding",
    "top",
    "right",
    "bottom",
    "left",
    "transform",
    "font-size",
    "letter-spacing",
    "flex-grow",
    "flex-shrink",
    "gap",
    "box-shadow",
];

/// Does `prop` hold different values in the two styles?
pub fn property_differs(prop: &str, a: &ComputedStyle, b: &ComputedStyle) -> bool {
    match prop {
        "opacity" => a.opacity != b.opacity,
        "color" => a.color != b.color,
        "background-color" => a.background_color != b.background_color,
        "border-color" => a.border_color != b.border_color,
        "border-radius" => a.border_radius != b.border_radius,
        "border-width" => a.border_width != b.border_width,
        "outline-color" => a.outline_color != b.outline_color,
        "width" => a.width != b.width,
        "height" => a.height != b.height,
        "min-width" => a.min_width != b.min_width,
        "min-height" => a.min_height != b.min_height,
        "max-width" => a.max_width != b.max_width,
        "max-height" => a.max_height != b.max_height,
        "margin" => a.margin != b.margin,
        "padding" => a.padding != b.padding,
        "top" => a.inset[0] != b.inset[0],
        "right" => a.inset[1] != b.inset[1],
        "bottom" => a.inset[2] != b.inset[2],
        "left" => a.inset[3] != b.inset[3],
        "transform" => a.transform != b.transform,
        "font-size" => a.font_size != b.font_size,
        "letter-spacing" => a.letter_spacing != b.letter_spacing,
        "flex-grow" => a.flex_grow != b.flex_grow,
        "flex-shrink" => a.flex_shrink != b.flex_shrink,
        "gap" => a.row_gap != b.row_gap || a.column_gap != b.column_gap,
        "box-shadow" => a.box_shadow != b.box_shadow,
        _ => false,
    }
}

/// Write the interpolation of `prop` between `from` and `to` at progress `t` into `out`.
pub fn interpolate_property(prop: &str, from: &ComputedStyle, to: &ComputedStyle, t: f32, out: &mut ComputedStyle) -> bool {
    macro_rules! arr4_dim {
        ($f:ident) => {{
            for i in 0..4 {
                out.$f[i] = lerp_dim(from.$f[i], to.$f[i], t);
            }
        }};
    }
    match prop {
        "opacity" => out.opacity = lerp(from.opacity, to.opacity, t),
        "color" => out.color = from.color.lerp(to.color, t),
        "background-color" => out.background_color = from.background_color.lerp(to.background_color, t),
        "border-color" => {
            for i in 0..4 {
                out.border_color[i] = from.border_color[i].lerp(to.border_color[i], t);
            }
        }
        "border-radius" => arr4_dim!(border_radius),
        "border-width" => {
            for i in 0..4 {
                out.border_width[i] = lerp(from.border_width[i], to.border_width[i], t);
            }
        }
        "outline-color" => out.outline_color = from.outline_color.lerp(to.outline_color, t),
        "width" => out.width = lerp_dim(from.width, to.width, t),
        "height" => out.height = lerp_dim(from.height, to.height, t),
        "min-width" => out.min_width = lerp_dim(from.min_width, to.min_width, t),
        "min-height" => out.min_height = lerp_dim(from.min_height, to.min_height, t),
        "max-width" => out.max_width = lerp_dim(from.max_width, to.max_width, t),
        "max-height" => out.max_height = lerp_dim(from.max_height, to.max_height, t),
        "margin" => arr4_dim!(margin),
        "padding" => arr4_dim!(padding),
        "top" => out.inset[0] = lerp_dim(from.inset[0], to.inset[0], t),
        "right" => out.inset[1] = lerp_dim(from.inset[1], to.inset[1], t),
        "bottom" => out.inset[2] = lerp_dim(from.inset[2], to.inset[2], t),
        "left" => out.inset[3] = lerp_dim(from.inset[3], to.inset[3], t),
        "transform" => out.transform = lerp_transform(&from.transform, &to.transform, t),
        "font-size" => out.font_size = lerp(from.font_size, to.font_size, t),
        "letter-spacing" => out.letter_spacing = lerp(from.letter_spacing, to.letter_spacing, t),
        "flex-grow" => out.flex_grow = lerp(from.flex_grow, to.flex_grow, t),
        "flex-shrink" => out.flex_shrink = lerp(from.flex_shrink, to.flex_shrink, t),
        "gap" => {
            out.row_gap = lerp_dim(from.row_gap, to.row_gap, t);
            out.column_gap = lerp_dim(from.column_gap, to.column_gap, t);
        }
        "box-shadow" => {
            if from.box_shadow.len() == to.box_shadow.len() {
                out.box_shadow = from
                    .box_shadow
                    .iter()
                    .zip(&to.box_shadow)
                    .map(|(a, b)| Shadow {
                        dx: lerp(a.dx, b.dx, t),
                        dy: lerp(a.dy, b.dy, t),
                        blur: lerp(a.blur, b.blur, t),
                        spread: lerp(a.spread, b.spread, t),
                        color: a.color.lerp(b.color, t),
                        inset: b.inset,
                    })
                    .collect();
            } else if t >= 0.5 {
                out.box_shadow = to.box_shadow.clone();
            }
        }
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Keyframe animations
// ---------------------------------------------------------------------------------------------

/// Properties a declaration list touches (shorthands expand to their animatable longhand names).
fn touched_properties(decls: &[Declaration]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for d in decls {
        let names: &[&'static str] = match d.name.as_str() {
            "opacity" => &["opacity"],
            "color" => &["color"],
            "background-color" | "background" => &["background-color"],
            "border-color" | "border" => &["border-color", "border-width"],
            "border-radius" => &["border-radius"],
            "border-width" => &["border-width"],
            "outline-color" => &["outline-color"],
            "width" => &["width"],
            "height" => &["height"],
            "min-width" => &["min-width"],
            "min-height" => &["min-height"],
            "max-width" => &["max-width"],
            "max-height" => &["max-height"],
            "margin" | "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => &["margin"],
            "padding" | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => &["padding"],
            "top" => &["top"],
            "right" => &["right"],
            "bottom" => &["bottom"],
            "left" => &["left"],
            "transform" => &["transform"],
            "font-size" => &["font-size"],
            "letter-spacing" => &["letter-spacing"],
            "flex-grow" => &["flex-grow"],
            "gap" | "row-gap" | "column-gap" => &["gap"],
            "box-shadow" => &["box-shadow"],
            _ => &[],
        };
        for n in names {
            if !out.contains(n) {
                out.push(n);
            }
        }
    }
    out
}

/// Result of sampling an animation at a point in time.
pub struct AnimationSample {
    pub style: ComputedStyle,
    pub finished: bool,
}

/// Sample a keyframe animation. `base` is the element's computed style without the animation;
/// the returned style has animated properties overridden. `None` means the animation has no effect right now.
pub fn sample_animation(
    base: &ComputedStyle,
    parent: &ComputedStyle,
    kf: &Keyframes,
    spec: &AnimationSpec,
    elapsed: f32,
    sc: &StyleContext,
) -> Option<AnimationSample> {
    if kf.frames.is_empty() {
        return None;
    }
    let local = elapsed - spec.delay;
    let active_duration = if spec.iterations.is_infinite() { f32::INFINITY } else { spec.duration * spec.iterations };
    let finished = spec.duration > 0.0 && local >= active_duration;
    if local < 0.0 {
        if !matches!(spec.fill, FillMode::Backwards | FillMode::Both) {
            return None;
        }
    } else if finished && !matches!(spec.fill, FillMode::Forwards | FillMode::Both) {
        return Some(AnimationSample { style: base.clone(), finished: true });
    }
    let (iteration, progress) = if spec.duration <= 0.0 {
        (0u32, 1.0)
    } else if local < 0.0 {
        (0, 0.0)
    } else if finished {
        let it = (spec.iterations - 1.0).max(0.0);
        let frac = spec.iterations - spec.iterations.floor();
        (it as u32, if frac == 0.0 { 1.0 } else { frac })
    } else {
        let t = local / spec.duration;
        (t.floor() as u32, t - t.floor())
    };
    let reversed = match spec.direction {
        AnimationDirection::Normal => false,
        AnimationDirection::Reverse => true,
        AnimationDirection::Alternate => iteration % 2 == 1,
        AnimationDirection::AlternateReverse => iteration % 2 == 0,
    };
    let p = if reversed { 1.0 - progress } else { progress };

    // Build a computed style for every keyframe (base + that frame's declarations).
    let mut frames: Vec<(f32, ComputedStyle, Vec<&'static str>)> = Vec::new();
    for (off, decls) in &kf.frames {
        let mut s = base.clone();
        for d in decls {
            apply_declaration(&mut s, parent, &d.name, &d.value, sc);
        }
        frames.push((*off, s, touched_properties(decls)));
    }
    let mut props: Vec<&'static str> = Vec::new();
    for (_, _, t) in &frames {
        for p in t {
            if !props.contains(p) {
                props.push(p);
            }
        }
    }
    // Implicit from/to frames use the base style.
    if frames.first().map(|f| f.0 > 0.0).unwrap_or(true) {
        frames.insert(0, (0.0, base.clone(), props.clone()));
    }
    if frames.last().map(|f| f.0 < 1.0).unwrap_or(true) {
        frames.push((1.0, base.clone(), props.clone()));
    }

    let mut out = base.clone();
    for prop in props {
        // Frames that specify this property, in order.
        let specified: Vec<&(f32, ComputedStyle, Vec<&'static str>)> = frames.iter().filter(|f| f.2.contains(&prop)).collect();
        let (lo, hi) = {
            let mut lo = specified[0];
            let mut hi = specified[specified.len() - 1];
            for f in &specified {
                if f.0 <= p {
                    lo = f;
                }
            }
            for f in specified.iter().rev() {
                if f.0 >= p {
                    hi = f;
                }
            }
            (lo, hi)
        };
        let t = if hi.0 > lo.0 { ((p - lo.0) / (hi.0 - lo.0)).clamp(0.0, 1.0) } else { 1.0 };
        let eased = spec.timing.eval(t);
        interpolate_property(prop, &lo.1, &hi.1, eased, &mut out);
    }
    Some(AnimationSample { style: out, finished })
}

pub fn color_eq_eps(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 1e-4 && (a.g - b.g).abs() < 1e-4 && (a.b - b.b).abs() < 1e-4 && (a.a - b.a).abs() < 1e-4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Stylesheet;
    use crate::parser::Rule;

    #[test]
    fn easing_endpoints_and_monotonic() {
        for tf in ["ease", "ease-in", "ease-out", "ease-in-out", "linear"] {
            let f = crate::properties::parse_timing(tf).unwrap();
            assert!((f.eval(0.0)).abs() < 1e-4 && (f.eval(1.0) - 1.0).abs() < 1e-4, "{tf}");
            let mut prev = 0.0;
            for i in 0..=20 {
                let v = f.eval(i as f32 / 20.0);
                assert!(v >= prev - 1e-4, "{tf} not monotonic");
                prev = v;
            }
        }
        let ease_in = crate::properties::parse_timing("ease-in").unwrap();
        assert!(ease_in.eval(0.5) < 0.5);
        let steps = crate::properties::parse_timing("steps(4, end)").unwrap();
        assert_eq!(steps.eval(0.3), 0.25);
    }

    #[test]
    fn matrix_rotate_about_center() {
        let m = transform_matrix(&[TransformOp::Rotate(std::f32::consts::FRAC_PI_2)], (Dim::Percent(50.0), Dim::Percent(50.0)), 100.0, 100.0);
        let (x, y) = mat_apply(m, 100.0, 50.0); // right-middle -> bottom-middle
        assert!((x - 50.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3, "{x},{y}");
        let inv = mat_invert(m).unwrap();
        let (bx, by) = mat_apply(inv, x, y);
        assert!((bx - 100.0).abs() < 1e-3 && (by - 50.0).abs() < 1e-3);
    }

    #[test]
    fn color_and_length_interpolation() {
        let mut a = ComputedStyle::default();
        let mut b = ComputedStyle::default();
        a.width = Dim::Px(100.0);
        b.width = Dim::Px(200.0);
        a.background_color = Color::BLACK;
        b.background_color = Color::WHITE;
        let mut out = a.clone();
        interpolate_property("width", &a, &b, 0.25, &mut out);
        interpolate_property("background-color", &a, &b, 0.5, &mut out);
        assert_eq!(out.width, Dim::Px(125.0));
        assert!((out.background_color.r - 0.5).abs() < 1e-5);
    }

    #[test]
    fn keyframes_sampling() {
        let sheet = Stylesheet::parse("@keyframes fade { from { opacity: 0 } to { opacity: 1 } }");
        let kf = match &sheet.rules[0] {
            Rule::Keyframes(k) => k.clone(),
            _ => panic!(),
        };
        let spec = AnimationSpec {
            name: "fade".into(),
            duration: 2.0,
            delay: 1.0,
            timing: TimingFunction { kind: TimingKind::Linear },
            iterations: 1.0,
            direction: AnimationDirection::Normal,
            fill: FillMode::None,
        };
        let base = ComputedStyle::default();
        let sc = StyleContext::default();
        assert!(sample_animation(&base, &base, &kf, &spec, 0.5, &sc).is_none(), "before delay, no backwards fill");
        let mid = sample_animation(&base, &base, &kf, &spec, 2.0, &sc).unwrap();
        assert!((mid.style.opacity - 0.5).abs() < 1e-4 && !mid.finished);
        let done = sample_animation(&base, &base, &kf, &spec, 5.0, &sc).unwrap();
        assert!(done.finished);
        assert_eq!(done.style.opacity, 1.0, "no forwards fill: reverts to base");
        let mut alt = spec.clone();
        alt.direction = AnimationDirection::Alternate;
        alt.iterations = 2.0;
        let second = sample_animation(&base, &base, &kf, &alt, 1.0 + 2.0 + 0.5, &sc).unwrap();
        assert!((second.style.opacity - 0.75).abs() < 1e-4, "second iteration plays in reverse: {}", second.style.opacity);
    }
}

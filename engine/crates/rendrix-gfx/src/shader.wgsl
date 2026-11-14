// One pipeline draws everything: rounded rects with borders and gradients, blurred shadows,
// glyphs from the atlas and images. Instances live in a storage buffer; the vertex stage expands a
// quad per instance and the fragment stage evaluates signed-distance shapes in the instance's local
// (pre-transform) space, so rotated/scaled boxes stay crisp.

struct Globals { viewport: vec2<f32>, pad: vec2<f32> };

struct Inst {
    rect: vec4<f32>,
    xf0: vec4<f32>,    // a b c d
    xf1: vec4<f32>,    // e f kind gradient_kind
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
    c3: vec4<f32>,
    stops: vec4<f32>,
    radii: vec4<f32>,
    border: vec4<f32>,
    bcolor: vec4<f32>,
    clip: vec4<f32>,
    uv: vec4<f32>,
    extra: vec4<f32>,  // angle, nstops, clip_radius, pad
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<storage, read> insts: array<Inst>;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) idx: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    let inst = insts[ii];
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let pad = inst.extra.w + 1.5;
    let lo = inst.rect.xy - vec2<f32>(pad);
    let hi = inst.rect.xy + inst.rect.zw + vec2<f32>(pad);
    let local = mix(lo, hi, corner);
    let screen = vec2<f32>(
        inst.xf0.x * local.x + inst.xf0.z * local.y + inst.xf1.x,
        inst.xf0.y * local.x + inst.xf0.w * local.y + inst.xf1.y,
    );
    var o: VOut;
    o.pos = vec4<f32>(screen.x / g.viewport.x * 2.0 - 1.0, 1.0 - screen.y / g.viewport.y * 2.0, 0.0, 1.0);
    o.local = local;
    o.idx = ii;
    return o;
}

fn sd_rrect(p: vec2<f32>, half: vec2<f32>, r: vec4<f32>) -> f32 {
    var rr: f32;
    if (p.x < 0.0) {
        if (p.y < 0.0) { rr = r.x; } else { rr = r.w; }
    } else {
        if (p.y < 0.0) { rr = r.y; } else { rr = r.z; }
    }
    let q = abs(p) - half + vec2<f32>(rr);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - rr;
}

fn coverage(d: f32, aa: f32) -> f32 {
    return clamp(0.5 - d / aa, 0.0, 1.0);
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

fn gradient(inst: Inst, p: vec2<f32>) -> vec4<f32> {
    let size = inst.rect.zw;
    let center = inst.rect.xy + size * 0.5;
    var t: f32;
    if (inst.xf1.w < 1.5) {
        let a = inst.extra.x;
        let dir = vec2<f32>(sin(a), -cos(a));
        let len = max(abs(size.x * dir.x) + abs(size.y * dir.y), 1e-4);
        t = dot(p - center, dir) / len + 0.5;
    } else {
        t = length((p - center) / max(size * 0.5, vec2<f32>(1e-4))) / 1.41421356;
    }
    let n = inst.extra.y;
    let c0 = premul(inst.c0);
    let c1 = premul(inst.c1);
    let c2 = premul(inst.c2);
    let c3 = premul(inst.c3);
    let s = inst.stops;
    var col = c0;
    if (t <= s.x) {
        col = c0;
    } else if (n > 1.5 && t <= s.y) {
        col = mix(c0, c1, (t - s.x) / max(s.y - s.x, 1e-5));
    } else if (n > 2.5 && t <= s.z) {
        col = mix(c1, c2, (t - s.y) / max(s.z - s.y, 1e-5));
    } else if (n > 3.5 && t <= s.w) {
        col = mix(c2, c3, (t - s.z) / max(s.w - s.z, 1e-5));
    } else {
        if (n < 1.5) { col = c0; } else if (n < 2.5) { col = c1; } else if (n < 3.5) { col = c2; } else { col = c3; }
    }
    return col;
}

@fragment
fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    let inst = insts[v.idx];
    let kind = u32(inst.xf1.z + 0.5);
    let p = v.local;

    // Derivatives and texture sampling must happen in uniform control flow.
    let dl = fwidth(p);
    let aa = max(max(dl.x, dl.y), 1e-4);
    let uvp = inst.uv.xy + inst.uv.zw * ((p - inst.rect.xy) / max(inst.rect.zw, vec2<f32>(1e-4)));
    let texel = textureSample(tex, samp, uvp);

    // Clip (screen space, device pixels).
    var clip_cov = 1.0;
    {
        let ch = inst.clip.zw * 0.5;
        let cd = sd_rrect(v.pos.xy - (inst.clip.xy + ch), ch, vec4<f32>(inst.extra.z));
        clip_cov = clamp(0.5 - cd, 0.0, 1.0);
    }

    let center = inst.rect.xy + inst.rect.zw * 0.5;
    let half = inst.rect.zw * 0.5;
    var out = vec4<f32>(0.0);

    if (kind == 0u) {
        // Rounded rect: fill (solid or gradient) plus optional border.
        let d_outer = sd_rrect(p - center, half, inst.radii);
        let cov_outer = coverage(d_outer, aa);
        var fill = premul(inst.c0);
        if (inst.xf1.w > 0.5) { fill = gradient(inst, p); }
        let bw = inst.border; // top right bottom left
        if (max(max(bw.x, bw.y), max(bw.z, bw.w)) > 0.0) {
            let ihalf = max(half - vec2<f32>((bw.y + bw.w) * 0.5, (bw.x + bw.z) * 0.5), vec2<f32>(0.0));
            let ioff = vec2<f32>((bw.w - bw.y) * 0.5, (bw.x - bw.z) * 0.5);
            let shrink = vec4<f32>(
                (bw.x + bw.w) * 0.5, (bw.x + bw.y) * 0.5, (bw.z + bw.y) * 0.5, (bw.z + bw.w) * 0.5);
            let d_inner = sd_rrect(p - center - ioff, ihalf, max(inst.radii - shrink, vec4<f32>(0.0)));
            let cov_inner = coverage(d_inner, aa);
            out = (fill * cov_inner + premul(inst.bcolor) * (1.0 - cov_inner)) * cov_outer;
        } else {
            out = fill * cov_outer;
        }
    } else if (kind == 1u) {
        // Outer box shadow: soft falloff around the (offset, spread) rounded rect.
        let d = sd_rrect(p - center, half, inst.radii);
        let blur = max(inst.extra.x, 0.001);
        var t = clamp(0.5 - d / blur, 0.0, 1.0);
        t = t * t * (3.0 - 2.0 * t);
        out = premul(inst.c0) * t;
    } else if (kind == 2u) {
        // Inset shadow: darkness inside the box, outside the offset "hole".
        let d_box = sd_rrect(p - center, half, inst.radii);
        let hc = inst.border.xy + inst.border.zw * 0.5;
        let d_hole = sd_rrect(p - hc, inst.border.zw * 0.5, inst.bcolor);
        let blur = max(inst.extra.x, 0.001);
        var t = clamp(0.5 + d_hole / blur, 0.0, 1.0);
        t = t * t * (3.0 - 2.0 * t);
        out = premul(inst.c0) * t * coverage(d_box, aa);
    } else if (kind == 3u) {
        // Mask glyph: atlas holds premultiplied white; tint with the text color.
        let inside = f32(all(p >= inst.rect.xy) && all(p <= inst.rect.xy + inst.rect.zw));
        out = premul(inst.c0) * texel.a * inside;
    } else {
        // Image or colour glyph: premultiplied texel, rounded-rect masked, modulated by opacity.
        let d = sd_rrect(p - center, half, inst.radii);
        out = texel * inst.c0.a * coverage(d, aa);
    }
    return out * clip_cov;
}

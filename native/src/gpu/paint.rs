//! Draw-list → GPU encoding: rounded-rect quad pipeline + glyph-atlas text pipeline.
//! Owner: agent B. See docs/RENDERER_TSGPU.md for the architecture.
//!
//! Contract (fixed):
//! - `Painter::new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self`
//! - `Painter::resize(&mut self, width: u32, height: u32, scale: f32)`
//! - `Painter::set_glyph_atlas(&mut self, atlas: &types::GlyphAtlas)` — upload when generation changed
//! - `Painter::paint(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, frame: &types::PaintFrame)`
//!   clears to `frame.background`, draws rects (rounded corners, borders, per-op scissor from
//!   `clip`) then glyph batches. Coordinates are logical pixels × `frame.scale`.
//!
//! Coordinate conventions:
//! - Everything in `types::ShapeRect` / `types::TextRun` is LOGICAL px and gets multiplied by
//!   `frame.scale` (device px per logical px) before landing in instance buffers.
//! - `types::GlyphInstance::pos` is logical and gets scaled; `uv`/`size` are ATLAS TEXELS and
//!   are used as-is: the quad on screen is `size` device px across, sampling `size` texels.
//!   This is only pixel-crisp if `text.rs` rasterizes the atlas at DEVICE pixel resolution
//!   (1 atlas texel = 1 device px). See GLYPH_CONVENTION note near `glyph_instance_data`.

use crate::gpu::types;
use bytemuck::{Pod, Zeroable};

// ---------------------------------------------------------------------------
// Shaders
// ---------------------------------------------------------------------------

/// Rounded-rect quad shader. One instanced unit quad (triangle strip, 4 vertices, no vertex
/// buffer — corners come from `@builtin(vertex_index)`). Per-instance data is authored in
/// PHYSICAL pixels (logical × frame.scale, done on the CPU in `rect_instance`).
///
/// Fragment: classic rounded-box signed distance function. `d < 0` is inside the box; we take
/// anti-aliased coverage with a ~1px smoothstep, and derive the border ring as the band between
/// the outer shape and the same shape shrunk by the border width.
const RECT_WGSL: &str = r#" // jshint ignore:line
struct Globals {
    viewport: vec2<f32>, // surface size, physical px
    pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;

struct InstanceInput {
    @location(0) rect: vec4<f32>,         // x, y, w, h in physical px
    @location(1) params: vec4<f32>,       // x = corner radius (px), y = border width (px), zw unused
    @location(2) fill: vec4<f32>,         // straight (non-premultiplied) RGBA
    @location(3) border_color: vec4<f32>, // straight RGBA
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>, // px, from the rect's top-left
    @location(1) size: vec2<f32>,  // rect size in px
    @location(2) @interpolate(flat) radius: f32,
    @location(3) @interpolate(flat) border_width: f32,
    @location(4) fill: vec4<f32>,
    @location(5) border_color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: InstanceInput) -> VertexOut {
    // Strip order: (0,0) (1,0) (0,1) (1,1) — two triangles cover the unit quad.
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let px = inst.rect.xy + corner * inst.rect.zw;
    // wgpu follows Vulkan NDC: y points down, (-1,-1) is the top-left of the target.
    var out: VertexOut;
    out.clip = vec4<f32>(px.x / globals.viewport.x * 2.0 - 1.0, 1.0 - px.y / globals.viewport.y * 2.0, 0.0, 1.0);
    out.local = corner * inst.rect.zw;
    out.size = inst.rect.zw;
    out.radius = inst.params.x;
    out.border_width = inst.params.y;
    out.fill = inst.fill;
    out.border_color = inst.border_color;
    return out;
}

// Signed distance to a rounded box centered at the origin with half-extents `half_size`
// and corner radius r, evaluated at p (px). Negative inside. Standard iq rounded-box SDF.
fn sd_rounded_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Anti-aliased "inside" coverage of a shape whose signed distance is d (px).
fn coverage(d: f32, aa: f32) -> f32 {
    return 1.0 - smoothstep(-aa, aa, d);
}

@fragment
fn fs_main(v: VertexOut) -> @location(0) vec4<f32> {
    let half_size = v.size * 0.5;
    let p = v.local - half_size;
    let r = min(v.radius, min(half_size.x, half_size.y)); // clamp pill-shaped overflow

    // ~half-pixel AA width from the actual pixel footprint (p is in device px, 1:1 mapping,
    // so this is normally ~0.7; fwidth keeps it correct if that ever stops being true).
    let aa = max(length(vec2<f32>(dpdx(p.x), dpdy(p.y))) * 0.5, 0.25);

    let d = sd_rounded_box(p, half_size, r);
    let outer = coverage(d, aa);

    var rgb = v.fill.rgb;
    var a = v.fill.a * outer;

    // Border ring: outer shape minus the same shape shrunk by border_width.
    if (v.border_width > 0.0) {
        let d_inner = d + v.border_width;
        let ring = outer * (1.0 - coverage(d_inner, aa));
        // Straight-alpha approximation: border replaces fill inside the ring. Exact when
        // either color is opaque (the common case); translucent fill+border composites
        // slightly differently than source-over would. Premultiplied output would fix it
        // but the pipeline is contractually SrcAlpha/OneMinusSrcAlpha.
        rgb = mix(v.fill.rgb, v.border_color.rgb, ring);
        a = mix(v.fill.a, max(v.fill.a, v.border_color.a), ring) * outer;
    }

    return vec4<f32>(rgb, a);
}
"#; // jshint ignore:line

/// Glyph quad shader. Same instanced unit quad; each instance is one glyph sampling the shared
/// atlas. UVs are in texels and get divided by the atlas size in the fragment shader.
///
/// Atlas convention (agreed with text.rs): coverage lives in the ALPHA channel of an RGBA8
/// atlas (or the red channel of an R8 atlas); rgb is ignored. Bilinear sampling.
const GLYPH_WGSL: &str = r#" // jshint ignore:line
struct Globals {
    viewport: vec2<f32>, // surface size, physical px
    atlas: vec2<f32>,    // atlas size in texels (0 if no atlas yet)
    flags: vec2<f32>,    // x = 1.0 when the atlas is R8 (coverage in .r instead of .a)
    pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct InstanceInput {
    @location(0) pos_size: vec4<f32>, // xy = pos (physical px), zw = quad extent (atlas texels as device px)
    @location(1) uv: vec4<f32>,       // xy = uv top-left in texels, zw unused
    @location(2) color: vec4<f32>,    // straight RGBA
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>, // texels
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: InstanceInput) -> VertexOut {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let px = inst.pos_size.xy + corner * inst.pos_size.zw;
    var out: VertexOut;
    out.clip = vec4<f32>(px.x / globals.viewport.x * 2.0 - 1.0, 1.0 - px.y / globals.viewport.y * 2.0, 0.0, 1.0);
    out.uv = inst.uv.xy + corner * inst.pos_size.zw;
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(v: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_tex, atlas_sampler, v.uv / globals.atlas);
    let coverage = mix(texel.a, texel.r, globals.flags.x);
    return vec4<f32>(v.color.rgb, v.color.a * coverage);
}
"#; // jshint ignore:line

// ---------------------------------------------------------------------------
// Instance data (bytemuck-friendly, no padding)
// ---------------------------------------------------------------------------

const RECT_STRIDE: u64 = 64; // 4 × vec4
const TEXT_STRIDE: u64 = 48; // 3 × vec4

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RectInstance {
    /// x, y, w, h in physical px.
    rect: [f32; 4],
    /// corner radius (px), border width (px), unused, unused.
    params: [f32; 4],
    fill: [f32; 4],
    border_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphInstanceData {
    /// pos.x, pos.y (physical px), size.x, size.y (texels, drawn 1:1 as device px).
    pos_size: [f32; 4],
    /// uv.x, uv.y (texels), unused, unused.
    uv: [f32; 4],
    color: [f32; 4],
}

fn rect_instance(sr: &types::ShapeRect, scale: f32) -> RectInstance {
    let fill = sr.fill.unwrap_or([0.0, 0.0, 0.0, 0.0]);
    let (border_width, border_color) = match &sr.border {
        Some(b) => (b.width * scale, b.color),
        None => (0.0, fill),
    };
    RectInstance {
        rect: [sr.rect.x * scale, sr.rect.y * scale, sr.rect.w * scale, sr.rect.h * scale],
        params: [sr.radius * scale, border_width, 0.0, 0.0],
        fill,
        border_color,
    }
}

/// GLYPH_CONVENTION: `pos` is logical → scaled by `frame.scale`. `uv`/`size` are atlas texels
/// used as-is (the quad is `size` device px across). text.rs must rasterize the atlas at device
/// resolution so 1 texel = 1 device px; if it instead rasterizes at logical resolution, quads
/// will be `1/scale` too small on hidpi — either fix here or have text.rs report scaled sizes.
fn glyph_instance_data(g: &types::GlyphInstance, scale: f32) -> GlyphInstanceData {
    GlyphInstanceData {
        pos_size: [g.pos[0] * scale, g.pos[1] * scale, g.size[0], g.size[1]],
        uv: [g.uv[0], g.uv[1], 0.0, 0.0],
        color: g.color,
    }
}

// ---------------------------------------------------------------------------
// Clipping
// ---------------------------------------------------------------------------

/// Convert a list of logical clip rects (flattened by the backend, intersected here) into a
/// scissor rect in physical px, clamped to the surface. `None` = fully clipped, skip the op.
fn clip_to_scissor(clips: &[types::Rect], frame: &types::PaintFrame) -> Option<(u32, u32, u32, u32)> {
    let scale = frame.scale;
    if !scale.is_finite() || scale <= 0.0 || frame.width == 0 || frame.height == 0 {
        return None;
    }
    // Intersect in logical space against the logical viewport.
    let mut acc = types::Rect {
        x: 0.0,
        y: 0.0,
        w: frame.width as f32 / scale,
        h: frame.height as f32 / scale,
    };
    for c in clips {
        let x0 = acc.x.max(c.x);
        let y0 = acc.y.max(c.y);
        let x1 = (acc.x + acc.w).min(c.x + c.w);
        let y1 = (acc.y + acc.h).min(c.y + c.h);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        acc = types::Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    }
    // Logical → physical: grow outward (floor top-left, ceil bottom-right) so AA edges of
    // rounded rects are never scissored mid-pixel, then clamp to the surface.
    let x0 = (acc.x * scale).floor().max(0.0) as u32;
    let y0 = (acc.y * scale).floor().max(0.0) as u32;
    let x1 = ((acc.x + acc.w) * scale).ceil().min(frame.width as f32) as u32;
    let y1 = ((acc.y + acc.h) * scale).ceil().min(frame.height as f32) as u32;
    if x1 <= x0 || y1 <= y0 || x0 >= frame.width || y0 >= frame.height {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

/// Grow (or recreate) an instance buffer so `cursor + count` instances fit. Returns the cursor
/// to write at — 0 when the buffer was recreated (fresh buffer, restart the frame's cursor).
/// TODO(M1+): one write_buffer + one draw per op is the M0-simple path; batch ops that share
/// state (pipeline/scissor) into fewer writes and use dynamic offsets instead.
fn ensure_instance_capacity(
    device: &wgpu::Device,
    buffer: &mut wgpu::Buffer,
    capacity: &mut u32,
    cursor: u32,
    count: u32,
    stride: u64,
    label: &str,
) -> u32 {
    let needed = cursor + count;
    if needed <= *capacity {
        return cursor;
    }
    let new_capacity = needed.max((*capacity).max(16) * 2);
    *buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: new_capacity as u64 * stride,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    *capacity = new_capacity;
    0
}

// ---------------------------------------------------------------------------
// Painter
// ---------------------------------------------------------------------------

struct AtlasGpu {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    single_channel: bool,
}

pub struct Painter {
    queue: wgpu::Queue,
    rect_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    rect_bind_group: wgpu::BindGroup,
    text_bind_group_layout: wgpu::BindGroupLayout,
    text_bind_group: Option<wgpu::BindGroup>,
    rect_globals: wgpu::Buffer,
    text_globals: wgpu::Buffer,
    sampler: wgpu::Sampler,
    rect_instances: wgpu::Buffer,
    text_instances: wgpu::Buffer,
    rect_capacity: u32,
    text_capacity: u32,
    atlas: Option<AtlasGpu>,
    /// Atlas handed over by `set_glyph_atlas` but not yet uploaded (upload needs `&Device`,
    /// which only `paint` receives).
    pending_atlas: Option<types::GlyphAtlas>,
    /// Generation of the atlas currently living on the GPU.
    uploaded_generation: u64,
    /// Last generation seen by `set_glyph_atlas`, so an unchanged atlas is never re-cloned.
    seen_generation: u64,
    debug_logged: bool,
    /// Viewport from `resize` (width, height, scale). `paint` trusts `frame` instead (per
    /// contract they should agree); kept for the fixed signature and future use.
    #[allow(dead_code)]
    viewport: (u32, u32, f32),
}

impl Painter {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let rect_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tsgpu rect shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_WGSL.into()),
        });
        let text_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tsgpu glyph shader"),
            source: wgpu::ShaderSource::Wgsl(GLYPH_WGSL.into()),
        });

        // Globals: rect pipeline needs the viewport; text pipeline needs viewport + atlas size
        // + format flag. Both are uniform buffers rewritten every frame via queue.write_buffer
        // (called before the caller submits the encoder, so ordering is guaranteed).
        let rect_globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tsgpu rect globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            }],
        });
        let text_globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tsgpu text globals layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(32),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let rect_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tsgpu rect pipeline layout"),
            bind_group_layouts: &[&rect_globals_layout],
            push_constant_ranges: &[],
        });
        let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tsgpu text pipeline layout"),
            bind_group_layouts: &[&text_globals_layout],
            push_constant_ranges: &[],
        });

        let rect_instance_layout = wgpu::VertexBufferLayout {
            array_stride: RECT_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 3 },
            ],
        };
        let text_instance_layout = wgpu::VertexBufferLayout {
            array_stride: TEXT_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
            ],
        };

        let make_pipeline = |label: &'static str,
                             module: &wgpu::ShaderModule,
                             layout: &wgpu::PipelineLayout,
                             instance_layout: &wgpu::VertexBufferLayout| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[instance_layout.clone()],
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // Straight-alpha blending, per contract. The shaders output
                        // non-premultiplied color to match.
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };

        let rect_pipeline = make_pipeline("tsgpu rect pipeline", &rect_module, &rect_pipeline_layout, &rect_instance_layout);
        let text_pipeline = make_pipeline("tsgpu text pipeline", &text_module, &text_pipeline_layout, &text_instance_layout);

        let rect_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tsgpu rect globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let text_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tsgpu text globals"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let rect_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tsgpu rect bind group"),
            layout: &rect_globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: rect_globals.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tsgpu glyph sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Instance buffers, grown on demand. 512 instances is plenty for the M0 tree.
        let make_instances = |label: &'static str, capacity: u32, stride: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity as u64 * stride,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (initial_capacity, rect_instances) = (512u32, make_instances("tsgpu rect instances", 512, RECT_STRIDE));
        let text_instances = make_instances("tsgpu text instances", 512, TEXT_STRIDE);

        Self {
            debug_logged: false,
            queue: queue.clone(),
            rect_pipeline,
            text_pipeline,
            rect_bind_group,
            text_bind_group_layout: text_globals_layout,
            text_bind_group: None,
            rect_globals,
            text_globals,
            sampler,
            rect_instances,
            text_instances,
            rect_capacity: initial_capacity,
            text_capacity: 512,
            atlas: None,
            pending_atlas: None,
            uploaded_generation: 0,
            seen_generation: 0,
            viewport: (0, 0, 1.0),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
        self.viewport = (width, height, scale);
    }

    /// Store the atlas for upload during the next `paint` (that's where `&Device` is available).
    /// Cloning only happens when the generation actually changed.
    pub fn set_glyph_atlas(&mut self, atlas: &types::GlyphAtlas) {
        if atlas.generation != self.seen_generation {
            self.seen_generation = atlas.generation;
            self.pending_atlas = Some(atlas.clone());
        }
    }

    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        frame: &types::PaintFrame,
    ) {
        // Clear to the frame background (linear RGBA; wgpu applies the sRGB encode for
        // *-srgb swapchain formats after the clear, so this lands on screen as intended).
        let bg = frame.background;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tsgpu frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: bg[0] as f64,
                        g: bg[1] as f64,
                        b: bg[2] as f64,
                        a: bg[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        let scale = frame.scale;
        if std::env::var_os("TSGPU_DEBUG_PAINT").is_some() && !self.debug_logged {
            self.debug_logged = true;
            let mut dump = String::new();
            dump.push_str(&format!("surface paint: w={} h={} scale={} atlas_pending={} gen_uploaded={}\n", frame.width, frame.height, frame.scale, self.pending_atlas.is_some(), self.uploaded_generation));
            for op in frame.ops.iter().take(64) {
                match op {
                    types::DrawOp::Glyphs(batch) => {
                        if let Some(first) = batch.instances.first() {
                            dump.push_str(&format!("glyphbatch n={} first pos=({:.1},{:.1}) uv=({:.1},{:.1}) size=({:.1},{:.1})\n", batch.instances.len(), first.pos[0], first.pos[1], first.uv[0], first.uv[1], first.size[0], first.size[1]));
                        }
                    }
                    types::DrawOp::Rect(_) => {}
                    types::DrawOp::Text(_) => {}
                }
            }
            let path = std::env::temp_dir().join("tsgpu-paint-debug.txt");
            let _ = std::fs::write(&path, dump);
        }
        if scale.is_finite() && scale > 0.0 && frame.width > 0 && frame.height > 0 {
            // Upload a newly generated atlas (needs `&Device`, hence here rather than in
            // `set_glyph_atlas`). TODO(M1+): partial/gpu-side atlas updates instead of full
            // re-upload; also pad rows to 256B in text.rs so a single fast-path copy suffices.
            let needs_upload = self
                .pending_atlas
                .as_ref()
                .is_some_and(|a| a.generation != self.uploaded_generation);
            if needs_upload {
                let atlas = self.pending_atlas.take().expect("checked above");
                self.upload_atlas(device, &atlas);
            }

            // Globals for this frame (viewport in physical px; atlas size + format flag).
            self.queue.write_buffer(
                &self.rect_globals,
                0,
                bytemuck::cast_slice(&[frame.width as f32, frame.height as f32, 0.0, 0.0]),
            );
            let (atlas_w, atlas_h, r8_flag) = match &self.atlas {
                Some(a) => (a.width as f32, a.height as f32, if a.single_channel { 1.0 } else { 0.0 }),
                None => (0.0, 0.0, 0.0),
            };
            self.queue.write_buffer(
                &self.text_globals,
                0,
                bytemuck::cast_slice(&[frame.width as f32, frame.height as f32, atlas_w, atlas_h, r8_flag, 0.0, 0.0, 0.0]),
            );

            // Rolling write cursors: each draw this frame reads a private slice of its instance
            // buffer. queue.write_buffer calls all land before the caller submits the encoder,
            // so every recorded draw must own a distinct offset (the last write at a shared
            // offset would win for ALL earlier draws recorded against it).
            let mut rect_cursor: u32 = 0;
            let mut text_cursor: u32 = 0;
            let mut glyph_draws: u32 = 0;
            let mut glyph_skips: u32 = 0;

            for op in &frame.ops {
                match op {
                    types::DrawOp::Rect(sr) => {
                        if sr.rect.w <= 0.0 || sr.rect.h <= 0.0 {
                            continue; // degenerate
                        }
                        if sr.fill.is_none() && sr.border.is_none() {
                            continue; // invisible
                        }
                        let Some(scissor) = clip_to_scissor(&sr.clip, frame) else {
                            continue; // fully clipped
                        };
                        rect_cursor = ensure_instance_capacity(
                            device,
                            &mut self.rect_instances,
                            &mut self.rect_capacity,
                            rect_cursor,
                            1,
                            RECT_STRIDE,
                            "tsgpu rect instances",
                        );
                        let offset = rect_cursor as u64 * RECT_STRIDE;
                        self.queue
                            .write_buffer(&self.rect_instances, offset, bytemuck::cast_slice(&[rect_instance(sr, scale)]));
                        pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                        pass.set_pipeline(&self.rect_pipeline);
                        pass.set_bind_group(0, &self.rect_bind_group, &[]);
                        pass.set_vertex_buffer(0, self.rect_instances.slice(offset..offset + RECT_STRIDE));
                        pass.draw(0..4, 0..1);
                        rect_cursor += 1;
                    }
                    types::DrawOp::Glyphs(batch) => {
                        if batch.instances.is_empty() || self.text_bind_group.is_none() {
                            glyph_skips += batch.instances.len() as u32;
                            continue; // nothing to draw, or no atlas uploaded yet
                        }
                        let count = batch.instances.len() as u32;
                        text_cursor = ensure_instance_capacity(
                            device,
                            &mut self.text_instances,
                            &mut self.text_capacity,
                            text_cursor,
                            count,
                            TEXT_STRIDE,
                            "tsgpu text instances",
                        );
                        let base = text_cursor as u64 * TEXT_STRIDE;
                        if std::env::var_os("TSGPU_DEBUG_PAINT").is_some() && text_cursor == 0 {
                            for g in batch.instances.iter().take(2) {
                                let d = glyph_instance_data(g, scale);
                                println!("instance: pos_size={:?} uv={:?} color={:?}", d.pos_size, d.uv, d.color);
                            }
                        }                        let data: Vec<GlyphInstanceData> = batch
                            .instances
                            .iter()
                            .map(|g| glyph_instance_data(g, scale))
                            .collect();
                        self.queue
                            .write_buffer(&self.text_instances, base, bytemuck::cast_slice(&data));

                        pass.set_pipeline(&self.text_pipeline);
                        pass.set_bind_group(0, self.text_bind_group.as_ref().expect("checked above"), &[]);

                        // Glyph instances each carry their own clip; scissor must change per
                        // clip, so batch consecutive instances that agree on their clip.
                        let mut i = 0usize;
                        while i < batch.instances.len() {
                            let clip = &batch.instances[i].clip;
                            let mut j = i + 1;
                            while j < batch.instances.len() && batch.instances[j].clip == *clip {
                                j += 1;
                            }
                            if let Some(scissor) = clip_to_scissor(clip, frame) {
                                pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
                                let start = (text_cursor as usize + i) as u64 * TEXT_STRIDE;
                                let end = (text_cursor as usize + j) as u64 * TEXT_STRIDE;
                                pass.set_vertex_buffer(0, self.text_instances.slice(start..end));
                                pass.draw(0..4, 0..((j - i) as u32));
                            }
                            i = j;
                        }
                        text_cursor += count;
                        if std::env::var_os("TSGPU_DEBUG_PAINT").is_some() {
                            println!("paint batch: n={count} cursor={text_cursor} bind_group={} uploaded_gen={}", self.text_bind_group.is_some(), self.uploaded_generation);
                        }                        glyph_draws += count;
                    }
                    types::DrawOp::Text(_) => {
                        // Replaced by `DrawOp::Glyphs` in text.rs before paint sees the frame.
                    }
                }
            }
        }

        drop(pass);
    }

    /// (Re)create the atlas texture when its size or channel format changed, rebuild the text
    /// bind group, and upload the pixels.
    fn upload_atlas(&mut self, device: &wgpu::Device, atlas: &types::GlyphAtlas) {
        self.uploaded_generation = atlas.generation;
        let (w, h) = (atlas.width, atlas.height);
        if w == 0 || h == 0 {
            return; // empty atlas; keep whatever is on the GPU (or nothing)
        }
        let single_channel = atlas.rgba.len() == (w as usize) * (h as usize);
        let bytes_per_px: usize = if single_channel { 1 } else { 4 };
        let bytes_per_row = w as usize * bytes_per_px;
        if atlas.rgba.len() != bytes_per_row * h as usize {
            // Mismatched buffer: text.rs bug. Record the generation so we don't retry every
            // frame, and keep the previous atlas.
            return;
        }
        let format = if single_channel {
            wgpu::TextureFormat::R8Unorm
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        let recreate = self
            .atlas
            .as_ref()
            .is_none_or(|a| a.width != w || a.height != h || a.single_channel != single_channel);
        if recreate {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tsgpu glyph atlas"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.atlas = Some(AtlasGpu { texture, view, width: w, height: h, single_channel });
            let a = self.atlas.as_ref().expect("just set");
            self.text_bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tsgpu text bind group"),
                layout: &self.text_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.text_globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&a.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }));
        }

        // Upload. Texture copies require bytes_per_row to be 256-aligned, which an arbitrary
        // atlas width never is — so fall back to one write per row (buffer_offset per row is
        // fine since each write covers a single-row texture extent).
        let tex = self.atlas.as_ref().expect("atlas present").texture.clone();
        let data = &atlas.rgba;
        if bytes_per_row % 256 == 0 {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        } else {
            for row in 0..h {
                let offset = row as usize * bytes_per_row;
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: row, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &data[offset..offset + bytes_per_row],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: None,
                        rows_per_image: None,
                    },
                    wgpu::Extent3d { width: w, height: 1, depth_or_array_layers: 1 },
                );
            }
        }
    }
}

#[cfg(test)]
mod offscreen_tests {
    use super::*;
    use crate::gpu::backend::App;
    use crate::gpu::types::*;

    /// Renders rame into an offscreen RGBA8 texture and returns the pixels.
    /// This is the headless framebuffer harness: CI-runnable, no window needed.
    fn render_offscreen(painter: &mut Painter, device: &wgpu::Device, queue: &wgpu::Queue, frame: &PaintFrame, format: wgpu::TextureFormat) -> Vec<u8> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tsgpu offscreen"),
            size: wgpu::Extent3d { width: frame.width, height: frame.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tsgpu offscreen encoder") });
        painter.paint(device, &mut encoder, &view, frame);
        queue.submit(Some(encoder.finish()));

        let bytes_per_row = frame.width * 4;
        let padded = (bytes_per_row + 255) / 256 * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tsgpu readback"),
            size: (padded * frame.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tsgpu readback encoder") });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(frame.height) },
            },
            wgpu::Extent3d { width: frame.width, height: frame.height, depth_or_array_layers: 1 },
        );
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
        device.poll(wgpu::PollType::Wait).ok();
        rx.recv().expect("map result").expect("map ok");
        let data = slice.get_mapped_range().to_vec();
        readback.unmap();
        // Strip row padding.
        let mut out = Vec::with_capacity((frame.width * frame.height * 4) as usize);
        for row in 0..frame.height {
            let start = (row * padded) as usize;
            out.extend_from_slice(&data[start..start + (frame.width * 4) as usize]);
        }
        out
    }

    /// Counts pixels whose alpha exceeds 	hreshold in the logical-px rect region.
    fn opaque_in(pixels: &[u8], width: u32, region: &Rect) -> u32 {
        let mut count = 0u32;
        for y in region.y as u32..(region.y + region.h) as u32 {
            for x in region.x as u32..(region.x + region.w) as u32 {
                let a = pixels[((y * width + x) * 4 + 3) as usize];
                if a > 64 { count += 1; }
            }
        }
        count
    }

    /// Minimal repro: one text run, no rects. Isolates the glyph pipeline from any
    /// interference, with a human-inspectable magnified crop of the render.
    #[test]
    fn offscreen_minimal_text_repro() {
        let json = r##"{"format":"tsgpu-1","name":"app","title":"T","width":400,"height":160,
            "backend_js":"","root":{"tag":"div","key":1,"style":{"background":"#090b14","color":"#ffffff"},
            "children":[{"tag":"div","key":2,"style":{"background":"#ff0000","color":"#ffffff","font-size":"48px","font-weight":"700"},"children":[{"tag":"#text","key":3,"text":"TS-GPU"}]}]}}"##;
        let doc: crate::package::AppPackage = serde_json::from_str(json).unwrap();
        let mut app = App::new(doc);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        })) else {
            eprintln!("skipping: no GPU adapter available");
            return;
        };
        let info = adapter.get_info();
        println!("ADAPTER: backend={:?} device={} vendor={:#x}", info.backend, info.name, info.vendor);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        // Surface validation errors that would otherwise vanish with the window subsystem.
        device.on_uncaptured_error(Box::new(|error| println!("WGPU ERROR: {error}")));
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut painter = Painter::new(&device, &queue, format);
        painter.resize(400, 160, 1.0);

        let frame = app.frame(1.0, (400, 160));
        for op in frame.ops.iter() {
            match op {
                types::DrawOp::Rect(r) => println!("rect node={} x={:.1} y={:.1} w={:.1} h={:.1}", r.node, r.rect.x, r.rect.y, r.rect.w, r.rect.h),
                types::DrawOp::Text(t) => println!("text node={} x={:.1} y={:.1} size={:.1} text={:?}", t.node, t.x, t.y, t.size, t.text.chars().take(8).collect::<String>()),
                types::DrawOp::Glyphs(g) => println!("glyphs n={}", g.instances.len()),
            }
        }
        let atlas = app.atlas(); println!("atlas: {}x{} gen={}", atlas.width, atlas.height, atlas.generation);
        {
            let mut alpha_view = image::RgbaImage::new(atlas.width, atlas.height);
            for (x, y, p) in alpha_view.enumerate_pixels_mut() {
                let a = atlas.rgba[((y * atlas.width + x) * 4 + 3) as usize];
                *p = image::Rgba([a, a, a, 255]);
            }
            let _ = alpha_view.save(std::env::temp_dir().join("tsgpu-minimal-atlas.png"));
        }
        painter.set_glyph_atlas(app.atlas());
        let pixels = render_offscreen(&mut painter, &device, &queue, &frame, format);
        if let Some(img) = image::RgbaImage::from_raw(400, 160, pixels.clone()) {
            let path = std::env::temp_dir().join("tsgpu-minimal-repro.png");
            let _ = img.save(&path);
            println!("minimal repro saved (full 400x160): {}", path.display());
        }
    }
    #[test]
    fn offscreen_counter_renders_text_pixels() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/counter/dist/tsgpu-counter.tgp");
        let Ok(json) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: counter bundle not built at {}", path.display());
            return;
        };
        let doc: crate::package::AppPackage = serde_json::from_str(&json).expect("parse bundle");
        let mut app = App::new(doc);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        })) else {
            eprintln!("skipping: no GPU adapter available");
            return;
        };
        let info = adapter.get_info();
        println!("ADAPTER: backend={:?} device={} vendor={:#x}", info.backend, info.name, info.vendor);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        // The live window surface on Windows is typically Bgra8UnormSrgb; verify the pipeline
        // is correct on that exact target, not just the neutral Rgba8Unorm offscreen path.
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let mut painter = Painter::new(&device, &queue, format);
        painter.resize(1125, 750, 1.25);

        let frame = app.frame(1.25, (1125, 750));
        painter.set_glyph_atlas(app.atlas());
        // Paint, then paint a SECOND frame with a FRESH atlas (the live app rebuilds the atlas
        // every frame, so the texture/view/bind-group are recreated while earlier frames may
        // still be in flight â€” this is the live-window cycle).
        let _ = render_offscreen(&mut painter, &device, &queue, &frame, format);
        let frame2 = app.frame(1.25, (1125, 750));
        painter.set_glyph_atlas(app.atlas());
        let pixels = render_offscreen(&mut painter, &device, &queue, &frame2, format);

        // The h1 "TS-GPU" glyphs: logical x 166..380, y ~126..166, at 1.25 scale => physical.
        let heading = opaque_in(&pixels, 1125, &Rect { x: 205.0, y: 155.0, w: 280.0, h: 65.0 });
        println!("heading opaque px: {heading}");
        // Save the render for visual inspection â€” pixel counts cannot distinguish correct
        // glyphs from fragments.
        if let Some(img) = image::RgbaImage::from_raw(1125, 750, pixels.clone()) {
            let path = std::env::temp_dir().join("tsgpu-offscreen-view.png");
            let _ = img.save(&path);
            println!("offscreen saved: {}", path.display());
        }
        assert!(heading > 300, "the h1 heading should paint hundreds of text pixels, got {heading}");
    }
}

//! The wgpu renderer: flattens a [`DisplayList`] into GPU instances and draws them.

use crate::display::*;
use crate::text::TextSystem;
use bytemuck::{Pod, Zeroable};
use rendrix_css::animation::{mat_apply, mat_mul, Matrix, IDENTITY};
use rendrix_css::Color;
use std::collections::HashMap;

const ATLAS_SIZE: u32 = 2048;
const NO_CLIP: [f32; 4] = [-1.0e6, -1.0e6, 2.0e6, 2.0e6];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct Inst {
    rect: [f32; 4],
    xf0: [f32; 4],
    xf1: [f32; 4],
    c: [[f32; 4]; 4],
    stops: [f32; 4],
    radii: [f32; 4],
    border: [f32; 4],
    bcolor: [f32; 4],
    clip: [f32; 4],
    uv: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    pad: [f32; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tex {
    #[allow(dead_code)]
    White,
    Atlas,
    Image(ImageId),
}

struct Batch {
    tex: Tex,
    start: u32,
    end: u32,
}

struct ImageTex {
    bind: wgpu::BindGroup,
    #[allow(dead_code)]
    texture: wgpu::Texture,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy)]
struct AtlasEntry {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
    is_color: bool,
}

struct Atlas {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    map: HashMap<cosmic_text::CacheKey, Option<AtlasEntry>>,
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
    pub evictions: u32,
}

struct State {
    xf: Vec<Matrix>,
    clip: Vec<([f32; 4], f32)>,
    opacity: Vec<f32>,
}

pub struct Renderer {
    format: wgpu::TextureFormat,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    g_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white_bind: wgpu::BindGroup,
    atlas: Atlas,
    images: HashMap<ImageId, ImageTex>,
    next_image: ImageId,
    inst_buf: wgpu::Buffer,
    inst_cap: usize,
    /// Number of instances drawn in the last frame (profiling).
    pub last_instances: usize,
    pub last_batches: usize,
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Renderer {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rendrix shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let g_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rendrix globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rendrix texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("rendrix layout"), bind_group_layouts: &[&g_layout, &tex_layout], push_constant_ranges: &[] });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rendrix pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("rendrix globals"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let make_tex = |w: u32, h: u32, label: &str| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let bind_for = |tex: &wgpu::Texture| {
            let view = tex.create_view(&Default::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rendrix tex bind"),
                layout: &tex_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
            })
        };
        let white = make_tex(1, 1, "rendrix white");
        queue.write_texture(
            white.as_image_copy(),
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let white_bind = bind_for(&white);
        let atlas_tex = make_tex(ATLAS_SIZE, ATLAS_SIZE, "rendrix glyph atlas");
        // A 2x2 opaque white block at the atlas origin: shapes sample it, so rects, shadows and glyphs
        // share one texture and consecutive draws merge into a single batch.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &atlas_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &[255u8; 16],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: Some(2) },
            wgpu::Extent3d { width: 2, height: 2, depth_or_array_layers: 1 },
        );
        let atlas_bind = bind_for(&atlas_tex);
        let inst_cap = 1024;
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rendrix instances"),
            size: (inst_cap * std::mem::size_of::<Inst>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Renderer {
            format,
            device,
            queue,
            pipeline,
            globals_buf,
            g_layout,
            tex_layout,
            sampler,
            white_bind,
            atlas: Atlas { texture: atlas_tex, bind: atlas_bind, map: HashMap::new(), cursor_x: 3, cursor_y: 0, row_h: 3, evictions: 0 },
            images: HashMap::new(),
            next_image: 1,
            inst_buf,
            inst_cap,
            last_instances: 0,
            last_batches: 0,
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Upload straight-alpha RGBA8 pixels; returns a handle for [`ImageItem`].
    pub fn upload_image(&mut self, rgba: &[u8], width: u32, height: u32) -> ImageId {
        let id = self.next_image;
        self.next_image += 1;
        self.upload_image_as(id, rgba, width, height);
        id
    }

    /// Upload under a caller-chosen id (the engine owns id allocation).
    pub fn upload_image_as(&mut self, id: ImageId, rgba: &[u8], width: u32, height: u32) {
        let mut pm = rgba.to_vec();
        for px in pm.chunks_exact_mut(4) {
            let a = px[3] as u32;
            px[0] = (px[0] as u32 * a / 255) as u8;
            px[1] = (px[1] as u32 * a / 255) as u8;
            px[2] = (px[2] as u32 * a / 255) as u8;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rendrix image"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            &pm,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rendrix image bind"),
            layout: &self.tex_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
        });
        self.images.insert(id, ImageTex { bind, texture, width, height });
    }

    pub fn image_size(&self, id: ImageId) -> Option<(u32, u32)> {
        self.images.get(&id).map(|i| (i.width, i.height))
    }

    pub fn free_image(&mut self, id: ImageId) {
        self.images.remove(&id);
    }

    // ------------------------------------------------------------------------------------

    fn atlas_entry(&mut self, text: &mut TextSystem, key: cosmic_text::CacheKey) -> Option<AtlasEntry> {
        if let Some(e) = self.atlas.map.get(&key) {
            return *e;
        }
        let Some(r) = text.raster(key) else {
            self.atlas.map.insert(key, None);
            return None;
        };
        let (w, h) = (r.width + 1, r.height + 1);
        if self.atlas.cursor_x + w > ATLAS_SIZE {
            self.atlas.cursor_x = 0;
            self.atlas.cursor_y += self.atlas.row_h;
            self.atlas.row_h = 0;
        }
        if self.atlas.cursor_y + h > ATLAS_SIZE {
            // Full: start over. Glyphs already emitted this frame may show stale data for one frame.
            self.atlas.map.clear();
            self.atlas.cursor_x = 3;
            self.atlas.cursor_y = 0;
            self.atlas.row_h = 3;
            self.atlas.evictions += 1;
        }
        let (x, y) = (self.atlas.cursor_x, self.atlas.cursor_y);
        self.atlas.cursor_x += w;
        self.atlas.row_h = self.atlas.row_h.max(h);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.atlas.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            &r.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(r.width * 4), rows_per_image: Some(r.height) },
            wgpu::Extent3d { width: r.width, height: r.height, depth_or_array_layers: 1 },
        );
        let e = AtlasEntry { x, y, w: r.width, h: r.height, left: r.left, top: r.top, is_color: r.is_color };
        self.atlas.map.insert(key, Some(e));
        Some(e)
    }

    /// Draw a display list into `target` (`size_px` physical pixels, `scale` device px per logical px).
    pub fn render(&mut self, list: &DisplayList, text: &mut TextSystem, target: &wgpu::TextureView, size_px: (u32, u32), scale: f32) {
        let mut insts: Vec<Inst> = Vec::with_capacity(512);
        let mut batches: Vec<Batch> = Vec::new();
        self.flatten(list, text, scale, &mut insts, &mut batches);
        self.last_instances = insts.len();
        self.last_batches = batches.len();

        if insts.len() > self.inst_cap {
            self.inst_cap = insts.len().next_power_of_two();
            self.inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rendrix instances"),
                size: (self.inst_cap * std::mem::size_of::<Inst>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !insts.is_empty() {
            self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&insts));
        }
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&Globals { viewport: [size_px.0 as f32, size_px.1 as f32], pad: [0.0; 2] }));
        let g_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rendrix globals bind"),
            layout: &self.g_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.inst_buf.as_entire_binding() },
            ],
        });

        let bg = list.background;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rendrix frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rendrix pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg.r as f64, g: bg.g as f64, b: bg.b as f64, a: bg.a as f64 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &g_bind, &[]);
            for b in &batches {
                let bind = match b.tex {
                    Tex::White => &self.white_bind,
                    Tex::Atlas => &self.atlas.bind,
                    Tex::Image(id) => match self.images.get(&id) {
                        Some(i) => &i.bind,
                        None => continue,
                    },
                };
                pass.set_bind_group(1, bind, &[]);
                pass.draw(0..4, b.start..b.end);
            }
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Render to an offscreen RGBA8 image and read it back (used for screenshots and tests).
    pub fn render_to_rgba(&mut self, list: &DisplayList, text: &mut TextSystem, size_px: (u32, u32), scale: f32) -> Vec<u8> {
        let (w, h) = size_px;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rendrix capture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.render(list, text, &view, size_px, scale);
        let padded = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("rendrix readback"), size: (padded * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rendrix readback") });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::Wait);
        rx.recv().expect("map channel").expect("map readback");
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let s = (row * padded) as usize;
            out.extend_from_slice(&data[s..s + (w * 4) as usize]);
        }
        drop(data);
        buf.unmap();
        if matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        out
    }

    // ------------------------------------------------------------------------------------

    fn flatten(&mut self, list: &DisplayList, text: &mut TextSystem, scale: f32, insts: &mut Vec<Inst>, batches: &mut Vec<Batch>) {
        let mut st = State { xf: vec![[scale, 0.0, 0.0, scale, 0.0, 0.0]], clip: vec![(NO_CLIP, 0.0)], opacity: vec![1.0] };
        let push = |insts: &mut Vec<Inst>, batches: &mut Vec<Batch>, tex: Tex, inst: Inst| {
            let idx = insts.len() as u32;
            insts.push(inst);
            match batches.last_mut() {
                Some(b) if b.tex == tex && b.end == idx => b.end += 1,
                _ => batches.push(Batch { tex, start: idx, end: idx + 1 }),
            }
        };

        for item in &list.items {
            let xf = *st.xf.last().unwrap();
            let (clip, clip_r) = *st.clip.last().unwrap();
            let op = *st.opacity.last().unwrap();
            let base = |rect: [f32; 4], kind: f32| Inst {
                rect,
                xf0: [xf[0], xf[1], xf[2], xf[3]],
                xf1: [xf[4], xf[5], kind, 0.0],
                clip,
                // Shapes sample the atlas's white block (see `Renderer::new`).
                uv: [1.0 / ATLAS_SIZE as f32, 1.0 / ATLAS_SIZE as f32, 0.0, 0.0],
                extra: [0.0, 0.0, clip_r, 0.0],
                ..Default::default()
            };
            match item {
                Item::Rect(r) => {
                    let mut i = base(r.rect, 0.0);
                    i.radii = clamp_radii(r.radii, r.rect[2], r.rect[3]);
                    match &r.fill {
                        Fill::None => {}
                        Fill::Solid(c) => i.c[0] = mul_alpha(*c, op),
                        Fill::Linear { angle, stops } => set_gradient(&mut i, 1.0, *angle, stops, op),
                        Fill::Radial { stops } => set_gradient(&mut i, 2.0, 0.0, stops, op),
                    }
                    if let Some((w, c)) = &r.border {
                        i.border = *w;
                        i.bcolor = mul_alpha(*c, op);
                    }
                    if matches!(r.fill, Fill::None) && r.border.is_none() {
                        continue;
                    }
                    push(insts, batches, Tex::Atlas, i);
                }
                Item::Shadow(s) => {
                    let mut i = base(s.rect, if s.inset { 2.0 } else { 1.0 });
                    i.radii = clamp_radii(s.radii, s.rect[2], s.rect[3]);
                    i.c[0] = mul_alpha(s.color, op);
                    i.extra[0] = s.blur;
                    i.extra[3] = if s.inset { 0.0 } else { s.blur };
                    if s.inset {
                        i.border = s.hole;
                        i.bcolor = clamp_radii(s.radii, s.hole[2], s.hole[3]);
                    }
                    push(insts, batches, Tex::Atlas, i);
                }
                Item::Image(im) => {
                    let mut i = base(im.rect, 4.0);
                    i.radii = clamp_radii(im.radii, im.rect[2], im.rect[3]);
                    i.c[0] = [1.0, 1.0, 1.0, op];
                    i.uv = im.uv;
                    push(insts, batches, Tex::Image(im.image), i);
                }
                Item::Text(t) => {
                    let para = text.paragraph(&t.content, Some(t.width));
                    let simple = xf[1].abs() < 1e-6 && xf[2].abs() < 1e-6;
                    let sc = (xf[0] * xf[3] - xf[1] * xf[2]).abs().sqrt().max(1e-3);
                    for g in &para.glyphs {
                        let run = &t.content.runs[g.run.min(t.content.runs.len() - 1)];
                        let color = mul_alpha(run.style.color, op);
                        if color[3] <= 0.0 {
                            continue;
                        }
                        let (ox, oy) = (t.x, t.y + g.line_y);
                        let phys = g.glyph.physical((0.0, 0.0), sc);
                        let Some(e) = self.atlas_entry(text, phys.cache_key) else { continue };
                        let uv = [e.x as f32 / ATLAS_SIZE as f32, e.y as f32 / ATLAS_SIZE as f32, e.w as f32 / ATLAS_SIZE as f32, e.h as f32 / ATLAS_SIZE as f32];
                        let kind = if e.is_color { 4.0 } else { 3.0 };
                        let mut i;
                        if simple {
                            // Pixel-snapped placement in device space.
                            // `phys` already carries the glyph's own x/y offset within the line.
                            let (dx, dy) = mat_apply(xf, ox, oy);
                            let px = (dx.round() as i32 + phys.x + e.left) as f32;
                            let py = (dy.round() as i32 + phys.y - e.top) as f32;
                            i = Inst { rect: [px, py, e.w as f32, e.h as f32], xf0: [1.0, 0.0, 0.0, 1.0], xf1: [0.0, 0.0, kind, 0.0], clip, extra: [0.0, 0.0, clip_r, 0.0], ..Default::default() };
                        } else {
                            // Rotated/skewed text: place the glyph quad in local space and let the matrix transform it.
                            let lx = ox + (phys.x + e.left) as f32 / sc;
                            let ly = oy + (phys.y - e.top) as f32 / sc;
                            i = base([lx, ly, e.w as f32 / sc, e.h as f32 / sc], kind);
                        }
                        i.c[0] = color;
                        i.uv = uv;
                        push(insts, batches, Tex::Atlas, i);
                    }
                    // Underline / line-through spans.
                    for seg in &para.segments {
                        let run = &t.content.runs[seg.run.min(t.content.runs.len() - 1)];
                        let deco = run.style.text_decoration;
                        if !(deco.underline || deco.line_through || deco.overline) {
                            continue;
                        }
                        let thick = (run.style.font_size / 14.0).max(1.0);
                        let mut lines = Vec::new();
                        if deco.underline {
                            lines.push(seg.line_y + run.style.font_size * 0.12);
                        }
                        if deco.line_through {
                            lines.push(seg.line_y - run.style.font_size * 0.3);
                        }
                        if deco.overline {
                            lines.push(seg.line_y - run.style.font_size * 0.85);
                        }
                        for y in lines {
                            let mut i = base([t.x + seg.x, t.y + y, seg.w, thick], 0.0);
                            i.c[0] = mul_alpha(run.style.color, op);
                            push(insts, batches, Tex::Atlas, i);
                        }
                    }
                }
                Item::PushClip { rect, radius } => {
                    let corners = [mat_apply(xf, rect[0], rect[1]), mat_apply(xf, rect[0] + rect[2], rect[1]), mat_apply(xf, rect[0], rect[1] + rect[3]), mat_apply(xf, rect[0] + rect[2], rect[1] + rect[3])];
                    let (x0, x1) = corners.iter().fold((f32::MAX, f32::MIN), |a, c| (a.0.min(c.0), a.1.max(c.0)));
                    let (y0, y1) = corners.iter().fold((f32::MAX, f32::MIN), |a, c| (a.0.min(c.1), a.1.max(c.1)));
                    let (px0, py0, px1, py1) = (clip[0], clip[1], clip[0] + clip[2], clip[1] + clip[3]);
                    let (nx0, ny0, nx1, ny1) = (x0.max(px0), y0.max(py0), x1.min(px1), y1.min(py1));
                    let sc = (xf[0] * xf[3] - xf[1] * xf[2]).abs().sqrt();
                    st.clip.push(([nx0, ny0, (nx1 - nx0).max(0.0), (ny1 - ny0).max(0.0)], radius * sc));
                }
                Item::PopClip => {
                    if st.clip.len() > 1 {
                        st.clip.pop();
                    }
                }
                Item::PushTransform(m) => st.xf.push(mat_mul(xf, *m)),
                Item::PopTransform => {
                    if st.xf.len() > 1 {
                        st.xf.pop();
                    }
                }
                Item::PushOpacity(o) => st.opacity.push(op * o),
                Item::PopOpacity => {
                    if st.opacity.len() > 1 {
                        st.opacity.pop();
                    }
                }
            }
        }
        let _ = IDENTITY;
    }
}

fn mul_alpha(c: Color, k: f32) -> [f32; 4] {
    [c.r, c.g, c.b, c.a * k]
}

fn clamp_radii(r: [f32; 4], w: f32, h: f32) -> [f32; 4] {
    // CSS: scale all radii down uniformly if adjacent radii exceed a side.
    let f = [
        if r[0] + r[1] > 0.0 { w / (r[0] + r[1]) } else { f32::MAX },
        if r[1] + r[2] > 0.0 { h / (r[1] + r[2]) } else { f32::MAX },
        if r[2] + r[3] > 0.0 { w / (r[2] + r[3]) } else { f32::MAX },
        if r[3] + r[0] > 0.0 { h / (r[3] + r[0]) } else { f32::MAX },
    ]
    .into_iter()
    .fold(1.0f32, f32::min);
    [r[0] * f, r[1] * f, r[2] * f, r[3] * f]
}

fn set_gradient(i: &mut Inst, kind: f32, angle: f32, stops: &[(Color, f32)], op: f32) {
    let n = stops.len().min(4);
    i.xf1[3] = kind;
    i.extra[0] = angle;
    i.extra[1] = n as f32;
    for (k, (c, p)) in stops.iter().take(4).enumerate() {
        i.c[k] = mul_alpha(*c, op);
        i.stops[k] = *p;
    }
    // Stops beyond the 4th are dropped; make the last kept stop the end of the ramp.
    if stops.len() > 4 {
        i.stops[3] = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::GpuContext;
    use rendrix_css::Color;

    fn render(list: &DisplayList, w: u32, h: u32) -> Option<Vec<u8>> {
        let ctx = GpuContext::headless().ok()?;
        let mut r = Renderer::new(ctx.device, ctx.queue, wgpu::TextureFormat::Rgba8Unorm);
        let mut t = TextSystem::new();
        Some(r.render_to_rgba(list, &mut t, (w, h), 1.0))
    }

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    fn rect(x: f32, y: f32, w: f32, h: f32, c: Color) -> Item {
        Item::Rect(RectItem { rect: [x, y, w, h], radii: [0.0; 4], fill: Fill::Solid(c), border: None })
    }

    #[test]
    fn draws_filled_rects_in_order() {
        let mut l = DisplayList::new(64.0, 64.0, Color::WHITE);
        l.push(rect(8.0, 8.0, 32.0, 32.0, Color::from_u8(255, 0, 0, 1.0)));
        l.push(rect(24.0, 24.0, 32.0, 32.0, Color::from_u8(0, 0, 255, 1.0)));
        let Some(img) = render(&l, 64, 64) else { eprintln!("no GPU adapter, skipping");  return };
        assert_eq!(px(&img, 64, 2, 2), [255, 255, 255, 255]);
        assert_eq!(px(&img, 64, 12, 12), [255, 0, 0, 255]);
        assert_eq!(px(&img, 64, 30, 30), [0, 0, 255, 255], "later item paints over earlier");
        assert_eq!(px(&img, 64, 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn rounded_corners_borders_and_clip() {
        let mut l = DisplayList::new(64.0, 64.0, Color::WHITE);
        l.push(Item::Rect(RectItem { rect: [4.0, 4.0, 56.0, 56.0], radii: [20.0; 4], fill: Fill::Solid(Color::BLACK), border: Some(([4.0; 4], Color::from_u8(0, 255, 0, 1.0))) }));
        l.push(Item::PushClip { rect: [0.0, 0.0, 64.0, 10.0], radius: 0.0 });
        l.push(rect(0.0, 0.0, 64.0, 64.0, Color::from_u8(255, 0, 255, 0.5)));
        l.push(Item::PopClip);
        let Some(img) = render(&l, 64, 64) else { return };
        assert_eq!(px(&img, 64, 1, 30), [255, 255, 255, 255], "outside the box");
        let corner = px(&img, 64, 5, 33);
        assert!(corner[1] > 200 && corner[0] < 60, "left border is green: {corner:?}");
        assert_eq!(px(&img, 64, 1, 11), [255, 255, 255, 255], "rounded corner shows background");
        let center = px(&img, 64, 32, 32);
        assert_eq!(center, [0, 0, 0, 255], "fill inside the border; clip excluded the magenta wash");
        let clipped = px(&img, 64, 1, 5);
        assert!(clipped[0] > 240 && clipped[2] > 240 && (clipped[1] as i32 - 128).abs() < 4, "inside clip: translucent magenta over white: {clipped:?}");
        assert_eq!(px(&img, 64, 1, 20), [255, 255, 255, 255], "below the clip nothing is washed");
    }

    #[test]
    fn linear_gradient_runs_left_to_right() {
        let mut l = DisplayList::new(64.0, 8.0, Color::WHITE);
        l.push(Item::Rect(RectItem {
            rect: [0.0, 0.0, 64.0, 8.0],
            radii: [0.0; 4],
            fill: Fill::Linear { angle: std::f32::consts::FRAC_PI_2, stops: vec![(Color::BLACK, 0.0), (Color::WHITE, 1.0)] },
            border: None,
        }));
        let Some(img) = render(&l, 64, 8) else { return };
        let (a, b, c) = (px(&img, 64, 2, 4)[0], px(&img, 64, 32, 4)[0], px(&img, 64, 61, 4)[0]);
        assert!(a < 30 && (b as i32 - 128).abs() < 12 && c > 225, "{a} {b} {c}");
    }

    #[test]
    fn transform_rotates_geometry() {
        let mut l = DisplayList::new(100.0, 100.0, Color::WHITE);
        l.push(Item::PushTransform([0.0, 1.0, -1.0, 0.0, 100.0, 0.0])); // rotate 90° about origin, shifted right
        l.push(rect(0.0, 0.0, 40.0, 10.0, Color::BLACK)); // becomes a 10-wide, 40-tall column at x=90..100
        l.push(Item::PopTransform);
        let Some(img) = render(&l, 100, 100) else { return };
        assert_eq!(px(&img, 100, 95, 30), [0, 0, 0, 255]);
        assert_eq!(px(&img, 100, 30, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn text_ink_matches_the_measured_extent() {
        use rendrix_css::ComputedStyle;
        use rendrix_layout::{InlineContent, InlineRun, TextMeasurer};
        use std::sync::Arc;
        let mut s = ComputedStyle::default();
        s.font_size = 32.0;
        s.color = Color::BLACK;
        let s = Arc::new(s);
        let content = Arc::new(InlineContent { runs: vec![InlineRun { text: "Hello, Rendrix".into(), style: s.clone(), node: 1, is_break: false }], container: s });
        let mut ts = TextSystem::new();
        let (mw, mh) = ts.measure(&content, None);
        let (x0, y0) = (10.0f32, 8.0f32);
        let (w, h) = ((mw + 40.0) as u32, (mh + 24.0) as u32);
        let mut l = DisplayList::new(w as f32, h as f32, Color::WHITE);
        l.push(Item::Text(TextItem { content, x: x0, y: y0, width: mw }));
        let Some(ctx) = GpuContext::headless().ok() else { return };
        let mut r = Renderer::new(ctx.device, ctx.queue, wgpu::TextureFormat::Rgba8Unorm);
        let img = r.render_to_rgba(&l, &mut ts, (w, h), 1.0);
        let mut ink: Vec<(u32, u32)> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if px(&img, w, x, y)[0] < 128 {
                    ink.push((x, y));
                }
            }
        }
        assert!(ink.len() > 80, "text produced ink: {}", ink.len());
        let (minx, maxx) = (ink.iter().map(|p| p.0).min().unwrap() as f32, ink.iter().map(|p| p.0).max().unwrap() as f32);
        let (miny, maxy) = (ink.iter().map(|p| p.1).min().unwrap() as f32, ink.iter().map(|p| p.1).max().unwrap() as f32);
        assert!(minx >= x0 - 1.0 && minx <= x0 + 6.0, "ink starts at the text origin: {minx}");
        assert!(maxx <= x0 + mw + 1.0 && maxx >= x0 + mw * 0.85, "ink ends at the measured width ({maxx} vs {}), i.e. glyphs are not double-spaced", x0 + mw);
        assert!(miny >= y0 - 1.0 && maxy <= y0 + mh + 1.0, "ink stays inside the measured line box: {miny}..{maxy} vs {y0}..{}", y0 + mh);
    }

    #[test]
    fn box_shadow_is_soft_and_outside() {
        let mut l = DisplayList::new(80.0, 80.0, Color::WHITE);
        l.push(Item::Shadow(ShadowItem { rect: [20.0, 20.0, 40.0, 40.0], radii: [4.0; 4], blur: 16.0, color: Color::BLACK, inset: false, hole: [0.0; 4] }));
        let Some(img) = render(&l, 80, 80) else { return };
        let inside = px(&img, 80, 40, 40)[0];
        let near = px(&img, 80, 62, 40)[0];
        let far = px(&img, 80, 78, 40)[0];
        assert!(inside < 40, "dense under the box: {inside}");
        assert!(near > inside && near < 255 && far > near, "falls off with distance: {inside} {near} {far}");
    }
}

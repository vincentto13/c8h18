//! wgpu renderer: 3D world pass (terrain, models, billboards, sky) and the indexed 2D canvas composite.
pub mod canvas;
pub mod model;

use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

pub const FOG_START: f32 = 12.0;
pub const FOG_END: f32 = 16.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, Default)]
pub struct Vertex {
    /// world: x (east, cells), height (cells), row (south, cells)
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// light-table row (32 = unchanged)
    pub light: f32,
    /// low byte: 0 terrain texture, 1 model atlas, 2 flat colour, 3 billboard; >>8: fog 0 none, 1 terrain, 2 sprite
    pub kind: u32,
    /// terrain: texture id; flat: palette index; billboard: blend byte
    pub param: u32,
}

pub const K_TERRAIN: u32 = 0 | (1 << 8);
pub const K_ATLAS: u32 = 1;
pub const K_FLAT: u32 = 2;
pub const K_SPRITE: u32 = 3 | (2 << 8);
pub const K_FLAT_TERRAIN: u32 = 2 | (1 << 8);

/// Camera: position (x, row, height in cells) and 16-bit angles as f32 raw units (65536 per turn).
#[derive(Clone, Copy, Debug, Default)]
pub struct Camera {
    pub x: f32,
    pub row: f32,
    pub height: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// focal length in pixels at 320 width (94 in the race, 240 in the carousels)
    pub focal320: f32,
    /// focal relative to the central 4:3 area (window height × 4/3) instead of the full window width
    pub fixed_43: bool,
}

pub fn ang(raw: f32) -> f32 {
    raw / 65536.0 * std::f32::consts::TAU
}

impl Camera {
    /// (right, up, forward) in world (x, height, row) coordinates.
    pub fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let (sy, cy) = ang(self.yaw).sin_cos();
        let (sp, cp) = ang(self.pitch).sin_cos();
        let (sr, cr) = ang(self.roll).sin_cos();
        let fwd = [sy * cp, -sp, -cy * cp];
        let right0 = [cy, 0.0, sy];
        // up = right x fwd ... compute explicitly: up0 for pitch
        let up0 = [sy * sp, cp, -cy * sp];
        let right = [right0[0] * cr - up0[0] * sr, right0[1] * cr - up0[1] * sr, right0[2] * cr - up0[2] * sr];
        let up = [up0[0] * cr + right0[0] * sr, up0[1] * cr + right0[1] * sr, up0[2] * cr + right0[2] * sr];
        (right, up, fwd)
    }
    pub fn eye(&self) -> [f32; 3] {
        [self.x, self.height, self.row]
    }
    /// View-space coordinates (vx right, vy up, vz depth) of a world point (x, height, row).
    pub fn to_view(&self, p: [f32; 3]) -> [f32; 3] {
        let (r, u, f) = self.basis();
        let d = [p[0] - self.x, p[1] - self.height, p[2] - self.row];
        let dot = |a: [f32; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        [dot(r), dot(u), dot(f)]
    }
}

/// What to draw in 3D this frame.
#[derive(Default)]
pub struct Scene {
    pub camera: Camera,
    /// draw the sky first (else clear to the haze colour when `world` is set)
    pub sky: bool,
    /// draw the uploaded level mesh and fill the background (race view); false = transparent clear (carousels)
    pub world: bool,
    pub opaque: Vec<Vertex>,
    /// translucent / alpha-tested triangles, drawn after opaque, in the given order (sort far to near)
    pub sprites: Vec<Vertex>,
    /// the near clip in cells
    pub near: f32,
    pub far: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct Globals {
    eye: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    fwd: [f32; 4],
    proj: [f32; 4],
    fog: [f32; 4],
    sky: [f32; 4],
    screen: [f32; 4],
    misc: [f32; 4],
}

pub struct Gfx {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    globals_buf: wgpu::Buffer,
    bind0_layout: wgpu::BindGroupLayout,
    bind1_layout: wgpu::BindGroupLayout,
    bind0: Option<wgpu::BindGroup>,
    palette_tex: wgpu::Texture,
    light_tex: wgpu::Texture,
    terrain_tex: wgpu::Texture,
    atlas_tex: wgpu::Texture,
    tmap_tex: wgpu::Texture,
    sky_tex: wgpu::Texture,
    scene_tex: wgpu::Texture,
    depth_tex: wgpu::Texture,
    bg_tex: wgpu::Texture,
    fg_tex: wgpu::Texture,
    canvas_size: (u32, u32),
    sky_pipe: wgpu::RenderPipeline,
    world_pipe: wgpu::RenderPipeline,
    sprite_pipe: wgpu::RenderPipeline,
    comp_pipe: wgpu::RenderPipeline,
    terrain_buf: Option<(wgpu::Buffer, u32)>,
    dyn_buf: wgpu::Buffer,
    dyn_cap: u64,
    /// atlas layout of the billboard images: (x, y) per tmap image in the tmap texture
    pub tmap_pos: Vec<(u32, u32)>,
    /// save the next presented frame as PNG to this path
    pub shot: Option<String>,
}

const SCENE_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn tex2d(device: &wgpu::Device, w: u32, h: u32, layers: u32, fmt: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: layers },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: fmt,
        usage,
        view_formats: &[],
    })
}

impl Gfx {
    pub async fn new(window: Arc<Window>) -> Gfx {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("no GPU adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor { label: None, ..Default::default() })
            .await
            .expect("device");
        let size = window.inner_size();
        let mut config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("surface config");
        let caps = surface.get_capabilities(&adapter);
        config.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let tex_entry = |b: u32, st: wgpu::TextureSampleType, dim: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Texture { sample_type: st, view_dimension: dim, multisampled: false },
            count: None,
        };
        use wgpu::TextureSampleType as ST;
        use wgpu::TextureViewDimension as VD;
        let bind0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex_entry(1, ST::Float { filterable: false }, VD::D2),
                tex_entry(2, ST::Uint, VD::D2),
                tex_entry(3, ST::Uint, VD::D2Array),
                tex_entry(4, ST::Uint, VD::D2),
                tex_entry(5, ST::Uint, VD::D2),
                tex_entry(6, ST::Uint, VD::D2),
            ],
        });
        let bind1_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[tex_entry(0, ST::Float { filterable: false }, VD::D2), tex_entry(1, ST::Uint, VD::D2), tex_entry(2, ST::Uint, VD::D2)],
        });
        let layout0 = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind0_layout)],
            immediate_size: 0,
        });
        let layout01 = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind0_layout), Some(&bind1_layout)],
            immediate_size: 0,
        });
        let vbuf_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Uint32, 4 => Uint32],
        };
        let full_pipe = |fs: &str, fmt: wgpu::TextureFormat, layout: &wgpu::PipelineLayout, depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: if depth {
                    Some(wgpu::DepthStencilState {
                        format: DEPTH_FMT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    })
                } else {
                    None
                },
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: fmt, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sky_pipe = full_pipe("fs_sky", SCENE_FMT, &layout0, true);
        let comp_pipe = full_pipe("fs_composite", config.format, &layout01, false);
        let world = |blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world"),
                layout: Some(&layout0),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_world"), compilation_options: Default::default(), buffers: &[Some(vbuf_layout.clone())] },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FMT,
                    depth_write_enabled: Some(!blend),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_world"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SCENE_FMT,
                        blend: if blend {
                            Some(wgpu::BlendState {
                                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                                alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max },
                            })
                        } else {
                            None
                        },
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let world_pipe = world(false);
        let sprite_pipe = world(true);

        use wgpu::TextureFormat as TF;
        use wgpu::TextureUsages as TU;
        let sampled = TU::TEXTURE_BINDING | TU::COPY_DST;
        let palette_tex = tex2d(&device, 256, 1, 1, TF::Rgba8Unorm, sampled);
        let light_tex = tex2d(&device, 256, 328, 1, TF::R8Uint, sampled);
        let terrain_tex = tex2d(&device, 64, 64, 256, TF::R8Uint, sampled);
        let atlas_tex = tex2d(&device, 256, 768, 1, TF::R8Uint, sampled);
        let tmap_tex = tex2d(&device, 1024, 1024, 1, TF::Rg8Uint, sampled);
        let sky_tex = tex2d(&device, 256, 256, 1, TF::R8Uint, sampled);
        let (w, h) = (config.width, config.height);
        let scene_tex = tex2d(&device, w, h, 1, SCENE_FMT, TU::RENDER_ATTACHMENT | TU::TEXTURE_BINDING);
        let depth_tex = tex2d(&device, w, h, 1, DEPTH_FMT, TU::RENDER_ATTACHMENT);
        let bg_tex = tex2d(&device, 640, 480, 1, TF::R16Uint, sampled);
        let fg_tex = tex2d(&device, 640, 480, 1, TF::R16Uint, sampled);
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dyn_cap = 1 << 20;
        let dyn_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: dyn_cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut g = Gfx {
            window,
            surface,
            device,
            queue,
            config,
            globals_buf,
            bind0_layout,
            bind1_layout,
            bind0: None,
            palette_tex,
            light_tex,
            terrain_tex,
            atlas_tex,
            tmap_tex,
            sky_tex,
            scene_tex,
            depth_tex,
            bg_tex,
            fg_tex,
            canvas_size: (640, 480),
            sky_pipe,
            world_pipe,
            sprite_pipe,
            comp_pipe,
            terrain_buf: None,
            dyn_buf,
            dyn_cap,
            tmap_pos: Vec::new(),
            shot: None,
        };
        g.rebuild_bind0();
        g
    }

    fn rebuild_bind0(&mut self) {
        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let terrain_view = self.terrain_tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        self.bind0 = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bind0_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&v(&self.palette_tex)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&v(&self.light_tex)) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&terrain_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&v(&self.atlas_tex)) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&v(&self.tmap_tex)) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&v(&self.sky_tex)) },
            ],
        }));
    }

    fn write_tex(&self, tex: &wgpu::Texture, data: &[u8], w: u32, h: u32, layer: u32, bpp: u32) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: layer }, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * bpp), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Window size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Canvas size matching the window aspect: height 480, width >= 640.
    pub fn canvas_dims(&self) -> (i32, i32) {
        let (w, h) = self.size();
        let cw = ((480.0 * w as f32 / h.max(1) as f32).round() as i32).max(640);
        (cw, 480)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        use wgpu::TextureUsages as TU;
        self.scene_tex = tex2d(&self.device, w, h, 1, SCENE_FMT, TU::RENDER_ATTACHMENT | TU::TEXTURE_BINDING);
        self.depth_tex = tex2d(&self.device, w, h, 1, DEPTH_FMT, TU::RENDER_ATTACHMENT);
    }

    /// Palette as 6-bit VGA values.
    pub fn set_palette(&mut self, vga: &[[u8; 3]; 256]) {
        let mut d = Vec::with_capacity(1024);
        for c in vga {
            for &v in c {
                d.push((v << 2) | (v >> 4));
            }
            d.push(255);
        }
        self.write_tex(&self.palette_tex, &d, 256, 1, 0, 4);
    }
    pub fn set_colour_table(&mut self, data: &[u8]) {
        self.write_tex(&self.light_tex, &data[..83968], 256, 328, 0, 1);
    }
    pub fn set_terrain_textures(&mut self, data: &[u8]) {
        for i in 0..256u32 {
            let o = (i * 4096) as usize;
            self.write_tex(&self.terrain_tex, &data[o..o + 4096], 64, 64, i, 1);
        }
    }
    pub fn set_sky(&mut self, data: &[u8]) {
        self.write_tex(&self.sky_tex, &data[..65536], 256, 256, 0, 1);
    }
    pub fn set_model_atlas(&mut self, pix: &[u8], h: u32) {
        let h = h.min(768);
        self.write_tex(&self.atlas_tex, &pix[..(256 * h) as usize], 256, h, 0, 1);
    }
    /// Pack the billboard images into the tmap texture (shelf packing); colour 0 is transparent.
    pub fn set_tmaps(&mut self, imgs: &[crate::data::tmaps::TmapImage]) {
        let mut data = vec![0u8; 1024 * 1024 * 2];
        let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
        self.tmap_pos.clear();
        for im in imgs {
            if x + im.w > 1024 {
                x = 0;
                y += shelf;
                shelf = 0;
            }
            self.tmap_pos.push((x, y));
            for iy in 0..im.h {
                for ix in 0..im.w {
                    let c = im.pix[(iy * im.w + ix) as usize];
                    let o = (((y + iy) * 1024 + x + ix) * 2) as usize;
                    if o + 1 < data.len() {
                        data[o] = c;
                        data[o + 1] = (c != 0) as u8;
                    }
                }
            }
            x += im.w;
            shelf = shelf.max(im.h);
        }
        self.write_tex(&self.tmap_tex, &data, 1024, 1024, 0, 2);
    }
    /// Upload the static level mesh.
    pub fn set_terrain_mesh(&mut self, verts: &[Vertex]) {
        if verts.is_empty() {
            self.terrain_buf = None;
            return;
        }
        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain"),
            contents: bytemuck::cast_slice(verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.terrain_buf = Some((buf, verts.len() as u32));
    }

    fn upload_canvas(&mut self, bg: Option<&canvas::Canvas>, fg: &canvas::Canvas) {
        let (cw, ch) = (fg.w as u32, fg.h as u32);
        if self.canvas_size != (cw, ch) {
            use wgpu::TextureUsages as TU;
            self.bg_tex = tex2d(&self.device, cw, ch, 1, wgpu::TextureFormat::R16Uint, TU::TEXTURE_BINDING | TU::COPY_DST);
            self.fg_tex = tex2d(&self.device, cw, ch, 1, wgpu::TextureFormat::R16Uint, TU::TEXTURE_BINDING | TU::COPY_DST);
            self.canvas_size = (cw, ch);
        }
        self.write_tex(&self.fg_tex, bytemuck::cast_slice(&fg.pix), cw, ch, 0, 2);
        if let Some(bg) = bg {
            if bg.w == fg.w && bg.h == fg.h {
                self.write_tex(&self.bg_tex, bytemuck::cast_slice(&bg.pix), cw, ch, 0, 2);
            }
        }
    }

    /// Focal length in pixels for the current window: keeps the original's horizontal field of view
    /// for a 4:3 view of the window height... widened for wide windows (spec: focal = 94 × width / 320).
    pub fn focal_px(&self, focal320: f32) -> f32 {
        let (w, _) = self.size();
        focal320 * w as f32 / 320.0
    }

    /// Render one frame: optional 3D scene, background canvas (below 3D), foreground canvas, fade.
    pub fn render(&mut self, scene: Option<&Scene>, bg: Option<&canvas::Canvas>, fg: &canvas::Canvas, fade: f32) {
        self.upload_canvas(bg, fg);
        let (w, h) = self.size();
        let mut glob = Globals::default();
        glob.screen = [w as f32, h as f32, fg.w as f32, fg.h as f32];
        glob.misc = [fade.clamp(0.0, 1.0), bg.is_some() as u32 as f32, 0.0, 0.0];
        if let Some(s) = scene {
            let (r, u, f) = s.camera.basis();
            let focal = if s.camera.fixed_43 { s.camera.focal320 * (h as f32 * 4.0 / 3.0) / 320.0 } else { self.focal_px(s.camera.focal320) };
            glob.eye = [s.camera.x, s.camera.height, s.camera.row, 0.0];
            glob.right = [r[0], r[1], r[2], 0.0];
            glob.up = [u[0], u[1], u[2], 0.0];
            glob.fwd = [f[0], f[1], f[2], 0.0];
            let near = if s.near > 0.0 { s.near } else { 200.0 / 2048.0 };
            let far = if s.far > 0.0 { s.far } else { 40.0 };
            glob.proj = [focal / (w as f32 * 0.5), focal / (h as f32 * 0.5), near, far];
            glob.fog = [FOG_START, FOG_END, FOG_END + 1.5, 1.0];
            glob.sky = [s.camera.yaw / 65536.0, s.camera.pitch, s.camera.roll, s.sky as u32 as f32];
        }
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&glob));

        // dynamic vertices
        let mut dyn_verts: Vec<Vertex> = Vec::new();
        let (mut n_op, mut n_sp) = (0u32, 0u32);
        if let Some(s) = scene {
            dyn_verts.extend_from_slice(&s.opaque);
            dyn_verts.extend_from_slice(&s.sprites);
            n_op = s.opaque.len() as u32;
            n_sp = s.sprites.len() as u32;
        }
        let bytes = (dyn_verts.len() * std::mem::size_of::<Vertex>()) as u64;
        if bytes > self.dyn_cap {
            self.dyn_cap = bytes.next_power_of_two();
            self.dyn_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: self.dyn_cap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if bytes > 0 {
            self.queue.write_buffer(&self.dyn_buf, 0, bytemuck::cast_slice(&dyn_verts));
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let out_view = frame.texture.create_view(&Default::default());
        let scene_view = self.scene_tex.create_view(&Default::default());
        let depth_view = self.depth_tex.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scene_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(s) = scene {
                let b0 = self.bind0.as_ref().unwrap();
                pass.set_bind_group(0, b0, &[]);
                if s.world {
                    pass.set_pipeline(&self.sky_pipe);
                    pass.draw(0..3, 0..1);
                    if let Some((buf, n)) = &self.terrain_buf {
                        pass.set_pipeline(&self.world_pipe);
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.draw(0..*n, 0..1);
                    }
                }
                if n_op > 0 {
                    pass.set_pipeline(&self.world_pipe);
                    pass.set_vertex_buffer(0, self.dyn_buf.slice(..));
                    pass.draw(0..n_op, 0..1);
                }
                if n_sp > 0 {
                    pass.set_pipeline(&self.sprite_pipe);
                    pass.set_vertex_buffer(0, self.dyn_buf.slice(..));
                    pass.draw(n_op..n_op + n_sp, 0..1);
                }
            }
        }
        let v = |t: &wgpu::Texture| t.create_view(&Default::default());
        let bind1 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bind1_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&scene_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&v(&self.bg_tex)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&v(&self.fg_tex)) },
            ],
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.comp_pipe);
            pass.set_bind_group(0, self.bind0.as_ref().unwrap(), &[]);
            pass.set_bind_group(1, &bind1, &[]);
            pass.draw(0..3, 0..1);
        }
        let shot = self.shot.take().filter(|_| self.config.usage.contains(wgpu::TextureUsages::COPY_SRC));
        let mut readback = None;
        if shot.is_some() {
            let bpr = (w * 4 + 255) / 256 * 256;
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (bpr * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            enc.copy_texture_to_buffer(
                frame.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            readback = Some((buf, bpr));
        }
        self.queue.submit([enc.finish()]);
        if let (Some(path), Some((buf, bpr))) = (shot, readback) {
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            let data: Vec<u8> = buf.slice(..).get_mapped_range().map(|v| v.to_vec()).unwrap_or_default();
            let bgra = matches!(self.config.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
            let mut img = Vec::with_capacity((w * h * 3) as usize);
            for y in 0..h {
                for x in 0..w {
                    let o = (y * bpr + x * 4) as usize;
                    let (r, g, b) = if bgra { (data[o + 2], data[o + 1], data[o]) } else { (data[o], data[o + 1], data[o + 2]) };
                    img.extend_from_slice(&[r, g, b]);
                }
            }
            if let Ok(f) = std::fs::File::create(&path) {
                let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                e.set_color(png::ColorType::Rgb);
                e.set_depth(png::BitDepth::Eight);
                if let Ok(mut wr) = e.write_header() {
                    let _ = wr.write_image_data(&img);
                }
            }
        }
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

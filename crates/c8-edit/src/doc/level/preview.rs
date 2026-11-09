//! 3D preview of the level: the game's mesh (c8-render) drawn offscreen with the editor's wgpu
//! device and shown as an egui image, with a fly camera and optional walls, waypoints and
//! checkpoints as lines.
use super::LevelDoc;
use bytemuck::{Pod, Zeroable};
use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, Vec2};
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;
use c8_render::terrain::{build_mesh, MeshOpts};
use c8_render::Vertex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ViewMode {
    Map,
    Split,
    ThreeD,
}

/// Fly camera: position (x east, height, z south, in cells), yaw (0 = looking north, clockwise)
/// and pitch, in radians.
#[derive(Clone, Copy, Debug)]
pub(super) struct Cam {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

type Mat = [[f32; 4]; 4];

fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [[0.0; 4]; 4];
    for c in 0..4 {
        for row in 0..4 {
            r[c][row] = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-6);
    [a[0] / l, a[1] / l, a[2] / l]
}

impl Cam {
    pub fn forward(&self) -> [f32; 3] {
        [self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), -self.yaw.cos() * self.pitch.cos()]
    }
    /// View-projection matrix (column-major, wgpu depth 0..1).
    pub fn matrix(&self, aspect: f32) -> Mat {
        let f = norm(self.forward());
        let s = norm(cross(f, [0.0, 1.0, 0.0]));
        let u = cross(s, f);
        let e = self.pos;
        let view = [[s[0], u[0], -f[0], 0.0], [s[1], u[1], -f[1], 0.0], [s[2], u[2], -f[2], 0.0], [-dot(s, e), -dot(u, e), dot(f, e), 1.0]];
        let (n, far) = (0.05, 600.0);
        let t = 1.0 / (60f32.to_radians() / 2.0).tan();
        let proj = [[t / aspect, 0.0, 0.0, 0.0], [0.0, t, 0.0, 0.0], [0.0, 0.0, far / (n - far), -1.0], [0.0, 0.0, n * far / (n - far), 0.0]];
        mul(&proj, &view)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    vp: Mat,
    eye: [f32; 4],
    haze: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineVertex {
    pos: [f32; 3],
    colour: [f32; 4],
}

struct Gpu {
    pipeline: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
    vbuf: Option<(wgpu::Buffer, u32)>,
    lbuf: Option<(wgpu::Buffer, u32)>,
    target: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2])>,
    tex_id: Option<egui::TextureId>,
    clear: wgpu::Color,
}

pub(super) struct Preview {
    pub cam: Option<Cam>,
    gpu: Option<Gpu>,
    /// texture set the GPU textures were made from
    set: Option<usize>,
    pub mesh_dirty: bool,
    pub lines_dirty: bool,
    last_mesh: Instant,
    redraw: bool,
    pub show_lines: bool,
    pub haze: bool,
    speed: f32,
    pub hovered: bool,
    error: Option<String>,
}

impl Default for Preview {
    fn default() -> Self {
        Preview { cam: None, gpu: None, set: None, mesh_dirty: true, lines_dirty: true, last_mesh: Instant::now() - Duration::from_secs(1), redraw: true, show_lines: true, haze: false, speed: 12.0, hovered: false, error: None }
    }
}

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn srgb_to_linear(c: [u8; 3], a: f32) -> [f32; 4] {
    let f = |v: u8| (v as f32 / 255.0).powf(2.2);
    [f(c[0]), f(c[1]), f(c[2]), a]
}

impl Gpu {
    fn new(device: &wgpu::Device) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("preview"), source: wgpu::ShaderSource::Wgsl(include_str!("preview.wgsl").into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("preview"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let depth = Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() });
        let target = [Some(wgpu::ColorTargetState { format: COLOUR, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let vattrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Uint32, 4 => Uint32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("preview-mesh"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &vattrs })] },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: depth.clone(),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &target }),
            multiview_mask: None,
            cache: None,
        });
        let lattrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let lines = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("preview-lines"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_line"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<LineVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &lattrs })] },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: depth,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_line"), compilation_options: Default::default(), targets: &target }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: Some("preview-uniforms"), size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Gpu { pipeline, lines, layout, uniforms, bind: None, vbuf: None, lbuf: None, target: None, tex_id: None, clear: wgpu::Color::BLACK }
    }
}

impl LevelDoc {
    /// Upload the texture set: index textures and the light lookup (palette through the table).
    fn upload_look(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(g) = self.preview.gpu.as_mut() else { return };
        let ext = wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 256 };
        let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some("preview-textures"), size: ext, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::R8Uint, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        queue.write_texture(tex.as_image_copy(), &self.look.tex, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(64) }, ext);
        let mut lut = vec![0u8; 256 * 64 * 4];
        for row in 0..64 {
            for i in 0..256 {
                let c = self.look.rgb(i as u8, row);
                lut[(row * 256 + i) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let lext = wgpu::Extent3d { width: 256, height: 64, depth_or_array_layers: 1 };
        let lt = device.create_texture(&wgpu::TextureDescriptor { label: Some("preview-lut"), size: lext, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: COLOUR, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        queue.write_texture(lt.as_image_copy(), &lut, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256 * 4), rows_per_image: Some(64) }, lext);
        let tv = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let lv = lt.create_view(&Default::default());
        g.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview"),
            layout: &g.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: g.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lv) },
            ],
        }));
        // the sky: haze colour of the far distance (light row 0 of the brightest colour)
        let c = srgb_to_linear(self.look.rgb(255, 0), 1.0);
        g.clear = wgpu::Color { r: c[0] as f64, g: c[1] as f64, b: c[2] as f64, a: 1.0 };
        self.preview.set = Some(self.look.set);
    }

    fn line_vertices(&self) -> Vec<LineVertex> {
        let l = &self.level;
        let mut out = Vec::new();
        let ground = |x: f32, y: f32| l.floor_below(x, y, 255.0) + 0.15;
        let mut seg = |a: [f32; 3], b: [f32; 3], c: [f32; 4]| {
            out.push(LineVertex { pos: a, colour: c });
            out.push(LineVertex { pos: b, colour: c });
        };
        let by_index: std::collections::HashMap<usize, &c8_data::level::Entity> = l.entities.iter().map(|e| (e.index, e)).collect();
        for e in &l.entities {
            let (x, y) = (e.x as f32 + 0.5, e.y as f32 + 0.5);
            match e.kind {
                9 => {
                    let Some(n) = by_index.get(&(e.link as usize)) else { continue };
                    let (nx, ny) = (n.x as f32 + 0.5, n.y as f32 + 0.5);
                    if e.sub == 0 {
                        // a wall: a low fence between the points
                        let c = srgb_to_linear([255, 60, 60], 1.0);
                        let (h0, h1) = (ground(x, y), ground(nx, ny));
                        seg([x, h0, y], [nx, h1, ny], c);
                        seg([x, h0 + 0.6, y], [nx, h1 + 0.6, ny], c);
                        seg([x, h0, y], [x, h0 + 0.6, y], c);
                    } else {
                        let c = if e.sub == 6 { srgb_to_linear([255, 0, 255], 1.0) } else { srgb_to_linear([0, 230, 230], 1.0) };
                        seg([x, ground(x, y) + 0.3, y], [nx, ground(nx, ny) + 0.3, ny], c);
                    }
                }
                1 => {
                    let (x0, y0) = (x, y);
                    let (x1, y1) = (x0 + e.w as f32 + 1.0, y0 + e.h as f32 + 1.0);
                    let c = srgb_to_linear([255, 255, 0], 1.0);
                    for (ax, ay, bx, by) in [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)] {
                        seg([ax, ground(ax, ay) + 1.5, ay], [bx, ground(bx, by) + 1.5, by], c);
                    }
                }
                5 | 10 => {
                    let c = if e.kind == 5 { srgb_to_linear([60, 255, 60], 1.0) } else { srgb_to_linear([255, 150, 0], 1.0) };
                    let h = ground(x, y);
                    seg([x, h, y], [x, h + 1.2, y], c);
                }
                _ => {}
            }
        }
        for r in &l.regions {
            let c = match r.kind {
                1 => srgb_to_linear([80, 220, 80], 1.0),
                2 => srgb_to_linear([80, 140, 255], 1.0),
                3 => srgb_to_linear([255, 220, 60], 1.0),
                _ => srgb_to_linear([255, 255, 255], 1.0),
            };
            let (x0, y0, x1, y1) = (r.cx - r.hw, r.cy - r.hh, r.cx + r.hw, r.cy + r.hh);
            for (ax, ay, bx, by) in [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)] {
                seg([ax, ground(ax, ay), ay], [bx, ground(bx, by), by], c);
            }
        }
        out
    }

    /// Place the camera over the centre of the 2D map view, looking north and down.
    pub(super) fn camera_to_map_view(&mut self) {
        let c = if self.fitted {
            (self.pan + self.view_size / 2.0) / self.zoom
        } else {
            // the map has not been shown yet: start behind the start area, else the track's middle
            match self.level.regions.iter().find(|r| r.kind == 4) {
                Some(r) => Vec2::new(r.cx, r.cy - 4.0),
                None => Vec2::new(self.level.max_x as f32 / 2.0, self.level.max_y as f32 / 2.0),
            }
        };
        let (x, y) = (c.x.clamp(0.0, 255.0), c.y.clamp(0.0, 159.0));
        // above the highest surface around the spot, looking north and down
        let mut h: f32 = 0.0;
        for dy in -4..=16 {
            for dx in -6..=6 {
                h = h.max(self.level.floor_below(x + dx as f32, y + dy as f32, 255.0));
            }
        }
        self.preview.cam = Some(Cam { pos: [x, h + 10.0, y + 16.0], yaw: 0.0, pitch: -0.6 });
        self.preview.redraw = true;
    }

    /// Put the camera above a map point, keeping its direction.
    pub(super) fn camera_to(&mut self, x: f32, y: f32) {
        let mut cam = self.preview.cam.unwrap_or(Cam { pos: [0.0; 3], yaw: 0.0, pitch: -0.55 });
        let f = cam.forward();
        let back = 8.0;
        let h = self.level.floor_below(x, y, 255.0);
        cam.pos = [x - f[0] * back, h + 5.0, y - f[2] * back];
        self.preview.cam = Some(cam);
        self.preview.redraw = true;
    }

    fn camera_input(&mut self, ui: &egui::Ui, resp: &egui::Response) {
        let Some(mut cam) = self.preview.cam else { return };
        let before = (cam.pos, cam.yaw, cam.pitch);
        if resp.dragged() {
            let d = resp.drag_delta();
            cam.yaw += d.x * 0.006;
            cam.pitch = (cam.pitch - d.y * 0.006).clamp(-1.5, 1.5);
        }
        if resp.hovered() {
            let (dt, scroll, keys) = ui.input(|i| {
                let k = |a: Key, b: Key| (i.key_down(a) || i.key_down(b)) as i32 as f32;
                let fast = if i.modifiers.shift { 3.0 } else { 1.0 };
                (i.stable_dt.min(0.1), i.smooth_scroll_delta.y, [k(Key::W, Key::ArrowUp) - k(Key::S, Key::ArrowDown), k(Key::D, Key::ArrowRight) - k(Key::A, Key::ArrowLeft), k(Key::E, Key::Space) - k(Key::Q, Key::C), fast])
            });
            let f = cam.forward();
            let r = norm(cross(f, [0.0, 1.0, 0.0]));
            let step = self.preview.speed * keys[3] * dt;
            let fwd = keys[0] * step + scroll * 0.02;
            for k in 0..3 {
                cam.pos[k] += f[k] * fwd + r[k] * keys[1] * step;
            }
            cam.pos[1] += keys[2] * step;
            if keys[0] != 0.0 || keys[1] != 0.0 || keys[2] != 0.0 {
                ui.ctx().request_repaint();
            }
        }
        cam.pos[0] = cam.pos[0].clamp(-20.0, 276.0);
        cam.pos[2] = cam.pos[2].clamp(-20.0, 180.0);
        cam.pos[1] = cam.pos[1].clamp(-5.0, 120.0);
        if (cam.pos, cam.yaw, cam.pitch) != before {
            self.preview.redraw = true;
        }
        self.preview.cam = Some(cam);
    }

    pub(super) fn preview_ui(&mut self, ui: &mut egui::Ui) {
        let Some(rs) = crate::gpu::state() else {
            ui.centered_and_justified(|ui| ui.label("The 3D preview needs the wgpu renderer."));
            return;
        };
        if let Some(e) = &self.preview.error {
            ui.label(egui::RichText::new(e).color(Color32::from_rgb(255, 110, 90)));
            return;
        }
        if self.preview.cam.is_none() {
            self.camera_to_map_view();
        }
        let (device, queue) = (&rs.device, &rs.queue);
        if self.preview.gpu.is_none() {
            self.preview.gpu = Some(Gpu::new(device));
        }
        if self.preview.set != Some(self.look.set) {
            self.upload_look(device, queue);
            self.preview.redraw = true;
        }
        // rebuild the mesh after edits (at most 8 times a second while painting)
        if self.preview.mesh_dirty && self.preview.last_mesh.elapsed() > Duration::from_millis(120) {
            let verts = build_mesh(&self.level, &MeshOpts { shading: true, flat: None });
            let g = self.preview.gpu.as_mut().unwrap();
            g.vbuf = Some((device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("preview-mesh"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX }), verts.len() as u32));
            self.preview.mesh_dirty = false;
            self.preview.lines_dirty = true;
            self.preview.last_mesh = Instant::now();
            self.preview.redraw = true;
        } else if self.preview.mesh_dirty {
            ui.ctx().request_repaint_after(Duration::from_millis(130));
        }
        if self.preview.lines_dirty {
            let lv = self.line_vertices();
            let g = self.preview.gpu.as_mut().unwrap();
            g.lbuf = (!lv.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("preview-lines"), contents: bytemuck::cast_slice(&lv), usage: wgpu::BufferUsages::VERTEX }), lv.len() as u32));
            self.preview.lines_dirty = false;
            self.preview.redraw = true;
        }

        // toolbar
        ui.horizontal(|ui| {
            if ui.button("Camera to map view").clicked() {
                self.camera_to_map_view();
            }
            if ui.checkbox(&mut self.preview.show_lines, "walls, route, checkpoints").changed() || ui.checkbox(&mut self.preview.haze, "haze").changed() {
                self.preview.redraw = true;
            }
            ui.add(egui::Slider::new(&mut self.preview.speed, 2.0..=60.0).text("speed"));
        });
        let size = ui.available_size().max(Vec2::splat(32.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.preview.hovered = resp.hovered() || resp.dragged();
        self.camera_input(ui, &resp);
        let ppp = ui.ctx().pixels_per_point();
        let px = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];

        let g = self.preview.gpu.as_mut().unwrap();
        if g.target.as_ref().is_none_or(|t| t.2 != px) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("preview-target"),
                size: wgpu::Extent3d { width: px[0], height: px[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOUR,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            let mut r = rs.renderer.write();
            match g.tex_id {
                Some(id) => r.update_egui_texture_from_wgpu_texture(device, &view, wgpu::FilterMode::Linear, id),
                None => g.tex_id = Some(r.register_native_texture(device, &view, wgpu::FilterMode::Linear)),
            }
            g.target = Some((tex, view, px));
            self.preview.redraw = true;
        }
        if self.preview.redraw {
            let cam = self.preview.cam.unwrap();
            let u = Uniforms { vp: cam.matrix(px[0] as f32 / px[1] as f32), eye: [cam.pos[0], cam.pos[1], cam.pos[2], 1.0], haze: [24.0, 48.0, if self.preview.haze { 1.0 } else { 0.0 }, 0.0] };
            queue.write_buffer(&g.uniforms, 0, bytemuck::bytes_of(&u));
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("preview-depth"),
                size: wgpu::Extent3d { width: px[0], height: px[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let dv = depth.create_view(&Default::default());
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("preview") });
            {
                let (_, view, _) = g.target.as_ref().unwrap();
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("preview"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(g.clear), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                if let (Some(bind), Some((vb, n))) = (&g.bind, &g.vbuf) {
                    pass.set_pipeline(&g.pipeline);
                    pass.set_bind_group(0, bind, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(0..*n, 0..1);
                    if let (true, Some((lb, ln))) = (self.preview.show_lines, &g.lbuf) {
                        pass.set_pipeline(&g.lines);
                        pass.set_vertex_buffer(0, lb.slice(..));
                        pass.draw(0..*ln, 0..1);
                    }
                }
            }
            queue.submit([enc.finish()]);
            self.preview.redraw = false;
        }
        if let Some(id) = g.tex_id {
            ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let cam = self.preview.cam.unwrap();
        ui.painter().text(rect.left_bottom() + Vec2::new(6.0, -4.0), egui::Align2::LEFT_BOTTOM, format!("{:.1}, {:.1}, height {:.1} · drag to look, WASD / arrows to fly, Q/E down/up, Shift faster, wheel forward", cam.pos[0], cam.pos[2], cam.pos[1]), egui::FontId::proportional(11.0), Color32::from_white_alpha(200));
    }

    /// The 3D camera on the 2D map: a triangle pointing where it looks.
    pub(super) fn draw_camera(&self, p: &egui::Painter, origin: Pos2) {
        let Some(cam) = self.preview.cam else { return };
        let c = self.to_screen(origin, cam.pos[0], cam.pos[2]);
        let (s, co) = (cam.yaw.sin(), -cam.yaw.cos());
        let d = Vec2::new(s, co);
        let n = Vec2::new(-co, s);
        let pts = vec![c + d * 14.0, c - d * 6.0 + n * 7.0, c - d * 6.0 - n * 7.0];
        p.add(egui::Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(255, 255, 255, 200), egui::Stroke::new(1.5, Color32::BLACK)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_projects_ahead_into_view() {
        let cam = Cam { pos: [10.0, 5.0, 50.0], yaw: 0.0, pitch: 0.0 };
        let m = cam.matrix(1.5);
        let project = |p: [f32; 3]| {
            let v = [p[0], p[1], p[2], 1.0];
            let c: Vec<f32> = (0..4).map(|r| (0..4).map(|k| m[k][r] * v[k]).sum()).collect();
            (c[0] / c[3], c[1] / c[3], c[2] / c[3], c[3])
        };
        // straight ahead (north = smaller z) lands in the middle, in front of the camera
        let (x, y, z, w) = project([10.0, 5.0, 40.0]);
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4 && (0.0..1.0).contains(&z) && w > 0.0);
        // east appears on the right, higher appears above
        assert!(project([12.0, 5.0, 40.0]).0 > 0.0);
        assert!(project([10.0, 7.0, 40.0]).1 > 0.0);
        // behind the camera
        assert!(project([10.0, 5.0, 60.0]).3 < 0.0);
    }
}

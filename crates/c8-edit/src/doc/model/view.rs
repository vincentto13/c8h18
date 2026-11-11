//! The model viewer's 3D view: the game's model triangles (c8-render) drawn offscreen with the
//! editor's wgpu device and shown as an egui image, an orbit camera, wireframe and axes as lines,
//! and triangle picking on the CPU.
use crate::model_io::Look;
use bytemuck::{Pod, Zeroable};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;
use c8_data::model::Model;
use c8_render::Vertex;

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
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A file vertex in the view's world: the game's model space (x right, height, row backwards)
/// in file units, i.e. file (x, z, y) (spec 08 §8).
pub fn world(v: [i16; 3]) -> [f32; 3] {
    [v[0] as f32, v[2] as f32, v[1] as f32]
}

/// Orbit camera around `target` (world units), yaw 0 = looking along −row (the model's
/// forward), pitch positive = from above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}

impl Orbit {
    /// Front three-quarter view of a model.
    pub fn frame(m: &Model) -> Orbit {
        let (lo, hi) = m.bounds();
        let (lo, hi) = (world(lo), world(hi));
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let r = (0..3).map(|k| (hi[k] - lo[k]) / 2.0).fold(0.0f32, |a, b| a.hypot(b)).max(50.0);
        Orbit { target: c, yaw: std::f32::consts::PI + 0.7, pitch: 0.4, dist: r * 2.6 }
    }
    pub fn eye(&self) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // yaw 0: the eye sits behind (+row) and looks along −row
        [self.target[0] - sy * cp * self.dist, self.target[1] + sp * self.dist, self.target[2] + cy * cp * self.dist]
    }
    /// (right, up) unit vectors of the view.
    pub fn basis(&self) -> ([f32; 3], [f32; 3]) {
        let e = self.eye();
        let f = norm([self.target[0] - e[0], self.target[1] - e[1], self.target[2] - e[2]]);
        let s = norm(cross(f, [0.0, 1.0, 0.0]));
        (s, cross(s, f))
    }
    /// View-projection matrix (column-major, wgpu depth 0..1).
    pub fn matrix(&self, aspect: f32) -> Mat {
        let e = self.eye();
        let f = norm([self.target[0] - e[0], self.target[1] - e[1], self.target[2] - e[2]]);
        let (s, u) = self.basis();
        let view = [[s[0], u[0], -f[0], 0.0], [s[1], u[1], -f[1], 0.0], [s[2], u[2], -f[2], 0.0], [-dot(s, e), -dot(u, e), dot(f, e), 1.0]];
        let (n, far) = (self.dist * 0.01, self.dist * 20.0);
        let t = 1.0 / (40f32.to_radians() / 2.0).tan();
        let proj = [[t / aspect, 0.0, 0.0, 0.0], [0.0, t, 0.0, 0.0], [0.0, 0.0, far / (n - far), -1.0], [0.0, 0.0, n * far / (n - far), 0.0]];
        mul(&proj, &view)
    }
}

/// Project a world point: (screen position in `rect`, depth 0..1), None behind the camera.
pub fn project(m: &Mat, rect: Rect, p: [f32; 3]) -> Option<(Pos2, f32)> {
    let c: [f32; 4] = std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
    if c[3] <= 1e-6 {
        return None;
    }
    let (x, y, z) = (c[0] / c[3], c[1] / c[3], c[2] / c[3]);
    Some((Pos2::new(rect.left() + (x + 1.0) * 0.5 * rect.width(), rect.top() + (1.0 - y) * 0.5 * rect.height()), z))
}

/// The front-most triangle under `p`: (triangle index, depth).
pub fn pick(model: &Model, vp: &Mat, rect: Rect, p: Pos2) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, t) in model.tris.iter().enumerate() {
        let Some(s) = t.v.iter().map(|&v| project(vp, rect, world(model.verts[v as usize]))).collect::<Option<Vec<_>>>() else { continue };
        let (a, b, c) = (s[0].0, s[1].0, s[2].0);
        let area = (b - a).x * (c - a).y - (b - a).y * (c - a).x;
        if area.abs() < 1e-6 {
            continue;
        }
        let w1 = ((p - a).x * (c - a).y - (p - a).y * (c - a).x) / area;
        let w2 = ((b - a).x * (p - a).y - (b - a).y * (p - a).x) / area;
        let w0 = 1.0 - w1 - w2;
        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
            continue;
        }
        let z = w0 * s[0].1 + w1 * s[1].1 + w2 * s[2].1;
        if best.is_none_or(|(_, bz)| z < bz) {
            best = Some((i, z));
        }
    }
    best.map(|b| b.0)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    vp: Mat,
    light: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub colour: [f32; 4],
}

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub fn linear(c: [u8; 3], a: f32) -> [f32; 4] {
    let f = |v: u8| (v as f32 / 255.0).powf(2.2);
    [f(c[0]), f(c[1]), f(c[2]), a]
}

/// Display options.
#[derive(Clone, Copy, PartialEq)]
pub struct Show {
    pub faces: bool,
    pub wire: bool,
    pub shading: bool,
    pub axes: bool,
}

impl Default for Show {
    fn default() -> Self {
        Show { faces: true, wire: false, shading: true, axes: true }
    }
}

pub struct Gpu {
    mesh: wgpu::RenderPipeline,
    wire: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    bind: Option<wgpu::BindGroup>,
    vbuf: Option<(wgpu::Buffer, u32)>,
    lbuf: Option<(wgpu::Buffer, u32)>,
    hbuf: Option<(wgpu::Buffer, u32)>,
    target: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2])>,
    tex_id: Option<egui::TextureId>,
}

impl Gpu {
    pub fn new(device: &wgpu::Device) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("model-view"), source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()) });
        let tex_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model-view"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                tex_entry(1, wgpu::TextureSampleType::Uint),
                tex_entry(2, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("model-view"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let depth = |write: bool, cmp| Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(write), depth_compare: Some(cmp), stencil: Default::default(), bias: Default::default() });
        let target = [Some(wgpu::ColorTargetState { format: COLOUR, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })];
        let vattrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Uint32, 4 => Uint32];
        let mesh = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model-view-mesh"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &vattrs })] },
            // like the game's GPU path: no culling, back faces hidden by the depth buffer
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &target }),
            multiview_mask: None,
            cache: None,
        });
        let lattrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let lines = |label, d| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_line"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<LineVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &lattrs })] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
                depth_stencil: d,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_line"), compilation_options: Default::default(), targets: &target }),
                multiview_mask: None,
                cache: None,
            })
        };
        let wire = lines("model-view-wire", depth(false, wgpu::CompareFunction::LessEqual));
        let overlay = lines("model-view-overlay", depth(false, wgpu::CompareFunction::Always));
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: Some("model-view-uniforms"), size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Gpu { mesh, wire, overlay, layout, uniforms, bind: None, vbuf: None, lbuf: None, hbuf: None, target: None, tex_id: None }
    }

    /// Upload the atlas and the light lookup (palette through the colour table).
    pub fn upload_look(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, look: &Look) {
        let a = &look.atlas;
        let ext = wgpu::Extent3d { width: 256, height: a.h.max(1), depth_or_array_layers: 1 };
        let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some("model-atlas"), size: ext, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::R8Uint, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let mut pix = a.pix.clone();
        pix.resize((256 * ext.height) as usize, 0);
        queue.write_texture(tex.as_image_copy(), &pix, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(ext.height) }, ext);
        let mut lut = vec![0u8; 256 * 64 * 4];
        for row in 0..64 {
            for i in 0..256 {
                let c = look.rgb(i as u8, row);
                lut[(row * 256 + i) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let lext = wgpu::Extent3d { width: 256, height: 64, depth_or_array_layers: 1 };
        let lt = device.create_texture(&wgpu::TextureDescriptor { label: Some("model-lut"), size: lext, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: COLOUR, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        queue.write_texture(lt.as_image_copy(), &lut, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256 * 4), rows_per_image: Some(64) }, lext);
        let (tv, lv) = (tex.create_view(&Default::default()), lt.create_view(&Default::default()));
        self.bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model-view"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lv) },
            ],
        }));
    }

    /// Rebuild the triangles (exactly as the game builds them) and the wireframe and axes.
    pub fn upload_model(&mut self, device: &wgpu::Device, m: &Model, look: &Look) {
        let mut verts = Vec::new();
        let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // scale 2048: world units = file units
        c8_render::model::push_model(&mut verts, m, &look.atlas, [0.0; 3], &ident, 2048.0, 0);
        self.vbuf = (!verts.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("model-mesh"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX }), verts.len() as u32));
        let mut lines = Vec::new();
        let wire = linear([20, 20, 20], 1.0);
        for t in &m.tris {
            for k in 0..3 {
                lines.push(LineVertex { pos: world(m.verts[t.v[k] as usize]), colour: wire });
                lines.push(LineVertex { pos: world(m.verts[t.v[(k + 1) % 3] as usize]), colour: wire });
            }
        }
        self.lbuf = (!lines.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("model-wire"), contents: bytemuck::cast_slice(&lines), usage: wgpu::BufferUsages::VERTEX }), lines.len() as u32));
    }

    /// Lines drawn on top of everything: axes and the hovered / selected triangle.
    pub fn upload_overlay(&mut self, device: &wgpu::Device, lines: &[LineVertex]) {
        self.hbuf = (!lines.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("model-overlay"), contents: bytemuck::cast_slice(lines), usage: wgpu::BufferUsages::VERTEX }), lines.len() as u32));
    }

    /// Draw into the target (made or resized to `px`); returns the egui texture to show.
    pub fn draw(&mut self, rs: &eframe::egui_wgpu::RenderState, px: [u32; 2], cam: &Orbit, show: Show, background: [f32; 4]) -> Option<egui::TextureId> {
        let (device, queue) = (&rs.device, &rs.queue);
        if self.target.as_ref().is_none_or(|t| t.2 != px) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("model-view-target"),
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
            match self.tex_id {
                Some(id) => r.update_egui_texture_from_wgpu_texture(device, &view, wgpu::FilterMode::Linear, id),
                None => self.tex_id = Some(r.register_native_texture(device, &view, wgpu::FilterMode::Linear)),
            }
            self.target = Some((tex, view, px));
        }
        let view = self.target.as_ref().unwrap().1.clone();
        self.render(device, queue, &view, px, cam, show, background);
        self.tex_id
    }

    /// Render the model from `cam` at `w`×`h` and read it back as RGBA.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn snapshot(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, cam: &Orbit, show: Show, background: [f32; 4]) -> Result<Vec<u8>, String> {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model-snapshot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOUR,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.render(device, queue, &tex.create_view(&Default::default()), [w, h], cam, show, background);
        let bpr = (w * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("model-snapshot-read"), size: (bpr * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("model-snapshot") });
        enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        let data = buf.slice(..).get_mapped_range().map(|v| v.to_vec()).unwrap_or_default();
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            rgba.extend_from_slice(&data[y * bpr as usize..y * bpr as usize + w as usize * 4]);
        }
        Ok(rgba)
    }

    #[allow(clippy::too_many_arguments)]
    fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView, px: [u32; 2], cam: &Orbit, show: Show, background: [f32; 4]) {
        let (s, u) = cam.basis();
        // light from the upper left of the view
        let l = norm([u[0] * 0.8 - s[0] * 0.5 + (cam.eye()[0] - cam.target[0]) / cam.dist * 0.4, u[1] * 0.8 - s[1] * 0.5 + (cam.eye()[1] - cam.target[1]) / cam.dist * 0.4, u[2] * 0.8 - s[2] * 0.5 + (cam.eye()[2] - cam.target[2]) / cam.dist * 0.4]);
        let un = Uniforms { vp: cam.matrix(px[0] as f32 / px[1] as f32), light: [l[0], l[1], l[2], if show.shading { 1.0 } else { 0.0 }] };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&un));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model-view-depth"),
            size: wgpu::Extent3d { width: px[0], height: px[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let dv = depth.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("model-view") });
        {
            let clear = wgpu::Color { r: background[0] as f64, g: background[1] as f64, b: background[2] as f64, a: 1.0 };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model-view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(bind) = &self.bind {
                pass.set_bind_group(0, bind, &[]);
                if let (true, Some((vb, n))) = (show.faces, &self.vbuf) {
                    pass.set_pipeline(&self.mesh);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(0..*n, 0..1);
                }
                if let (true, Some((lb, n))) = (show.wire || !show.faces, &self.lbuf) {
                    pass.set_pipeline(&self.wire);
                    pass.set_vertex_buffer(0, lb.slice(..));
                    pass.draw(0..*n, 0..1);
                }
                if let Some((hb, n)) = &self.hbuf {
                    pass.set_pipeline(&self.overlay);
                    pass.set_vertex_buffer(0, hb.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            }
        }
        queue.submit([enc.finish()]);
    }
}

/// Camera input on the view: drag to orbit, right/middle drag or Shift-drag to pan, wheel or
/// pinch to zoom. Returns whether the camera moved.
pub fn camera_input(ui: &egui::Ui, resp: &egui::Response, cam: &mut Orbit) -> bool {
    let before = *cam;
    let (shift, scroll, zoom) = ui.input(|i| (i.modifiers.shift, i.smooth_scroll_delta.y, i.zoom_delta()));
    let pan = resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) || (shift && resp.dragged_by(egui::PointerButton::Primary));
    let d = resp.drag_delta();
    if pan {
        let (s, u) = cam.basis();
        let k = cam.dist * 0.0018;
        for i in 0..3 {
            cam.target[i] += (-s[i] * d.x + u[i] * d.y) * k;
        }
    } else if resp.dragged_by(egui::PointerButton::Primary) {
        cam.yaw += d.x * 0.008;
        cam.pitch = (cam.pitch + d.y * 0.008).clamp(-1.55, 1.55);
    }
    if resp.hovered() {
        if scroll != 0.0 {
            cam.dist *= (-scroll * 0.003).exp();
        }
        if zoom != 1.0 {
            cam.dist /= zoom;
        }
        cam.dist = cam.dist.clamp(10.0, 200_000.0);
    }
    *cam != before
}

/// Allocate the view's area: (rect, response, pixel size).
pub fn allocate(ui: &mut egui::Ui) -> (Rect, egui::Response, [u32; 2]) {
    let size = ui.available_size().max(Vec2::splat(32.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    (rect, resp, [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32])
}

pub fn paint(ui: &egui::Ui, rect: Rect, id: egui::TextureId) {
    ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_looks_at_the_target_and_picks_the_front_face() {
        // two parallel triangles facing the camera, one nearer (file y backwards = +row)
        let mut m = Model { verts: vec![[-100, 0, -100], [100, 0, -100], [0, 0, 100], [-100, 300, -100], [100, 300, -100], [0, 300, 100]], shade: vec![128; 6], ..Default::default() };
        m.tris.push(c8_data::model::Tri { kind: 0, v: [0, 1, 2], col: 1, ..Default::default() });
        m.tris.push(c8_data::model::Tri { kind: 0, v: [3, 4, 5], col: 2, ..Default::default() });
        // looking along −row from behind: the triangle at row 300 is nearer
        let cam = Orbit { target: [0.0, 0.0, 150.0], yaw: 0.0, pitch: 0.0, dist: 1000.0 };
        assert!(cam.eye()[2] > 1000.0);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
        let vp = cam.matrix(400.0 / 300.0);
        let (c, z) = project(&vp, rect, cam.target).unwrap();
        assert!((c - rect.center()).length() < 0.01 && (0.0..1.0).contains(&z));
        assert_eq!(pick(&m, &vp, rect, rect.center()), Some(1));
        assert_eq!(pick(&m, &vp, rect, Pos2::new(2.0, 2.0)), None);
        // the frame of a model puts it in front of the camera
        let o = Orbit::frame(&m);
        assert!(project(&o.matrix(1.0), rect, o.target).is_some());
    }
}

//! Capture decoration (#135 Phase 5): a geometric **axes system** + a **wireframe room**.
//!
//! The XYZ axes are shaded **tubes with conical arrowheads** (a triangle surface, lit) so
//! they read as solid 3-D geometry; thickness is a slider. The bounding box is drawn as
//! gridded **back walls only** (the 3 faces facing away from the camera) — a hidden-line
//! "room/stage" look that avoids the busy interior lattice. Both render in the scene pass,
//! sharing the camera + depth buffer. The X/Y/Z labels (projected to screen) are drawn by
//! the overlay text pass (`bin/visual.rs`).
//!
//! The pure mesh builders (`build_axis_solids`, `build_box_lines`) are unit-tested.
//! Compiled only by the visual binary (via `#[path]`), like `render.rs`.

use glam::Vec3;

// Mirror render.rs's scene targets (kept local so axes.rs is self-contained).
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// Conventional axis colours (X red, Y green, Z blue).
pub const AXIS_X: [f32; 3] = [1.0, 0.30, 0.30];
pub const AXIS_Y: [f32; 3] = [0.35, 1.0, 0.40];
pub const AXIS_Z: [f32; 3] = [0.40, 0.60, 1.0];

const RADIAL_SEG: usize = 18;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug, PartialEq)]
pub struct SurfVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug, PartialEq)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

/// Everything the mesh builders need (filled from `Shared.axes` by the visual).
#[derive(Clone, Copy, Debug)]
pub struct AxesConfig {
    pub axes_on: bool,
    pub len: f32,
    pub thick: f32,
    pub ticks: bool,
    pub axis_alpha: f32,
    pub box_on: bool,
    pub extent: f32,
    pub subdiv: u32,
    pub box_color: [f32; 4], // rgb + alpha
    pub eye: [f32; 3],       // camera position (chooses the box back walls)
}

fn tri(out: &mut Vec<SurfVertex>, a: Vec3, b: Vec3, c: Vec3, na: Vec3, nb: Vec3, nc: Vec3, col: [f32; 4]) {
    out.push(SurfVertex { pos: a.into(), normal: na.into(), color: col });
    out.push(SurfVertex { pos: b.into(), normal: nb.into(), color: col });
    out.push(SurfVertex { pos: c.into(), normal: nc.into(), color: col });
}

/// A tube (open cylinder) of `length` along `dir` from `base`, radius `r`, radial normals.
fn push_cylinder(out: &mut Vec<SurfVertex>, base: Vec3, dir: Vec3, p1: Vec3, p2: Vec3, length: f32, r: f32, col: [f32; 4]) {
    let tip = base + dir * length;
    for i in 0..RADIAL_SEG {
        let a0 = (i as f32 / RADIAL_SEG as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / RADIAL_SEG as f32) * std::f32::consts::TAU;
        let n0 = p1 * a0.cos() + p2 * a0.sin();
        let n1 = p1 * a1.cos() + p2 * a1.sin();
        let b00 = base + n0 * r;
        let b10 = base + n1 * r;
        let t00 = tip + n0 * r;
        let t10 = tip + n1 * r;
        tri(out, b00, b10, t10, n0, n1, n1, col);
        tri(out, b00, t10, t00, n0, n1, n0, col);
    }
}

/// A solid cone (arrowhead): apex at `base + dir*height`, base radius `r`, closed base.
fn push_cone(out: &mut Vec<SurfVertex>, base: Vec3, dir: Vec3, p1: Vec3, p2: Vec3, height: f32, r: f32, col: [f32; 4]) {
    let apex = base + dir * height;
    for i in 0..RADIAL_SEG {
        let a0 = (i as f32 / RADIAL_SEG as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / RADIAL_SEG as f32) * std::f32::consts::TAU;
        let r0 = p1 * a0.cos() + p2 * a0.sin();
        let r1 = p1 * a1.cos() + p2 * a1.sin();
        let rim0 = base + r0 * r;
        let rim1 = base + r1 * r;
        // side: outward normal perpendicular to the slant (= radial*height + dir*r)
        let sn0 = (r0 * height + dir * r).normalize_or_zero();
        let sn1 = (r1 * height + dir * r).normalize_or_zero();
        let snap = ((sn0 + sn1) * 0.5).normalize_or_zero();
        tri(out, rim0, rim1, apex, sn0, sn1, snap, col);
        // base disk (faces back along -dir)
        tri(out, base, rim1, rim0, -dir, -dir, -dir, col);
    }
}

/// Build the axes' surface geometry (3 tubes + arrowheads, optional tick rings). Pure.
pub fn build_axis_solids(cfg: &AxesConfig) -> Vec<SurfVertex> {
    let mut v = Vec::new();
    if !cfg.axes_on {
        return v;
    }
    let len = cfg.len.max(0.001);
    let r = cfg.thick.max(0.001);
    let a = cfg.axis_alpha.clamp(0.0, 1.0);
    let cone_len = (len * 0.16).clamp(r * 3.0, len * 0.5);
    let base_r = r * 2.6;
    let shaft = (len - cone_len).max(0.0);
    let axes = [
        (Vec3::X, Vec3::Y, Vec3::Z, AXIS_X),
        (Vec3::Y, Vec3::Z, Vec3::X, AXIS_Y),
        (Vec3::Z, Vec3::X, Vec3::Y, AXIS_Z),
    ];
    for (dir, p1, p2, col) in axes {
        let c = [col[0], col[1], col[2], a];
        push_cylinder(&mut v, Vec3::ZERO, dir, p1, p2, shaft, r, c);
        push_cone(&mut v, dir * shaft, dir, p1, p2, cone_len, base_r, c);
        if cfg.ticks && shaft >= 1.0 {
            let ring_len = (r * 0.6).max(0.01);
            let ring_r = r * 1.7;
            let mut d = 1.0f32;
            while d <= shaft - cone_len * 0.1 {
                push_cylinder(&mut v, dir * (d - ring_len * 0.5), dir, p1, p2, ring_len, ring_r, c);
                d += 1.0;
            }
        }
    }
    v
}

/// Build the box as gridded **back walls** (the 3 faces facing away from `eye`). Pure.
pub fn build_box_lines(cfg: &AxesConfig) -> Vec<LineVertex> {
    let mut v = Vec::new();
    if !cfg.box_on {
        return v;
    }
    let e = cfg.extent.max(0.0);
    let n = cfg.subdiv.clamp(1, 32);
    let col = cfg.box_color;
    let coord = |t: u32| -e + 2.0 * e * (t as f32 / n as f32);
    let mut seg = |a: [f32; 3], b: [f32; 3]| {
        v.push(LineVertex { pos: a, color: col });
        v.push(LineVertex { pos: b, color: col });
    };
    for i in 0..3usize {
        // The far wall on axis i is the one on the opposite side of the camera.
        let wall = if cfg.eye[i] >= 0.0 { -e } else { e };
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        // grid lines parallel to j (one per k node) and parallel to k (one per j node)
        for kk in 0..=n {
            let mut a = [0.0f32; 3];
            let mut b = [0.0f32; 3];
            a[i] = wall;
            b[i] = wall;
            a[k] = coord(kk);
            b[k] = coord(kk);
            a[j] = -e;
            b[j] = e;
            seg(a, b);
        }
        for jj in 0..=n {
            let mut a = [0.0f32; 3];
            let mut b = [0.0f32; 3];
            a[i] = wall;
            b[i] = wall;
            a[j] = coord(jj);
            b[j] = coord(jj);
            a[k] = -e;
            b[k] = e;
            seg(a, b);
        }
    }
    v
}

/// GPU renderer: a shared camera UBO + a lit surface pipeline (tubes/cones) and a line
/// pipeline (box walls), both drawn inside the scene pass.
pub struct Axes {
    bgl: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    surf_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    sample_count: u32,
    surf_vbuf: Option<wgpu::Buffer>,
    surf_cap: u64,
    surf_count: u32,
    line_vbuf: Option<wgpu::Buffer>,
    line_cap: u64,
    line_count: u32,
}

impl Axes {
    pub fn new(device: &wgpu::Device, sample_count: u32) -> Axes {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("axes-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("axes.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axes-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axes-ubuf"),
            size: 64, // mat4x4<f32>
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axes-bind"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let surf_pipeline = make_surf_pipeline(device, &shader, &bgl, sample_count);
        let line_pipeline = make_line_pipeline(device, &shader, &bgl, sample_count);
        Axes {
            bgl,
            shader,
            ubuf,
            bind,
            surf_pipeline,
            line_pipeline,
            sample_count,
            surf_vbuf: None,
            surf_cap: 0,
            surf_count: 0,
            line_vbuf: None,
            line_cap: 0,
            line_count: 0,
        }
    }

    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        if n != self.sample_count {
            self.sample_count = n;
            self.surf_pipeline = make_surf_pipeline(device, &self.shader, &self.bgl, n);
            self.line_pipeline = make_line_pipeline(device, &self.shader, &self.bgl, n);
        }
    }

    /// Upload camera + geometry (call before the scene pass).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: &[[f32; 4]; 4],
        solids: &[SurfVertex],
        lines: &[LineVertex],
    ) {
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(view_proj));
        self.surf_count = solids.len() as u32;
        if !solids.is_empty() {
            let bytes = bytemuck::cast_slice(solids);
            grow_write(device, queue, &mut self.surf_vbuf, &mut self.surf_cap, "axes-surf-vbuf", bytes);
        }
        self.line_count = lines.len() as u32;
        if !lines.is_empty() {
            let bytes = bytemuck::cast_slice(lines);
            grow_write(device, queue, &mut self.line_vbuf, &mut self.line_cap, "axes-line-vbuf", bytes);
        }
    }

    /// Draw the prepared axes (surface) + box (lines) into the scene pass.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if self.surf_count > 0 {
            if let Some(vb) = self.surf_vbuf.as_ref() {
                rp.set_pipeline(&self.surf_pipeline);
                rp.set_bind_group(0, &self.bind, &[]);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.draw(0..self.surf_count, 0..1);
            }
        }
        if self.line_count > 0 {
            if let Some(vb) = self.line_vbuf.as_ref() {
                rp.set_pipeline(&self.line_pipeline);
                rp.set_bind_group(0, &self.bind, &[]);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.draw(0..self.line_count, 0..1);
            }
        }
    }
}

fn grow_write(device: &wgpu::Device, queue: &wgpu::Queue, buf: &mut Option<wgpu::Buffer>, cap: &mut u64, label: &str, bytes: &[u8]) {
    let need = bytes.len() as u64;
    if buf.is_none() || *cap < need {
        *cap = need.next_power_of_two().max(2048);
        *buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: *cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    queue.write_buffer(buf.as_ref().unwrap(), 0, bytes);
}

fn pipeline_layout(device: &wgpu::Device, bgl: &wgpu::BindGroupLayout) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("axes-pl"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    })
}

fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

fn color_target() -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

fn make_surf_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, bgl: &wgpu::BindGroupLayout, sample_count: u32) -> wgpu::RenderPipeline {
    let layout = pipeline_layout(device, bgl);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axes-surf-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_surf"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SurfVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_surf"),
            targets: &[color_target()],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(depth_state()),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn make_line_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, bgl: &wgpu::BindGroupLayout, sample_count: u32) -> wgpu::RenderPipeline {
    let layout = pipeline_layout(device, bgl);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("axes-line-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_line"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<LineVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_line"),
            targets: &[color_target()],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(depth_state()),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AxesConfig {
        AxesConfig {
            axes_on: true,
            len: 4.0,
            thick: 0.12,
            ticks: false,
            axis_alpha: 1.0,
            box_on: false,
            extent: 4.0,
            subdiv: 1,
            box_color: [0.5, 0.5, 0.7, 0.5],
            eye: [6.0, 4.0, 8.0],
        }
    }

    #[test]
    fn axes_build_three_tubes_and_cones() {
        let v = build_axis_solids(&cfg());
        // 3 axes × (cylinder RADIAL_SEG*2 tris + cone RADIAL_SEG*2 tris) × 3 verts
        let per_axis = (RADIAL_SEG * 2 + RADIAL_SEG * 2) * 3;
        assert_eq!(v.len(), 3 * per_axis);
        // every normal is unit-ish (no degenerate)
        for sv in &v {
            let n = Vec3::from(sv.normal).length();
            assert!(n > 0.5, "degenerate normal {:?}", sv.normal);
        }
    }

    #[test]
    fn axes_off_is_empty() {
        let mut c = cfg();
        c.axes_on = false;
        assert!(build_axis_solids(&c).is_empty());
    }

    #[test]
    fn box_walls_are_three_faces() {
        let mut c = cfg();
        c.axes_on = false;
        c.box_on = true;
        c.subdiv = 1;
        let v = build_box_lines(&c);
        // 3 walls × 2 line-dirs × (n+1) lines × 2 verts = 3*2*2*2 = 24
        assert_eq!(v.len(), 24);
    }

    #[test]
    fn box_back_walls_follow_camera() {
        let mut c = cfg();
        c.axes_on = false;
        c.box_on = true;
        // eye on +x,+y,+z → far walls at -extent on each axis.
        c.eye = [5.0, 5.0, 5.0];
        let v = build_box_lines(&c);
        for vert in &v {
            // at least one coord pinned to -extent (a far wall), none to +extent only.
            let on_neg = vert.pos.iter().any(|&p| (p + 4.0).abs() < 1e-4);
            assert!(on_neg, "wall vertex not on a -extent face: {:?}", vert.pos);
        }
    }

    #[test]
    fn box_subdiv_grids_walls() {
        let mut c = cfg();
        c.axes_on = false;
        c.box_on = true;
        c.subdiv = 4;
        let v = build_box_lines(&c);
        // 3 walls × 2 dirs × (n+1) × 2 = 3*2*5*2 = 60
        assert_eq!(v.len(), 60);
    }
}

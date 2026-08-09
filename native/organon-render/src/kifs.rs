//! Kaleidoscopic Fractal (KIFS) generator — a fullscreen screen-space field:
//! N-fold kaleidoscopic symmetry feeding a choice of fractal engines (circle
//! inversion, Mandelbox, Sierpinski, log-spiral, Kleinian), with a selectable
//! colour palette cycled at its own rate, optional domain warp, and a flat OR a
//! receding-tunnel projection (`kifs.wgsl`). Camera-independent — it paints linear
//! HDR over the background, riding the shared bloom/tonemap/EDR + Speed/beat stack.
//!
//! Included by `render.rs` via `#[path]`, so it compiles only into the visual binary.

use bytemuck::{Pod, Zeroable};

/// Live KIFS params from the editor (animation already resolved by the visual:
/// `time` is the accumulated generic phase this frame).
#[derive(Clone, Copy)]
pub struct KifsParams {
    pub time: f32,        // accumulated gen_phase (drives rotation/breathe/tunnel flow)
    pub sectors: f32,     // N-fold kaleidoscopic symmetry
    pub fold: f32,        // the engine's main shape constant (c)
    pub iterations: f32,  // IFS iteration count
    pub iter_rot: f32,    // extra rotation applied per iteration
    pub spin: f32,        // overall rotation speed (× time)
    pub breathe: f32,     // breathing-zoom amount
    pub zoom: f32,        // base zoom
    pub tunnel: f32,      // 0 = flat field, 1 = tunnel projection
    pub rays: f32,        // radial ray / spoke count (0 = none)
    pub ring: f32,        // concentric-ring gain
    pub glow: f32,        // overall output gain
    pub hue: f32,         // palette phase offset
    pub pattern: f32,     // fractal engine id (0..4)
    pub palette: f32,     // colour palette id (0..6)
    pub color_speed: f32, // hue-cycle rate (independent of spin)
    pub warp: f32,        // domain-warp amount (organic variability)
    pub flow: f32,        // tunnel forward-motion speed
    pub petals: f32,      // petal-motif count
    pub contrast: f32,    // emission contrast (punch)
    pub sharp: f32,       // motif filament crispness (0..1)
    pub invert: f32,      // figure↔ground: 0 = bright-on-dark, 1 = dark bands on bright
    pub dispersion: f32,  // chromatic prism split (0 = off)
    pub space: f32,       // geometry: 0 Eucl / 1 Hyp / 2 Quasi / 3 Weyl / 4 E8
    pub churn: f32,       // intrinsic self-animation rate (1 = current, 0 = frozen)
    pub view: f32,          // 3D mode: 0 Field / 1 Relief / 2 Conformal (KifsView)
    pub relief: f32,        // relief height (Relief mode)
    pub relief_elev: f32,   // relief auto-orbit camera elevation
    pub relief_steps: f32,  // relief raymarch step budget (perf)
    pub relief_shine: f32,  // relief rim/specular intensity
    pub e8_rings: [f32; 16], // E8 Petrie rings: 8×(radius, phase), from math::e8_petrie_rings
    pub e8_flow: f32,        // 8-D plane-rotation rate (0 = static rings + shell flow)
    pub e8_points: [[f32; 2]; 240], // all 240 roots re-projected each frame (8-D rotation)
    pub e8_points3: [[f32; 3]; 240], // 240 roots in 3-D (Lattice mode E8 shadow)
}

impl Default for KifsParams {
    fn default() -> Self {
        KifsParams {
            time: 0.0,
            sectors: 12.0,
            fold: 0.65,
            iterations: 6.0,
            iter_rot: 0.0,
            spin: 1.0,
            breathe: 0.25,
            zoom: 1.2,
            tunnel: 0.0,
            rays: 18.0,
            ring: 1.0,
            glow: 1.0,
            hue: 0.0,
            pattern: 0.0,
            palette: 0.0,
            color_speed: 0.08,
            warp: 0.0,
            flow: 0.004,
            petals: 6.0,
            contrast: 1.0,
            sharp: 0.5,
            invert: 0.0,
            dispersion: 0.0,
            space: 0.0,
            churn: 1.0,
            view: 0.0,
            relief: 0.5,
            relief_elev: 1.25,
            relief_steps: 96.0,
            relief_shine: 0.5,
            e8_rings: [0.0; 16],
            e8_flow: 0.0,
            e8_points: [[0.0; 2]; 240],
            e8_points3: [[0.0; 3]; 240],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct KifsU {
    p0: [f32; 4], // aspect, time, zoom, sectors
    p1: [f32; 4], // fold_c, iter_rot, iterations, tunnel
    p2: [f32; 4], // ring_gain, ray_count, spin, glow_gain
    p3: [f32; 4], // hue, breathe, pattern_id, palette_id
    p4: [f32; 4], // color_speed, warp, flow, petals
    p5: [f32; 4],     // contrast, sharp, invert, dispersion
    p6: [f32; 4],     // space_id, churn, e8_flow, view (3D mode)
    p7: [f32; 4],     // relief, relief_elev, relief_steps, relief_shine
    e8: [[f32; 4]; 4], // E8 Petrie rings: 8×(radius, phase) packed 2-per-vec4
    e8pts: [[f32; 4]; 120], // 240 re-projected roots (8-D rotation), 2-per-vec4
    e8pts3: [[f32; 4]; 240], // 240 roots in 3-D (Lattice E8 shadow): xyz + pad
}

pub struct KifsField {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    ub: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
}

impl KifsField {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kifs-ub"),
            size: std::mem::size_of::<KifsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kifs-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kifs-bind"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kifs-field"),
            source: wgpu::ShaderSource::Wgsl(include_str!("kifs.wgsl").into()),
        });

        // Flat/tunnel field: only the params (group 0). Overpaints (Always/no-write).
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kifs-field-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = make_pipeline(device, &shader, &layout, color_format, depth_format, sample_count);

        KifsField {
            shader,
            layout,
            pipeline,
            ub,
            bind,
            color_format,
            depth_format,
            sample_count,
        }
    }

    /// Rebuild the pipeline for a new MSAA sample count (matches the scene pass).
    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        let n = n.max(1);
        if n == self.sample_count {
            return;
        }
        self.sample_count = n;
        self.pipeline = make_pipeline(
            device, &self.shader, &self.layout, self.color_format, self.depth_format, n,
        );
    }

    /// Upload the per-frame params. Call once per frame BEFORE the scene pass.
    pub fn prepare(&self, queue: &wgpu::Queue, aspect: f32, p: &KifsParams) {
        let u = KifsU {
            p0: [aspect.max(1e-3), p.time, p.zoom, p.sectors.max(1.0)],
            p1: [p.fold, p.iter_rot, p.iterations.max(1.0), p.tunnel],
            p2: [p.ring, p.rays.max(0.0), p.spin, p.glow.max(0.0)],
            p3: [p.hue, p.breathe, p.pattern.max(0.0), p.palette.max(0.0)],
            p4: [p.color_speed, p.warp.max(0.0), p.flow, p.petals.max(1.0)],
            p5: [
                p.contrast.max(0.05),
                p.sharp.clamp(0.0, 1.0),
                p.invert.clamp(0.0, 1.0),
                p.dispersion.max(0.0),
            ],
            p6: [p.space.max(0.0), p.churn, p.e8_flow.max(0.0), p.view.max(0.0)],
            p7: [
                p.relief.max(0.0),
                p.relief_elev,
                p.relief_steps.max(16.0),
                p.relief_shine.max(0.0),
            ],
            e8: [
                [p.e8_rings[0], p.e8_rings[1], p.e8_rings[2], p.e8_rings[3]],
                [p.e8_rings[4], p.e8_rings[5], p.e8_rings[6], p.e8_rings[7]],
                [p.e8_rings[8], p.e8_rings[9], p.e8_rings[10], p.e8_rings[11]],
                [p.e8_rings[12], p.e8_rings[13], p.e8_rings[14], p.e8_rings[15]],
            ],
            e8pts: {
                let mut packed = [[0.0f32; 4]; 120];
                for i in 0..120 {
                    let a = p.e8_points[2 * i];
                    let b = p.e8_points[2 * i + 1];
                    packed[i] = [a[0], a[1], b[0], b[1]];
                }
                packed
            },
            e8pts3: {
                let mut packed = [[0.0f32; 4]; 240];
                for i in 0..240 {
                    let q = p.e8_points3[i];
                    packed[i] = [q[0], q[1], q[2], 0.0];
                }
                packed
            },
        };
        queue.write_buffer(&self.ub, 0, bytemuck::bytes_of(&u));
    }

    /// Draw the flat/tunnel field into the active scene pass (after the background).
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

/// Fullscreen overpaint — keep depth state (the pass has a depth attachment) but
/// never test or write (the field is the foreground).
fn make_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("kifs-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_kifs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_kifs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                // Alpha blend: flat/tunnel write alpha = 1 (opaque overpaint, the old
                // behaviour); the 3-D relief writes alpha = 0 on a miss so the shared
                // background (starfield / terrain) shows through around the medallion.
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

//! Neural field raymarch mode (#200 Tier 1): a tiny SIREN MLP `(x,y,z,t) →
//! (density, rgb)` raymarched per pixel as an implicit isosurface — a sibling of
//! the Mandelbulb path. The network is regenerated inline in the shader from two
//! integer seeds + a latent-walk `t` (see `mlp.wgsl` / `math.rs::mlp_eval`), so
//! the whole organism travels as a handful of scalars and the beat drives a
//! continuous seed-to-seed morph. Hits shade with the SAME metallic-roughness
//! IBL + key/fill PBR as `cube.wgsl` / `mandelbulb.wgsl` (Standard path). Misses
//! `discard` so the skybox shows through; hits write `frag_depth`.
//!
//! Included by `render.rs` via `#[path]`, like `mandelbulb`/`metaball`, so it
//! compiles only into the visual binary. Reuses the cube uniform bind group
//! (group 0) + the IBL bind group (group 1) + the reaction–diffusion field
//! (group 3); group 2 is this module's params uniform. The shader is
//! `mlp.wgsl` (the MLP library) concatenated with `neural.wgsl` (the raymarch).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Fixed raymarch step cap (the editor's `steps` clamps below this).
const MAX_STEPS: u32 = 400;

/// Live neural-field params from the editor (already animation-resolved by the
/// visual: `time` is the accumulated phase this frame, `walk` the resolved
/// latent-walk position — manual walk + the beat-driven morph).
#[derive(Clone, Copy)]
pub struct NeuralFieldParams {
    pub seed_a: u32,
    pub seed_b: u32,
    pub walk: f32,   // latent-walk position (0 = seed A, 1 = seed B)
    pub omega: f32,  // SIREN feature scale (network frequency)
    pub scale: f32,  // world radius the unit field is blown up to
    pub coord: f32,  // spatial feature scale (detail density)
    pub iso: f32,    // isosurface threshold
    pub steps: u32,  // raymarch step budget
    pub march: f32,  // surface-smoothing (normal sampling epsilon scale)
    pub color: f32,  // colour intensity (0 = near-white, 1 = network hue)
    pub time: f32,   // (x,y,z,t) 4th input — the field's own animation
    pub center: Vec3,
}

impl Default for NeuralFieldParams {
    fn default() -> Self {
        NeuralFieldParams {
            seed_a: 1,
            seed_b: 2,
            walk: 0.0,
            omega: 4.0,
            scale: 120.0,
            coord: 1.5,
            iso: 0.0,
            steps: 96,
            march: 0.6,
            color: 0.8,
            time: 0.0,
            center: Vec3::ZERO,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NeuralU {
    inv_vp: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    p0: [f32; 4],     // seed_a, seed_b, walk, omega
    p1: [f32; 4],     // world_scale, coord_scale, iso, surface_smooth
    p2: [f32; 4],     // steps, color_intensity, time, samples
    p3: [f32; 4],     // size.x, size.y, _, _
    center: [f32; 4], // xyz = world centre, w = bound-sphere radius
}

pub struct NeuralField {
    ray_shader: wgpu::ShaderModule,
    ray_layout: wgpu::PipelineLayout,
    ray_pipeline: wgpu::RenderPipeline,
    // Depth-only variant (#: neural GI): the SAME march writing only frag_depth
    // into the single-sample screen-space-FX prepass, so SSR/SSGI can reconstruct
    // normals and gather off the neural surface. Always sample_count 1 (the prepass
    // depth is single-sample), so it never needs the MSAA rebuild.
    depth_pipeline: wgpu::RenderPipeline,
    ub: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
}

impl NeuralField {
    pub fn new(
        device: &wgpu::Device,
        uniform_layout: &wgpu::BindGroupLayout, // group 0 (cube uniforms)
        ibl_layout: &wgpu::BindGroupLayout,     // group 1 (IBL maps)
        rd_layout: &wgpu::BindGroupLayout,      // group 3 (reaction–diffusion field)
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("neural-ub"),
            size: std::mem::size_of::<NeuralU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("neural-layout"),
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
            label: Some("neural-bind"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });

        // The MLP library + the raymarch, concatenated (mlp.wgsl defines the
        // functions neural.wgsl calls). Same bit-math as math.rs::mlp_eval.
        let src = concat!(include_str!("mlp.wgsl"), "\n", include_str!("neural.wgsl"));
        let ray_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("neural-ray"),
            source: wgpu::ShaderSource::Wgsl(src.into()),
        });
        let ray_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("neural-ray-pl"),
            bind_group_layouts: &[
                Some(uniform_layout),
                Some(ibl_layout),
                Some(&bgl),
                Some(rd_layout),
            ],
            immediate_size: 0,
        });
        let ray_pipeline =
            make_ray_pipeline(device, &ray_shader, &ray_layout, color_format, depth_format, sample_count);
        // Reuses ray_layout (all 4 groups) so draw_depth can bind the same groups;
        // fs_ray_depth only reads groups 0 & 2, but binding the rest is harmless.
        let depth_pipeline = make_depth_pipeline(device, &ray_shader, &ray_layout, depth_format);

        NeuralField {
            ray_shader,
            ray_layout,
            ray_pipeline,
            depth_pipeline,
            ub,
            bind,
            color_format,
            depth_format,
            sample_count,
        }
    }

    /// Rebuild the raymarch pipeline for a new MSAA sample count. No-op if unchanged.
    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        let n = n.max(1);
        if n == self.sample_count {
            return;
        }
        self.sample_count = n;
        self.ray_pipeline = make_ray_pipeline(
            device,
            &self.ray_shader,
            &self.ray_layout,
            self.color_format,
            self.depth_format,
            n,
        );
    }

    /// Upload the per-frame params. `inv_view_proj` is the UNSCALED inverse VP
    /// (the same one the skybox uses), so the field stays put against the backdrop.
    pub fn prepare(&self, queue: &wgpu::Queue, inv_view_proj: Mat4, size: (u32, u32), p: &NeuralFieldParams) {
        let view_proj = inv_view_proj.inverse();
        let steps = p.steps.clamp(8, MAX_STEPS) as f32;
        let radius = p.scale.max(1e-3) * 1.35; // bound sphere padded past the surface
        let samples = self.sample_count.clamp(1, 4) as f32;
        let u = NeuralU {
            inv_vp: inv_view_proj.to_cols_array_2d(),
            view_proj: view_proj.to_cols_array_2d(),
            p0: [p.seed_a as f32, p.seed_b as f32, p.walk, p.omega.max(1e-3)],
            p1: [p.scale.max(1e-3), p.coord.max(1e-3), p.iso, p.march.max(0.0)],
            p2: [steps, p.color.clamp(0.0, 1.0), p.time, samples],
            p3: [size.0.max(1) as f32, size.1.max(1) as f32, 0.0, 0.0],
            center: [p.center.x, p.center.y, p.center.z, radius],
        };
        queue.write_buffer(&self.ub, 0, bytemuck::bytes_of(&u));
    }

    /// Draw the raymarched field into the active scene render pass (after the
    /// skybox). `uniform_bind` = group 0; `ibl_bind` = group 1; `rd_bind` = group 3.
    pub fn draw<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        uniform_bind: &'a wgpu::BindGroup,
        ibl_bind: &'a wgpu::BindGroup,
        rd_bind: &'a wgpu::BindGroup,
    ) {
        rp.set_pipeline(&self.ray_pipeline);
        rp.set_bind_group(0, uniform_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_bind_group(2, &self.bind, &[]);
        rp.set_bind_group(3, rd_bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Depth-only draw into the screen-space-FX depth prepass (single-sample, no
    /// colour target): the same isosurface march writing just `frag_depth`, so
    /// SSR/SSGI reconstruct normals and gather off the neural surface. Binds the
    /// same 4 groups as `draw` (the depth pipeline shares `ray_layout`; only 0 & 2
    /// are read). Call inside the `depth-prepass` render pass.
    pub fn draw_depth<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        uniform_bind: &'a wgpu::BindGroup,
        ibl_bind: &'a wgpu::BindGroup,
        rd_bind: &'a wgpu::BindGroup,
    ) {
        rp.set_pipeline(&self.depth_pipeline);
        rp.set_bind_group(0, uniform_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_bind_group(2, &self.bind, &[]);
        rp.set_bind_group(3, rd_bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

/// Depth-only pipeline for the screen-space-FX prepass: `fs_ray_depth` marches the
/// field and writes `frag_depth` with no colour attachment. Always single-sample
/// (the prepass depth is single-sample) with `Less` depth write.
fn make_depth_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    depth_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("neural-depth-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_ray"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_ray_depth"),
            targets: &[],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: 1, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn make_ray_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("neural-ray-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_ray"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_ray"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                // Premultiplied-alpha "over": coverage as alpha for silhouette AA
                // (same as the Mandelbulb path).
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
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
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

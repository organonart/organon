//! Mandelbulb raymarch mode: a distance-estimated 3-D fractal sphere-traced per
//! pixel (a sibling of the Metaball raymarch path). Unlike Metaball there is no
//! node set and no compute prebake — the distance estimator is evaluated
//! analytically in `mandelbulb.wgsl`. A fullscreen pass marches a ray per pixel
//! against the fractal, takes the DE gradient as the surface normal, and shades
//! it with the SAME metallic-roughness IBL + key/fill PBR as cube.wgsl /
//! metaball.wgsl (Standard path) so the look matches every other mode. Ray
//! misses `discard` so the skybox shows through; hits write `frag_depth` so the
//! surface depth-composites with the skybox and feeds bloom.
//!
//! Included by `render.rs` via `#[path]`, like `metaball`/`post`/`env`, so it
//! compiles only into the visual binary. Reuses the cube uniform bind group
//! (group 0) + the IBL bind group (group 1) + the reaction–diffusion field
//! (group 3); group 2 is this module's small params uniform.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Fixed raymarch step cap (the editor's `detail` clamps below this).
const MAX_STEPS: u32 = 400;

/// Live Mandelbulb params from the editor (already animation-resolved by the
/// visual: `spin_angle`/`morph_angle` are the accumulated phases this frame).
#[derive(Clone, Copy)]
pub struct MandelParams {
    pub power: f32,
    pub iterations: u32,
    pub scale: f32, // world radius the unit fractal is blown up to
    pub steps: u32, // raymarch step budget
    pub spin_angle: f32,
    pub morph_angle: f32,
    pub color: f32, // orbit-trap colour intensity
    pub bailout: f32,
    pub color_phase: f32, // bioluminescent colour-cycle phase (turns)
    pub center: Vec3,
}

impl Default for MandelParams {
    fn default() -> Self {
        MandelParams {
            power: 8.0,
            iterations: 8,
            scale: 150.0,
            steps: 96,
            spin_angle: 0.0,
            morph_angle: 0.0,
            color: 1.0,
            bailout: 2.0,
            color_phase: 0.0,
            center: Vec3::ZERO,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MandelU {
    inv_vp: [[f32; 4]; 4],    // inverse view-projection, to unproject screen rays
    view_proj: [[f32; 4]; 4], // forward matching inv_vp (UNSCALED); for frag_depth
    p0: [f32; 4],             // power, iterations, scale, steps
    p1: [f32; 4],             // spin_angle, morph_angle, bailout, color_intensity
    p2: [f32; 4],             // color_phase, _, _, _
    center: [f32; 4],         // xyz = world centre, w = bound-sphere radius
}

pub struct MandelField {
    ray_shader: wgpu::ShaderModule,
    ray_layout: wgpu::PipelineLayout,
    ray_pipeline: wgpu::RenderPipeline,
    ub: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
}

impl MandelField {
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
            label: Some("mandelbulb-ub"),
            size: std::mem::size_of::<MandelU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mandelbulb-layout"),
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
            label: Some("mandelbulb-bind"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });

        let ray_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mandelbulb-ray"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mandelbulb.wgsl").into()),
        });
        let ray_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mandelbulb-ray-pl"),
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

        MandelField {
            ray_shader,
            ray_layout,
            ray_pipeline,
            ub,
            bind,
            color_format,
            depth_format,
            sample_count,
        }
    }

    /// Rebuild the raymarch pipeline for a new MSAA sample count (matches the
    /// scene pass, like the cube/sky/metaball pipelines). No-op if unchanged.
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

    /// Upload the per-frame params. Call once per frame BEFORE the scene pass.
    /// `inv_view_proj` is the UNSCALED inverse VP (the same one the skybox uses),
    /// so the fractal stays put against the backdrop; the forward matrix for
    /// `frag_depth` is recovered from it.
    pub fn prepare(&self, queue: &wgpu::Queue, inv_view_proj: Mat4, size: (u32, u32), p: &MandelParams) {
        let view_proj = inv_view_proj.inverse();
        let steps = p.steps.clamp(8, MAX_STEPS) as f32;
        let radius = p.scale.max(1e-3) * 1.35; // bound sphere padded past the surface
        // Supersample count = the scene MSAA level (capped at 4 to bound raymarch
        // cost even at 8×); 1 = single ray (the old behaviour).
        let samples = self.sample_count.clamp(1, 4) as f32;
        let u = MandelU {
            inv_vp: inv_view_proj.to_cols_array_2d(),
            view_proj: view_proj.to_cols_array_2d(),
            p0: [p.power, p.iterations.max(1) as f32, p.scale.max(1e-3), steps],
            p1: [p.spin_angle, p.morph_angle, p.bailout.max(1.1), p.color],
            p2: [p.color_phase, samples, size.0.max(1) as f32, size.1.max(1) as f32],
            center: [p.center.x, p.center.y, p.center.z, radius],
        };
        queue.write_buffer(&self.ub, 0, bytemuck::bytes_of(&u));
    }

    /// Draw the raymarched fractal into the active scene render pass (after the
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
        label: Some("mandelbulb-ray-pipeline"),
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
                // Premultiplied-alpha "over": the supersampled fragment emits
                // coverage as alpha so partly-covered silhouette pixels blend with
                // the already-drawn background (true edge AA). Full coverage (α = 1)
                // = opaque overwrite, so interior pixels are unchanged.
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

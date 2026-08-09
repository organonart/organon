//! Lens raymarch mode (#258 Tier 3): an analytic double-convex / plano-convex lens
//! body sphere-traced per pixel as a signed distance field — a sibling of the
//! Mandelbulb / Minimal-surface raymarch paths. There is no node set and no compute
//! prebake; the lens SDF (the CSG of two spheres / a sphere + half-space, clipped by
//! an aperture cylinder) is evaluated analytically in `lens.wgsl`. A fullscreen pass
//! marches a ray per pixel against the body, takes the SDF gradient as the surface
//! normal, and shades it with the SAME metallic-roughness IBL + key/fill PBR as
//! cube.wgsl / minimal.wgsl (Standard / Chrome / Glass) — so the Glass/Refractive
//! material makes it refract, and under the #258 Tier-2 dielectric tracer (a separate
//! branch) the lens focuses. Ray misses `discard` so the skybox shows through; hits
//! write `frag_depth` so the surface depth-composites with the skybox and feeds bloom.
//!
//! Included by `render.rs` via `#[path]`, like `mandelbulb`/`minimal`, so it compiles
//! only into the visual binary. Reuses the cube uniform bind group (group 0) + the IBL
//! bind group (group 1) + the reaction–diffusion field (group 3); group 2 is this
//! module's small params uniform.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Fixed sphere-trace step cap (the editor's `detail` clamps below this). A true SDF
/// converges fast, so this can be modest.
const MAX_STEPS: u32 = 256;

/// Live lens params from the editor. `focal` sets the sphere radius (curvature) the
/// caps are cut from; `aperture` / `thickness` are fractions of `scale` (world size).
#[derive(Clone, Copy)]
pub struct LensParams {
    pub focal: f32,     // curvature dial → sphere radius R = focal · scale
    pub aperture: f32,  // clear-aperture radius as a fraction of scale
    pub thickness: f32, // centre half-thickness as a fraction of scale
    pub plano: bool,    // false = biconvex, true = plano-convex
    pub scale: f32,     // world size
    pub steps: u32,     // raymarch step budget
    pub center: Vec3,
}

impl Default for LensParams {
    fn default() -> Self {
        LensParams {
            focal: 1.0,
            aperture: 0.6,
            thickness: 0.25,
            plano: false,
            scale: 150.0,
            steps: 128,
            center: Vec3::ZERO,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LensU {
    inv_vp: [[f32; 4]; 4],    // inverse view-projection, to unproject screen rays
    view_proj: [[f32; 4]; 4], // forward matching inv_vp (UNSCALED); for frag_depth
    p0: [f32; 4],             // focal, aperture_frac, thickness_frac, plano(0/1)
    p1: [f32; 4],             // scale, steps, _, _
    p2: [f32; 4],             // _, samples, res_x, res_y
    center: [f32; 4],         // xyz = world centre, w = bound-sphere radius
}

pub struct LensField {
    ray_shader: wgpu::ShaderModule,
    ray_layout: wgpu::PipelineLayout,
    ray_pipeline: wgpu::RenderPipeline,
    ub: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
}

impl LensField {
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
            label: Some("lens-ub"),
            size: std::mem::size_of::<LensU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lens-layout"),
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
            label: Some("lens-bind"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });

        let ray_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lens-ray"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lens.wgsl").into()),
        });
        let ray_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lens-ray-pl"),
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

        LensField {
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

    /// Rebuild the raymarch pipeline for a new MSAA sample count (matches the scene
    /// pass, like the cube/sky/mandelbulb pipelines). No-op if unchanged.
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
    /// `inv_view_proj` is the UNSCALED inverse VP (the same one the skybox uses), so
    /// the lens stays put against the backdrop; the forward matrix for `frag_depth`
    /// is recovered from it.
    pub fn prepare(&self, queue: &wgpu::Queue, inv_view_proj: Mat4, size: (u32, u32), p: &LensParams) {
        let view_proj = inv_view_proj.inverse();
        let steps = p.steps.clamp(16, MAX_STEPS) as f32;
        let radius = p.scale.max(1e-3) * 1.35; // bound sphere padded past the body
        // Supersample count = the scene MSAA level (capped at 4); 1 = single ray.
        let samples = self.sample_count.clamp(1, 4) as f32;
        let u = LensU {
            inv_vp: inv_view_proj.to_cols_array_2d(),
            view_proj: view_proj.to_cols_array_2d(),
            p0: [
                p.focal.max(0.05),
                p.aperture.max(0.02),
                p.thickness.max(0.01),
                if p.plano { 1.0 } else { 0.0 },
            ],
            p1: [p.scale.max(1e-3), steps, 0.0, 0.0],
            p2: [0.0, samples, size.0.max(1) as f32, size.1.max(1) as f32],
            center: [p.center.x, p.center.y, p.center.z, radius],
        };
        queue.write_buffer(&self.ub, 0, bytemuck::bytes_of(&u));
    }

    /// Draw the raymarched lens into the active scene render pass (after the skybox).
    /// `uniform_bind` = group 0; `ibl_bind` = group 1; `rd_bind` = group 3.
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
        label: Some("lens-ray-pipeline"),
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
                // Premultiplied-alpha "over": the supersampled fragment emits coverage
                // as alpha so partly-covered silhouette pixels blend with the already-
                // drawn background (true edge AA). Full coverage (α = 1) = opaque
                // overwrite, so interior pixels are unchanged.
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

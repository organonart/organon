//! Creature Engine raymarch mode (GitHub #476, Tier 1): a synthetic sea creature
//! assembled from a union of simple SDF primitives (ellipsoids / tapered
//! round-cones / flattened paddles) placed along a spine, sphere-traced per pixel
//! (a sibling of the Mandelbulb raymarch path). Unlike Mandelbulb the geometry is
//! a small primitive list built CPU-side (`math::creature_body_plan`) and uploaded
//! to the shader; the fragment shader folds them with a per-primitive smooth-union
//! and shades the surface with the SAME metallic-roughness IBL + key/fill PBR as
//! cube.wgsl / mandelbulb.wgsl (Standard path) so the look matches every mode. Ray
//! misses `discard` so the backdrop shows through; hits write `frag_depth` so the
//! surface depth-composites with the skybox and feeds bloom.
//!
//! Included by `render.rs` via `#[path]`, like `mandelbulb`/`metaball`/`post`, so
//! it compiles only into the visual binary. Reuses the cube uniform bind group
//! (group 0) + the IBL bind group (group 1) + the reaction–diffusion field
//! (group 3); group 2 is this module's params uniform (binding 0) + the body-plan
//! primitive list (binding 1).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use organon_core::math::{CreaturePrim, CPRIM_ELLIPSOID, CREATURE_MAX_PRIMS};

/// Fixed raymarch step cap (the editor's `detail` clamps below this).
const MAX_STEPS: u32 = 400;

/// Live Creature params from the editor / beat resolve. `prims` is the body plan
/// (borrowed from the visual's per-form cache); `swim_phase` is the accumulated
/// peristaltic phase this frame.
#[derive(Clone)]
pub struct CreatureParams<'a> {
    pub prims: &'a [CreaturePrim],
    pub scale: f32,     // world size the unit body is blown up to
    pub steps: u32,     // raymarch step budget
    pub swim_phase: f32, // travelling-warp phase (beat-driven)
    pub warp_freq: f32, // spatial frequency of the peristaltic wave
    pub warp_amp: f32,  // amplitude of the peristaltic wave (unit space)
    pub rim: f32,       // Fresnel-rim emissive intensity
    pub glow_scale: f32, // multiplier on the per-primitive emissive glow
    pub bound: f32,     // unit-space bounding-sphere radius of the plan
    // Metachronal wave (#476 Tier 2a): a bright band of light running along +Z.
    pub wave_freq: f32,  // bands per unit length
    pub wave_phase: f32, // travelling phase (beat-driven)
    pub wave_sharp: f32, // band tightness (>= 1)
    pub wave_amount: f32, // 0 = base glow (Tier 1 look), up brightens the band
    // Anatomy overlay (#476 Tier 2c): the projected spine / ring / limb diagram.
    pub overlay_on: bool,
    pub overlay_opacity: f32,
    pub overlay_bright: f32,
    // Colour palette / spectrum (#476 full-PBR follow-up): the `params::Palette` id.
    // 0 = Native (the bioluminescent blue); anything else retints the body along its
    // spine coordinate through `math::palette_tint` (in-shader).
    pub palette: u32,
}

impl<'a> Default for CreatureParams<'a> {
    fn default() -> Self {
        CreatureParams {
            prims: &[],
            scale: 120.0,
            steps: 128,
            swim_phase: 0.0,
            warp_freq: 4.0,
            warp_amp: 0.06,
            rim: 0.6,
            glow_scale: 1.0,
            bound: 2.0,
            wave_freq: 4.0,
            wave_phase: 0.0,
            wave_sharp: 2.5,
            wave_amount: 0.0,
            overlay_on: false,
            overlay_opacity: 1.0,
            overlay_bright: 1.0,
            palette: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CreatureU {
    inv_vp: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    p0: [f32; 4],     // count, steps, scale, swim_phase
    p1: [f32; 4],     // warp_freq, warp_amp, rim, glow_scale
    center: [f32; 4], // xyz world centre, w bound-sphere radius (world)
    p2: [f32; 4],     // samples(msaa), size.x, size.y, _
    p3: [f32; 4],     // metachronal wave: freq, phase, sharp, amount
    p4: [f32; 4],     // palette: id, spine_min (unit z), spine_inv_range, _
}

/// One primitive packed for the GPU (mirrors `CPrim` in `creature.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CPrimGpu {
    v0: [f32; 4], // a.xyz, kind
    v1: [f32; 4], // b.xyz, r1
    v2: [f32; 4], // r2, k, glow, _
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PrimsU {
    prims: [CPrimGpu; CREATURE_MAX_PRIMS],
}

pub struct CreatureField {
    ray_shader: wgpu::ShaderModule,
    ray_layout: wgpu::PipelineLayout,
    ray_pipeline: wgpu::RenderPipeline,
    // Depth-only pipeline for the screen-space-FX prepass (SSR / SSGI). Always
    // single-sample; shares `ray_layout` so `draw_depth` binds the same 4 groups.
    depth_pipeline: wgpu::RenderPipeline,
    ub: wgpu::Buffer,
    prim_ub: wgpu::Buffer,
    bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
}

impl CreatureField {
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
            label: Some("creature-ub"),
            size: std::mem::size_of::<CreatureU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let prim_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("creature-prims"),
            size: std::mem::size_of::<PrimsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("creature-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("creature-bind"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: prim_ub.as_entire_binding() },
            ],
        });

        let ray_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("creature-ray"),
            source: wgpu::ShaderSource::Wgsl(include_str!("creature.wgsl").into()),
        });
        let ray_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("creature-ray-pl"),
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
        let depth_pipeline = make_depth_pipeline(device, &ray_shader, &ray_layout, depth_format);

        CreatureField {
            ray_shader,
            ray_layout,
            ray_pipeline,
            depth_pipeline,
            ub,
            prim_ub,
            bind,
            color_format,
            depth_format,
            sample_count,
        }
    }

    /// Rebuild the raymarch pipeline for a new MSAA sample count (matches the
    /// scene pass, like the cube/sky/mandelbulb pipelines). No-op if unchanged.
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

    /// Upload the per-frame params + body-plan primitives. Call once per frame
    /// BEFORE the scene pass. `inv_view_proj` is the UNSCALED inverse VP (the same
    /// one the skybox uses), so the creature stays put against the backdrop; the
    /// forward matrix for `frag_depth` is recovered from it.
    pub fn prepare(&self, queue: &wgpu::Queue, inv_view_proj: Mat4, size: (u32, u32), p: &CreatureParams) {
        let view_proj = inv_view_proj.inverse();
        let steps = p.steps.clamp(8, MAX_STEPS) as f32;
        let scale = p.scale.max(1e-3);
        let radius = p.bound.max(0.1) * scale; // bound already padded past the surface
        let count = p.prims.len().min(CREATURE_MAX_PRIMS);
        // Supersample count = the scene MSAA level (capped at 4 to bound cost).
        let samples = self.sample_count.clamp(1, 4) as f32;
        // Palette sweep axis: the in-shader palette gradient runs along the body's
        // DOMINANT axis (its longest bounding-box extent in unit space), so it sweeps
        // head→tail on a Z-elongated swimmer AND top→bottom on a Y-oriented bell jelly
        // — a Z-only sweep collapses to a flat block on any non-Z-aligned plan. We
        // pass the chosen axis (0=x/1=y/2=z) + its min + inverse span; the shader
        // normalizes the hit's unit-space coord on that axis to 0..1.
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for pr in p.prims.iter().take(CREATURE_MAX_PRIMS) {
            // `a`/`b` bracket every primitive's placement (centre for ellipsoids/
            // paddles, both endpoints for cones); good enough to span the body.
            lo = lo.min(pr.a).min(pr.b);
            hi = hi.max(pr.a).max(pr.b);
        }
        if !lo.x.is_finite() {
            lo = Vec3::ZERO;
            hi = Vec3::ZERO;
        }
        let span = hi - lo;
        // Dominant axis = the largest extent.
        let axis = if span.x >= span.y && span.x >= span.z {
            0
        } else if span.y >= span.z {
            1
        } else {
            2
        };
        let (amin, aspan) = match axis {
            0 => (lo.x, span.x),
            1 => (lo.y, span.y),
            _ => (lo.z, span.z),
        };
        let ainv = if aspan > 1e-4 { 1.0 / aspan } else { 0.0 };
        let u = CreatureU {
            inv_vp: inv_view_proj.to_cols_array_2d(),
            view_proj: view_proj.to_cols_array_2d(),
            p0: [count as f32, steps, scale, p.swim_phase],
            p1: [p.warp_freq, p.warp_amp, p.rim.max(0.0), p.glow_scale.max(0.0)],
            center: [0.0, 0.0, 0.0, radius],
            p2: [samples, size.0.max(1) as f32, size.1.max(1) as f32, 0.0],
            p3: [p.wave_freq, p.wave_phase, p.wave_sharp.max(1.0), p.wave_amount.max(0.0)],
            p4: [p.palette as f32, amin, ainv, axis as f32],
        };
        queue.write_buffer(&self.ub, 0, bytemuck::bytes_of(&u));

        let mut packed = PrimsU {
            prims: [CPrimGpu { v0: [0.0; 4], v1: [0.0; 4], v2: [0.0; 4] }; CREATURE_MAX_PRIMS],
        };
        for (i, pr) in p.prims.iter().take(CREATURE_MAX_PRIMS).enumerate() {
            packed.prims[i] = CPrimGpu {
                v0: [pr.a.x, pr.a.y, pr.a.z, pr.kind as f32],
                v1: [pr.b.x, pr.b.y, pr.b.z, pr.r1],
                v2: [pr.r2, pr.k, pr.glow, organon_core::math::prim_spine_coord(pr)],
            };
        }
        // A degenerate plan (no primitives) would leave count 0 → nothing marched;
        // seed one tiny ellipsoid so the pass is well-defined (never happens with
        // the built-in forms, which are non-empty).
        if count == 0 {
            packed.prims[0] = CPrimGpu {
                v0: [0.0, 0.0, 0.0, CPRIM_ELLIPSOID as f32],
                v1: [0.2, 0.2, 0.2, 0.0],
                v2: [0.0, 0.1, 0.0, 0.0],
            };
        }
        queue.write_buffer(&self.prim_ub, 0, bytemuck::bytes_of(&packed));
    }

    /// Draw the raymarched creature into the active scene render pass (after the
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
    /// colour target): the same union-of-SDF march writing just `frag_depth`, so
    /// SSR/SSGI reconstruct normals and gather off the creature's surface. Binds the
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
/// creature and writes `frag_depth` with no colour attachment. Always single-sample
/// (the prepass depth is single-sample) with `Less` depth write.
fn make_depth_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    depth_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("creature-depth-pipeline"),
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
        label: Some("creature-ray-pipeline"),
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
                // Premultiplied-alpha "over": coverage from missed sub-samples
                // blends the silhouette against the already-drawn background
                // (true edge AA); full coverage (α = 1) = opaque overwrite.
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

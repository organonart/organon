//! Hardware-RT diffuse global illumination — one bounce (#195 Tier 4, Option B).
//!
//! Per pixel, a cosine-weighted hemisphere gather against the Tier-0 TLAS: each
//! ray's CLOSEST hit is shaded as the outgoing radiance of the neighbour it
//! struck (its glow + a fraction of its direct key light, optionally traced-
//! shadowed), and the average is one bounce of real inter-cube colour bleed —
//! off-screen emitters included, which SSGI can't reach. Runs at SSGI's slot
//! (after the depth prepass, before the composite) and writes into the SAME
//! buffer SSGI fills, so `composite.wgsl` adds it unchanged and a miss (→ 0)
//! leaves the scene's own IBL ambient as the only indirect term — no seam.
//! Supersedes the SSGI march while on.
//!
//! Hits are shaded in the shader from the hit instance's transform + tint
//! (storage-bound copies of the raster buffers) — see rt_gi.wgsl for the
//! local-space reconstruction. Like the other `rt_*` modules, every
//! experimental-API call site stays in here.

use glam::Mat4;

/// Per-frame inputs, threaded through `LightTransport.rt_gi`. `None` there =
/// off (byte-identical path).
#[derive(Clone, Copy)]
pub struct RtGiFrame<'a> {
    /// The Tier-0 TLAS, built by the visual this frame (`rt::RtContext`).
    pub tlas: &'a wgpu::Tlas,
    /// GI intensity (the gathered-radiance scale; 0 = the pass is skipped).
    pub intensity: f32,
    /// Rays per pixel (1–4; TAA integrates the noise).
    pub rays: u32,
    /// Gather ray reach (world units) — how far indirect light travels;
    /// finite-clamped by the visual.
    pub t_max: f32,
    /// Trace a key-light shadow ray at each hit (bounced light is shadowed).
    pub hit_shadows: bool,
    /// The hit-shadow rays' own reach (world units, finite-clamped by the
    /// visual — the Tier-1 scene reach). Deliberately NOT the gather `t_max`:
    /// a short GI-reach dial must not hide occluders from the shadow rays.
    pub shadow_t_max: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RtGiU {
    inv_view_proj: [[f32; 4]; 4],
    // The shader declares `cam_pos: vec4` between inv_view_proj and params and
    // reads the frame index (jitter seed) from its `.w` — this field was
    // missing, so the UBO was 96 B where the shader expects 112 B (draw-time
    // validation failure the moment RT GI was enabled). xyz unused: the gather's
    // world pos comes from the depth reconstruction, not the camera.
    cam_pos: [f32; 4],
    params: [f32; 4],  // intensity, rays, gi reach, _
    params2: [f32; 4], // tube flag, hit-shadow flag, shadow ray reach, _
}

pub struct RtGi {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,      // group 0: scene uniforms + params + depth + instances + tints
    tlas_layout: wgpu::BindGroupLayout, // group 1: the acceleration structure
    scene_ubuf: wgpu::Buffer,           // a verbatim copy of render::Uniforms
    ubuf: wgpu::Buffer,
    frame: u32,
}

impl RtGi {
    /// Requires a device with `EXPERIMENTAL_RAY_QUERY` (the caller only
    /// constructs this once a TLAS exists, which already implies it).
    pub fn new(device: &wgpu::Device) -> Self {
        let storage_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-gi-bgl"),
            entries: &[
                uniform_entry(0),
                uniform_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                storage_entry(3),
                storage_entry(4),
            ],
        });
        let tlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-gi-tlas-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-gi-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_gi.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-gi-pl"),
            bind_group_layouts: &[Some(&layout), Some(&tlas_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rt-gi-pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    // The SSGI buffer's format (post.rs HDR_FORMAT): exposed
                    // indirect radiance the composite adds.
                    format: super::post::HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let scene_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-gi-scene-uniforms"),
            size: std::mem::size_of::<super::Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-gi-uniforms"),
            size: std::mem::size_of::<RtGiU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        RtGi { pipeline, layout, tlas_layout, scene_ubuf, ubuf, frame: 0 }
    }

    /// Gather this frame's one-bounce GI into `target` (the composite's SSGI
    /// buffer). `depth_view` is the shared single-sample prepass depth;
    /// `uniforms` the (jittered) scene uniforms; `inst_buf`/`tint_buf` the
    /// live instance/tint buffers (storage-bound; the TLAS custom indices
    /// point into them).
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        uniforms: &super::Uniforms,
        inst_buf: &wgpu::Buffer,
        tint_buf: &wgpu::Buffer,
        tube: bool,
        p: &RtGiFrame,
    ) {
        self.frame = self.frame.wrapping_add(1);
        queue.write_buffer(&self.scene_ubuf, 0, bytemuck::bytes_of(uniforms));
        let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
        let u = RtGiU {
            inv_view_proj: inv_vp.to_cols_array_2d(),
            // Frame index in .w (the shader's jitter seed); xyz unused.
            cam_pos: [0.0, 0.0, 0.0, self.frame as f32],
            params: [
                p.intensity.max(0.0),
                p.rays.clamp(1, 4) as f32,
                p.t_max.max(1.0),
                0.0,
            ],
            params2: [
                if tube { 1.0 } else { 0.0 },
                if p.hit_shadows { 1.0 } else { 0.0 },
                p.shadow_t_max.max(1.0),
                0.0,
            ],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-gi-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.scene_ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
                wgpu::BindGroupEntry { binding: 3, resource: inst_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: tint_buf.as_entire_binding() },
            ],
        });
        let tlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-gi-tlas-bind"),
            layout: &self.tlas_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::AccelerationStructure(p.tlas),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-gi-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &tlas_bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

//! Hardware-RT shadow mask pass (#195 Tier 1).
//!
//! Runs between the depth prepass and the scene pass: one any-hit ray per
//! pixel toward the key light (plus an optional fill ray) against the Tier-0
//! TLAS, into a screen-space visibility mask (r = key, g = fill) at the render
//! resolution. `cube.wgsl::shadow_factor` samples the mask (group 4 binding 5)
//! instead of the PCF shadow map while the RT strengths are non-zero — the
//! same seam, ground-truth occlusion, no light-frustum fitting or bias tuning.
//!
//! Created lazily on the first frame that actually asks for RT shadows — the
//! pipeline binds an acceleration structure, which requires the ray-query
//! feature, and a TLAS can only reach this module when the feature is live
//! (`rt.rs` gates it). Like `rt.rs`, every experimental-API call site stays in
//! the `rt_*` modules.

use glam::Mat4;

/// Per-frame inputs, threaded through `LightTransport.rt_shadow`. `None` there
/// = RT shadows off (byte-identical path).
#[derive(Clone, Copy)]
pub struct RtShadowFrame<'a> {
    /// The Tier-0 TLAS, built by the visual this frame (`rt::RtContext`).
    pub tlas: &'a wgpu::Tlas,
    /// Key-shadow strength (0..1); 0 = the RT branch is off in the shader.
    pub key_strength: f32,
    /// Fill-shadow strength (0..1); 0 = no fill ray.
    pub fill_strength: f32,
    /// Light angular size (0 = hard shadows; TAA integrates the cone jitter).
    pub softness: f32,
    /// Ray reach (world units) — comfortably past the field from any node.
    pub t_max: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RtShadowU {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    key_dir: [f32; 4],
    fill_dir: [f32; 4],
    params: [f32; 4],
}

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct RtShadow {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,      // group 0: uniforms + prepass depth
    tlas_layout: wgpu::BindGroupLayout, // group 1: the acceleration structure
    ubuf: wgpu::Buffer,
    /// The mask target, sized to the render resolution ((w, h), view).
    mask: Option<((u32, u32), wgpu::TextureView)>,
    /// Bumped whenever the mask texture is (re)created, so the shadow group's
    /// cached bind (group 4) knows to re-point at the new view. Starts at 1
    /// (0 is the shadow group's "dummy bound" state).
    gen: u64,
    /// Frame counter seeding the per-pixel cone jitter.
    frame: u32,
}

impl RtShadow {
    /// Requires a device with `EXPERIMENTAL_RAY_QUERY` (the caller only
    /// constructs this once a TLAS exists, which already implies it).
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-shadow-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let tlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-shadow-tlas-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-shadow-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_shadow.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-shadow-pl"),
            bind_group_layouts: &[Some(&layout), Some(&tlas_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rt-shadow-pipeline"),
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
                    format: MASK_FORMAT,
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
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-shadow-uniforms"),
            size: std::mem::size_of::<RtShadowU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        RtShadow {
            pipeline,
            layout,
            tlas_layout,
            ubuf,
            mask: None,
            gen: 0,
            frame: 0,
        }
    }

    /// The current mask view + its generation, for the shadow group's rebind
    /// check. `None` until the first `run`.
    pub fn mask(&self) -> Option<(&wgpu::TextureView, u64)> {
        self.mask.as_ref().map(|(_, v)| (v, self.gen))
    }

    /// Trace the mask for this frame. `depth_view` is the single-sample
    /// prepass depth the screen-space effects share; `dims` its size;
    /// `uniforms` the (jittered) scene uniforms — the reconstruction must
    /// match the prepass rasterization exactly.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth_view: &wgpu::TextureView,
        dims: (u32, u32),
        uniforms: &super::Uniforms,
        p: &RtShadowFrame,
    ) {
        if self.mask.as_ref().map(|(d, _)| *d) != Some(dims) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("rt-shadow-mask"),
                size: wgpu::Extent3d {
                    width: dims.0.max(1),
                    height: dims.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: MASK_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.mask =
                Some((dims, tex.create_view(&wgpu::TextureViewDescriptor::default())));
            self.gen += 1;
        }
        self.frame = self.frame.wrapping_add(1);
        let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
        let u = RtShadowU {
            inv_view_proj: inv_vp.to_cols_array_2d(),
            cam_pos: [
                uniforms.camera_pos[0],
                uniforms.camera_pos[1],
                uniforms.camera_pos[2],
                self.frame as f32,
            ],
            key_dir: [
                uniforms.key_light[0],
                uniforms.key_light[1],
                uniforms.key_light[2],
                p.softness.clamp(0.0, 1.0),
            ],
            fill_dir: [
                uniforms.fill_light[0],
                uniforms.fill_light[1],
                uniforms.fill_light[2],
                if p.fill_strength > 0.0 { 1.0 } else { 0.0 },
            ],
            params: [p.t_max.max(1.0), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-shadow-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
            ],
        });
        let tlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-shadow-tlas-bind"),
            layout: &self.tlas_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::AccelerationStructure(p.tlas),
            }],
        });
        let (_, mask_view) = self.mask.as_ref().unwrap();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-shadow-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: mask_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
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

//! Hardware-RT ambient occlusion pass (#195 Tier 3).
//!
//! Ground-truth short-range occlusion: 1–4 cosine-weighted hemisphere rays per
//! pixel (t_max = the AO radius) against the Tier-0 TLAS, written into the
//! SAME raw-AO target GTAO fills — `Post::blur_ao` then runs the existing
//! blur, and everything downstream (the composite AO-multiply, the Lagarde
//! specular occlusion in cube.wgsl) is untouched. Kills GTAO's screen-space
//! haloing and picks up off-screen occluders; TAA integrates the low ray
//! count. Selected by the AO card's source switch — GTAO remains the default
//! and the fallback on machines without ray queries.
//!
//! Like the other `rt_*` modules, every experimental-API call site stays here.

use glam::Mat4;

/// Per-frame inputs, threaded through `LightTransport.rt_ao`. `None` there =
/// GTAO (the byte-identical default path).
#[derive(Clone, Copy)]
pub struct RtAoFrame<'a> {
    /// The Tier-0 TLAS, built by the visual this frame (`rt::RtContext`).
    pub tlas: &'a wgpu::Tlas,
    /// Occlusion radius in world units (the SSAO radius dial, shared).
    pub radius: f32,
    /// Rays per pixel (1–4; TAA integrates).
    pub rays: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RtAoU {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4],
}

pub struct RtAo {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,      // group 0: uniforms + prepass depth
    tlas_layout: wgpu::BindGroupLayout, // group 1: the acceleration structure
    ubuf: wgpu::Buffer,
    frame: u32,
}

impl RtAo {
    /// Requires a device with `EXPERIMENTAL_RAY_QUERY` (the caller only
    /// constructs this once a TLAS exists, which already implies it).
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-ao-bgl"),
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
            label: Some("rt-ao-tlas-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-ao-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_ao.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-ao-pl"),
            bind_group_layouts: &[Some(&layout), Some(&tlas_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rt-ao-pipeline"),
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
                    // GTAO's raw target format (post.rs AO_FORMAT).
                    format: wgpu::TextureFormat::R8Unorm,
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
            label: Some("rt-ao-uniforms"),
            size: std::mem::size_of::<RtAoU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        RtAo { pipeline, layout, tlas_layout, ubuf, frame: 0 }
    }

    /// Trace this frame's AO into `target` (the raw-AO texture GTAO would
    /// write; the caller runs `Post::blur_ao` after). `depth_view` is the
    /// shared single-sample prepass depth; `uniforms` the (jittered) scene
    /// uniforms so the reconstruction matches the prepass exactly.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        uniforms: &super::Uniforms,
        p: &RtAoFrame,
    ) {
        self.frame = self.frame.wrapping_add(1);
        let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
        let u = RtAoU {
            inv_view_proj: inv_vp.to_cols_array_2d(),
            cam_pos: [
                uniforms.camera_pos[0],
                uniforms.camera_pos[1],
                uniforms.camera_pos[2],
                self.frame as f32,
            ],
            params: [p.radius.max(1e-3), p.rays.clamp(1, 4) as f32, 0.0, 0.0],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-ao-bind"),
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
            label: Some("rt-ao-tlas-bind"),
            layout: &self.tlas_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::AccelerationStructure(p.tlas),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-ao-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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

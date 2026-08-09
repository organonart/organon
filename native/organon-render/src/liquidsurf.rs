//! Refractive liquid surface (#182 T3b route B, first slice) — host for
//! `liquidsurf.wgsl`.
//!
//! The optically-correct see-through water mode: after the scene resolves
//! (VXGI/SSR/SSGI already in), copy the HDR buffer to a scratch texture, then
//! a fullscreen pass marches the liquid MetaField, Fresnel-splits at the live
//! IOR, refracts the view ray and fetches the REAL scene where the bent ray
//! lands (Beer–Lambert-absorbed over the measured body thickness), env
//! reflection on top. Misses `discard`, so uncovered pixels keep the scene.
//! Included by `render.rs` via `#[path]` (visual binary only).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Matches `LsU` in liquidsurf.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LsU {
    inv_vp: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    cam: [f32; 4],
    bmin: [f32; 4],
    bmax: [f32; 4],
    liq: [f32; 4],
    ctl: [f32; 4],
    misc: [f32; 4],
    vgi0: [f32; 4],
    vgi1: [f32; 4],
}

/// Per-frame inputs for the refractive pass.
pub struct LiquidSurfFrame<'a> {
    pub view_proj: Mat4,
    pub inv_vp: Mat4,
    pub cam: Vec3,
    pub tank_min: Vec3,
    pub tank_max: Vec3,
    pub threshold: f32,
    pub steps: f32,
    /// Liquid colour (what survives absorption) + Beer–Lambert strength.
    pub color: Vec3,
    pub absorption: f32,
    pub ior: f32,
    pub roughness: f32,
    pub env_rotation: f32,
    pub env_intensity: f32,
    pub field_view: &'a wgpu::TextureView,
    /// Scene depth prepass (None → no occlusion clamp, full-volume march).
    pub depth: Option<&'a wgpu::TextureView>,
    /// GI the body scatters (#182 T4): probe uniform buffer, VXGI volume +
    /// sampler, its AABB, and the gain (0 = volume not voxelized this frame).
    pub gi: (&'a wgpu::Buffer, &'a wgpu::TextureView, &'a wgpu::Sampler, Vec3, Vec3, f32),
    /// The many-lights uniform (ghost-lit point lights sparkle on the water).
    pub lights_buf: &'a wgpu::Buffer,
}

pub struct LiquidSurf {
    ubo: wgpu::Buffer,
    samp: wgpu::Sampler,
    dummy_depth: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    // Scratch copy of the resolved HDR scene (recreated on size change).
    scratch: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
}

impl LiquidSurf {
    pub fn new(
        device: &wgpu::Device,
        ibl_layout: &wgpu::BindGroupLayout,
        hdr_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquidsurf"),
            source: wgpu::ShaderSource::Wgsl(include_str!("liquidsurf.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquidsurf-ubo"),
            size: std::mem::size_of::<LsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("liquidsurf-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        // 1×1 depth dummy for the no-prepass case (cam.w = 0 → never sampled
        // for occlusion, but the binding must exist).
        let dtex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("liquidsurf-dummy-depth"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_depth = dtex.create_view(&wgpu::TextureViewDescriptor::default());

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquidsurf-layout"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // #182 T4: probe-GI uniform + the VXGI radiance volume — the
                // murky body scatters the scene's bounce light.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquidsurf-pl"),
            bind_group_layouts: &[Some(&bgl), Some(ibl_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("liquidsurf-pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_liquid"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: hdr_format,
                    blend: None, // covered pixels replace; misses discard
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        LiquidSurf {
            ubo,
            samp,
            dummy_depth,
            pipeline,
            bgl,
            scratch: None,
        }
    }

    /// Copy the resolved HDR scene to the scratch, then draw the refractive
    /// liquid over `hdr_view`. Call after the screen-space GI passes, before
    /// the ink march.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr_tex: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
        size: (u32, u32),
        ibl_bind: &wgpu::BindGroup,
        f: &LiquidSurfFrame,
    ) {
        // (Re)build the scene scratch at the render size.
        let need = self
            .scratch
            .as_ref()
            .map(|(w, h, _, _)| (*w, *h) != size)
            .unwrap_or(true);
        if need {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("liquidsurf-scene"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: hdr_tex.format(),
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.scratch = Some((size.0, size.1, tex, view));
        }
        let (_, _, scratch_tex, scratch_view) = self.scratch.as_ref().unwrap();
        encoder.copy_texture_to_texture(
            hdr_tex.as_image_copy(),
            scratch_tex.as_image_copy(),
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );

        let u = LsU {
            inv_vp: f.inv_vp.to_cols_array_2d(),
            view_proj: f.view_proj.to_cols_array_2d(),
            cam: [f.cam.x, f.cam.y, f.cam.z, if f.depth.is_some() { 1.0 } else { 0.0 }],
            bmin: [f.tank_min.x, f.tank_min.y, f.tank_min.z, f.threshold.max(1e-3)],
            bmax: [f.tank_max.x, f.tank_max.y, f.tank_max.z, f.steps.clamp(16.0, 256.0)],
            liq: [f.color.x, f.color.y, f.color.z, f.absorption.max(0.0)],
            ctl: [
                f.ior.max(1.001),
                f.roughness.clamp(0.0, 1.0),
                f.env_rotation,
                f.env_intensity.max(0.0),
            ],
            misc: [1.0 / size.0.max(1) as f32, 1.0 / size.1.max(1) as f32, 0.0, 0.0],
            vgi0: [f.gi.3.x, f.gi.3.y, f.gi.3.z, f.gi.5.max(0.0)],
            vgi1: [f.gi.4.x, f.gi.4.y, f.gi.4.z, 0.0],
        };
        queue.write_buffer(&self.ubo, 0, bytemuck::bytes_of(&u));

        // The HDR texture + depth can be recreated across frames — rebuild the
        // bind group every run (once per frame, trivially cheap).
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquidsurf-bind"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(f.field_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.samp),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(scratch_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        f.depth.unwrap_or(&self.dummy_depth),
                    ),
                },
                wgpu::BindGroupEntry { binding: 5, resource: f.gi.0.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(f.gi.1),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(f.gi.2),
                },
                wgpu::BindGroupEntry { binding: 8, resource: f.lights_buf.as_entire_binding() },
            ],
        });

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("liquidsurf-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: hdr_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

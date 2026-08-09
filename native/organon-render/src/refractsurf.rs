//! Screen-space refraction (#214 Tier 5 part 2) — host for `refractsurf.wgsl`.
//!
//! The instanced Refractive material's see-through of the ACTUAL scene behind it.
//! After the scene resolves (the cube field shaded with its env-only refraction),
//! copy the HDR buffer to a scratch, then a fullscreen pass reconstructs each
//! covered pixel's world position + normal from the single-sample depth prepass,
//! refracts the view ray at the live IOR, projects the bent ray back to screen, and
//! fetches the RESOLVED SCENE there — so a cube shows its neighbours / the world
//! behind it, displaced. Off-screen fetches fall back to the pixel's own colour, so
//! there's no seam. Gated on the Refractive material + a strength dial; when off it
//! is not dispatched, so the frame is byte-identical.
//!
//! Modeled on `liquidsurf.rs` but reconstructs from depth (no density field to
//! march), with a slimmer bind group (no IBL / GI / lights). Included by `render.rs`
//! via `#[path]` (visual binary only).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Matches `RsU` in refractsurf.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RsU {
    inv_vp: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    cam: [f32; 4],
    ctl: [f32; 4],  // ior, absorption, strength, displace dist
    tint: [f32; 4], // rgb = absorption colour
    texel: [f32; 4],
}

/// Per-frame inputs for the refraction pass.
pub struct RefractSurfFrame<'a> {
    pub view_proj: Mat4,
    pub inv_vp: Mat4,
    pub cam: Vec3,
    pub ior: f32,
    /// Beer–Lambert absorption strength; `tint` is what survives.
    pub absorption: f32,
    pub tint: Vec3,
    /// Blend strength (0 → not dispatched); how far the transmission is replaced.
    pub strength: f32,
    /// World-space step along the refracted ray before re-projecting.
    pub dist: f32,
    /// Scene depth prepass (single-sample, the same one SSAO/SSR read).
    pub depth: &'a wgpu::TextureView,
}

pub struct RefractSurf {
    ubo: wgpu::Buffer,
    samp_n: wgpu::Sampler,
    samp_l: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    // Scratch copy of the resolved HDR scene (recreated on size change).
    scratch: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
}

impl RefractSurf {
    pub fn new(device: &wgpu::Device, hdr_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("refractsurf"),
            source: wgpu::ShaderSource::Wgsl(include_str!("refractsurf.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refractsurf-ubo"),
            size: std::mem::size_of::<RsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let samp_n = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("refractsurf-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let samp_l = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("refractsurf-linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("refractsurf-layout"),
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
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("refractsurf-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("refractsurf-pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_refract"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: hdr_format,
                    blend: None, // covered pixels replace; uncovered pass the scene through
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
        RefractSurf {
            ubo,
            samp_n,
            samp_l,
            pipeline,
            bgl,
            scratch: None,
        }
    }

    /// Copy the resolved HDR scene to the scratch, then draw the refraction over
    /// `hdr_view`. Call after the scene resolves (the SSR / liquidsurf seam),
    /// before bloom/composite.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr_tex: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
        size: (u32, u32),
        f: &RefractSurfFrame,
    ) {
        let need = self
            .scratch
            .as_ref()
            .map(|(w, h, _, _)| (*w, *h) != size)
            .unwrap_or(true);
        if need {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("refractsurf-scene"),
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

        let u = RsU {
            inv_vp: f.inv_vp.to_cols_array_2d(),
            view_proj: f.view_proj.to_cols_array_2d(),
            cam: [f.cam.x, f.cam.y, f.cam.z, 0.0],
            ctl: [f.ior.max(1.001), f.absorption.max(0.0), f.strength.clamp(0.0, 1.0), f.dist.max(0.0)],
            tint: [f.tint.x, f.tint.y, f.tint.z, 0.0],
            texel: [
                1.0 / size.0.max(1) as f32,
                1.0 / size.1.max(1) as f32,
                size.0 as f32,
                size.1 as f32,
            ],
        };
        queue.write_buffer(&self.ubo, 0, bytemuck::bytes_of(&u));

        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("refractsurf-bind"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(f.depth),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.samp_n) },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(scratch_view),
                },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.samp_l) },
            ],
        });

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("refractsurf-pass"),
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
        rp.draw(0..3, 0..1);
    }
}

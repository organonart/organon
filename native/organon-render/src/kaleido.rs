//! Scene Kaleidoscope (#361 Tier 1) — host for `kaleido.wgsl`.
//!
//! A post-stage kaleidoscopic fold applied to the LIVE, physically-lit generator
//! render (any generator + surface mode), rather than a procedural fractal field
//! like the KIFS generator. After the scene resolves into the linear HDR buffer
//! (and after the pre-bloom light passes), this copies that buffer to a scratch
//! texture and runs a fullscreen pass that folds each output pixel's screen
//! coordinate through N-fold kaleidoscopic symmetry and samples the scene there —
//! so the reflected shards are real, moving, lit geometry. It writes back into the
//! HDR buffer BEFORE the bloom/tonemap composite, so highlights + EDR stay physical.
//!
//! Included by `render.rs` via `#[path]`, so it compiles only into the visual binary.

use bytemuck::{Pod, Zeroable};

/// Live kaleidoscope params (already resolved by the visual: `angle` folds in the
/// animation clock so the fold rides global Speed + the beat). `enabled` gates the
/// whole pass in `render.rs` (off → the HDR buffer is untouched, byte-identical).
#[derive(Clone, Copy)]
pub struct KaleidoParams {
    pub enabled: bool,
    pub sectors: f32,  // N-fold symmetry
    pub mode: f32,     // 0 = FullFrame (whole frame per slice), 1 = Wedge (classic)
    pub angle: f32,    // field rotation this frame (spin·time + roll·TAU)
    pub zoom: f32,     // source sample zoom (grab the busy part of the frame)
    pub center: [f32; 2], // source sample centre offset
    pub mix: f32,      // 0 = untouched scene … 1 = fully folded
    pub twist: f32,    // log-polar spiral twist (0 = none)
    pub tint_hue: f32, // hue grade on the folded scene (degrees)
    pub tint_amt: f32, // hue grade amount (0 = off)
    pub seam: f32,     // mirror-seam supersample softening (0 = sharp single tap)
}

impl Default for KaleidoParams {
    fn default() -> Self {
        KaleidoParams {
            enabled: false,
            sectors: 6.0,
            mode: 0.0,
            angle: 0.0,
            zoom: 1.0,
            center: [0.0, 0.0],
            mix: 1.0,
            twist: 0.0,
            tint_hue: 0.0,
            tint_amt: 0.0,
            seam: 0.5,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct KaleidoU {
    p0: [f32; 4], // aspect, sectors, mode, angle
    p1: [f32; 4], // zoom, center_x, center_y, mix
    p2: [f32; 4], // twist, tint_hue, tint_amt, seam
    p3: [f32; 4], // texel_x, texel_y, _, _
}

pub struct Kaleido {
    ubo: wgpu::Buffer,
    samp: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    // Scratch snapshot of the resolved HDR scene (recreated on size change).
    scratch: Option<(u32, u32, wgpu::Texture, wgpu::TextureView)>,
}

impl Kaleido {
    pub fn new(device: &wgpu::Device, hdr_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kaleido"),
            source: wgpu::ShaderSource::Wgsl(include_str!("kaleido.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kaleido-ubo"),
            size: std::mem::size_of::<KaleidoU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kaleido-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kaleido-layout"),
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
                        view_dimension: wgpu::TextureViewDimension::D2,
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
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kaleido-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kaleido-pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_kaleido"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_kaleido"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: hdr_format,
                    blend: None, // full overwrite of the HDR buffer
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
        Kaleido {
            ubo,
            samp,
            pipeline,
            bgl,
            scratch: None,
        }
    }

    /// Snapshot the resolved HDR scene into the scratch, then fold it back over
    /// `hdr_view`. Call after the pre-bloom light passes, before the composite.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr_tex: &wgpu::Texture,
        hdr_view: &wgpu::TextureView,
        size: (u32, u32),
        p: &KaleidoParams,
    ) {
        // (Re)allocate the scratch to match the HDR buffer.
        let need = self.scratch.as_ref().map(|(w, h, ..)| (*w, *h) != size).unwrap_or(true);
        if need {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("kaleido-scratch"),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
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

        let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
        let u = KaleidoU {
            p0: [aspect.max(1e-3), p.sectors.max(1.0), p.mode, p.angle],
            p1: [p.zoom.max(1e-3), p.center[0], p.center[1], p.mix.clamp(0.0, 1.0)],
            p2: [p.twist, p.tint_hue, p.tint_amt.clamp(0.0, 1.0), p.seam.max(0.0)],
            p3: [
                1.0 / size.0.max(1) as f32,
                1.0 / size.1.max(1) as f32,
                0.0,
                0.0,
            ],
        };
        queue.write_buffer(&self.ubo, 0, bytemuck::bytes_of(&u));

        // The HDR texture (and thus the scratch view) can be recreated across frames
        // — rebuild the bind group each run (once per frame, trivially cheap).
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kaleido-bind"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(scratch_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.samp),
                },
            ],
        });

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kaleido-pass"),
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

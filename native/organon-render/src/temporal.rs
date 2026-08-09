//! Temporal pass (#152 Tier 2): motion blur + TAA.
//!
//! A final pass AFTER the composite (composite.wgsl untouched). When enabled, the
//! composite renders into this module's `src` texture instead of the view; `apply()`
//! then runs `temporal.wgsl` — motion blur + TAA — and writes the resolved image to
//! the **view** and a **history** texture (MRT) reprojected next frame.
//!
//! Velocity is reconstructed from depth via CAMERA reprojection (current world
//! position from depth → previous-frame clip via the previous view-proj). That
//! captures camera motion (the dominant motion in this app); per-object deformation
//! ghosts mildly and is bounded by the neighbourhood clamp. #174 T3 made this a
//! real jittered supersampler: the visual applies a Halton-(2,3) sub-pixel jitter
//! to the scene view-proj while TAA is on, this pass reprojects with the
//! UNJITTERED matrices, clamps history in YCoCg, and dilates the velocity to the
//! closest depth in a 3×3 neighbourhood (silhouettes reproject with the
//! foreground's motion).
//!
//! Same structure as the renderer's other final passes; included by `render.rs` via
//! `#[path]`, so it compiles only into the visual binary.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

/// History ping-pong format — always float (see `build_targets`): an 8-bit history
/// quantize-stalls the TAA moving average on SDR surfaces.
const HIST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Live temporal params (built from `Shared.temporal` in the visual). `Copy` so it
/// rides in `RenderFrame`. Carries the current + previous scene view-proj for the
/// camera-reprojection velocity.
#[derive(Clone, Copy)]
pub struct TemporalParams {
    pub enabled: bool, // master (TAA or motion blur on)
    pub taa: bool,
    pub motion_blur: bool,
    pub taa_blend: f32,   // current-frame weight (0.05..1; lower = more history)
    pub taa_sharpen: f32, // post-blend sharpen
    pub mb_amount: f32,   // shutter strength
    pub mb_samples: f32,  // motion-blur taps
    pub cur_view_proj: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
}

/// Matches `TemporalU` in temporal.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TemporalU {
    inv_cur_vp: [[f32; 4]; 4],
    prev_vp: [[f32; 4]; 4],
    texel: [f32; 4],
    params: [f32; 4], // taa_blend, taa_sharpen, mb_amount, mb_samples
    flags: [f32; 4],  // taa_enabled, mb_enabled, depth_valid, _
}

struct Targets {
    size: (u32, u32),
    _src: wgpu::Texture,
    src_view: wgpu::TextureView,
    _hist: [wgpu::Texture; 2],
    hist_views: [wgpu::TextureView; 2],
}

pub struct Temporal {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    ub: wgpu::Buffer,
    _dummy_depth_tex: wgpu::Texture,
    dummy_depth: wgpu::TextureView,
    surface_format: wgpu::TextureFormat,
    targets: Option<Targets>,
    parity: usize,
}

impl Temporal {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("temporal"),
            source: wgpu::ShaderSource::Wgsl(include_str!("temporal.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("temporal-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("temporal-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("temporal-ub"),
            size: std::mem::size_of::<TemporalU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline = make_pipeline(device, &module, &layout, surface_format);

        let dummy_depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("temporal-dummy-depth"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_depth = dummy_depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        Temporal {
            layout,
            pipeline,
            module,
            sampler,
            ub,
            _dummy_depth_tex: dummy_depth_tex,
            dummy_depth,
            surface_format,
            targets: None,
            parity: 0,
        }
    }

    pub fn set_surface_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if format == self.surface_format {
            return;
        }
        self.surface_format = format;
        self.pipeline = make_pipeline(device, &self.module, &self.layout, format);
        self.targets = None;
    }

    pub fn ensure(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
            self.parity = 0;
        }
    }

    pub fn src_view(&self) -> &wgpu::TextureView {
        &self.targets.as_ref().expect("temporal::ensure must run first").src_view
    }

    fn build_targets(&self, device: &wgpu::Device, size: (u32, u32)) -> Targets {
        let mk = |label: &str, format: wgpu::TextureFormat| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            (t, v)
        };
        // The TAA history is an exponential moving average — through an 8-bit SDR
        // surface format the EMA quantize-stalls (once |prev − cur| is under half an
        // LSB, mix() rounds back to prev), which bands gradients and freezes decay.
        // The history is sampled, never presented, so it can be Rgba16Float
        // unconditionally; only `src` must match the composite's output format.
        let (src, src_view) = mk("temporal-src", self.surface_format);
        let (h0, h0v) = mk("temporal-hist-0", HIST_FORMAT);
        let (h1, h1v) = mk("temporal-hist-1", HIST_FORMAT);
        Targets {
            size,
            _src: src,
            src_view,
            _hist: [h0, h1],
            hist_views: [h0v, h1v],
        }
    }

    /// Resolve: read `src` (+ depth + previous history) and write the result to
    /// `view` and the next history (MRT). `depth` is the single-sample scene depth
    /// for velocity, or `None` (TAA still blends, just without reprojection).
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        size: (u32, u32),
        p: &TemporalParams,
    ) {
        self.ensure(device, size);
        let Some(t) = self.targets.as_ref() else { return };
        let (w, h) = t.size;

        let inv_cur = Mat4::from_cols_array_2d(&p.cur_view_proj).inverse();
        let depth_valid = if depth.is_some() { 1.0 } else { 0.0 };
        queue.write_buffer(
            &self.ub,
            0,
            bytemuck::bytes_of(&TemporalU {
                inv_cur_vp: inv_cur.to_cols_array_2d(),
                prev_vp: p.prev_view_proj,
                texel: [1.0 / w as f32, 1.0 / h as f32, w as f32, h as f32],
                params: [p.taa_blend, p.taa_sharpen, p.mb_amount, p.mb_samples],
                flags: [
                    if p.taa { 1.0 } else { 0.0 },
                    if p.motion_blur { 1.0 } else { 0.0 },
                    depth_valid,
                    0.0,
                ],
            }),
        );

        let prev = self.parity;
        let cur = 1 - self.parity;
        let depth_view = depth.unwrap_or(&self.dummy_depth);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("temporal-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.src_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(depth_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&t.hist_views[prev]) },
            ],
        });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("temporal-pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &t.hist_views[cur],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        self.parity = cur;
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("temporal-pl"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let target = wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    };
    // MRT: location 0 = the view (surface format), location 1 = the float history.
    let hist_target = wgpu::ColorTargetState { format: HIST_FORMAT, ..target.clone() };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("temporal-pipeline"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_fullscreen"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_temporal"),
            targets: &[Some(target), Some(hist_target)],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

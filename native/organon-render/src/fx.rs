//! Post-composite creative FX (#152, Tier 1).
//!
//! A final pass that runs AFTER the HDR composite (`composite.wgsl` is left
//! untouched, so the precious HDR/EDR/gamut path is unchanged). When the editor's
//! "Post FX" master is on, the composite writes into this module's `src` texture
//! instead of the swapchain; `apply()` then runs `fx.wgsl` over it — pixelate →
//! DoF → chromatic aberration → NPR style → grade → vignette → grain → feedback —
//! and writes the result to BOTH the view (swapchain / production texture) and a
//! history texture (MRT) sampled next frame for echo trails.
//!
//! When the master is off the renderer never calls into here, so the default look
//! is byte-identical.
//!
//! Included by `render.rs` via `#[path]`, like `post`/`metaball` — it compiles
//! only into the visual binary.

use bytemuck::{Pod, Zeroable};

/// Feedback-history ping-pong format — always float (see `build_targets`): through
/// an 8-bit SDR surface format the trail's exponential decay quantize-stalls (once
/// |prev − cur| is under half an LSB, mix() rounds back to prev), so trails froze
/// as permanent smears instead of fading out.
const HIST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Live FX params (built from `Shared.fx` in the visual). `Copy` so it rides in
/// `RenderFrame`.
#[derive(Clone, Copy)]
pub struct FxParams {
    pub enabled: bool,
    pub style: f32,
    pub style_amt: f32,
    pub dof: f32,
    pub dof_focus: f32,
    pub dof_range: f32,
    pub chroma: f32,
    pub vignette: f32,
    pub grain: f32,
    pub grade_sat: f32,
    pub grade_contrast: f32,
    pub grade_temp: f32,
    pub grade_gain: f32,
    pub feedback: f32,
    pub outline: f32,
    /// Wall-clock seconds, for animated film grain.
    pub time: f32,
    // Cinematic finishing (#167 Tier 1) — halation + lens flares.
    pub hal_amount: f32,
    pub hal_threshold: f32,
    pub hal_width: f32,
    pub hal_warmth: f32,
    pub lf_amount: f32,
    pub lf_ghosts: f32,
    pub lf_halo: f32,
    pub lf_streak: f32,
    /// Key-light screen position (uv) the flare is anchored to, + its visibility
    /// (0 when the light is off-frame / behind the camera; scaled by key intensity).
    /// Computed CPU-side in visual.rs by projecting the key-light direction.
    pub lf_light_x: f32,
    pub lf_light_y: f32,
    pub lf_visibility: f32,
    /// Camera projection near/far — the DoF focus slider maps 0..1 onto an
    /// exponential near→far world distance, converted to raw depth in the shader.
    pub cam_near: f32,
    pub cam_far: f32,
}

/// Matches `FxU` in fx.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FxU {
    p0: [f32; 4],    // style, style_amt, dof_amount, dof_focus
    p1: [f32; 4],    // dof_range, chroma, vignette, grain
    p2: [f32; 4],    // grade_sat, grade_contrast, grade_temp, grade_gain
    p3: [f32; 4],    // feedback, outline_thresh, time, dof_enabled
    texel: [f32; 4], // 1/w, 1/h, w, h
    p4: [f32; 4],    // #167 T1 halation: amount, threshold, width, warmth
    p5: [f32; 4],    // #167 T1 lens flare: amount, ghosts, halo, streak
    p6: [f32; 4],    // #167 T1 lens flare anchor: light_u, light_v, visibility, _
    p7: [f32; 4],    // camera near, camera far (DoF focus remap), _, _
}

struct Targets {
    size: (u32, u32),
    _src: wgpu::Texture,
    src_view: wgpu::TextureView,
    // Ping-pong history (echo trails): read prev, write cur, then swap.
    _hist: [wgpu::Texture; 2],
    hist_views: [wgpu::TextureView; 2],
}

pub struct Fx {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    ub: wgpu::Buffer,
    // 1×1 depth bound when no scene depth is available (DoF then off).
    _dummy_depth_tex: wgpu::Texture,
    dummy_depth: wgpu::TextureView,
    surface_format: wgpu::TextureFormat,
    targets: Option<Targets>,
    parity: usize,
}

impl Fx {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fx"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fx.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fx-layout"),
            entries: &[
                // 0: composited source colour
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
                // 1: filtering sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // 2: uniform
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
                // 3: scene depth (sampled by textureLoad — no sampler)
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
                // 4: previous frame (feedback trails)
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
            label: Some("fx-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fx-ub"),
            size: std::mem::size_of::<FxU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline = make_pipeline(device, &module, &layout, surface_format);

        // 1×1 dummy depth so the bind group is always valid when there's no
        // scene-depth prepass (raymarch modes / DoF off).
        let dummy_depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fx-dummy-depth"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_depth = dummy_depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        Fx {
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

    /// Rebuild the pipeline + drop the targets for a new surface format (HDR toggle:
    /// sRGB 8-bit ↔ `Rgba16Float`). `src`/`hist` follow the surface format, so the
    /// round-trip stays gamut-correct.
    pub fn set_surface_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if format == self.surface_format {
            return;
        }
        self.surface_format = format;
        self.pipeline = make_pipeline(device, &self.module, &self.layout, format);
        self.targets = None; // force a rebuild at the new format
    }

    /// (Re)create the source + history textures if the size changed. Call before
    /// `src_view()` / `apply()`.
    pub fn ensure(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
            self.parity = 0;
        }
    }

    /// The texture the composite should render into when FX is engaged.
    pub fn src_view(&self) -> &wgpu::TextureView {
        &self.targets.as_ref().expect("fx::ensure must run first").src_view
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
        // History is sampled, never presented → float always (see HIST_FORMAT);
        // only `src` must match the composite's output format.
        let (src, src_view) = mk("fx-src", self.surface_format);
        let (h0, h0v) = mk("fx-hist-0", HIST_FORMAT);
        let (h1, h1v) = mk("fx-hist-1", HIST_FORMAT);
        Targets {
            size,
            _src: src,
            src_view,
            _hist: [h0, h1],
            hist_views: [h0v, h1v],
        }
    }

    /// Apply the FX stack: read `src` (+ depth + previous history) and write the
    /// final image to `view` and the next history texture (MRT). `depth` is the
    /// single-sample scene depth for DoF, or `None` (DoF off).
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        size: (u32, u32),
        p: &FxParams,
    ) {
        self.ensure(device, size);
        let Some(t) = self.targets.as_ref() else { return };
        let (w, h) = t.size;

        let dof_enabled = if depth.is_some() && p.dof > 0.0 { 1.0 } else { 0.0 };
        queue.write_buffer(
            &self.ub,
            0,
            bytemuck::bytes_of(&FxU {
                p0: [p.style, p.style_amt, p.dof, p.dof_focus],
                p1: [p.dof_range, p.chroma, p.vignette, p.grain],
                p2: [p.grade_sat, p.grade_contrast, p.grade_temp, p.grade_gain],
                p3: [p.feedback, p.outline, p.time, dof_enabled],
                texel: [1.0 / w as f32, 1.0 / h as f32, w as f32, h as f32],
                p4: [p.hal_amount, p.hal_threshold, p.hal_width, p.hal_warmth],
                p5: [p.lf_amount, p.lf_ghosts, p.lf_halo, p.lf_streak],
                p6: [p.lf_light_x, p.lf_light_y, p.lf_visibility, 0.0],
                p7: [p.cam_near, p.cam_far, 0.0, 0.0],
            }),
        );

        let prev = self.parity; // history to READ
        let cur = 1 - self.parity; // history to WRITE
        let depth_view = depth.unwrap_or(&self.dummy_depth);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fx-bind"),
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
                label: Some("fx-pass"),
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
        label: Some("fx-pl"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    // Two targets: location 0 = view (surface format), location 1 = next history
    // (always float — see HIST_FORMAT).
    let target = wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    };
    let hist_target = wgpu::ColorTargetState { format: HIST_FORMAT, ..target.clone() };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fx-pipeline"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_fullscreen"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_fx"),
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

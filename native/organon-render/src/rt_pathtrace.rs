//! Hardware-RT progressive path tracer (#200 Tier 4 — the ground-truth substrate).
//!
//! When the ground-truth toggle is on, this REPLACES the raster scene pass: it
//! path-traces the whole image against the #195 Tier-0 TLAS (`rt_pathtrace.wgsl`)
//! and MRTs the result into (a) its own ping-pong accumulation buffer and (b) the
//! HDR scene buffer bloom + the composite tonemap already consume — so exposure /
//! tone-map / EDR all apply unchanged. One sample/pixel/frame is progressively
//! averaged whenever the camera is still (the visual resets the sample count on
//! any camera move), so a held frame converges to reference over seconds. Like
//! the other `rt_*` modules, every experimental-API call site stays in here.

use glam::Mat4;

/// Per-frame inputs, threaded through `LightTransport.pathtrace`. `None` there =
/// off (the normal raster path runs, byte-identical).
#[derive(Clone, Copy)]
pub struct PathtraceFrame<'a> {
    /// The Tier-0 TLAS, built by the visual this frame (`rt::RtContext`).
    pub tlas: &'a wgpu::Tlas,
    /// Samples already accumulated (0 = the camera just moved → restart).
    pub spp: u32,
    /// Path length (diffuse bounces, 1–12).
    pub bounces: u32,
    /// Ray reach (world units), finite-clamped by the visual.
    pub reach: f32,
    /// Frame index (RNG decorrelation seed).
    pub frame: u32,
    /// The **UNJITTERED** view-proj (the visual Halton-jitters `uniforms.view_proj`
    /// for TAA, but the tracer skips TAA — camera rays must use the clean matrix or
    /// the per-frame clip jitter fights the progressive accumulation while still).
    pub unjittered_view_proj: [[f32; 4]; 4],
    /// Dielectric BTDF enable (#258 Tier 2). `false` → the bounce loop stays
    /// diffuse-only, byte-identical to before. `true` → Glass/Refractive shade as a
    /// stochastic two-interface dielectric (Fresnel split, refract on entry + exit,
    /// TIR, Beer–Lambert) and Chrome as a perfect mirror.
    pub pt_dielectric: bool,
    /// Beer–Lambert absorption strength for rays travelling INSIDE the medium
    /// (σ per channel = (1 − albedo) × this). 0 = clear glass. Inert unless
    /// `pt_dielectric` is on.
    pub pt_absorb: f32,
    /// Composite mode: 0 = Replace (overwrite the HDR scene — ground truth), 1 =
    /// Blend (alpha-blend the trace over the raster PBR image by `pt_augment`), 2 =
    /// GI-add (add the trace's INDIRECT light onto the raster). Replace is the
    /// original behaviour.
    pub pt_composite: u32,
    /// Augment amount (0..1) for Blend / GI-add: the trace's opacity over the raster
    /// (Blend) or the indirect-light gain (GI-add). 0 = raster untouched.
    pub pt_augment: f32,
    /// Analytic lens (#258 Tier 3): the raymarched lens SDF isn't in the TLAS, so the
    /// tracer intersects it directly. `[cx, cy, cz, active, r, dz, aper, plano]` —
    /// world centre, active flag (1 = the Lens generator is running), sphere radius,
    /// sphere-centre axial offset, clear-aperture radius, plano-convex flag. All 0 =
    /// no lens (the tracer traces only the TLAS, as before).
    pub lens: [f32; 8],
    /// Spectral light transport (#258 Tier 4): `[spectral_on, abbe, secondaries, _]`.
    /// `spectral_on` = 0 → the RGB tracer (byte-identical). > 0 → each path is
    /// monochromatic and glass/lens refracts at a per-λ Cauchy IOR (dispersion set by
    /// the Abbe number); `secondaries` extra stratified wavelengths per pixel.
    pub spectral: [f32; 4],
    /// Photon-mapped caustics (#258 Tier 5): `[enable, photons, intensity, radius]`.
    /// `enable` = 0 → no photon pass is dispatched and the tracer's output is
    /// byte-identical. > 0 → `rt_caustic` light-traces `photons` paths from the key
    /// light through the specular chain each frame and the tracer adds the resolved
    /// splat map into its accumulation; `intensity` scales it, `radius` is the
    /// screen-space gather (KDE) radius in pixels.
    pub caustic: [f32; 4],
    /// Scene bounding sphere `[cx, cy, cz, r]` — the photon emission disc (and the
    /// photon ray reach). The visual derives it from the framing bounds.
    pub scene_sphere: [f32; 4],
    /// World-space size of one output pixel at unit distance from the camera
    /// (`2·tan(fovy/2) / height_px`) — converts a photon deposit's flux into
    /// per-pixel radiance (the splat footprint).
    pub pixel_scale: f32,
    /// Neural radiance cache (#256 Tier 0): `[enable, confidence, omega,
    /// terminate_bounce]`. When `enable > 0.5`, a diffuse path whose next bounce is
    /// at depth ≥ `terminate_bounce` **terminates into a cache query** of the
    /// incoming radiance along the bounce direction, added at `confidence` weight
    /// (`nrc.wgsl`), instead of tracing on. `enable = 0` → the tracer is byte-
    /// identical (no query, no weight upload).
    pub nrc: [f32; 4],
    /// The live cache's 419 SIREN weights (`math::NRC_WEIGHTS`), uploaded to the
    /// query storage buffer. Empty when the cache is off (the buffer stays zeroed
    /// and the shader never reads it because `nrc[0] == 0`).
    pub nrc_weights: &'a [f32],
    /// The field AABB the cache normalizes query positions against (must match the
    /// bbox the visual trains with). `[min, max]`.
    pub nrc_bbox_min: [f32; 3],
    pub nrc_bbox_max: [f32; 3],
    /// Neural radiance cache — RT-stack synergies (#256 Tier 1): `[guide_on,
    /// guide_candidates, firefly_on, firefly_clamp]`. `guide_on` → the diffuse bounce
    /// is chosen by RIS over the cache; `firefly_on` → per-sample outliers are clamped
    /// toward the cache mean. Both `0` (default) → the bounce/sample are unchanged.
    /// The visual only arms these when the Tier-0 cache is live.
    pub nrc1: [f32; 4],
    /// Cache-lit reflections (#256 Tier 2): when true (and the cache + dielectric are
    /// on), a specular Chrome/Glass ray terminates into a cache query along its
    /// reflected/refracted direction → it reflects the *lit* neighbours + off-screen
    /// light, not just the env map. `false` → the specular ray traces on as before.
    pub nrc_reflect: bool,
    /// Cache volumetrics (#256 Tier 3): `[volume_on, volume_density, volume_steps,
    /// volume_strength]`. When `volume_on` (+ the cache), the tracer marches the
    /// primary camera ray through a participating medium and queries the cache for the
    /// in-scattered radiance → god-rays / haze. `0` → no march (byte-identical).
    pub nrc_volume: [f32; 4],
    /// Cached caustics (#256 Tier 3): `[caustic_on, caustic_gain, _, _]`. When
    /// `caustic_on` (+ the cache), the tracer adds the cache's mirror-direction
    /// radiance at the primary hit — the focused light a camera path misses through
    /// glass — so it blooms. `0` → byte-identical.
    pub nrc_caustic: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PtU {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4], // xyz = camera world pos, w = spp
    params: [f32; 4],  // bounces, tube, reach, frame
    params2: [f32; 4], // #258 T2: x = dielectric enable, y = absorption, z = composite mode, w = augment
    lens0: [f32; 4],   // #258 T3: lens world centre + active flag (xyz, active)
    lens1: [f32; 4],   // #258 T3: lens shape (r, dz, aperture, plano)
    spectral: [f32; 4], // #258 T4: spectral_on, Abbe number, secondaries, _
    caustic: [f32; 4],  // #258 T5: x = caustic map live (add it to the radiance)
    nrc0: [f32; 4],     // #256 T0: enable, confidence, omega, terminate_bounce
    nrc_bmin: [f32; 4], // #256 T0: field AABB min (xyz) for the cache position encode
    nrc_bmax: [f32; 4], // #256 T0: field AABB max (xyz)
    nrc1: [f32; 4],     // #256 T1: guide_on, guide_candidates, firefly_on, firefly_clamp
    nrc_vol: [f32; 4],  // #256 T3: volume_on, volume_density, volume_steps, volume_strength
    nrc_caus: [f32; 4], // #256 T3: caustic_on, caustic_gain, _, _
}

pub struct PathTracer {
    pipeline: wgpu::RenderPipeline,        // Replace: overwrite the scene target
    pipeline_blend: wgpu::RenderPipeline,  // Blend: alpha-over the raster
    pipeline_add: wgpu::RenderPipeline,    // GI-add: additive onto the raster
    layout: wgpu::BindGroupLayout,
    tlas_layout: wgpu::BindGroupLayout,
    scene_ubuf: wgpu::Buffer,
    ubuf: wgpu::Buffer,
    // Ping-pong accumulation (read prev, write cur); parity flips each frame.
    accum: [Option<wgpu::TextureView>; 2],
    size: (u32, u32),
    parity: u32,
    // Photon-mapped caustics (#258 T5): lazily built while enabled; `dummy_caustic`
    // (a zero 1×1) satisfies binding 5 whenever the pass isn't live.
    caustic: Option<super::rt_caustic::CausticMap>,
    dummy_caustic: wgpu::TextureView,
    // #256 T0: the live radiance cache's weight storage buffer (419 f32). Written
    // each frame from `PathtraceFrame.nrc_weights` when the cache is on; stays
    // zeroed (and unread) when off.
    nrc_wbuf: wgpu::Buffer,
}

/// organon#217 T8 — where the tracer's group 0 carries the per-instance emission
/// buffer (`rt_pathtrace.wgsl`'s `emits`): after the caustic map (5) and the cache
/// weights (6). One constant, used by the layout, the bind group and the test.
pub(crate) const EMIT_BINDING: u32 = 7;

/// A fragment-visible, read-only storage-buffer layout entry — the shape every
/// per-instance buffer the hit shading reads (instances, tints, emission) takes.
pub(crate) fn readonly_storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl PathTracer {
    /// The group-0 layout as data — one entry per `@binding` the shader declares.
    /// Pure (no device) so a CPU test can hold it: wgpu validates a bind group
    /// against its layout at **draw** time, so an entry declared here with no
    /// matching `create_bind_group` entry, or a shader `@binding` that disagrees,
    /// is a runtime panic no leg of the bar can reach (`tests` pins the
    /// organon#217 T8 emit entry against the shader source).
    pub(crate) fn layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
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
        let storage_entry = readonly_storage_entry;
        vec![
            uniform_entry(0),
            uniform_entry(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            storage_entry(3),
            storage_entry(4),
            // #258 T5: the resolved photon-caustic map (a zero dummy when off).
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // #256 T0: the live radiance cache's SIREN weights (read-only storage;
            // a zeroed 419-float buffer when the cache is off — the shader gates).
            storage_entry(6),
            // organon#217 T8: the per-instance emission the cube pipeline draws at
            // @location(8) — `emit_buf`, bound as storage beside the instances and
            // tints. All-zero on every non-glyph frame, so the trace is byte-identical.
            storage_entry(EMIT_BINDING),
        ]
    }

    /// Requires a device with `EXPERIMENTAL_RAY_QUERY` (the caller only builds
    /// this once a TLAS exists, which already implies it).
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-pt-bgl"),
            entries: &Self::layout_entries(),
        });
        let tlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-pt-tlas-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-pt-shader"),
            // #256 T0: the radiance-cache query library (`nrc.wgsl`) is concatenated
            // ahead of the tracer, exactly like the `mlp.wgsl` include pattern, so the
            // early-termination can call `nrc_query`. The `enable wgpu_ray_query;`
            // directive is prepended so it precedes nrc.wgsl's global consts (WGSL
            // requires all directives ahead of any global declaration).
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    "enable wgpu_ray_query;\n",
                    include_str!("nrc.wgsl"),
                    "\n",
                    include_str!("rt_pathtrace.wgsl")
                )
                .into(),
            ),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-pt-pl"),
            bind_group_layouts: &[Some(&layout), Some(&tlas_layout)],
            immediate_size: 0,
        });
        let hdr = super::post::HDR_FORMAT;
        // Three pipelines that differ ONLY in the SCENE target's (loc1) blend, for
        // the composite modes. The accumulation target (loc0) is always overwrite.
        //   Replace  → blend None: the scene write overwrites the raster (ground truth).
        //   Blend    → alpha over: src.a = augment → mix(raster, trace, augment).
        //   GI-add   → additive:   raster + trace (the shader pre-scales by augment).
        let alpha_over = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let mk = |scene_blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rt-pt-pipeline"),
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
                    // loc0 = accumulation (ping-pong), loc1 = HDR scene buffer.
                    targets: &[
                        Some(wgpu::ColorTargetState { format: hdr, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                        Some(wgpu::ColorTargetState { format: hdr, blend: scene_blend, write_mask: wgpu::ColorWrites::ALL }),
                    ],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = mk(None);
        let pipeline_blend = mk(Some(alpha_over));
        let pipeline_add = mk(Some(additive));
        let scene_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-pt-scene-uniforms"),
            size: std::mem::size_of::<super::Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-pt-uniforms"),
            size: std::mem::size_of::<PtU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // wgpu zero-initializes textures on first use, so the dummy reads black.
        let dummy_caustic = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("rt-pt-caustic-dummy"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        // #256 T0: 419 SIREN weights → a small read-only storage buffer. Zero-init
        // so a query on a not-yet-uploaded cache reads 0 (a black, harmless field).
        let nrc_wbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-pt-nrc-weights"),
            size: (organon_core::math::NRC_WEIGHTS * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        PathTracer {
            pipeline,
            pipeline_blend,
            pipeline_add,
            layout,
            tlas_layout,
            scene_ubuf,
            ubuf,
            accum: [None, None],
            size: (0, 0),
            parity: 0,
            caustic: None,
            dummy_caustic,
            nrc_wbuf,
        }
    }

    fn ensure_accum(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.size != size || self.accum[0].is_none() {
            let mk = |label: &str| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size.0.max(1),
                            height: size.1.max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: super::post::HDR_FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            self.accum = [Some(mk("rt-pt-accum-0")), Some(mk("rt-pt-accum-1"))];
            self.size = size;
        }
    }

    /// Path-trace one sample into `hdr_target` (the post HDR scene buffer),
    /// progressively averaged into the internal accumulation. `inst_buf`/
    /// `tint_buf`/`emit_buf` are the live instance/tint/emission buffers (the TLAS
    /// custom indices point into all three; `emit_buf` is the organon#217 T8
    /// addition — the cube pipeline's `@location(8)` buffer, all-zero on every
    /// non-glyph frame). `emit_lit` is how many of its entries may be non-zero this
    /// frame (the renderer's high-water mark, 0 when no ring is live); it is passed
    /// straight through to the photon pass, which needs a *count* to build its
    /// emitter CDF over and cannot learn one from an all-zero buffer without reading
    /// every entry of it. Call INSTEAD of the raster scene pass while active.
    #[allow(clippy::too_many_arguments)]
    pub fn trace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hdr_target: &wgpu::TextureView,
        size: (u32, u32),
        uniforms: &super::Uniforms,
        inst_buf: &wgpu::Buffer,
        tint_buf: &wgpu::Buffer,
        emit_buf: &wgpu::Buffer,
        emit_lit: u32,
        tube: bool,
        p: &PathtraceFrame,
    ) {
        self.ensure_accum(device, size);
        // On a restart (spp = 0) the previous accumulation is ignored by the
        // shader, so parity only needs to advance so read ≠ write.
        let prev_idx = (self.parity & 1) as usize;
        let cur_idx = 1 - prev_idx;
        self.parity = cur_idx as u32;

        queue.write_buffer(&self.scene_ubuf, 0, bytemuck::bytes_of(uniforms));

        // Photon-mapped caustics (#258 T5): light-trace + splat + resolve BEFORE the
        // trace pass reads the map. Only worth dispatching when something specular
        // exists to focus light (a dielectric/chrome material, or the analytic lens)
        // AND the tracer's own specular chain is active — the mirror/glass/lens
        // branches in `rt_pathtrace.wgsl` are all gated on `pt_dielectric`, so with
        // it off camera paths treat glass as diffuse; firing the photon pass then
        // would focus light the tracer never accounts for (inconsistent double-light).
        let caustics_live = p.caustic[0] > 0.5
            && p.pt_dielectric
            && (p.lens[3] > 0.5 || (uniforms.amb[1] >= 0.5 && uniforms.amb[1] < 3.5));
        if caustics_live {
            // organon#217 T8b: the photon pass takes the emission buffer too — not to
            // shade with, but to sample photon SOURCES from. `emit_lit` (0 on every
            // non-glyph frame) is what turns that on; there is no dial.
            self.caustic.get_or_insert_with(|| super::rt_caustic::CausticMap::new(device)).run(
                device,
                queue,
                encoder,
                size,
                &self.scene_ubuf,
                inst_buf,
                tint_buf,
                emit_buf,
                emit_lit,
                tube,
                p,
            );
        }
        let caustic_view = if caustics_live {
            self.caustic.as_ref().and_then(|c| c.view()).unwrap_or(&self.dummy_caustic)
        } else {
            &self.dummy_caustic
        };

        // Camera rays unproject through the UNJITTERED VP (not `uniforms.view_proj`,
        // which the visual Halton-jitters for TAA) so a still camera integrates a
        // stable pixel instead of one that wobbles by the clip jitter each frame.
        let inv_vp = Mat4::from_cols_array_2d(&p.unjittered_view_proj).inverse();
        let u = PtU {
            inv_view_proj: inv_vp.to_cols_array_2d(),
            cam_pos: [
                uniforms.camera_pos[0],
                uniforms.camera_pos[1],
                uniforms.camera_pos[2],
                p.spp as f32,
            ],
            params: [
                p.bounces.clamp(1, 12) as f32,
                if tube { 1.0 } else { 0.0 },
                p.reach.max(1.0),
                p.frame as f32,
            ],
            params2: [
                if p.pt_dielectric { 1.0 } else { 0.0 },
                p.pt_absorb.max(0.0),
                p.pt_composite as f32,          // composite mode (0 Replace / 1 Blend / 2 GI-add)
                p.pt_augment.clamp(0.0, 1.0),   // augment amount
            ],
            lens0: [p.lens[0], p.lens[1], p.lens[2], p.lens[3]],
            lens1: [p.lens[4], p.lens[5], p.lens[6], p.lens[7]],
            spectral: p.spectral,
            caustic: [if caustics_live { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            // #256 T0: the live-cache early-termination controls + field AABB.
            // nrc_bmin.w carries the #256 T2 cache-lit-reflections flag (spare slot).
            nrc0: p.nrc,
            nrc_bmin: [
                p.nrc_bbox_min[0],
                p.nrc_bbox_min[1],
                p.nrc_bbox_min[2],
                if p.nrc_reflect { 1.0 } else { 0.0 },
            ],
            nrc_bmax: [p.nrc_bbox_max[0], p.nrc_bbox_max[1], p.nrc_bbox_max[2], 0.0],
            // #256 T1: guided sampling + firefly clamp.
            nrc1: p.nrc1,
            // #256 T3: volumetrics + cached caustics.
            nrc_vol: p.nrc_volume,
            nrc_caus: p.nrc_caustic,
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));

        // #256 T0: upload the trained cache weights when the cache is live. When off
        // (`nrc[0] == 0`) the buffer keeps its zero fill and the shader never reads
        // it, so the trace is byte-identical.
        if p.nrc[0] > 0.5 && p.nrc_weights.len() >= organon_core::math::NRC_WEIGHTS {
            queue.write_buffer(
                &self.nrc_wbuf,
                0,
                bytemuck::cast_slice(&p.nrc_weights[..organon_core::math::NRC_WEIGHTS]),
            );
        }

        let prev = self.accum[prev_idx].as_ref().unwrap();
        let cur = self.accum[cur_idx].as_ref().unwrap();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-pt-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.scene_ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(prev) },
                wgpu::BindGroupEntry { binding: 3, resource: inst_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: tint_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(caustic_view) },
                wgpu::BindGroupEntry { binding: 6, resource: self.nrc_wbuf.as_entire_binding() },
                // organon#217 T8: the per-instance emission (every entry the layout
                // declares must be supplied here, or wgpu rejects the bind group at draw).
                wgpu::BindGroupEntry { binding: EMIT_BINDING, resource: emit_buf.as_entire_binding() },
            ],
        });
        let tlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-pt-tlas-bind"),
            layout: &self.tlas_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::AccelerationStructure(p.tlas),
            }],
        });
        let clear = wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
        };
        // Replace overwrites the HDR scene (Clear); the augment modes (Blend / GI-add)
        // KEEP the raster PBR image already in `hdr_target` (Load) and blend onto it.
        // The accumulation target `cur` is always cleared.
        let scene_ops = if p.pt_composite == 0 {
            clear
        } else {
            wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-pt-pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment { view: cur, depth_slice: None, resolve_target: None, ops: clear }),
                Some(wgpu::RenderPassColorAttachment { view: hdr_target, depth_slice: None, resolve_target: None, ops: scene_ops }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(match p.pt_composite {
            1 => &self.pipeline_blend,
            2 => &self.pipeline_add,
            _ => &self.pipeline,
        });
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &tlas_bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// organon#217 T8 — the one invariant shared by the three passes that shade a hit,
/// checked without a device. wgpu validates a bind group against its layout at
/// **draw** time and a shader's `@binding` against the layout at pipeline creation —
/// both on a GPU, which no leg of the bar has. So the CPU pins the two halves it can
/// read: the layout entry (index, read-only storage, fragment-visible) and the shader
/// source declaring `emits` at the same index.
#[cfg(test)]
pub(crate) mod emit_binding {
    /// The three passes that SHADE a hit read the emission in `fs_main`, so their
    /// entry is fragment-visible. organon#217 T8b's photon pass reads the same buffer
    /// from a compute entry point, to sample photon sources rather than to shade —
    /// same invariant, other stage.
    pub fn check(pass: &str, entries: &[wgpu::BindGroupLayoutEntry], binding: u32, shader: &str) {
        check_visible(pass, entries, binding, shader, wgpu::ShaderStages::FRAGMENT);
    }

    pub fn check_visible(
        pass: &str,
        entries: &[wgpu::BindGroupLayoutEntry],
        binding: u32,
        shader: &str,
        visibility: wgpu::ShaderStages,
    ) {
        let mut seen = std::collections::HashSet::new();
        for e in entries {
            assert!(seen.insert(e.binding), "{pass}: @binding({}) is declared twice in the layout", e.binding);
        }
        let e = entries.iter().find(|e| e.binding == binding).unwrap_or_else(|| {
            panic!(
                "{pass}: no bind-group layout entry for the emit buffer at @binding({binding}) — \
                 the shader's `emits` would be rejected at pipeline creation, on a GPU CI does not have"
            )
        });
        assert_eq!(
            e.visibility, visibility,
            "{pass}: the emit buffer must be visible to the stage that reads it"
        );
        assert_eq!(
            e.ty,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            "{pass}: the emit buffer must be a read-only storage buffer, like the instances and tints"
        );
        let decl = format!("@group(0) @binding({binding}) var<storage, read> emits: array<vec4<f32>>;");
        assert!(
            shader.contains(&decl),
            "{pass}: the shader does not declare `emits` at @binding({binding}) — layout and shader have drifted"
        );
    }

    /// The text of a shader's `instance_emission` function, from its `fn` line to the
    /// closing brace — what the three passes must agree on. Line endings are
    /// normalised first: this checkout is CRLF on Windows and LF elsewhere, and the
    /// agreement is about the expression, not the platform.
    pub fn instance_emission_fn(shader: &str) -> String {
        let shader = shader.replace('\r', "");
        let start = shader
            .find("fn instance_emission(")
            .expect("the shader has no `instance_emission` function");
        let end = shader[start..].find("\n}").expect("unterminated `instance_emission`") + start + 2;
        shader[start..end].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SHADER: &str = include_str!("rt_pathtrace.wgsl");

    #[test]
    fn the_tracer_binds_the_emit_buffer_where_its_shader_reads_it() {
        let entries = PathTracer::layout_entries();
        emit_binding::check("rt_pathtrace", &entries, EMIT_BINDING, SHADER);
        // Seven bindings before T8 (scene, params, accum, insts, tints, caustic, nrc) + one.
        assert_eq!(entries.len(), 8, "rt_pathtrace: group 0 should carry exactly eight entries");
    }

    #[test]
    fn an_emissive_hit_terminates_both_integrators() {
        // The RGB loop and the hero-wavelength loop each shade a hit; an emitter must
        // end the path in BOTH, gated on the emission's value (never on "is a glyph").
        let gate = "if (any(h.emit > vec3<f32>(0.0))) { break; }";
        assert_eq!(
            SHADER.matches(gate).count(),
            2,
            "rt_pathtrace.wgsl: the emissive-hit termination must appear in the RGB and the spectral loop"
        );
        // And the term itself is added as its own product, so `throughput * 0.0 + 0.0`
        // leaves the pre-T8 sum byte-identical (invariant #4).
        assert!(SHADER.contains("radiance += throughput * h.albedo * u.mat.z + throughput * h.emit;"));
        assert!(SHADER.contains("l_rad += tp * spectral_response(h.emit, lambda);"));
    }

    #[test]
    fn the_three_traced_passes_add_the_same_expression_cube_wgsl_adds() {
        // §9's second law: a cell's apparent brightness must track the effect's value on
        // BOTH paths. The raster term is `in.emit.rgb * in.emit.w` at @location(8); every
        // traced pass must add the same product, and the three must not drift apart.
        let pt = emit_binding::instance_emission_fn(SHADER);
        let rf = emit_binding::instance_emission_fn(include_str!("rt_reflect.wgsl"));
        let gi = emit_binding::instance_emission_fn(include_str!("rt_gi.wgsl"));
        assert_eq!(pt, rf, "rt_pathtrace and rt_reflect disagree on `instance_emission`");
        assert_eq!(pt, gi, "rt_pathtrace and rt_gi disagree on `instance_emission`");
        assert!(pt.contains("let e = emits[idx];") && pt.contains("return e.rgb * e.w;"), "{pt}");
        assert!(
            include_str!("cube.wgsl").contains("+ in.emit.rgb * in.emit.w"),
            "cube.wgsl no longer adds `emit.rgb * emit.w` — the traced passes copy that expression"
        );
    }

    #[test]
    fn the_visibility_only_passes_do_not_read_emission() {
        // rt_shadow / rt_ao never shade a hit — a hit is a boolean — so emission cannot
        // mean anything to them. If one grows an `emits` binding, that is a design change
        // this test should make someone write down.
        for (name, src) in [
            ("rt_shadow.wgsl", include_str!("rt_shadow.wgsl")),
            ("rt_ao.wgsl", include_str!("rt_ao.wgsl")),
        ] {
            assert!(!src.contains("var<storage, read> emits"), "{name} binds the emit buffer");
        }
    }

    #[test]
    fn the_photon_pass_reads_emission_to_sample_with_never_to_shade_with() {
        // organon#217 T8b: rt_caustic DOES bind the emit buffer now — the entry the layout
        // comment here reserved. What has not changed is why the pass would not have: a
        // photon's transport is the LANDING surface's BSDF, and that surface's own emission
        // plays no part in it. So the caustic shader must never grow the `instance_emission`
        // the three shading passes agree on, and its deposit must stay the BSDF product.
        const CAUSTIC: &str = include_str!("rt_caustic.wgsl");
        assert!(
            CAUSTIC.contains("var<storage, read> emits"),
            "rt_caustic.wgsl no longer binds the emit buffer — T8b's photon sources are gone"
        );
        assert!(
            !CAUSTIC.contains("fn instance_emission("),
            "rt_caustic.wgsl grew `instance_emission` — it would be adding a landing \
             surface's emission into a photon deposit, which is not what a photon carries"
        );
        assert!(
            CAUSTIC.replace('\r', "").contains("            dep = tp * alb;\n"),
            "rt_caustic.wgsl's deposit is no longer throughput × the receiver's albedo"
        );
    }
}

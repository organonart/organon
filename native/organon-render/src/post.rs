//! HDR post-processing. The scene renders into a linear `Rgba16Float` buffer
//! (`hdr`); this module then builds bloom from it (soft-knee bright-pass →
//! downsample/upsample chain) and runs a final composite pass that applies
//! exposure, adds bloom, ACES-tonemaps, and writes to the sRGB surface.
//!
//! Keeping the scene in float until this last step is what lets the 128-bit HDR
//! environment drive highlights and bloom instead of being clamped per-fragment.

/// Format of the scene + bloom buffers. The cube/skybox pipelines target this.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const MAX_BLOOM_MIPS: usize = 6;

pub struct PostParams {
    pub exposure: f32,
    pub bloom_intensity: f32,
    pub bloom_threshold: f32,
    /// EDR headroom for the composite tonemap. `1.0` = SDR (ACES); `> 1.0` =
    /// HDR output (highlights roll off toward this value). Set from the display's
    /// measured headroom when the HDR surface is active; `1.0` otherwise.
    pub hdr_max: f32,
    /// HDR roll-off knee (0..1, in SDR-white units): where highlights start
    /// rolling off toward `hdr_max`. Only used in HDR mode.
    pub hdr_knee: f32,
    /// SDR tone-map operator id (0 ACES, 1 AgX, 2 Reinhard, 3 Neutral, 4 ACES
    /// Fitted) for geometry.
    pub tonemap: f32,
    /// Tone-map operator id for the environment backdrop (same id space). Lets the
    /// HDR panorama use a gentler curve than the cubes. Used in both SDR and HDR
    /// output (in HDR the backdrop still tone-maps while geometry uses the shoulder).
    pub bg_tonemap: f32,
    /// Ambient occlusion: enabled (0/1) + intensity. When disabled the composite
    /// uses AO = 1 (no darkening) and the AO texture is not sampled.
    pub ao_enabled: f32,
    pub ao_intensity: f32,
    /// Wide-gamut output (#119): 0 = Rec.709 (no conversion), 1 = the EDR surface is
    /// tagged Rec.2020 so the composite converts/expands into it. Only acts in HDR.
    pub gamut: f32,
    /// Gamut-expansion amount (0..1): 0 = colour-accurate Rec.709→Rec.2020, 1 = full
    /// stretch of the spectrum to the Rec.2020 primaries (max vividness).
    pub vivid: f32,
    /// Wall-clock seconds — drives the SDR output dither's per-frame offset
    /// (#174 T3).
    pub time: f32,
    /// Learned upscaler (#200 Tier 5c): `up_mode` 0 = bilinear (byte-identical),
    /// 1 = content-adaptive sharpen reconstruction. The renderer sets it to 1 only
    /// when actually upscaling (render_scale < 1) AND the feature is enabled.
    pub up_mode: f32,
    /// Sharpen strength (also the network influence) + the filter-network seed.
    pub up_sharpen: f32,
    pub up_seed: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DownU {
    texel: [f32; 2],
    threshold: f32,
    knee: f32,
    prefilter: f32,
    exposure: f32,
    _p: [f32; 2],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UpU {
    texel: [f32; 2],
    radius: f32,
    _p: f32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompU {
    exposure: f32,
    bloom_intensity: f32,
    hdr_max: f32,
    hdr_knee: f32,
    tonemap: f32,
    ao_enabled: f32,
    ao_intensity: f32,
    bg_tonemap: f32,
    // SSR (#80 A): when > 0.5 the composite adds the reflection buffer (exposed).
    ssr_enabled: f32,
    // Wide-gamut output (#119): gamut flag (0 Rec.709 / 1 Rec.2020) + expansion amount.
    gamut: f32,
    vivid: f32,
    // SSGI (#152 Tier 2): when > 0.5 the composite adds the one-bounce GI buffer.
    ssgi_enabled: f32,
    // Frame counter for the SDR output dither (#174 T3).
    frame: f32,
    // Learned upscaler (#200 Tier 5c): mode (0 bilinear / 1 adaptive-sharpen),
    // sharpen strength, seed. Repurposes the old 3 tail pad slots (struct stays 64
    // bytes). matches composite.wgsl's CompU tail.
    up_mode: f32,
    up_sharpen: f32,
    up_seed: f32,
}

struct Targets {
    size: (u32, u32),
    _hdr: wgpu::Texture, // kept alive; the view + bind groups reference it
    hdr_view: wgpu::TextureView,
    // Multisampled scene color (when sample_count > 1): the scene renders here and
    // resolves into `hdr_view`, which the bloom/composite read as single-sample.
    msaa_view: Option<wgpu::TextureView>,
    // SSAO: raw (noisy) AO then blurred AO. Always allocated; only written when
    // SSAO is enabled (the composite samples `ao_blur_view`).
    ao_raw_view: wgpu::TextureView,
    ao_blur_view: wgpu::TextureView,
    // SSR reflection buffer (#80 A). Allocated LAZILY on first use (#174 T2 — a
    // full-res Rgba16Float, ~66 MB at 4K, idled whenever SSR was off); a 1×1 dummy
    // keeps the composite bind group valid until then (composite gates on a flag).
    ssr_view: Option<wgpu::TextureView>,
    // SSGI buffer (#152 Tier 2). Same lazy scheme as SSR.
    ssgi_view: Option<wgpu::TextureView>,
    // Temporal accumulator (#200 Tier 4½ p3): the RT reflection/GI pass writes
    // this shared raw buffer (instead of the SSR/SSGI view) when temporal is on;
    // the accumulator reads it + the ping-pong history and writes the SSR/SSGI
    // view (composite reads) + the new history. Lazy; a `[valid, parity]` pair
    // per effect drives the ping-pong. All reset on resize (Targets rebuild).
    rt_raw: Option<wgpu::TextureView>,
    refl_hist: [Option<wgpu::TextureView>; 2],
    gi_hist: [Option<wgpu::TextureView>; 2],
    // Part 4 (variance-guided SVGF): per-effect ping-pong moments (μ1, μ2, n,
    // σ²), lockstep with the colour history above (same parity/validity).
    refl_mom: [Option<wgpu::TextureView>; 2],
    gi_mom: [Option<wgpu::TextureView>; 2],
    refl_hist_valid: bool,
    gi_hist_valid: bool,
    refl_parity: u32,
    gi_parity: u32,
    // À-trous denoiser scratch (#200 Tier 4½ part 2): the ping-pong partner the
    // 2-iteration filter bounces the reflection/GI buffer through, landing back
    // in the source. Lazily allocated on first denoise.
    denoise_scratch: Option<wgpu::TextureView>,
    // Cached per-pass bind groups (#174 T2 — these were recreated every frame).
    // Keyed by the caller's depth key (epoch + prepass route); rebuilt when the
    // depth source or the targets change.
    ao_bind: Option<(u64, wgpu::BindGroup, wgpu::BindGroup)>,
    ssr_bind: Option<(u64, wgpu::BindGroup)>,
    ssgi_bind: Option<(u64, wgpu::BindGroup)>,
    // bloom mip chain (level 0 = half res), each its own texture/view
    bloom_views: Vec<wgpu::TextureView>,
    bloom_sizes: Vec<(u32, u32)>,
    down_binds: Vec<wgpu::BindGroup>, // one per down pass (len = N)
    up_binds: Vec<wgpu::BindGroup>,   // one per up pass   (len = N-1)
    comp_bind: wgpu::BindGroup,
}

pub struct Post {
    sampler: wgpu::Sampler,
    sample_layout: wgpu::BindGroupLayout,
    comp_layout: wgpu::BindGroupLayout,
    down_pipeline: wgpu::RenderPipeline,
    up_pipeline: wgpu::RenderPipeline,
    comp_pipeline: wgpu::RenderPipeline,
    // Kept so the composite pipeline can be rebuilt when the surface format
    // changes (SDR sRGB ↔ HDR Rgba16Float on the HDR toggle).
    comp_module: wgpu::ShaderModule,
    // per-pass uniform buffers (fixed count; only the first N used each resize)
    down_ubs: Vec<wgpu::Buffer>,
    up_ubs: Vec<wgpu::Buffer>,
    comp_ub: wgpu::Buffer,
    targets: Option<Targets>,
    // MSAA sample count for the scene pass (1 = off). Changing it rebuilds the
    // targets (so the multisampled color is recreated/dropped).
    sample_count: u32,
    // --- SSAO (depth-prepass ambient occlusion) ---
    ao_pipeline: wgpu::RenderPipeline,   // reads prepass depth → raw AO (R8)
    blur_pipeline: wgpu::RenderPipeline, // box-blur the raw AO → blurred AO (R8)
    ao_bgl: wgpu::BindGroupLayout,       // {uniform, depth_tex, nearest sampler}
    blur_bgl: wgpu::BindGroupLayout,     // {uniform, nearest sampler, ao_tex}
    ssao_ub: wgpu::Buffer,
    nearest: wgpu::Sampler, // non-filtering, for depth + AO sampling
    // --- SSR (inter-cube reflections, #80 Part A) ---
    ssr_pipeline: wgpu::RenderPipeline, // marches depth + HDR → reflection buffer
    ssr_bgl: wgpu::BindGroupLayout,     // {uniform, depth, nearest, hdr, linear}
    ssr_ub: wgpu::Buffer,
    // --- SSGI (#152 Tier 2): same bind layout as SSR, a different marcher ---
    ssgi_pipeline: wgpu::RenderPipeline,
    ssgi_bgl: wgpu::BindGroupLayout,
    ssgi_ub: wgpu::Buffer,
    // --- Temporal accumulator (#200 Tier 4½ p3): reproject + clamp + beat
    //     relax, MRT (buffer + history). {uniform, depth, raw, history, samp}. ---
    temporal_pipeline: wgpu::RenderPipeline,
    temporal_bgl: wgpu::BindGroupLayout,
    temporal_ub: wgpu::Buffer,
    // --- RT denoiser (#200 Tier 4½ part 2): edge-aware à-trous over the RT
    //     reflection / GI buffers. Two uniform buffers (one per à-trous step,
    //     so a single-encoder ping-pong doesn't clobber the step). ---
    denoise_pipeline: wgpu::RenderPipeline,
    denoise_bgl: wgpu::BindGroupLayout, // {uniform, depth, src}
    denoise_ub: [wgpu::Buffer; 2],
    // --- Neural denoiser (#200 Tier 5a): kernel-predicting filter (classical
    //     bilateral base × seeded-MLP modulation). Reuses `denoise_bgl` (same
    //     {uniform, depth, src}) + the shared `denoise_scratch`; its own pipeline
    //     (different shader) + two wider (NdU) uniform buffers for the two steps.
    ndenoise_pipeline: wgpu::RenderPipeline,
    ndenoise_ub: [wgpu::Buffer; 2],
    // 1×1 black HDR texture standing in for the lazily-allocated SSR/SSGI buffers
    // in the composite bind group while those effects are off (#174 T2).
    fx_dummy: wgpu::TextureView,
}

const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// SSAO uniform (matches `SsaoU` in ssao.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SsaoParams {
    pub proj: [[f32; 4]; 4],
    pub inv_proj: [[f32; 4]; 4],
    pub params: [f32; 4], // radius, intensity (composite-side), bias, _
    pub texel: [f32; 4],  // 1/w, 1/h, w, h
}

/// SSR uniform (#80 Part A; matches `SsrU` in ssr.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SsrParams {
    pub proj: [[f32; 4]; 4],
    pub inv_proj: [[f32; 4]; 4],
    pub mat: [f32; 4],   // metallic, roughness, material_type, _
    pub ssr: [f32; 4],   // intensity, max_roughness, thickness, _
    pub perf: [f32; 4],  // max_steps, stride, _, _
    pub texel: [f32; 4], // 1/w, 1/h, w, h
}

/// SSGI uniform (#152 Tier 2; matches `SsgiU` in ssgi.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SsgiParams {
    pub proj: [[f32; 4]; 4],
    pub inv_proj: [[f32; 4]; 4],
    pub params: [f32; 4], // intensity, radius, max_steps, rays
    pub extra: [f32; 4],  // thickness, frame_seed, _, _
    pub texel: [f32; 4],  // 1/w, 1/h, w, h
}

/// Which RT light buffer the temporal accumulator refines (#200 Tier 4½ p3).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RtBuffer {
    /// The reflection buffer (the SSR slot).
    Reflection,
    /// The GI buffer (the SSGI slot).
    Gi,
}

/// Temporal-accumulator uniform (matches `TmpU` in rt_temporal.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TmpU {
    inv_view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    params: [f32; 4],  // feedback, beat_relax_factor, _, _
    params2: [f32; 4], // variance_on, max_accum, clamp_gamma, history_valid
}

/// Which RT light buffer `Post::denoise` filters (#200 Tier 4½ part 2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DenoiseTarget {
    /// The reflection buffer (the SSR slot; premultiplied colour + weight).
    Reflection,
    /// The GI buffer (the SSGI slot; radiance).
    Gi,
}

/// À-trous denoiser uniform (matches `DnU` in rt_denoise.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DnU {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4],  // texel.x, texel.y, step, strength
    params2: [f32; 4], // pos_sigma (rel), lum_sigma, _, _
}

/// Neural-denoiser uniform (#200 Tier 5a; matches `NdU` in rt_ndenoise.wgsl).
/// The classical `DnU` fields plus `net` (network influence) in `params2.z` and
/// the network identity (seed, omega) in `net`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct NdU {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4],  // texel.x, texel.y, step, strength
    params2: [f32; 4], // pos_sigma (rel), lum_sigma, net (network influence), _
    net: [f32; 4],     // seed, omega, _, _
}

impl Post {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let bloom = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post-bloom"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let comp = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post-composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });

        let sample_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post-sample-layout"),
            entries: &[
                tex_entry(0),
                samp_entry(1),
                uniform_entry(2),
            ],
        });
        let comp_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post-comp-layout"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                samp_entry(2),
                uniform_entry(3),
                tex_entry(4), // AO (blurred)
                tex_entry(5), // SSR reflection buffer (#80 A)
                tex_entry(6), // SSGI buffer (#152 Tier 2)
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let down_pipeline = make_pipeline(
            device, "post-down", &bloom, "fs_down", &sample_layout, HDR_FORMAT, None,
        );
        // Additive blend: each upsample adds its (already tent-blurred) contribution.
        let add_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let up_pipeline = make_pipeline(
            device, "post-up", &bloom, "fs_up", &sample_layout, HDR_FORMAT, Some(add_blend),
        );
        let comp_pipeline = make_pipeline(
            device, "post-composite", &comp, "fs_composite", &comp_layout, surface_format, None,
        );

        // --- SSAO pipelines ---
        let ssao = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssao"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ssao.wgsl").into()),
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ssao-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default() // nearest, non-filtering
        });
        let depth_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let nonfilter_samp = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
        };
        let ao_tex_entry = wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let ao_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao-ao-layout"),
            entries: &[uniform_entry(0), depth_entry, nonfilter_samp],
        });
        // The blur is depth-aware (#174 T1: bilateral weights), so it binds the
        // prepass depth (binding 1) alongside the raw AO.
        let blur_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao-blur-layout"),
            entries: &[uniform_entry(0), depth_entry, nonfilter_samp, ao_tex_entry],
        });
        let ao_pipeline =
            make_pipeline(device, "ssao", &ssao, "fs_ao", &ao_bgl, AO_FORMAT, None);
        let blur_pipeline =
            make_pipeline(device, "ssao-blur", &ssao, "fs_blur", &blur_bgl, AO_FORMAT, None);
        let ssao_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssao-ub"),
            size: std::mem::size_of::<SsaoParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- SSR pipeline (#80 Part A): {uniform, depth, nearest, hdr, linear} ---
        let ssr = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssr"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ssr.wgsl").into()),
        });
        let ssr_depth_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let ssr_nearest = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
        };
        let ssr_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssr-layout"),
            entries: &[uniform_entry(0), ssr_depth_entry, ssr_nearest, tex_entry(3), samp_entry(4)],
        });
        let ssr_pipeline = make_pipeline(device, "ssr", &ssr, "fs_ssr", &ssr_bgl, HDR_FORMAT, None);
        let ssr_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssr-ub"),
            size: std::mem::size_of::<SsrParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- SSGI pipeline (#152 Tier 2): same {uniform, depth, nearest, hdr, linear}
        //     bind layout as SSR, a hemisphere-gather marcher (ssgi.wgsl). ---
        let ssgi = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssgi"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ssgi.wgsl").into()),
        });
        let ssgi_depth_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let ssgi_nearest = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
        };
        let ssgi_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssgi-layout"),
            entries: &[uniform_entry(0), ssgi_depth_entry, ssgi_nearest, tex_entry(3), samp_entry(4)],
        });
        let ssgi_pipeline = make_pipeline(device, "ssgi", &ssgi, "fs_ssgi", &ssgi_bgl, HDR_FORMAT, None);
        let ssgi_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssgi-ub"),
            size: std::mem::size_of::<SsgiParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Temporal accumulator (#200 Tier 4½ p3): {uniform, depth, raw,
        //     history, sampler} → MRT [buffer, history]. Built inline (not
        //     make_pipeline) for the two color targets. ---
        let temporal = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-temporal"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_temporal.wgsl").into()),
        });
        let temporal_depth_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        // --- RT denoiser (#200 Tier 4½ part 2): {uniform, depth, src} → à-trous
        //     edge-aware bilateral. Uses textureLoad, so no sampler binding. ---
        let denoise = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-denoise"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_denoise.wgsl").into()),
        });
        let denoise_depth_entry = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let temporal_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-temporal-layout"),
            // uniform(0), depth(1), raw(2), colour history(3), linear sampler(4),
            // moments history(5) — part 4's variance state.
            entries: &[
                uniform_entry(0),
                temporal_depth_entry,
                tex_entry(2),
                tex_entry(3),
                samp_entry(4),
                tex_entry(5),
            ],
        });
        let temporal_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-temporal-pl"),
            bind_group_layouts: &[Some(&temporal_bgl)],
            immediate_size: 0,
        });
        let temporal_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rt-temporal-pipeline"),
            layout: Some(&temporal_pl),
            vertex: wgpu::VertexState {
                module: &temporal,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &temporal,
                entry_point: Some("fs_main"),
                targets: &[
                    // loc0 = SSR/SSGI buffer, loc1 = colour history, loc2 = moments.
                    Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let temporal_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-temporal-ub"),
            size: std::mem::size_of::<TmpU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let denoise_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-denoise-layout"),
            entries: &[uniform_entry(0), denoise_depth_entry, tex_entry(2)],
        });
        let denoise_pipeline =
            make_pipeline(device, "rt-denoise", &denoise, "fs_main", &denoise_bgl, HDR_FORMAT, None);
        let denoise_ub = [
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rt-denoise-ub-0"),
                size: std::mem::size_of::<DnU>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rt-denoise-ub-1"),
                size: std::mem::size_of::<DnU>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        ];

        // --- Neural denoiser (#200 Tier 5a): same {uniform, depth, src} layout
        //     as the classical à-trous (reuses `denoise_bgl`), a different shader
        //     + a wider uniform (NdU). ---
        let ndenoise = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-ndenoise"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_ndenoise.wgsl").into()),
        });
        let ndenoise_pipeline =
            make_pipeline(device, "rt-ndenoise", &ndenoise, "fs_main", &denoise_bgl, HDR_FORMAT, None);
        let ndenoise_ub = [
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rt-ndenoise-ub-0"),
                size: std::mem::size_of::<NdU>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rt-ndenoise-ub-1"),
                size: std::mem::size_of::<NdU>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        ];

        let mkbuf = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let down_ubs = (0..MAX_BLOOM_MIPS)
            .map(|i| mkbuf(&format!("post-down-ub-{i}"), std::mem::size_of::<DownU>() as u64))
            .collect();
        let up_ubs = (0..MAX_BLOOM_MIPS)
            .map(|i| mkbuf(&format!("post-up-ub-{i}"), std::mem::size_of::<UpU>() as u64))
            .collect();
        let comp_ub = mkbuf("post-comp-ub", std::mem::size_of::<CompU>() as u64);

        let fx_dummy = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("post-fx-dummy"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        Post {
            sampler,
            sample_layout,
            comp_layout,
            down_pipeline,
            up_pipeline,
            comp_pipeline,
            comp_module: comp,
            down_ubs,
            up_ubs,
            comp_ub,
            targets: None,
            sample_count: 1,
            ao_pipeline,
            blur_pipeline,
            ao_bgl,
            blur_bgl,
            ssao_ub,
            nearest,
            ssr_pipeline,
            ssr_bgl,
            ssr_ub,
            ssgi_pipeline,
            ssgi_bgl,
            ssgi_ub,
            temporal_pipeline,
            temporal_bgl,
            temporal_ub,
            denoise_pipeline,
            denoise_bgl,
            denoise_ub,
            ndenoise_pipeline,
            ndenoise_ub,
            fx_dummy,
        }
    }

    /// Run the SSAO + blur passes from the single-sample prepass `depth_view`,
    /// leaving the blurred AO in the target the composite samples. Only called
    /// when SSAO is enabled.
    pub fn compute_ao(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth_view: &wgpu::TextureView,
        depth_key: u64,
        size: (u32, u32),
        ssao: &SsaoParams,
    ) {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let Some(t) = self.targets.as_mut() else { return };
        queue.write_buffer(&self.ssao_ub, 0, bytemuck::bytes_of(ssao));

        // Bind groups cached across frames (#174 T2 — they were recreated every
        // frame); every input is stable until the targets or the depth source
        // change, both captured by the targets rebuild + the caller's depth key.
        if t.ao_bind.as_ref().map(|(k, ..)| *k != depth_key).unwrap_or(true) {
            let ao_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao-ao-bind"),
                layout: &self.ao_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ssao_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                ],
            });
            let blur_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao-blur-bind"),
                layout: &self.blur_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ssao_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.ao_raw_view) },
                ],
            });
            t.ao_bind = Some((depth_key, ao_bind, blur_bind));
        }
        let (_, ao_bind, blur_bind) = t.ao_bind.as_ref().unwrap();
        {
            let mut rp = color_pass(encoder, "ssao-pass", &t.ao_raw_view, false);
            rp.set_pipeline(&self.ao_pipeline);
            rp.set_bind_group(0, ao_bind, &[]);
            rp.draw(0..3, 0..1);
        }
        {
            let mut rp = color_pass(encoder, "ssao-blur-pass", &t.ao_blur_view, false);
            rp.set_pipeline(&self.blur_pipeline);
            rp.set_bind_group(0, blur_bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    /// The raw-AO target for an EXTERNAL AO writer (#195 Tier 3): the
    /// hardware-RT AO pass renders into the same texture GTAO would, then
    /// `blur_ao` runs the existing blur — everything downstream (the composite
    /// AO-multiply, the specular occlusion) is untouched. `None` only at a
    /// zero size.
    pub fn ao_raw_target(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> Option<&wgpu::TextureView> {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        self.targets.as_ref().map(|t| &t.ao_raw_view)
    }

    /// Blur the raw AO into the blurred-AO target the composite + cube shader
    /// read — the second half of `compute_ao`, for the RT-AO route (#195
    /// Tier 3), sharing its cached bind groups. `ssao` is still written (the
    /// blur reads the texel sizes from the same uniform).
    #[allow(clippy::too_many_arguments)]
    pub fn blur_ao(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth_view: &wgpu::TextureView,
        depth_key: u64,
        ssao: &SsaoParams,
    ) {
        let Some(t) = self.targets.as_mut() else { return };
        queue.write_buffer(&self.ssao_ub, 0, bytemuck::bytes_of(ssao));
        if t.ao_bind.as_ref().map(|(k, ..)| *k != depth_key).unwrap_or(true) {
            let ao_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao-ao-bind"),
                layout: &self.ao_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ssao_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                ],
            });
            let blur_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao-blur-bind"),
                layout: &self.blur_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ssao_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.ao_raw_view) },
                ],
            });
            t.ao_bind = Some((depth_key, ao_bind, blur_bind));
        }
        let (_, _, blur_bind) = t.ao_bind.as_ref().unwrap();
        let mut rp = color_pass(encoder, "ssao-blur-pass", &t.ao_blur_view, false);
        rp.set_pipeline(&self.blur_pipeline);
        rp.set_bind_group(0, blur_bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Ensure the SSR/reflection buffer exists — and the composite bind group
    /// points at it — WITHOUT running the SSR march: the hardware-RT reflection
    /// pass (#195 Tier 2) renders into this same target, riding the composite's
    /// existing confidence-weighted blend. `None` only at a zero size.
    pub fn ensure_ssr_target(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> Option<&wgpu::TextureView> {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let t = self.targets.as_mut()?;
        if t.ssr_view.is_none() {
            t.ssr_view = Some(make_fx_view(device, size, "ssr-reflection"));
            t.ssr_bind = None;
            let cb = make_comp_bind(
                device, &self.comp_layout, &self.sampler, &self.comp_ub, &self.fx_dummy, t,
            );
            t.comp_bind = cb;
        }
        t.ssr_view.as_ref()
    }

    /// Run the SSR pass (#80 A): march the single-sample `depth_view` + the
    /// resolved HDR scene colour into the reflection buffer the composite adds.
    /// Only called when SSR is enabled.
    pub fn compute_ssr(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth_view: &wgpu::TextureView,
        depth_key: u64,
        size: (u32, u32),
        ssr: &SsrParams,
    ) {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let Some(t) = self.targets.as_mut() else { return };
        queue.write_buffer(&self.ssr_ub, 0, bytemuck::bytes_of(ssr));
        // Lazily allocate the full-res reflection target on first use (#174 T2)
        // and re-point the composite bind group at it (was a dummy).
        if t.ssr_view.is_none() {
            t.ssr_view = Some(make_fx_view(device, size, "ssr-reflection"));
            t.ssr_bind = None;
            let cb = make_comp_bind(
                device, &self.comp_layout, &self.sampler, &self.comp_ub, &self.fx_dummy, t,
            );
            t.comp_bind = cb;
        }
        if t.ssr_bind.as_ref().map(|(k, _)| *k != depth_key).unwrap_or(true) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssr-bind"),
                layout: &self.ssr_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ssr_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.hdr_view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            t.ssr_bind = Some((depth_key, bind));
        }
        let mut rp = color_pass(encoder, "ssr-pass", t.ssr_view.as_ref().unwrap(), false);
        rp.set_pipeline(&self.ssr_pipeline);
        rp.set_bind_group(0, &t.ssr_bind.as_ref().unwrap().1, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Ensure the SSGI buffer exists — and the composite bind group points at
    /// it — WITHOUT running the SSGI march: the hardware-RT GI pass (#195
    /// Tier 4) gathers into this same target, riding the composite's existing
    /// additive blend. `None` only at a zero size.
    pub fn ensure_ssgi_target(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> Option<&wgpu::TextureView> {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let t = self.targets.as_mut()?;
        if t.ssgi_view.is_none() {
            t.ssgi_view = Some(make_fx_view(device, size, "ssgi"));
            t.ssgi_bind = None;
            let cb = make_comp_bind(
                device, &self.comp_layout, &self.sampler, &self.comp_ub, &self.fx_dummy, t,
            );
            t.comp_bind = cb;
        }
        t.ssgi_view.as_ref()
    }

    /// Run the SSGI pass (#152 Tier 2): gather one diffuse bounce from the depth
    /// prepass + resolved HDR into the GI buffer the composite adds. Only called
    /// when SSGI is enabled (and the prepass ran).
    pub fn compute_ssgi(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        depth_view: &wgpu::TextureView,
        depth_key: u64,
        size: (u32, u32),
        ssgi: &SsgiParams,
    ) {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let Some(t) = self.targets.as_mut() else { return };
        queue.write_buffer(&self.ssgi_ub, 0, bytemuck::bytes_of(ssgi));
        // Same lazy-allocation + bind-caching scheme as SSR (#174 T2).
        if t.ssgi_view.is_none() {
            t.ssgi_view = Some(make_fx_view(device, size, "ssgi"));
            t.ssgi_bind = None;
            let cb = make_comp_bind(
                device, &self.comp_layout, &self.sampler, &self.comp_ub, &self.fx_dummy, t,
            );
            t.comp_bind = cb;
        }
        if t.ssgi_bind.as_ref().map(|(k, _)| *k != depth_key).unwrap_or(true) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssgi-bind"),
                layout: &self.ssgi_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ssgi_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t.hdr_view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            t.ssgi_bind = Some((depth_key, bind));
        }
        let mut rp = color_pass(encoder, "ssgi-pass", t.ssgi_view.as_ref().unwrap(), false);
        rp.set_pipeline(&self.ssgi_pipeline);
        rp.set_bind_group(0, &t.ssgi_bind.as_ref().unwrap().1, &[]);
        rp.draw(0..3, 0..1);
    }

    /// The shared **raw** RT buffer the reflection/GI pass writes into when the
    /// temporal accumulator is on (#200 Tier 4½ p3), instead of the SSR/SSGI
    /// view — so the accumulator can read a separate current frame and write the
    /// composite's view without a read/write hazard. Lazily allocated.
    pub fn ensure_rt_raw(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> Option<&wgpu::TextureView> {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let t = self.targets.as_mut()?;
        if t.rt_raw.is_none() {
            t.rt_raw = Some(make_fx_view(device, size, "rt-raw"));
        }
        t.rt_raw.as_ref()
    }

    /// Temporally accumulate the RT reflection/GI buffer (#200 Tier 4½ p3/p4):
    /// reproject the ping-pong history by camera motion, clamp it, beat-relax the
    /// history weight, and MRT the result into the SSR/SSGI view (composite reads)
    /// **plus** the new colour history **plus** the new moments (part 4). The RT
    /// pass must have written `rt_raw` this frame (`ensure_rt_raw`). First frame
    /// per effect seeds the history (`history_valid = 0` → passthrough).
    ///
    /// Part 4 (`variance_on`): the shader switches to history-length-adaptive
    /// blending (`feedback` becomes the ceiling, `max_accum` the frame cap) + a
    /// luminance-variance clamp of width `clamp_gamma`·σ. `variance_on = false`
    /// reproduces part 3 exactly (the moments texture is written but ignored).
    #[allow(clippy::too_many_arguments)]
    pub fn temporal(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        which: RtBuffer,
        depth_view: &wgpu::TextureView,
        inv_view_proj: [[f32; 4]; 4],
        prev_view_proj: [[f32; 4]; 4],
        feedback: f32,
        beat_relax_factor: f32,
        variance_on: bool,
        max_accum: f32,
        clamp_gamma: f32,
        size: (u32, u32),
    ) {
        let Some(t) = self.targets.as_mut() else { return };
        if t.rt_raw.is_none() {
            return; // the RT pass didn't write the raw buffer this frame
        }
        let has_buf = match which {
            RtBuffer::Reflection => t.ssr_view.is_some(),
            RtBuffer::Gi => t.ssgi_view.is_some(),
        };
        if !has_buf {
            return;
        }
        // Ensure the effect's two ping-pong colour-history + moments views.
        {
            let (hist, mom) = match which {
                RtBuffer::Reflection => (&mut t.refl_hist, &mut t.refl_mom),
                RtBuffer::Gi => (&mut t.gi_hist, &mut t.gi_mom),
            };
            for slot in hist.iter_mut() {
                if slot.is_none() {
                    *slot = Some(make_fx_view(device, size, "rt-hist"));
                }
            }
            for slot in mom.iter_mut() {
                if slot.is_none() {
                    *slot = Some(make_fx_view(device, size, "rt-mom"));
                }
            }
        }
        // Read + advance the ping-pong parity + validity.
        let (prev_idx, cur_idx, valid_now);
        {
            let (valid, parity) = match which {
                RtBuffer::Reflection => (&mut t.refl_hist_valid, &mut t.refl_parity),
                RtBuffer::Gi => (&mut t.gi_hist_valid, &mut t.gi_parity),
            };
            prev_idx = (*parity & 1) as usize;
            cur_idx = 1 - prev_idx;
            valid_now = *valid;
            *valid = true;
            *parity = cur_idx as u32;
        }
        // Part 3 fixed-feedback path forces feedback→0 on the first frame; the
        // variance path instead reads `history_valid` (params2.w) to seed.
        let eff_feedback = if valid_now { feedback } else { 0.0 };
        let u = TmpU {
            inv_view_proj,
            prev_view_proj,
            params: [eff_feedback, beat_relax_factor, 0.0, 0.0],
            params2: [
                if variance_on { 1.0 } else { 0.0 },
                max_accum.max(1.0),
                clamp_gamma.max(0.0),
                if valid_now { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&self.temporal_ub, 0, bytemuck::bytes_of(&u));
        let raw = t.rt_raw.as_ref().unwrap();
        let (buf, hist_ref, mom_ref) = match which {
            RtBuffer::Reflection => (t.ssr_view.as_ref().unwrap(), &t.refl_hist, &t.refl_mom),
            RtBuffer::Gi => (t.ssgi_view.as_ref().unwrap(), &t.gi_hist, &t.gi_mom),
        };
        let prev = hist_ref[prev_idx].as_ref().unwrap();
        let cur = hist_ref[cur_idx].as_ref().unwrap();
        let prev_mom = mom_ref[prev_idx].as_ref().unwrap();
        let cur_mom = mom_ref[cur_idx].as_ref().unwrap();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-temporal-bind"),
            layout: &self.temporal_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.temporal_ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(raw) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(prev) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(prev_mom) },
            ],
        });
        let clear = wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
        };
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-temporal-pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment { view: buf, depth_slice: None, resolve_target: None, ops: clear }),
                Some(wgpu::RenderPassColorAttachment { view: cur, depth_slice: None, resolve_target: None, ops: clear }),
                Some(wgpu::RenderPassColorAttachment { view: cur_mom, depth_slice: None, resolve_target: None, ops: clear }),
            ],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.temporal_pipeline);
        rp.set_bind_group(0, &bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Edge-aware à-trous denoise of an RT light buffer (#200 Tier 4½ part 2),
    /// **in place**: two iterations (step 1 then 2) ping-pong the reflection /
    /// GI buffer through a scratch and land back in the source, so the composite
    /// reads the same view unchanged. Called only for RT-written buffers (SSR /
    /// SSGI screen-space stay untouched); `strength` 0 is a no-op the caller
    /// already skips. `pos_sigma`/`lum_sigma` are the edge-stop widths.
    #[allow(clippy::too_many_arguments)]
    pub fn denoise(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        which: DenoiseTarget,
        depth_view: &wgpu::TextureView,
        inv_view_proj: [[f32; 4]; 4],
        cam_pos: [f32; 4],
        size: (u32, u32),
        strength: f32,
        pos_sigma: f32,
        lum_sigma: f32,
    ) {
        if strength <= 0.0 {
            return;
        }
        let Some(t) = self.targets.as_mut() else { return };
        // The source buffer must already exist (the RT pass wrote it this frame).
        let has_src = match which {
            DenoiseTarget::Reflection => t.ssr_view.is_some(),
            DenoiseTarget::Gi => t.ssgi_view.is_some(),
        };
        if !has_src {
            return;
        }
        if t.denoise_scratch.is_none() {
            t.denoise_scratch = Some(make_fx_view(device, size, "rt-denoise-scratch"));
        }
        let source = match which {
            DenoiseTarget::Reflection => t.ssr_view.as_ref().unwrap(),
            DenoiseTarget::Gi => t.ssgi_view.as_ref().unwrap(),
        };
        let scratch = t.denoise_scratch.as_ref().unwrap();
        let texel = [1.0 / size.0.max(1) as f32, 1.0 / size.1.max(1) as f32];
        // Two à-trous steps → two uniform buffers (a single one would be
        // clobbered: both passes in this encoder would read the last write).
        for (i, step) in [1.0f32, 2.0].into_iter().enumerate() {
            let u = DnU {
                inv_view_proj,
                cam_pos,
                params: [texel[0], texel[1], step, strength],
                params2: [pos_sigma.max(1e-4), lum_sigma.max(1e-4), 0.0, 0.0],
            };
            queue.write_buffer(&self.denoise_ub[i], 0, bytemuck::bytes_of(&u));
        }
        // iter 0: source → scratch (step 1); iter 1: scratch → source (step 2).
        let iters: [(&wgpu::TextureView, &wgpu::TextureView, usize); 2] =
            [(source, scratch, 0), (scratch, source, 1)];
        for (src, dst, i) in iters {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rt-denoise-bind"),
                layout: &self.denoise_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.denoise_ub[i].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(src) },
                ],
            });
            let mut rp = color_pass(encoder, "rt-denoise-pass", dst, false);
            rp.set_pipeline(&self.denoise_pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    /// Neural denoiser (#200 Tier 5a) — the kernel-predicting sibling of
    /// `denoise`. Identical two-step à-trous ping-pong through the shared
    /// `denoise_scratch`; the only difference is the shader, which multiplies the
    /// classical bilateral tap weight by a bounded seeded-MLP modulation. `net`
    /// is the network influence (0 → reproduces `denoise` exactly); `seed` /
    /// `omega` are the network identity. Same premultiplied operator + per-step
    /// `strength` blend as the classical pass.
    #[allow(clippy::too_many_arguments)]
    pub fn neural_denoise(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        which: DenoiseTarget,
        depth_view: &wgpu::TextureView,
        inv_view_proj: [[f32; 4]; 4],
        cam_pos: [f32; 4],
        size: (u32, u32),
        strength: f32,
        pos_sigma: f32,
        lum_sigma: f32,
        net: f32,
        seed: f32,
        omega: f32,
    ) {
        if strength <= 0.0 {
            return;
        }
        let Some(t) = self.targets.as_mut() else { return };
        let has_src = match which {
            DenoiseTarget::Reflection => t.ssr_view.is_some(),
            DenoiseTarget::Gi => t.ssgi_view.is_some(),
        };
        if !has_src {
            return;
        }
        if t.denoise_scratch.is_none() {
            t.denoise_scratch = Some(make_fx_view(device, size, "rt-denoise-scratch"));
        }
        let source = match which {
            DenoiseTarget::Reflection => t.ssr_view.as_ref().unwrap(),
            DenoiseTarget::Gi => t.ssgi_view.as_ref().unwrap(),
        };
        let scratch = t.denoise_scratch.as_ref().unwrap();
        let texel = [1.0 / size.0.max(1) as f32, 1.0 / size.1.max(1) as f32];
        for (i, step) in [1.0f32, 2.0].into_iter().enumerate() {
            let u = NdU {
                inv_view_proj,
                cam_pos,
                params: [texel[0], texel[1], step, strength],
                params2: [pos_sigma.max(1e-4), lum_sigma.max(1e-4), net.max(0.0), 0.0],
                net: [seed.max(0.0), omega, 0.0, 0.0],
            };
            queue.write_buffer(&self.ndenoise_ub[i], 0, bytemuck::bytes_of(&u));
        }
        let iters: [(&wgpu::TextureView, &wgpu::TextureView, usize); 2] =
            [(source, scratch, 0), (scratch, source, 1)];
        for (src, dst, i) in iters {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rt-ndenoise-bind"),
                layout: &self.denoise_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ndenoise_ub[i].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(src) },
                ],
            });
            let mut rp = color_pass(encoder, "rt-ndenoise-pass", dst, false);
            rp.set_pipeline(&self.ndenoise_pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    /// Set the MSAA sample count for the scene pass; rebuilds targets on change.
    pub fn set_sample_count(&mut self, n: u32) {
        if n != self.sample_count {
            self.sample_count = n;
            self.targets = None; // force rebuild with the new sample count
        }
    }

    /// The resolved single-sample HDR scene buffer (after the scene pass), so an
    /// external additive pass (e.g. VXGI, #152 Tier 3) can add light into it before
    /// bloom/composite. `None` until the targets are built.
    pub fn hdr_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.hdr_view)
    }
    /// The HDR texture itself (#182 T3b: the refractive liquid copies it).
    pub fn hdr_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t._hdr)
    }

    /// The blurred AO buffer (valid after `compute_ao` ran this frame) — bound by
    /// the cube pipeline for specular occlusion (#174 T3).
    pub fn ao_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.ao_blur_view)
    }

    /// The scene's color attachment for this frame: `(render_into, resolve)`.
    /// With MSAA on, render into the multisampled view and resolve into the
    /// single-sample HDR buffer; with MSAA off, render straight into it.
    pub fn scene_targets(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        let stale = self.targets.as_ref().map(|t| t.size != size).unwrap_or(true);
        if stale && size.0 > 0 && size.1 > 0 {
            self.targets = Some(self.build_targets(device, size));
        }
        let t = self.targets.as_ref().unwrap();
        match &t.msaa_view {
            Some(m) => (m, Some(&t.hdr_view)),
            None => (&t.hdr_view, None),
        }
    }

    /// Rebuild the final composite pipeline for a new surface format. Called when
    /// the HDR toggle swaps the swapchain between an sRGB 8-bit surface and an
    /// `Rgba16Float` HDR surface — only this last pass targets the surface, so the
    /// bloom chain (which targets `HDR_FORMAT`) is untouched.
    pub fn set_surface_format(&mut self, device: &wgpu::Device, surface_format: wgpu::TextureFormat) {
        self.comp_pipeline = make_pipeline(
            device,
            "post-composite",
            &self.comp_module,
            "fs_composite",
            &self.comp_layout,
            surface_format,
            None,
        );
    }


    fn build_targets(&self, device: &wgpu::Device, size: (u32, u32)) -> Targets {
        let (w, h) = size;
        let hdr = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr-scene"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            // COPY_SRC: the refractive liquid (#182 T3b) snapshots the resolved
            // scene before bending it through the water.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let hdr_view = hdr.create_view(&wgpu::TextureViewDescriptor::default());

        // Multisampled scene color (when MSAA is on). Render-attachment only — it
        // is resolved into `hdr` and never sampled directly.
        let msaa_view = if self.sample_count > 1 {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("hdr-scene-msaa"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: self.sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            Some(t.create_view(&wgpu::TextureViewDescriptor::default()))
        } else {
            None
        };

        // SSAO targets (full-res R8). Always created so the composite bind group
        // has a valid AO texture even when SSAO is off (it just isn't sampled).
        let mk_ao = |label: &str| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: AO_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let ao_raw_view = mk_ao("ssao-raw");
        let ao_blur_view = mk_ao("ssao-blur");

        // SSR + SSGI buffers are allocated lazily on first use (#174 T2 — two
        // full-res Rgba16Float allocations, ~130 MB at 4K, idled while the effects
        // were off). The composite bind group points at the 1×1 `fx_dummy` until
        // then (it only samples them behind the enabled flags anyway).

        // Bloom levels: keep halving from half-res until small or capped.
        let mut n = 0usize;
        let mut s = w.min(h);
        while s > 8 && n < MAX_BLOOM_MIPS {
            s /= 2;
            n += 1;
        }
        let n = n.max(1);

        let mut bloom_texs = Vec::with_capacity(n);
        let mut bloom_views = Vec::with_capacity(n);
        let mut bloom_sizes = Vec::with_capacity(n);
        for i in 0..n {
            let bw = (w >> (i + 1)).max(1);
            let bh = (h >> (i + 1)).max(1);
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("bloom-mip"),
                size: wgpu::Extent3d { width: bw, height: bh, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            bloom_views.push(t.create_view(&wgpu::TextureViewDescriptor::default()));
            bloom_texs.push(t);
            bloom_sizes.push((bw, bh));
        }
        drop(bloom_texs); // views keep the textures alive via wgpu refcount

        // Down binds: level 0 reads hdr, level i>0 reads bloom[i-1].
        let down_binds = (0..n)
            .map(|i| {
                let src = if i == 0 { &hdr_view } else { &bloom_views[i - 1] };
                self.sample_bind(device, src, &self.down_ubs[i])
            })
            .collect();
        // Up binds: pass j upsamples bloom[n-1-j] onto bloom[n-2-j]; reads bloom[i].
        let up_binds = (0..n.saturating_sub(1))
            .map(|j| {
                let src_level = n - 1 - j;
                self.sample_bind(device, &bloom_views[src_level], &self.up_ubs[j])
            })
            .collect();

        let comp_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post-comp-bind"),
            layout: &self.comp_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&bloom_views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: self.comp_ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&ao_blur_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.fx_dummy) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.fx_dummy) },
            ],
        });

        Targets {
            size,
            _hdr: hdr,
            hdr_view,
            msaa_view,
            ao_raw_view,
            ao_blur_view,
            ssr_view: None,
            ssgi_view: None,
            rt_raw: None,
            refl_hist: [None, None],
            gi_hist: [None, None],
            refl_mom: [None, None],
            gi_mom: [None, None],
            refl_hist_valid: false,
            gi_hist_valid: false,
            refl_parity: 0,
            gi_parity: 0,
            denoise_scratch: None,
            ao_bind: None,
            ssr_bind: None,
            ssgi_bind: None,
            bloom_views,
            bloom_sizes,
            down_binds,
            up_binds,
            comp_bind,
        }
    }

    fn sample_bind(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        ub: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post-sample-bind"),
            layout: &self.sample_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
            ],
        })
    }

    /// Bloom + tonemap composite: reads the scene HDR buffer (already rendered by
    /// the caller into `hdr_view`) and writes the final image to `surface_view`.
    pub fn run(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        p: &PostParams,
        ssr_on: bool,
        ssgi_on: bool,
    ) {
        let Some(t) = self.targets.as_ref() else { return };
        let n = t.bloom_sizes.len();
        let (fw, fh) = t.size;

        // Bloom is purely additive and the composite multiplies it by
        // `bloom_intensity`, so when that's zero the entire down/up chain (up to
        // ~10 passes) contributes nothing — skip it. The composite still reads
        // `bloom_views[0]` but multiplies by 0, so the output is identical.
        let do_bloom = p.bloom_intensity > 0.0;

        // Per-frame uniforms (texel sizes from the current targets; live params).
        let knee = (p.bloom_threshold * 0.5).max(1e-4);
        if do_bloom {
            for i in 0..n {
                let src = if i == 0 { (fw, fh) } else { t.bloom_sizes[i - 1] };
                queue.write_buffer(
                    &self.down_ubs[i],
                    0,
                    bytemuck::bytes_of(&DownU {
                        texel: [1.0 / src.0 as f32, 1.0 / src.1 as f32],
                        threshold: p.bloom_threshold,
                        knee,
                        prefilter: if i == 0 { 1.0 } else { 0.0 },
                        exposure: p.exposure,
                        _p: [0.0; 2],
                    }),
                );
            }
            for j in 0..n.saturating_sub(1) {
                let src_level = n - 1 - j;
                let s = t.bloom_sizes[src_level];
                queue.write_buffer(
                    &self.up_ubs[j],
                    0,
                    bytemuck::bytes_of(&UpU {
                        texel: [1.0 / s.0 as f32, 1.0 / s.1 as f32],
                        radius: 1.0,
                        _p: 0.0,
                    }),
                );
            }
        }
        queue.write_buffer(
            &self.comp_ub,
            0,
            bytemuck::bytes_of(&CompU {
                exposure: p.exposure,
                // Normalize bloom energy by the mip count (#174 T3): each up-pass
                // ADDS a full-weight octave, so total bloom brightness grew with
                // the number of mips — which depends on the window/DRS size (< 6
                // below a 512-px min dimension). Scale so the full 6-mip chain is
                // unchanged and smaller chains match its energy instead of
                // dimming as the resolution drops.
                bloom_intensity: p.bloom_intensity
                    * (MAX_BLOOM_MIPS as f32 / t.bloom_sizes.len().max(1) as f32),
                hdr_max: p.hdr_max,
                hdr_knee: p.hdr_knee,
                tonemap: p.tonemap,
                ao_enabled: p.ao_enabled,
                ao_intensity: p.ao_intensity,
                bg_tonemap: p.bg_tonemap,
                ssr_enabled: if ssr_on { 1.0 } else { 0.0 },
                gamut: p.gamut,
                vivid: p.vivid,
                ssgi_enabled: if ssgi_on { 1.0 } else { 0.0 },
                frame: (p.time * 60.0) % 4096.0,
                up_mode: p.up_mode,
                up_sharpen: p.up_sharpen,
                up_seed: p.up_seed,
            }),
        );

        if do_bloom {
            // Downsample chain (hdr → bloom[0] → … → bloom[n-1]).
            for i in 0..n {
                let mut rp = color_pass(encoder, "post-down-pass", &t.bloom_views[i], false);
                rp.set_pipeline(&self.down_pipeline);
                rp.set_bind_group(0, &t.down_binds[i], &[]);
                rp.draw(0..3, 0..1);
            }
            // Upsample chain (bloom[i] additively onto bloom[i-1]).
            for j in 0..n.saturating_sub(1) {
                let dst_level = n - 2 - j;
                let mut rp = color_pass(encoder, "post-up-pass", &t.bloom_views[dst_level], true);
                rp.set_pipeline(&self.up_pipeline);
                rp.set_bind_group(0, &t.up_binds[j], &[]);
                rp.draw(0..3, 0..1);
            }
        }
        // Composite (hdr + bloom[0]) → surface.
        {
            let mut rp = color_pass(encoder, "post-composite-pass", surface_view, false);
            rp.set_pipeline(&self.comp_pipeline);
            rp.set_bind_group(0, &t.comp_bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn samp_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    fs_entry: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_fullscreen"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
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

fn color_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &'a wgpu::TextureView,
    load: bool,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                // Up passes blend additively onto existing content (Load); down +
                // composite overwrite (Clear is cheap and avoids a stale read).
                load: if load {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Full-res `HDR_FORMAT` render+sample target for the lazily-allocated SSR/SSGI
/// buffers (#174 T2).
fn make_fx_view(device: &wgpu::Device, size: (u32, u32), label: &str) -> wgpu::TextureView {
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
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// (Re)build the composite bind group against the current targets, using `dummy`
/// for whichever of the SSR/SSGI buffers hasn't been allocated yet (#174 T2).
fn make_comp_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    comp_ub: &wgpu::Buffer,
    dummy: &wgpu::TextureView,
    t: &Targets,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("post-comp-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.hdr_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.bloom_views[0]) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 3, resource: comp_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&t.ao_blur_view) },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(t.ssr_view.as_ref().unwrap_or(dummy)),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(t.ssgi_view.as_ref().unwrap_or(dummy)),
            },
        ],
    })
}

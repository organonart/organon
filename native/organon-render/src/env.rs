//! Image-based-lighting precompute for the cube field's PBR renderer.
//!
//! Split-sum (Karis/Epic). Every map is an EQUIRECTANGULAR Rgba16Float 2D texture
//! (no cubemaps): the source env, diffuse irradiance, and roughness-mip
//! prefiltered specular. A BRDF LUT completes the split-sum. All maps are produced
//! by RENDER-TO-TEXTURE fullscreen passes (no compute, no storage textures). The
//! source env is a loaded Radiance .hdr (uploaded as f16) OR a procedural sky
//! gradient, fed through the SAME pipeline so the cubes are always lit.

use std::path::Path;

/// Procedural-sky env size (no source image). Equirect is 2:1.
pub const PROC_ENV_W: u32 = 2048;
pub const PROC_ENV_H: u32 = 1024;
/// Cap for a loaded `.hdr`'s equirect resolution. 4096×2048 Rgba16Float ≈ 64 MB
/// (+ mips); the skybox samples this at mip 0, so it sets background sharpness.
pub const MAX_ENV_W: u32 = 4096;
pub const IRR_W: u32 = 64;
pub const IRR_H: u32 = 32;
/// Prefiltered specular (environment reflections). mip 0 = sharp mirror.
pub const PRE_W: u32 = 1024;
pub const PRE_H: u32 = 512;
pub const PREFILTER_MIPS: u32 = 5;
pub const LUT_SIZE: u32 = 256;

const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[derive(Clone, Copy)]
pub struct SkyParams {
    pub top: [f32; 3],
    pub horizon: [f32; 3],
    pub bottom: [f32; 3],
    pub intensity: f32,
}

// Dusky look matching the web app: violet-blue zenith, warm horizon, dark base.
pub const DEFAULT_SKY: SkyParams = SkyParams {
    top: [0.05, 0.09, 0.22],
    horizon: [0.55, 0.50, 0.62],
    bottom: [0.02, 0.03, 0.06],
    intensity: 1.0,
};

/// Physically based atmosphere params (#100). Baked into the env equirect by a
/// Nishita single-scattering pass, then run through the SAME split-sum precompute
/// — so the cubes are lit by the real sky at the real sun angle. The sun direction
/// rides the terrain sun elevation/azimuth (the day cycle).
#[derive(Clone, Copy)]
pub struct AtmosphereParams {
    pub sun_dir: [f32; 3], // unit direction TO the sun
    pub sun_intensity: f32,
    pub turbidity: f32,     // aerosol (Mie) density — haze + sun aureole
    pub mie_g: f32,         // Mie forward-scatter anisotropy
    pub ground_albedo: f32, // ground-bounce ambient lift
    pub rayleigh: f32,      // Rayleigh (blue) strength
    pub exposure: f32,      // overall HDR gain on the baked sky
}

pub enum EnvSource<'a> {
    Procedural(SkyParams),
    Hdr(&'a Path),
    Atmosphere(AtmosphereParams),
}

struct DecodedHdr {
    w: u32,
    h: u32,
    rgba: Vec<f32>,
}

fn load_hdr(path: &Path) -> Result<DecodedHdr, String> {
    use image::ImageReader;
    let img = ImageReader::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .with_guessed_format()
        .map_err(|e| format!("guess format: {e}"))?
        .decode()
        .map_err(|e| format!("decode hdr: {e}"))?;
    let rgba = img.to_rgba32f();
    let (w, h) = rgba.dimensions();
    let raw = rgba.into_raw();
    // Diagnostic: a real HDR has radiance well above 1.0. If this prints a max
    // near 1.0, the file decoded as LDR (and there's nothing to tonemap).
    let max = raw.iter().copied().fold(0.0f32, f32::max);
    eprintln!(
        "[organic-math env] loaded {} ({}x{}), peak radiance = {:.3} ({})",
        path.display(),
        w,
        h,
        max,
        if max > 1.001 { "HDR" } else { "LDR-range" }
    );
    Ok(DecodedHdr { w, h, rgba: raw })
}

/// The precomputed IBL set + both bind groups the cube and skybox passes sample.
pub struct Environment {
    // keep textures alive (views borrow them)
    _env_tex: wgpu::Texture,
    _irradiance: wgpu::Texture,
    _prefilter: wgpu::Texture,
    _brdf_lut: wgpu::Texture,
    _sampler: wgpu::Sampler,
    ibl_bind: wgpu::BindGroup,
    sky_env_bind: wgpu::BindGroup,
}

impl Environment {
    pub fn ibl_bind(&self) -> &wgpu::BindGroup {
        &self.ibl_bind
    }
    pub fn sky_env_bind(&self) -> &wgpu::BindGroup {
        &self.sky_env_bind
    }
    pub fn prefilter_mips(&self) -> u32 {
        PREFILTER_MIPS
    }

    /// Cube `group(1)` layout: 0=irr, 1=prefilter, 2=brdf, 3=Filtering sampler.
    pub fn ibl_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ibl-layout"),
            entries: &[
                tex(0),
                tex(1),
                tex(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    /// Skybox `group(1)` layout: 0=env tex, 1=Filtering sampler.
    pub fn sky_env_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky-env-layout"),
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
            ],
        })
    }

    pub fn procedural(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ibl_layout: &wgpu::BindGroupLayout,
        sky_env_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        Self::build(device, queue, EnvSource::Procedural(DEFAULT_SKY), ibl_layout, sky_env_layout)
    }

    pub fn build(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: EnvSource,
        ibl_layout: &wgpu::BindGroupLayout,
        sky_env_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let pre = Precompute::new(device);

        // Resolve the source first so we can size the env texture to the .hdr
        // (capped) — the skybox samples this at mip 0, so its resolution is what
        // makes the background look sharp rather than blocky.
        let mut use_procedural: Option<SkyParams> = None;
        let mut use_atmosphere: Option<AtmosphereParams> = None;
        let mut decoded: Option<DecodedHdr> = None;
        match source {
            EnvSource::Hdr(path) => match load_hdr(path) {
                Ok(dec) => decoded = Some(dec),
                Err(e) => {
                    eprintln!("[organic-math env] hdr load failed ({e}); procedural sky");
                    use_procedural = Some(DEFAULT_SKY);
                }
            },
            EnvSource::Procedural(p) => use_procedural = Some(p),
            EnvSource::Atmosphere(a) => use_atmosphere = Some(a),
        }
        let (env_w, env_h) = match &decoded {
            Some(d) => capped_equirect(d.w, d.h),
            None => (PROC_ENV_W, PROC_ENV_H),
        };

        let env_mips = mip_count(env_w, env_h);
        let env_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("env"),
            size: wgpu::Extent3d { width: env_w, height: env_h, depth_or_array_layers: 1 },
            mip_level_count: env_mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FMT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Write ALL per-pass uniforms BEFORE any pass (queue writes are ordered
        // before the encoder submit; one buffer per distinct value).
        if let Some(d) = &decoded {
            upload_hdr_to_mip0(queue, &env_tex, d, env_w, env_h);
        }
        if let Some(p) = use_procedural {
            queue.write_buffer(
                &pre.sky_ub,
                0,
                bytemuck::bytes_of(&SkyUniform {
                    top: [p.top[0], p.top[1], p.top[2], 0.0],
                    horizon: [p.horizon[0], p.horizon[1], p.horizon[2], 0.0],
                    bottom: [p.bottom[0], p.bottom[1], p.bottom[2], 0.0],
                    intensity: [p.intensity, 0.0, 0.0, 0.0],
                }),
            );
        }
        if let Some(a) = use_atmosphere {
            queue.write_buffer(
                &pre.atmos_ub,
                0,
                bytemuck::bytes_of(&AtmosUniform {
                    sun: [a.sun_dir[0], a.sun_dir[1], a.sun_dir[2], a.sun_intensity],
                    p0: [a.turbidity, a.mie_g, a.ground_albedo, a.rayleigh],
                    p1: [a.exposure, 0.0, 0.0, 0.0],
                }),
            );
        }
        queue.write_buffer(&pre.down_ub, 0, bytemuck::bytes_of(&DownUniform { src_lod: 0.0, _p: [0.0; 3] }));
        for mip in 0..PREFILTER_MIPS {
            let roughness = mip as f32 / (PREFILTER_MIPS - 1) as f32;
            queue.write_buffer(
                &pre.pre_ubs[mip as usize],
                0,
                bytemuck::bytes_of(&PreUniform {
                    roughness,
                    src_w: env_w as f32,
                    src_h: env_h as f32,
                    _p: 0.0,
                }),
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ibl-precompute"),
        });

        if use_procedural.is_some() {
            pre.sky(device, &mut encoder, &env_tex);
        }
        if use_atmosphere.is_some() {
            pre.atmosphere(device, &mut encoder, &env_tex);
        }
        pre.gen_env_mips(device, &mut encoder, &env_tex, env_mips);
        let env_view = env_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let (irradiance, irr_view) = pre.irradiance(device, &mut encoder, &env_view);
        let (prefilter, pre_view) = pre.prefilter(device, &mut encoder, &env_view);
        let (brdf_lut, lut_view) = pre.brdf_lut(device, &mut encoder);

        queue.submit(std::iter::once(encoder.finish()));

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ibl-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let ibl_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ibl-bind"),
            layout: ibl_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&irr_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lut_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let sky_env_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky-env-bind"),
            layout: sky_env_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&env_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        Environment {
            _env_tex: env_tex,
            _irradiance: irradiance,
            _prefilter: prefilter,
            _brdf_lut: brdf_lut,
            _sampler: sampler,
            ibl_bind,
            sky_env_bind,
        }
    }
}

fn mip_count(w: u32, h: u32) -> u32 {
    32 - w.max(h).leading_zeros()
}


/// Target equirect size for a loaded image: native resolution, scaled down to
/// fit `MAX_ENV_W` while preserving aspect (no upscaling).
fn capped_equirect(w: u32, h: u32) -> (u32, u32) {
    if w <= MAX_ENV_W {
        (w.max(2), h.max(1))
    } else {
        let nw = MAX_ENV_W;
        let nh = ((h as u64 * nw as u64) / w as u64) as u32;
        (nw, nh.max(1))
    }
}

fn upload_hdr_to_mip0(queue: &wgpu::Queue, env: &wgpu::Texture, dec: &DecodedHdr, dw: u32, dh: u32) {
    let resampled = resample_bilinear(dec, dw, dh);
    let half: Vec<half::f16> = resampled.iter().map(|&v| half::f16::from_f32(v)).collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: env,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&half),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(dw * 8), // 4ch * 2 bytes
            rows_per_image: Some(dh),
        },
        wgpu::Extent3d { width: dw, height: dh, depth_or_array_layers: 1 },
    );
}

/// Bilinear resample the decoded RGBA-f32 equirect to (dw, dh), flipped
/// VERTICALLY. Equirect HDRs store the zenith in the top row, but our sampling
/// convention puts +Y (up) at v=1, so the image is mirrored on the way in.
/// Clamped at the edges (the longitude seam is hidden by the sampler's Repeat).
fn resample_bilinear(d: &DecodedHdr, dw: u32, dh: u32) -> Vec<f32> {
    let mut out = vec![0f32; (dw * dh * 4) as usize];
    let sw = d.w as f32;
    let sh = d.h as f32;
    let sample = |xi: u32, yi: u32, c: u32| d.rgba[((yi * d.w + xi) * 4 + c) as usize];
    for y in 0..dh {
        // Sample from the mirrored source row → vertical flip.
        let fy = ((dh - 1 - y) as f32 + 0.5) * sh / dh as f32 - 0.5;
        let fy0 = fy.floor();
        let wy = fy - fy0;
        let y0 = fy0.clamp(0.0, sh - 1.0) as u32;
        let y1 = (fy0 + 1.0).clamp(0.0, sh - 1.0) as u32;
        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * sw / dw as f32) - 0.5;
            let fx0 = fx.floor();
            let wx = fx - fx0;
            let x0 = fx0.clamp(0.0, sw - 1.0) as u32;
            let x1 = (fx0 + 1.0).clamp(0.0, sw - 1.0) as u32;
            let di = ((y * dw + x) * 4) as usize;
            for c in 0..4 {
                let top = sample(x0, y0, c) * (1.0 - wx) + sample(x1, y0, c) * wx;
                let bot = sample(x0, y1, c) * (1.0 - wx) + sample(x1, y1, c) * wx;
                out[di + c as usize] = top * (1.0 - wy) + bot * wy;
            }
        }
    }
    out
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniform {
    top: [f32; 4],
    horizon: [f32; 4],
    bottom: [f32; 4],
    intensity: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AtmosUniform {
    sun: [f32; 4], // xyz = dir to sun, w = intensity
    p0: [f32; 4],  // turbidity, mie_g, ground_albedo, rayleigh
    p1: [f32; 4],  // exposure, _, _, _
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DownUniform {
    src_lod: f32,
    _p: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PreUniform {
    roughness: f32,
    src_w: f32,
    src_h: f32,
    _p: f32,
}

struct Precompute {
    sampler: wgpu::Sampler,
    src_layout: wgpu::BindGroupLayout,
    uni_only_layout: wgpu::BindGroupLayout,
    empty_layout: wgpu::BindGroupLayout,
    sky_pipeline: wgpu::RenderPipeline,
    atmos_pipeline: wgpu::RenderPipeline,
    down_pipeline: wgpu::RenderPipeline,
    irr_pipeline: wgpu::RenderPipeline,
    pre_pipeline: wgpu::RenderPipeline,
    lut_pipeline: wgpu::RenderPipeline,
    sky_ub: wgpu::Buffer,
    atmos_ub: wgpu::Buffer,
    down_ub: wgpu::Buffer,
    pre_ubs: Vec<wgpu::Buffer>,
}

impl Precompute {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ibl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ibl.wgsl").into()),
        });

        let src_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ibl-src-layout"),
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
            ],
        });
        let uni_only_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ibl-uni-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ibl-empty-layout"),
            entries: &[],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ibl-precompute-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let mk = |label: &str, entry: &str, group0: &wgpu::BindGroupLayout| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(group0)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FMT,
                        blend: None,
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
        };

        let sky_pipeline = mk("ibl-sky", "fs_sky", &uni_only_layout);
        // The atmosphere pass reuses the single-uniform layout (its AtmosU buffer
        // is bound at group(0)/binding(0), like the sky uniform).
        let atmos_pipeline = mk("ibl-atmos", "fs_atmosphere", &uni_only_layout);
        let down_pipeline = mk("ibl-down", "fs_downsample", &src_layout);
        let irr_pipeline = mk("ibl-irr", "fs_irradiance", &src_layout);
        let pre_pipeline = mk("ibl-pre", "fs_prefilter", &src_layout);
        let lut_pipeline = mk("ibl-lut", "fs_brdf", &empty_layout);

        let mkbuf = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let sky_ub = mkbuf("ibl-sky-ub", std::mem::size_of::<SkyUniform>() as u64);
        let atmos_ub = mkbuf("ibl-atmos-ub", std::mem::size_of::<AtmosUniform>() as u64);
        let down_ub = mkbuf("ibl-down-ub", std::mem::size_of::<DownUniform>() as u64);
        let pre_ubs = (0..PREFILTER_MIPS)
            .map(|i| mkbuf(&format!("ibl-pre-ub-{i}"), std::mem::size_of::<PreUniform>() as u64))
            .collect();

        Precompute {
            sampler,
            src_layout,
            uni_only_layout,
            empty_layout,
            sky_pipeline,
            atmos_pipeline,
            down_pipeline,
            irr_pipeline,
            pre_pipeline,
            lut_pipeline,
            sky_ub,
            atmos_ub,
            down_ub,
            pre_ubs,
        }
    }

    fn pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        label: &str,
        view: &'a wgpu::TextureView,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    fn src_bind(
        &self,
        device: &wgpu::Device,
        src_view: &wgpu::TextureView,
        ub: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ibl-src-bind"),
            layout: &self.src_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
            ],
        })
    }

    fn sky(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, env: &wgpu::Texture) {
        let target = env.create_view(&mip_view_desc(0));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ibl-sky-bind"),
            layout: &self.uni_only_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.sky_ub.as_entire_binding() }],
        });
        let mut rp = Self::pass(encoder, "ibl-sky-pass", &target);
        rp.set_pipeline(&self.sky_pipeline);
        rp.set_bind_group(0, &bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Render the physically based atmosphere (#100) into env mip0, in place of the
    /// procedural gradient. The split-sum precompute then runs on it unchanged.
    fn atmosphere(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, env: &wgpu::Texture) {
        let target = env.create_view(&mip_view_desc(0));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ibl-atmos-bind"),
            layout: &self.uni_only_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.atmos_ub.as_entire_binding() }],
        });
        let mut rp = Self::pass(encoder, "ibl-atmos-pass", &target);
        rp.set_pipeline(&self.atmos_pipeline);
        rp.set_bind_group(0, &bind, &[]);
        rp.draw(0..3, 0..1);
    }

    fn gen_env_mips(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        env: &wgpu::Texture,
        mips: u32,
    ) {
        for dst in 1..mips {
            let src_view = env.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: dst - 1,
                mip_level_count: Some(1),
                ..mip_view_desc(0)
            });
            let dst_view = env.create_view(&mip_view_desc(dst));
            let bind = self.src_bind(device, &src_view, &self.down_ub);
            let mut rp = Self::pass(encoder, "ibl-down-pass", &dst_view);
            rp.set_pipeline(&self.down_pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    fn irradiance(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        env_view: &wgpu::TextureView,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&result_desc("irradiance", IRR_W, IRR_H, 1));
        let target = tex.create_view(&wgpu::TextureViewDescriptor::default());
        // fs_irradiance ignores binding 2; reuse down_ub to satisfy src_layout.
        let bind = self.src_bind(device, env_view, &self.down_ub);
        {
            let mut rp = Self::pass(encoder, "ibl-irr-pass", &target);
            rp.set_pipeline(&self.irr_pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    fn prefilter(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        env_view: &wgpu::TextureView,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&result_desc("prefilter", PRE_W, PRE_H, PREFILTER_MIPS));
        for mip in 0..PREFILTER_MIPS {
            let target = tex.create_view(&mip_view_desc(mip));
            let bind = self.src_bind(device, env_view, &self.pre_ubs[mip as usize]);
            let mut rp = Self::pass(encoder, "ibl-pre-pass", &target);
            rp.set_pipeline(&self.pre_pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    fn brdf_lut(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&result_desc("brdf-lut", LUT_SIZE, LUT_SIZE, 1));
        let target = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ibl-lut-bind"),
            layout: &self.empty_layout,
            entries: &[],
        });
        {
            let mut rp = Self::pass(encoder, "ibl-lut-pass", &target);
            rp.set_pipeline(&self.lut_pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }
}

fn mip_view_desc(mip: u32) -> wgpu::TextureViewDescriptor<'static> {
    wgpu::TextureViewDescriptor {
        label: Some("ibl-mip"),
        format: None,
        dimension: Some(wgpu::TextureViewDimension::D2),
        usage: None,
        aspect: wgpu::TextureAspect::All,
        base_mip_level: mip,
        mip_level_count: Some(1),
        base_array_layer: 0,
        array_layer_count: Some(1),
    }
}

fn result_desc(label: &'static str, w: u32, h: u32, mips: u32) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FMT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    }
}

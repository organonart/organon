//! Infinite terrain backdrop — the GPU pass + the CPU noise synthesis it reads.
//!
//! The terrain is a fullscreen raymarch (see `terrain.wgsl`) drawn as a parallax
//! BACKDROP behind the generator scene, mirroring the skybox pipeline pattern:
//! two variants — `Always`/write (glass/fallback path) and `Equal`/no-write
//! (opaque early-Z path) — so it fills only background pixels and the generator
//! geometry always composites in front.
//!
//! The landscape's character comes from a procedurally synthesized 256×256 noise
//! tile uploaded as an `R16Float` texture (`gen_noise`). The same value-noise fBm
//! is mirrored on the CPU (`terrain_height`) so the visual can ride the synthetic
//! fly-camera above the terrain. Both are pure + unit-tested; the constants below
//! must stay in sync with `terrain.wgsl`.

use bytemuck::{Pod, Zeroable};

/// Side length of the square noise tile (wrapped via `& 255` on both sides).
pub const NOISE_DIM: usize = 256;
/// world.xz → noise-domain frequency. **Must match `FREQ` in terrain.wgsl.**
const FREQ: f32 = 0.0016;

/// Per-octave domain rotation+scale, matching `ROT` in terrain.wgsl
/// (`mat2x2(0.8,-0.6, 0.6,0.8)`): `M·v = (0.8x + 0.6y, −0.6x + 0.8y)`.
#[inline]
fn rot2(x: f32, y: f32) -> (f32, f32) {
    (0.8 * x + 0.6 * y, -0.6 * x + 0.8 * y)
}

/// Terrain pass uniform (matches `TerrainU` in terrain.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct TerrainUniforms {
    pub inv_view_proj: [[f32; 4]; 4],
    pub ro: [f32; 4],  // synthetic fly-camera world pos (xyz)
    pub sun: [f32; 4], // xyz = unit dir TO sun, w = intensity
    pub p0: [f32; 4],  // height, snow_level, fog_density, ridged(0/1)
    pub p1: [f32; 4],  // brightness, haze, time, _
    pub p2: [f32; 4],  // PERF: march_steps, march_octaves, res_divisor, palette
    pub p3: [f32; 4],  // emissive, scatter, godray, water_on
    pub p4: [f32; 4],  // water_level (world y), water_hue, water_ripple, _
    // #100: physically based atmosphere. atmos = [enabled, turbidity, mie_g,
    // sun_intensity]; atmos2 = [ground_albedo, exposure, aerial_strength, rayleigh].
    pub atmos: [f32; 4],
    pub atmos2: [f32; 4],
    // #102 volumetric clouds. clouds = [enabled, coverage, density, base_alt];
    // clouds2 = [thickness, march_steps, detail, drift_speed];
    // clouds3 = [hg, absorption, shadow_strength, ambient].
    pub clouds: [f32; 4],
    pub clouds2: [f32; 4],
    pub clouds3: [f32; 4],
    // #102B FFT ocean. ocean = [enabled, level(world y), foam, glitter];
    // ocean2 = [hue, depth_absorption, tile_size, land_on(0/1)].
    pub ocean: [f32; 4],
    pub ocean2: [f32; 4],
}

// --- CPU noise + heightfield (mirrors terrain.wgsl) -------------------------

/// Deterministic 32-bit hash → `[0,1)`.
fn hash01(mut x: u32) -> f32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x as f32) / (u32::MAX as f32)
}

/// Synthesize a 256×256 noise tile in row-major `[0,1]`, deterministic from
/// `seed`. `kind` selects the lattice's *statistics*, which steer the fBm look:
///   0 White       — uniform per-texel random → the classic fractal-mountain base.
///   1 Cellular    — Worley F1 (distance to nearest feature point) → ridged mesas.
///   2 Smooth      — white noise box-blurred → gentler, rolling hills.
///   3 Billows     — `|2·white−1|` peaks stacked sparse → jagged spires.
/// The tile is sampled with `& 255` wrap, so it always tiles seamlessly.
pub fn gen_noise(kind: u32, seed: u32) -> Vec<f32> {
    let n = NOISE_DIM;
    let idx = |x: usize, y: usize| y * n + x;
    let h = |x: i32, y: i32, salt: u32| {
        let xi = (x.rem_euclid(n as i32)) as u32;
        let yi = (y.rem_euclid(n as i32)) as u32;
        hash01(seed ^ salt ^ xi.wrapping_mul(0x9e37_79b9) ^ yi.wrapping_mul(0x85eb_ca77))
    };
    let mut out = vec![0.0f32; n * n];

    match kind {
        // Cellular / Worley F1 on a tileable 16×16 jittered grid.
        1 => {
            let cells = 16i32;
            let cw = n as i32 / cells;
            for y in 0..n as i32 {
                for x in 0..n as i32 {
                    let (cx, cy) = (x / cw, y / cw);
                    let mut best = f32::MAX;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let gx = cx + dx;
                            let gy = cy + dy;
                            // Feature point inside cell (gx,gy), wrapped toroidally.
                            let fx = (gx.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x11) * cw as f32;
                            let fy = (gy.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x22) * cw as f32;
                            // Nearest image across the tile seam.
                            let mut ddx = (x as f32) - fx;
                            let mut ddy = (y as f32) - fy;
                            ddx -= (n as f32) * (ddx / n as f32).round();
                            ddy -= (n as f32) * (ddy / n as f32).round();
                            best = best.min(ddx * ddx + ddy * ddy);
                        }
                    }
                    let d = best.sqrt() / (cw as f32 * 1.4);
                    out[idx(x as usize, y as usize)] = d.clamp(0.0, 1.0);
                }
            }
        }
        // White noise box-blurred (3×3, wrapping) → lower-frequency, rolling.
        2 => {
            let white: Vec<f32> = (0..n * n)
                .map(|i| hash01(seed ^ (i as u32).wrapping_mul(2_654_435_761)))
                .collect();
            for y in 0..n as i32 {
                for x in 0..n as i32 {
                    let mut acc = 0.0;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let xi = (x + dx).rem_euclid(n as i32) as usize;
                            let yi = (y + dy).rem_euclid(n as i32) as usize;
                            acc += white[idx(xi, yi)];
                        }
                    }
                    out[idx(x as usize, y as usize)] = acc / 9.0;
                }
            }
        }
        // Sparse jagged spires: a few sharp peaks over a low floor.
        3 => {
            for y in 0..n {
                for x in 0..n {
                    let base = hash01(seed ^ (idx(x, y) as u32).wrapping_mul(0x27d4_eb2f));
                    let spike = hash01(seed ^ 0x5bd1 ^ (idx(x, y) as u32).wrapping_mul(0x1656_67b1));
                    let v = 0.25 * base + 0.75 * (1.0 - (2.0 * spike - 1.0).abs()).powf(3.0);
                    out[idx(x, y)] = v.clamp(0.0, 1.0);
                }
            }
        }
        // Voronoi / cracked (C4D "Voronoi"): F2−F1 → ~0 along cell borders (cracks),
        // high inside → cracked plateaus / dry-lake polygons after fBm.
        4 => {
            let cells = 14i32;
            let cw = n as i32 / cells;
            for y in 0..n as i32 {
                for x in 0..n as i32 {
                    let (cx, cy) = (x / cw, y / cw);
                    let (mut d1, mut d2) = (f32::MAX, f32::MAX);
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (gx, gy) = (cx + dx, cy + dy);
                            let fx = (gx.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x31) * cw as f32;
                            let fy = (gy.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x32) * cw as f32;
                            let mut ddx = (x as f32) - fx;
                            let mut ddy = (y as f32) - fy;
                            ddx -= (n as f32) * (ddx / n as f32).round();
                            ddy -= (n as f32) * (ddy / n as f32).round();
                            let d = ddx * ddx + ddy * ddy;
                            if d < d1 {
                                d2 = d1;
                                d1 = d;
                            } else if d < d2 {
                                d2 = d;
                            }
                        }
                    }
                    let edge = ((d2.sqrt() - d1.sqrt()) / (cw as f32 * 0.7)).clamp(0.0, 1.0);
                    out[idx(x as usize, y as usize)] = edge;
                }
            }
        }
        // Dents (C4D "Dents"): a plateau pocked by sharp circular pits at sparse
        // feature points → cratered / dented terrain.
        5 => {
            let cells = 12i32;
            let cw = n as i32 / cells;
            for y in 0..n as i32 {
                for x in 0..n as i32 {
                    let (cx, cy) = (x / cw, y / cw);
                    let mut best = f32::MAX;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (gx, gy) = (cx + dx, cy + dy);
                            let fx = (gx.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x41) * cw as f32;
                            let fy = (gy.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x42) * cw as f32;
                            let mut ddx = (x as f32) - fx;
                            let mut ddy = (y as f32) - fy;
                            ddx -= (n as f32) * (ddx / n as f32).round();
                            ddy -= (n as f32) * (ddy / n as f32).round();
                            best = best.min(ddx * ddx + ddy * ddy);
                        }
                    }
                    let r = cw as f32 * 0.32;
                    let pit = (-best / (r * r)).exp(); // 1 at the point → deep pit
                    out[idx(x as usize, y as usize)] = (0.78 - 0.78 * pit).clamp(0.0, 1.0);
                }
            }
        }
        // Wavy (C4D "Wavy Turbulence"): noise-warped sinusoidal bands → sedimentary
        // strata / terraced ridges after fBm.
        6 => {
            let k = 6.0f32; // integer band count → tiles in y
            for y in 0..n {
                for x in 0..n {
                    let wn = h((x / 6) as i32, (y / 6) as i32, 0x51);
                    let phase = std::f32::consts::TAU * k * (y as f32 / n as f32) + (wn - 0.5) * 6.0;
                    out[idx(x, y)] = 0.5 + 0.5 * phase.sin();
                }
            }
        }
        // Sparse (C4D "Sparse Convolution"): isolated soft buttes on a low floor →
        // lone mesas / hoodoos standing out of the flats.
        7 => {
            let cells = 9i32;
            let cw = n as i32 / cells;
            for y in 0..n as i32 {
                for x in 0..n as i32 {
                    let (cx, cy) = (x / cw, y / cw);
                    let mut v = 0.12f32; // low floor
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (gx, gy) = (cx + dx, cy + dy);
                            if h(gx, gy, 0x61) > 0.5 {
                                // ~half the cells host a butte (seed-gated).
                                let fx = (gx.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x62) * cw as f32;
                                let fy = (gy.rem_euclid(cells) * cw) as f32 + h(gx, gy, 0x63) * cw as f32;
                                let mut ddx = (x as f32) - fx;
                                let mut ddy = (y as f32) - fy;
                                ddx -= (n as f32) * (ddx / n as f32).round();
                                ddy -= (n as f32) * (ddy / n as f32).round();
                                let r = cw as f32 * 0.42;
                                let bump = (-(ddx * ddx + ddy * ddy) / (r * r)).exp();
                                v = v.max(0.12 + 0.88 * bump);
                            }
                        }
                    }
                    out[idx(x as usize, y as usize)] = v.clamp(0.0, 1.0);
                }
            }
        }
        // White noise (default).
        _ => {
            for y in 0..n {
                for x in 0..n {
                    out[idx(x, y)] = hash01(seed ^ (idx(x, y) as u32).wrapping_mul(2_654_435_761));
                }
            }
        }
    }
    out
}

#[inline]
fn cell(noise: &[f32], ix: i32, iy: i32) -> f32 {
    let x = (ix & 255) as usize;
    let y = (iy & 255) as usize;
    noise[y * NOISE_DIM + x]
}

/// Smooth value noise over the tile (mirrors `vnoise` in terrain.wgsl).
fn vnoise(noise: &[f32], x: f32, y: f32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (tx, ty) = (x - fx, y - fy);
    let wx = tx * tx * (3.0 - 2.0 * tx);
    let wy = ty * ty * (3.0 - 2.0 * ty);
    let a = cell(noise, ix, iy);
    let b = cell(noise, ix + 1, iy);
    let c = cell(noise, ix, iy + 1);
    let d = cell(noise, ix + 1, iy + 1);
    let ab = a + (b - a) * wx;
    let cd = c + (d - c) * wx;
    ab + (cd - ab) * wy
}

fn fbm(noise: &[f32], mut x: f32, mut y: f32, octaves: i32, ridged: bool) -> f32 {
    let mut amp = 0.5;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        let mut nv = vnoise(noise, x, y);
        if ridged {
            nv = 1.0 - (2.0 * nv - 1.0).abs();
        }
        sum += amp * nv;
        norm += amp;
        amp *= 0.5;
        let (rx, ry) = rot2(x, y);
        x = rx * 2.0;
        y = ry * 2.0;
    }
    sum / norm
}

/// World-space terrain height at `(wx, wz)` for the synthetic camera to ride.
/// A low-octave version of the GPU heightfield (gross shape only); `height` is the
/// vertical scale and `ridged` the same fold the shader uses.
pub fn terrain_height(noise: &[f32], wx: f32, wz: f32, height: f32, ridged: bool) -> f32 {
    height * fbm(noise, wx * FREQ, wz * FREQ, 3, ridged)
}

// --- GPU pass ---------------------------------------------------------------

pub struct Terrain {
    uniform: wgpu::Buffer,
    u_bind: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    tex_bind: wgpu::BindGroup,
    // #102B: the FFT ocean tile (group 2) — a filterable RGBA16F texture + sampler,
    // owned by `Ocean`, bound here so the water shader can tile it.
    ocean_layout: wgpu::BindGroupLayout,
    ocean_bind: Option<wgpu::BindGroup>,
    shader: wgpu::ShaderModule,
    pl: wgpu::PipelineLayout,
    // Always/write (glass/fallback path) + Equal/no-write (opaque early-Z path).
    pipeline_always: wgpu::RenderPipeline,
    pipeline_eq: wgpu::RenderPipeline,
    hdr_format: wgpu::TextureFormat,
    // --- Half-resolution path (perf): raymarch into a half-size HDR target, then
    // point-upscale it into the scene pass with the same far-plane depth. ---
    blit_layout: wgpu::BindGroupLayout, // group(2): the half-res source texture
    blit_pl: wgpu::PipelineLayout,
    pipeline_half: wgpu::RenderPipeline, // vs_terrain + fs_terrain → half target, no depth
    blit_always: wgpu::RenderPipeline,   // vs_terrain + fs_blit (Always/write)
    blit_eq: wgpu::RenderPipeline,       // vs_terrain + fs_blit (Equal/no-write)
    half_dim: (u32, u32),
    half_view: Option<wgpu::TextureView>,
    blit_bind: Option<wgpu::BindGroup>,
}

impl Terrain {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hdr_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("terrain.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain-uniforms"),
            size: std::mem::size_of::<TerrainUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let u_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain-uniform-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let u_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain-uniform-bind"),
            layout: &u_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });

        // Noise tile: an unfilterable R16Float texture read via textureLoad only.
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain-tex-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let tex_bind = make_noise_bind(device, queue, &tex_layout, &gen_noise(0, 1));

        // #102B ocean tile (group 2): a filterable RGBA16F texture + a sampler.
        let ocean_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain-ocean-layout"),
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
        });

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain-pipeline-layout"),
            bind_group_layouts: &[Some(&u_layout), Some(&tex_layout), Some(&ocean_layout)],
            immediate_size: 0,
        });
        let pipeline_always = make_pipeline(
            device, &shader, &pl, hdr_format, depth_format, sample_count,
            wgpu::CompareFunction::Always, true,
        );
        let pipeline_eq = make_pipeline(
            device, &shader, &pl, hdr_format, depth_format, sample_count,
            wgpu::CompareFunction::Equal, false,
        );

        // Half-res path: group(2) = the half-size terrain colour to upscale.
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain-blit-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain-blit-pl"),
            // group(2) = ocean (unused by fs_blit but kept so the layout matches the
            // raymarch pipeline); group(3) = the half-res source.
            bind_group_layouts: &[
                Some(&u_layout),
                Some(&tex_layout),
                Some(&ocean_layout),
                Some(&blit_layout),
            ],
            immediate_size: 0,
        });
        // The half-res render target itself is single-sample, no depth.
        let pipeline_half = make_half_pipeline(device, &shader, &pl, hdr_format);
        let blit_always = make_blit_pipeline(
            device, &shader, &blit_pl, hdr_format, depth_format, sample_count,
            wgpu::CompareFunction::Always, true,
        );
        let blit_eq = make_blit_pipeline(
            device, &shader, &blit_pl, hdr_format, depth_format, sample_count,
            wgpu::CompareFunction::Equal, false,
        );

        Terrain {
            uniform,
            u_bind,
            tex_layout,
            tex_bind,
            ocean_layout,
            ocean_bind: None,
            shader,
            pl,
            pipeline_always,
            pipeline_eq,
            hdr_format,
            blit_layout,
            blit_pl,
            pipeline_half,
            blit_always,
            blit_eq,
            half_dim: (0, 0),
            half_view: None,
            blit_bind: None,
        }
    }

    /// Rebuild the pipelines for a new MSAA sample count (called alongside the
    /// scene pipelines).
    pub fn set_sample_count(
        &mut self,
        device: &wgpu::Device,
        hdr_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        n: u32,
    ) {
        self.pipeline_always = make_pipeline(
            device, &self.shader, &self.pl, hdr_format, depth_format, n,
            wgpu::CompareFunction::Always, true,
        );
        self.pipeline_eq = make_pipeline(
            device, &self.shader, &self.pl, hdr_format, depth_format, n,
            wgpu::CompareFunction::Equal, false,
        );
        // The blit (upscale) pipelines run in the scene pass, so they match MSAA.
        self.blit_always = make_blit_pipeline(
            device, &self.shader, &self.blit_pl, hdr_format, depth_format, n,
            wgpu::CompareFunction::Always, true,
        );
        self.blit_eq = make_blit_pipeline(
            device, &self.shader, &self.blit_pl, hdr_format, depth_format, n,
            wgpu::CompareFunction::Equal, false,
        );
    }

    /// Replace the noise tile (when the editor changes noise type or seed). `data`
    /// is row-major `[0,1]`, length `NOISE_DIM²`.
    pub fn set_noise(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[f32]) {
        self.tex_bind = make_noise_bind(device, queue, &self.tex_layout, data);
    }

    /// Bind the FFT ocean tile (#102B). Called once after the `Ocean` is created
    /// (its texture is fixed-size, so the bind group never needs rebuilding).
    pub fn set_ocean(&mut self, device: &wgpu::Device, view: &wgpu::TextureView, sampler: &wgpu::Sampler) {
        self.ocean_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain-ocean-bind"),
            layout: &self.ocean_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        }));
    }

    /// Ensure the low-resolution target matches `size/scale` (recreate on resize
    /// or scale change). Call before `render_low`. `size` is the full scene res,
    /// `scale` the divisor (2 = half, 4 = quarter).
    fn ensure_low(&mut self, device: &wgpu::Device, size: (u32, u32), scale: u32) {
        let s = scale.max(1);
        let low = ((size.0.max(s) / s).max(1), (size.1.max(s) / s).max(1));
        if self.half_view.is_some() && self.half_dim == low {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain-low"),
            size: wgpu::Extent3d { width: low.0, height: low.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.hdr_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.blit_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain-blit-bind"),
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        }));
        self.half_view = Some(view);
        self.half_dim = low;
    }

    /// Raymarch the terrain into the low-res target (own pass, no depth). Call
    /// before the scene pass when downscaling is on; then `upscale_draw` in the
    /// scene. `scale` is the divisor (2 = half, 4 = quarter).
    pub fn render_low(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        scale: u32,
    ) {
        self.ensure_low(device, size, scale);
        let view = self.half_view.as_ref().unwrap();
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terrain-low-pass"),
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
        });
        rp.set_pipeline(&self.pipeline_half);
        rp.set_bind_group(0, &self.u_bind, &[]);
        rp.set_bind_group(1, &self.tex_bind, &[]);
        if let Some(ocean) = self.ocean_bind.as_ref() {
            rp.set_bind_group(2, ocean, &[]);
        }
        rp.draw(0..3, 0..1);
    }

    /// Upscale the half-res target into the scene pass (point sample + far depth),
    /// in place of `draw`. `opaque_path` picks the Equal/no-write variant.
    pub fn upscale_draw(&self, rp: &mut wgpu::RenderPass<'_>, opaque_path: bool) {
        let Some(blit_bind) = self.blit_bind.as_ref() else { return };
        rp.set_pipeline(if opaque_path { &self.blit_eq } else { &self.blit_always });
        rp.set_bind_group(0, &self.u_bind, &[]);
        rp.set_bind_group(1, &self.tex_bind, &[]);
        if let Some(ocean) = self.ocean_bind.as_ref() {
            rp.set_bind_group(2, ocean, &[]);
        }
        rp.set_bind_group(3, blit_bind, &[]);
        rp.draw(0..3, 0..1);
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, u: &TerrainUniforms) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(u));
    }

    /// Draw the backdrop into the current scene pass. `opaque_path` selects the
    /// Equal/no-write variant (depth already filled by the early-Z prepass).
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>, opaque_path: bool) {
        rp.set_pipeline(if opaque_path { &self.pipeline_eq } else { &self.pipeline_always });
        rp.set_bind_group(0, &self.u_bind, &[]);
        rp.set_bind_group(1, &self.tex_bind, &[]);
        if let Some(ocean) = self.ocean_bind.as_ref() {
            rp.set_bind_group(2, ocean, &[]);
        }
        rp.draw(0..3, 0..1);
    }
}

fn make_noise_bind(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    data: &[f32],
) -> wgpu::BindGroup {
    let dim = NOISE_DIM as u32;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain-noise"),
        size: wgpu::Extent3d { width: dim, height: dim, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let halfs: Vec<half::f16> = data.iter().map(|&v| half::f16::from_f32(v)).collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&halfs),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(dim * 2),
            rows_per_image: Some(dim),
        },
        wgpu::Extent3d { width: dim, height: dim, depth_or_array_layers: 1 },
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("terrain-tex-bind"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
    })
}

fn make_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    hdr_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terrain-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_terrain"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_terrain"),
            targets: &[Some(wgpu::ColorTargetState {
                format: hdr_format,
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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(depth_compare),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

/// The half-res raymarch pipeline: `fs_terrain` into a single-sample HDR target
/// with NO depth (the half pass fills every pixel; depth comes later at upscale).
fn make_half_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    hdr_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terrain-half-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_terrain"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_terrain"),
            targets: &[Some(wgpu::ColorTargetState {
                format: hdr_format,
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
}

/// The upscale (blit) pipeline: `fs_blit` point-samples the half-res target into
/// the scene pass with the same far-plane depth semantics as the full-res draw.
fn make_blit_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    hdr_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terrain-blit-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_terrain"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_blit"),
            targets: &[Some(wgpu::ColorTargetState {
                format: hdr_format,
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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(depth_compare),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_noise_is_deterministic_and_bounded() {
        for kind in 0..8u32 {
            let a = gen_noise(kind, 7);
            let b = gen_noise(kind, 7);
            assert_eq!(a, b, "kind {kind} not deterministic");
            assert_eq!(a.len(), NOISE_DIM * NOISE_DIM);
            assert!(a.iter().all(|v| (0.0..=1.0).contains(v)), "kind {kind} out of [0,1]");
            // Actually varies (not a constant field).
            let v0 = a[0];
            assert!(a.iter().any(|&v| (v - v0).abs() > 1e-3), "kind {kind} is constant");
        }
    }

    #[test]
    fn gen_noise_seed_changes_field() {
        let a = gen_noise(0, 1);
        let b = gen_noise(0, 2);
        assert_ne!(a, b, "different seeds gave identical tiles");
    }

    #[test]
    fn terrain_height_is_finite_deterministic_and_scales() {
        let noise = gen_noise(0, 1);
        let mut max_h = 0.0f32;
        for i in 0..200 {
            let x = (i as f32) * 13.7 - 400.0;
            let z = (i as f32) * -9.3 + 250.0;
            let h = terrain_height(&noise, x, z, 120.0, false);
            assert!(h.is_finite());
            assert!((0.0..=120.0).contains(&h), "height {h} out of band at {x},{z}");
            max_h = max_h.max(h);
            // Deterministic.
            assert_eq!(h, terrain_height(&noise, x, z, 120.0, false));
        }
        // The field is not flat, and doubling the scale doubles the height.
        assert!(max_h > 1.0, "terrain looks flat");
        let single = terrain_height(&noise, 12.0, -7.0, 100.0, false);
        let double = terrain_height(&noise, 12.0, -7.0, 200.0, false);
        assert!((double - 2.0 * single).abs() < 1e-3, "height should scale linearly");
    }
}

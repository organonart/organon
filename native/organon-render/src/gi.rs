//! Bounced-GI irradiance probe volume (#80 Part B).
//!
//! A coarse 6³ grid of RGB irradiance probes, filled CPU-side from the field's
//! nodes (`math::compute_gi_probes` — the node field is a pure function) and
//! uploaded as a single small uniform buffer. The cube shader trilinearly samples
//! it and adds coloured diffuse "bounce" into the ambient term, so a bright/
//! coloured strand bleeds its colour onto its neighbours. The grid is tiny enough
//! to live in a uniform, so there's no 3D texture / sampler — just one extra
//! uniform bind group (group 3) on the cube pipeline. Inert at intensity 0.

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};
use organon_core::math::{GI_GRID_DIM, GI_SLOTS};

use super::MetaNode;

/// Max emissive cubes used as real point lights (#167 Tier 3). Matches
/// `array<MLight, 64>` in `cube.wgsl` (group 3 binding 1).
const MAX_LIGHTS: usize = 64;

/// Matches `LightsU` in cube.wgsl (group 3 binding 1). Per light: two vec4 —
/// `[pos.xyz, radius]` then `[color.rgb, _]`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightsU {
    header: [f32; 4], // x = count, y = intensity, z/w reserved
    lights: [[f32; 4]; MAX_LIGHTS * 2],
}

/// Matches `GiUniform` in cube.wgsl (group 3). #152 Tier 3 (DDGI/SH): each probe is
/// **3 vec4 slots** `[L0, L1x, L1y, L1z]` per colour channel (band-1 SH →
/// directional bounce), indexed `(z·DIM² + y·DIM + x)·3 + channel`, matching
/// `math::compute_gi_probes`. All 4 components carry data now (`.w` = L1z).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GiUniform {
    bbox_min: [f32; 4], // xyz = field AABB min
    bbox_max: [f32; 4], // xyz = field AABB max
    info: [f32; 4],     // x = enabled, y = intensity, z = grid dim, w = falloff
    probes: [[f32; 4]; GI_SLOTS],
}

/// One tracked emissive light (#167 T3): the brightest-N selection is re-derived
/// every frame over an ANIMATED field with thousands of near-equal luminance keys,
/// so the raw top-N set churned frame to frame — visible popping of crisp specular
/// glints. Selection now has hysteresis (a light keeps its slot while a nearby
/// candidate exists) and a per-light fade envelope (in/out over ~10 frames).
struct ActiveLight {
    pos: Vec3,
    color: [f32; 3],
    lum: f32,
    weight: f32, // 0..1 fade envelope (multiplies the uploaded colour)
    target: f32, // 1 = matched into the current top-N, 0 = fading out
}

/// Per-frame approach rate of the fade envelope (~10 frames to settle).
const LIGHT_FADE_RATE: f32 = 0.25;

pub struct GiVolume {
    buf: wgpu::Buffer,
    lights_buf: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    // Specular occlusion (#174 T3): the blurred SSAO rides this group as binding
    // 2/3 (cube-pipeline-only, so no new bind group). A 1×1 WHITE dummy stands in
    // whenever AO wasn't computed this frame — SO(ao = 1) is a no-op, so a stale
    // or missing AO can never darken anything.
    ao_dummy: wgpu::TextureView,
    ao_samp: wgpu::Sampler,
    ao_key: Option<u64>,
    active: Vec<ActiveLight>,
    // Edge-detection for the off state (#174 T2): with GI / many-lights disabled
    // the ~10 KB + ~2 KB uniforms were re-uploaded as zeros every frame.
    probes_zeroed: bool,
    lights_zeroed: bool,
    // ReSTIR many-lights (#200 Tier 5d): per-frame RNG seed so the reservoir
    // importance-sampling picks a different tail of the field each frame (the fade
    // envelope integrates the rotation). Increments per `update_lights` call.
    light_frame: u32,
}

impl GiVolume {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gi-probes"),
            size: std::mem::size_of::<GiUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // #167 Tier 3: emissive-cubes-as-lights uniform (group 3 binding 1).
        let lights_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gi-lights"),
            size: std::mem::size_of::<LightsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gi-layout"),
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
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // #174 T3: blurred SSAO for specular occlusion (+ its sampler).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // 1×1 WHITE AO dummy (R8 = 255): SO(ao = 1) is the identity, so binding it
        // whenever real AO isn't available makes the term a guaranteed no-op.
        let ao_dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gi-ao-dummy"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &ao_dummy_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let ao_dummy = ao_dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let ao_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gi-ao-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind = make_gi_bind(device, &layout, &buf, &lights_buf, &ao_dummy, &ao_samp);
        GiVolume {
            buf,
            lights_buf,
            layout,
            bind,
            ao_dummy,
            ao_samp,
            ao_key: None,
            active: Vec::new(),
            probes_zeroed: false,
            lights_zeroed: false,
            light_frame: 0,
        }
    }

    /// The raw probe uniform buffer (#182 T4: the ink march binds it too, so
    /// the smoke breathes with the same probe GI the surfaces use).
    pub fn probes_buffer(&self) -> &wgpu::Buffer {
        &self.buf
    }
    /// The raw many-lights uniform buffer (#182 T4: the refractive water
    /// binds it so ghost-lit point lights land on the water too).
    pub fn lights_buffer(&self) -> &wgpu::Buffer {
        &self.lights_buf
    }
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }
    pub fn bind(&self) -> &wgpu::BindGroup {
        &self.bind
    }

    /// Point group-3 binding 2 at the frame's blurred AO (specular occlusion,
    /// #174 T3) — or back at the white dummy when AO wasn't computed. Cached by
    /// `depth_key` (bumps on any resize / MSAA / prepass-route change, which is
    /// exactly when the AO texture is recreated) + the availability bit.
    pub fn ensure_ao(
        &mut self,
        device: &wgpu::Device,
        ao: Option<&wgpu::TextureView>,
        depth_key: u64,
    ) {
        let key = depth_key.wrapping_mul(2).wrapping_add(ao.is_some() as u64);
        if self.ao_key == Some(key) {
            return;
        }
        self.ao_key = Some(key);
        let view = ao.unwrap_or(&self.ao_dummy);
        self.bind = make_gi_bind(device, &self.layout, &self.buf, &self.lights_buf, view, &self.ao_samp);
    }

    /// Upload the probe grid + metadata. `probes` may be shorter than `GI_CELLS`
    /// (empty when GI is off) — missing cells stay black, so the shader (which
    /// also gates on `enabled`) adds nothing.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        min: Vec3,
        max: Vec3,
        enabled: bool,
        intensity: f32,
        falloff: f32,
        probes: &[Vec4],
    ) {
        // Off → the shader ignores everything but `enabled`; upload the zeroed
        // uniform once and skip until re-enabled (#174 T2).
        if !enabled {
            if self.probes_zeroed {
                return;
            }
            self.probes_zeroed = true;
        } else {
            self.probes_zeroed = false;
        }
        let mut u = GiUniform::zeroed();
        u.bbox_min = [min.x, min.y, min.z, 0.0];
        u.bbox_max = [max.x, max.y, max.z, 0.0];
        u.info = [
            if enabled { 1.0 } else { 0.0 },
            intensity,
            GI_GRID_DIM as f32,
            falloff,
        ];
        // All four components carry SH data now (`.w` = L1z), so copy verbatim.
        for (i, p) in probes.iter().take(GI_SLOTS).enumerate() {
            u.probes[i] = [p.x, p.y, p.z, p.w];
        }
        queue.write_buffer(&self.buf, 0, bytemuck::bytes_of(&u));
    }

    /// Emissive cubes as real lights (#167 Tier 3): pick the brightest `count` nodes
    /// (by colour luminance) and upload them as point lights (each with the given
    /// `radius` falloff). Selection is stabilized by hysteresis + a per-light fade
    /// (see `ActiveLight`) so the set doesn't pop on animated fields. `enabled =
    /// false` / `count = 0` uploads count 0 → the shader loop adds nothing (today's
    /// look). Called once per frame from `render.rs` with the same `meta_nodes`
    /// VXGI uses.
    pub fn update_lights(
        &mut self,
        queue: &wgpu::Queue,
        nodes: &[MetaNode],
        enabled: bool,
        intensity: f32,
        radius: f32,
        count: usize,
        restir: bool,
    ) {
        self.light_frame = self.light_frame.wrapping_add(1);
        let mut u = LightsU::zeroed();
        if enabled && count > 0 && !nodes.is_empty() {
            self.lights_zeroed = false;
            let lum = |c: &[f32; 4]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            let want = count.min(MAX_LIGHTS).min(nodes.len());
            let mut idx: Vec<usize> = (0..nodes.len()).collect();
            if restir && want < idx.len() {
                // ReSTIR many-lights (#200 Tier 5d): weighted reservoir sampling
                // WITHOUT replacement instead of a hard brightest-`want` cap. Each
                // node gets an Efraimidis–Spirakis key `u^(1/lum)`; the top-`want`
                // keys are the sample. The brightest cubes almost always key ~1 (a
                // stable core), while the remaining slots rotate through dimmer/
                // off-screen emitters by importance — so every glowing cube can
                // become a light over time, not just the top `want`. `lum = 0` cubes
                // key 0 (never lit).
                //
                // The random draw `u` is HELD for `RESEED_FRAMES` frames rather than
                // re-rolled every frame: re-rolling per frame churned the selection
                // and the rotating lights' specular glints flickered (a per-frame
                // re-roll can't be temporally integrated on sharp highlights). Now
                // the set holds still for ~½ s, then shifts to the next importance
                // sample — the positional match + fade below smooth each shift, so
                // the rotation reads as a slow drift instead of a twinkle.
                const RESEED_FRAMES: u32 = 30;
                let seed = self.light_frame / RESEED_FRAMES;
                let keys: Vec<f32> = nodes
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        organon_core::math::es_key(organon_core::math::restir_rand(seed, i as u32), lum(&n.color))
                    })
                    .collect();
                idx.select_nth_unstable_by(want - 1, |&a, &b| {
                    keys[b].partial_cmp(&keys[a]).unwrap_or(std::cmp::Ordering::Equal)
                });
                idx.truncate(want);
            } else if want < idx.len() {
                // Brightest-`want` by luminance — a partial select, not a full sort
                // of every node (the order within the top set doesn't matter).
                idx.select_nth_unstable_by(want - 1, |&a, &b| {
                    lum(&nodes[b].color)
                        .partial_cmp(&lum(&nodes[a].color))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                idx.truncate(want);
            }
            // Match existing lights to nearby candidates (the field animates, so
            // identity is positional): a matched light tracks its node; an
            // unmatched one fades out instead of vanishing.
            let match_r = (radius * 0.75).max(1e-3);
            let match_r2 = match_r * match_r;
            let mut used = vec![false; idx.len()];
            for al in self.active.iter_mut() {
                al.target = 0.0;
                let mut best = usize::MAX;
                let mut best_d = match_r2;
                for (k, &i) in idx.iter().enumerate() {
                    if used[k] {
                        continue;
                    }
                    let p = nodes[i].pos;
                    let d = (Vec3::new(p[0], p[1], p[2]) - al.pos).length_squared();
                    if d < best_d {
                        best_d = d;
                        best = k;
                    }
                }
                if best != usize::MAX {
                    used[best] = true;
                    let i = idx[best];
                    let (p, c) = (nodes[i].pos, nodes[i].color);
                    al.pos = Vec3::new(p[0], p[1], p[2]);
                    al.color = [c[0], c[1], c[2]];
                    al.lum = lum(&c);
                    al.target = 1.0;
                }
            }
            // Unmatched candidates become new lights, fading in from 0.
            for (k, &i) in idx.iter().enumerate() {
                if used[k] {
                    continue;
                }
                let (p, c) = (nodes[i].pos, nodes[i].color);
                self.active.push(ActiveLight {
                    pos: Vec3::new(p[0], p[1], p[2]),
                    color: [c[0], c[1], c[2]],
                    lum: lum(&c),
                    weight: 0.0,
                    target: 1.0,
                });
            }
            for al in self.active.iter_mut() {
                al.weight += (al.target - al.weight) * LIGHT_FADE_RATE;
            }
            self.active.retain(|al| al.target > 0.5 || al.weight > 0.02);
            // Upload the strongest faded contributions (fading-out lights compete
            // with fading-in replacements, so the transition cross-fades).
            self.active.sort_unstable_by(|a, b| {
                (b.weight * b.lum)
                    .partial_cmp(&(a.weight * a.lum))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let take = self.active.len().min(MAX_LIGHTS);
            for (k, al) in self.active.iter().take(take).enumerate() {
                u.lights[k * 2] = [al.pos.x, al.pos.y, al.pos.z, radius];
                u.lights[k * 2 + 1] = [
                    al.color[0] * al.weight,
                    al.color[1] * al.weight,
                    al.color[2] * al.weight,
                    1.0,
                ];
            }
            u.header = [take as f32, intensity, 0.0, 0.0];
        } else {
            self.active.clear();
            // Off → upload the zeroed light list once, then skip (#174 T2).
            if self.lights_zeroed {
                return;
            }
            self.lights_zeroed = true;
        }
        queue.write_buffer(&self.lights_buf, 0, bytemuck::bytes_of(&u));
    }
}

/// Build the group-3 bind group (probes + lights + the AO/SO texture pair).
fn make_gi_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buf: &wgpu::Buffer,
    lights_buf: &wgpu::Buffer,
    ao: &wgpu::TextureView,
    ao_samp: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gi-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: lights_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(ao) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(ao_samp) },
        ],
    })
}

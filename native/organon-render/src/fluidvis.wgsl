// Fluid Ink (#182 Tier 1) — render the dye the generator stirs into the medium.
//
// Three entry points:
//   cs_dye_blit — copy the fluid solver's dye storage buffer into a 3D texture
//                 (trilinear-sampleable by the march).
//   fs_ink      — front-to-back volumetric raymarch of the dye into the INK
//                 TARGET (premultiplied rgb + coverage alpha): Beer–Lambert
//                 extinction, Henyey–Greenstein single scatter from the key
//                 light with a short self-shadow light-march, ambient in-scatter
//                 from the IBL irradiance map (the environment lights the ink),
//                 and an emissive-dye dial. The march is clamped by the scene
//                 depth prepass, so ink composites correctly against geometry.
//   fs_upsample — composite the (possibly half-res) ink target over the HDR
//                 scene buffer with a depth-aware (joint bilateral) upsample, so
//                 geometry edges don't halo at half resolution.
//
// Outputs LINEAR HDR radiance pre-bloom (exposure/bloom/tonemap in composite).

struct InkU {
    inv_vp: mat4x4<f32>,  // unproject screen rays (matches the scene view-proj)
    cam: vec4<f32>,       // xyz = camera world pos, w = use_depth (0/1)
    bmin: vec4<f32>,      // xyz = fluid AABB min, w = extinction (Beer–Lambert σ)
    bmax: vec4<f32>,      // xyz = fluid AABB max, w = march steps
    key: vec4<f32>,       // xyz = dir TO key light (unit), w = intensity
    scat: vec4<f32>,      // x = scatter, y = emissive, z = anisotropy g, w = ambient
    envt: vec4<f32>,      // x = env rotation (rad), yzw = env tint rgb
    texel: vec4<f32>,     // xy = 1 / full-res size, z = full→ink scale (1 or 2), w = time
    grid: vec4<f32>,      // xyz = dye grid resolution, w = detail amount (#182 T2)
    sizes: vec4<f32>,     // xy = ink target size (px), z = reveal threshold,
                          // w = scene-shadow receive strength (#182 T4, 0 = off)
    light_vp: mat4x4<f32>,// world → the scene shadow map's light clip (#182 T4)
    vgi0: vec4<f32>,      // xyz = VXGI volume min, w = VXGI in-scatter gain (0 = off)
    vgi1: vec4<f32>,      // xyz = VXGI volume max
};

@group(0) @binding(0) var<uniform> u: InkU;
@group(0) @binding(1) var dye_tex: texture_3d<f32>;
@group(0) @binding(2) var lin_samp: sampler;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var ink_tex: texture_2d<f32>;          // fs_upsample only
@group(0) @binding(5) var<storage, read> dye_buf: array<vec4<f32>>; // cs_dye_blit only
@group(0) @binding(6) var dye_out: texture_storage_3d<rgba16float, write>; // cs_dye_blit only
// #182 T2: the solver's vorticity buffer (xyz = ω, w = |ω|) — the blit folds a
// soft-mapped |ω| into dye_tex.a so the march can scale its micro-detail by it.
@group(0) @binding(7) var<storage, read> curl_buf: array<vec4<f32>>; // cs_dye_blit only

// ----- group(1): IBL maps (same layout as cube.wgsl; only irradiance is used) -----
@group(1) @binding(0) var irradiance_tex: texture_2d<f32>;
@group(1) @binding(3) var ibl_samp: sampler;

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.15915494, 0.31830989);
const SHADOW_STEPS: i32 = 5;

fn rotate_y(d: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
}

fn dir_to_equirect_uv(dir: vec3<f32>) -> vec2<f32> {
    let d = normalize(dir);
    var uv = vec2<f32>(atan2(d.z, d.x), asin(clamp(d.y, -1.0, 1.0)));
    uv = uv * INV_ATAN + vec2<f32>(0.5, 0.5);
    return uv;
}

// Henyey–Greenstein phase function; cos_t = dot(view ray, dir to light) so
// g > 0 brightens the medium when looking toward the light (forward scatter).
fn hg_phase(cos_t: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = 1.0 + g2 - 2.0 * g * cos_t;
    return (1.0 - g2) / (4.0 * PI * pow(max(denom, 1e-4), 1.5));
}

// Dye sample: rgb = colour × amount; density is the max component. The texture
// alpha carries a soft-mapped |vorticity| (folded in by the blit) that scales
// the render-time micro-detail (#182 T2).
fn sample_dye4(p: vec3<f32>) -> vec4<f32> {
    let uvw = (p - u.bmin.xyz) / (u.bmax.xyz - u.bmin.xyz);
    return textureSampleLevel(dye_tex, lin_samp, uvw, 0.0);
}
fn sample_dye(p: vec3<f32>) -> vec3<f32> {
    return sample_dye4(p).rgb;
}
fn dye_density(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b));
}

// #182 T2 — render-time micro-detail: an analytic divergence-free "curl noise"
// (the curl of a sinusoidal vector potential, two rotated octaves; each octave
// G(p) = Rᵀ·F(s·R·p) of a divergence-free F stays divergence-free). The march
// perturbs its SAMPLE positions by this, scaled by the local |ω| — cheap
// wavelet-turbulence-flavoured detail where the flow actually swirls, so a
// coarse grid reads finer than it is.
fn curl_base(p: vec3<f32>, t: f32) -> vec3<f32> {
    // curl of A = (sin(y + t), sin(z + 1.3t), sin(x + 1.7t)).
    return -vec3<f32>(cos(p.z + 1.3 * t), cos(p.x + 1.7 * t), cos(p.y + t));
}
fn detail_curl(p: vec3<f32>, t: f32) -> vec3<f32> {
    // An arbitrary orthonormal rotation to decorrelate the second octave.
    let r = mat3x3<f32>(
        vec3<f32>(0.36, 0.48, -0.8),
        vec3<f32>(-0.8, 0.6, 0.0),
        vec3<f32>(0.48, 0.64, 0.6),
    );
    let o1 = curl_base(p, t);
    let o2 = transpose(r) * curl_base(2.3 * (r * p) + vec3<f32>(11.7, 5.3, 27.1), 1.6 * t);
    return o1 + 0.5 * o2;
}

// ===================== compute: dye buffer → 3D texture =====================
@compute @workgroup_size(4, 4, 4)
fn cs_dye_blit(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = vec3<u32>(u.grid.xyz);
    if (gid.x >= res.x || gid.y >= res.y || gid.z >= res.z) {
        return;
    }
    let i = (gid.z * res.y + gid.y) * res.x + gid.x;
    // a = soft-mapped |vorticity| (0..1) for the march's micro-detail scale.
    let w = 1.0 - exp(-curl_buf[i].w * 0.5);
    textureStore(dye_out, vec3<i32>(gid), vec4<f32>(dye_buf[i].rgb, w));
}

// ===================== vertex (fullscreen triangle + ray) =====================
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) rd: vec3<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> VsOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    let ndc = uv * 2.0 - vec2<f32>(1.0);
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    let near = u.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = u.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    out.rd = (far.xyz / far.w) - (near.xyz / near.w);
    return out;
}

// World-space distance along the pixel's ray to the scene surface at `full_px`
// (reconstructed from the depth prepass). 1e30 = no occluder (sky / no depth).
fn scene_t(full_px: vec2<i32>, ro: vec3<f32>) -> f32 {
    if (u.cam.w < 0.5) {
        return 1e30;
    }
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let px = clamp(full_px, vec2<i32>(0), dims - vec2<i32>(1));
    let d = textureLoad(depth_tex, px, 0);
    if (d >= 1.0) {
        return 1e30;
    }
    let uvpx = (vec2<f32>(px) + vec2<f32>(0.5)) / vec2<f32>(dims);
    let ndc = vec2<f32>(uvpx.x * 2.0 - 1.0, 1.0 - uvpx.y * 2.0);
    let wp = u.inv_vp * vec4<f32>(ndc, d, 1.0);
    return length(wp.xyz / wp.w - ro);
}

// ---- #182 T4 "the GI lights the smoke": the probe-GI uniform (the same
// GiUniform the cube shader reads) + the VXGI radiance volume. Both sampled
// per march step as extra in-scatter, so the ink responds to the GI and
// Voxel GI toggles like any other material.
struct GiUniform {
    bbox_min: vec4<f32>,
    bbox_max: vec4<f32>,
    info: vec4<f32>, // x=enabled, y=intensity, z=grid_dim, w=falloff
    probes: array<vec4<f32>, 648>,
};
@group(0) @binding(10) var<uniform> gi: GiUniform;
@group(0) @binding(11) var vxgi_tex: texture_3d<f32>;
@group(0) @binding(12) var vxgi_samp: sampler;

// Flat (L0) probe irradiance, trilinear over the 6³ grid (slot layout matches
// math::compute_gi_probes: ((z·dim + y)·dim + x)·3 + channel).
fn gi_l0_at(x: i32, y: i32, z: i32, dim: i32) -> vec3<f32> {
    let c = clamp(vec3<i32>(x, y, z), vec3<i32>(0), vec3<i32>(dim - 1));
    let base = ((c.z * dim + c.y) * dim + c.x) * 3;
    return vec3<f32>(gi.probes[base].x, gi.probes[base + 1].x, gi.probes[base + 2].x);
}

fn gi_l0(p: vec3<f32>) -> vec3<f32> {
    if (gi.info.x < 0.5 || gi.info.y <= 0.0) {
        return vec3<f32>(0.0);
    }
    let dim = i32(gi.info.z);
    let ext = max(gi.bbox_max.xyz - gi.bbox_min.xyz, vec3<f32>(1e-3));
    let g = (p - gi.bbox_min.xyz) / ext * f32(dim) - vec3<f32>(0.5);
    let gc = clamp(g, vec3<f32>(0.0), vec3<f32>(f32(dim - 1)));
    let i0 = vec3<i32>(floor(gc));
    let f = fract(gc);
    let x00 = mix(gi_l0_at(i0.x, i0.y, i0.z, dim), gi_l0_at(i0.x + 1, i0.y, i0.z, dim), f.x);
    let x10 = mix(gi_l0_at(i0.x, i0.y + 1, i0.z, dim), gi_l0_at(i0.x + 1, i0.y + 1, i0.z, dim), f.x);
    let x01 = mix(gi_l0_at(i0.x, i0.y, i0.z + 1, dim), gi_l0_at(i0.x + 1, i0.y, i0.z + 1, dim), f.x);
    let x11 = mix(gi_l0_at(i0.x, i0.y + 1, i0.z + 1, dim), gi_l0_at(i0.x + 1, i0.y + 1, i0.z + 1, dim), f.x);
    // × intensity, matching cube.wgsl's gi_irradiance — the GI intensity
    // slider is the same lever on the fluid as on the geometry.
    return max(mix(mix(x00, x10, f.y), mix(x01, x11, f.y), f.z), vec3<f32>(0.0)) * gi.info.y;
}

// World-space bounce radiance from the VXGI volume (the world-space twin of
// the screen-space gather geometry gets). 0 outside the volume / when off.
fn vxgi_bounce(p: vec3<f32>) -> vec3<f32> {
    if (u.vgi0.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let ext = max(u.vgi1.xyz - u.vgi0.xyz, vec3<f32>(1e-5));
    let uvw = (p - u.vgi0.xyz) / ext;
    // Soft volume edge: fade over the outer 8% instead of a hard cubic cut
    // (negative outside → 0, so this also rejects out-of-volume points).
    let m3 = min(uvw, vec3<f32>(1.0) - uvw);
    let edge = smoothstep(0.0, 0.08, min(m3.x, min(m3.y, m3.z)));
    if (edge <= 0.0) {
        return vec3<f32>(0.0);
    }
    return textureSampleLevel(vxgi_tex, vxgi_samp, uvw, 0.0).rgb * (u.vgi0.w * edge);
}

// #182 T4 "geometry shades the smoke": the scene shadow map + comparison
// sampler (group 0 bindings 8/9 — the same map cube.wgsl PCF-samples).
@group(0) @binding(8) var scene_shadow_map: texture_depth_2d;
@group(0) @binding(9) var scene_shadow_samp: sampler_comparison;

// Scene-shadow factor at a march sample: 1 when off / outside the map. Uses
// the *Level* compare variant — the march loop is non-uniform control flow.
fn scene_shadow(p: vec3<f32>) -> f32 {
    let amt = u.sizes.w;
    if (amt <= 0.0) {
        return 1.0;
    }
    let lc = u.light_vp * vec4<f32>(p, 1.0);
    if (lc.w <= 0.0) {
        return 1.0;
    }
    let ndc = lc.xyz / lc.w;
    if (ndc.z <= 0.0 || ndc.z >= 1.0) {
        return 1.0;
    }
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return 1.0;
    }
    let vis = textureSampleCompareLevel(scene_shadow_map, scene_shadow_samp, uv, ndc.z - 0.002);
    return mix(1.0, vis, clamp(amt, 0.0, 1.0));
}

// Short light-march toward the key light: accumulated dye density → the
// self-shadow transmittance (silver linings on the lit rim of a plume).
fn key_shadow(p: vec3<f32>, l: vec3<f32>, extinction: f32) -> f32 {
    let diag = length(u.bmax.xyz - u.bmin.xyz);
    let sstep = diag * 0.06;
    var acc = 0.0;
    for (var i = 1; i <= SHADOW_STEPS; i = i + 1) {
        let q = p + l * (sstep * f32(i));
        acc = acc + dye_density(sample_dye(q));
    }
    return exp(-acc * extinction * sstep);
}

// ===================== fragment: the ink march =====================
@fragment
fn fs_ink(in: VsOut) -> @location(0) vec4<f32> {
    let ro = u.cam.xyz;
    let rd = normalize(in.rd);

    // Ray vs the fluid AABB (slab test).
    let bmin = u.bmin.xyz;
    let bmax = u.bmax.xyz;
    let inv = 1.0 / rd;
    let ta = (bmin - ro) * inv;
    let tb = (bmax - ro) * inv;
    let tsmall = min(ta, tb);
    let tbig = max(ta, tb);
    let tmin = max(max(tsmall.x, tsmall.y), max(tsmall.z, 0.0));
    var tmax = min(min(tbig.x, tbig.y), tbig.z);

    // Clamp the march at the scene surface (the depth prepass): ink behind
    // geometry doesn't show through it.
    let full_px = vec2<i32>(in.clip.xy * u.texel.z);
    tmax = min(tmax, scene_t(full_px, ro));
    if (tmax <= tmin) {
        return vec4<f32>(0.0);
    }

    let extinction = max(u.bmin.w, 0.0);
    let scatter = max(u.scat.x, 0.0);
    let emissive = max(u.scat.y, 0.0);
    let g = clamp(u.scat.z, -0.95, 0.95);
    let ambient = max(u.scat.w, 0.0);
    let env_rot = u.envt.x;
    let etint = u.envt.yzw;
    let l_key = normalize(u.key.xyz);
    let key_i = u.key.w;
    let steps = max(i32(u.bmax.w), 8);
    let dt = (tmax - tmin) / f32(steps);

    // Per-ray constants: the HG lobe and the ambient in-scatter (the IBL
    // irradiance sampled along the view ray — the sky colours the ink).
    let phase = hg_phase(dot(rd, l_key), g);
    let amb_irr = textureSampleLevel(
        irradiance_tex, ibl_samp, dir_to_equirect_uv(rotate_y(rd, env_rot)), 0.0
    ).rgb * (ambient) * etint;

    // #182 T2 micro-detail: perturbation amplitude ≈ a cell and a half at full
    // detail, frequency ≈ a 2-cell period, drifting on the clock.
    let detail = max(u.grid.w, 0.0);
    let cell = (u.bmax.x - u.bmin.x) / max(u.grid.x, 1.0);
    let dfreq = 3.1 / max(cell, 1e-4);
    let time = u.texel.w;
    // Reveal (like the vector-field reveal): a soft density knee that culls the
    // dilute haze — fully visible above `reveal`, gone below half of it — so
    // the dense vortex filaments inside show through instead of being occluded
    // by a uniform fog crust.
    let reveal = u.sizes.z;

    var t = tmin + dt * 0.5;
    var trans = 1.0;
    var accum = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        var p = ro + rd * t;
        if (detail > 0.0) {
            // Scale the swirl by the local |ω| (dye_tex.a) so detail appears
            // where the flow actually turns, not as a uniform wobble.
            let w = sample_dye4(p).a;
            p = p + detail_curl(p * dfreq, time) * (detail * cell * 1.5 * w);
        }
        var c = sample_dye(p);
        var d = dye_density(c);
        if (reveal > 0.0) {
            let vis = smoothstep(reveal * 0.5, reveal, d);
            c = c * vis;
            d = d * vis;
        }
        if (d > 1e-3) {
            let sh = key_shadow(p, l_key, extinction) * scene_shadow(p);
            // #182 T4: bounce in-scatter — probe GI + the VXGI radiance
            // volume, both spatial (per step). The GI toggles light the ink.
            let light = vec3<f32>(key_i * phase * sh) + amb_irr + gi_l0(p) + vxgi_bounce(p);
            let src = c * (scatter * light) + c * emissive;
            accum = accum + trans * src * dt;
            trans = trans * exp(-d * extinction * dt);
            if (trans < 0.01) {
                break;
            }
        }
        t = t + dt;
    }
    return vec4<f32>(accum, clamp(1.0 - trans, 0.0, 1.0));
}

// ===================== fragment: depth-aware upsample/composite =====================
// Runs at full render resolution over the HDR buffer (blend One /
// OneMinusSrcAlpha, RGB-only write). At full-res ink it degenerates to a
// pass-through; at half-res the joint-bilateral weights keep geometry edges
// crisp (an ink texel across a depth discontinuity is down-weighted).
@fragment
fn fs_upsample(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.clip.xy * u.texel.xy;
    if (u.cam.w < 0.5 || u.texel.z <= 1.001) {
        // No depth to be aware of (or no upscale) → plain bilinear.
        return textureSampleLevel(ink_tex, lin_samp, uv, 0.0);
    }

    let dims = vec2<i32>(textureDimensions(depth_tex));
    let px_c = clamp(vec2<i32>(in.clip.xy), vec2<i32>(0), dims - vec2<i32>(1));
    let d_c = textureLoad(depth_tex, px_c, 0);

    // Manual bilinear over the 4 surrounding ink texels, each weight scaled by
    // depth similarity (sampled from the full-res depth at the texel's centre).
    let ink_size = u.sizes.xy;
    let q = uv * ink_size - vec2<f32>(0.5);
    let base = floor(q);
    let f = q - base;
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var j = 0; j < 2; j = j + 1) {
        for (var i = 0; i < 2; i = i + 1) {
            let ti = clamp(base + vec2<f32>(f32(i), f32(j)),
                           vec2<f32>(0.0), ink_size - vec2<f32>(1.0));
            let wb = (select(1.0 - f.x, f.x, i == 1)) * (select(1.0 - f.y, f.y, j == 1));
            // The full-res pixel under this ink texel's centre.
            let fp = clamp(vec2<i32>((ti + vec2<f32>(0.5)) * u.texel.z),
                           vec2<i32>(0), dims - vec2<i32>(1));
            let d_i = textureLoad(depth_tex, fp, 0);
            let wz = 1.0 / (1e-3 + abs(d_c - d_i) * 64.0);
            let w = wb * wz;
            sum = sum + textureLoad(ink_tex, vec2<i32>(ti), 0) * w;
            wsum = wsum + w;
        }
    }
    if (wsum <= 1e-6) {
        return textureSampleLevel(ink_tex, lin_samp, uv, 0.0);
    }
    return sum / wsum;
}

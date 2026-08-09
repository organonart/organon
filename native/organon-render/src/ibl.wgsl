// IBL precompute (split-sum). Fullscreen-triangle passes into Rgba16Float equirect
// targets. All math LINEAR; no tonemap (these are HDR intermediates). The
// direction<->equirect convention MUST match cube.wgsl / skybox.wgsl.

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.15915494, 0.31830989); // (1/2π, 1/π)

fn dir_to_uv(d: vec3<f32>) -> vec2<f32> {
    let uv = vec2<f32>(atan2(d.z, d.x), asin(clamp(d.y, -1.0, 1.0)));
    return uv * INV_ATAN + vec2<f32>(0.5, 0.5);
}
fn uv_to_dir(uv: vec2<f32>) -> vec3<f32> {
    let lon = (uv.x - 0.5) * 2.0 * PI;   // -π..π
    let lat = (uv.y - 0.5) * PI;         // -π/2..π/2
    let cl = cos(lat);
    return vec3<f32>(cl * cos(lon), sin(lat), cl * sin(lon));
}

struct FsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> FsOut {
    let uv = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u)); // (0,0)(2,0)(0,2)
    var o: FsOut;
    o.pos = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(uv.x, 1.0 - uv.y); // flip so uv.y=0 is the north pole (top row)
    return o;
}

// ===== 1. Procedural sky =====
struct SkyU { top: vec4<f32>, horizon: vec4<f32>, bottom: vec4<f32>, intensity: vec4<f32> };
@group(0) @binding(0) var<uniform> sky: SkyU;
@fragment
fn fs_sky(in: FsOut) -> @location(0) vec4<f32> {
    let d = uv_to_dir(in.uv);
    let t = d.y;
    var col: vec3<f32>;
    if (t >= 0.0) { col = mix(sky.horizon.rgb, sky.top.rgb, pow(t, 0.55)); }
    else          { col = mix(sky.horizon.rgb, sky.bottom.rgb, pow(-t, 0.55)); }
    return vec4<f32>(col * sky.intensity.x, 1.0);
}

// ===== 1b. Physically based atmosphere (#100) =====
// Nishita single-scattering — Rayleigh (∝1/λ⁴, the blue) + Mie (aerosol forward-
// scatter, the sun halo) integrated along the view ray, with a nested light-ray
// optical-depth march for transmittance. Earth-like constants in KILOMETRES (f32
// precision near the 6360 km planet radius). Self-contained (no textures) so it is
// uniformity-safe in any control flow; identical to the copy in terrain.wgsl.
// Derives — at all sun elevations — the blue zenith, the full sunset gradient, the
// Mie aureole hugging the sun, the reddened low sun, and true blue-hour twilight.
const ATM_PLANET_R: f32 = 6360.0;
const ATM_TOP_R: f32    = 6420.0;
const ATM_H_RAY: f32 = 8.0;
const ATM_H_MIE: f32 = 1.2;
const ATM_BETA_RAY: vec3<f32> = vec3<f32>(5.8e-3, 13.5e-3, 33.1e-3); // per km
const ATM_BETA_MIE: f32 = 21.0e-3;                                   // per km
const ATM_VIEW_STEPS: i32 = 16;
const ATM_LIGHT_STEPS: i32 = 8;

// Far intersection t of ray (o,d) with the atmosphere top sphere (o is inside it).
fn atm_ray_top(o: vec3<f32>, d: vec3<f32>) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - ATM_TOP_R * ATM_TOP_R;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    return -b + sqrt(disc);
}
// Near intersection t with the planet (>0 ⇒ the ray meets the ground ahead).
fn atm_ray_planet(o: vec3<f32>, d: vec3<f32>) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - ATM_PLANET_R * ATM_PLANET_R;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    return -b - sqrt(disc);
}

fn atmosphere(rd: vec3<f32>, sun: vec3<f32>, turbidity: f32, mie_g: f32,
              sun_i: f32, ground_albedo: f32, rayleigh_s: f32) -> vec3<f32> {
    let o = vec3<f32>(0.0, ATM_PLANET_R + 1.0, 0.0); // observer ~1 km above ground
    var t_max = atm_ray_top(o, rd);
    if (t_max <= 0.0) { return vec3<f32>(0.0); }
    let t_g = atm_ray_planet(o, rd);
    if (t_g > 0.0) { t_max = min(t_max, t_g); } // don't march through the planet

    let beta_r = ATM_BETA_RAY * max(rayleigh_s, 0.0);
    let beta_m = vec3<f32>(ATM_BETA_MIE * max(turbidity, 0.0));
    let mu = dot(rd, sun);
    let g = clamp(mie_g, 0.0, 0.95);
    let phase_r = (3.0 / (16.0 * PI)) * (1.0 + mu * mu);
    let mdenom = pow(max(1.0 + g * g - 2.0 * g * mu, 1e-4), 1.5);
    let phase_m = (3.0 / (8.0 * PI)) * ((1.0 - g * g) * (1.0 + mu * mu)) / ((2.0 + g * g) * mdenom);

    let seg = t_max / f32(ATM_VIEW_STEPS);
    var od_r = 0.0;
    var od_m = 0.0;
    var sum_r = vec3<f32>(0.0);
    var sum_m = vec3<f32>(0.0);
    for (var i = 0; i < ATM_VIEW_STEPS; i = i + 1) {
        let p = o + rd * (seg * (f32(i) + 0.5));
        let h = length(p) - ATM_PLANET_R;
        let hr = exp(-h / ATM_H_RAY) * seg;
        let hm = exp(-h / ATM_H_MIE) * seg;
        od_r = od_r + hr;
        od_m = od_m + hm;
        // Transmittance from this sample toward the sun (skip when the sun is below
        // the local horizon — the planet shadows it).
        var lit = true;
        var od_lr = 0.0;
        var od_lm = 0.0;
        if (atm_ray_planet(p, sun) > 0.0) {
            lit = false;
        } else {
            let tl = atm_ray_top(p, sun);
            let segl = tl / f32(ATM_LIGHT_STEPS);
            for (var j = 0; j < ATM_LIGHT_STEPS; j = j + 1) {
                let pl = p + sun * (segl * (f32(j) + 0.5));
                let hl = length(pl) - ATM_PLANET_R;
                od_lr = od_lr + exp(-hl / ATM_H_RAY) * segl;
                od_lm = od_lm + exp(-hl / ATM_H_MIE) * segl;
            }
        }
        if (lit) {
            let tau = beta_r * (od_r + od_lr) + beta_m * 1.1 * (od_m + od_lm);
            let attn = exp(-tau);
            sum_r = sum_r + attn * hr;
            sum_m = sum_m + attn * hm;
        }
    }
    var col = sun_i * (sum_r * beta_r * phase_r + sum_m * beta_m * phase_m);
    // Cheap multiple-scatter / ground-bounce ambient lift (single scattering alone
    // leaves the zenith too dark); fades out at night with the sun height.
    col = col + vec3<f32>(0.42, 0.52, 0.70) * (ground_albedo * 0.02 * sun_i)
              * smoothstep(-0.1, 0.3, sun.y);
    return max(col, vec3<f32>(0.0));
}

// AtmosU shares group(0)/binding(0) with `sky`/`src_tex` — only one entry point
// uses each, which the module already relies on (see fs_sky vs the sampling passes).
struct AtmosU {
    sun: vec4<f32>, // xyz = unit dir TO sun, w = sun intensity
    p0: vec4<f32>,  // turbidity, mie_g, ground_albedo, rayleigh
    p1: vec4<f32>,  // exposure, _, _, _
};
@group(0) @binding(0) var<uniform> atm: AtmosU;
@fragment
fn fs_atmosphere(in: FsOut) -> @location(0) vec4<f32> {
    let dir = uv_to_dir(in.uv);
    let sun = normalize(atm.sun.xyz);
    var col: vec3<f32>;
    if (dir.y < -0.02) {
        // Lower hemisphere → a dim ground bounce (horizon sky × albedo) so the IBL
        // irradiance from below isn't pitch black.
        let horiz = atmosphere(normalize(vec3<f32>(dir.x, 0.03, dir.z)), sun,
                               atm.p0.x, atm.p0.y, atm.sun.w, atm.p0.z, atm.p0.w);
        col = horiz * atm.p0.z * 0.6;
    } else {
        col = atmosphere(dir, sun, atm.p0.x, atm.p0.y, atm.sun.w, atm.p0.z, atm.p0.w);
    }
    return vec4<f32>(col * atm.p1.x, 1.0);
}

// ===== shared source binding for sampling passes (group(0)) =====
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
struct DownU { src_lod: f32, _p0: f32, _p1: f32, _p2: f32 };
struct PreU  { roughness: f32, src_w: f32, src_h: f32, _p: f32 };

// ===== 2. Box-filter downsample (mip generation) =====
@group(0) @binding(2) var<uniform> downu: DownU;
@fragment
fn fs_downsample(in: FsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(src_tex, 0)); // src view = single mip
    let texel = 1.0 / dims;
    var acc = vec4<f32>(0.0);
    let offs = array<vec2<f32>, 4>(
        vec2<f32>(-0.25,-0.25), vec2<f32>(0.25,-0.25),
        vec2<f32>(-0.25, 0.25), vec2<f32>(0.25, 0.25));
    for (var i = 0; i < 4; i = i + 1) {
        acc = acc + textureSampleLevel(src_tex, src_samp, in.uv + offs[i] * texel, 0.0);
    }
    return acc * 0.25;
}

// ===== shared math =====
fn build_tbn(n: vec3<f32>) -> mat3x3<f32> {
    let up = select(vec3<f32>(0.0,1.0,0.0), vec3<f32>(1.0,0.0,0.0), abs(n.y) > 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return mat3x3<f32>(t, b, n);
}

// ===== 3. Irradiance (cosine-weighted hemisphere integral) =====
@fragment
fn fs_irradiance(in: FsOut) -> @location(0) vec4<f32> {
    let n = uv_to_dir(in.uv);
    let tbn = build_tbn(n);
    var irr = vec3<f32>(0.0);
    var samples = 0.0;
    // 0.05 rad steps (#174 T2 — was 0.025): 4× fewer taps (~2M vs ~8M per texel
    // row group). The source is already sampled at mip 4 (heavily band-limited)
    // and the target is only 64×32, so the difference is invisible — but the bake
    // runs on the render thread at .hdr load / atmosphere re-bake, where it was a
    // visible frame hitch.
    let d_phi = 0.05;
    let d_theta = 0.05;
    var phi = 0.0;
    loop {
        if (phi >= 2.0 * PI) { break; }
        var theta = 0.0;
        loop {
            if (theta >= 0.5 * PI) { break; }
            let st = sin(theta); let ct = cos(theta);
            let tangent = vec3<f32>(st * cos(phi), st * sin(phi), ct);
            let world = tbn * tangent;
            // sample env at a low mip to denoise the integral
            let c = textureSampleLevel(src_tex, src_samp, dir_to_uv(world), 4.0).rgb;
            irr = irr + c * ct * st;     // cos weight * sin jacobian
            samples = samples + 1.0;
            theta = theta + d_theta;
        }
        phi = phi + d_phi;
    }
    // PI is the hemisphere solid-angle constant (LearnOpenGL normalization).
    // The cube shader does diffuse = irradiance * albedo with NO further /PI.
    // DO NOT remove this PI.
    irr = PI * irr / max(samples, 1.0);
    return vec4<f32>(irr, 1.0);
}

// ===== 4. Prefiltered specular (GGX importance sampling) =====
@group(0) @binding(2) var<uniform> preu: PreU;
fn radical_inverse_vdc(bits_in: u32) -> f32 {
    var bits = bits_in;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}
fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), radical_inverse_vdc(i));
}
fn importance_ggx(xi: vec2<f32>, n: vec3<f32>, rough: f32) -> vec3<f32> {
    let a = rough * rough;
    let phi = 2.0 * PI * xi.x;
    let denom = max(1.0 + (a*a - 1.0) * xi.y, 1e-6); // NaN guard at rough=0, xi.y→1
    let ct = sqrt((1.0 - xi.y) / denom);
    let st = sqrt(max(1.0 - ct*ct, 0.0));
    let h_t = vec3<f32>(cos(phi)*st, sin(phi)*st, ct);
    let tbn = build_tbn(n);
    return tbn * h_t;
}
@fragment
fn fs_prefilter(in: FsOut) -> @location(0) vec4<f32> {
    let n = uv_to_dir(in.uv);
    let v = n; // split-sum assumption: V = R = N
    let rough = preu.roughness;

    // roughness 0 → crisp mirror: direct env tap, skip the integral. Sample at the
    // mip matching the source→target minification (#174 T1/T2: a raw lod-0 tap of
    // a 4096-wide env into the 1024-wide prefilter base is a 4× undersample —
    // sharp reflections shimmered). 1024 = env.rs::PRE_W (the prefilter mip-0 width).
    if (rough <= 0.0) {
        let lod = max(log2(preu.src_w / 1024.0), 0.0);
        return vec4<f32>(textureSampleLevel(src_tex, src_samp, dir_to_uv(n), lod).rgb, 1.0);
    }

    let SAMPLES = 128u;
    var color = vec3<f32>(0.0);
    var weight = 0.0;
    // EQUIRECT per-texel solid angle at the equator (NOT the cubemap 4π/6 form).
    let sa_texel = (2.0 * PI / preu.src_w) * (PI / preu.src_h);
    for (var i = 0u; i < SAMPLES; i = i + 1u) {
        let xi = hammersley(i, SAMPLES);
        let h = importance_ggx(xi, n, rough);
        let l = normalize(2.0 * dot(v, h) * h - v);
        let ndl = dot(n, l);
        if (ndl > 0.0) {
            let ndh = max(dot(n, h), 0.0);
            let a = rough * rough;
            let d = (a*a) / max(PI * pow(ndh*ndh*(a*a-1.0)+1.0, 2.0), 1e-4);
            let pdf = (d * ndh / (4.0 * max(dot(h, v), 1e-4))) + 1e-4;
            let sa_sample = 1.0 / (f32(SAMPLES) * pdf + 1e-4);
            let lod = max(0.5 * log2(sa_sample / sa_texel), 0.0);
            color = color + textureSampleLevel(src_tex, src_samp, dir_to_uv(l), lod).rgb * ndl;
            weight = weight + ndl;
        }
    }
    return vec4<f32>(color / max(weight, 1e-4), 1.0);
}

// ===== 5. BRDF integration LUT (env-independent) =====
fn geom_schlick_ggx_ibl(ndv: f32, rough: f32) -> f32 {
    let k = (rough * rough) / 2.0;
    return ndv / (ndv * (1.0 - k) + k);
}
@fragment
fn fs_brdf(in: FsOut) -> @location(0) vec4<f32> {
    let ndv = max(in.uv.x, 1e-4);
    let rough = in.uv.y;
    let v = vec3<f32>(sqrt(1.0 - ndv*ndv), 0.0, ndv);
    let n = vec3<f32>(0.0, 0.0, 1.0);
    var a_sum = 0.0; var b_sum = 0.0;
    let SAMPLES = 512u;
    for (var i = 0u; i < SAMPLES; i = i + 1u) {
        let xi = hammersley(i, SAMPLES);
        let h = importance_ggx(xi, n, rough);
        let l = normalize(2.0 * dot(v, h) * h - v);
        let ndl = max(l.z, 0.0);
        let ndh = max(h.z, 0.0);
        let vdh = max(dot(v, h), 0.0);
        if (ndl > 0.0) {
            let g = geom_schlick_ggx_ibl(ndl, rough) * geom_schlick_ggx_ibl(ndv, rough);
            let g_vis = (g * vdh) / max(ndh * ndv, 1e-4);
            let fc = pow(1.0 - vdh, 5.0);
            a_sum = a_sum + (1.0 - fc) * g_vis;
            b_sum = b_sum + fc * g_vis;
        }
    }
    return vec4<f32>(a_sum / f32(SAMPLES), b_sum / f32(SAMPLES), 0.0, 1.0);
}

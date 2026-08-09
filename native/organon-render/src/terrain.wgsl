// Organic Math — infinite terrain backdrop.
//
// A self-contained raymarched landscape drawn as a parallax BACKDROP behind the
// generator scene. A fullscreen pass reconstructs a view ray from the camera's
// inverse view-projection (so the horizon tracks the orbit camera), sphere-traces
// a value-noise fBm heightfield from a synthetic "fly" camera, and shades it with
// a single key sun + sky-dome ambient + height/slope snow + exponential distance
// fog. It outputs LINEAR HDR (no tonemap/gamma here — the composite owns that)
// with alpha = 0 so it reads as background coverage, and writes the far-plane clip
// depth so the generator geometry always composites in front.
//
// This is an original implementation of standard, widely documented techniques
// (fBm value noise, heightfield sphere-tracing, Lambert + hemispheric ambient,
// height/slope material blends, exponential fog); it shares no code with any
// specific shader. The lattice value noise is read from a procedurally synthesized
// 256x256 tile (terrain.rs::gen_noise) — swap the tile and the landscape changes
// character. Only textureLoad is used (never textureSample), so the noise needs no
// filtering and the raymarch loops are free of any uniformity constraint.

struct TerrainU {
    inv_view_proj: mat4x4<f32>,
    ro: vec4<f32>,  // synthetic fly-camera world position (xyz); w unused
    sun: vec4<f32>, // xyz = unit direction TO the sun; w = sun intensity
    // height, snow_level (0..1 of height), fog_density, ridged (0/1)
    p0: vec4<f32>,
    // brightness (overall backdrop gain), haze (horizon), time (cloud drift), _
    p1: vec4<f32>,
    // march_steps, march_octaves, res_divisor (1/2/4/8 for the upscale blit),
    // palette id (rock/veg/snow colour set).
    p2: vec4<f32>,
    // emissive strength, scattering strength, god-ray strength, water_on (0/1).
    p3: vec4<f32>,
    // water_level (world y), water_hue (0..1), water_ripple strength, _.
    p4: vec4<f32>,
    // Physically based atmosphere (#100). atmos = [enabled(0/1), turbidity, mie_g,
    // sun_intensity]; atmos2 = [ground_albedo, exposure, aerial_strength, rayleigh].
    // When atmos.x > 0.5 the sky + aerial perspective are derived from single
    // scattering (the same integral) instead of the hand-tuned gradient/fog.
    atmos: vec4<f32>,
    atmos2: vec4<f32>,
    // #102 volumetric clouds. clouds = [enabled, coverage, density, base_alt];
    // clouds2 = [thickness, march_steps, detail, drift_speed];
    // clouds3 = [hg, absorption, shadow_strength, ambient].
    clouds: vec4<f32>,
    clouds2: vec4<f32>,
    clouds3: vec4<f32>,
    // #102B FFT ocean. ocean = [enabled, level(world y), foam, glitter];
    // ocean2 = [hue, depth_absorption, tile_size, land_on(0/1)].
    ocean: vec4<f32>,
    ocean2: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: TerrainU;
@group(1) @binding(0) var noise_tex: texture_2d<f32>;
// #102B ocean tile (group 2): filterable RGBA16F (normal.x, normal.z, height, foam),
// sampled with Repeat so it tiles across the world.
@group(2) @binding(0) var ocean_tex: texture_2d<f32>;
@group(2) @binding(1) var ocean_samp: sampler;
// group(3) = the half-resolution terrain colour, sampled by `fs_blit` on the upscale
// path. Unused (but declared) by the raymarch entry points.
@group(3) @binding(0) var src_tex: texture_2d<f32>;

// --- These constants must stay in sync with terrain.rs (CPU camera height). ---
const FREQ: f32 = 0.0016;          // world.xz -> noise-domain frequency
const ROT: mat2x2<f32> = mat2x2<f32>(0.8, -0.6, 0.6, 0.8); // per-octave domain rotation
const SHADOW_STEPS: i32 = 48;
const CLOUD_H: f32 = 900.0;        // cloud-plane altitude (terrain units)
// March steps + octaves are now editor dials (u.p2.xy); normal/shadow octaves
// derive from the march octaves (crisper normal, cheaper shadow), clamped sane.
fn march_steps() -> i32 { return clamp(i32(u.p2.x), 16, 400); }
fn march_oct()   -> i32 { return clamp(i32(u.p2.y), 2, 9); }
fn normal_oct()  -> i32 { return min(march_oct() + 2, 9); }
fn shadow_oct()  -> i32 { return max(march_oct() - 2, 3); }

// One lattice cell, wrapped to the 256x256 tile (negative coords wrap via two's
// complement, matching the CPU `& 255`).
fn cell(ip: vec2<i32>) -> f32 {
    let q = ip & vec2<i32>(255, 255);
    return textureLoad(noise_tex, q, 0).r;
}

// Smooth value noise over the lattice tile.
fn vnoise(p: vec2<f32>) -> f32 {
    let ip = vec2<i32>(floor(p));
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    let a = cell(ip + vec2<i32>(0, 0));
    let b = cell(ip + vec2<i32>(1, 0));
    let c = cell(ip + vec2<i32>(0, 1));
    let d = cell(ip + vec2<i32>(1, 1));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

// Normalized fBm in ~[0,1]. `ridged` folds each octave into sharp alpine spines.
fn fbm(p0: vec2<f32>, octaves: i32, ridged: f32) -> f32 {
    var p = p0;
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    for (var i = 0; i < octaves; i = i + 1) {
        var n = vnoise(p);
        if (ridged > 0.5) {
            n = 1.0 - abs(2.0 * n - 1.0);
        }
        sum = sum + amp * n;
        norm = norm + amp;
        amp = amp * 0.5;
        p = ROT * p * 2.0;
    }
    return sum / norm;
}

fn terrain_h(xz: vec2<f32>, octaves: i32) -> f32 {
    return u.p0.x * fbm(xz * FREQ, octaves, u.p0.w);
}

// Sphere-trace the heightfield. Returns the hit distance, or `tmax` on a miss.
fn raymarch(ro: vec3<f32>, rd: vec3<f32>, tmax: f32) -> f32 {
    var t = 1.0;
    let steps = march_steps();
    let oct = march_oct();
    for (var i = 0; i < steps; i = i + 1) {
        let pos = ro + rd * t;
        let h = pos.y - terrain_h(pos.xz, oct);
        if (h < 0.0015 * t) { break; }
        t = t + 0.45 * h;
        if (t > tmax) { return tmax; }
    }
    return t;
}

fn calc_normal(pos: vec3<f32>, t: f32) -> vec3<f32> {
    let e = max(0.002 * t, 0.05);
    let no = normal_oct();
    let nx = terrain_h(pos.xz - vec2<f32>(e, 0.0), no)
           - terrain_h(pos.xz + vec2<f32>(e, 0.0), no);
    let nz = terrain_h(pos.xz - vec2<f32>(0.0, e), no)
           - terrain_h(pos.xz + vec2<f32>(0.0, e), no);
    return normalize(vec3<f32>(nx, 2.0 * e, nz));
}

// Cheap penumbra: track the closest the shadow ray passes above the terrain.
fn soft_shadow(ro: vec3<f32>, rd: vec3<f32>, maxh: f32) -> f32 {
    var res = 1.0;
    var t = 4.0;
    let step_min = max(u.p0.x * 0.01, 1.0);
    let so = shadow_oct();
    for (var i = 0; i < SHADOW_STEPS; i = i + 1) {
        let p = ro + rd * t;
        let h = p.y - terrain_h(p.xz, so);
        res = min(res, 12.0 * h / t);
        t = t + max(step_min, h);
        if (res < 0.01 || p.y > maxh) { break; }
    }
    return clamp(res, 0.0, 1.0);
}

struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) ndc: vec2<f32> };

@vertex
fn vs_terrain(@builtin(vertex_index) vid: u32) -> VsOut {
    let uv = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    let p = uv * 2.0 - vec2<f32>(1.0, 1.0);
    var out: VsOut;
    out.clip = vec4<f32>(p, 1.0, 1.0); // far plane (clip z = 1) → behind the scene
    out.ndc = p;
    return out;
}

// ===== Physically based atmosphere (#100) — Nishita single scattering =====
// Identical model to ibl.wgsl::atmosphere (km units; Rayleigh + Mie with a nested
// light-ray transmittance march). Self-contained (no textures). Used for the sky
// (rays that miss the terrain) and the aerial-perspective haze when atmos is on.
const ATM_PI: f32 = 3.14159265359;
const ATM_PLANET_R: f32 = 6360.0;
const ATM_TOP_R: f32    = 6420.0;
const ATM_H_RAY: f32 = 8.0;
const ATM_H_MIE: f32 = 1.2;
const ATM_BETA_RAY: vec3<f32> = vec3<f32>(5.8e-3, 13.5e-3, 33.1e-3);
const ATM_BETA_MIE: f32 = 21.0e-3;
const ATM_VIEW_STEPS: i32 = 16;
const ATM_LIGHT_STEPS: i32 = 8;

fn atm_ray_top(o: vec3<f32>, d: vec3<f32>) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - ATM_TOP_R * ATM_TOP_R;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    return -b + sqrt(disc);
}
fn atm_ray_planet(o: vec3<f32>, d: vec3<f32>) -> f32 {
    let b = dot(o, d);
    let c = dot(o, o) - ATM_PLANET_R * ATM_PLANET_R;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    return -b - sqrt(disc);
}
fn atmosphere(rd: vec3<f32>, sun: vec3<f32>, turbidity: f32, mie_g: f32,
              sun_i: f32, ground_albedo: f32, rayleigh_s: f32) -> vec3<f32> {
    let o = vec3<f32>(0.0, ATM_PLANET_R + 1.0, 0.0);
    var t_max = atm_ray_top(o, rd);
    if (t_max <= 0.0) { return vec3<f32>(0.0); }
    let t_g = atm_ray_planet(o, rd);
    if (t_g > 0.0) { t_max = min(t_max, t_g); }

    let beta_r = ATM_BETA_RAY * max(rayleigh_s, 0.0);
    let beta_m = vec3<f32>(ATM_BETA_MIE * max(turbidity, 0.0));
    let mu = dot(rd, sun);
    let g = clamp(mie_g, 0.0, 0.95);
    let phase_r = (3.0 / (16.0 * ATM_PI)) * (1.0 + mu * mu);
    let mdenom = pow(max(1.0 + g * g - 2.0 * g * mu, 1e-4), 1.5);
    let phase_m = (3.0 / (8.0 * ATM_PI)) * ((1.0 - g * g) * (1.0 + mu * mu)) / ((2.0 + g * g) * mdenom);

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
    col = col + vec3<f32>(0.42, 0.52, 0.70) * (ground_albedo * 0.02 * sun_i)
              * smoothstep(-0.1, 0.3, sun.y);
    return max(col, vec3<f32>(0.0));
}

// Sky radiance from the physical atmosphere for the terrain pass (clamps downward
// rays to the horizon so the dome below the horizon reads as ground haze).
fn atm_sky(rd: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    var d = rd;
    if (d.y < -0.02) { d = normalize(vec3<f32>(d.x, 0.02, d.z)); }
    let col = atmosphere(d, sun, u.atmos.y, u.atmos.z, u.atmos.w, u.atmos2.x, u.atmos2.w);
    return col * u.atmos2.y; // exposure
}

// Day factor from the sun's height (sun.y = sin elevation): 1 in daylight, 0 once
// the sun is below the horizon. Drives the whole sky + ambient toward night so the
// day cycle actually darkens the world (and the starfield becomes visible).
fn day_factor(sun: vec3<f32>) -> f32 {
    return smoothstep(-0.12, 0.10, sun.y);
}

// Clear-sky radiance (no volumetric clouds) for a ray that misses the terrain.
// Linear HDR. `sky_color` wraps this and overlays the #102 cloud layer.
fn clear_sky(ro: vec3<f32>, rd: vec3<f32>, sun: vec3<f32>, sundot: f32) -> vec3<f32> {
    // #100: physically based sky — derived single scattering, correct at every sun
    // angle (blue zenith → sunset gradient → Mie aureole → reddened low sun → blue
    // hour). The hand-tuned gradient below is the fallback when atmosphere is off.
    if (u.atmos.x > 0.5) {
        return atm_sky(rd, sun);
    }
    let day = day_factor(sun);
    // A warm sunset/sunrise band when the sun is near the horizon.
    let dusk = (1.0 - day) * smoothstep(-0.35, 0.18, sun.y);
    // Vertical gradient: deeper blue at the zenith, pale toward the horizon.
    var col = vec3<f32>(0.30, 0.50, 0.85) - rd.y * 0.45;
    col = mix(col, vec3<f32>(0.66, 0.72, 0.82), pow(1.0 - max(rd.y, 0.0), 5.0));
    // Sun: a tight core inside a broad warm halo (gone once the sun sets).
    col = col + vec3<f32>(1.0, 0.78, 0.45) * (0.28 * pow(sundot, 6.0) + 0.30 * pow(sundot, 256.0)) * (day + dusk);
    // Clouds: drift a value-noise sheet across a high plane (guard the horizon). The
    // flat sheet is the fallback — when the #102 volumetric layer is on, `sky_color`
    // replaces it with the raymarched clouds.
    if (u.clouds.x <= 0.5 && rd.y > 0.01) {
        let sc = ro.xz + rd.xz * ((CLOUD_H - ro.y) / rd.y);
        let drift = vec2<f32>(u.p1.z * 12.0, u.p1.z * 4.0);
        let f = fbm((sc + drift) * FREQ * 1.7, 5, 0.0);
        col = mix(col, vec3<f32>(1.05, 1.02, 1.05), 0.55 * smoothstep(0.45, 0.78, f) * smoothstep(0.02, 0.18, rd.y));
    }
    // Horizon haze band.
    col = mix(col, vec3<f32>(0.62, 0.70, 0.85) * (0.7 + 0.3 * u.p1.y), pow(1.0 - max(rd.y, 0.0), 14.0));
    // Fade the lit day sky down to a near-black night sky as the sun sets; add a
    // warm horizon glow at dusk so the transition reads (and faint stars show).
    let night_sky = vec3<f32>(0.002, 0.004, 0.011) * (1.0 - 0.4 * max(rd.y, 0.0));
    col = mix(night_sky, col, day);
    col = col + vec3<f32>(0.55, 0.26, 0.10) * dusk * pow(1.0 - max(rd.y, 0.0), 8.0);
    return col;
}

// ===========================================================================
// #102 Volumetric clouds (Part A). A raymarched cloud layer replacing the flat
// value-noise sheet: a coverage/erosion density from procedural 3D fBm, a light
// march toward the sun for self-shadowing (silver linings + sun-behind glow), a
// Henyey–Greenstein forward-scatter phase, and Beer–Lambert extinction. Composited
// over the (atmosphere or gradient) sky. Self-contained 3D noise (no texture).
//   clouds  = [enabled, coverage, density, base_alt]
//   clouds2 = [thickness, march_steps, detail(erosion), drift_speed]
//   clouds3 = [hg(forward scatter), absorption, shadow_strength(on terrain), ambient]
// ===========================================================================
fn h3(p3: vec3<f32>) -> f32 {
    var p = fract(p3 * 0.1031);
    p = p + dot(p, p.zyx + 31.32);
    return fract((p.x + p.y) * p.z);
}
fn vnoise3(x: vec3<f32>) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let w = f * f * (3.0 - 2.0 * f);
    let c000 = h3(i + vec3<f32>(0.0, 0.0, 0.0));
    let c100 = h3(i + vec3<f32>(1.0, 0.0, 0.0));
    let c010 = h3(i + vec3<f32>(0.0, 1.0, 0.0));
    let c110 = h3(i + vec3<f32>(1.0, 1.0, 0.0));
    let c001 = h3(i + vec3<f32>(0.0, 0.0, 1.0));
    let c101 = h3(i + vec3<f32>(1.0, 0.0, 1.0));
    let c011 = h3(i + vec3<f32>(0.0, 1.0, 1.0));
    let c111 = h3(i + vec3<f32>(1.0, 1.0, 1.0));
    let x00 = mix(c000, c100, w.x);
    let x10 = mix(c010, c110, w.x);
    let x01 = mix(c001, c101, w.x);
    let x11 = mix(c011, c111, w.x);
    return mix(mix(x00, x10, w.y), mix(x01, x11, w.y), w.z);
}
fn fbm3(p0: vec3<f32>, oct: i32) -> f32 {
    var p = p0;
    var a = 0.5;
    var s = 0.0;
    var n = 0.0;
    for (var i = 0; i < oct; i = i + 1) {
        s = s + a * vnoise3(p);
        n = n + a;
        a = a * 0.5;
        p = p * 2.02 + vec3<f32>(13.1, 7.3, 5.7);
    }
    return s / max(n, 1e-4);
}
fn hg_phase(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (12.566370 * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

// Cloud density at a world point (0 outside the slab). Coverage lifts the field;
// erosion carves the edges with higher-frequency detail.
fn cloud_density(p: vec3<f32>) -> f32 {
    let base = u.clouds.w;
    let thick = max(u.clouds2.x, 1.0);
    let hf = clamp((p.y - base) / thick, 0.0, 1.0);
    // Vertical profile: rounded base, eroded anvil top.
    let grad = smoothstep(0.0, 0.15, hf) * smoothstep(1.0, 0.6, hf);
    if (grad <= 0.0) { return 0.0; }
    let drift = vec3<f32>(u.p1.z * u.clouds2.w * 40.0, 0.0, u.p1.z * u.clouds2.w * 12.0);
    let shape = fbm3((p + drift) * 0.0012, 4);
    var d = shape * grad - (1.0 - u.clouds.y);
    if (d <= 0.0) { return 0.0; }
    let detail = clamp(u.clouds2.z, 0.0, 1.0);
    if (detail > 0.0) {
        let er = fbm3((p + drift * 2.0) * 0.006, 3);
        d = d - er * detail * 0.6;
    }
    return max(d, 0.0) * u.clouds.z;
}

// Transmittance from `p` toward the sun through the cloud layer (self-shadow).
fn cloud_light(p: vec3<f32>, sun: vec3<f32>) -> f32 {
    let thick = max(u.clouds2.x, 1.0);
    let dt = thick / 6.0 / max(sun.y, 0.15);
    var tau = 0.0;
    for (var i = 0; i < 6; i = i + 1) {
        tau = tau + cloud_density(p + sun * (dt * (f32(i) + 0.5))) * dt;
    }
    return exp(-tau * u.clouds3.y * 0.02);
}

// Cloud-shadow transmittance from a terrain point up toward the sun (so clouds
// darken the land beneath them).
fn cloud_shadow(pos: vec3<f32>, sun: vec3<f32>) -> f32 {
    if (u.clouds.x <= 0.5 || u.clouds3.z <= 0.0 || sun.y < 0.05) { return 1.0; }
    let t_enter = (u.clouds.w - pos.y) / sun.y; // up to the cloud base
    if (t_enter < 0.0) { return 1.0; }
    let thick = max(u.clouds2.x, 1.0);
    let dt = thick / 5.0 / max(sun.y, 0.15);
    var tau = 0.0;
    for (var i = 0; i < 5; i = i + 1) {
        tau = tau + cloud_density(pos + sun * (t_enter + dt * (f32(i) + 0.5))) * dt;
    }
    return mix(1.0, exp(-tau * u.clouds3.y * 0.02), clamp(u.clouds3.z, 0.0, 1.0));
}

// Raymarch the cloud slab → (in-scattered rgb, transmittance). Front-to-back.
fn clouds_raymarch(ro: vec3<f32>, rd: vec3<f32>, sun: vec3<f32>,
                   sun_col: vec3<f32>, amb_col: vec3<f32>) -> vec4<f32> {
    if (abs(rd.y) < 1e-4) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    let base = u.clouds.w;
    let top = base + max(u.clouds2.x, 1.0);
    let tb = (base - ro.y) / rd.y;
    let tt = (top - ro.y) / rd.y;
    let t0 = max(min(tb, tt), 0.0);
    var t1 = max(tb, tt);
    if (t1 <= 0.0 || t1 <= t0) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    t1 = min(t1, 26000.0);
    let steps = clamp(i32(u.clouds2.y), 8, 128);
    let dt = (t1 - t0) / f32(steps);
    let g = clamp(u.clouds3.x, 0.0, 0.95);
    let ph = hg_phase(dot(rd, sun), g);
    var t = 1.0;       // transmittance
    var scat = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * (t0 + (f32(i) + 0.5) * dt);
        let d = cloud_density(p);
        if (d > 0.001) {
            let ls = cloud_light(p, sun);
            let trans = exp(-d * dt * 0.03);
            let lit = sun_col * (ls * ph) + amb_col * u.clouds3.w;
            scat = scat + t * (1.0 - trans) * lit;
            t = t * trans;
            if (t < 0.01) { break; }
        }
    }
    return vec4<f32>(scat, t);
}

// Sky + the #102 volumetric cloud overlay (when enabled). Linear HDR.
fn sky_color(ro: vec3<f32>, rd: vec3<f32>, sun: vec3<f32>, sundot: f32) -> vec3<f32> {
    var col = clear_sky(ro, rd, sun, sundot);
    if (u.clouds.x > 0.5) {
        let day = day_factor(sun);
        let dusk = (1.0 - day) * smoothstep(-0.35, 0.18, sun.y);
        // Cloud lighting: warm sun (reddening at dusk; faded at night) + cool sky fill,
        // staying consistent with the day cycle / atmosphere mood.
        let sun_col = mix(vec3<f32>(1.0, 0.95, 0.85), vec3<f32>(1.0, 0.5, 0.28), dusk)
                    * (u.sun.w * (0.5 + 1.3 * day));
        let amb_col = mix(vec3<f32>(0.03, 0.04, 0.06), vec3<f32>(0.45, 0.55, 0.78), day);
        let c = clouds_raymarch(ro, rd, sun, sun_col, amb_col);
        col = col * c.w + c.xyz;
    }
    return col;
}

// HSV→RGB (h,s,v in 0..1) for the tunable water colour.
fn hsv2rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(5.0, 3.0, 1.0);
    let p = abs(fract(vec3<f32>(h) + k / 6.0) * 6.0 - 3.0);
    return v * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

// Terrain albedo palette (id = u.p2.w): low rock, high rock, vegetation, snow,
// and an `emit` HDR colour (the glow the terrain radiates when `emissive` > 0).
struct Pal { rock_lo: vec3<f32>, rock_hi: vec3<f32>, veg: vec3<f32>, snow: vec3<f32>, emit: vec3<f32> };
fn palette(id: i32) -> Pal {
    var p: Pal;
    // 0 = Alpine (the original look) — a faint cool dusk glow.
    p.rock_lo = vec3<f32>(0.08, 0.06, 0.05);
    p.rock_hi = vec3<f32>(0.13, 0.11, 0.09);
    p.veg     = vec3<f32>(0.10, 0.13, 0.07);
    p.snow    = vec3<f32>(0.78, 0.82, 0.90);
    p.emit    = vec3<f32>(0.30, 0.55, 1.00);
    if (id == 1) { // Desert — ochre sand + pale dunes.
        p.rock_lo = vec3<f32>(0.30, 0.20, 0.10);
        p.rock_hi = vec3<f32>(0.55, 0.40, 0.22);
        p.veg     = vec3<f32>(0.40, 0.33, 0.16);
        p.snow    = vec3<f32>(0.85, 0.78, 0.62);
        p.emit    = vec3<f32>(1.00, 0.65, 0.25); // warm desert dusk
    } else if (id == 2) { // Volcanic — basalt + ember veins, ash caps.
        p.rock_lo = vec3<f32>(0.04, 0.03, 0.03);
        p.rock_hi = vec3<f32>(0.12, 0.08, 0.07);
        p.veg     = vec3<f32>(0.60, 0.14, 0.03);
        p.snow    = vec3<f32>(0.28, 0.28, 0.30);
        p.emit    = vec3<f32>(3.00, 0.55, 0.06); // molten lava (HDR)
    } else if (id == 3) { // Arctic — blue-grey rock, bright snow.
        p.rock_lo = vec3<f32>(0.10, 0.13, 0.18);
        p.rock_hi = vec3<f32>(0.20, 0.25, 0.32);
        p.veg     = vec3<f32>(0.16, 0.22, 0.28);
        p.snow    = vec3<f32>(0.90, 0.94, 1.00);
        p.emit    = vec3<f32>(0.45, 0.80, 1.00); // glacial blue glow
    } else if (id == 4) { // Verdant — mossy green slopes.
        p.rock_lo = vec3<f32>(0.06, 0.09, 0.05);
        p.rock_hi = vec3<f32>(0.12, 0.16, 0.08);
        p.veg     = vec3<f32>(0.10, 0.32, 0.10);
        p.snow    = vec3<f32>(0.80, 0.86, 0.78);
        p.emit    = vec3<f32>(0.20, 1.00, 0.45); // bioluminescent green
    } else if (id == 5) { // Mars — rusty oxide, dusty caps.
        p.rock_lo = vec3<f32>(0.22, 0.09, 0.05);
        p.rock_hi = vec3<f32>(0.46, 0.21, 0.12);
        p.veg     = vec3<f32>(0.35, 0.16, 0.10);
        p.snow    = vec3<f32>(0.72, 0.56, 0.46);
        p.emit    = vec3<f32>(1.00, 0.25, 0.12); // crimson rift
    } else if (id == 6) { // Monochrome — neutral greyscale.
        p.rock_lo = vec3<f32>(0.05, 0.05, 0.05);
        p.rock_hi = vec3<f32>(0.18, 0.18, 0.18);
        p.veg     = vec3<f32>(0.12, 0.12, 0.12);
        p.snow    = vec3<f32>(0.92, 0.92, 0.92);
        p.emit    = vec3<f32>(1.00, 1.00, 1.00); // white glow
    }
    return p;
}

// Distance fog → the haze the sky meets at the horizon, plus optional colored
// aerial-perspective scattering (u.p3.y): away from the sun the haze is blue,
// toward it warm — so distant ridges fade into a tinted atmosphere.
fn apply_fog(col: vec3<f32>, rd: vec3<f32>, sun: vec3<f32>, t: f32) -> vec3<f32> {
    // #100: physically based aerial perspective — blend the surface toward the
    // ACTUAL sky radiance in this direction (the airlight, from the same scattering
    // integral) by a distance transmittance, so distant ridges fade into the real
    // atmosphere (desaturated blue away from the sun, warm toward it) instead of a
    // tuned exponential fog. `aerial_strength` (atmos2.z) scales it; fog_density
    // still sets the falloff rate.
    if (u.atmos.x > 0.5) {
        let air = atm_sky(rd, sun);
        let ext = exp(-u.p0.z * 0.00022 * t);
        let aerial = clamp(u.atmos2.z, 0.0, 2.0);
        return mix(col, air, clamp((1.0 - ext) * aerial, 0.0, 1.0));
    }
    let fog = 1.0 - exp(-u.p0.z * 0.00018 * t);
    var fog_col = vec3<f32>(0.60, 0.68, 0.84) * (0.7 + 0.3 * u.p1.y);
    let sd = clamp(dot(rd, sun), 0.0, 1.0);
    let aerial = mix(vec3<f32>(0.45, 0.60, 0.95), vec3<f32>(1.0, 0.75, 0.45), pow(sd, 3.0));
    fog_col = mix(fog_col, aerial, clamp(u.p3.y, 0.0, 1.0));
    // Darken the haze toward a deep night fog as the sun sets.
    let day = day_factor(sun);
    fog_col = mix(vec3<f32>(0.012, 0.016, 0.038), fog_col, day);
    return mix(col, fog_col, clamp(fog, 0.0, 1.0));
}

@fragment
fn fs_terrain(in: VsOut) -> @location(0) vec4<f32> {
    // World ray direction from the real camera (translation cancels in the
    // subtraction, so only the orbit orientation matters); the origin is the
    // synthetic fly camera (terrain.rs rides it above the landscape).
    let near_h = u.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far_h  = u.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let rd = normalize(far_h.xyz / far_h.w - near_h.xyz / near_h.w);
    let ro = u.ro.xyz;

    let sun = normalize(u.sun.xyz);
    let sun_i = u.sun.w;
    let sundot = clamp(dot(rd, sun), 0.0, 1.0);
    let pal = palette(i32(u.p2.w));
    let emissive = u.p3.x;
    let godray = u.p3.z;
    let water_on = u.p3.w > 0.5;
    let water_y = u.p4.x;
    // #102B: the landscape can be turned off entirely (ocean-only mode) — the pass
    // still runs to draw the sky + the FFT ocean, just without the terrain raymarch.
    let land_on = u.ocean2.w > 0.5;
    let ocean_on = u.ocean.x > 0.5;

    // Peak altitude bounds the march: a ray climbing above it never meets terrain.
    let maxh = u.p0.x * 1.05;
    var tmax = 12000.0;
    if (rd.y > 0.0001 && ro.y < maxh) {
        tmax = min(tmax, (maxh - ro.y) / rd.y);
    }

    var col: vec3<f32>;
    var dist = tmax; // distance to the shaded surface (for fog / god-rays)
    if (!land_on) {
        // Ocean-only (or no-landscape): the background is pure sky + clouds.
        col = sky_color(ro, rd, sun, sundot);
    } else if (rd.y >= 0.0 && ro.y > maxh) {
        // Looking up from above the peaks → pure sky.
        col = sky_color(ro, rd, sun, sundot);
    } else {
        let t = raymarch(ro, rd, tmax);
        if (t >= tmax) {
            col = sky_color(ro, rd, sun, sundot);
        } else {
            dist = t;
            let pos = ro + rd * t;
            let nor = calc_normal(pos, t);
            let hnorm = clamp(pos.y / max(u.p0.x, 1.0), 0.0, 1.0);

            // Rock albedo: dark strata broken up by a little noise; greener and
            // softer on the flats, bare on the steeps. Colours come from the palette.
            let r = fbm(pos.xz * FREQ * 9.0, 4, 0.0);
            var rock = mix(pal.rock_lo, pal.rock_hi, r);
            rock = mix(rock, pal.veg * (0.5 + 0.5 * r), smoothstep(0.65, 0.92, nor.y));

            // Snow by altitude + flatness, breaking up with a little noise.
            let snow_start = u.p0.y;
            let h = smoothstep(snow_start - 0.08, snow_start + 0.12, hnorm + 0.10 * (r - 0.5));
            let slope = smoothstep(0.55, 0.75, nor.y);
            let snow = h * slope;
            var alb = mix(rock, pal.snow, snow);

            // Lighting: warm key sun (shadowed) + cool sky-dome ambient + a dim
            // bounce from the opposite side.
            let sh = soft_shadow(pos + nor * 2.0, sun, maxh);
            // #102: clouds also cast soft shadows onto the land beneath them.
            let csh = cloud_shadow(pos, sun);
            let dif = clamp(dot(nor, sun), 0.0, 1.0) * sh * csh;
            let amb = 0.5 + 0.5 * nor.y;
            let bac = clamp(0.3 + 0.7 * dot(normalize(vec3<f32>(-sun.x, 0.0, -sun.z)), nor), 0.0, 1.0);

            // Sky-dome ambient + back-bounce fade to a faint cool moonlight floor
            // as the sun sets (the warm sun term already vanishes once it's below
            // the horizon), so the land goes dark at night under the stars.
            let day = day_factor(sun);
            let amb_day = mix(0.03, 1.0, day);
            var lin = vec3<f32>(0.0);
            lin = lin + dif * vec3<f32>(1.30, 1.00, 0.72) * (2.6 * sun_i);
            lin = lin + amb * vec3<f32>(0.34, 0.46, 0.66) * 0.9 * amb_day;
            lin = lin + bac * vec3<f32>(0.20, 0.22, 0.26) * amb_day;
            col = alb * lin;

            // A crisp snow specular toward the sun.
            let hal = normalize(sun - rd);
            col = col + snow * dif * pow(clamp(dot(nor, hal), 0.0, 1.0), 32.0) * vec3<f32>(1.2, 1.1, 1.0);

            // Emissive HDR glow: the terrain radiates light from its low cracks /
            // valleys (lava, biolume, glacial glow — colour per palette), gently
            // pulsing on the clock. Outputs linear > 1 → it blooms + uses EDR.
            if (emissive > 0.0) {
                let veins = pow(1.0 - smoothstep(0.0, 0.5, hnorm), 2.0); // low ground
                let crack = pow(1.0 - r, 3.0);                          // dark strata
                let pulse = 0.7 + 0.3 * sin(u.p1.z * 1.7 + pos.x * 0.01 + pos.z * 0.013);
                col = col + pal.emit * emissive * (0.6 * veins + 0.5 * crack) * pulse;
            }

            col = apply_fog(col, rd, sun, t);
        }
    }

    // Reflective sea plane: when the view ray crosses sea level before the land, the
    // surface is water. With the #102B FFT ocean on, the wave normal + foam come from
    // the Tessendorf tile; otherwise it's the simple ripple water. Both Fresnel-blend a
    // sky reflection with a deep water colour and add an HDR sun glint.
    let sea_y = select(water_y, u.ocean.y, ocean_on);
    if ((water_on || ocean_on) && rd.y < -1.0e-4 && ro.y > sea_y) {
        let t_w = (sea_y - ro.y) / rd.y;
        if (t_w > 0.0 && t_w < dist) {
            let wp = ro + rd * t_w;
            var wn: vec3<f32>;
            var foam = 0.0;
            if (ocean_on) {
                // Tile the FFT ocean across the world; reconstruct the up-normal.
                let uv = wp.xz / max(u.ocean2.z, 1.0);
                let tex = textureSampleLevel(ocean_tex, ocean_samp, uv, 0.0);
                let ny = sqrt(max(1.0 - tex.x * tex.x - tex.y * tex.y, 0.0));
                wn = normalize(vec3<f32>(tex.x, ny, tex.y));
                foam = clamp(tex.w * u.ocean.z, 0.0, 1.0); // foam × strength
            } else {
                let rip = u.p4.z;
                let tt = u.p1.z;
                let n1 = vnoise(wp.xz * 0.06 + vec2<f32>(tt * 0.7, tt * 0.4)) - 0.5;
                let n2 = vnoise(wp.xz * 0.13 - vec2<f32>(tt * 0.5, tt * 0.9)) - 0.5;
                wn = normalize(vec3<f32>(rip * (n1 + n2) * 0.7, 1.0, rip * (n1 - n2) * 0.7));
            }
            let refl = reflect(rd, wn);
            // Force the reflected ray upward so it samples the sky dome.
            let up_refl = normalize(vec3<f32>(refl.x, abs(refl.y) + 0.02, refl.z));
            let rdot = clamp(dot(up_refl, sun), 0.0, 1.0);
            let sky = sky_color(wp, up_refl, sun, rdot);
            let fres = 0.02 + 0.98 * pow(1.0 - clamp(dot(-rd, wn), 0.0, 1.0), 5.0);
            // Depth-based absorption: a deep teal seen at grazing angles, brightening
            // toward the shallows / steeper view (ocean: hue/depth tunable).
            let hue = select(u.p4.y, u.ocean2.x, ocean_on);
            let deep = hsv2rgb(hue, 0.75, 0.05);
            let shallow = hsv2rgb(hue, 0.55, 0.20);
            let depth_amt = select(0.65, clamp(u.ocean2.y, 0.0, 1.0), ocean_on);
            let wcol = mix(deep, shallow, clamp(dot(-rd, wn), 0.0, 1.0) * depth_amt);
            var wat = mix(wcol, sky, fres);
            // Sun glitter from the slope distribution.
            let glit = select(1.0, u.ocean.w, ocean_on);
            wat = wat + pow(rdot, 200.0) * vec3<f32>(1.5, 1.25, 0.9) * sun_i * glit;
            // Foam: white crests, lit by the day so it doesn't glow at night.
            let day_f = day_factor(sun);
            wat = mix(wat, vec3<f32>(1.0, 1.04, 1.08) * (0.3 + 0.7 * day_f), foam);
            col = apply_fog(wat, rd, sun, t_w);
            dist = t_w;
        }
    }

    // God rays: in-scattered sunlight glowing through the haze toward the sun —
    // stronger the more atmosphere the ray crosses. Reads as volumetric shafts.
    // Fades out with the sun (day factor) so night is clean.
    let day = day_factor(sun);
    if (godray > 0.0) {
        let shaft = pow(sundot, 6.0) * (1.0 - exp(-u.p0.z * 0.00012 * dist));
        col = col + godray * shaft * vec3<f32>(1.0, 0.85, 0.6) * (0.6 + sun_i) * day;
    }

    // A touch of sun scatter over the whole frame, then overall brightness.
    col = col + 0.20 * vec3<f32>(1.0, 0.72, 0.40) * pow(sundot, 8.0) * day;
    col = col * u.p1.x;

    // Linear HDR; alpha 0 = background coverage (composite gives it bg_tonemap).
    return vec4<f32>(max(col, vec3<f32>(0.0)), 0.0);
}

// Half-res upscale: the terrain was raymarched into a half-size HDR target; this
// point-samples it at the full-res pixel (frag.xy → half-res texel) and writes it
// to the background with the same far-plane depth as `fs_terrain` (vs_terrain puts
// clip.z at the far plane), so the generator still composites in front. No sampler
// / UV convention — `textureLoad` by integer pixel/2 is unambiguous (no Y-flip).
@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4<f32> {
    let div = max(u.p2.z, 1.0); // resolution divisor (2 = half, 4 = quarter)
    let texel = vec2<i32>(in.clip.xy / div);
    let c = textureLoad(src_tex, texel, 0);
    return vec4<f32>(max(c.rgb, vec3<f32>(0.0)), 0.0);
}

// Organic Math — Creature Engine SDF raymarch (GitHub #476, Tier 1).
//
// A sibling of the Mandelbulb / Minimal-surface raymarch path: a fullscreen pass
// marches a ray per pixel against a synthetic sea creature assembled from a union
// of simple signed-distance primitives (ellipsoids / tapered round-cones /
// flattened paddles) placed along a spine. The body is the smooth-union of the
// primitives with a PER-PRIMITIVE blend radius `k`; each primitive carries a
// `glow` that blends through the same smin so bright organs (the dorsal rod) read
// as bioluminescent with no seam. A travelling peristaltic domain warp along the
// body axis is the swim. The surface is shaded with the SAME metallic-roughness
// IBL + key/fill PBR as cube.wgsl / mandelbulb.wgsl (Standard path) so the look
// matches every other mode. Ray misses `discard` so the backdrop shows through;
// hits write `frag_depth` so the surface depth-composites with the skybox + feeds
// bloom.
//
// The primitive SDFs + smin + warp mirror math.rs (`sd_ellipsoid` / `sd_round_cone`
// / `sd_round_box` / `creature_map` / `creature_warp`) 1:1 so the CPU tests pin
// the shader's geometry. Outputs LINEAR HDR radiance (exposure/bloom/tonemap
// happen in the composite).

// ----- group(0): the cube shader's uniform block (same buffer, reused) -----
struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    mat: vec4<f32>,        // x=metallic, y=roughness, z=glow, w=prefilter_mip_count
    env: vec4<f32>,        // x=exposure, y=env_intensity, z=env_rotation(rad), w=opacity
    key_light: vec4<f32>,  // xyz = dir TO key light (unit), w=intensity
    fill_light: vec4<f32>, // xyz = dir TO fill light (unit), w=intensity
    amb: vec4<f32>,        // x=ambient/IBL mult, y=material_type, z=glass IOR, w=palette-active
    sss: vec4<f32>,        // translucency: x=amount, y=distortion, z=power
    irid: vec4<f32>,       // iridescence: x=amount, y=scale, z=hue shift
    env_tint: vec4<f32>,   // xyz = environment/IBL tint colour
    ripple: vec4<f32>,      // emissive ripple: x=intensity, y=phase, z=freq, w=sharpness
    ripple_ctr: vec4<f32>,  // xyz = field centre (world), w = field radius
    ripple_mode: vec4<f32>, // x = geom (0 radial / 1 axial), yzw = axial axis dir
    glassx: vec4<f32>,      // spectral glass: x=dispersion, y=caustic, z=thin_film, w=samples
    // --- padding fields to reach `thinfilm` at the shared byte offset (matches the
    // full cube.wgsl / render.rs Uniforms; this shader only reads through `thinfilm`) ---
    reflect_ctl: vec4<f32>,
    refl_box_min: vec4<f32>,
    refl_box_max: vec4<f32>,
    refr: vec4<f32>,
    aniso: vec4<f32>,
    coat: vec4<f32>,
    sheen: vec4<f32>,
    body: vec4<f32>,
    micro: vec4<f32>,
    micro2: vec4<f32>,
    emit: vec4<f32>,
    thinfilm: vec4<f32>,    // physical thin-film (#258 T1): x=base thickness (nm; 0 → OFF),
                            // y=marbling, z=film IOR, w=drainage
    // #476 full-PBR parity: expose the colour-grade uniforms so the creature honours
    // Material Hue/Sat/Value + the calibrated-colour law like the cubes. These live
    // AFTER `thinfilm` in the shared cube/render Uniforms — `demo_light_pos/col` and
    // `skyrefl` MUST be declared as padding so `matcol`/`cal` land at the right byte
    // offset in the shared buffer (this shader reads through `cal`).
    demo_light_pos: vec4<f32>, // #288 T3 demo point light (padding — not read here)
    demo_light_col: vec4<f32>, // padding
    matcol: vec4<f32>,      // #305 T1: x=hue offset (turns), y=saturation, z=value, w reserved
    skyrefl: vec4<f32>,     // #305 T2 live-sky reflection (padding — not read here)
    cal: vec4<f32>,         // #349 T1: x=mode (0 Aesthetic/identity, 1 Calibrated), y=LUT id,
                            // z=amount, w=measured level t
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// ----- group(1): IBL maps + filtering sampler (same layout as cube.wgsl) -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): Creature params + the body-plan primitive list -----
struct CreatureU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED); for frag_depth
    p0: vec4<f32>,          // count, steps, scale, swim_phase
    p1: vec4<f32>,          // warp_freq, warp_amp, rim, glow_scale
    center: vec4<f32>,      // xyz = world centre, w = bound-sphere radius (world)
    p2: vec4<f32>,          // samples(msaa), size.x, size.y, _
    p3: vec4<f32>,          // metachronal wave: freq, phase, sharp, amount
    p4: vec4<f32>,          // palette: id, spine_min (unit z), spine_inv_range, _
};
@group(2) @binding(0) var<uniform> c: CreatureU;

// One SDF primitive. Field meaning depends on kind (v0.w):
//   Ellipsoid (0): v0.xyz = centre, v1.xyz = radii.
//   RoundCone (1): v0.xyz = endpoint A, v1.xyz = endpoint B, v1.w = r1, v2.x = r2.
//   Paddle    (2): v0.xyz = centre, v1.xyz = half-extents, v1.w = corner round.
// v2.y = smooth-union blend `k`; v2.z = emissive glow; v2.w = spine coordinate
// (position along +Z), used to phase the metachronal wave.
struct CPrim {
    v0: vec4<f32>,
    v1: vec4<f32>,
    v2: vec4<f32>,
};
struct Prims { p: array<CPrim, 64> };
@group(2) @binding(1) var<uniform> prims: Prims;

// ----- group(3): reaction–diffusion (Turing) surface field -----
@group(3) @binding(0) var rd_tex  : texture_2d<f32>;
@group(3) @binding(1) var rd_samp : sampler;
struct RdLook { params: vec4<f32> }; // x=intensity, y=scale, z=albedo_mix
@group(3) @binding(2) var<uniform> rdu: RdLook;

fn rd_dapple(world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    if (rdu.params.x <= 0.0 && rdu.params.z <= 0.0) {
        return 0.0;
    }
    let scale = rdu.params.y;
    let an = abs(n);
    let w = an / max(an.x + an.y + an.z, 1e-4);
    let vx = textureSampleLevel(rd_tex, rd_samp, world_pos.yz * scale, 0.0).g;
    let vy = textureSampleLevel(rd_tex, rd_samp, world_pos.xz * scale, 0.0).g;
    let vz = textureSampleLevel(rd_tex, rd_samp, world_pos.xy * scale, 0.0).g;
    let v = vx * w.x + vy * w.y + vz * w.z;
    return clamp((v - 0.2) * 2.5, 0.0, 1.0);
}

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.15915494, 0.31830989);

fn rotate_y(d: vec3<f32>, ang: f32) -> vec3<f32> {
    let cs = cos(ang);
    let sn = sin(ang);
    return vec3<f32>(d.x * cs + d.z * sn, d.y, -d.x * sn + d.z * cs);
}

fn dir_to_equirect_uv(dir: vec3<f32>) -> vec2<f32> {
    let d = normalize(dir);
    var uv = vec2<f32>(atan2(d.z, d.x), asin(clamp(d.y, -1.0, 1.0)));
    uv = uv * INV_ATAN + vec2<f32>(0.5, 0.5);
    return uv;
}

// --- BRDF helpers (ported 1:1 from cube.wgsl / mandelbulb.wgsl) ---
fn fresnel_schlick_roughness(cos_theta: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let one_minus_r = vec3<f32>(1.0 - roughness);
    return f0 + (max(one_minus_r, f0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}
fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}
fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let g_v = geometry_schlick_ggx(max(dot(n, v), 0.0), roughness);
    let g_l = geometry_schlick_ggx(max(dot(n, l), 0.0), roughness);
    return g_v * g_l;
}
fn direct_light(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let f = fresnel_schlick(max(dot(h, v), 0.0), f0);
    let d = distribution_ggx(n, h, roughness);
    let g = geometry_smith(n, v, l, roughness);
    let specular = (d * f * g) / (4.0 * max(dot(n, v), 0.0) * n_dot_l + 1e-3);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    return (kd * albedo / PI + specular) * radiance * n_dot_l;
}
fn sample_irradiance(n_env: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(irradiance_tex, ibl_samp, dir_to_equirect_uv(n_env), 0.0).rgb;
}
fn sample_prefiltered(r_env: vec3<f32>, roughness: f32) -> vec3<f32> {
    let mip_count = u.mat.w;
    let lod = roughness * max(mip_count - 1.0, 0.0);
    return textureSampleLevel(prefilter_tex, ibl_samp, dir_to_equirect_uv(r_env), lod).rgb;
}
fn iridescent_tint(cos_theta: f32, scale: f32, shift: f32) -> vec3<f32> {
    let opd = scale * (1.0 - clamp(cos_theta, 0.0, 1.0));
    let phase = (opd + shift) * 2.0 * PI;
    return 0.5 + 0.5 * cos(vec3<f32>(phase) + vec3<f32>(0.0, 2.0944, 4.1888));
}
fn translucency_lobe(v: vec3<f32>, l: vec3<f32>, n: vec3<f32>, distortion: f32, power: f32) -> f32 {
    let lt = normalize(l + n * distortion);
    return pow(clamp(dot(v, -lt), 0.0, 1.0), power);
}

// --- Glass / thin-film helpers (ported 1:1 from cube.wgsl / minimal.wgsl) ---
fn fract1(x: f32) -> f32 { return x - floor(x); }
fn hash3(c_in: vec3<f32>) -> vec3<f32> {
    let p = vec3<f32>(
        dot(c_in, vec3<f32>(127.1, 311.7, 74.7)),
        dot(c_in, vec3<f32>(269.5, 183.3, 246.1)),
        dot(c_in, vec3<f32>(113.5, 271.9, 124.6)),
    );
    return vec3<f32>(
        fract1(sin(p.x) * 43758.547),
        fract1(sin(p.y) * 43758.547),
        fract1(sin(p.z) * 43758.547),
    );
}
fn thin_film_tint(cos_theta: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return vec3<f32>(1.0);
    }
    let opd = (1.0 - clamp(cos_theta, 0.0, 1.0)) * 6.2831853 * (2.0 + amount * 6.0);
    let tint = 0.5 + 0.5 * cos(vec3<f32>(opd) + vec3<f32>(0.0, 2.0944, 4.1888));
    return mix(vec3<f32>(1.0), tint, clamp(amount, 0.0, 1.0));
}
fn fresnel_dielectric(cos_i: f32, n1: f32, n2: f32) -> f32 {
    let s2 = (n1 / n2) * (n1 / n2) * max(1.0 - cos_i * cos_i, 0.0);
    if (s2 >= 1.0) { return 1.0; }
    let cos_t = sqrt(1.0 - s2);
    let rs = (n1 * cos_i - n2 * cos_t) / (n1 * cos_i + n2 * cos_t);
    let rp = (n1 * cos_t - n2 * cos_i) / (n1 * cos_t + n2 * cos_i);
    return clamp(0.5 * (rs * rs + rp * rp), 0.0, 1.0);
}
fn film_thickness_at(world_pos: vec3<f32>) -> f32 {
    let base = u.thinfilm.x;
    let drainage = u.thinfilm.w;
    let marble = u.thinfilm.y;
    let grad = -tanh(world_pos.y * 0.12);
    var d = base * (1.0 + drainage * grad);
    let n0 = hash3(floor(world_pos * 1.7)).x;
    let n1 = hash3(floor(world_pos * 0.6) + vec3<f32>(11.0)).x;
    d = d * (1.0 + marble * (n0 * 0.7 + n1 * 0.3 - 0.5));
    return max(d, 0.0);
}
fn film_airy(cos_t: f32, d: f32, n_film: f32, lambda: f32, r_top: f32) -> f32 {
    let phase = (4.0 * PI * n_film * d * cos_t) / max(lambda, 1.0);
    let s = sin(phase * 0.5);
    let s2 = s * s;
    let r = clamp(r_top, 0.0, 0.999);
    return (4.0 * r * s2) / ((1.0 - r) * (1.0 - r) + 4.0 * r * s2);
}
fn thin_film_physical(cos_theta: f32, world_pos: vec3<f32>) -> vec3<f32> {
    let n_film = max(u.thinfilm.z, 1.0);
    let d = film_thickness_at(world_pos);
    let cos_i = clamp(cos_theta, 0.02, 1.0);
    let s2 = (1.0 / n_film) * (1.0 / n_film) * max(1.0 - cos_i * cos_i, 0.0);
    let cos_t = sqrt(max(1.0 - s2, 0.0));
    let r_top = fresnel_dielectric(cos_i, 1.0, n_film);
    return vec3<f32>(
        film_airy(cos_t, d, n_film, 680.0, r_top),
        film_airy(cos_t, d, n_film, 550.0, r_top),
        film_airy(cos_t, d, n_film, 440.0, r_top),
    );
}
fn sample_refract_chan(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32) -> vec3<f32> {
    let refr_dir = refract(-v, n, 1.0 / max(ior, 1.0));
    if (dot(refr_dir, refr_dir) < 1e-6) {
        return sample_prefiltered(rotate_y(reflect(-v, n), env_rot), roughness); // total internal reflection
    }
    return sample_prefiltered(rotate_y(refr_dir, env_rot), roughness);
}
fn glass_dispersion(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32,
                    dispersion: f32, caustic: f32) -> vec3<f32> {
    let spread = dispersion * 0.06; // fractional IOR offset between R and B
    let cr = sample_refract_chan(v, n, ior * (1.0 - spread), roughness, env_rot);
    let cg = sample_refract_chan(v, n, ior,                  roughness, env_rot);
    let cb = sample_refract_chan(v, n, ior * (1.0 + spread), roughness, env_rot);
    var col = vec3<f32>(cr.r, cg.g, cb.b);
    if (caustic > 0.0) {
        let lum = max(max(col.r, col.g), col.b);
        col += col * smoothstep(0.6, 2.0, lum) * caustic * 2.0;
    }
    return col;
}

// --- Colour palettes / spectrums (mirrors math.rs::palette_tint 1:1). The
// creature's default albedo is a bioluminescent cool blue; picking a Palette other
// than Native tints the whole body through a 1-D LUT, so the creature participates
// in the same spectrums as every other generator (#476 full-PBR follow-up). ---
fn hsv_tint(hue_turns: f32) -> vec3<f32> {
    let h = fract1(fract1(hue_turns) + 1.0) * 6.0;
    let x = 1.0 - abs((h - 2.0 * floor(h * 0.5)) - 1.0);
    if (h < 1.0) { return vec3<f32>(1.0, x, 0.0); }
    if (h < 2.0) { return vec3<f32>(x, 1.0, 0.0); }
    if (h < 3.0) { return vec3<f32>(0.0, 1.0, x); }
    if (h < 4.0) { return vec3<f32>(0.0, x, 1.0); }
    if (h < 5.0) { return vec3<f32>(x, 0.0, 1.0); }
    return vec3<f32>(1.0, 0.0, x);
}
fn iq_palette(t: f32, a: vec3<f32>, b: vec3<f32>, cc: vec3<f32>, d: vec3<f32>) -> vec3<f32> {
    let phase = (cc * t + d) * 6.2831853;
    return clamp(a + b * cos(phase), vec3<f32>(0.0), vec3<f32>(1.0));
}
fn palette_tint(pal: i32, t: f32) -> vec3<f32> {
    if (pal == 1) { return hsv_tint(t); } // Spectrum
    if (pal == 2) { return iq_palette(t, vec3<f32>(0.55, 0.42, 0.45), vec3<f32>(0.45, 0.32, 0.38), vec3<f32>(1.0), vec3<f32>(0.00, 0.12, 0.55)); } // Coral Reef
    if (pal == 3) { return iq_palette(t, vec3<f32>(0.20, 0.42, 0.50), vec3<f32>(0.22, 0.30, 0.38), vec3<f32>(1.0), vec3<f32>(0.60, 0.50, 0.40)); } // Deep Sea
    if (pal == 4) { return iq_palette(t, vec3<f32>(0.60, 0.40, 0.48), vec3<f32>(0.40, 0.28, 0.36), vec3<f32>(1.0), vec3<f32>(0.90, 0.70, 0.20)); } // Anemone
    if (pal == 5) { return iq_palette(t, vec3<f32>(0.72, 0.62, 0.74), vec3<f32>(0.22, 0.22, 0.20), vec3<f32>(1.0), vec3<f32>(0.90, 0.83, 0.70)); } // Jellyfish
    if (pal == 6) { return iq_palette(t, vec3<f32>(0.58, 0.50, 0.40), vec3<f32>(0.30, 0.28, 0.22), vec3<f32>(1.0), vec3<f32>(0.10, 0.20, 0.30)); } // Nautilus
    if (pal == 7) { return iq_palette(t, vec3<f32>(0.44, 0.50, 0.30), vec3<f32>(0.34, 0.34, 0.24), vec3<f32>(1.0), vec3<f32>(0.30, 0.42, 0.18)); } // Kelp
    if (pal == 8) { return iq_palette(t, vec3<f32>(0.10, 0.38, 0.38), vec3<f32>(0.16, 0.42, 0.40), vec3<f32>(1.0, 1.2, 1.1), vec3<f32>(0.00, 0.45, 0.40)); } // Bioluminescence
    if (pal == 9) { return iq_palette(t, vec3<f32>(0.70, 0.40, 0.38), vec3<f32>(0.18, 0.14, 0.12), vec3<f32>(1.0), vec3<f32>(0.00, 0.08, 0.15)); } // Flesh
    if (pal == 10) { return iq_palette(t, vec3<f32>(0.5), vec3<f32>(0.5), vec3<f32>(2.0, 3.0, 1.0), vec3<f32>(0.00, 0.25, 0.50)); } // Candy
    if (pal == 11) { return iq_palette(t, vec3<f32>(0.5), vec3<f32>(0.5), vec3<f32>(1.0), vec3<f32>(0.50, 0.20, 0.25)); } // Plasma
    if (pal == 12) { return iq_palette(t, vec3<f32>(0.5), vec3<f32>(0.5), vec3<f32>(1.0), vec3<f32>(0.80, 0.90, 0.30)); } // Neon
    return hsv_tint(t);
}

// --- Colour grade (#305 HSV + #349 calibrated colour), ported 1:1 from cube.wgsl so
// the creature honours the Material Hue/Saturation/Value + calibrated-colour law. ---
fn cal_p6(x: f32, c0: f32, c1: f32, c2: f32, c3: f32, c4: f32, c5: f32, c6: f32) -> f32 {
    return c0 + x * (c1 + x * (c2 + x * (c3 + x * (c4 + x * (c5 + x * c6)))));
}
fn calibrated_colour(t: f32, lut: u32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    var rgb: vec3<f32>;
    if (lut == 1u) { // Viridis
        rgb = vec3<f32>(
            cal_p6(x, 0.27772733, 0.10509304, -0.33086183, -4.63423050, 6.22826994, 4.77638500, -5.43545586),
            cal_p6(x, 0.00540734, 1.40461353, 0.21484756, -5.79910097, 14.17993337, -13.74514538, 4.64585261),
            cal_p6(x, 0.33409981, 1.38459016, 0.09509516, -19.33244096, 56.69055260, -65.35303263, 26.31243525));
    } else if (lut == 2u) { // Inferno
        rgb = vec3<f32>(
            cal_p6(x, 0.00021894, 0.10651342, 11.60249308, -41.70399613, 77.16293570, -71.31942824, 25.13112622),
            cal_p6(x, 0.00165100, 0.56395644, -3.97285397, 17.43639888, -33.40235894, 32.62606426, -12.24266895),
            cal_p6(x, -0.01948090, 3.93271239, -15.94239411, 44.35414520, -81.80730926, 73.20951986, -23.07032500));
    } else if (lut == 3u) { // Magma
        rgb = vec3<f32>(
            cal_p6(x, -0.00213649, 0.25166054, 8.35371728, -27.66873309, 52.17613981, -50.76852536, 18.65570507),
            cal_p6(x, -0.00074966, 0.67752324, -3.57771951, 14.26473078, -27.94360607, 29.04658282, -11.48977352),
            cal_p6(x, -0.00538613, 2.49402660, 0.31446790, -13.64921319, 12.94416944, 4.23415299, -5.60196151));
    } else { // Turbo (default)
        rgb = vec3<f32>(
            cal_p6(x, 0.11408901, 6.71641950, -66.09402360, 228.76607915, -334.83515658, 218.76372184, -52.88903478),
            cal_p6(x, 0.06288341, 3.18228675, -4.92798270, 25.04986700, -69.31749713, 67.52150568, -21.54527365),
            cal_p6(x, 0.22483372, 7.57158159, -10.09439368, -91.54105330, 288.58588506, -305.20457722, 110.51746477));
    }
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}
fn apply_calibrated(albedo: vec3<f32>) -> vec3<f32> {
    if (u.cal.x < 0.5) { return albedo; }
    let cc = calibrated_colour(u.cal.w, u32(u.cal.y + 0.5));
    return mix(albedo, cc, clamp(u.cal.z, 0.0, 1.0));
}
fn apply_hsv(rgb: vec3<f32>, hsv: vec4<f32>) -> vec3<f32> {
    var c = rgb;
    let hue = hsv.x;
    if (hue != 0.0) {
        let a = hue * 6.2831853;
        let cs = cos(a);
        let sn = sin(a);
        let k = 0.57735026;
        let one_c = 1.0 - cs;
        let m0 = vec3<f32>(cs + k * k * one_c,      k * k * one_c - k * sn, k * k * one_c + k * sn);
        let m1 = vec3<f32>(k * k * one_c + k * sn,  cs + k * k * one_c,     k * k * one_c - k * sn);
        let m2 = vec3<f32>(k * k * one_c - k * sn,  k * k * one_c + k * sn, cs + k * k * one_c);
        c = vec3<f32>(dot(m0, c), dot(m1, c), dot(m2, c));
    }
    let luma = dot(max(c, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, hsv.y); // saturation
    c = c * hsv.z;                      // value
    return max(c, vec3<f32>(0.0));
}
// Spectral emission (#214 T5 pt1): fluorescence (re-emit the env's blue at a hue) +
// incandescence (blackbody glow). Both 0 → no change. Ported 1:1 from cube.wgsl.
fn hue2rgb(h: f32) -> vec3<f32> {
    let r = abs(h * 6.0 - 3.0) - 1.0;
    let g = 2.0 - abs(h * 6.0 - 2.0);
    let b = 2.0 - abs(h * 6.0 - 4.0);
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}
fn blackbody(kelvin: f32) -> vec3<f32> {
    let t = clamp(kelvin, 1000.0, 40000.0) / 100.0;
    var r: f32;
    var g: f32;
    var b: f32;
    if (t <= 66.0) {
        r = 1.0;
        g = clamp(0.3900816 * log(max(t, 1.0)) - 0.6318414, 0.0, 1.0);
    } else {
        r = clamp(1.2929362 * pow(t - 60.0, -0.1332047), 0.0, 1.0);
        g = clamp(1.1298909 * pow(t - 60.0, -0.0755148), 0.0, 1.0);
    }
    if (t >= 66.0) {
        b = 1.0;
    } else if (t <= 19.0) {
        b = 0.0;
    } else {
        b = clamp(0.5432068 * log(t - 10.0) - 1.1962541, 0.0, 1.0);
    }
    return vec3<f32>(r, g, b);
}

// ===================== Creature signed distance field =====================

// SDF of an axis-aligned ellipsoid with radii `r` (IQ's bounded approximation).
fn sd_ellipsoid(p: vec3<f32>, r: vec3<f32>) -> f32 {
    let rr = max(r, vec3<f32>(1e-4));
    let k0 = length(p / rr);
    let k1 = length(p / (rr * rr));
    return k0 * (k0 - 1.0) / max(k1, 1e-8);
}

// SDF of a rounded box, half-extents `b`, corner radius `rad`.
fn sd_round_box(p: vec3<f32>, b: vec3<f32>, rad: f32) -> f32 {
    let q = abs(p) - b + vec3<f32>(rad);
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - rad;
}

// SDF of a round cone between endpoints `a` and `b`, end radii `r1`,`r2`
// (IQ's exact two-point round-cone).
fn sd_round_cone(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r1: f32, r2: f32) -> f32 {
    let ba = b - a;
    let l2 = max(dot(ba, ba), 1e-8);
    let rr = r1 - r2;
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = p - a;
    let y = dot(pa, ba);
    let z = y - l2;
    let x2v = pa * l2 - ba * y;
    let x2 = dot(x2v, x2v);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = sign(rr) * rr * rr * x2;
    if (sign(z) * a2 * z2 > k) {
        return sqrt(max(x2 + z2, 0.0)) * il2 - r2;
    }
    if (sign(y) * a2 * y2 < k) {
        return sqrt(max(x2 + y2, 0.0)) * il2 - r1;
    }
    return (sqrt(max(x2 * a2 * il2, 0.0)) + y * rr) * il2 - r1;
}

fn prim_sdf(pr: CPrim, p: vec3<f32>) -> f32 {
    let kind = i32(pr.v0.w + 0.5);
    if (kind == 1) {
        return sd_round_cone(p, pr.v0.xyz, pr.v1.xyz, pr.v1.w, pr.v2.x);
    }
    if (kind == 2) {
        return sd_round_box(p - pr.v0.xyz, pr.v1.xyz, pr.v1.w);
    }
    return sd_ellipsoid(p - pr.v0.xyz, pr.v1.xyz);
}

// Travelling metachronal wave along the body (#476 Tier 2a): a rectified,
// sharpened sine of the spine coordinate `s` sliding with `phase`. Mirrors
// math.rs::metachronal_wave. Returns 0..1.
fn metachronal_wave(s: f32, phase: f32, freq: f32, sharp: f32) -> f32 {
    let x = max(sin(freq * s - phase), 0.0);
    return pow(x, max(sharp, 1.0));
}

// Modulated emissive glow of a primitive: the base glow brightened by a bright
// band that runs along the body (amount 0 → base glow, byte-identical to Tier 1).
fn prim_glow(pr: CPrim) -> f32 {
    let wave = metachronal_wave(pr.v2.w, c.p3.y, c.p3.x, c.p3.z);
    return pr.v2.z * (1.0 + c.p3.w * wave);
}

// Fold the primitives together with a per-primitive smooth-union, blending the
// (wave-modulated) glow through the same weight `h`. Returns (distance, glow).
// Mirrors math.rs::creature_map.
fn creature_map(p: vec3<f32>) -> vec2<f32> {
    let count = i32(c.p0.x + 0.5);
    var d = 1.0e9;
    var glow = 0.0;
    for (var i = 0; i < count; i = i + 1) {
        let pr = prims.p[i];
        let sd = prim_sdf(pr, p);
        let g = prim_glow(pr);
        if (i == 0) {
            d = sd;
            glow = g;
            continue;
        }
        let k = max(pr.v2.y, 1e-4);
        let h = clamp(0.5 + 0.5 * (sd - d) / k, 0.0, 1.0);
        d = (sd * (1.0 - h) + d * h) - k * h * (1.0 - h);
        glow = g * (1.0 - h) + glow * h;
    }
    return vec2<f32>(d, glow);
}

// Travelling peristaltic domain warp along the body axis (+Z). Mirrors
// math.rs::creature_warp.
fn warp(pu: vec3<f32>) -> vec3<f32> {
    let amp = c.p1.y;
    if (abs(amp) <= 1e-6) {
        return pu;
    }
    let off = sin(c.p1.x * pu.z - c.p0.w) * amp;
    return vec3<f32>(pu.x - off, pu.y, pu.z);
}

// World-space map: into unit space (÷scale), warp, evaluate, scale distance back.
// Returns (world_distance, glow).
fn map_world(world_p: vec3<f32>) -> vec2<f32> {
    let scale = c.p0.z;
    var pu = (world_p - c.center.xyz) / scale;
    pu = warp(pu);
    let res = creature_map(pu);
    return vec2<f32>(res.x * scale, res.y);
}

// ===================== vertex (fullscreen triangle + ray) =====================
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) rd: vec3<f32>,
};

@vertex
fn vs_ray(@builtin(vertex_index) vi: u32) -> VsOut {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    let ndc = uv * 2.0 - vec2<f32>(1.0);
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    let near = c.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = c.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    out.rd = (far.xyz / far.w) - (near.xyz / near.w);
    return out;
}

// ===================== fragment (raymarch + shade) =====================
struct FragOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

struct Trace {
    color: vec3<f32>,
    depth: f32,
    hit: bool,
    alpha: f32, // material opacity (1 = Standard/Chrome; < 1 = Glass, Fresnel-lifted)
};

fn ray_dir(ndc: vec2<f32>) -> vec3<f32> {
    let near = c.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = c.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize((far.xyz / far.w) - (near.xyz / near.w));
}

// Sub-pixel offset (in pixel units) for supersample `i` of `n` (RGSS at 4×).
fn aa_offset(i: i32, n: i32) -> vec2<f32> {
    if (n <= 1) { return vec2<f32>(0.0); }
    if (n == 2) {
        if (i == 0) { return vec2<f32>(-0.25, -0.25); }
        return vec2<f32>(0.25, 0.25);
    }
    var rg = array<vec2<f32>, 4>(
        vec2<f32>(0.125, 0.375), vec2<f32>(-0.375, 0.125),
        vec2<f32>(0.375, -0.125), vec2<f32>(-0.125, -0.375),
    );
    return rg[i & 3];
}

// --- #476 full-PBR parity: material-TYPE overlays (Anisotropic / Clearcoat / Velvet)
// ported 1:1 from cube.wgsl so the Material selector drives the creature the same way.
// (Glitter/retro microstructure is not ported — it needs the cube's spatial hash and
// is niche; the type lobes + reflection controls are what the panel actually drives.) ---
struct AnisoFrame { t: vec3<f32>, b: vec3<f32> };
fn aniso_frame(n: vec3<f32>, brush_world: vec3<f32>, rot: f32) -> AnisoFrame {
    var t = brush_world - n * dot(n, brush_world);
    if (dot(t, t) < 1e-8) {
        let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
        t = a - n * dot(n, a);
    }
    t = normalize(t);
    let b = cross(n, t);
    let c = cos(rot);
    let s = sin(rot);
    var out: AnisoFrame;
    out.t = normalize(t * c + b * s);
    out.b = normalize(b * c - t * s);
    return out;
}
fn aniso_alpha(roughness: f32, amount: f32) -> vec2<f32> {
    let a_abs = clamp(abs(amount), 0.0, 1.0);
    let aspect = sqrt(1.0 - 0.9 * a_abs);
    let a = max(roughness * roughness, 4e-4);
    var at = a / aspect;
    var ab = a * aspect;
    if (amount < 0.0) {
        let tmp = at;
        at = ab;
        ab = tmp;
    }
    return vec2<f32>(at, ab);
}
fn direct_light_aniso(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, t: vec3<f32>, b: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, at: f32, ab: f32, f0: vec3<f32>,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) { return vec3<f32>(0.0); }
    let h = normalize(v + l);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let t_dot_h = dot(t, h);
    let b_dot_h = dot(b, h);
    let dd = t_dot_h * t_dot_h / (at * at) + b_dot_h * b_dot_h / (ab * ab) + n_dot_h * n_dot_h;
    let dist = 1.0 / (PI * at * ab * dd * dd);
    let lv = n_dot_l * length(vec3<f32>(at * dot(t, v), ab * dot(b, v), n_dot_v));
    let ll = n_dot_v * length(vec3<f32>(at * dot(t, l), ab * dot(b, l), n_dot_l));
    let vis = 0.5 / max(lv + ll, 1e-5);
    let fres = fresnel_schlick(max(dot(h, v), 0.0), f0);
    let spec = dist * vis * fres;
    let kd = (vec3<f32>(1.0) - fres) * (1.0 - metallic);
    return (kd * albedo / PI + spec) * radiance * n_dot_l;
}
fn aniso_reflect(n: vec3<f32>, v: vec3<f32>, t: vec3<f32>, b: vec3<f32>,
                 amount: f32, roughness: f32) -> vec3<f32> {
    let dir = select(t, b, amount >= 0.0);
    let a_t = cross(dir, v);
    let a_n = cross(a_t, dir);
    let bend = abs(amount) * clamp(5.0 * roughness, 0.0, 1.0);
    let bent = normalize(mix(n, a_n, bend));
    return reflect(-v, bent);
}
fn sheen_d_charlie(roughness: f32, n_dot_h: f32) -> f32 {
    let inv_a = 1.0 / max(roughness, 1e-3);
    let sin2 = max(1.0 - n_dot_h * n_dot_h, 0.0);
    return (2.0 + inv_a) * pow(sin2, inv_a * 0.5) / (2.0 * PI);
}
fn sheen_lobe(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) { return 0.0; }
    let n_dot_v = max(dot(n, v), 1e-4);
    let h = normalize(v + l);
    let n_dot_h = clamp(dot(n, h), 0.0, 1.0);
    let d = sheen_d_charlie(roughness, n_dot_h);
    let vis = 1.0 / (4.0 * (n_dot_l + n_dot_v - n_dot_l * n_dot_v));
    return d * vis * n_dot_l;
}
fn demo_point_light(world: vec3<f32>, n: vec3<f32>, v: vec3<f32>,
                    albedo: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let intensity = u.demo_light_pos.w;
    if (intensity <= 0.0) { return vec3<f32>(0.0); }
    let to = u.demo_light_pos.xyz - world;
    let dist = length(to);
    let l = to / max(dist, 1e-3);
    let r = max(u.demo_light_col.w, 1e-3);
    let atten = 1.0 / (1.0 + (dist * dist) / (r * r));
    let radiance = u.demo_light_col.xyz * intensity * atten;
    return direct_light(n, v, l, albedo, metallic, roughness, f0, radiance);
}

fn trace(ro: vec3<f32>, rd: vec3<f32>) -> Trace {
    var tr: Trace;
    tr.hit = false;
    tr.color = vec3<f32>(0.0);
    tr.depth = 1.0;
    tr.alpha = 1.0;

    // Ray vs the creature's bounding sphere.
    let oc = ro - c.center.xyz;
    let bb = dot(oc, rd);
    let cc = dot(oc, oc) - c.center.w * c.center.w;
    let disc = bb * bb - cc;
    if (disc < 0.0) {
        return tr;
    }
    let sq = sqrt(disc);
    let tmin = max(-bb - sq, 0.0);
    let tmax = -bb + sq;
    if (tmax <= tmin) {
        return tr;
    }

    let steps = i32(c.p0.y);
    let scale = c.p0.z;
    var t = tmin;
    var hit = false;
    var hp = vec3<f32>(0.0);
    var glow = 0.0;
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * t;
        let res = map_world(p);
        let eps = max(0.0004 * t, 0.0003 * scale);
        if (res.x < eps) {
            hp = p;
            glow = res.y;
            hit = true;
            break;
        }
        // Conservative step (the warp shear + smin make the field a slight
        // under-estimate near seams).
        t = t + max(res.x * 0.8, 0.0006 * scale);
        if (t > tmax) {
            break;
        }
    }
    if (!hit) {
        return tr;
    }

    // Surface normal from the map gradient (central differences, world space).
    let h = 0.0008 * scale;
    let dx = vec3<f32>(h, 0.0, 0.0);
    let dy = vec3<f32>(0.0, h, 0.0);
    let dz = vec3<f32>(0.0, 0.0, h);
    var n = normalize(vec3<f32>(
        map_world(hp + dx).x - map_world(hp - dx).x,
        map_world(hp + dy).x - map_world(hp - dy).x,
        map_world(hp + dz).x - map_world(hp - dz).x,
    ));
    if (dot(n, n) < 1e-12) {
        n = -rd;
    }

    // Cool bioluminescent base albedo; a chosen Palette (spectrum) retints the whole
    // body along its DOMINANT axis (Native = 0 keeps the bioluminescent blue); the
    // RD field can dapple it. The axis + its min/inv-span come from `c.p4` (chosen
    // CPU-side as the body's longest extent), so the sweep runs the length of the
    // creature whatever its orientation — not a flat block on a non-Z-aligned plan.
    var base_albedo = vec3<f32>(0.52, 0.68, 0.86);
    let pal = i32(c.p4.x + 0.5);
    if (pal != 0) {
        let unit_pos = (hp - c.center.xyz) / scale;
        let axis = i32(c.p4.w + 0.5);
        var coord = unit_pos.z;
        if (axis == 0) { coord = unit_pos.x; }
        else if (axis == 1) { coord = unit_pos.y; }
        let t = clamp((coord - c.p4.y) * c.p4.z, 0.0, 1.0);
        base_albedo = palette_tint(pal, t);
    }
    let rd_d = rd_dapple(hp, n);
    // #476 full-PBR parity: recolour by Material Hue/Saturation/Value (#305) then the
    // calibrated-colour law (#349) — the SAME single application point the cubes use,
    // so identity [0,1,1] / Aesthetic mode → byte-identical to before.
    let albedo = apply_calibrated(apply_hsv(
        mix(base_albedo, base_albedo * rd_d, clamp(rdu.params.z, 0.0, 1.0)), u.matcol));

    // ---- Material-branched shading (Standard / Chrome / Glass / Refractive),
    // env-only — the SAME metallic-roughness PBR + IBL + key/fill the cubes use, so
    // the Material selector drives the creature too (#476 full-PBR follow-up). ----
    let metallic = clamp(u.mat.x, 0.0, 1.0);
    let roughness = clamp(u.mat.y, 0.0, 1.0);
    let mat_glow = u.mat.z;
    let env_intensity = u.env.y;
    let env_rot = u.env.z;
    let ambient_mul = u.amb.x;
    let etint = u.env_tint.rgb;
    let mat_type = u.amb.y;     // 0=Standard, 1=Chrome, 2=Glass (3=Refractive → Glass)
    let ior = max(u.amb.z, 1.0);
    let opacity = u.env.w;

    let v = normalize(u.camera_pos.xyz - hp);
    let n_dot_v = max(dot(n, v), 1e-4);
    let r = reflect(-v, n);
    let n_env = rotate_y(n, env_rot);
    let r_env = rotate_y(r, env_rot);

    let l_key = normalize(u.key_light.xyz);
    let l_fill = normalize(u.fill_light.xyz);

    // Bioluminescence: the per-primitive glow (blended through smin) lit warm-cool,
    // scaled by the editor glow_scale, plus the material glow. A Fresnel rim adds
    // the luminous silhouette these creatures are known for.
    let glow_col = vec3<f32>(0.6, 0.85, 1.0);
    let rim = c.p1.z * pow(1.0 - n_dot_v, 3.0);
    var emissive = albedo * mat_glow
        + glow_col * (glow * c.p1.w)
        + glow_col * rim
        + base_albedo * (rd_d * rdu.params.x);
    // Spectral emission (#214 T5 pt1): fluorescence re-emits the env's short-wavelength
    // light at a chosen hue; incandescence adds a blackbody glow by temperature. Both
    // 0 → no change. Applies on every creature material branch via `emissive`.
    if (u.emit.x > 0.0) {
        let excite = sample_irradiance(n_env).b * env_intensity * ambient_mul;
        emissive += hue2rgb(u.emit.y) * (excite * u.emit.x);
    }
    if (u.emit.z > 0.0) {
        emissive += blackbody(u.emit.w) * u.emit.z;
    }

    // ---- #476 full-PBR parity: reflection controls + material-TYPE overlays, ported
    // from cube.wgsl. GI bounce / SSAO / cast shadows / instance-chord Refractive
    // absorption are NEUTRALISED here (they need cube-only bind groups or instance
    // geometry an SDF lacks). key_rad/fill_w carry no shadow term on the creature. ----
    let key_rad = vec3<f32>(u.key_light.w);
    let fill_w = u.fill_light.w;
    let reflect_tint  = u.reflect_ctl.x;
    let chrome_purity = clamp(u.reflect_ctl.y, 0.0, 1.0);
    let glass_clarity = clamp(u.reflect_ctl.z, 0.0, 1.0);
    let f0_override   = clamp(u.reflect_ctl.w, 0.0, 1.0);
    let refl_palette  = max(vec3<f32>(0.0), mix(vec3<f32>(1.0), albedo, reflect_tint));

    let irid_amount = u.irid.x;
    let irid_raw = iridescent_tint(n_dot_v, u.irid.y, u.irid.z);
    let irid_tint = mix(vec3<f32>(1.0), irid_raw, irid_amount);
    let irid_sheen = irid_raw * (irid_amount * pow(1.0 - n_dot_v, 3.0));

    let is_glassy = mat_type >= 1.5 && mat_type < 3.5;
    // Subsurface (type 7) forces the translucency on even at a 0 dial.
    let is_subsurface_type = mat_type > 6.5 && mat_type < 7.5;
    let sss_amount = select(u.sss.x, select(1.0, u.sss.x, u.sss.x > 0.0), is_subsurface_type);
    let sss_distortion = u.sss.y;
    let sss_power = max(u.sss.z, 1.0);
    var sss = vec3<f32>(0.0);
    if (sss_amount > 0.0) {
        let lobe = translucency_lobe(v, l_key, n, sss_distortion, sss_power) * u.key_light.w
                 + translucency_lobe(v, l_fill, n, sss_distortion, sss_power) * u.fill_light.w;
        sss = albedo * lobe * sss_amount;
    }

    // Anisotropy (#214 T1): type 4 or the Standard/Chrome overlay. The creature has no
    // per-instance brush, so the streak runs along the body's spine axis (c.p4.w).
    let aniso_is_type = mat_type > 3.5 && mat_type < 4.5;
    let aniso_amt = select(
        select(0.0, clamp(u.aniso.x, -1.0, 1.0) * clamp(u.aniso.w, 0.0, 1.0), u.aniso.z > 0.5),
        clamp(u.aniso.x, -1.0, 1.0),
        aniso_is_type);
    let aniso_on = abs(aniso_amt) > 1e-4 && !is_glassy;
    let saxis = i32(c.p4.w + 0.5);
    var brush = vec3<f32>(0.0, 0.0, 1.0);
    if (saxis == 0) { brush = vec3<f32>(1.0, 0.0, 0.0); }
    else if (saxis == 1) { brush = vec3<f32>(0.0, 1.0, 0.0); }
    var af_t = vec3<f32>(1.0, 0.0, 0.0);
    var af_b = vec3<f32>(0.0, 1.0, 0.0);
    var a_t = max(roughness * roughness, 4e-4);
    var a_b = a_t;
    var r_env_a = r_env;
    if (aniso_on) {
        let af = aniso_frame(n, brush, u.aniso.y);
        af_t = af.t;
        af_b = af.b;
        let ab = aniso_alpha(roughness, aniso_amt);
        a_t = ab.x;
        a_b = ab.y;
        r_env_a = rotate_y(aniso_reflect(n, v, af_t, af_b, aniso_amt, roughness), env_rot);
    }

    // Clearcoat (type 5) / Velvet (type 6) overlays (also usable on Standard/Chrome).
    let is_clearcoat_type = mat_type > 4.5 && mat_type < 5.5;
    let is_velvet_type    = mat_type > 5.5 && mat_type < 6.5;
    let coat_amt  = select(select(0.0, u.coat.x, u.coat.z > 0.5), u.coat.x, is_clearcoat_type);
    let sheen_amt = select(select(0.0, u.sheen.x, u.coat.w > 0.5), u.sheen.x, is_velvet_type);
    let coat_on  = coat_amt > 1e-4 && !is_glassy;
    let sheen_on = sheen_amt > 1e-4 && !is_glassy;
    var coat_spec  = vec3<f32>(0.0);
    var sheen_add  = vec3<f32>(0.0);
    var base_scale = 1.0;
    if (coat_on) {
        let cc_rough = clamp(u.coat.y, 0.02, 1.0);
        let fc = (0.04 + 0.96 * pow(1.0 - n_dot_v, 5.0)) * coat_amt;
        let coat_r = rotate_y(reflect(-v, n), env_rot);
        let coat_env = sample_prefiltered(coat_r, cc_rough) * fc * env_intensity * ambient_mul * etint;
        let coat_dir = (direct_light(n, v, l_key, vec3<f32>(0.0), 0.0, cc_rough, vec3<f32>(0.04), key_rad)
                      + direct_light(n, v, l_fill, vec3<f32>(0.0), 0.0, cc_rough, vec3<f32>(0.04), vec3<f32>(fill_w))) * coat_amt;
        coat_spec = coat_env + coat_dir;
        base_scale = base_scale * (1.0 - fc);
    }
    if (sheen_on) {
        let sh_rough = clamp(u.sheen.y, 0.02, 1.0);
        let sheen_col = mix(vec3<f32>(1.0), albedo, clamp(u.sheen.z, 0.0, 1.0)) * sheen_amt;
        let sh = sheen_lobe(n, v, l_key, sh_rough) * key_rad
               + sheen_lobe(n, v, l_fill, sh_rough) * vec3<f32>(fill_w);
        let rim = pow(1.0 - n_dot_v, 2.0) * env_intensity * ambient_mul;
        sheen_add = sheen_col * (sh + rim * 0.5);
        base_scale = base_scale * (1.0 - 0.25 * sheen_amt
            * max(max(sheen_col.r, sheen_col.g), sheen_col.b));
    }

    var shaded: vec3<f32>;
    var alpha = 1.0;
    if (mat_type > 0.5 && mat_type < 1.5) {
        // ===== Chrome: polished mirror, chrome_purity → pure untinted mirror =====
        let f0c_base = mix(vec3<f32>(0.85), albedo, metallic);
        let f0c = mix(f0c_base, vec3<f32>(1.0), chrome_purity);
        let refl_rough = roughness * (1.0 - chrome_purity);
        let brdf_c = textureSampleLevel(brdf_lut_tex, ibl_samp,
            vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0), max(refl_rough, 1e-3)), 0.0).rg;
        let refl = sample_prefiltered(r_env_a, refl_rough);
        let mirror = refl * (f0c * brdf_c.x + vec3<f32>(brdf_c.y))
            * env_intensity * ambient_mul * etint * irid_tint * refl_palette;
        let rough_g = max(refl_rough, 0.02);
        var direct: vec3<f32>;
        if (aniso_on) {
            let cab = aniso_alpha(rough_g, aniso_amt);
            direct = direct_light_aniso(n, v, l_key, af_t, af_b, albedo, 1.0, cab.x, cab.y, f0c, key_rad)
                   + direct_light_aniso(n, v, l_fill, af_t, af_b, albedo, 1.0, cab.x, cab.y, f0c, vec3<f32>(fill_w));
        } else {
            direct = direct_light(n, v, l_key, albedo, 1.0, rough_g, f0c, key_rad)
                   + direct_light(n, v, l_fill, albedo, 1.0, rough_g, f0c, vec3<f32>(fill_w));
        }
        let demo_pt = demo_point_light(hp, n, v, albedo, 1.0, rough_g, f0c);
        shaded = (mirror + direct + emissive + irid_sheen + demo_pt) * base_scale + coat_spec + sheen_add;
    } else if (is_glassy) {
        // ===== Glass / Refractive: reflect + refract, with glass_clarity + reflect_tint.
        // Refractive (3) shares Glass optics here — the Beer–Lambert body absorption needs
        // an instance chord the raymarched SDF doesn't have. =====
        let dispersion = u.glassx.x;
        let caustic = u.glassx.y;
        let thin_film = u.glassx.z;
        let g_rough = roughness * (1.0 - glass_clarity);
        var thru: vec3<f32>;
        if (dispersion > 0.0 || caustic > 0.0) {
            thru = glass_dispersion(v, n, ior, g_rough, env_rot, dispersion, caustic);
        } else {
            let refr_dir = refract(-v, n, 1.0 / ior);
            if (dot(refr_dir, refr_dir) < 1e-6) {
                thru = sample_prefiltered(r_env, g_rough);
            } else {
                thru = sample_prefiltered(rotate_y(refr_dir, env_rot), g_rough);
            }
        }
        let reflected = sample_prefiltered(r_env, g_rough);
        let f0s = (ior - 1.0) / (ior + 1.0);
        let fr = fresnel_schlick_roughness(n_dot_v, vec3<f32>(f0s * f0s), g_rough).x;
        let tint = mix(mix(vec3<f32>(1.0), albedo, 0.5), vec3<f32>(1.0), glass_clarity);
        var film = thin_film_tint(n_dot_v, thin_film);
        if (u.thinfilm.x > 0.0) {
            let refl = thin_film_physical(n_dot_v, hp);
            let lum = max((refl.r + refl.g + refl.b) * 0.3333333, 1e-3);
            film = refl / lum;
        }
        let env_mul = tint * env_intensity * ambient_mul * etint;
        let glass_body = thru * (1.0 - fr) * env_mul;
        let glass_surf = reflected * irid_tint * film * refl_palette * fr * env_mul;
        let dg_rough = max(g_rough, 0.02);
        let glass_f0 = vec3<f32>(f0s * f0s);
        let direct = direct_light(n, v, l_key, albedo, metallic, dg_rough, glass_f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, dg_rough, glass_f0, vec3<f32>(fill_w));
        shaded = glass_body + glass_surf + direct + emissive + sss + irid_sheen;
        alpha = clamp(opacity + (1.0 - opacity) * fr, 0.0, 1.0);
    } else {
        // ===== Standard (+ Anisotropic / Clearcoat / Velvet / Subsurface overlays) =====
        let f0_dielectric = mix(vec3<f32>(0.04), vec3<f32>(0.9), f0_override);
        let f0 = mix(f0_dielectric, albedo, metallic);
        let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
        let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
        let irradiance = sample_irradiance(n_env);
        let diffuse = irradiance * albedo;
        let prefiltered = sample_prefiltered(r_env_a, roughness);
        let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
            vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0), max(roughness, 1e-3)), 0.0).rg;
        // Multiple-scattering energy compensation (#174 T3, Fdez-Agüera).
        let fss_ess = f0 * brdf.x + vec3<f32>(brdf.y);
        let ems = 1.0 - (brdf.x + brdf.y);
        let favg = f0 + (vec3<f32>(1.0) - f0) * (1.0 / 21.0);
        let fms = fss_ess * favg / (vec3<f32>(1.0) - ems * favg);
        let specular = prefiltered * (fss_ess + fms * ems) * irid_tint * refl_palette;
        let ambient = (kd * diffuse + specular) * env_intensity * ambient_mul * etint;
        var direct: vec3<f32>;
        if (aniso_on) {
            direct = direct_light_aniso(n, v, l_key, af_t, af_b, albedo, metallic, a_t, a_b, f0, key_rad)
                   + direct_light_aniso(n, v, l_fill, af_t, af_b, albedo, metallic, a_t, a_b, f0, vec3<f32>(fill_w));
        } else {
            direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(fill_w));
        }
        let demo_pt = demo_point_light(hp, n, v, albedo, metallic, roughness, f0);
        let color = ambient + direct + emissive + sss + irid_sheen + demo_pt;
        shaded = color * base_scale + coat_spec + sheen_add;
    }

    tr.color = shaded;
    tr.alpha = alpha;
    // `hp` is in unscaled world space (rays from c.inv_vp), so project through the
    // matching UNSCALED c.view_proj for a depth that composites with the skybox.
    let clip = c.view_proj * vec4<f32>(hp, 1.0);
    tr.depth = clip.z / clip.w;
    tr.hit = true;
    return tr;
}

@fragment
fn fs_ray(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    let n = max(i32(c.p2.x + 0.5), 1);
    let res = max(c.p2.yz, vec2<f32>(1.0));
    let px = in.clip.xy;
    var acc = vec3<f32>(0.0);
    var acc_a = 0.0;
    var depth = 1.0;
    var hits = 0;
    for (var i = 0; i < n; i = i + 1) {
        let sp = (px + aa_offset(i, n)) / res;
        let ndc = vec2<f32>(sp.x * 2.0 - 1.0, 1.0 - sp.y * 2.0);
        let tr = trace(ro, ray_dir(ndc));
        if (tr.hit) {
            // Premultiplied accumulation: colour weighted by the material's own alpha
            // (1 for Standard/Chrome, < 1 for Glass) so transparency AND edge coverage
            // both fall out of the divide-by-total below.
            acc = acc + tr.color * tr.alpha;
            acc_a = acc_a + tr.alpha;
            depth = min(depth, tr.depth);
            hits = hits + 1;
        }
    }
    if (hits == 0) {
        discard;
    }
    var out: FragOut;
    // Coverage + material premultiplied-alpha: missed sub-samples (and the see-through
    // part of glass) show the already-drawn background through the premultiplied "over"
    // blend — edge AA + glass transparency together (same as minimal/mandelbulb).
    out.color = vec4<f32>(acc / f32(n), acc_a / f32(n));
    out.depth = depth;
    return out;
}

// ===================== depth-only prepass (SSR / SSGI) =====================
// Screen-space reflections (#80 A) + SSGI (#152 T2) reconstruct surface normals from
// the depth prepass and gather / composite in POST, so a depth-only creature march
// into the same single-sample prepass lets those effects see the creature's surface
// (mirrors neural.rs / voxel.rs `fs_ray_depth`). One centre sample (no AA) — the
// effects tolerate the sub-pixel edge difference and it keeps the extra march cheap.
struct DepthOut { @builtin(frag_depth) depth: f32 };

@fragment
fn fs_ray_depth(in: VsOut) -> DepthOut {
    let ro = u.camera_pos.xyz;
    let res = max(c.p2.yz, vec2<f32>(1.0));
    let sp = in.clip.xy / res;
    let ndc = vec2<f32>(sp.x * 2.0 - 1.0, 1.0 - sp.y * 2.0);
    let rd = ray_dir(ndc);

    // Ray vs the creature's bounding sphere.
    let oc = ro - c.center.xyz;
    let bb = dot(oc, rd);
    let cc = dot(oc, oc) - c.center.w * c.center.w;
    let disc = bb * bb - cc;
    if (disc < 0.0) { discard; }
    let sq = sqrt(disc);
    let tmin = max(-bb - sq, 0.0);
    let tmax = -bb + sq;
    if (tmax <= tmin) { discard; }

    let steps = i32(c.p0.y);
    let scale = c.p0.z;
    var t = tmin;
    var hit = false;
    var hp = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * t;
        let d = map_world(p).x;
        let eps = max(0.0004 * t, 0.0003 * scale);
        if (d < eps) {
            hp = p;
            hit = true;
            break;
        }
        t = t + max(d * 0.8, 0.0006 * scale);
        if (t > tmax) { break; }
    }
    if (!hit) { discard; }

    let clip = c.view_proj * vec4<f32>(hp, 1.0);
    var out: DepthOut;
    out.depth = clip.z / clip.w;
    return out;
}

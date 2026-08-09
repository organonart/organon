// Organic Math — Minimal-surface / TPMS isosurface raymarch (GitHub #127 P1).
//
// A sibling of the Mandelbulb raymarch path: a fullscreen pass marches a ray per
// pixel against a triply-periodic minimal surface (gyroid / Schwarz P / Schwarz
// D), defined by the implicit field `F(domain(p)) = iso`. We render the thickened
// wall `|F − iso| ≤ thickness` (a soap-film band; a true film is thickness→floor),
// take the world-space field gradient as the surface normal, and shade it with
// the SAME metallic-roughness IBL + key/fill PBR as cube.wgsl / mandelbulb.wgsl
// (Standard path) — so glass, iridescence (thin-film soap rainbows), SSS and the
// reaction-diffusion dapple all carry straight through. Ray misses `discard` so
// the skybox shows through; hits write `frag_depth` so the surface
// depth-composites with the skybox and feeds bloom.
//
// `tpms()` mirrors math.rs::tpms_field 1:1. Outputs LINEAR HDR radiance
// (exposure/bloom/tonemap happen in the composite).

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
    thinfilm: vec4<f32>,    // physical thin-film (#258 T1): x=base thickness (nm; 0 → OFF,
                            // keep the cosine path), y=marbling, z=film IOR, w=drainage
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// ----- group(1): IBL maps + filtering sampler (same layout as cube.wgsl) -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): Minimal-surface params -----
struct MinimalU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED); for frag_depth
    p0: vec4<f32>,          // family, scale, cells, iso
    p1: vec4<f32>,          // thickness, twist, steps, color_intensity
    p2: vec4<f32>,          // color_phase, _, _, _
    center: vec4<f32>,      // xyz = world centre, w = bound-sphere radius
};
@group(2) @binding(0) var<uniform> m: MinimalU;

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

// --- Glass / thin-film helpers (ported 1:1 from cube.wgsl) ---
fn thin_film_tint(cos_theta: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return vec3<f32>(1.0);
    }
    let opd = (1.0 - clamp(cos_theta, 0.0, 1.0)) * 6.2831853 * (2.0 + amount * 6.0);
    let tint = 0.5 + 0.5 * cos(vec3<f32>(opd) + vec3<f32>(0.0, 2.0944, 4.1888));
    return mix(vec3<f32>(1.0), tint, clamp(amount, 0.0, 1.0));
}

// --- Physical thin-film interference (#258 T1, ported 1:1 from cube.wgsl) ---
// Wavelength-resolved Airy model of a real soap film / bubble; gated by
// `u.thinfilm.x` (base thickness nm) > 0. See cube.wgsl for the full derivation.
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

fn ripple_emission(world_pos: vec3<f32>, albedo: vec3<f32>) -> vec3<f32> {
    let intensity = u.ripple.x;
    if (intensity <= 0.0) {
        return vec3<f32>(0.0);
    }
    let phase = u.ripple.y;
    let freq = max(u.ripple.z, 0.0);
    let sharp = max(u.ripple.w, 1.0);
    let center = u.ripple_ctr.xyz;
    let radius = max(u.ripple_ctr.w, 1e-3);
    var coord: f32;
    if (u.ripple_mode.x >= 0.5) {
        let axis = normalize(u.ripple_mode.yzw);
        coord = dot(world_pos - center, axis) / radius * 0.5 + 0.5;
    } else {
        coord = length(world_pos - center) / radius;
    }
    let band = pow(0.5 + 0.5 * cos(6.2831853 * (coord * freq - phase)), sharp);
    return albedo * (intensity * band);
}

// ===================== TPMS implicit field =====================

// The triply-periodic minimal-surface field at a DOMAIN-space point (period 2π).
// Mirrors math.rs::tpms_field 1:1. The surface is the level set tpms == iso.
fn tpms(p: vec3<f32>, family: u32) -> f32 {
    let s = sin(p);
    let c = cos(p);
    if (family == 1u) {
        // Schwarz P.
        return c.x + c.y + c.z;
    } else if (family == 2u) {
        // Schwarz D (diamond), trig (nodal) approximation.
        return s.x * s.y * s.z + s.x * c.y * c.z + c.x * s.y * c.z + c.x * c.y * s.z;
    }
    // Gyroid (default).
    return s.x * c.y + s.y * c.z + s.z * c.x;
}

// ----- Bubbles & foam (#127 Phase 3) — mirror math.rs 1:1 -----

fn fract1(x: f32) -> f32 { return x - floor(x); }

fn hash3(c: vec3<f32>) -> vec3<f32> {
    let p = vec3<f32>(
        dot(c, vec3<f32>(127.1, 311.7, 74.7)),
        dot(c, vec3<f32>(269.5, 183.3, 246.1)),
        dot(c, vec3<f32>(113.5, 271.9, 124.6)),
    );
    return vec3<f32>(
        fract1(sin(p.x) * 43758.547),
        fract1(sin(p.y) * 43758.547),
        fract1(sin(p.z) * 43758.547),
    );
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// Soap-bubble cluster: smooth-union of one sphere per unit cell over the 3³
// neighbourhood (signed; negative inside, the `= iso` shell is the bubble film).
fn bubble_field(q: vec3<f32>) -> f32 {
    let cell = floor(q);
    let radius = 0.42;
    let k = 0.22;
    var d = 1e9;
    for (var dz = -1; dz <= 1; dz = dz + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let c = cell + vec3<f32>(f32(dx), f32(dy), f32(dz));
                let center = c + vec3<f32>(0.5);
                let sd = length(q - center) - radius;
                d = smin(d, sd, k);
            }
        }
    }
    return d;
}

// Voronoi froth: F2 − F1 over jittered per-cell seeds, zero on the Plateau-border
// walls (the `≈ iso` band is the dry-foam film).
fn foam_field(q: vec3<f32>) -> f32 {
    let cell = floor(q);
    // Track the two smallest SQUARED distances (no per-cell sqrt — 27 → 2 sqrts).
    var d1 = 1e18;
    var d2 = 1e18;
    for (var dz = -1; dz <= 1; dz = dz + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let c = cell + vec3<f32>(f32(dx), f32(dy), f32(dz));
                let seed = c + vec3<f32>(0.5) + (hash3(c) - vec3<f32>(0.5)) * 0.8;
                let diff = q - seed;
                let dd = dot(diff, diff);
                if (dd < d1) {
                    d2 = d1;
                    d1 = dd;
                } else if (dd < d2) {
                    d2 = dd;
                }
            }
        }
    }
    return sqrt(d2) - sqrt(d1);
}

// The implicit field for the raymarched families — TPMS (0..2) or bubble/foam (6,7).
fn implicit_field(p: vec3<f32>, family: u32) -> f32 {
    if (family == 6u) { return bubble_field(p); }
    if (family == 7u) { return foam_field(p); }
    return tpms(p, family);
}

// ----- Algebraic-surface bank (#127 Phase 4) — mirror math.rs 1:1 -----
// Classic implicit polynomials F(x,y,z)=0 on a UNIT-range point (scaled per surface).

fn clebsch_field(p: vec3<f32>) -> f32 {
    let x = p.x; let y = p.y; let z = p.z;
    return 81.0 * (x*x*x + y*y*y + z*z*z)
        - 189.0 * (x*x*y + x*x*z + y*y*x + y*y*z + z*z*x + z*z*y)
        + 54.0 * x*y*z
        + 126.0 * (x*y + x*z + y*z)
        - 9.0 * (x*x + y*y + z*z)
        - 9.0 * (x + y + z)
        + 1.0;
}

fn barth_field(p: vec3<f32>) -> f32 {
    let phi = 1.618034;
    let x2 = p.x*p.x; let y2 = p.y*p.y; let z2 = p.z*p.z;
    let phi2 = phi*phi;
    let r2 = x2 + y2 + z2 - 1.0;
    return 4.0 * (phi2*x2 - y2) * (phi2*y2 - z2) * (phi2*z2 - x2)
        - (1.0 + 2.0*phi) * r2 * r2;
}

fn kummer_field(p: vec3<f32>) -> f32 {
    let x = p.x; let y = p.y; let z = p.z;
    let mu2 = 1.3;
    let lambda = (3.0*mu2 - 1.0) / (3.0 - mu2);
    let s = 1.4142136;
    let p1 = 1.0 - z - x*s;
    let p2 = 1.0 - z + x*s;
    let p3 = 1.0 + z + y*s;
    let p4 = 1.0 + z - y*s;
    let q = x*x + y*y + z*z - mu2;
    return q*q - lambda * p1*p2*p3*p4;
}

fn heart_field(p: vec3<f32>) -> f32 {
    let x = p.x; let y = p.y; let z = p.z;
    let a = x*x + 2.25*y*y + z*z - 1.0;
    return a*a*a - x*x*z*z*z - 0.1125*y*y*z*z*z;
}

fn tangle_field(p: vec3<f32>) -> f32 {
    let x2 = p.x*p.x; let y2 = p.y*p.y; let z2 = p.z*p.z;
    return x2*x2 - 5.0*x2 + y2*y2 - 5.0*y2 + z2*z2 - 5.0*z2 + 11.8;
}

fn algebraic_field(p: vec3<f32>, family: u32) -> f32 {
    if (family == 8u) { return clebsch_field(p * 3.2); }
    if (family == 9u) { return barth_field(p * 1.9); }
    if (family == 10u) { return kummer_field(p * 1.9); }
    if (family == 11u) { return heart_field(p * 1.3); }
    return tangle_field(p * 2.6);
}

// World point → domain point: recentre, normalise by `scale`, scale to `cells`
// periods, and twist the xy plane about the vertical (domain churn / shear).
fn to_domain(world_p: vec3<f32>) -> vec3<f32> {
    let unit = (world_p - m.center.xyz) / m.p0.y; // normalise by scale
    let twist = m.p1.y * unit.z;
    let cw = cos(twist);
    let sw = sin(twist);
    let tw = vec3<f32>(unit.x * cw - unit.y * sw, unit.x * sw + unit.y * cw, unit.z);
    return tw * m.p0.z; // cells
}

// Signed field minus the isolevel: zero on the surface, ± in the two channels.
fn map_signed(world_p: vec3<f32>) -> f32 {
    let family = u32(m.p0.x + 0.5);
    if (family >= 8u) {
        // Algebraic surfaces are framed in the UNIT sphere (each internally scaled),
        // so evaluate on the recentred/normalised point — not the cells-tiled domain.
        let unit = (world_p - m.center.xyz) / m.p0.y;
        return algebraic_field(unit, family) - m.p0.w; // − iso
    }
    return implicit_field(to_domain(world_p), family) - m.p0.w; // − iso
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
    let near = m.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = m.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    out.rd = (far.xyz / far.w) - (near.xyz / near.w);
    return out;
}

// ===================== fragment (raymarch + shade) =====================
struct FragOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// One traced ray: linear HDR colour + framebuffer depth, or hit = false on a miss
// (so the caller composites the background through).
struct Trace {
    color: vec3<f32>,
    depth: f32,
    hit: bool,
    alpha: f32, // material opacity (1 = Standard/Chrome; < 1 = Glass, Fresnel-lifted)
};

// Reconstruct a world-space ray direction for a clip-space NDC point (used for
// per-sub-sample supersampling rays — the unscaled inv-VP keeps the surface put).
fn ray_dir(ndc: vec2<f32>) -> vec3<f32> {
    let near = m.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = m.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize((far.xyz / far.w) - (near.xyz / near.w));
}

// Sub-pixel offset (in pixel units) for sample `i` of `n` — a fullscreen
// raymarch gets no MSAA (full coverage → one fragment), so we supersample here.
// 2× = a diagonal pair; 4× = a rotated grid (RGSS) for clean near-horizontal edges.
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

fn trace(ro: vec3<f32>, rd: vec3<f32>) -> Trace {
    var tr: Trace;
    tr.hit = false;
    tr.color = vec3<f32>(0.0);
    tr.depth = 1.0;
    tr.alpha = 1.0;

    // Ray vs the structure's bounding sphere (centre, radius = center.w).
    let oc = ro - m.center.xyz;
    let b = dot(oc, rd);
    let cc = dot(oc, oc) - m.center.w * m.center.w;
    let disc = b * b - cc;
    if (disc < 0.0) {
        return tr;
    }
    let sq = sqrt(disc);
    var tmin = max(-b - sq, 0.0);
    let tmax = -b + sq;
    if (tmax <= tmin) {
        return tr;
    }

    // The wall is the band |F − iso| ≤ thickness. A small floor keeps a thin film
    // from aliasing (the undersampled-zero-set lesson).
    let family = u32(m.p0.x + 0.5);
    let steps = i32(m.p1.z);
    let thickness = max(m.p1.x, 0.012); // p1 = [thickness, twist, steps, color]
    var t = tmin;
    var hit = false;
    var hp = vec3<f32>(0.0);

    if (family == 6u || family == 7u) {
        // Bubbles/foam are (approximately) DISTANCE fields, so SPHERE-TRACE them:
        // take big jumps through empty space and early-out — a fixed-step march of
        // these 27-cell fields was the perf cliff. The domain-space field converts to
        // a safe world step via its Lipschitz bound (bubble ≈ 1, foam's F2−F1 ≈ 2·dist)
        // and the domain→world scale (scale / cells).
        let lipschitz = select(1.0, 2.0, family == 7u);
        // `to_domain`'s twist is a SHEAR, so when twist ≠ 0 the field changes faster
        // in world space (tangential stretch ≤ r·twist, r ≤ the ~1.35 unit bound
        // radius). Shrink the step by that factor so we never overstep a thin film.
        let twist_stretch = 1.0 + abs(m.p1.y) * 1.35;
        let wpd = (m.p0.y / max(m.p0.z, 0.01)) / twist_stretch;
        var prev_s = map_signed(ro + rd * t);
        var prev_t = t;
        if (abs(prev_s) <= thickness) {
            hp = ro + rd * t; // started already inside the band
            hit = true;
        }
        for (var i = 0; i < steps && !hit; i = i + 1) {
            // No min-step floor: past the band check `abs(prev_s) > thickness`, so the
            // distance-field step is already bounded below — and a floor could exceed
            // the band-safe step and overshoot a thin film. The 0.8 factor keeps every
            // step ≤ 0.8 · distance-to-surface, so we converge without overstepping.
            var next_t = prev_t + abs(prev_s) / lipschitz * wpd * 0.8;
            // Clamp the final step to the bound exit so `[prev_t, tmax]` IS sampled
            // (a surface in the last segment was previously missed on `t > tmax`).
            let last = next_t >= tmax;
            next_t = min(next_t, tmax);
            let s = map_signed(ro + rd * next_t);
            // Inside the band, OR the signed field flipped (a big step jumped across
            // the surface without sampling the thin band) → bisect onto the band edge.
            if (abs(s) <= thickness || (s * prev_s) < 0.0) {
                var lo = prev_t;
                var hi = next_t;
                for (var k = 0; k < 12; k = k + 1) {
                    let mid = 0.5 * (lo + hi);
                    let sm = map_signed(ro + rd * mid);
                    if (abs(sm) <= thickness) { hi = mid; } else { lo = mid; }
                }
                hp = ro + rd * hi;
                hit = true;
            }
            if (last) { break; }
            prev_s = s;
            prev_t = next_t;
        }
    } else {
        // TPMS is NOT a distance field once scaled, so march in conservative fixed
        // steps and refine the wall boundary by bisection.
        let span = tmax - tmin;
        let dt = span / f32(steps);
        var prev_out = map_signed(ro + rd * tmin); // signed field at entry
        var prev_t = tmin;
        for (var i = 0; i < steps; i = i + 1) {
            t = t + dt;
            if (t > tmax) { break; }
            let p = ro + rd * t;
            let s = map_signed(p);
            // Entered the wall band, or crossed the surface between samples.
            if (abs(s) <= thickness || (s * prev_out) < 0.0) {
                // Bisect between the last sample and this one toward the band edge.
                var lo = prev_t;
                var hi = t;
                for (var k = 0; k < 14; k = k + 1) {
                    let mid = 0.5 * (lo + hi);
                    let sm = map_signed(ro + rd * mid);
                    if (abs(sm) <= thickness) { hi = mid; } else { lo = mid; }
                }
                hp = ro + rd * hi;
                hit = true;
                break;
            }
            prev_out = s;
            prev_t = t;
        }
    }
    if (!hit) {
        return tr;
    }

    // World-space surface normal from the signed-field gradient (central
    // differences). Sign so the normal faces the camera-side of the wall.
    let h = max(0.0015 * m.p0.y, 0.002);
    let dx = vec3<f32>(h, 0.0, 0.0);
    let dy = vec3<f32>(0.0, h, 0.0);
    let dz = vec3<f32>(0.0, 0.0, h);
    var n = normalize(vec3<f32>(
        map_signed(hp + dx) - map_signed(hp - dx),
        map_signed(hp + dy) - map_signed(hp - dy),
        map_signed(hp + dz) - map_signed(hp - dz),
    ));
    if (dot(n, n) < 1e-12) {
        n = -rd;
    }
    // The wall has two faces; orient toward the viewer for stable shading.
    if (dot(n, ro - hp) < 0.0) {
        n = -n;
    }

    // Colour: a gentle channel band from the domain coordinate phase. color 0 →
    // near-white (let the IBL/material speak); 1 → saturated bands. color_phase
    // cycles the gradient (rides the bioluminescent colour clock).
    let color_intensity = clamp(m.p1.w, 0.0, 1.0);
    // Phase the colour band from the SAME coordinate the field uses: the algebraic
    // families (≥ 8) evaluate on the recentred unit point (cells/twist inert), so
    // their banding must too — otherwise cells/twist would shift colour without
    // changing the surface. TPMS/bubbles/foam keep the cells-tiled domain.
    var dpos: vec3<f32>;
    if (u32(m.p0.x + 0.5) >= 8u) {
        dpos = (hp - m.center.xyz) / m.p0.y; // unit point, matching algebraic_field
    } else {
        dpos = to_domain(hp);
    }
    let phase = (dpos.x + dpos.y + dpos.z) * 0.5 + m.p2.x * 6.2831853;
    let band = 0.5 + 0.5 * sin(vec3<f32>(0.0, 2.0944, 4.1888) + phase);
    let base_albedo = mix(vec3<f32>(0.85), band, color_intensity);
    let rd_d = rd_dapple(hp, n);
    let albedo = mix(base_albedo, base_albedo * rd_d, clamp(rdu.params.z, 0.0, 1.0));

    // ---- Material-branched shading (Standard / Chrome / Glass), env-only — the
    // same metallic-roughness PBR + IBL + key/fill the cubes use, so the Material
    // selector now drives these raymarched surfaces too. ----
    let metallic = clamp(u.mat.x, 0.0, 1.0);
    let roughness = clamp(u.mat.y, 0.0, 1.0);
    let glow = u.mat.z;
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
    let emissive = albedo * glow + ripple_emission(hp, albedo)
        + base_albedo * (rd_d * rdu.params.x);

    let sss_amount = u.sss.x;
    let sss_distortion = u.sss.y;
    let sss_power = max(u.sss.z, 1.0);
    let irid_amount = u.irid.x;
    let irid_raw = iridescent_tint(n_dot_v, u.irid.y, u.irid.z);
    let irid_tint = mix(vec3<f32>(1.0), irid_raw, irid_amount);
    let irid_sheen = irid_raw * (irid_amount * pow(1.0 - n_dot_v, 3.0));
    var sss = vec3<f32>(0.0);
    if (sss_amount > 0.0) {
        let lobe = translucency_lobe(v, l_key, n, sss_distortion, sss_power) * u.key_light.w
                 + translucency_lobe(v, l_fill, n, sss_distortion, sss_power) * u.fill_light.w;
        sss = albedo * lobe * sss_amount;
    }

    // Intrinsic thin-film iridescence for the soap families (bubble/foam only):
    // a grazing-angle rainbow wired on by default. Inert for TPMS + algebraic.
    var soap_sheen = vec3<f32>(0.0);
    if (family == 6u || family == 7u) {
        // `u.thinfilm.x` (base thickness nm) > 0 → the PHYSICAL Airy model drives the
        // rainbow from a real thickness field (drainage + marbling); 0 → the legacy
        // grazing-angle cosine sheen (byte-identical default).
        var film: vec3<f32>;
        if (u.thinfilm.x > 0.0) {
            film = thin_film_physical(n_dot_v, hp);
        } else {
            film = iridescent_tint(n_dot_v, 3.5, m.p2.x);
        }
        soap_sheen = film * (0.22 + 0.55 * pow(1.0 - n_dot_v, 2.0));
    }

    var shaded: vec3<f32>;
    var alpha = 1.0;
    if (mat_type > 0.5 && mat_type < 1.5) {
        // ===== Chrome: a polished mirror reflecting the environment =====
        let f0c = mix(vec3<f32>(0.85), albedo, metallic);
        let fres = fresnel_schlick_roughness(n_dot_v, f0c, roughness);
        let mirror = sample_prefiltered(r_env, roughness) * fres
                   * env_intensity * ambient_mul * etint * irid_tint;
        let rough_g = max(roughness, 0.02);
        let direct = direct_light(n, v, l_key, albedo, 1.0, rough_g, f0c, vec3<f32>(u.key_light.w))
                   + direct_light(n, v, l_fill, albedo, 1.0, rough_g, f0c, vec3<f32>(u.fill_light.w));
        shaded = mirror + direct + emissive + irid_sheen;
    } else if (mat_type >= 1.5) {
        // ===== Glass: Fresnel-blended reflect + refract of the environment =====
        let dispersion = u.glassx.x;
        let caustic = u.glassx.y;
        let thin_film = u.glassx.z;
        var thru: vec3<f32>;
        if (dispersion > 0.0 || caustic > 0.0) {
            thru = glass_dispersion(v, n, ior, roughness, env_rot, dispersion, caustic);
        } else {
            let refr_dir = refract(-v, n, 1.0 / ior);
            if (dot(refr_dir, refr_dir) < 1e-6) {
                thru = sample_prefiltered(r_env, roughness); // total internal reflection
            } else {
                thru = sample_prefiltered(rotate_y(refr_dir, env_rot), roughness);
            }
        }
        let reflected = sample_prefiltered(r_env, roughness);
        let fr = fresnel_schlick_roughness(n_dot_v, vec3<f32>(0.04), roughness).x;
        let tint = mix(vec3<f32>(1.0), albedo, 0.5);
        // Thin-film sheen: `u.thinfilm.x` > 0 → the physical Airy model (normalized to
        // a mean-1 colour ratio so it tints like the legacy multiplier); 0 → the
        // cosine-hack `thin_film_tint` (byte-identical default).
        var film = thin_film_tint(n_dot_v, thin_film);
        if (u.thinfilm.x > 0.0) {
            let refl = thin_film_physical(n_dot_v, hp);
            let lum = max((refl.r + refl.g + refl.b) * 0.3333333, 1e-3);
            film = refl / lum;
        }
        let glass = mix(thru, reflected * irid_tint * film, fr)
                  * tint * env_intensity * ambient_mul * etint;
        let direct = direct_light(n, v, l_key, albedo, metallic, roughness, vec3<f32>(0.04), vec3<f32>(u.key_light.w))
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, vec3<f32>(0.04), vec3<f32>(u.fill_light.w));
        shaded = glass + direct + emissive + sss + irid_sheen;
        // Edge-on faces read more opaque (Fresnel-lifted); face-on is clearer.
        alpha = clamp(opacity + (1.0 - opacity) * fr, 0.0, 1.0);
    } else {
        // ===== Standard: metallic-roughness PBR (IBL + direct) =====
        let f0 = mix(vec3<f32>(0.04), albedo, metallic);
        let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
        let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
        let irradiance = sample_irradiance(n_env);
        let diffuse = irradiance * albedo;
        let prefiltered = sample_prefiltered(r_env, roughness);
        let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                      vec2<f32>(n_dot_v, max(roughness, 1e-3)), 0.0).rg;
        let specular = prefiltered * (f0 * brdf.x + vec3<f32>(brdf.y)) * irid_tint;
        let ambient = (kd * diffuse + specular) * env_intensity * ambient_mul * etint;
        let direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, vec3<f32>(u.key_light.w))
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(u.fill_light.w));
        shaded = ambient + direct + emissive + sss + irid_sheen;
    }

    tr.color = shaded + soap_sheen;
    tr.alpha = alpha;
    // `hp` is unscaled world space (rays from m.inv_vp); project through the
    // matching UNSCALED m.view_proj for a depth that composites with the skybox.
    let clip = m.view_proj * vec4<f32>(hp, 1.0);
    tr.depth = clip.z / clip.w;
    tr.hit = true;
    return tr;
}

@fragment
fn fs_ray(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    // Supersample: cast `n` jittered rays per pixel and average. `m.p2.y` = the
    // MSAA sample count (1/2/4) the rest of the scene uses; `m.p2.zw` = the render
    // target size (px). A miss contributes the background (no accumulation); if
    // every sub-sample misses, discard so the skybox shows cleanly.
    // The bubble/foam fields are far heavier (27-cell loops), so cap their
    // supersampling at 2× — each sub-ray reruns the whole march.
    var n = max(i32(m.p2.y + 0.5), 1);
    if (u32(m.p0.x + 0.5) >= 6u) {
        n = min(n, 2);
    }
    let res = max(m.p2.zw, vec2<f32>(1.0));
    let px = in.clip.xy; // framebuffer pixel coordinate
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
    // part of glass) contribute the background already in this HDR attachment. Divide
    // the premultiplied colour AND the summed alpha by the TOTAL sample count; the
    // pipeline blends `src·1 + dst·(1−α)`, so the pixel resolves to
    // [Σ colour·α + background·(n − Σα)] / n — edge AA + glass transparency together.
    // Opaque interior (every α = 1) reduces to the old average-and-overwrite.
    out.color = vec4<f32>(acc / f32(n), acc_a / f32(n));
    out.depth = depth;
    return out;
}

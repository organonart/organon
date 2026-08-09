// Organic Math — Neural field (#200 Tier 1) raymarch.
//
// A sibling of the Mandelbulb raymarch path: a fullscreen pass marches a ray per
// pixel against an implicit isosurface of a tiny SIREN MLP `(x,y,z,t) →
// (density, r, g, b)` — the network from `mlp.wgsl` (regenerated inline from two
// integer seeds + a latent-walk `t`), so the whole organism is a seed and the
// beat drives a continuous morph between seeds. The MLP output is NOT a true
// signed-distance field, so this uses a robust FIXED-STEP march (uniform samples
// through the fractal's bounding sphere, sign-change detection on `density − iso`,
// linear crossing refine) rather than sphere tracing; the surface normal is a
// tetrahedron gradient of the density, and the hit shades with the SAME
// metallic-roughness IBL + key/fill PBR as cube.wgsl / mandelbulb.wgsl. Ray
// misses `discard` so the skybox shows through; hits write `frag_depth` so the
// surface depth-composites with the skybox and feeds bloom.
//
// `mlp.wgsl` is concatenated ABOVE this file (it defines mlp_eval / MLP_*), so
// this file must not redefine those. Outputs LINEAR HDR radiance.

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
    // Material tail — the SAME fields cube.wgsl declares (this struct is a byte
    // prefix of the shared buffer). Reading them lets the neural field honour the
    // Material card (Chrome / Glass / Refractive / Anisotropic), not just Standard.
    glassx: vec4<f32>,      // spectral glass: x=dispersion, y=caustic, z=thin_film, w=spec-occ enable
    reflect_ctl: vec4<f32>, // x=reflect_tint, y=chrome_purity, z=glass_clarity, w=f0_override
    refl_box_min: vec4<f32>,// reflection probe (unused here): xyz=box min, w=source_id
    refl_box_max: vec4<f32>,// xyz=box max, w=parallax blend
    refr: vec4<f32>,        // x=Beer–Lambert absorption strength (Refractive), yzw overlay (unused here)
    aniso: vec4<f32>,       // x=amount(−1..1), y=brush rotation(rad), z=overlay enable, w=overlay blend
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// ----- group(1): IBL maps + filtering sampler (same layout as cube.wgsl) -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): Neural field params -----
struct NeuralU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED); for frag_depth
    p0: vec4<f32>,          // seed_a, seed_b, walk, omega
    p1: vec4<f32>,          // world_scale, coord_scale, iso, surface_smooth
    p2: vec4<f32>,          // steps, color_intensity, time, samples
    p3: vec4<f32>,          // size.x, size.y, _, _
    center: vec4<f32>,      // xyz = world centre, w = bound-sphere radius
};
@group(2) @binding(0) var<uniform> m: NeuralU;

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
// Bioluminescent emissive ripple — the same travelling HDR band as
// mandelbulb.wgsl / minimal.wgsl, so the beat-driven ripple/pulse also lights the
// neural field (#224 review). intensity 0 → no contribution.
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

// --- Anisotropy (ported 1:1 from cube.wgsl; the brush comes from a world-space
//     reference here instead of a per-vertex instance axis — see the shade site) ---
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
    if (amount < 0.0) { let tmp = at; at = ab; ab = tmp; }
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

// --- Glass / spectral glass (ported 1:1 from cube.wgsl) ---
fn thin_film_tint(cos_theta: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) { return vec3<f32>(1.0); }
    let opd = (1.0 - clamp(cos_theta, 0.0, 1.0)) * 6.2831853 * (2.0 + amount * 6.0);
    let tint = 0.5 + 0.5 * cos(vec3<f32>(opd) + vec3<f32>(0.0, 2.0944, 4.1888));
    return mix(vec3<f32>(1.0), tint, clamp(amount, 0.0, 1.0));
}
fn sample_refract_chan(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32) -> vec3<f32> {
    let refr_dir = refract(-v, n, 1.0 / max(ior, 1.0));
    if (dot(refr_dir, refr_dir) < 1e-6) {
        return sample_prefiltered(rotate_y(reflect(-v, n), env_rot), roughness);
    }
    return sample_prefiltered(rotate_y(refr_dir, env_rot), roughness);
}
fn glass_dispersion(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32,
                    dispersion: f32, caustic: f32) -> vec3<f32> {
    let spread = dispersion * 0.06;
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

// ===================== the neural field =====================

// Evaluate the network at a WORLD-space point. Returns the raw 4-vector
// (x = density, yzw = colour logits). Transforms world → unit space, feeds
// (x·coord_scale, y·, z·, time) through the SIREN MLP walked between the seeds.
fn neural_eval(world_p: vec3<f32>) -> vec4<f32> {
    let sa = u32(max(m.p0.x, 0.0));
    let sb = u32(max(m.p0.y, 0.0));
    let pu = (world_p - m.center.xyz) / max(m.p1.x, 1e-3);
    let inp = vec4<f32>(pu * m.p1.y, m.p2.z);
    return mlp_eval(sa, sb, m.p0.z, inp, m.p0.w);
}
fn neural_density(world_p: vec3<f32>) -> f32 {
    return neural_eval(world_p).x;
}

// Distance from `p` along `dir` to the far side of the field's bounding sphere,
// as a 0..1 fraction of the diameter — a cheap thickness proxy for Refractive
// glass's Beer–Lambert through-body absorption (no per-vertex box like the cubes,
// so the bound chord stands in: thick centres absorb more, thin rims stay clear).
fn bound_chord_frac(p: vec3<f32>, dir: vec3<f32>) -> f32 {
    let oc = p - m.center.xyz;
    let b = dot(oc, dir);
    let disc = b * b - (dot(oc, oc) - m.center.w * m.center.w);
    if (disc < 0.0) { return 0.0; }
    let t = max(-b + sqrt(disc), 0.0);
    return clamp(t / max(2.0 * m.center.w, 1e-3), 0.0, 1.0);
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
struct Trace {
    color: vec3<f32>,
    depth: f32,
    hit: bool,
};

fn ray_dir(ndc: vec2<f32>) -> vec3<f32> {
    let near = m.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = m.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize((far.xyz / far.w) - (near.xyz / near.w));
}
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

    // Ray vs the field's bounding sphere.
    let oc = ro - m.center.xyz;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - m.center.w * m.center.w;
    let disc = b * b - c;
    if (disc < 0.0) {
        return tr;
    }
    let sq = sqrt(disc);
    let tmin = max(-b - sq, 0.0);
    let tmax = -b + sq;
    if (tmax <= tmin) {
        return tr;
    }

    // Fixed-step isosurface march: uniform samples across the sphere segment,
    // hit on the first sign change of (density − iso). The MLP is not a true SDF
    // so a uniform march (not sphere tracing) is the robust choice.
    let iso = m.p1.z;
    let steps = max(i32(m.p2.x + 0.5), 8);
    let dt = (tmax - tmin) / f32(steps);
    var t = tmin;
    var prev = neural_density(ro + rd * t) - iso;
    var hit = false;
    var hp = vec3<f32>(0.0);
    for (var i = 1; i <= steps; i = i + 1) {
        let tc = tmin + dt * f32(i);
        let cur = neural_density(ro + rd * tc) - iso;
        // Any sign change of (density − iso) is a surface crossing — detect BOTH
        // directions (not just above→below), so rays that enter through a lobe
        // whose field sign is inverted, or that start below the iso, still hit
        // (#224 review). The linear-crossing interp + the normal flip below handle
        // orientation either way.
        if ((prev > 0.0) != (cur > 0.0)) {
            // Linear crossing between the last two samples. `prev - cur` is nonzero
            // on a sign change and carries the correct sign for either direction
            // (a fixed `max(.., ε)` would break the upward crossing); clamp guards
            // the degenerate near-zero case.
            let frac = clamp(prev / (prev - cur), 0.0, 1.0);
            let th = tc - dt + dt * frac;
            hp = ro + rd * th;
            hit = true;
            break;
        }
        prev = cur;
    }
    if (!hit) {
        return tr;
    }

    // Surface normal: tetrahedron gradient of the density. `surface_smooth`
    // scales the sampling epsilon (larger = smoother, softer normals).
    let hstep = max(m.p1.w, 0.05) * dt * 0.75 + 1e-4;
    let k = vec2<f32>(1.0, -1.0);
    var n = normalize(
        k.xyy * neural_density(hp + k.xyy * hstep)
      + k.yyx * neural_density(hp + k.yyx * hstep)
      + k.yxy * neural_density(hp + k.yxy * hstep)
      + k.xxx * neural_density(hp + k.xxx * hstep));
    // The density decreases into the solid (we hit where it crosses below iso),
    // so the outward normal is the +gradient; flip if it faces away from the ray.
    if (dot(n, n) < 1e-12) { n = -rd; }
    if (dot(n, rd) > 0.0) { n = -n; }

    // Per-point colour from the network's colour logits, mapped to a bounded,
    // vivid albedo; colour_intensity 0 → near-white, 1 → the saturated network hue.
    let logits = neural_eval(hp).yzw;
    let color_intensity = clamp(m.p2.y, 0.0, 1.0);
    let band = 0.5 + 0.5 * sin(logits * PI + vec3<f32>(0.0, 2.0944, 4.1888));
    let base_albedo = mix(vec3<f32>(0.85), band, color_intensity);
    let rd_d = rd_dapple(hp, n);
    let albedo = mix(base_albedo, base_albedo * rd_d, clamp(rdu.params.z, 0.0, 1.0));

    // ---- material inputs (metallic-roughness PBR + the Material card, as cube.wgsl) ----
    let metallic = clamp(u.mat.x, 0.0, 1.0);
    let roughness = clamp(u.mat.y, 0.0, 1.0);
    let glow = u.mat.z;
    let env_intensity = u.env.y;
    let env_rot = u.env.z;
    let ambient_mul = u.amb.x;
    let etint = u.env_tint.rgb;
    let mat_type = u.amb.y;                        // 0 Standard, 1 Chrome, 2 Glass, 3 Refractive, 4 Anisotropic
    let ior = max(u.amb.z, 1.0);
    let chrome_purity = clamp(u.reflect_ctl.y, 0.0, 1.0);
    let glass_clarity = clamp(u.reflect_ctl.z, 0.0, 1.0);
    let f0_override = clamp(u.reflect_ctl.w, 0.0, 1.0);

    let v = normalize(u.camera_pos.xyz - hp);
    let n_dot_v = max(dot(n, v), 1e-4);
    var r = reflect(-v, n);
    let n_env = rotate_y(n, env_rot);

    let l_key = normalize(u.key_light.xyz);
    let l_fill = normalize(u.fill_light.xyz);
    let key_rad = vec3<f32>(u.key_light.w);
    let fill_w = u.fill_light.w;
    let emissive = albedo * glow + base_albedo * (rd_d * rdu.params.x)
                 + ripple_emission(hp, base_albedo);

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

    // Anisotropy frame (#214): active on the Anisotropic material (type 4) or the
    // overlay on Standard/Chrome. No per-vertex brush on a raymarch, so the brush
    // is world-up projected onto the surface; the rotation dial re-aims it.
    let aniso_is_type = mat_type > 3.5 && mat_type < 4.5;
    let is_glassy = mat_type >= 1.5 && mat_type < 3.5;
    let aniso_amt = select(
        select(0.0, clamp(u.aniso.x, -1.0, 1.0) * clamp(u.aniso.w, 0.0, 1.0), u.aniso.z > 0.5),
        clamp(u.aniso.x, -1.0, 1.0),
        aniso_is_type);
    let aniso_on = abs(aniso_amt) > 1e-4 && !is_glassy;
    var af_t = vec3<f32>(1.0, 0.0, 0.0);
    var af_b = vec3<f32>(0.0, 1.0, 0.0);
    var a_t = max(roughness * roughness, 4e-4);
    var a_b = a_t;
    if (aniso_on) {
        let af = aniso_frame(n, vec3<f32>(0.0, 1.0, 0.0), u.aniso.y);
        af_t = af.t;
        af_b = af.b;
        let ab = aniso_alpha(roughness, aniso_amt);
        a_t = ab.x;
        a_b = ab.y;
        let is_chrome = mat_type > 0.5 && mat_type < 1.5;
        let bend_rough = select(roughness, roughness * (1.0 - chrome_purity), is_chrome);
        r = aniso_reflect(n, v, af_t, af_b, aniso_amt, bend_rough);
    }
    let r_env = rotate_y(r, env_rot);

    var shaded: vec3<f32>;
    if (mat_type > 0.5 && mat_type < 1.5) {
        // ===== Chrome: polished env mirror (+ optional brushed anisotropy) =====
        let f0c_base = mix(vec3<f32>(0.85), albedo, metallic);
        let f0c = mix(f0c_base, vec3<f32>(1.0), chrome_purity);
        let refl_rough = roughness * (1.0 - chrome_purity);
        let brdf_c = textureSampleLevel(brdf_lut_tex, ibl_samp,
            vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0), max(refl_rough, 1e-3)), 0.0).rg;
        let refl = sample_prefiltered(r_env, refl_rough);
        let mirror = refl * (f0c * brdf_c.x + vec3<f32>(brdf_c.y))
            * env_intensity * ambient_mul * etint * irid_tint;
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
        shaded = mirror + direct + emissive + irid_sheen;
    } else if (mat_type >= 1.5 && mat_type < 3.5) {
        // ===== Glass / Refractive: Fresnel-blended env reflect + refract =====
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
                thru = sample_prefiltered(r_env, g_rough); // total internal reflection
            } else {
                thru = sample_prefiltered(rotate_y(refr_dir, env_rot), g_rough);
            }
        }
        let reflected = sample_prefiltered(r_env, g_rough);
        let f0s = (ior - 1.0) / (ior + 1.0);
        let fr = fresnel_schlick_roughness(n_dot_v, vec3<f32>(f0s * f0s), g_rough).x;
        // Refractive (type 3): Beer–Lambert absorption over the bound-sphere chord
        // along the refracted ray (thick centres go murky in the node colour).
        var absorb = vec3<f32>(1.0);
        if (mat_type >= 2.5 && u.refr.x > 0.0) {
            var tdir = refract(-v, n, 1.0 / ior);
            if (dot(tdir, tdir) < 1e-6) { tdir = reflect(-v, n); }
            let thick = bound_chord_frac(hp, normalize(tdir));
            let sigma = (vec3<f32>(1.0) - clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0))) * u.refr.x;
            absorb = exp(-sigma * thick);
        }
        let film = thin_film_tint(n_dot_v, thin_film);
        let tint = mix(mix(vec3<f32>(1.0), albedo, 0.5), vec3<f32>(1.0), glass_clarity);
        let env_mul = tint * env_intensity * ambient_mul * etint;
        let glass_body = thru * (1.0 - fr) * env_mul * absorb;
        let glass_surf = reflected * irid_tint * film * fr * env_mul;
        let dg_rough = max(g_rough, 0.02);
        let glass_f0 = vec3<f32>(f0s * f0s);
        let direct = direct_light(n, v, l_key, albedo, metallic, dg_rough, glass_f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, dg_rough, glass_f0, vec3<f32>(fill_w));
        // Opaque composite (the neural pass isn't alpha-blended): the refracted
        // env stands in for "what's behind", so the blob still reads as glass.
        shaded = glass_body + glass_surf + direct + emissive + sss + irid_sheen;
    } else {
        // ===== Standard metallic-roughness PBR (+ Anisotropic type 4 / overlay) =====
        let f0_dielectric = mix(vec3<f32>(0.04), vec3<f32>(0.9), f0_override);
        let f0 = mix(f0_dielectric, albedo, metallic);
        let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
        let ks = f;
        let kd = (vec3<f32>(1.0) - ks) * (1.0 - metallic);
        let irradiance = sample_irradiance(n_env);
        let diffuse = irradiance * albedo;
        let prefiltered = sample_prefiltered(r_env, roughness);
        let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
            vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0), max(roughness, 1e-3)), 0.0).rg;
        // Multiple-scattering energy compensation (Fdez-Agüera), as cube.wgsl.
        let fss_ess = f0 * brdf.x + vec3<f32>(brdf.y);
        let ems = 1.0 - (brdf.x + brdf.y);
        let favg = f0 + (vec3<f32>(1.0) - f0) * (1.0 / 21.0);
        let fms = fss_ess * favg / (vec3<f32>(1.0) - ems * favg);
        let specular = prefiltered * (fss_ess + fms * ems) * irid_tint;
        let ambient = (kd * diffuse + specular) * env_intensity * ambient_mul * etint;
        var direct: vec3<f32>;
        if (aniso_on) {
            direct = direct_light_aniso(n, v, l_key, af_t, af_b, albedo, metallic, a_t, a_b, f0, key_rad)
                   + direct_light_aniso(n, v, l_fill, af_t, af_b, albedo, metallic, a_t, a_b, f0, vec3<f32>(fill_w));
        } else {
            direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(fill_w));
        }
        shaded = ambient + direct + emissive + sss + irid_sheen;
    }

    tr.color = shaded;
    let clip = m.view_proj * vec4<f32>(hp, 1.0);
    tr.depth = clip.z / clip.w;
    tr.hit = true;
    return tr;
}

@fragment
fn fs_ray(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    let n = max(i32(m.p2.w + 0.5), 1);
    let res = max(m.p3.xy, vec2<f32>(1.0));
    let px = in.clip.xy;
    var acc = vec3<f32>(0.0);
    var depth = 1.0;
    var hits = 0;
    for (var i = 0; i < n; i = i + 1) {
        let sp = (px + aa_offset(i, n)) / res;
        let ndc = vec2<f32>(sp.x * 2.0 - 1.0, 1.0 - sp.y * 2.0);
        let tr = trace(ro, ray_dir(ndc));
        if (tr.hit) {
            acc = acc + tr.color;
            depth = min(depth, tr.depth);
            hits = hits + 1;
        }
    }
    if (hits == 0) {
        discard;
    }
    var out: FragOut;
    let cov = f32(hits) / f32(n);
    out.color = vec4<f32>(acc / f32(n), cov);
    out.depth = depth;
    return out;
}

// ===================== depth-only prepass (screen-space GI) =====================
// The SAME isosurface march as `trace`, but shading-free: it only finds the hit
// and writes `frag_depth` into the single-sample screen-space-FX prepass, so SSR
// (#80 A) and SSGI (#152 T2) can reconstruct the neural surface's normal from
// depth derivatives and gather off it. Miss → `discard` (the prepass keeps its
// cleared far depth there). One centre sample (no AA) — the effects tolerate the
// sub-pixel edge difference, and it keeps the extra march cheap.
struct DepthOut { @builtin(frag_depth) depth: f32 };

@fragment
fn fs_ray_depth(in: VsOut) -> DepthOut {
    let ro = u.camera_pos.xyz;
    let res = max(m.p3.xy, vec2<f32>(1.0));
    let sp = in.clip.xy / res;
    let ndc = vec2<f32>(sp.x * 2.0 - 1.0, 1.0 - sp.y * 2.0);
    let rd = ray_dir(ndc);

    let oc = ro - m.center.xyz;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - m.center.w * m.center.w;
    let disc = b * b - c;
    if (disc < 0.0) { discard; }
    let sq = sqrt(disc);
    let tmin = max(-b - sq, 0.0);
    let tmax = -b + sq;
    if (tmax <= tmin) { discard; }

    let iso = m.p1.z;
    let steps = max(i32(m.p2.x + 0.5), 8);
    let dt = (tmax - tmin) / f32(steps);
    var prev = neural_density(ro + rd * tmin) - iso;
    var th = -1.0;
    for (var i = 1; i <= steps; i = i + 1) {
        let tc = tmin + dt * f32(i);
        let cur = neural_density(ro + rd * tc) - iso;
        // Match trace()'s bidirectional crossing (#224 review) so the depth prepass
        // hits exactly where the colour pass does — SSR/SSGI read a consistent surface.
        if ((prev > 0.0) != (cur > 0.0)) {
            let frac = clamp(prev / (prev - cur), 0.0, 1.0);
            th = tc - dt + dt * frac;
            break;
        }
        prev = cur;
    }
    if (th < 0.0) { discard; }

    let hp = ro + rd * th;
    let clip = m.view_proj * vec4<f32>(hp, 1.0);
    var out: DepthOut;
    out.depth = clip.z / clip.w;
    return out;
}

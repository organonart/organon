// Organic Math — Lens generator (#258 Tier 3): an analytic double-convex /
// plano-convex lens body, raymarched per-pixel as a SIGNED DISTANCE FIELD.
//
// A sibling of the Mandelbulb / Minimal-surface raymarch paths: a fullscreen pass
// sphere-traces a ray per pixel against the lens SDF, takes the SDF gradient as the
// surface normal, and shades it with the SAME metallic-roughness IBL + key/fill PBR
// as cube.wgsl / minimal.wgsl (Standard / Chrome / Glass) — so the Glass/Refractive
// material makes it refract, and under the #258 Tier-2 dielectric tracer (a separate
// branch) the lens focuses. Ray misses `discard` so the skybox shows through; hits
// write `frag_depth` so the surface depth-composites with the skybox and feeds bloom.
//
// The lens body is the exact CSG of primitives (proper SDF, so a plain sphere-trace):
//   biconvex  = intersection of two mirrored spheres (radius R from `focal`)
//   plano-cvx = intersection of one sphere with the flat half-space z <= 0
//   aperture  = intersection with an axial cylinder of radius `aperture` (the stop)
// The optical axis is local +z (world point recentred by the lens centre).
//
// Outputs LINEAR HDR radiance (exposure/bloom/tonemap happen in the composite).

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
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// ----- group(1): IBL maps + filtering sampler (same layout as cube.wgsl) -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): Lens params -----
struct LensU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED); for frag_depth
    p0: vec4<f32>,          // focal, aperture_frac, thickness_frac, plano(0/1)
    p1: vec4<f32>,          // scale, steps, _, _
    p2: vec4<f32>,          // _, samples, res_x, res_y
    center: vec4<f32>,      // xyz = world centre, w = bound-sphere radius
};
@group(2) @binding(0) var<uniform> m: LensU;

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

// --- BRDF helpers (ported 1:1 from cube.wgsl / minimal.wgsl) ---
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
fn thin_film_tint(cos_theta: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return vec3<f32>(1.0);
    }
    let opd = (1.0 - clamp(cos_theta, 0.0, 1.0)) * 6.2831853 * (2.0 + amount * 6.0);
    let tint = 0.5 + 0.5 * cos(vec3<f32>(opd) + vec3<f32>(0.0, 2.0944, 4.1888));
    return mix(vec3<f32>(1.0), tint, clamp(amount, 0.0, 1.0));
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

// ===================== Lens signed distance field =====================

fn sd_sphere(p: vec3<f32>, r: f32) -> f32 {
    return length(p) - r;
}

// The lens SDF at a WORLD-space point (recentred to the lens centre internally).
// Optical axis = z. Distances are true world-space (Lipschitz-1) so a plain
// sphere-trace steps by the returned value.
fn lens_sdf(world_p: vec3<f32>) -> f32 {
    let pl = world_p - m.center.xyz;          // recentre; optical axis = z
    let scale = max(m.p1.x, 1e-3);
    let focal = max(m.p0.x, 0.05);
    let aper = clamp(m.p0.y, 0.02, 1.5) * scale;   // aperture (clear) radius, world
    let r = focal * scale;                         // sphere radius from focal/curvature
    var t = clamp(m.p0.z, 0.01, 0.98) * scale;     // centre half-thickness, world
    t = min(t, r * 0.98);                          // keep the cap convex (t <= R)
    let dz = r - t;                                // sphere-centre axial offset
    let plano = m.p0.w > 0.5;
    // Front cap: sphere centred at (0,0,+dz), bulging toward -z.
    let front = sd_sphere(pl - vec3<f32>(0.0, 0.0, dz), r);
    var body: f32;
    if (plano) {
        // Plano-convex: curved front ∩ flat back (half-space z <= 0).
        body = max(front, pl.z);
    } else {
        // Biconvex: intersection of two mirrored spherical caps.
        let back = sd_sphere(pl - vec3<f32>(0.0, 0.0, -dz), r);
        body = max(front, back);
    }
    // Aperture stop: clip to an axial cylinder of radius `aper` (no effect when the
    // stop is wider than the caps' rim).
    let cyl = length(pl.xy) - aper;
    return max(body, cyl);
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

// ===================== fragment (sphere-trace + shade) =====================
struct FragOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

struct Trace {
    color: vec3<f32>,
    depth: f32,
    hit: bool,
    alpha: f32,
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
    tr.alpha = 1.0;

    // Ray vs the lens's bounding sphere (centre, radius = center.w).
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

    let scale = max(m.p1.x, 1e-3);
    let steps = i32(m.p1.y);
    let eps = max(scale * 5e-4, 1e-3);
    var t = tmin;
    var hit = false;
    var hp = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * t;
        let d = lens_sdf(p);
        if (d < eps) {
            hp = p;
            hit = true;
            break;
        }
        t = t + max(d, eps * 0.5);
        if (t > tmax) { break; }
    }
    if (!hit) {
        return tr;
    }

    // World-space normal from the SDF gradient (central differences).
    let h = max(scale * 8e-4, 2e-3);
    let dx = vec3<f32>(h, 0.0, 0.0);
    let dy = vec3<f32>(0.0, h, 0.0);
    let dz = vec3<f32>(0.0, 0.0, h);
    var n = normalize(vec3<f32>(
        lens_sdf(hp + dx) - lens_sdf(hp - dx),
        lens_sdf(hp + dy) - lens_sdf(hp - dy),
        lens_sdf(hp + dz) - lens_sdf(hp - dz),
    ));
    if (dot(n, n) < 1e-12) {
        n = -rd;
    }
    if (dot(n, ro - hp) < 0.0) {
        n = -n;
    }

    // Clear-glass base albedo (a lens, not a coloured field) — let the material speak.
    let base_albedo = vec3<f32>(0.9);
    let rd_d = rd_dapple(hp, n);
    let albedo = mix(base_albedo, base_albedo * rd_d, clamp(rdu.params.z, 0.0, 1.0));

    // ---- Material-branched shading (Standard / Chrome / Glass), env-only — the same
    // metallic-roughness PBR + IBL + key/fill the cubes / minimal surfaces use. ----
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
    let r_reflect = reflect(-v, n);
    let n_env = rotate_y(n, env_rot);
    let r_env = rotate_y(r_reflect, env_rot);

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
        let film = thin_film_tint(n_dot_v, thin_film);
        let glass = mix(thru, reflected * irid_tint * film, fr)
                  * tint * env_intensity * ambient_mul * etint;
        let direct = direct_light(n, v, l_key, albedo, metallic, roughness, vec3<f32>(0.04), vec3<f32>(u.key_light.w))
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, vec3<f32>(0.04), vec3<f32>(u.fill_light.w));
        shaded = glass + direct + emissive + sss + irid_sheen;
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

    tr.color = shaded;
    tr.alpha = alpha;
    let clip = m.view_proj * vec4<f32>(hp, 1.0);
    tr.depth = clip.z / clip.w;
    tr.hit = true;
    return tr;
}

@fragment
fn fs_ray(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    let n = max(i32(m.p2.y + 0.5), 1);
    let res = max(m.p2.zw, vec2<f32>(1.0));
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
    out.color = vec4<f32>(acc / f32(n), acc_a / f32(n));
    out.depth = depth;
    return out;
}

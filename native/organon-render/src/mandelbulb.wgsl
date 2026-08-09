// Organic Math — Mandelbulb distance-estimated raymarch.
//
// A sibling of the Metaball raymarch path: a fullscreen pass marches a ray per
// pixel against the White–Nylander Mandelbulb (an analytic distance estimator,
// NOT a baked field — no compute prebake), takes the DE gradient as the surface
// normal, and shades it with the SAME metallic-roughness IBL + key/fill PBR as
// cube.wgsl / metaball.wgsl (Standard path) so the look matches the other modes.
// Colour comes from the orbit trap (the signature fractal banding). Ray misses
// `discard` so the skybox shows through; hits write `frag_depth` so the surface
// depth-composites with the skybox and feeds bloom.
//
// The DE is clean-room from the published formula (Daniel White / Paul Nylander),
// mirroring math.rs::mandelbulb_de — NOT the CC-BY-NC shadertoy source.
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
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// ----- group(1): IBL maps + filtering sampler (same layout as cube.wgsl) -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): Mandelbulb params -----
struct MandelU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED); for frag_depth
    p0: vec4<f32>,          // power, iterations, scale, steps
    p1: vec4<f32>,          // spin_angle, morph_angle, bailout, color_intensity
    p2: vec4<f32>,          // color_phase, _, _, _
    center: vec4<f32>,      // xyz = world centre, w = bound-sphere radius
};
@group(2) @binding(0) var<uniform> m: MandelU;

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

// --- BRDF helpers (ported 1:1 from cube.wgsl / metaball.wgsl) ---
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

// ===================== Mandelbulb distance estimator =====================

// Rotation applied to the sample point (a tumble off the spin angle), so the
// fractal turns under the camera/beat. Returns world→fractal-space direction.
fn rot_pt(p: vec3<f32>) -> vec3<f32> {
    let ay = m.p1.x;          // primary spin about Y
    let ax = m.p1.x * 0.37;   // gentle cross-tumble about X
    var q = rotate_y(p, ay);
    let c = cos(ax);
    let s = sin(ax);
    q = vec3<f32>(q.x, q.y * c - q.z * s, q.y * s + q.z * c);
    return q;
}

// White–Nylander DE in fractal/unit space. Returns vec2(distance, orbit_trap).
fn mandelbulb_de(pos: vec3<f32>) -> vec2<f32> {
    let power = m.p0.x;
    let iters = i32(m.p0.y);
    let bailout = m.p1.z;
    let phase = m.p1.y;
    var z = pos;
    var dr = 1.0;
    var r = 0.0;
    var trap = bailout;
    for (var i = 0; i < iters; i = i + 1) {
        r = length(z);
        if (r > bailout) { break; }
        let theta = acos(clamp(z.z / max(r, 1e-9), -1.0, 1.0));
        let phi = atan2(z.y, z.x) + phase;
        dr = pow(r, power - 1.0) * power * dr + 1.0;
        let zr = pow(r, power);
        let th = theta * power;
        let ph = phi * power;
        z = zr * vec3<f32>(sin(th) * cos(ph), sin(th) * sin(ph), cos(th)) + pos;
        trap = min(trap, r);
    }
    let dist = 0.5 * log(max(r, 1e-9)) * r / max(dr, 1e-9);
    return vec2<f32>(dist, trap);
}

// World-space map: transform into the unit fractal, evaluate, scale the distance
// back to world units. Returns vec2(world_distance, orbit_trap).
fn map_world(world_p: vec3<f32>) -> vec2<f32> {
    let scale = m.p0.z;
    let pu = rot_pt((world_p - m.center.xyz) / scale);
    let de = mandelbulb_de(pu);
    return vec2<f32>(de.x * scale, de.y);
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
};

// Reconstruct a world-space ray direction for a clip-space NDC point (used for
// per-sub-sample supersampling rays — the unscaled inv-VP keeps the fractal put).
fn ray_dir(ndc: vec2<f32>) -> vec3<f32> {
    let near = m.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = m.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    return normalize((far.xyz / far.w) - (near.xyz / near.w));
}

// Sub-pixel offset (in pixel units) for sample `i` of `n` — a fullscreen raymarch
// gets no MSAA (full coverage → one fragment), so we supersample here. 2× = a
// diagonal pair; 4× = a rotated grid (RGSS) for clean near-horizontal edges.
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

    // Ray vs the fractal's bounding sphere (centre, radius = center.w).
    let oc = ro - m.center.xyz;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - m.center.w * m.center.w;
    let disc = b * b - c;
    if (disc < 0.0) {
        return tr;
    }
    let sq = sqrt(disc);
    var tmin = max(-b - sq, 0.0);
    let tmax = -b + sq;
    if (tmax <= tmin) {
        return tr;
    }

    let steps = i32(m.p0.w);
    let scale = m.p0.z;
    var t = tmin;
    var hit = false;
    var hp = vec3<f32>(0.0);
    var trap = 0.0;
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * t;
        let res = map_world(p);
        let eps = max(0.00035 * t, 0.0002 * scale);
        if (res.x < eps) {
            hp = p;
            trap = res.y;
            hit = true;
            break;
        }
        t = t + max(res.x, 0.0003 * scale);
        if (t > tmax) {
            break;
        }
    }
    if (!hit) {
        return tr;
    }

    // Surface normal from the DE gradient (central differences, world space).
    let h = 0.0006 * scale;
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

    // Orbit-trap colour (the signature Mandelbulb banding). color_intensity 0 →
    // near-white shading; 1 → saturated bands. color_phase cycles the gradient.
    let color_intensity = clamp(m.p1.w, 0.0, 1.0);
    let tt = pow(clamp(trap, 0.0, 1.0), 0.55);
    let band = 0.5 + 0.5 * sin(
        vec3<f32>(0.0, 0.5, 1.0) + (tt + m.p2.x) * 6.2831853 + tt * 4.2 + 3.0);
    let base_albedo = mix(vec3<f32>(0.85), band, color_intensity);
    let rd_d = rd_dapple(hp, n);
    let albedo = mix(base_albedo, base_albedo * rd_d, clamp(rdu.params.z, 0.0, 1.0));

    // ---- Standard metallic-roughness PBR (IBL + key/fill), same as cube.wgsl ----
    let metallic = clamp(u.mat.x, 0.0, 1.0);
    let roughness = clamp(u.mat.y, 0.0, 1.0);
    let glow = u.mat.z;
    let env_intensity = u.env.y;
    let env_rot = u.env.z;
    let ambient_mul = u.amb.x;
    let etint = u.env_tint.rgb;

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

    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let ks = f;
    let kd = (vec3<f32>(1.0) - ks) * (1.0 - metallic);

    let irradiance = sample_irradiance(n_env);
    let diffuse = irradiance * albedo;
    let prefiltered = sample_prefiltered(r_env, roughness);
    let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                  vec2<f32>(n_dot_v, max(roughness, 1e-3)), 0.0).rg;
    let specular = prefiltered * (f0 * brdf.x + vec3<f32>(brdf.y)) * irid_tint;
    let ambient = (kd * diffuse + specular) * env_intensity * ambient_mul * etint;

    let direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, vec3<f32>(u.key_light.w))
               + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(u.fill_light.w));

    tr.color = ambient + direct + emissive + sss + irid_sheen;
    // `hp` is in unscaled world space (rays from m.inv_vp), so project through the
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
    // target size (px). If every sub-sample misses, discard so the skybox shows.
    let n = max(i32(m.p2.y + 0.5), 1);
    let res = max(m.p2.zw, vec2<f32>(1.0));
    let px = in.clip.xy; // framebuffer pixel coordinate
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
    // Coverage-based premultiplied-alpha output: missed sub-samples contribute the
    // background (already drawn into this HDR attachment), so divide the summed hit
    // colour by the TOTAL sample count and carry coverage as alpha. The pipeline
    // blends `src·1 + dst·(1−α)`, so a partly-covered silhouette pixel averages to
    // [Σ hit colour + background·(n−hits)] / n — true edge anti-aliasing. A fully
    // covered interior pixel (α = 1) reduces to the old opaque overwrite.
    let cov = f32(hits) / f32(n);
    out.color = vec4<f32>(acc / f32(n), cov);
    out.depth = depth;
    return out;
}

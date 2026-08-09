// Organic Math — metaball isosurface raymarch.
//
// A new surface mode that wraps the whole node set in ONE smooth contiguous skin
// instead of drawing geometry per node. The node field is baked into a 3D texture
// each frame by field.wgsl; here a fullscreen pass marches a ray per pixel
// through that field, stops at the iso-threshold crossing, takes the field
// gradient as the surface normal, and shades it with the SAME metallic-roughness
// IBL + key/fill PBR as cube.wgsl (Standard path) so the look matches the other
// modes. The blended colour field provides the albedo (RGB-cube by position in
// Native mode, or the palette / HSV sweep when one is active). Ray misses
// `discard` so the skybox shows through; hits write `frag_depth` so the surface
// depth-composites with the skybox and feeds bloom.
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
    glassx: vec4<f32>,      // spectral glass (#80 C): x=dispersion, y=caustic, z=thin_film
    reflect_ctl: vec4<f32>, // #163 T1: x=reflect_tint, y=chrome_purity, z=glass_clarity, w=f0_override
    // The shared cube uniform buffer continues past here (refl_box/refr/aniso/coat/
    // sheen/body/micro/micro2/emit/thinfilm/demo_light*/matcol/skyrefl = 15 vec4s);
    // this shader only needs `cal` at the tail, so skip the rest with padding (#349).
    _calpad: array<vec4<f32>, 15>,
    cal: vec4<f32>,         // Calibrated colour (#349 T1): x=mode (0 Aesthetic → identity,
                            // 1 Calibrated), y=lut, z=amount, w=cal_t (db_to_colour_t coord).
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// Calibrated-colour LUT (#349 Tier 1) — MUST mirror `math::calibrated_colour` +
// `cube.wgsl` EXACTLY (same coefficients, same Horner order) so every surface type
// tints identically. Applied to the volume/metaball colour so the Field Volume cloud
// inherits the same colour-means-a-level law as the instanced cubes.
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
// mode < 0.5 (Aesthetic) or amount 0 → returns `col` unchanged (byte-identical).
fn apply_calibrated(col: vec3<f32>) -> vec3<f32> {
    if (u.cal.x < 0.5) {
        return col;
    }
    let cc = calibrated_colour(u.cal.w, u32(u.cal.y + 0.5));
    return mix(col, cc, clamp(u.cal.z, 0.0, 1.0));
}

// ----- group(1): IBL maps + filtering sampler (same layout as cube.wgsl) -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): the baked field + raymarch params -----
struct MetaU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED — no breath); for frag_depth
    bmin: vec4<f32>,        // xyz = field AABB min, w = iso threshold
    bmax: vec4<f32>,        // xyz = field AABB max, w = max ray steps
    grid: vec4<f32>,        // xyz = field resolution, w unused
    vol: vec4<f32>,         // emissive-volume (#152): x=density, y=emission, z=absorption, w unused
};
@group(2) @binding(0) var<uniform> m: MetaU;
@group(2) @binding(1) var field_tex  : texture_3d<f32>;
@group(2) @binding(2) var field_samp : sampler;

// ----- group(3): reaction–diffusion (Turing) surface field -----
@group(3) @binding(0) var rd_tex  : texture_2d<f32>;
@group(3) @binding(1) var rd_samp : sampler;
struct RdLook { params: vec4<f32> }; // x=intensity, y=scale, z=albedo_mix
@group(3) @binding(2) var<uniform> rdu: RdLook;

// ---- group(4): the probe-GI volume (#182 T4 — the same GiUniform the cube
// shader reads; bindings 1-3 of the shared group are unused here). The liquid
// and metaball surfaces finally respond to the GI toggle: the flat L0 term is
// trilinearly sampled into the ambient diffuse.
struct GiUniform {
    bbox_min: vec4<f32>,
    bbox_max: vec4<f32>,
    info: vec4<f32>, // x=enabled, y=intensity, z=grid_dim, w=falloff
    probes: array<vec4<f32>, 648>,
};
@group(4) @binding(0) var<uniform> gi: GiUniform;

// ---- group(5): the VXGI radiance volume (#182 T4) — the same texture the
// screen-space gather marches, sampled directly so the liquid/metaball
// surface responds to the Voxel GI toggle. bmin.w = gain (0 = off/stale).
struct VxSampleU {
    bmin: vec4<f32>,
    bmax: vec4<f32>,
};
@group(5) @binding(0) var<uniform> vxs: VxSampleU;
@group(5) @binding(1) var vxgi_tex: texture_3d<f32>;
@group(5) @binding(2) var vxgi_samp: sampler;

fn vxgi_bounce(p: vec3<f32>) -> vec3<f32> {
    if (vxs.bmin.w <= 0.0) {
        return vec3<f32>(0.0);
    }
    let ext = max(vxs.bmax.xyz - vxs.bmin.xyz, vec3<f32>(1e-5));
    let uvw = (p - vxs.bmin.xyz) / ext;
    // Soft volume edge: fade over the outer 8% instead of a hard cubic cut
    // (negative outside → 0, so this also rejects out-of-volume points).
    let m3 = min(uvw, vec3<f32>(1.0) - uvw);
    let edge = smoothstep(0.0, 0.08, min(m3.x, min(m3.y, m3.z)));
    if (edge <= 0.0) {
        return vec3<f32>(0.0);
    }
    return textureSampleLevel(vxgi_tex, vxgi_samp, uvw, 0.0).rgb * (vxs.bmin.w * edge);
}

// ---- group(4) binding(1): emissive cubes as real lights (#167 T3 / #182 T4
// ghost light) — the same LightsU the cube shader loops, so a (possibly
// hidden) generator's brightest nodes throw real point lights onto the
// liquid/metaball surface. Ported from cube.wgsl::many_lights.
struct MLight { pos: vec4<f32>, color: vec4<f32> };
struct LightsU {
    header: vec4<f32>, // x=count, y=intensity
    lights: array<MLight, 64>,
};
@group(4) @binding(1) var<uniform> mlights: LightsU;

fn many_lights(world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, albedo: vec3<f32>,
               metallic: f32, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let count = min(i32(mlights.header.x), 64);
    if (count <= 0) {
        return vec3<f32>(0.0);
    }
    let intensity = mlights.header.y;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < count; i = i + 1) {
        let lp = mlights.lights[i].pos.xyz;
        let radius = max(mlights.lights[i].pos.w, 1e-3);
        let to_l = lp - world;
        let dist = length(to_l);
        if (dist < radius * 0.08) {
            continue;
        }
        let l = to_l / max(dist, 1e-4);
        // Windowed falloff (see cube.wgsl): smoothly 0 at 4×radius.
        let window = pow(clamp(1.0 - pow(dist / (radius * 4.0), 4.0), 0.0, 1.0), 2.0);
        let atten = window / (1.0 + (dist * dist) / (radius * radius));
        if (atten < 1e-4) {
            continue;
        }
        let radiance = mlights.lights[i].color.rgb * intensity * atten;
        acc = acc + direct_light(n, v, l, albedo, metallic, roughness, f0, radiance);
    }
    return acc;
}

// ---- Chrome/Glass helpers (ported from cube.wgsl — keep in lockstep) ----
// Thin-film interference tint (amount 0 → white).
fn thin_film_tint(cos_theta: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return vec3<f32>(1.0);
    }
    let opd = (1.0 - clamp(cos_theta, 0.0, 1.0)) * 6.2831853 * (2.0 + amount * 6.0);
    let tint = 0.5 + 0.5 * cos(vec3<f32>(opd) + vec3<f32>(0.0, 2.0944, 4.1888));
    return mix(vec3<f32>(1.0), tint, clamp(amount, 0.0, 1.0));
}

// Refract one wavelength's channel through the environment at a given IOR;
// total internal reflection falls back to the reflection.
fn sample_refract_chan(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32) -> vec3<f32> {
    let refr_dir = refract(-v, n, 1.0 / max(ior, 1.0));
    if (dot(refr_dir, refr_dir) < 1e-6) {
        return sample_prefiltered(rotate_y(reflect(-v, n), env_rot), roughness);
    }
    return sample_prefiltered(rotate_y(refr_dir, env_rot), roughness);
}

// Spectral glass (#80 C): 3-tap RGB refraction at offset IORs + a focus boost.
fn glass_dispersion(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32,
                    dispersion: f32, caustic: f32) -> vec3<f32> {
    let spread = dispersion * 0.06;
    let cr = sample_refract_chan(v, n, ior * (1.0 - spread), roughness, env_rot);
    let cg = sample_refract_chan(v, n, ior, roughness, env_rot);
    let cb = sample_refract_chan(v, n, ior * (1.0 + spread), roughness, env_rot);
    var col = vec3<f32>(cr.r, cg.g, cb.b);
    if (caustic > 0.0) {
        let lum = max(max(col.r, col.g), col.b);
        col += col * smoothstep(0.6, 2.0, lum) * caustic * 2.0;
    }
    return col;
}

// Flat (L0) probe irradiance at a world position, trilinear over the 6³ grid.
// Slot layout matches math::compute_gi_probes: ((z·dim + y)·dim + x)·3 + ch.
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

// Triplanar sample of the RD field's V channel (world-space) → 0..1 dapple.
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

// --- BRDF helpers (ported 1:1 from cube.wgsl) ---
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

// --- field sampling (uvw normalised into the AABB) ---
fn field_uvw(p: vec3<f32>) -> vec3<f32> {
    return (p - m.bmin.xyz) / (m.bmax.xyz - m.bmin.xyz);
}
fn sample_density(p: vec3<f32>) -> f32 {
    return textureSampleLevel(field_tex, field_samp, field_uvw(p), 0.0).a;
}
fn sample_color(p: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(field_tex, field_samp, field_uvw(p), 0.0).rgb;
}

// Bioluminescent ripple (same as cube.wgsl): a travelling band of HDR emission
// through world space, brightening the local albedo. Inert at intensity 0.
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

// ===================== vertex (fullscreen triangle + ray) =====================
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) rd: vec3<f32>,
};

@vertex
fn vs_ray(@builtin(vertex_index) vi: u32) -> VsOut {
    // 0,1,2 -> a triangle that covers the screen: uv (0,0),(2,0),(0,2).
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    let ndc = uv * 2.0 - vec2<f32>(1.0);
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    // Unproject near + far to build the world-space ray direction.
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

@fragment
fn fs_ray(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    let rd = normalize(in.rd);

    // Ray vs field AABB (slab test).
    let bmin = m.bmin.xyz;
    let bmax = m.bmax.xyz;
    let inv = 1.0 / rd;
    let ta = (bmin - ro) * inv;
    let tb = (bmax - ro) * inv;
    let tsmall = min(ta, tb);
    let tbig = max(ta, tb);
    var tmin = max(max(tsmall.x, tsmall.y), max(tsmall.z, 0.0));
    let tmax = min(min(tbig.x, tbig.y), tbig.z);
    if (tmax <= tmin) {
        discard;
    }

    let thresh = m.bmin.w;
    let steps = i32(m.bmax.w);
    let dt = (tmax - tmin) / f32(steps);

    var t = tmin;
    var prev = 0.0;
    var hit = false;
    var hp = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * t;
        let d = sample_density(p);
        if (d >= thresh) {
            // Linear refine between the last empty sample and this solid one.
            let frac = clamp((thresh - prev) / max(d - prev, 1e-5), 0.0, 1.0);
            hp = ro + rd * (t - dt + frac * dt);
            hit = true;
            break;
        }
        prev = d;
        t = t + dt;
    }
    if (!hit) {
        discard;
    }

    // Surface normal from the field gradient (central differences, one voxel
    // step per axis). Density decreases outward, so the outward normal = −∇.
    let e = (bmax - bmin) / max(m.grid.xyz, vec3<f32>(1.0));
    let gx = sample_density(hp + vec3<f32>(e.x, 0.0, 0.0)) - sample_density(hp - vec3<f32>(e.x, 0.0, 0.0));
    let gy = sample_density(hp + vec3<f32>(0.0, e.y, 0.0)) - sample_density(hp - vec3<f32>(0.0, e.y, 0.0));
    let gz = sample_density(hp + vec3<f32>(0.0, 0.0, e.z)) - sample_density(hp - vec3<f32>(0.0, 0.0, e.z));
    var n = -vec3<f32>(gx, gy, gz);
    if (dot(n, n) < 1e-12) {
        n = -rd;
    }
    n = normalize(n);

    let base_albedo = apply_calibrated(sample_color(hp));
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
    // #247 Tier 3 (energy → liquid): the liquid's field rgb carries an additive HDR
    // ember glow from the Maxwell energization (values > 1 only where energy was
    // injected). Emit the HDR excess so it glows independently of the material glow
    // dial. Normal albedo (RGB-cube / liquid colour) is <= 1 → zero excess →
    // byte-identical for every existing metaball/liquid look.
    let energy_glow = max(base_albedo - vec3<f32>(1.0), vec3<f32>(0.0));
    let emissive = albedo * glow + ripple_emission(hp, albedo)
        + base_albedo * (rd_d * rdu.params.x) + energy_glow;

    // Additive surface modifiers (inert at amount 0), matching cube.wgsl.
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

    // Material branches, ported from cube.wgsl (env-only, like the cubes'
    // Chrome/Glass — no scene-behind refraction). The full dial set now works
    // on the metaball surface + the liquid: material type, chrome purity,
    // glass clarity, f0 override, IOR, dispersion/caustic/thin-film.
    let mat_type = u.amb.y;
    let ior = max(u.amb.z, 1.0);
    let chrome_purity = clamp(u.reflect_ctl.y, 0.0, 1.0);
    let glass_clarity = clamp(u.reflect_ctl.z, 0.0, 1.0);
    let f0_override = clamp(u.reflect_ctl.w, 0.0, 1.0);

    // #163 f0_override: lift the dielectric reflectance toward a neutral
    // mirror (cube.wgsl's Standard semantics). 0 → classic 0.04.
    let f0_dielectric = mix(vec3<f32>(0.04), vec3<f32>(0.9), f0_override);
    let f0 = mix(f0_dielectric, albedo, metallic);
    let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let ks = f;
    let kd = (vec3<f32>(1.0) - ks) * (1.0 - metallic);

    // #182 T4: probe-GI + VXGI bounce (both GI toggles), and the emissive-cube
    // point lights (#167 — so a ghost-lit generator throws real light here).
    let bounce = gi_l0(hp) + vxgi_bounce(hp);
    let ml_term = many_lights(hp, n, v, albedo, metallic, roughness, f0);

    var color: vec3<f32>;
    if (mat_type > 0.5 && mat_type < 1.5) {
        // ===== Chrome: a polished mirror reflecting the environment =====
        let f0c_base = mix(vec3<f32>(0.85), albedo, metallic);
        let f0c = mix(f0c_base, vec3<f32>(1.0), chrome_purity);
        let refl_rough = roughness * (1.0 - chrome_purity);
        let brdf_c = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                        vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0),
                                                  max(refl_rough, 1e-3)), 0.0).rg;
        let refl = sample_prefiltered(r_env, refl_rough);
        let mirror = refl * (f0c * brdf_c.x + vec3<f32>(brdf_c.y))
            * env_intensity * ambient_mul * etint * irid_tint;
        let rough_g = max(refl_rough, 0.02);
        let direct_c = direct_light(n, v, l_key, albedo, 1.0, rough_g, f0c, vec3<f32>(u.key_light.w))
                     + direct_light(n, v, l_fill, albedo, 1.0, rough_g, f0c, vec3<f32>(u.fill_light.w));
        color = mirror + direct_c + emissive + irid_sheen + bounce * albedo + ml_term;
    } else if (mat_type >= 1.5) {
        // ===== Glass: Fresnel-blended env reflection + refraction =====
        // (Refractive (3) falls back to Glass here — the raymarched isosurface
        // has no per-instance body to measure a Beer–Lambert chord through.)
        let dispersion = u.glassx.x;
        let g_caustic = u.glassx.y;
        let thin_film = u.glassx.z;
        let g_rough = roughness * (1.0 - glass_clarity);
        var thru: vec3<f32>;
        if (dispersion > 0.0 || g_caustic > 0.0) {
            thru = glass_dispersion(v, n, ior, g_rough, env_rot, dispersion, g_caustic);
        } else {
            thru = sample_refract_chan(v, n, ior, g_rough, env_rot);
        }
        let reflected = sample_prefiltered(r_env, g_rough);
        // F0 from the live IOR (cube.wgsl semantics).
        let f0s = (ior - 1.0) / (ior + 1.0);
        let fr = fresnel_schlick_roughness(n_dot_v, vec3<f32>(f0s * f0s), g_rough).x;
        // Body tint → colourless as clarity rises.
        let tint = mix(mix(vec3<f32>(1.0), albedo, 0.5), vec3<f32>(1.0), glass_clarity);
        let film = thin_film_tint(n_dot_v, thin_film);
        let env_mul = tint * env_intensity * ambient_mul * etint;
        let glass_body = thru * (1.0 - fr) * env_mul;
        let glass_surf = reflected * irid_tint * film * fr * env_mul;
        let dg_rough = max(g_rough, 0.02);
        let glass_f0 = vec3<f32>(f0s * f0s);
        let direct_g = direct_light(n, v, l_key, albedo, metallic, dg_rough, glass_f0, vec3<f32>(u.key_light.w))
                     + direct_light(n, v, l_fill, albedo, metallic, dg_rough, glass_f0, vec3<f32>(u.fill_light.w));
        color = glass_body + glass_surf + direct_g + emissive + sss + irid_sheen
            + bounce * albedo * 0.5 + ml_term;
    } else {
        // ===== Standard metallic-roughness PBR (the original path) =====
        let irradiance = sample_irradiance(n_env);
        let diffuse = irradiance * albedo;
        let prefiltered = sample_prefiltered(r_env, roughness);
        let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                      vec2<f32>(n_dot_v, max(roughness, 1e-3)), 0.0).rg;
        let specular = prefiltered * (f0 * brdf.x + vec3<f32>(brdf.y)) * irid_tint;
        let ambient = (kd * diffuse + specular) * env_intensity * ambient_mul * etint;
        let direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, vec3<f32>(u.key_light.w))
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(u.fill_light.w));
        let gi_bounce = kd * bounce * albedo;
        color = ambient + gi_bounce + direct + emissive + sss + irid_sheen + ml_term;
    }

    var out: FragOut;
    out.color = vec4<f32>(color, 1.0);
    // Write the hit's depth so it occludes the skybox and feeds the composite.
    // `hp` is reconstructed in unscaled world space (from `m.inv_vp`), so project it
    // through the matching UNSCALED `m.view_proj` — NOT the scene's breath-scaled
    // `u.view_proj`, which would desync the metaball surface's depth when breathing.
    let clip = m.view_proj * vec4<f32>(hp, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}

// ===================== fragment (emissive volume — #152) =====================
// Surface mode "Volume": instead of finding an isosurface, integrate the SAME
// baked field as a glowing participating medium — emission · density along the
// ray with Beer–Lambert extinction. Front-to-back accumulation; the colour field
// (RGB-cube / palette / HSV sweep) tints the glow. Reuses the metaball bake, so
// every node-field generator can render as a nebula. Alpha-blended over the sky
// (the pipeline uses an alpha-over blend + Always/no-write depth), so no discard
// is needed; near-empty rays just contribute ~0.
@fragment
fn fs_volume(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    let rd = normalize(in.rd);

    let bmin = m.bmin.xyz;
    let bmax = m.bmax.xyz;
    let inv = 1.0 / rd;
    let ta = (bmin - ro) * inv;
    let tb = (bmax - ro) * inv;
    let tsmall = min(ta, tb);
    let tbig = max(ta, tb);
    let tmin = max(max(tsmall.x, tsmall.y), max(tsmall.z, 0.0));
    let tmax = min(min(tbig.x, tbig.y), tbig.z);

    var out: FragOut;
    if (tmax <= tmin) {
        out.color = vec4<f32>(0.0);
        out.depth = 1.0;
        return out;
    }

    let density_scale = max(m.vol.x, 0.0);
    let emission = max(m.vol.y, 0.0);
    let absorption = max(m.vol.z, 0.0);
    let steps = i32(m.bmax.w);
    let dt = (tmax - tmin) / f32(steps);

    var t = tmin;
    var trans = 1.0;             // transmittance along the ray
    var accum = vec3<f32>(0.0);  // accumulated in-scattered/emitted radiance
    var first_t = -1.0;          // depth of the first non-empty sample
    for (var i = 0; i < steps; i = i + 1) {
        let p = ro + rd * t;
        let d = sample_density(p) * density_scale;
        if (d > 1e-3) {
            if (first_t < 0.0) {
                first_t = t;
            }
            // Spectral shells (#348/#349 T3): when the bake wrote per-band colours
            // (m.vol.w = band_coloured), show them directly — they already ARE the
            // calibrated per-frequency colour. Else apply the single calibrated tint.
            let raw_c = sample_color(p);
            let c = select(apply_calibrated(raw_c), raw_c, m.vol.w > 0.5);
            // Emissive glow (HDR), attenuated by the transmittance so far.
            accum = accum + trans * c * d * emission * dt;
            // Beer–Lambert extinction for this segment.
            trans = trans * exp(-d * absorption * dt);
            if (trans < 0.01) {
                break;
            }
        }
        t = t + dt;
    }

    let alpha = clamp(1.0 - trans, 0.0, 1.0);
    out.color = vec4<f32>(accum, alpha);
    // Depth at the first occupied sample so it composites sensibly with the
    // skybox (ignored by the Always/no-write volume pipeline, but kept correct).
    let hp = ro + rd * select(tmin, first_t, first_t >= 0.0);
    let clip = m.view_proj * vec4<f32>(hp, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}

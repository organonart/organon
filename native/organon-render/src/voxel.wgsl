// Organic Math — crisp voxel raymarch (the "Teardown / MagicaVoxel" surface mode),
// now physically shaded (#voxel-pbs).
//
// The Eulerian render path. The node field is splatted into a 3D occupancy+colour
// grid each frame (voxelize.wgsl); here a fullscreen pass marches a ray per pixel
// through that grid by Amanatides–Woo DDA and stops at the first cell whose density
// crosses the fill threshold. The hit is an axis-aligned flat FACE (crisp
// grid-snapped cube), but it now shades with the SAME metallic-roughness PBR + IBL
// + Material card (Standard / Chrome / Glass / Refractive / Anisotropic) as
// cube.wgsl / neural.wgsl — so changing the material type, metallic, roughness,
// env intensity/tint, etc. all take effect on voxels. The voxel-specific charm is
// preserved and folded IN as occlusion/shadow on top of the PBR response:
//
//   • flat FACE normal — the axis-aligned face the ray entered through (the
//     deliberate opposite of the smooth PBR cubes), fed straight into the BRDF so
//     the faces read as crisp faceted metal / glass;
//   • voxel ambient occlusion — the classic per-corner neighbour-occupancy AO,
//     bilerped across the face, multiplied into the INDIRECT (IBL + bounce) term
//     for the soft crevice darkening that makes the blocks read as a solid world;
//   • one soft shadow ray DDA-marched toward the key light — threaded into the key
//     light's radiance so the analytic key contribution is occluded;
//   • palette posterization — optional colour quantization for the blocky-colour
//     charm (the per-node tint already carries the active palette / HSV sweep);
//   • voxel cone-traced GI (#89) — bounced colour from the field's mip pyramid,
//     added as extra indirect (off by default → the pre-PBS look is unaffected);
//   • emissive glow — hot voxels exceed 1.0 and bloom in the shared HDR composite.
//
// Screen-space GI (#voxel-pbs Level 2): a depth-only variant (`fs_ray_depth`)
// marches the SAME grid and writes just `frag_depth` into the single-sample
// screen-space-FX prepass, so SSR (#80 A) and SSGI (#152 T2) reconstruct the voxel
// surface's normal from depth and gather off it, composited in post.
//
// Renders into the linear `Rgba16Float` scene HDR buffer + depth, so bloom, tone-
// mapping, EDR, and MSAA all inherit. Ray misses `discard` so the sky shows
// through; hits write `frag_depth` so voxels composite with the background.

// ----- group(0): the cube shader's uniform block (same buffer, reused) -----
// Declared through the material tail (a byte prefix of the shared buffer, exactly
// as cube.wgsl / neural.wgsl declare it) so the voxel pass can honour the whole
// Material card, not just the camera + key/fill lights.
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
    ripple: vec4<f32>,
    ripple_ctr: vec4<f32>,
    ripple_mode: vec4<f32>,
    // Material tail — the SAME fields cube.wgsl declares.
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

// ----- group(2): the splatted voxel grid + raymarch params -----
struct VoxU {
    inv_vp: mat4x4<f32>,    // inverse view-projection, to unproject screen rays
    view_proj: mat4x4<f32>, // forward matching inv_vp (UNSCALED — no breath); for frag_depth
    bmin: vec4<f32>,        // xyz = grid AABB min, w = fill threshold
    bmax: vec4<f32>,        // xyz = grid AABB max, w = max ray steps
    grid: vec4<f32>,        // xyz = resolution, w unused
    shade: vec4<f32>,       // x = AO strength, y = shadow strength, z = emission gain, w = quantize levels
    gi: vec4<f32>,          // x = GI enabled, y = strength, z = max distance (world), w = sky/ambient amount
};
@group(2) @binding(0) var<uniform> vox: VoxU;
@group(2) @binding(1) var field_tex: texture_3d<f32>;
@group(2) @binding(2) var field_samp: sampler;

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.15915494, 0.31830989);

// ===================== BRDF / IBL helpers (ported 1:1 from cube.wgsl) =====================
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

// --- Anisotropy (ported 1:1 from cube.wgsl; the brush comes from a world-space
//     reference here — the flat voxel face has no per-vertex instance axis) ---
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

// ===================== grid sampling =====================

fn grid_res() -> vec3<i32> {
    return vec3<i32>(vox.grid.xyz);
}

fn in_bounds(c: vec3<i32>) -> bool {
    let res = grid_res();
    return all(c >= vec3<i32>(0)) && all(c < res);
}

fn cell_color(c: vec3<i32>) -> vec3<f32> {
    let res = grid_res();
    let cc = clamp(c, vec3<i32>(0), res - vec3<i32>(1));
    return textureLoad(field_tex, cc, 0).rgb;
}

// Is this cell solid? Additive: density ≥ threshold.
fn solid(c: vec3<i32>) -> bool {
    if (!in_bounds(c)) {
        return false;
    }
    let d = textureLoad(field_tex, c, 0).a;
    return d >= vox.bmin.w;
}

// Per-corner occupancy → ambient occlusion (the 0fps / Minecraft rule): a vertex
// touched by both edge neighbours is fully occluded; otherwise it dims by the
// number of occupied neighbours (two edges + the diagonal corner).
fn vertex_ao(side1: bool, side2: bool, corner: bool) -> f32 {
    if (side1 && side2) {
        return 0.0;
    }
    let occ = f32(i32(side1) + i32(side2) + i32(corner));
    return (3.0 - occ) / 3.0;
}

// ---- voxel cone tracing (#89): bounced colour from the field's mip pyramid ----
// March a cone from the surface; at each step sample the field at a mip level
// matching the cone's widening footprint, accumulate emitted colour front-to-back
// and integrate occlusion. emitted = albedo·(1+emission) so emissive voxels bleed
// brightest, but even unlit albedo bounces its colour (diffuse colour bleed). The
// mip pyramid is built each frame by voxgi.wgsl.
fn cone_trace(origin: vec3<f32>, dir: vec3<f32>, aperture: f32) -> vec4<f32> {
    let bmin = vox.bmin.xyz;
    let extent = max(vox.bmax.xyz - vox.bmin.xyz, vec3<f32>(1e-4));
    let res = vox.grid.xyz;
    let voxw = max(extent.x / max(res.x, 1.0), 1e-4); // ~world size of a voxel
    let emit_gain = 1.0 + max(vox.shade.z, 0.0);
    let max_dist = max(vox.gi.z, voxw);

    var color = vec3<f32>(0.0);
    var occ = 0.0;
    var dist = voxw * 2.0; // start beyond the originating voxel (no self-occlusion)
    for (var i = 0; i < 24; i = i + 1) {
        if (occ >= 0.98 || dist >= max_dist) {
            break;
        }
        let diameter = max(voxw, 2.0 * aperture * dist);
        let lod = clamp(log2(diameter / voxw), 0.0, 8.0);
        let p = origin + dir * dist;
        let uvw = (p - bmin) / extent;
        if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) {
            break;
        }
        let s = textureSampleLevel(field_tex, field_samp, uvw, lod);
        let a = clamp(s.a / max(vox.bmin.w, 1e-3), 0.0, 1.0); // density → opacity (vs threshold)
        let emit = s.rgb * emit_gain;
        color = color + (1.0 - occ) * a * emit;
        occ = occ + (1.0 - occ) * a;
        dist = dist + diameter * 0.5;
    }
    return vec4<f32>(color, occ);
}

// Gather diffuse GI over a fixed 6-cone hemisphere about the normal: one axial
// cone + five at ~60° around it. Returns the incoming bounced radiance (sky fills
// the unoccluded remainder), scaled by strength. Inert (0) when GI is disabled.
fn voxel_gi(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if (vox.gi.x < 0.5) {
        return vec3<f32>(0.0);
    }
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.95) {
        up = vec3<f32>(1.0, 0.0, 0.0);
    }
    let t = normalize(cross(up, n));
    let b = cross(n, t);

    let aperture = 0.55; // ~60° cones
    let side = 0.5;      // side-cone tilt off the normal
    var acc = cone_trace(p, n, aperture);
    for (var k = 0; k < 5; k = k + 1) {
        let ang = 6.2831853 * f32(k) / 5.0;
        let dir = normalize(n + (t * cos(ang) + b * sin(ang)) * side);
        acc = acc + cone_trace(p, dir, aperture) * 0.6;
    }
    let norm = 1.0 + 5.0 * 0.6;
    let bounced = acc.rgb / norm;
    let sky = vox.gi.w * (1.0 - clamp(acc.a / norm, 0.0, 1.0));
    return (bounced + vec3<f32>(sky)) * max(vox.gi.y, 0.0);
}

// ===================== the DDA march (shared by colour + depth passes) =====================
struct VoxHit {
    hit: bool,
    ci: vec3<i32>,   // the solid cell the ray stopped in
    hp: vec3<f32>,   // world hit point
    n: vec3<f32>,    // flat axis-aligned face normal
};

// Amanatides–Woo DDA through the grid AABB. Stops at the first solid cell; records
// the entry-face normal. Miss → hit=false.
fn march_voxels(ro: vec3<f32>, rd: vec3<f32>) -> VoxHit {
    var out: VoxHit;
    out.hit = false;
    out.ci = vec3<i32>(0);
    out.hp = ro;
    out.n = vec3<f32>(0.0, 1.0, 0.0);

    let bmin = vox.bmin.xyz;
    let bmax = vox.bmax.xyz;
    let extent = max(bmax - bmin, vec3<f32>(1e-4));
    let res = vox.grid.xyz;
    let cellw = extent / res; // world size of one voxel per axis

    // Ray vs grid AABB (slab test).
    let inv = 1.0 / rd;
    let ta = (bmin - ro) * inv;
    let tb = (bmax - ro) * inv;
    let tsmall = min(ta, tb);
    let tbig = max(ta, tb);
    let tenter = max(max(tsmall.x, tsmall.y), max(tsmall.z, 0.0));
    let texit = min(min(tbig.x, tbig.y), tbig.z);
    if (texit <= tenter) {
        return out;
    }

    // Entry cell (nudged just inside the volume).
    let pe = ro + rd * (tenter + 1e-4);
    let g = (pe - bmin) / extent * res;
    var ci = clamp(vec3<i32>(floor(g)), vec3<i32>(0), vec3<i32>(res) - vec3<i32>(1));

    let stepf = sign(rd);
    let stepi = vec3<i32>(stepf);
    let tdelta = abs(cellw * inv); // t to cross one voxel per axis
    // t to the next voxel boundary per axis: +face for forward axes, −face for backward.
    let next_face = (vec3<f32>(ci) + max(stepf, vec3<f32>(0.0))) * cellw + bmin;
    var tmaxv = (next_face - ro) * inv;

    // Entry face from whichever slab produced tenter (the axis-aligned normal).
    var mask: vec3<f32>;
    if (tsmall.x >= tsmall.y && tsmall.x >= tsmall.z) {
        mask = vec3<f32>(1.0, 0.0, 0.0);
    } else if (tsmall.y >= tsmall.z) {
        mask = vec3<f32>(0.0, 1.0, 0.0);
    } else {
        mask = vec3<f32>(0.0, 0.0, 1.0);
    }

    let max_steps = i32(vox.bmax.w);
    var tcur = tenter;
    var hit = solid(ci);
    if (!hit) {
        for (var i = 0; i < max_steps; i = i + 1) {
            // Advance into the next voxel along the nearest boundary.
            if (tmaxv.x <= tmaxv.y && tmaxv.x <= tmaxv.z) {
                ci.x = ci.x + stepi.x;
                tcur = tmaxv.x;
                tmaxv.x = tmaxv.x + tdelta.x;
                mask = vec3<f32>(1.0, 0.0, 0.0);
            } else if (tmaxv.y <= tmaxv.z) {
                ci.y = ci.y + stepi.y;
                tcur = tmaxv.y;
                tmaxv.y = tmaxv.y + tdelta.y;
                mask = vec3<f32>(0.0, 1.0, 0.0);
            } else {
                ci.z = ci.z + stepi.z;
                tcur = tmaxv.z;
                tmaxv.z = tmaxv.z + tdelta.z;
                mask = vec3<f32>(0.0, 0.0, 1.0);
            }
            if (!in_bounds(ci)) {
                break; // left the volume
            }
            if (solid(ci)) {
                hit = true;
                break;
            }
        }
    }
    if (!hit) {
        return out;
    }

    // The face we entered through points back along the step direction.
    var n = -stepf * mask;
    if (dot(n, n) < 0.5) {
        n = -rd;
    }
    out.hit = true;
    out.ci = ci;
    out.hp = ro + rd * tcur;
    out.n = normalize(n);
    return out;
}

// Beer–Lambert thickness proxy for the Refractive material: DDA-step the refracted
// ray onward and count the run of solid cells it passes through, as a 0..1 fraction
// of a small cap (thick interiors → murkier, thin rims → clear). A voxel analog of
// neural.wgsl's bound-sphere chord.
fn voxel_chord_frac(hp: vec3<f32>, n: vec3<f32>, dir: vec3<f32>) -> f32 {
    let bmin = vox.bmin.xyz;
    let extent = max(vox.bmax.xyz - vox.bmin.xyz, vec3<f32>(1e-4));
    let res = vox.grid.xyz;
    let cellw = extent / res;
    let stepw = length(cellw);
    let cap = 16.0;
    var sp = hp + dir * stepw * 0.5 - n * stepw * 0.5; // start just inside the surface
    var run = 0.0;
    for (var s = 0; s < 16; s = s + 1) {
        let sc = vec3<i32>(floor((sp - bmin) / extent * res));
        if (!in_bounds(sc)) {
            break;
        }
        if (solid(sc)) {
            run = run + 1.0;
        }
        sp = sp + dir * stepw;
    }
    return clamp(run / cap, 0.0, 1.0);
}

// ===================== PBR + Material shading (adapted from cube.wgsl / neural.wgsl) =====================
// Shade a voxel face with the full metallic-roughness PBR + IBL + Material card.
// The voxel-specific occlusion is folded in: `ao` multiplies the indirect (IBL +
// bounce) term, `shadow` (0..1) attenuates the analytic key light's radiance, and
// `gi_bounce` is the voxel cone-traced colour bleed added as extra indirect.
fn shade_voxel(
    hp: vec3<f32>, n: vec3<f32>, albedo: vec3<f32>,
    ao: f32, shadow: f32, gi_bounce: vec3<f32>, vox_emission: f32,
) -> vec3<f32> {
    let metallic = clamp(u.mat.x, 0.0, 1.0);
    let roughness = clamp(u.mat.y, 0.0, 1.0);
    let glow = u.mat.z;
    let env_intensity = max(u.env.y, 0.0);
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
    // Voxel soft-shadow threads directly into the key light's radiance.
    let key_rad = vec3<f32>(u.key_light.w * clamp(shadow, 0.0, 1.0));
    let fill_w = u.fill_light.w;
    // Indirect (IBL + cone bounce) is what the voxel AO occludes.
    let indirect_occ = clamp(ao, 0.0, 1.0);
    let bounce = gi_bounce * albedo * indirect_occ;
    let emissive = albedo * (glow + max(vox_emission, 0.0));

    let sss_amount = u.sss.x;
    let sss_distortion = u.sss.y;
    let sss_power = max(u.sss.z, 1.0);
    let irid_amount = u.irid.x;
    let irid_raw = iridescent_tint(n_dot_v, u.irid.y, u.irid.z);
    let irid_tint = mix(vec3<f32>(1.0), irid_raw, irid_amount);
    let irid_sheen = irid_raw * (irid_amount * pow(1.0 - n_dot_v, 3.0));
    var sss = vec3<f32>(0.0);
    if (sss_amount > 0.0) {
        let lobe = translucency_lobe(v, l_key, n, sss_distortion, sss_power) * key_rad.x
                 + translucency_lobe(v, l_fill, n, sss_distortion, sss_power) * fill_w;
        sss = albedo * lobe * sss_amount;
    }

    // Anisotropy frame (#214): active on the Anisotropic material (type 4) or the
    // overlay on Standard/Chrome. No per-vertex brush on a raymarched face, so the
    // brush is world-up projected onto the surface; the rotation dial re-aims it.
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
            * env_intensity * ambient_mul * etint * irid_tint * indirect_occ;
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
        shaded = mirror + direct + emissive + irid_sheen + bounce;
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
        // Refractive (type 3): Beer–Lambert absorption over the voxel chord along
        // the refracted ray (thick interiors go murky in the node colour).
        var absorb = vec3<f32>(1.0);
        if (mat_type >= 2.5 && u.refr.x > 0.0) {
            var tdir = refract(-v, n, 1.0 / ior);
            if (dot(tdir, tdir) < 1e-6) { tdir = reflect(-v, n); }
            let thick = voxel_chord_frac(hp, n, normalize(tdir));
            let sigma = (vec3<f32>(1.0) - clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0))) * u.refr.x;
            absorb = exp(-sigma * thick);
        }
        let film = thin_film_tint(n_dot_v, thin_film);
        let tint = mix(mix(vec3<f32>(1.0), albedo, 0.5), vec3<f32>(1.0), glass_clarity);
        // Voxel AO darkens the env reflect+refract in crevices, like the Chrome /
        // Standard branches occlude their IBL term (folded into env_mul so both
        // glass_body and glass_surf inherit it).
        let env_mul = tint * env_intensity * ambient_mul * etint * indirect_occ;
        let glass_body = thru * (1.0 - fr) * env_mul * absorb;
        let glass_surf = reflected * irid_tint * film * fr * env_mul;
        let dg_rough = max(g_rough, 0.02);
        let glass_f0 = vec3<f32>(f0s * f0s);
        let direct = direct_light(n, v, l_key, albedo, metallic, dg_rough, glass_f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, dg_rough, glass_f0, vec3<f32>(fill_w));
        // Opaque composite (the voxel pass isn't alpha-blended): the refracted env
        // stands in for "what's behind", so the block still reads as glass.
        shaded = glass_body + glass_surf + direct + emissive + sss + irid_sheen + bounce;
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
        let ambient = (kd * diffuse + specular) * env_intensity * ambient_mul * etint * indirect_occ;
        var direct: vec3<f32>;
        if (aniso_on) {
            direct = direct_light_aniso(n, v, l_key, af_t, af_b, albedo, metallic, a_t, a_b, f0, key_rad)
                   + direct_light_aniso(n, v, l_fill, af_t, af_b, albedo, metallic, a_t, a_b, f0, vec3<f32>(fill_w));
        } else {
            direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(fill_w));
        }
        shaded = ambient + direct + emissive + sss + irid_sheen + bounce;
    }
    return shaded;
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
    let near = vox.inv_vp * vec4<f32>(ndc, 0.0, 1.0);
    let far = vox.inv_vp * vec4<f32>(ndc, 1.0, 1.0);
    out.rd = (far.xyz / far.w) - (near.xyz / near.w);
    return out;
}

// ===================== fragment (DDA + physically-based shade) =====================
struct FragOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// Voxel ambient occlusion for the hit face: the classic per-corner
// neighbour-occupancy AO, bilerped across the face. Returns 1 (no occlusion) when
// AO strength is 0 — the 8 neighbour `solid()` loads are pure waste otherwise.
fn face_ao(ci: vec3<i32>, hp: vec3<f32>, n: vec3<f32>) -> f32 {
    if (vox.shade.x <= 0.0) {
        return 1.0;
    }
    let bmin = vox.bmin.xyz;
    let extent = max(vox.bmax.xyz - vox.bmin.xyz, vec3<f32>(1e-4));
    let res = vox.grid.xyz;
    // The "air" cell in front of the hit face, and the two in-plane axes.
    let air = ci + vec3<i32>(round(n));
    var ax: vec3<i32>;
    var bx: vec3<i32>;
    var uv: vec2<f32>;
    let frac = (hp - bmin) / extent * res - vec3<f32>(ci); // position within the cell
    let nabs = abs(n);
    if (nabs.x > 0.5) {
        ax = vec3<i32>(0, 1, 0);
        bx = vec3<i32>(0, 0, 1);
        uv = vec2<f32>(frac.y, frac.z);
    } else if (nabs.y > 0.5) {
        ax = vec3<i32>(1, 0, 0);
        bx = vec3<i32>(0, 0, 1);
        uv = vec2<f32>(frac.x, frac.z);
    } else {
        ax = vec3<i32>(1, 0, 0);
        bx = vec3<i32>(0, 1, 0);
        uv = vec2<f32>(frac.x, frac.y);
    }
    let s_pa = solid(air + ax);
    let s_ma = solid(air - ax);
    let s_pb = solid(air + bx);
    let s_mb = solid(air - bx);
    let s_pp = solid(air + ax + bx);
    let s_pm = solid(air + ax - bx);
    let s_mp = solid(air - ax + bx);
    let s_mm = solid(air - ax - bx);
    let ao00 = vertex_ao(s_ma, s_mb, s_mm);
    let ao10 = vertex_ao(s_pa, s_mb, s_pm);
    let ao01 = vertex_ao(s_ma, s_pb, s_mp);
    let ao11 = vertex_ao(s_pa, s_pb, s_pp);
    let ao_raw = mix(mix(ao00, ao10, uv.x), mix(ao01, ao11, uv.x), uv.y);
    return mix(1.0, ao_raw, clamp(vox.shade.x, 0.0, 1.0));
}

// One soft shadow ray DDA-stepped toward the key light. Returns 1 = lit, 0 =
// occluded (scaled by the shadow strength). Inert (1) when shadow strength is 0.
fn face_shadow(hp: vec3<f32>, n: vec3<f32>) -> f32 {
    if (vox.shade.y <= 0.0) {
        return 1.0;
    }
    let bmin = vox.bmin.xyz;
    let extent = max(vox.bmax.xyz - vox.bmin.xyz, vec3<f32>(1e-4));
    let res = vox.grid.xyz;
    let cellw = extent / res;
    let lk = normalize(u.key_light.xyz);
    let stepw = length(cellw);
    var sp = hp + n * stepw * 0.5 + lk * stepw * 0.75;
    var lit = 1.0;
    for (var s = 0; s < 24; s = s + 1) {
        let sc = vec3<i32>(floor((sp - bmin) / extent * res));
        if (!in_bounds(sc)) {
            break; // reached open sky → lit
        }
        if (solid(sc)) {
            lit = 0.0;
            break;
        }
        sp = sp + lk * stepw;
    }
    return mix(1.0, lit, clamp(vox.shade.y, 0.0, 1.0));
}

@fragment
fn fs_ray(in: VsOut) -> FragOut {
    let ro = u.camera_pos.xyz;
    let rd = normalize(in.rd);

    let h = march_voxels(ro, rd);
    if (!h.hit) {
        discard;
    }
    let ci = h.ci;
    let n = h.n;
    let hp = h.hp;

    // Albedo: the splatted node colour (palette / HSV sweep / RGB-cube).
    var albedo: vec3<f32> = cell_color(ci);
    // Optional palette posterization (the MagicaVoxel blocky-colour charm).
    let levels = vox.shade.w;
    if (levels >= 1.0) {
        albedo = floor(albedo * levels + 0.5) / levels;
    }

    let ao = face_ao(ci, hp, n);
    let shadow = face_shadow(hp, n);
    let gi_bounce = voxel_gi(hp, n); // cone-traced colour bleed (#89), 0 when GI off

    let shaded = shade_voxel(hp, n, albedo, ao, shadow, gi_bounce, vox.shade.z);

    var out: FragOut;
    out.color = vec4<f32>(shaded, 1.0);
    // Project the hit (reconstructed in unscaled world space from vox.inv_vp)
    // through the matching UNSCALED view_proj so the voxel depth-composites with
    // the background — NOT the scene's breath-scaled u.view_proj.
    let clip = vox.view_proj * vec4<f32>(hp, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}

// ===================== depth-only prepass (screen-space GI) =====================
// The SAME DDA march as `fs_ray`, shading-free: it only finds the hit and writes
// `frag_depth` into the single-sample screen-space-FX prepass, so SSR (#80 A) and
// SSGI (#152 T2) can reconstruct the voxel surface's normal from depth derivatives
// and gather off it. Miss → `discard` (the prepass keeps its cleared far depth).
struct DepthOut { @builtin(frag_depth) depth: f32 };

@fragment
fn fs_ray_depth(in: VsOut) -> DepthOut {
    let ro = u.camera_pos.xyz;
    let rd = normalize(in.rd);
    let h = march_voxels(ro, rd);
    if (!h.hit) {
        discard;
    }
    var out: DepthOut;
    let clip = vox.view_proj * vec4<f32>(h.hp, 1.0);
    out.depth = clip.z / clip.w;
    return out;
}

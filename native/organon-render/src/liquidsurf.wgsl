// Refractive liquid surface (#182 Tier 3b route B, first slice) — optically
// correct see-through water for the MLS-MPM liquid.
//
// Runs AFTER the scene resolves (with VXGI/SSR/SSGI already composited) and
// before the ink march / bloom. Per pixel:
//   1. March the liquid's density field to the isosurface (the same MetaField
//      the isosurface mode draws), clamped at scene depth — geometry in front
//      occludes the water.
//   2. Normal from the field gradient; energy-conserving Fresnel split with
//      F0 = ((ior−1)/(ior+1))².
//   3. TRANSMISSION: bend the view ray by Snell (refract at the live IOR),
//      march the body to the exit for the real THICKNESS, project the exit
//      point back to screen and fetch the RESOLVED SCENE — the cubes appear
//      through the water, displaced correctly — attenuated by Beer–Lambert
//      absorption over the measured path (σ tinted by the liquid colour).
//      Off-screen / occluded fetches fall back to the refracted environment.
//   4. REFLECTION: prefiltered environment at the reflect direction (roughness
//      picks the mip), weighted by F.
// Output replaces the covered pixels (miss → discard, scene untouched).
//
// Approximations (documented): single-interface refraction (the exit bend is
// dropped — thickness is measured, direction isn't re-bent), env-only
// reflection, and the screen fetch can't see geometry hidden behind other
// geometry (screen-space by construction).

struct LsU {
    inv_vp: mat4x4<f32>,   // clip → world (unproject rays)
    view_proj: mat4x4<f32>,// world → clip (project the refracted exit point)
    cam: vec4<f32>,        // xyz = camera pos, w = use_depth (0/1)
    bmin: vec4<f32>,       // tank min, w = iso threshold
    bmax: vec4<f32>,       // tank max, w = entry march steps
    liq: vec4<f32>,        // rgb = liquid colour (absorption tint), w = absorption σ
    ctl: vec4<f32>,        // x = ior, y = roughness, z = env rotation, w = env intensity
    misc: vec4<f32>,       // xy = 1 / target size, zw = _
    vgi0: vec4<f32>,       // xyz = VXGI volume min, w = VXGI gain (0 = off/stale)
    vgi1: vec4<f32>,       // xyz = VXGI volume max
};

@group(0) @binding(0) var<uniform> u: LsU;
@group(0) @binding(1) var field_tex: texture_3d<f32>;
@group(0) @binding(2) var lin_samp: sampler;
// The pre-liquid resolved HDR scene (a copy — we write the live buffer).
@group(0) @binding(3) var scene_tex: texture_2d<f32>;
@group(0) @binding(4) var depth_tex: texture_depth_2d;
// The GI the water body scatters (#182 T4): the probe uniform + the VXGI
// radiance volume — murky water glows with the scene's bounce light.
struct GiUniform {
    bbox_min: vec4<f32>,
    bbox_max: vec4<f32>,
    info: vec4<f32>, // x=enabled, y=intensity, z=grid_dim, w=falloff
    probes: array<vec4<f32>, 648>,
};
@group(0) @binding(5) var<uniform> gi: GiUniform;
@group(0) @binding(6) var vxgi_tex: texture_3d<f32>;
@group(0) @binding(7) var vxgi_samp: sampler;
// Emissive-cube point lights (#167 / #182 T4 ghost light) — the same LightsU
// the cube + metaball shaders loop; count 0 → off.
struct MLight { pos: vec4<f32>, color: vec4<f32> };
struct LightsU {
    header: vec4<f32>, // x=count, y=intensity
    lights: array<MLight, 64>,
};
@group(0) @binding(8) var<uniform> mlights: LightsU;

fn gi_l0_at(x: i32, y: i32, z: i32, dim: i32) -> vec3<f32> {
    let c = clamp(vec3<i32>(x, y, z), vec3<i32>(0), vec3<i32>(dim - 1));
    let base = ((c.z * dim + c.y) * dim + c.x) * 3;
    return vec3<f32>(gi.probes[base].x, gi.probes[base + 1].x, gi.probes[base + 2].x);
}

// Flat probe irradiance × intensity (matches cube.wgsl's gi_irradiance scale).
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
    return max(mix(mix(x00, x10, f.y), mix(x01, x11, f.y), f.z), vec3<f32>(0.0)) * gi.info.y;
}

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

// IBL (same layout as cube.wgsl group 1; only irradiance/prefilter are used).
@group(1) @binding(0) var irradiance_tex: texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex: texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex: texture_2d<f32>;
@group(1) @binding(3) var ibl_samp: sampler;

const THICK_STEPS: i32 = 24;

fn rotate_y(d: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

fn dir_to_equirect_uv(dir: vec3<f32>) -> vec2<f32> {
    let d = normalize(dir);
    let uv = vec2<f32>(atan2(d.z, d.x) / 6.2831853 + 0.5, acos(clamp(d.y, -1.0, 1.0)) / 3.1415927);
    return uv;
}

fn sample_prefiltered(dir: vec3<f32>, roughness: f32) -> vec3<f32> {
    let mips = f32(textureNumLevels(prefilter_tex));
    let lod = roughness * max(mips - 1.0, 0.0);
    return textureSampleLevel(prefilter_tex, ibl_samp,
                              dir_to_equirect_uv(rotate_y(dir, u.ctl.z)), lod).rgb;
}

// Field density at a world point (the splat pass already applied the reveal
// window, so the visible surface and the refraction agree).
fn field_at(p: vec3<f32>) -> f32 {
    let ext = max(u.bmax.xyz - u.bmin.xyz, vec3<f32>(1e-5));
    let uvw = (p - u.bmin.xyz) / ext;
    if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) {
        return 0.0;
    }
    return textureSampleLevel(field_tex, lin_samp, uvw, 0.0).a;
}

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

// World-space distance along the ray to the scene surface at a pixel (1e30 =
// no occluder). Same reconstruction as the ink march.
fn scene_t(px: vec2<i32>, ro: vec3<f32>) -> f32 {
    if (u.cam.w < 0.5) {
        return 1e30;
    }
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let p = clamp(px, vec2<i32>(0), dims - vec2<i32>(1));
    let d = textureLoad(depth_tex, p, 0);
    if (d >= 1.0) {
        return 1e30;
    }
    let uvpx = (vec2<f32>(p) + vec2<f32>(0.5)) / vec2<f32>(dims);
    let ndc = vec2<f32>(uvpx.x * 2.0 - 1.0, 1.0 - uvpx.y * 2.0);
    let wp = u.inv_vp * vec4<f32>(ndc, d, 1.0);
    return length(wp.xyz / wp.w - ro);
}

@fragment
fn fs_liquid(in: VsOut) -> @location(0) vec4<f32> {
    let ro = u.cam.xyz;
    let rd = normalize(in.rd);

    // Ray vs the tank AABB.
    let inv = 1.0 / rd;
    let ta = (u.bmin.xyz - ro) * inv;
    let tb = (u.bmax.xyz - ro) * inv;
    let tsm = min(ta, tb);
    let tbg = max(ta, tb);
    let tmin = max(max(tsm.x, tsm.y), max(tsm.z, 0.0));
    var tmax = min(min(tbg.x, tbg.y), tbg.z);
    // Geometry in front occludes the water.
    let px = vec2<i32>(in.clip.xy);
    tmax = min(tmax, scene_t(px, ro));
    if (tmax <= tmin) {
        discard;
    }

    // 1) Entry: march to the isosurface, linear refine.
    let thresh = max(u.bmin.w, 1e-3);
    let steps = max(i32(u.bmax.w), 16);
    let dt = (tmax - tmin) / f32(steps);
    var t = tmin + dt * 0.5;
    var prev = 0.0;
    var hit = -1.0;
    for (var i = 0; i < steps; i = i + 1) {
        let d = field_at(ro + rd * t);
        if (d >= thresh) {
            let frac = clamp((thresh - prev) / max(d - prev, 1e-5), 0.0, 1.0);
            hit = t - dt + frac * dt;
            break;
        }
        prev = d;
        t = t + dt;
    }
    if (hit < 0.0) {
        discard;
    }
    let hp = ro + rd * hit;

    // 2) Normal from the field gradient (density rises inward → outward = −∇).
    let e = max(u.bmax.xyz - u.bmin.xyz, vec3<f32>(1e-5)) / 64.0;
    var n = -vec3<f32>(
        field_at(hp + vec3<f32>(e.x, 0.0, 0.0)) - field_at(hp - vec3<f32>(e.x, 0.0, 0.0)),
        field_at(hp + vec3<f32>(0.0, e.y, 0.0)) - field_at(hp - vec3<f32>(0.0, e.y, 0.0)),
        field_at(hp + vec3<f32>(0.0, 0.0, e.z)) - field_at(hp - vec3<f32>(0.0, 0.0, e.z)),
    );
    if (dot(n, n) < 1e-12) {
        n = -rd;
    }
    n = normalize(n);
    if (dot(n, rd) > 0.0) {
        n = -n;
    }

    // 3) Fresnel split from the live IOR (energy conserving: T = 1 − F).
    let ior = max(u.ctl.x, 1.001);
    let f0s = (ior - 1.0) / (ior + 1.0);
    let f0 = f0s * f0s;
    let n_dot_v = max(dot(n, -rd), 1e-4);
    let fr = f0 + (1.0 - f0) * pow(1.0 - n_dot_v, 5.0);

    // 4) Transmission: refract, march the body for the real thickness.
    var refr = refract(rd, n, 1.0 / ior);
    if (dot(refr, refr) < 1e-6) {
        refr = reflect(rd, n); // grazing fallback (TIR can't happen entering)
    }
    refr = normalize(refr);
    // Exit distance along the refracted ray (slab test from just inside).
    let ro2 = hp + refr * 1e-3;
    let ta2 = (u.bmin.xyz - ro2) / refr;
    let tb2 = (u.bmax.xyz - ro2) / refr;
    let exit_t = max(min(min(max(ta2.x, tb2.x), max(ta2.y, tb2.y)), max(ta2.z, tb2.z)), 0.0);
    let tdt = exit_t / f32(THICK_STEPS);
    var thickness = 0.0;
    for (var i = 0; i < THICK_STEPS; i = i + 1) {
        let q = ro2 + refr * ((f32(i) + 0.5) * tdt);
        if (field_at(q) >= thresh * 0.5) {
            thickness = thickness + tdt;
        }
    }
    let pe = ro2 + refr * max(thickness, tdt);

    // Project the exit point to screen and fetch the resolved scene there.
    var trans_col = sample_prefiltered(refr, u.ctl.y); // env fallback
    let clip = u.view_proj * vec4<f32>(pe, 1.0);
    if (clip.w > 0.0) {
        let ndc = clip.xyz / clip.w;
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0) {
            // Reject fetches of geometry IN FRONT of the water surface (it
            // would smear foreground objects into the refraction): compare
            // the fetched pixel's scene distance against the entry distance.
            let dims = vec2<f32>(textureDimensions(scene_tex));
            let fpx = vec2<i32>(uv * dims);
            var ok = true;
            if (u.cam.w > 0.5) {
                let st = scene_t(fpx, ro);
                ok = st > hit * 0.9;
            }
            if (ok) {
                let fetched = textureSampleLevel(scene_tex, lin_samp, uv, 0.0).rgb;
                // Fade to the env fallback near the screen border.
                let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
                let w = smoothstep(0.0, 0.05, edge);
                trans_col = mix(trans_col, fetched, w);
            }
        }
    }
    // Beer–Lambert absorption over the measured path: the liquid colour is
    // what SURVIVES (σ per channel = (1 − colour) × strength).
    let sigma = (vec3<f32>(1.0) - clamp(u.liq.rgb, vec3<f32>(0.0), vec3<f32>(1.0)))
        * max(u.liq.w, 0.0);
    let absorb = exp(-sigma * thickness);

    // 5) Reflection: prefiltered env at the reflect direction.
    let refl = sample_prefiltered(reflect(rd, n), u.ctl.y) * u.ctl.w;

    // 5b) Point lights on the water (#167 / ghost light): each light adds a
    // Blinn specular sparkle at the surface (Fresnel-weighted) and a diffuse
    // glow into the murk (single-scatter, like the GI bounce below).
    var ml_spec = vec3<f32>(0.0);
    var ml_murk = vec3<f32>(0.0);
    let ml_count = min(i32(mlights.header.x), 64);
    if (ml_count > 0) {
        let shininess = mix(256.0, 8.0, clamp(u.ctl.y, 0.0, 1.0));
        for (var i = 0; i < ml_count; i = i + 1) {
            let lp = mlights.lights[i].pos.xyz;
            let radius = max(mlights.lights[i].pos.w, 1e-3);
            let to_l = lp - hp;
            let dist = length(to_l);
            let window = pow(clamp(1.0 - pow(dist / (radius * 4.0), 4.0), 0.0, 1.0), 2.0);
            let atten = window / (1.0 + (dist * dist) / (radius * radius));
            if (atten < 1e-4) {
                continue;
            }
            let radiance = mlights.lights[i].color.rgb * mlights.header.y * atten;
            let l = to_l / max(dist, 1e-4);
            let h = normalize(l - rd);
            ml_spec += radiance * pow(max(dot(n, h), 0.0), shininess);
            ml_murk += radiance;
        }
    }

    // 6) GI in the body (#182 T4): where absorption removes transmitted scene
    // light, murky water glows with scattered bounce instead — probe GI + the
    // VXGI radiance volume sampled at the body midpoint, single-scatter albedo
    // = the liquid colour. Crystal-clear water (absorb → 1) stays GI-free,
    // exactly as optics wants; the GI toggles light the murk.
    let pm = hp + refr * (max(thickness, tdt) * 0.5);
    let bounce = (gi_l0(pm) + vxgi_bounce(pm) + ml_murk)
        * clamp(u.liq.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let body = trans_col * absorb + bounce * (vec3<f32>(1.0) - absorb);

    let color = refl * fr + body * (1.0 - fr) + ml_spec * max(fr, 0.04);
    return vec4<f32>(color, 1.0);
}

// Hardware-RT reflections (#195 Tier 2). A fullscreen pass off the depth
// prepass: reconstruct each pixel's world position + geometric normal, reflect
// the view ray, and trace the CLOSEST hit against the Tier-0 TLAS. A hit is
// shaded PBR-lite from the hit instance's own geometry — the local-space trick:
// invert the instance's affine transform, and for a unit cube the local hit
// position IS both the RGB-cube colour (local + 0.5) and the face normal (the
// dominant axis); for the cylinder mesh the normal is radial and the colour is
// the white base the per-instance tint paints. Emissive glow + key/fill diffuse
// (with an optional traced shadow ray at the hit — reflections contain
// shadows) + a flat ambient stand-in for the IBL irradiance.
//
// Output goes into the SAME buffer SSR writes — rgb premultiplied by a
// confidence weight that is also stored in alpha — so the composite's existing
// blend (`color·(1-w) + rgb`) needs no change and a miss (weight 0) falls back
// to the cube shader's env reflection with no seam. Unlike SSR there is NO
// screen-edge fade: the trace is world-space, off-screen and behind-camera
// geometry reflects fine — that's the point.
//
// Roughness: the reflected direction is jittered in a cone scaled by the
// material roughness (stochastic; TAA integrates it), and the weight fades
// toward the max-roughness cutoff exactly like SSR so the two sources are
// interchangeable per-preset.

enable wgpu_ray_query;

// Mirror of render.rs::Uniforms (the scene's group-0 block, written verbatim).
struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    mat: vec4<f32>,        // x=metallic, y=roughness, z=glow, w=prefilter_mip_count
    env: vec4<f32>,        // x=exposure, y=env_intensity, z=env_rotation, w=opacity
    key_light: vec4<f32>,  // xyz = dir TO key light, w = intensity
    fill_light: vec4<f32>, // xyz = dir TO fill light, w = intensity
    amb: vec4<f32>,        // x=ambient/IBL mult, y=material_type, z=glass IOR
    sss: vec4<f32>,
    irid: vec4<f32>,
    env_tint: vec4<f32>,
    ripple: vec4<f32>,
    ripple_ctr: vec4<f32>,
    ripple_mode: vec4<f32>,
    glassx: vec4<f32>,
    reflect_ctl: vec4<f32>,
    refl_box_min: vec4<f32>,
    refl_box_max: vec4<f32>,
};

struct RtReflectU {
    inv_view_proj: mat4x4<f32>, // the JITTERED current VP inverse (matches the prepass)
    params: vec4<f32>,  // x = intensity, y = max_roughness, z = t_max, w = frame index
    params2: vec4<f32>, // x = tube, y = hit shadow (0/1), z = shadow reach, w = rays (1–16)
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<uniform> r: RtReflectU;
@group(0) @binding(2) var depth_tex: texture_depth_2d;
@group(0) @binding(3) var<storage, read> insts: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read> tints: array<vec4<f32>>;
@group(1) @binding(0) var tlas: acceleration_structure;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

// Spatiotemporal blue noise (#200 Tier 4½), texture-free — IGN spatial dither
// (blue-noise-like, TAA/bilateral-friendly) advanced per (frame, sample) by the
// golden ratio on a 64-frame cycle. A Cranley–Patterson rotation of the pass's
// GGX cone sample: same mean, higher-frequency (eye-friendly) error.
fn ign1(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}
fn stbn2(px: vec2<u32>, frame: u32, si: u32) -> vec2<f32> {
    let p = vec2<f32>(px);
    let s0 = ign1(p);
    let s1 = ign1(p + vec2<f32>(113.0, 71.0));
    // Cranley–Patterson rotation of the per-pixel IGN blue noise. The frame
    // advances by the FULL golden-ratio conjugate (φ⁻¹ / φ⁻²) and the per-sample
    // index by INDEPENDENT irrationals (√2−1 / √3−1), so consecutive frames jump
    // ~0.38–0.62 of the cycle (fast, grain-like — TAA/the AO bilateral integrate
    // it) and the samples within a frame stay decorrelated. The previous
    // `((frame%64)*5 + si)*φ⁻¹` collapsed the per-frame step to 5·φ⁻¹ mod 1 ≈
    // 0.09 ≈ 1/11 — a SLOW ~11-frame sweep that the whole screen rode together,
    // reading as a ~10 Hz "vibration" on RT AO (#208 review). No *5 = no rational
    // collision; the two channels use different rates so they don't correlate.
    let a = f32(frame % 64u);
    let b = f32(si);
    return fract(vec2<f32>(
        s0 + a * 0.6180339887 + b * 0.4142135624,
        s1 + a * 0.3819660113 + b * 0.7320508076,
    ));
}

// Jitter `dir` in a cone (tangent radius `radius`); xi = per-pixel randoms.
fn cone_dir(dir: vec3<f32>, radius: f32, xi: vec2<f32>) -> vec3<f32> {
    if (radius <= 0.0) {
        return dir;
    }
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(dir.y) > 0.9);
    let t = normalize(cross(up, dir));
    let b = cross(dir, t);
    let rr = sqrt(xi.x) * radius;
    let a = xi.y * 6.28318530718;
    return normalize(dir + (t * cos(a) + b * sin(a)) * rr);
}

// Inverse of the 3x3 linear part of an instance transform (rotation·scale —
// always invertible for drawn instances; det guarded anyway).
fn inv3(m: mat4x4<f32>) -> mat3x3<f32> {
    let a = vec3<f32>(m[0].xyz);
    let b = vec3<f32>(m[1].xyz);
    let c = vec3<f32>(m[2].xyz);
    let r0 = cross(b, c);
    let r1 = cross(c, a);
    let r2 = cross(a, b);
    let det = dot(a, r0);
    let inv_det = 1.0 / select(det, 1e-8, abs(det) < 1e-12);
    // rows of the inverse = the cross products / det (columns transposed).
    return mat3x3<f32>(
        vec3<f32>(r0.x, r1.x, r2.x) * inv_det,
        vec3<f32>(r0.y, r1.y, r2.y) * inv_det,
        vec3<f32>(r0.z, r1.z, r2.z) * inv_det,
    );
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(depth_tex));
    let px = vec2<u32>(clamp(in.pos.xy, vec2<f32>(0.0), dims - 1.0));
    let d = textureLoad(depth_tex, px, 0);
    // Reconstruct unconditionally so the derivative normal is well-defined.
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, (1.0 - in.uv.y) * 2.0 - 1.0);
    let clip = r.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    let wp = clip.xyz / clip.w;
    let n = normalize(cross(dpdy(wp), dpdx(wp)));
    if (d >= 1.0) {
        return vec4<f32>(0.0); // sky — env reflection stands
    }

    let cam = u.camera_pos.xyz;
    let v = normalize(wp - cam);           // view ray, camera → surface
    let roughness = clamp(u.mat.y, 0.0, 1.0);
    let max_rough = max(r.params.y, 1e-3);
    if (roughness >= max_rough) {
        return vec4<f32>(0.0); // too rough to mirror — env/IBL stands
    }
    let dist0 = length(wp - cam);
    let origin = wp + n * max(1e-3, dist0 * 3e-3);
    let base_refl = reflect(v, n);
    let frame = u32(r.params.w);
    let rays = clamp(u32(r.params2.w), 1u, 16u); // 1–16; 1 = the original look
    let l_key = normalize(u.key_light.xyz);
    let l_fill = normalize(u.fill_light.xyz);
    let do_shadow = r.params2.y > 0.5;

    // N stochastic reflection rays (GGX-ish cone by roughness). Accumulate the
    // hit radiance; misses dilute the confidence so the env reflection shows
    // through the missed fraction. Hoisted `rq` (reused for reflection + the
    // in-loop shadow ray) — a loop-local ray_query zero-inits to Metal's deleted
    // intersection_query operator= (the #195 T3 crash). N = 1 is byte-for-byte
    // the original single-ray path.
    var radiance = vec3<f32>(0.0);
    var hitcount = 0u;
    var rq: ray_query;
    for (var s = 0u; s < rays; s = s + 1u) {
        let xi = stbn2(px, frame, s);
        let refl = cone_dir(base_refl, roughness * 0.35, xi);
        rayQueryInitialize(&rq, tlas, RayDesc(0x1u /* FORCE_OPAQUE, closest hit */, 0xFFu, 0.0, r.params.z, origin, refl));
        loop {
            if (!rayQueryProceed(&rq)) { break; }
        }
        let hit = rayQueryGetCommittedIntersection(&rq);
        if (hit.kind != RAY_QUERY_INTERSECTION_TRIANGLE) {
            continue; // miss → env fallback (dilutes the weight below)
        }
        // Read the hit into locals BEFORE the shadow ray reuses `rq`.
        let idx = min(hit.instance_custom_data, arrayLength(&insts) - 1u);
        let ht = hit.t;
        let m = insts[idx];
        let ainv = inv3(m);
        let hp = origin + refl * ht;
        let local = ainv * (hp - m[3].xyz);
        var n_loc: vec3<f32>;
        var albedo_loc: vec3<f32>;
        if (r.params2.x > 0.5) {
            // Cylinder mesh: radial normal, white base (the tint carries colour).
            n_loc = normalize(vec3<f32>(local.xy, 0.0));
            albedo_loc = vec3<f32>(1.0);
        } else {
            // Unit cube: dominant local axis = the face normal; colour = the
            // classic RGB cube (local + 0.5), exactly like the vertex data.
            let al = abs(local);
            if (al.x >= al.y && al.x >= al.z) {
                n_loc = vec3<f32>(sign(local.x), 0.0, 0.0);
            } else if (al.y >= al.z) {
                n_loc = vec3<f32>(0.0, sign(local.y), 0.0);
            } else {
                n_loc = vec3<f32>(0.0, 0.0, sign(local.z));
            }
            albedo_loc = clamp(local + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
        }
        var hn = normalize(transpose(ainv) * n_loc);
        if (dot(hn, refl) > 0.0) {
            hn = -hn; // two-sided (cull is None in the raster path too)
        }
        let albedo = albedo_loc * tints[idx].rgb;

        // Optional traced key-shadow ray, reusing the hoisted `rq` (its own
        // reach, params2.z — #204: not the reflection reach dial).
        var key_vis = 1.0;
        if (do_shadow) {
            rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 0.0, r.params2.z, hp + hn * 1e-2, l_key));
            loop {
                if (!rayQueryProceed(&rq)) { break; }
            }
            let sh = rayQueryGetCommittedIntersection(&rq);
            key_vis = select(1.0, 0.0, sh.kind == RAY_QUERY_INTERSECTION_TRIANGLE);
        }
        let diffuse = albedo
            * (u.key_light.w * max(dot(hn, l_key), 0.0) * key_vis
                + u.fill_light.w * max(dot(hn, l_fill), 0.0));
        let ambient = albedo * u.amb.x * u.env_tint.rgb * 0.3;
        let emissive = albedo * u.mat.z;
        radiance = radiance + emissive + diffuse + ambient;
        hitcount = hitcount + 1u;
    }
    if (hitcount == 0u) {
        return vec4<f32>(0.0); // all rays missed → env stands (no seam)
    }
    let color = radiance / f32(hitcount);      // mean over the rays that hit
    let hit_frac = f32(hitcount) / f32(rays);  // partial coverage

    // ---- Confidence weight: mirror ssr.wgsl (Fresnel × material reflectivity
    // × roughness fade × intensity) — but NO screen-edge fade: the trace is
    // world-space, so there is no edge to dissolve at. Scaled by the hit
    // fraction so the env reflection shows through the rays that missed.
    let ndv = max(dot(n, -v), 1e-3);
    let mat_type = u.amb.y;
    var f0 = 0.04 + 0.8 * clamp(u.mat.x, 0.0, 1.0);
    if (mat_type > 0.5 && mat_type < 1.5) { f0 = 0.9; } // Chrome
    let fres = f0 + (1.0 - f0) * pow(1.0 - ndv, 5.0);
    let rough_fade = 1.0 - smoothstep(max_rough * 0.5, max_rough, roughness);
    let weight = clamp(fres * rough_fade * r.params.x, 0.0, 1.0) * hit_frac;
    return vec4<f32>(color * weight, weight);
}

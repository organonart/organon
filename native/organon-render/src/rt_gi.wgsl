// Hardware-RT diffuse global illumination — one bounce (#195 Tier 4, Option B).
// A fullscreen gather off the depth prepass: reconstruct each pixel's world
// position + geometric normal, fire N cosine-weighted hemisphere rays against
// the Tier-0 TLAS, and shade each CLOSEST hit as the outgoing radiance of the
// neighbour it struck — its emissive glow plus a fraction of its direct key
// light (the VXGI injection estimate, `glow + 0.3·key`, with an optional
// traced key-shadow ray so the bounced light is itself shadowed). The
// cosine-weighted average of that incoming radiance is one bounce of real
// inter-cube colour bleed, off-screen emitters included — what SSGI can only
// gather from on-screen neighbours.
//
// Output is exposed indirect radiance written into the SAME buffer SSGI fills;
// `composite.wgsl` adds it (× exposure) unchanged, so a miss (→ 0) leaves the
// scene's own IBL ambient as the only indirect term — no seam. Supersedes the
// SSGI march while on. Per-pixel + per-frame jittered; TAA integrates the low
// ray count.

enable wgpu_ray_query;

// Mirror of render.rs::Uniforms (the scene's group-0 block, written verbatim).
struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    mat: vec4<f32>,        // x=metallic, y=roughness, z=glow, w=prefilter_mip_count
    // organon#217: x is `bg_brightness`, not exposure (see render.rs::Uniforms). This
    // pass never reads it — it has no sky at all: a missed gather ray contributes 0 and
    // the receiver's own IBL ambient stands. There is nothing here to gate.
    env: vec4<f32>,        // x=bg_brightness, y=env_intensity, z=env_rotation, w=opacity
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

struct RtGiU {
    inv_view_proj: mat4x4<f32>, // the JITTERED current VP inverse (matches the prepass)
    cam_pos: vec4<f32>,         // xyz = camera world pos, w = frame index (jitter seed)
    params: vec4<f32>,          // x = intensity, y = rays (1–16), z = gi reach (world), w unused
    params2: vec4<f32>,         // x = tube (cyl mesh), y = hit shadow ray (0/1), z = shadow reach, w unused
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<uniform> g: RtGiU;
@group(0) @binding(2) var depth_tex: texture_depth_2d;
@group(0) @binding(3) var<storage, read> insts: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read> tints: array<vec4<f32>>;
// organon#217 T8 — the per-instance EMISSION the cube pipeline reads at @location(8):
// linear radiance in rgb, gain in w (the same `emit_buf` the raster path binds at vertex
// slot 3). A lit tile is a neighbour that emits, so its light bounces onto the backplane.
@group(0) @binding(5) var<storage, read> emits: array<vec4<f32>>;
@group(1) @binding(0) var tlas: acceleration_structure;

// The indirect fraction of a hit's direct key light that leaves it toward the
// receiver (matches VXGI's `glow + 0.3·key` node-radiance estimate).
const GI_FRACTION: f32 = 0.3;

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
// golden ratio on a 64-frame cycle. Rotates each hemisphere gather sample: same
// mean, higher-frequency (eye-friendly) error.
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

// Cosine-weighted hemisphere direction around `n` (importance-samples the
// diffuse gather, so the unweighted hit average is the irradiance estimator).
fn cosine_dir(n: vec3<f32>, xi: vec2<f32>) -> vec3<f32> {
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    let r = sqrt(xi.x);
    let a = xi.y * 6.28318530718;
    let z = sqrt(max(0.0, 1.0 - xi.x));
    return normalize(t * (r * cos(a)) + b * (r * sin(a)) + n * z);
}

// organon#217 T8 — the per-instance emission at a hit: THE SAME EXPRESSION `cube.wgsl` adds
// into its emissive term from @location(8) (`emit.rgb * emit.w`), so raster and traced agree
// on what a lit cell is worth (§9's second law). The all-zero buffer every non-glyph draw
// binds makes this exactly vec3(0.0) — invariant #4.
fn instance_emission(idx: u32) -> vec3<f32> {
    let e = emits[idx];
    return e.rgb * e.w;
}

// Inverse of the 3x3 linear part of an instance transform (rotation·scale).
fn inv3(m: mat4x4<f32>) -> mat3x3<f32> {
    let a = vec3<f32>(m[0].xyz);
    let b = vec3<f32>(m[1].xyz);
    let c = vec3<f32>(m[2].xyz);
    let r0 = cross(b, c);
    let r1 = cross(c, a);
    let r2 = cross(a, b);
    let det = dot(a, r0);
    let inv_det = 1.0 / select(det, 1e-8, abs(det) < 1e-12);
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
    let clip = g.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    let wp = clip.xyz / clip.w;
    let n = normalize(cross(dpdy(wp), dpdx(wp)));
    if (d >= 1.0) {
        return vec4<f32>(0.0); // sky — no indirect gather
    }

    let rays = clamp(u32(g.params.y), 1u, 16u);
    let reach = max(g.params.z, 1e-3);
    let intensity = g.params.x;
    let tube = g.params2.x > 0.5;
    let do_shadow = g.params2.y > 0.5;
    let shadow_reach = g.params2.z;
    let l_key = normalize(u.key_light.xyz);
    let dist0 = length(wp - u.camera_pos.xyz);
    let origin = wp + n * max(1e-3, dist0 * 3e-3);
    let frame = u32(g.cam_pos.w);

    var accum = vec3<f32>(0.0);
    // Hoisted OUTSIDE the loop (#195 T3 crash): a loop-local `var rq` makes
    // naga's MSL backend emit a per-iteration zero-init ASSIGNMENT to the
    // Metal intersection_query (deleted operator=). rayQueryInitialize
    // re-initializes the hoisted query — and the SAME `rq` is reused for the
    // in-loop shadow ray after the gather hit is read into locals.
    var rq: ray_query;
    for (var i = 0u; i < rays; i = i + 1u) {
        let xi = stbn2(px, frame, i);
        let dir = cosine_dir(n, xi);
        rayQueryInitialize(&rq, tlas, RayDesc(0x1u /* FORCE_OPAQUE */, 0xFFu, 0.0, reach, origin, dir));
        loop {
            if (!rayQueryProceed(&rq)) { break; }
        }
        let hit = rayQueryGetCommittedIntersection(&rq);
        if (hit.kind != RAY_QUERY_INTERSECTION_TRIANGLE) {
            continue; // miss → 0 (the receiver's own IBL ambient stands)
        }
        // Read the gather hit into locals BEFORE the shadow ray reuses `rq`.
        let idx = min(hit.instance_custom_data, arrayLength(&insts) - 1u);
        let ht = hit.t;
        let m = insts[idx];
        let ainv = inv3(m);
        let hp = origin + dir * ht;
        let local = ainv * (hp - m[3].xyz);
        var n_loc: vec3<f32>;
        var albedo_loc: vec3<f32>;
        if (tube) {
            n_loc = normalize(vec3<f32>(local.xy, 0.0));
            albedo_loc = vec3<f32>(1.0);
        } else {
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
        if (dot(hn, dir) > 0.0) {
            hn = -hn; // face the incoming ray (two-sided; cull None in raster)
        }
        let albedo = albedo_loc * tints[idx].rgb;

        // Optional traced key-shadow ray, reusing the hoisted `rq`.
        var key_vis = 1.0;
        if (do_shadow) {
            rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 0.0, shadow_reach, hp + hn * 1e-2, l_key));
            loop {
                if (!rayQueryProceed(&rq)) { break; }
            }
            let sh = rayQueryGetCommittedIntersection(&rq);
            key_vis = select(1.0, 0.0, sh.kind == RAY_QUERY_INTERSECTION_TRIANGLE);
        }
        // The neighbour's outgoing radiance toward the receiver: its glow, its own
        // per-instance emission (organon#217 T8 — the glyph ring's phosphor; `+ 0.0`
        // is exact, so the all-zero buffer is byte-identical), plus an indirect
        // fraction of its direct key light.
        let emit = albedo * u.mat.z + instance_emission(idx);
        let direct = albedo * u.key_light.w * max(dot(hn, l_key), 0.0) * key_vis;
        accum = accum + emit + GI_FRACTION * direct;
    }
    // Cosine-weighted → the average IS the one-bounce irradiance estimate.
    let result = accum / f32(rays) * intensity;
    return vec4<f32>(result, 1.0);
}

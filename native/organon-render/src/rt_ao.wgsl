// Hardware-RT ambient occlusion (#195 Tier 3). A fullscreen pass off the depth
// prepass: reconstruct each pixel's world position + geometric normal, fire
// 1–16 cosine-weighted SHORT hemisphere rays (t_max = the AO radius) against
// the Tier-0 TLAS, and write the visibility into the SAME raw-AO target GTAO
// fills — the blur, the composite AO-multiply, and the Lagarde specular
// occlusion downstream are all unchanged. Unlike GTAO (screen-space horizon
// integration) this is ground-truth short-range occlusion: no haloing, and
// off-screen geometry occludes. Per-pixel + per-frame jittered directions;
// TAA integrates the low ray count.

enable wgpu_ray_query;

struct RtAoU {
    inv_view_proj: mat4x4<f32>, // the JITTERED current VP inverse (matches the prepass)
    cam_pos: vec4<f32>,         // xyz = camera world pos, w = frame index (jitter seed)
    params: vec4<f32>,          // x = radius (world), y = ray count (1–4), zw unused
};

@group(0) @binding(0) var<uniform> u: RtAoU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
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
// golden ratio on a 64-frame cycle. Rotates each hemisphere sample: same mean,
// higher-frequency (eye-friendly) error.
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
// diffuse visibility integral, so an unweighted hit average is the estimator).
fn cosine_dir(n: vec3<f32>, xi: vec2<f32>) -> vec3<f32> {
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    let r = sqrt(xi.x);
    let a = xi.y * 6.28318530718;
    let z = sqrt(max(0.0, 1.0 - xi.x));
    return normalize(t * (r * cos(a)) + b * (r * sin(a)) + n * z);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(depth_tex));
    let px = vec2<u32>(clamp(in.pos.xy, vec2<f32>(0.0), dims - 1.0));
    let d = textureLoad(depth_tex, px, 0);
    // Reconstruct unconditionally so the derivative normal is well-defined.
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, (1.0 - in.uv.y) * 2.0 - 1.0);
    let clip = u.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    let wp = clip.xyz / clip.w;
    let n = normalize(cross(dpdy(wp), dpdx(wp)));
    if (d >= 1.0) {
        return vec4<f32>(1.0); // no geometry — fully open
    }
    let radius = max(u.params.x, 1e-3);
    let rays = clamp(u32(u.params.y), 1u, 16u);
    let dist0 = length(wp - u.cam_pos.xyz);
    let origin = wp + n * max(1e-3, dist0 * 2e-3);
    let frame = u32(u.cam_pos.w);
    var vis = 0.0;
    // Declared OUTSIDE the loop (#195 T3 crash): a loop-local `var rq` makes
    // naga's MSL backend emit a per-iteration zero-init ASSIGNMENT to the
    // Metal intersection_query — whose operator= is deleted — so the pipeline
    // failed to compile at first enable. rayQueryInitialize re-initializes
    // the hoisted query each iteration (legal WGSL; what the other RT
    // shaders' single-use pattern compiles to).
    var rq: ray_query;
    for (var i = 0u; i < rays; i = i + 1u) {
        let xi = stbn2(px, frame, i);
        let dir = cosine_dir(n, xi);
        // Short ray, FORCE_OPAQUE only (#205 review): the falloff below
        // weights by the NEAREST occluder's distance, and
        // TERMINATE_ON_FIRST_HIT commits an arbitrary (traversal-order) hit —
        // so pay for the closest-hit walk; the rays are radius-bounded and
        // cheap.
        rayQueryInitialize(&rq, tlas, RayDesc(0x1u, 0xFFu, 0.0, radius, origin, dir));
        loop {
            if (!rayQueryProceed(&rq)) { break; }
        }
        let hit = rayQueryGetCommittedIntersection(&rq);
        if (hit.kind == RAY_QUERY_INTERSECTION_TRIANGLE) {
            // Distance falloff: a graze at the radius edge barely occludes,
            // a contact hit occludes fully (matches GTAO's soft look).
            vis = vis + clamp(hit.t / radius, 0.0, 1.0);
        } else {
            vis = vis + 1.0;
        }
    }
    return vec4<f32>(vis / f32(rays), 0.0, 0.0, 1.0);
}

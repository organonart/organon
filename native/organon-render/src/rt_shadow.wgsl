// Hardware-RT shadow mask (#195 Tier 1). A fullscreen pass off the
// single-sample depth prepass: reconstruct each pixel's world position, offset
// along the derivative-reconstructed geometric normal, and fire one any-hit
// ray toward the key light (and optionally the fill light) against the Tier-0
// TLAS. Output is a screen-space visibility mask — r = key, g = fill — that
// `cube.wgsl::shadow_factor` samples instead of the PCF shadow map when the RT
// strengths (ShadowU.params2.zw) are non-zero.
//
// Softness: the ray direction is jittered inside a cone whose radius is the
// `softness` param (the light's angular size), seeded per pixel + frame — one
// ray per pixel, integrated by TAA into a real contact-hardening penumbra.

enable wgpu_ray_query;

struct RtShadowU {
    inv_view_proj: mat4x4<f32>, // the JITTERED current VP inverse (matches the prepass)
    cam_pos: vec4<f32>,         // xyz = camera world pos, w = frame index (jitter seed)
    key_dir: vec4<f32>,         // xyz = unit dir TO the key light, w = softness (0..1)
    fill_dir: vec4<f32>,        // xyz = unit dir TO the fill light, w = fill ray on (0/1)
    params: vec4<f32>,          // x = t_max, yzw unused
};

@group(0) @binding(0) var<uniform> u: RtShadowU;
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

// Spatiotemporal blue noise (#200 Tier 4½), texture-free: a per-pixel
// Interleaved-Gradient-Noise (Jimenez) spatial dither — blue-noise-like over
// the screen, so the eye, TAA and the bilateral filters resolve it far better
// than white noise — advanced each (frame, sample) by the golden-ratio
// conjugate (and its complement on the 2nd axis), a low-discrepancy temporal
// progression on a 64-frame cycle. Returns a [0,1)² pair that Cranley–Patterson-
// rotates the pass's cone/hemisphere sample: same mean, higher-frequency error.
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

// Jitter `dir` inside a cone: `soft` is the tangent of the cone half-angle
// (0 = hard shadow). `xi` is the per-pixel random pair.
fn cone_dir(dir: vec3<f32>, soft: f32, xi: vec2<f32>) -> vec3<f32> {
    if (soft <= 0.0) {
        return dir;
    }
    // Orthonormal basis around the light direction.
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(dir.y) > 0.9);
    let t = normalize(cross(up, dir));
    let b = cross(dir, t);
    let r = sqrt(xi.x) * soft * 0.25; // slider 1.0 ≈ 14° half-angle
    let a = xi.y * 6.28318530718;
    return normalize(dir + (t * cos(a) + b * sin(a)) * r);
}

// 1 = the point sees the light, 0 = occluded. Any-hit: first opaque hit ends it.
fn trace_vis(origin: vec3<f32>, dir: vec3<f32>) -> f32 {
    var rq: ray_query;
    // FORCE_OPAQUE (0x1) | TERMINATE_ON_FIRST_HIT (0x4) — a shadow ray needs
    // any hit, not the closest.
    rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 0.0, u.params.x, origin, dir));
    loop {
        if (!rayQueryProceed(&rq)) { break; }
    }
    let hit = rayQueryGetCommittedIntersection(&rq);
    return select(1.0, 0.0, hit.kind == RAY_QUERY_INTERSECTION_TRIANGLE);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(depth_tex));
    let px = vec2<u32>(clamp(in.pos.xy, vec2<f32>(0.0), dims - 1.0));
    let d = textureLoad(depth_tex, px, 0);
    // Reconstruct the world position UNCONDITIONALLY (uniform control flow), so
    // the screen derivatives below are well-defined; sky pixels early-out after.
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, (1.0 - in.uv.y) * 2.0 - 1.0);
    let clip = u.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    let wp = clip.xyz / clip.w;
    // Geometric normal of the visible surface (the #174 T1 winding: wgpu's
    // framebuffer origin is top-left, so cross(dpdy, dpdx) faces the camera).
    let n = normalize(cross(dpdy(wp), dpdx(wp)));
    if (d >= 1.0) {
        return vec4<f32>(1.0); // no geometry — fully lit
    }
    // Offset off the surface along the geometric normal to dodge self-hits; the
    // epsilon grows with distance (world-space size of a pixel grows too).
    let dist = length(wp - u.cam_pos.xyz);
    let origin = wp + n * max(1e-3, dist * 3e-3);
    let xi = stbn2(px, u32(u.cam_pos.w), 0u);
    let key = trace_vis(origin, cone_dir(u.key_dir.xyz, u.key_dir.w, xi));
    var fill = 1.0;
    if (u.fill_dir.w > 0.5) {
        // The fill ray is a decorrelated sample (index 1) of the same sequence.
        let xi2 = stbn2(px, u32(u.cam_pos.w), 1u);
        fill = trace_vis(origin, cone_dir(u.fill_dir.xyz, u.key_dir.w, xi2));
    }
    return vec4<f32>(key, fill, 1.0, 1.0);
}

// Screen-space ambient occlusion from the single-sample depth prepass.
//
// #174 T3: `fs_ao` is now **GTAO** (ground-truth ambient occlusion, Jimenez et
// al. 2016) — horizon-based visibility integration — replacing the classic
// Crysis/learnopengl hemisphere kernel. Per pixel: a few screen-space slice
// directions; along each, march both ways tracking the maximum horizon angle;
// project the surface normal into the slice plane and integrate the
// cosine-weighted visible arc analytically. Same inputs and outputs as before
// (radius/bias uniforms, R8 target, 1 = open), better quality per tap and a
// visibility integral that composes correctly with ambient light.
// `fs_blur` is the depth-aware 4×4 blur over the tiled rotation noise.

struct SsaoU {
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    params: vec4<f32>, // radius (world), intensity (unused here), bias/thickness, _
    texel: vec4<f32>,  // 1/w, 1/h, w, h
};
@group(0) @binding(0) var<uniform> u: SsaoU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var samp: sampler; // nearest, non-filtering

const PI: f32 = 3.14159265359;
const GTAO_DIRS: i32 = 2;   // slice directions per pixel (rotated by tiled noise)
const GTAO_STEPS: i32 = 6;  // marches per side per slice

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

// Reconstruct view-space position from the depth at `uv`.
fn view_pos(uv: vec2<f32>) -> vec3<f32> {
    let d = textureSampleLevel(depth_tex, samp, uv, 0);
    // wgpu: clip z is [0,1] (= d); framebuffer uv is top-left origin → flip y.
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, d, 1.0);
    let v = u.inv_proj * ndc;
    return v.xyz / v.w;
}

fn hash12(p: vec2<f32>) -> f32 {
    var h = fract(p.x * 0.1031 + p.y * 0.0973);
    h = fract(h * (h + 33.33));
    return fract(h * (h + 17.17));
}

@fragment
fn fs_ao(in: VsOut) -> @location(0) f32 {
    let d = textureSampleLevel(depth_tex, samp, in.uv, 0);
    if (d >= 1.0) { return 1.0; } // background: fully open (no AO on the sky)

    let p = view_pos(in.uv);
    // Normal from derivatives of reconstructed view position. wgpu's framebuffer
    // origin is TOP-left (dpdy points down-screen), so the winding is dpdy×dpdx —
    // the GL-sample order (dpdx×dpdy) yields a normal facing AWAY from the camera.
    let n = normalize(cross(dpdy(p), dpdx(p)));
    let v = normalize(-p); // surface → camera

    // Per-pixel slice rotation + step offset, tiled 4×4 so the companion 4×4 blur
    // (`fs_blur`) removes the pattern exactly (white noise leaves residual grain).
    let tile = vec2<f32>(f32(i32(in.pos.x) & 3), f32(i32(in.pos.y) & 3));
    let noise = hash12(tile);
    let step_jitter = hash12(tile + vec2<f32>(7.0, 13.0));

    let radius = max(u.params.x, 1e-3);
    let thickness = u.params.z; // horizon bias: shaves grazing self-occlusion
    // World radius → screen pixels at this depth (proj[1].y = focal scale).
    let px_radius = clamp(
        radius * u.proj[1].y * 0.5 * u.texel.w / max(-p.z, 1e-3),
        2.0,
        96.0,
    );

    var vis = 0.0;
    for (var i = 0; i < GTAO_DIRS; i = i + 1) {
        let phi = (noise + f32(i) / f32(GTAO_DIRS)) * PI;
        let omega = vec2<f32>(cos(phi), sin(phi)); // screen-space slice direction
        // The same direction in VIEW space (uv y runs down-screen → negate y).
        let omega_v = vec3<f32>(omega.x, -omega.y, 0.0);
        // Slice basis: plane normal + the in-plane tangent perpendicular to v.
        let plane_n = normalize(cross(omega_v, v));
        let t_dir = cross(v, plane_n); // in-slice direction, ⟂ v, ~along +omega

        // March both sides, tracking the max horizon cosine (angle from v).
        var cos_h = vec2<f32>(-1.0, -1.0); // x = +side, y = −side
        for (var s = 0; s < GTAO_STEPS; s = s + 1) {
            let t01 = (f32(s) + step_jitter) / f32(GTAO_STEPS);
            // Square distribution: denser taps near the pixel, where AO lives.
            let dist_px = max(t01 * t01 * px_radius, 1.0);
            let off = omega * dist_px * u.texel.xy;
            for (var side = 0; side < 2; side = side + 1) {
                let suv = in.uv + select(-off, off, side == 0);
                if (suv.x < 0.0 || suv.x > 1.0 || suv.y < 0.0 || suv.y > 1.0) {
                    continue;
                }
                let sp = view_pos(suv);
                let ds = sp - p;
                let dist = length(ds);
                if (dist < 1e-4 || dist > radius) {
                    continue;
                }
                // Horizon cosine, biased by `thickness` so near-coplanar geometry
                // (the surface itself) doesn't register as an occluder, and faded
                // toward "no horizon" at the radius edge.
                let cs = dot(ds / dist, v) - thickness;
                let fade = 1.0 - dist / radius;
                let attenuated = mix(-1.0, cs, fade * fade);
                if (side == 0) {
                    cos_h.x = max(cos_h.x, attenuated);
                } else {
                    cos_h.y = max(cos_h.y, attenuated);
                }
            }
        }

        // Project the normal into the slice plane; γ is its signed angle from v.
        let np = n - plane_n * dot(n, plane_n);
        let npl = length(np);
        if (npl < 1e-4) {
            continue; // normal ⟂ slice: the slice carries no visibility weight
        }
        let nd = np / npl;
        let gamma = atan2(dot(nd, t_dir), dot(nd, v));
        // Signed horizon angles about v (+side along +t_dir), clamped to the
        // hemisphere around the projected normal.
        var h1 = -acos(clamp(cos_h.y, -1.0, 1.0)); // −side
        var h2 = acos(clamp(cos_h.x, -1.0, 1.0));  // +side
        h1 = gamma + max(h1 - gamma, -0.5 * PI);
        h2 = gamma + min(h2 - gamma, 0.5 * PI);
        // Analytic cosine-weighted visible-arc integral (Jimenez et al. 2016).
        let a1 = -cos(2.0 * h1 - gamma) + cos(gamma) + 2.0 * h1 * sin(gamma);
        let a2 = -cos(2.0 * h2 - gamma) + cos(gamma) + 2.0 * h2 * sin(gamma);
        vis = vis + npl * 0.25 * (a1 + a2);
    }
    return clamp(vis / f32(GTAO_DIRS), 0.0, 1.0);
}

@group(0) @binding(3) var ao_tex: texture_2d<f32>;

// 4×4 blur to smear out the tiled rotation noise. Symmetric half-texel offsets
// (±0.5, ±1.5 — the old -2..1 window was centred half a texel up-left, shifting
// the AO image), and DEPTH-AWARE: taps across a silhouette edge are down-weighted
// so occlusion doesn't halo across depth discontinuities.
@fragment
fn fs_blur(in: VsOut) -> @location(0) f32 {
    let z0 = view_pos(in.uv).z;
    let zscale = 4.0 / max(u.params.x, 1e-3); // edge falloff relative to the AO radius
    var sum = 0.0;
    var wsum = 0.0;
    for (var x = 0; x < 4; x = x + 1) {
        for (var y = 0; y < 4; y = y + 1) {
            let off = vec2<f32>(f32(x) - 1.5, f32(y) - 1.5) * u.texel.xy;
            let suv = in.uv + off;
            let w = exp(-abs(view_pos(suv).z - z0) * zscale);
            sum = sum + textureSampleLevel(ao_tex, samp, suv, 0.0).r * w;
            wsum = wsum + w;
        }
    }
    return sum / max(wsum, 1e-4);
}

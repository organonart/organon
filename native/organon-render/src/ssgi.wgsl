// Organic Math — screen-space global illumination (#152 Tier 2).
//
// A sibling of ssr.wgsl: marches a handful of short rays in VIEW space against the
// single-sample depth prepass (shared with SSAO/SSR) and gathers the resolved HDR
// scene colour at each hit as a one-bounce DIFFUSE contribution — so a bright cube
// bleeds coloured light onto its neighbours. On a miss a ray contributes nothing.
// The composite adds this buffer (exposed) on top of the lit scene, like SSR.
//
// Coarse + noisy by design (a few rays); pairs with TAA (#152 Tier 2) to clean up
// the per-frame noise. Tune ray count / radius on the Mac.

struct SsgiU {
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    params: vec4<f32>, // intensity, radius (view units), max_steps, rays
    extra: vec4<f32>,  // thickness, frame_seed, _, _
    texel: vec4<f32>,  // 1/w, 1/h, w, h
};
@group(0) @binding(0) var<uniform> u: SsgiU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var samp_n: sampler;          // nearest (depth)
@group(0) @binding(3) var hdr_tex: texture_2d<f32>; // resolved HDR scene colour
@group(0) @binding(4) var samp_l: sampler;          // linear (colour)

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

fn view_pos(uv: vec2<f32>) -> vec3<f32> {
    let d = textureSampleLevel(depth_tex, samp_n, uv, 0);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, d, 1.0);
    let v = u.inv_proj * ndc;
    return v.xyz / v.w;
}

// Linear view-space z straight from raw depth (#174 T3) — see ssr.wgsl.
fn view_z(d: f32) -> f32 {
    return -u.proj[3].z / (d + u.proj[2].z);
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 345.45));
    q = q + dot(q, q + 34.345);
    return fract(q.x * q.y);
}

@fragment
fn fs_ssgi(in: VsOut) -> @location(0) vec4<f32> {
    let intensity = u.params.x;
    if (intensity <= 0.0) {
        return vec4<f32>(0.0);
    }
    let d = textureSampleLevel(depth_tex, samp_n, in.uv, 0);
    if (d >= 1.0) {
        return vec4<f32>(0.0); // sky — no surface to gather light onto
    }

    let p = view_pos(in.uv);                    // surface, view space (camera at origin)
    // Normal from depth derivatives. wgpu framebuffer origin is TOP-left, so the
    // correct winding is dpdy×dpdx — the GL order (dpdx×dpdy) flipped the gather
    // hemisphere INTO the surface, so every ray immediately "hit" its own pixel and
    // SSGI was a self-colour glow, not neighbour bounce.
    let n = normalize(cross(dpdy(p), dpdx(p)));
    let radius = max(u.params.y, 1e-3);
    let steps = i32(clamp(u.params.z, 1.0, 64.0));
    let rays = i32(clamp(u.params.w, 1.0, 16.0));
    let thickness = max(u.extra.x, 1e-3);
    let seed = u.extra.y;

    // Build a tangent basis to spread rays into the hemisphere about `n`.
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.95) { up = vec3<f32>(1.0, 0.0, 0.0); }
    let tangent = normalize(cross(up, n));
    let bitangent = cross(n, tangent);

    var gi = vec3<f32>(0.0);
    let base = hash21(in.uv * u.texel.zw + vec2<f32>(seed));
    for (var r = 0; r < rays; r = r + 1) {
        // Cosine-ish hemisphere direction (jittered per ray + per pixel).
        let a = (base + f32(r) / f32(rays)) * 6.2831853;
        let rr = sqrt(fract(base * 13.0 + f32(r) * 0.37));
        let dir_local = vec3<f32>(cos(a) * rr, sin(a) * rr, sqrt(max(1.0 - rr * rr, 0.0)));
        let dir = normalize(tangent * dir_local.x + bitangent * dir_local.y + n * dir_local.z);

        // Start a hair off the surface so grazing rays don't self-hit at step 1.
        let p0 = p + n * (radius * 0.05);
        var t = radius * 0.1;
        for (var i = 0; i < steps; i = i + 1) {
            let sp = p0 + dir * t;
            if (sp.z > -0.001) { break; }
            let clip = u.proj * vec4<f32>(sp, 1.0);
            if (clip.w <= 0.0) { break; }
            let ndc = clip.xy / clip.w;
            let uv2 = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            if (uv2.x < 0.0 || uv2.x > 1.0 || uv2.y < 0.0 || uv2.y > 1.0) { break; }
            let dd = textureSampleLevel(depth_tex, samp_n, uv2, 0);
            if (dd < 1.0) {
                let stored = view_z(dd);
                if (sp.z < stored && sp.z > stored - thickness) {
                    // Hit: gather the neighbour's radiance as bounced light.
                    gi = gi + textureSampleLevel(hdr_tex, samp_l, uv2, 0.0).rgb;
                    break;
                }
            }
            t = t + radius / f32(steps);
        }
    }
    gi = gi / f32(rays) * intensity;
    return vec4<f32>(gi, 1.0);
}

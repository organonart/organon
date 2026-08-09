// Inter-cube reflections — screen-space reflections (#80 Part A).
//
// Marches a reflection ray in VIEW space against the single-sample depth prepass
// (the same one SSAO uses) and, on a hit, samples the resolved HDR scene colour so
// each cube reflects its *neighbours* — what the material shader can't do (it only
// sees the IBL/env). On a miss the pass outputs 0, so the env reflection the cube
// shader already wrote stays put: no hard seam. The composite adds this buffer's
// rgb (premultiplied by a Fresnel/roughness weight) on top of the lit scene.
//
// Material is a GLOBAL uniform in this renderer (one metallic/roughness/type for
// the whole field), so the reflection weight is derived from those — no G-buffer
// needed. Roughness above the cutoff returns 0 (keeps SSR off the diffuse cubes).
// First-cut marcher (geometric step growth + a thickness band); tune on the Mac.

struct SsrU {
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    mat: vec4<f32>,   // metallic, roughness, material_type, _
    ssr: vec4<f32>,   // intensity, max_roughness, thickness, frame seed (#174 T3)
    perf: vec4<f32>,  // max_steps, stride (unused/reserved), _, _
    texel: vec4<f32>, // 1/w, 1/h, w, h
};
@group(0) @binding(0) var<uniform> u: SsrU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var samp_n: sampler;          // nearest, non-filtering (depth)
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

// Reconstruct view-space position from the depth at `uv` (matches ssao.wgsl).
fn view_pos(uv: vec2<f32>) -> vec3<f32> {
    let d = textureSampleLevel(depth_tex, samp_n, uv, 0);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, d, 1.0);
    let v = u.inv_proj * ndc;
    return v.xyz / v.w;
}

// Linear view-space z straight from raw depth (#174 T3): d is affine in 1/z for
// the RH [0,1] projection, so z = −P₃₂/(d + P₂₂) — the march only ever compared
// the z, yet paid a full mat4 inverse-projection per step per pixel.
fn view_z(d: f32) -> f32 {
    return -u.proj[3].z / (d + u.proj[2].z);
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 345.45));
    q = q + dot(q, q + 34.345);
    return fract(q.x * q.y);
}

@fragment
fn fs_ssr(in: VsOut) -> @location(0) vec4<f32> {
    let intensity = u.ssr.x;
    let max_rough = u.ssr.y;
    let roughness = u.mat.y;
    // Off / too rough → no reflection (env fallback already in the HDR buffer).
    if (intensity <= 0.0 || roughness > max_rough) {
        return vec4<f32>(0.0);
    }
    let d = textureSampleLevel(depth_tex, samp_n, in.uv, 0);
    if (d >= 1.0) {
        return vec4<f32>(0.0); // sky pixel — nothing to reflect from here
    }

    let p = view_pos(in.uv);                       // camera at origin in view space
    // Normal from depth derivatives. wgpu framebuffer origin is TOP-left, so the
    // correct winding is dpdy×dpdx (the GL order pointed the normal away from the
    // camera, which pinned the Fresnel term at maximum everywhere).
    let n = normalize(cross(dpdy(p), dpdx(p)));
    let v = normalize(p);                          // camera → surface
    var refl = reflect(v, n);                      // reflection ray (view space)
    // Stochastic roughness (#174 T3): below the cutoff SSR was mirror-sharp at
    // ANY roughness (only the intensity faded) while the IBL blurred — the two
    // never matched. Jitter the ray inside a GGX-α cone (per pixel + per frame,
    // seed = u.ssr.w) so TAA resolves the distribution into a genuinely rough
    // reflection. roughness ≈ 0 → no jitter (mirror unchanged); without TAA the
    // jitter reads as sparkle, bounded by rough_fade below.
    if (roughness > 0.005) {
        let a = roughness * roughness;
        let seed = u.ssr.w;
        let r1 = hash21(in.uv * u.texel.zw + vec2<f32>(seed, seed * 1.7));
        let r2 = hash21(in.uv.yx * u.texel.wz + vec2<f32>(seed * 3.1, seed * 0.7));
        let phi = 6.2831853 * r1;
        // GGX-distributed polar angle about the reflection axis.
        let ct = sqrt((1.0 - r2) / max(1.0 + (a * a - 1.0) * r2, 1e-6));
        let st = sqrt(max(1.0 - ct * ct, 0.0));
        var up = vec3<f32>(0.0, 1.0, 0.0);
        if (abs(refl.y) > 0.95) { up = vec3<f32>(1.0, 0.0, 0.0); }
        let t1 = normalize(cross(up, refl));
        let t2 = cross(refl, t1);
        refl = normalize(t1 * (cos(phi) * st) + t2 * (sin(phi) * st) + refl * ct);
        // Keep the jittered ray out of the surface.
        if (dot(refl, n) < 0.0) {
            refl = normalize(reflect(refl, n));
        }
    }
    // March from a hair off the surface so grazing rays don't self-hit.
    let p0 = p + n * 0.02;

    let max_steps = i32(clamp(u.perf.x, 1.0, 256.0));
    let thickness = max(u.ssr.z, 1e-3);
    var t = max(length(p) * 0.01, 0.05);           // first step scales with distance
    var t_prev = t * 0.5;
    var hit_uv = vec2<f32>(-1.0);
    var found = false;
    for (var i = 0; i < max_steps; i = i + 1) {
        let sp = p0 + refl * t;
        if (sp.z > -0.001) { break; }              // crossed behind the camera
        let clip = u.proj * vec4<f32>(sp, 1.0);
        if (clip.w <= 0.0) { break; }
        let ndc = clip.xy / clip.w;
        let uv2 = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (uv2.x < 0.0 || uv2.x > 1.0 || uv2.y < 0.0 || uv2.y > 1.0) { break; }
        let dd = textureSampleLevel(depth_tex, samp_n, uv2, 0);
        if (dd < 1.0) {
            let stored = view_z(dd);               // geometry view-z under this pixel
            // Hit: the ray has passed just behind the stored surface. The band grows
            // with the step length — a constant band lets the 1.3×-growing march step
            // clean THROUGH a cube once the step exceeds it (dropped reflections of
            // anything not adjacent), while a step-scaled band can't tunnel.
            let band = max(thickness, t - t_prev);
            if (sp.z < stored && sp.z > stored - band) {
                // Bisection refinement: the coarse geometric march quantizes hits to
                // the step positions (visible stair-step/wobble as the camera moves);
                // a few halvings between the last miss and this hit recover a stable
                // intersection point.
                var lo = t_prev;
                var hi = t;
                for (var j = 0; j < 4; j = j + 1) {
                    let mid = 0.5 * (lo + hi);
                    let mp = p0 + refl * mid;
                    let mclip = u.proj * vec4<f32>(mp, 1.0);
                    let mndc = mclip.xy / max(mclip.w, 1e-4);
                    let muv = clamp(vec2<f32>(mndc.x * 0.5 + 0.5, 0.5 - mndc.y * 0.5),
                                    vec2<f32>(0.0), vec2<f32>(1.0));
                    let md = textureSampleLevel(depth_tex, samp_n, muv, 0);
                    let mstored = view_z(md);
                    if (mp.z < mstored) { hi = mid; } else { lo = mid; }
                }
                let hp = p0 + refl * hi;
                let hclip = u.proj * vec4<f32>(hp, 1.0);
                let hndc = hclip.xy / max(hclip.w, 1e-4);
                hit_uv = clamp(vec2<f32>(hndc.x * 0.5 + 0.5, 0.5 - hndc.y * 0.5),
                               vec2<f32>(0.0), vec2<f32>(1.0));
                found = true;
                break;
            }
        }
        t_prev = t;
        t = t * 1.3 + 0.05;                        // geometric growth covers range
    }
    if (!found) {
        return vec4<f32>(0.0);                     // miss → env fallback (no seam)
    }

    let refl_color = textureSampleLevel(hdr_tex, samp_l, hit_uv, 0.0).rgb;
    // Reflectivity from the global material + a Fresnel rim. Chrome → strong mirror.
    let ndv = max(dot(n, -v), 1e-3);
    let mat_type = u.mat.z;
    var f0 = 0.04 + 0.8 * clamp(u.mat.x, 0.0, 1.0);
    if (mat_type > 0.5 && mat_type < 1.5) { f0 = 0.9; }    // Chrome
    let fres = f0 + (1.0 - f0) * pow(1.0 - ndv, 5.0);
    // Fade as roughness approaches the cutoff, and toward the screen edges, so the
    // reflection dissolves into the env instead of cutting off hard.
    let rough_fade = 1.0 - smoothstep(max_rough * 0.5, max_rough, roughness);
    let e = hit_uv * (vec2<f32>(1.0) - hit_uv) * 4.0; // 0 at edges, 1 at centre
    let edge = clamp(min(e.x, e.y), 0.0, 1.0);
    let weight = fres * rough_fade * edge * intensity;
    return vec4<f32>(refl_color * weight, weight);
}

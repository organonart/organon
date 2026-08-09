// Screen-space refraction (#214 Tier 5 part 2) — the instanced Refractive material's
// see-through of the ACTUAL scene behind it, not just the environment.
//
// Runs AFTER the scene resolves into the HDR buffer (the cube field already shaded
// with its env-only refraction) and BEFORE bloom/composite — the same seam SSR /
// liquidsurf occupy. Per pixel covered by the Refractive field:
//   1. Reconstruct the world position from the single-sample depth prepass and a
//      geometric normal from its screen derivatives (faceted, like SSR/SSGI —
//      there's no G-buffer; material is global, so every covered pixel is glass).
//   2. Refract the view ray at the live IOR, step along it, project the bent point
//      back to screen, and sample the RESOLVED SCENE (a copy) there — so a cube
//      shows its neighbours / the world behind it, DISPLACED. Off-screen / behind
//      fetches fall back to the pixel's own colour (the forward env refraction), so
//      there is no hard seam.
//   3. Beer–Lambert absorption tints the transmission by the node colour; a Fresnel
//      term keeps the surface reflection the forward pass already wrote.
// The covered pixel is replaced by `mix(scene_here, refracted·absorb, strength·(1−F))`
// — at strength 0 the pass isn't dispatched at all, so the frame is byte-identical.
//
// Approximations (documented, tune on the Mac): normal-from-depth is faceted at cube
// edges; the exit is a single fixed step (no second-interface bend); thickness for
// absorption is folded into the `absorption` dial (no measured back-depth in screen
// space — the forward pass owns the per-instance chord).

struct RsU {
    inv_vp: mat4x4<f32>,   // clip → world
    view_proj: mat4x4<f32>,// world → clip (project the refracted exit)
    cam: vec4<f32>,        // xyz = camera world pos, w unused
    ctl: vec4<f32>,        // x = ior, y = absorption, z = strength, w = displace dist
    tint: vec4<f32>,       // rgb = absorption colour (what survives), w unused
    texel: vec4<f32>,      // xy = 1/size, zw = size
};
@group(0) @binding(0) var<uniform> u: RsU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var samp_n: sampler;          // nearest (depth)
@group(0) @binding(3) var scene_tex: texture_2d<f32>; // resolved HDR scene (a copy)
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

// World position from the depth at `uv`.
fn world_pos(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let clip = u.inv_vp * vec4<f32>(ndc, d, 1.0);
    return clip.xyz / clip.w;
}

@fragment
fn fs_refract(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let d = textureSampleLevel(depth_tex, samp_n, uv, 0);
    let scene_here = textureSampleLevel(scene_tex, samp_l, uv, 0.0).rgb;
    if (d >= 1.0) {
        return vec4<f32>(scene_here, 1.0); // uncovered (sky) — untouched
    }

    let wp = world_pos(uv, d);
    // Geometric normal from depth derivatives (wgpu top-left origin → dpdy×dpdx).
    var n = normalize(cross(dpdy(wp), dpdx(wp)));
    let v = normalize(u.cam.xyz - wp); // surface → camera
    if (dot(n, v) < 0.0) { n = -n; }
    let n_dot_v = max(dot(n, v), 1e-4);

    let ior = max(u.ctl.x, 1.001);
    // Refract the view ray into the body; TIR → reflect (still leaves the surface).
    var refr = refract(-v, n, 1.0 / ior);
    if (dot(refr, refr) < 1e-6) {
        refr = reflect(-v, n);
    }
    refr = normalize(refr);

    // Step along the refracted ray and project the exit back to screen.
    let pe = wp + refr * max(u.ctl.w, 0.0);
    var refracted = scene_here; // fallback = the forward env-refraction pixel
    let clip = u.view_proj * vec4<f32>(pe, 1.0);
    if (clip.w > 0.0) {
        let sndc = clip.xyz / clip.w;
        let suv = vec2<f32>(sndc.x * 0.5 + 0.5, 0.5 - sndc.y * 0.5);
        if (suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0) {
            // Foreground rejection: refraction should only pull in geometry BEHIND the
            // glass. Reconstruct the landing pixel's world pos and keep the fallback if
            // it is NEARER to the camera than the glass surface (a foreground occluder),
            // so nearer objects aren't smeared through the refractive field. Sky
            // (d_hit ≥ 1) counts as far, so it's legitimately shown behind.
            let d_hit = textureSampleLevel(depth_tex, samp_n, suv, 0);
            let glass_dist = length(wp - u.cam.xyz);
            let hit_dist = select(length(world_pos(suv, d_hit) - u.cam.xyz), 1e9, d_hit >= 1.0);
            if (hit_dist >= glass_dist - 0.01) {
                let fetched = textureSampleLevel(scene_tex, samp_l, suv, 0.0).rgb;
                // Fade to the fallback near the screen border (no hard edge).
                let edge = min(min(suv.x, 1.0 - suv.x), min(suv.y, 1.0 - suv.y));
                refracted = mix(scene_here, fetched, smoothstep(0.0, 0.05, edge));
            }
        }
    }

    // Beer–Lambert absorption: the node colour is what survives (like Refractive).
    let sigma = (vec3<f32>(1.0) - clamp(u.tint.rgb, vec3<f32>(0.0), vec3<f32>(1.0)))
        * max(u.ctl.y, 0.0);
    let absorb = exp(-sigma);
    let body = refracted * absorb;

    // Fresnel keeps the forward reflection: only the transmitted fraction is
    // replaced by the displaced scene, weighted by the strength dial.
    let f0s = (ior - 1.0) / (ior + 1.0);
    let f0 = f0s * f0s;
    let fr = f0 + (1.0 - f0) * pow(1.0 - n_dot_v, 5.0);
    let weight = clamp(u.ctl.z, 0.0, 1.0) * (1.0 - fr);
    return vec4<f32>(mix(scene_here, body, weight), 1.0);
}

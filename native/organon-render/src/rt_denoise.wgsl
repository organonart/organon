// Edge-aware à-trous denoiser (#200 Tier 4½ part 2) for the RT reflection / GI
// buffers — the spatial half of SVGF. A joint bilateral: a 3×3 B3-spline
// kernel at an à-trous step, edge-stopped by world-position distance
// (silhouettes) and relative luminance (bright reflected highlights), so
// stochastic noise smooths WITHOUT crossing depth edges or bleeding highlights.
// Filters the full RGBA — for reflections that's PREMULTIPLIED colour (radiance
// × confidence) + the confidence in alpha. Filtering the premultiplied form is
// the CORRECT operator for the composite (`env·(1-a) + rgb`, premultiplied
// over): a low-confidence/miss tap contributes (0,0), pulling both colour and
// confidence down together so the env shows through — exactly the "over"
// result. Filtering straight (un-premultiplied) radiance would instead average
// a miss as black, which is wrong. For GI it's radiance with alpha = 1
// (constant, so moot). It blends toward the original by `strength`, applied on
// EACH à-trous step — so `amount` is a per-step blend: 0 = raw (passthrough),
// 1 = full two-step filter, continuous + monotonic between (not a single lerp).
// The renderer runs it twice (step 1 then 2) ping-ponging back into
// the source buffer, so the composite reads the same view unchanged.
//
// Scale-invariant: the position edge-stop is normalized by the camera distance
// (a silhouette gap grows with distance), so no scene-size tuning is needed.

struct DnU {
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,  // xyz = camera world pos
    params: vec4<f32>,   // texel.x, texel.y, step (à-trous), strength
    params2: vec4<f32>,  // pos_sigma (rel), lum_sigma, _, _
};
@group(0) @binding(0) var<uniform> u: DnU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var src_tex: texture_2d<f32>;

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

fn world_at(px: vec2<i32>, dims: vec2<f32>, d: f32) -> vec3<f32> {
    let uv = (vec2<f32>(px) + 0.5) / dims;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    let clip = u.inv_view_proj * vec4<f32>(ndc, d, 1.0);
    return clip.xyz / clip.w;
}

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dimsu = vec2<i32>(textureDimensions(depth_tex));
    let dims = vec2<f32>(dimsu);
    let px = vec2<i32>(clamp(in.pos.xy, vec2<f32>(0.0), dims - 1.0));
    let d0 = textureLoad(depth_tex, px, 0);
    let c0 = textureLoad(src_tex, px, 0);
    let strength = clamp(u.params.w, 0.0, 1.0);
    if (d0 >= 1.0 || strength <= 0.0) {
        return c0; // sky or off → passthrough
    }
    let wp0 = world_at(px, dims, d0);
    let l0 = lum(c0.rgb);
    let cam_dist = max(length(wp0 - u.cam_pos.xyz), 1e-3);
    let pos_sigma = max(u.params2.x, 1e-4) * cam_dist; // relative → world scale
    let lum_sigma = max(u.params2.y, 1e-4);
    let step = i32(u.params.z);

    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let tp = clamp(px + vec2<i32>(x, y) * step, vec2<i32>(0), dimsu - vec2<i32>(1));
            let dd = textureLoad(depth_tex, tp, 0);
            // B3-spline weights [1,2,1]⊗[1,2,1] = (2-|x|)(2-|y|).
            var w = f32((2 - abs(x)) * (2 - abs(y)));
            if (dd >= 1.0) {
                w = 0.0; // sky tap contributes nothing
            } else {
                let cc = textureLoad(src_tex, tp, 0);
                let wp = world_at(tp, dims, dd);
                w = w * exp(-length(wp - wp0) / pos_sigma);
                // Relative-luminance stop (HDR-safe): a highlight next to a dim
                // pixel is a large relative gap → low weight → highlight kept.
                w = w * exp(-abs(lum(cc.rgb) - l0) / (l0 + lum_sigma));
                sum = sum + cc * w;
                wsum = wsum + w;
                continue;
            }
        }
    }
    let filtered = select(c0, sum / max(wsum, 1e-4), wsum > 1e-4);
    return mix(c0, filtered, strength);
}

// Organic Math — temporal pass (#152 Tier 2): motion blur + TAA.
//
// A final pass (after the composite — composite.wgsl untouched). It reads the
// composited image (`src`), the previous resolved frame (`hist`), and the scene
// depth, and writes the resolved image to TWO targets (MRT): @location(0) = the
// view, @location(1) = the new history.
//
// Velocity is reconstructed from depth via CAMERA reprojection (current world
// position from depth → previous clip via the previous view-proj), so it captures
// camera motion (the orbit camera + beat kicks — the dominant motion here) but not
// per-object deformation (which ghosts slightly; neighbourhood clamping bounds it).
// #174 T3: the scene rasterizes with a Halton sub-pixel jitter while TAA is on
// (this pass receives UNJITTERED matrices), history clamps in YCoCg, and the
// velocity source is depth-dilated to the closest 3×3 neighbour.
//
// Order: motion blur (smear the current frame along velocity) → TAA (blend a
// neighbourhood-clamped, reprojected history toward the current). Both gated by
// their own flags; with both off + depth invalid this is a passthrough.

struct TemporalU {
    inv_cur_vp: mat4x4<f32>, // unproject current-frame depth → world
    prev_vp: mat4x4<f32>,    // reproject world → previous-frame clip
    texel: vec4<f32>,        // 1/w, 1/h, w, h
    params: vec4<f32>,       // taa_blend (current weight), taa_sharpen, mb_amount, mb_samples
    flags: vec4<f32>,        // taa_enabled, mb_enabled, depth_valid, _
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: TemporalU;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var hist_tex: texture_2d<f32>;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

fn sample_src(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

// YCoCg <-> RGB (#174 T3): clamping the history in a luma/chroma basis instead of
// RGB noticeably reduces coloured ghost fringes — the chroma axes are where
// reprojection error shows first, and an axis-aligned box in RGB is a poor fit
// for the neighbourhood's actual colour distribution.
fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b,
    );
}
fn ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let tmp = c.x - c.z;
    return vec3<f32>(tmp + c.y, c.x + c.z, tmp - c.y);
}

// Depth-dilated velocity source (#174 T3): use the CLOSEST depth in the 3×3
// neighbourhood to pick where the motion vector is computed — silhouette pixels
// then reproject with the foreground's motion instead of the background's,
// removing the classic trailing ghost fringe at object edges.
fn closest_depth_uv(uv: vec2<f32>) -> vec2<f32> {
    if (u.flags.z < 0.5) {
        return uv;
    }
    let dd = vec2<f32>(textureDimensions(depth_tex));
    var best_uv = uv;
    var best_d = 1.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let suv = uv + vec2<f32>(f32(dx), f32(dy)) * u.texel.xy;
            let px = clamp(vec2<i32>(suv * dd), vec2<i32>(0), vec2<i32>(dd) - vec2<i32>(1));
            let d = textureLoad(depth_tex, px, 0);
            if (d < best_d) {
                best_d = d;
                best_uv = suv;
            }
        }
    }
    return best_uv;
}

// Screen-space motion vector (uv units) at this pixel, from camera reprojection.
// Returns 0 when depth is unavailable or this is a sky pixel.
fn motion_vector(uv: vec2<f32>) -> vec2<f32> {
    if (u.flags.z < 0.5) {
        return vec2<f32>(0.0);
    }
    let dd = vec2<f32>(textureDimensions(depth_tex));
    let px = clamp(vec2<i32>(uv * dd), vec2<i32>(0), vec2<i32>(dd) - vec2<i32>(1));
    let d = textureLoad(depth_tex, px, 0);
    if (d >= 1.0) {
        return vec2<f32>(0.0); // sky — no parallax velocity
    }
    // Current world position from depth.
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, d, 1.0);
    let wp = u.inv_cur_vp * ndc;
    let world = wp.xyz / wp.w;
    // Reproject into the previous frame.
    let pc = u.prev_vp * vec4<f32>(world, 1.0);
    if (pc.w <= 0.0) {
        return vec2<f32>(0.0);
    }
    let pndc = pc.xy / pc.w;
    let prev_uv = vec2<f32>(pndc.x * 0.5 + 0.5, 0.5 - pndc.y * 0.5);
    return uv - prev_uv; // current − previous, in uv space
}

struct FragOut {
    @location(0) view: vec4<f32>,
    @location(1) hist: vec4<f32>,
};

@fragment
fn fs_temporal(in: VsOut) -> FragOut {
    let uv = in.uv;
    let vel = motion_vector(closest_depth_uv(uv));

    // --- Motion blur: average a few taps back along the velocity vector. ---
    // Taps are placed at (i+0.5)/n − 0.5 — symmetric about the pixel by
    // construction. (The old 1..n over i/(n−1)−0.5 always included +0.5 but never
    // −0.5, so the tap centroid was biased and enabling blur slightly TRANSLATED
    // the image along the velocity — a visible wobble when the orbit reverses.)
    var cur = sample_src(uv);
    if (u.flags.y > 0.5 && u.params.z > 0.0) {
        let samples = i32(clamp(u.params.w, 1.0, 32.0));
        let scale = u.params.z; // shutter strength
        var acc = vec3<f32>(0.0);
        for (var i = 0; i < samples; i = i + 1) {
            let t = (f32(i) + 0.5) / f32(samples) - 0.5;
            acc = acc + sample_src(uv - vel * t * scale);
        }
        cur = acc / f32(samples);
    }

    // --- TAA: blend the reprojected, neighbourhood-clamped history. ---
    var out_color = cur;
    if (u.flags.x > 0.5) {
        let t = u.texel.xy;
        // 3×3 neighbourhood min/max of the current frame, in YCoCg (#174 T3 —
        // the AABB the history is clamped into; luma/chroma clipping rejects
        // coloured ghosting far better than an RGB box).
        var nmin = rgb_to_ycocg(cur);
        var nmax = nmin;
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let s = rgb_to_ycocg(sample_src(uv + vec2<f32>(f32(dx), f32(dy)) * t));
                nmin = min(nmin, s);
                nmax = max(nmax, s);
            }
        }
        let prev_uv = uv - vel;
        var hist = textureSampleLevel(hist_tex, samp, clamp(prev_uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
        hist = max(ycocg_to_rgb(clamp(rgb_to_ycocg(hist), nmin, nmax)), vec3<f32>(0.0));
        // Drop history that reprojected off-screen (first frame / disocclusion).
        let on = select(0.0, 1.0, prev_uv.x >= 0.0 && prev_uv.x <= 1.0 && prev_uv.y >= 0.0 && prev_uv.y <= 1.0);
        let blend = mix(1.0, clamp(u.params.x, 0.05, 1.0), on); // off-screen → take current
        out_color = mix(hist, cur, blend);
        // Light sharpen to counter TAA softening.
        let sharpen = u.params.y;
        if (sharpen > 0.0) {
            let blur = (sample_src(uv + vec2<f32>(t.x, 0.0)) + sample_src(uv - vec2<f32>(t.x, 0.0))
                + sample_src(uv + vec2<f32>(0.0, t.y)) + sample_src(uv - vec2<f32>(0.0, t.y))) * 0.25;
            out_color = out_color + (out_color - blur) * sharpen;
        }
    }

    out_color = max(out_color, vec3<f32>(0.0));
    var o: FragOut;
    o.view = vec4<f32>(out_color, 1.0);
    o.hist = vec4<f32>(out_color, 1.0);
    return o;
}

// Beat-aware temporal accumulator (#200 Tier 4½ parts 3 + 4) for the RT
// reflection / GI buffers — the temporal half of SVGF, complementing part 2's
// spatial à-trous. Each frame it reprojects the previous accumulated history by
// CAMERA motion (current world pos → previous clip) and blends it with this
// frame's noisy RT result via an exponential moving average, so the 1-Nspp
// grain integrates toward the true value over time.
//
// Robust to the animating field two ways: (1) a 3×3 neighborhood clamp rejects
// stale history where content changed (a cube moved into this pixel) — the
// standard TAA/SVGF trick, since camera-only reprojection can't track each
// cube's own motion; (2) **beat relaxation** — the caller folds the PLL beat
// pulse into `beat_relax`, which drops the history weight on each beat (when the
// auto-orbit camera kicks and the field pulses hardest), so history relaxes
// smoothly into the fresh frame instead of smearing across the kick.
// Disocclusion (reprojection off-screen) falls back to the current frame.
//
// **Part 4 — variance-guided (true SVGF).** When `variance_on`, two upgrades
// over part 3's fixed feedback + raw min/max box clamp:
//   • **History-length-adaptive blend** — a per-pixel accumulated-sample count
//     `n` (stored in the moments texture) drives the history weight
//     `hw = min(feedback, 1 − 1/n)`: a fresh / disoccluded pixel converges fast
//     (early frames weight the current frame heavily) then asymptotes to the
//     user's feedback ceiling. Beat relaxation still multiplies on top.
//   • **Luminance-variance clamp** — integrated luminance moments (μ₁, μ₂) give
//     the temporal variance σ² = μ₂ − μ₁²; for short history it blends in the
//     3×3 spatial variance (SVGF's cold-start fallback). History luma is clamped
//     to μ ± γ·σ instead of the raw neighborhood min/max, so a single firefly no
//     longer swells the box — a statistically grounded reject that keeps edges.
// `variance_on = 0` reproduces part 3 exactly (fixed feedback + box clamp).

struct TmpU {
    inv_view_proj: mat4x4<f32>,  // current (unjittered) VP⁻¹
    prev_view_proj: mat4x4<f32>, // last frame's VP (for reprojection)
    params: vec4<f32>,           // feedback (history weight), beat_relax, _, _
    params2: vec4<f32>,          // variance_on, max_accum, clamp_gamma, history_valid
};
@group(0) @binding(0) var<uniform> u: TmpU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var cur_tex: texture_2d<f32>;  // this frame's raw RT buffer
@group(0) @binding(3) var hist_tex: texture_2d<f32>; // last frame's accumulation
@group(0) @binding(4) var samp: sampler;             // linear (history reprojection)
@group(0) @binding(5) var mom_tex: texture_2d<f32>;  // last frame's moments (μ1, μ2, n, σ²)

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

// Three outputs: loc0 → the reflection/GI buffer the composite reads, loc1 →
// the new colour history (ping-ponged so this frame's read history is a
// different texture from the write), loc2 → the new moments (μ1, μ2, n, σ²).
// MRT avoids any read/write aliasing and any texture copy — the RT pass wrote a
// separate raw buffer (`cur_tex`).
struct Out {
    @location(0) buf: vec4<f32>,
    @location(1) hist: vec4<f32>,
    @location(2) mom: vec4<f32>,
};

fn luma(c: vec3<f32>) -> f32 {
    // Cap before use so a firefly's luma² can't overflow the 16-bit-float
    // moments texture (and shouldn't dominate the variance anyway).
    return min(dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)), 64.0);
}

@fragment
fn fs_main(in: VsOut) -> Out {
    let dimsu = vec2<i32>(textureDimensions(depth_tex));
    let dims = vec2<f32>(dimsu);
    let px = vec2<i32>(clamp(in.pos.xy, vec2<f32>(0.0), dims - 1.0));
    let cur = textureLoad(cur_tex, px, 0);
    let d = textureLoad(depth_tex, px, 0);

    let variance_on = u.params2.x > 0.5;
    let valid_hist = u.params2.w > 0.5;
    let l_cur = luma(cur.rgb);

    var result = cur;
    var mom_out = vec4<f32>(l_cur, l_cur * l_cur, 1.0, 0.0); // fresh-pixel seed
    // Everything below only refines `result`; sky / disocclusion keep `cur`.
    if (d < 1.0) {
        // Current world position, then reproject into the previous frame's clip.
        let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, (1.0 - in.uv.y) * 2.0 - 1.0);
        let clip = u.inv_view_proj * vec4<f32>(ndc, d, 1.0);
        let wp = clip.xyz / clip.w;
        let pclip = u.prev_view_proj * vec4<f32>(wp, 1.0);
        let pndc = pclip.xy / pclip.w;
        let puv = vec2<f32>(pndc.x * 0.5 + 0.5, 0.5 - pndc.y * 0.5);
        let in_bounds = pclip.w > 0.0
            && all(puv >= vec2<f32>(0.0)) && all(puv <= vec2<f32>(1.0));
        if (in_bounds && valid_hist) {
            var hist = textureSampleLevel(hist_tex, samp, puv, 0.0);
            // 3×3 neighborhood: colour min/max box (outer safety bound) AND the
            // spatial luminance moments (SVGF's short-history variance fallback).
            var mn = cur;
            var mx = cur;
            var s1 = 0.0;
            var s2 = 0.0;
            for (var y = -1; y <= 1; y = y + 1) {
                for (var x = -1; x <= 1; x = x + 1) {
                    let tp = clamp(px + vec2<i32>(x, y), vec2<i32>(0), dimsu - vec2<i32>(1));
                    let c = textureLoad(cur_tex, tp, 0);
                    mn = min(mn, c);
                    mx = max(mx, c);
                    let cl = luma(c.rgb);
                    s1 = s1 + cl;
                    s2 = s2 + cl * cl;
                }
            }
            let spatial_mu = s1 / 9.0;
            let spatial_var = max(s2 / 9.0 - spatial_mu * spatial_mu, 0.0);

            if (variance_on) {
                // NEAREST fetch (textureLoad), not the linear `samp`: the moments
                // carry the accumulated sample count in .z, which must stay a
                // discrete integer — a bilinear tap would make `n` fractional and
                // corrupt the `1 − 1/n` adaptive weight (#215 review).
                let mp = clamp(vec2<i32>(puv * dims), vec2<i32>(0), dimsu - vec2<i32>(1));
                let mom = textureLoad(mom_tex, mp, 0);
                // Accumulated sample count → adaptive history weight: fresh
                // pixels converge fast (hw≈0), settling toward `feedback`.
                let n = min(mom.z + 1.0, max(u.params2.y, 1.0));
                let ceiling = clamp(u.params.x, 0.0, 0.98);
                var hw = min(ceiling, 1.0 - 1.0 / n);
                hw = clamp(hw * (1.0 - clamp(u.params.y, 0.0, 1.0)), 0.0, 0.98);
                // Variance: temporal once history is long, spatial when cold.
                let var_t = max(mom.y - mom.x * mom.x, 0.0);
                let blend = clamp(n / 4.0, 0.0, 1.0);
                let sigma = sqrt(mix(spatial_var, var_t, blend));
                let gamma = max(u.params2.z, 0.0);
                // Clamp history LUMA to μ ± γσ, rescale colour to preserve
                // chroma + the (reflection) confidence alpha, then a hard box.
                let hl = luma(hist.rgb);
                let cl = clamp(hl, spatial_mu - gamma * sigma, spatial_mu + gamma * sigma);
                let scale = select(1.0, cl / max(hl, 1e-4), hl > 1e-4);
                hist = vec4<f32>(clamp(hist.rgb * scale, mn.rgb, mx.rgb), hist.a);
                result = mix(cur, hist, hw);
                // Integrate the moments with the same weight, so σ tracks the
                // accumulated signal rather than the raw single frame.
                let mu1 = mix(l_cur, mom.x, hw);
                let mu2 = mix(l_cur * l_cur, mom.y, hw);
                // Relax the STORED sample count by the beat kick too (#215
                // review): a kick drops `hw` so this frame follows the fresh
                // frame, but if `n` kept climbing the NEXT frames would re-grow
                // the adaptive weight and smear history back across the beat.
                // Relaxing n → the pixel re-converges fresh after a kick, like a
                // disocclusion (full kick → n = 1, no kick → n unchanged).
                let n_out = mix(n, 1.0, clamp(u.params.y, 0.0, 1.0));
                mom_out = vec4<f32>(mu1, mu2, n_out, max(mu2 - mu1 * mu1, 0.0));
            } else {
                // Part 3: fixed feedback + raw min/max box clamp.
                hist = clamp(hist, mn, mx);
                let hw = clamp(u.params.x * (1.0 - clamp(u.params.y, 0.0, 1.0)), 0.0, 0.98);
                result = mix(cur, hist, hw);
            }
        }
    }
    var o: Out;
    o.buf = result;
    o.hist = result;
    o.mom = mom_out;
    return o;
}

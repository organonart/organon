// Scene Kaleidoscope (#361 Tier 1) — a post-stage kaleidoscopic fold applied to
// the LIVE, physically-lit generator render instead of a procedural fractal field.
//
// The scene has already resolved into the linear HDR buffer (PBR + IBL + bloom-to-
// come + beat motion). This pass reads a snapshot of that buffer (`src_tex`) and,
// for every output pixel, folds its screen coordinate through N-fold kaleidoscopic
// symmetry and samples the scene there — so the reflected shards are real, moving,
// lit geometry. It runs BEFORE the bloom/tonemap composite, in HDR-linear, so
// highlights and the EDR headroom stay physical.
//
// Two mirror modes (`mode`):
//   0 FullFrame — each pie-slice shows the WHOLE frame squished + mirror-tiled
//                 (the angle in the wedge is stretched to a half-turn of source),
//                 so adjacent slices reflect different real geometry — swimmy.
//   1 Wedge     — the classic optical kaleidoscope: every slice samples the SAME
//                 thin source wedge, so all sectors are identical mirror images.
// Clean-room from the kaleidoscope fold maths (no external shader copied).

struct KaleidoU {
    p0: vec4<f32>, // aspect, sectors, mode(0 FullFrame / 1 Wedge), angle (spin·t + roll)
    p1: vec4<f32>, // zoom, center_x, center_y, mix (0 = scene … 1 = folded)
    p2: vec4<f32>, // twist (log-polar spiral), tint_hue (deg), tint_amt, seam (0..1)
    p3: vec4<f32>, // texel_x, texel_y, _, _
};
@group(0) @binding(0) var<uniform> u: KaleidoU;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;

fn hue2rgb(h: f32) -> vec3<f32> {
    // Fully-saturated hue (0..1) → RGB, via the standard 6-segment ramp.
    let r = abs(h * 6.0 - 3.0) - 1.0;
    let g = 2.0 - abs(h * 6.0 - 2.0);
    let b = 2.0 - abs(h * 6.0 - 4.0);
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn scene_at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

// Map an output UV (0..1, matching the composite's flipped fullscreen convention)
// to the source UV the kaleidoscope samples.
fn fold_uv(uv: vec2<f32>) -> vec2<f32> {
    let aspect = u.p0.x;
    let sectors = max(u.p0.y, 1.0);
    let mode = u.p0.z;
    let angle = u.p0.w;
    let zoom = max(u.p1.x, 1e-3);
    let center = vec2<f32>(u.p1.y, u.p1.z);
    let twist = u.p2.x;

    // Centred, aspect-corrected coordinate about the screen middle.
    var c = uv * 2.0 - vec2<f32>(1.0);
    c.x = c.x * aspect;
    let r = length(c);

    // Field rotation (spin/beat) + optional log-polar spiral twist (bounded near 0).
    var a = atan2(c.y, c.x) + angle + twist * log(r + 0.05);

    // Fold the angle into one 2π/sectors wedge, mirrored at the seams → [0, k/2].
    let k = TAU / sectors;
    a = a - floor(a / k) * k;   // [0, k)
    a = abs(a - 0.5 * k);       // mirror within the wedge → [0, k/2]

    // FullFrame stretches the wedge angle to a half-turn (each slice = the whole
    // frame, mirror-tiled); Wedge keeps the thin sliver (classic, identical slices).
    var a_src = a;
    if (mode < 0.5) {
        a_src = a * sectors;
    }

    var srcp = vec2<f32>(cos(a_src), sin(a_src)) * r;
    // Source framing: zoom into (and offset toward) the busy part of the frame.
    srcp = srcp / zoom + center;
    srcp.x = srcp.x / aspect;
    return srcp * 0.5 + vec2<f32>(0.5);
}

// A folded sample, sampling the source at the folded coordinate for a jittered
// output position (the jitter supersamples the high-frequency mirror seams).
fn folded_at(uv: vec2<f32>) -> vec3<f32> {
    return scene_at(fold_uv(uv));
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_kaleido(@builtin(vertex_index) vid: u32) -> VsOut {
    // Fullscreen triangle; UV convention matches composite.wgsl (v flipped) so the
    // identity sample (mix = 0) reproduces the scene upright.
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

@fragment
fn fs_kaleido(in: VsOut) -> @location(0) vec4<f32> {
    let base = in.uv;

    // 4-tap rotated-grid supersample of the fold. The offset scales with `seam`, so
    // seam = 0 collapses all taps onto the primary sample (single-tap, sharp) and
    // higher values soften the mirror-line aliasing the fold introduces.
    let off = max(u.p2.w, 0.0) * u.p3.xy * 0.75;
    var col = folded_at(base + vec2<f32>( off.x,  off.y))
            + folded_at(base + vec2<f32>(-off.x,  off.y))
            + folded_at(base + vec2<f32>( off.x, -off.y))
            + folded_at(base + vec2<f32>(-off.x, -off.y));
    col = col * 0.25;

    // Optional hue grade on the folded scene (the palette/hue analogue of the KIFS
    // field). amt = 0 → no change; energy-preserving multiply toward the tint hue.
    let amt = clamp(u.p2.z, 0.0, 1.0);
    if (amt > 0.0) {
        let tint = hue2rgb(fract(u.p2.y / 360.0));
        col = col * mix(vec3<f32>(1.0), tint, amt);
    }

    // Crossfade against the untouched scene so the effect can be dialled in.
    let scene = scene_at(base);
    col = mix(scene, col, clamp(u.p1.w, 0.0, 1.0));

    return vec4<f32>(col, 1.0);
}

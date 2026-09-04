// Organic Math — post-composite creative FX (#152, Tier 1).
//
// Runs AFTER the HDR composite (composite.wgsl is left untouched — this never
// touches the precious HDR/EDR/gamut path). It reads the composited image
// (`src`), applies a stack of screen-space effects, and writes the result to TWO
// targets via MRT: @location(0) = the final view (swapchain / production texture)
// and @location(1) = a history texture sampled next frame for feedback trails.
//
// The whole pass only runs when the editor's "Post FX" master is on; with every
// amount at 0 and style = None it is a near-passthrough, so the default look is
// unchanged. Effects (in order): pixelate → DoF → chromatic aberration → NPR
// style → colour grade → vignette → film grain → feedback trail.
//
// DoF uses the scene depth (textureLoad, no sampler) — a raw-depth artistic
// focus, gated to the instanced/node-field path where a single-sample depth
// prepass exists (a 1×1 dummy depth is bound otherwise, with dof_enabled = 0).

struct FxU {
    // style (0 none / 1 toon / 2 outline / 3 halftone / 4 dither / 5 pixelate),
    // style_amt, dof_amount, dof_focus (0..1 raw depth)
    p0: vec4<f32>,
    // dof_range, chroma, vignette, grain
    p1: vec4<f32>,
    // grade_sat (1 neutral), grade_contrast (1 neutral), grade_temp (0 neutral),
    // grade_gain (1 neutral)
    p2: vec4<f32>,
    // feedback (0..0.95), outline_thresh, time (seconds), dof_enabled (0/1)
    p3: vec4<f32>,
    // 1/w, 1/h, w, h
    texel: vec4<f32>,
    // #167 T1 halation: amount, threshold, width, warmth
    p4: vec4<f32>,
    // #167 T1 lens flare: amount, ghosts, halo, streak
    p5: vec4<f32>,
    // #167 T1 lens flare anchor: light screen pos (uv).xy, visibility, _
    p6: vec4<f32>,
    // camera projection near / far (for the DoF focus-distance remap), _, _
    p7: vec4<f32>,
    // organon#217 T15 the scatter: amount, max streak length in PIXELS (the §9 bound,
    // already converted from cell widths CPU-side), RGB split, primed (0/1 — whether
    // last frame's pass wrote a luminance reference into the history's alpha lane)
    p8: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> u: FxU;
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

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Cheap hash → [0,1), for film grain. Stable per (pixel, time-quantum).
fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 345.45));
    q = q + dot(q, q + 34.345);
    return fract(q.x * q.y);
}

fn sample_src(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

// Bayer 4×4 ordered-dither threshold in [0,1).
fn bayer4(p: vec2<i32>) -> f32 {
    let m = array<f32, 16>(
        0.0,  8.0,  2.0, 10.0,
        12.0, 4.0, 14.0,  6.0,
        3.0, 11.0,  1.0,  9.0,
        15.0, 7.0, 13.0,  5.0,
    );
    let x = p.x & 3;
    let y = p.y & 3;
    return m[y * 4 + x] / 16.0;
}

// Depth-of-field: a small disc blur whose radius grows with the raw-depth
// distance from the focus plane. Raw depth is affine in 1/z, so |d − d_focus| is
// PROPORTIONAL to the physical CoC |1/z − 1/z_focus| — the shape was always right.
// What was wrong was the FOCUS SLIDER: it picked a raw-depth value directly, and
// with near 0.1 / far 5000 the far ~90% of the world compresses into the last few
// thousandths of raw depth, so almost all slider travel sat uselessly at one end.
// The slider now picks a world DISTANCE on an exponential near→far ramp (equal
// slider travel ≈ equal multiplicative distance change), converted to raw depth.
// Off when dof_enabled = 0.
fn dof(uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p0.z;
    if (u.p3.w < 0.5 || amount <= 0.0) {
        return sample_src(uv);
    }
    // The depth texture may be a different resolution than this pass (the composite
    // upscales the scaled render buffer), so address it in its OWN dimensions.
    let dd = vec2<f32>(textureDimensions(depth_tex));
    let px = vec2<i32>(uv * dd);
    let d = textureLoad(depth_tex, clamp(px, vec2<i32>(0), vec2<i32>(dd) - vec2<i32>(1)), 0);
    let range = max(u.p1.x, 1e-4);
    // Focus slider (0..1) → world distance → the focus plane's raw depth.
    let near = max(u.p7.x, 1e-3);
    let far = max(u.p7.y, near * 2.0);
    let zf = near * pow(far / near, clamp(u.p0.w, 0.0, 1.0));
    let df = (far / (far - near)) * (1.0 - near / zf);
    let coc = clamp(abs(d - df) / range, 0.0, 1.0) * amount;
    let r = coc * 6.0; // max blur radius, in pixels
    if (r < 0.5) {
        return sample_src(uv);
    }
    // 12-tap ring + centre. Each tap is weighted by its OWN CoC when it lies nearer
    // than this pixel: an in-focus foreground tap otherwise bleeds its sharp
    // silhouette into the blurred background behind it.
    var acc = sample_src(uv);
    var wsum = 1.0;
    let TAU = 6.2831853;
    for (var i = 0; i < 12; i = i + 1) {
        let a = TAU * f32(i) / 12.0;
        let off = vec2<f32>(cos(a), sin(a)) * r * u.texel.xy;
        let suv = clamp(uv + off, vec2<f32>(0.0), vec2<f32>(1.0));
        let tpx = clamp(vec2<i32>(suv * dd), vec2<i32>(0), vec2<i32>(dd) - vec2<i32>(1));
        let td = textureLoad(depth_tex, tpx, 0);
        let tcoc = clamp(abs(td - df) / range, 0.0, 1.0) * amount;
        let w = select(1.0, clamp(tcoc / max(coc, 1e-4), 0.0, 1.0), td < d);
        acc = acc + sample_src(suv) * w;
        wsum = wsum + w;
    }
    return acc / wsum;
}

// NPR style stack. `base` is the (DoF'd, aberrated) colour at this pixel.
fn apply_style(base: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    let style = u.p0.x;
    let amt = u.p0.y;
    if (style < 0.5) {
        return base; // None
    } else if (style < 1.5) {
        // Toon / posterize: quantise luminance into `amt` bands, keep chroma.
        let bands = max(round(amt), 2.0);
        let l = luma(base);
        let q = floor(l * bands + 0.5) / bands;
        return base * (q / max(l, 1e-3));
    } else if (style < 2.5) {
        // Outline: luminance Sobel over a 3×3 neighbourhood → dark ink edges.
        // Sample through dof() (not the raw source) so the edges are derived from the
        // same DoF-processed image that's displayed — otherwise, with DoF on, sharp
        // raw-source edges wouldn't line up with the blurred picture. (dof() is a
        // single raw tap when DoF is off, so this is free in the common case.)
        let t = u.texel.xy;
        var gx = vec3<f32>(0.0);
        var gy = vec3<f32>(0.0);
        let l00 = luma(dof(uv + vec2<f32>(-t.x, -t.y)));
        let l10 = luma(dof(uv + vec2<f32>(0.0, -t.y)));
        let l20 = luma(dof(uv + vec2<f32>(t.x, -t.y)));
        let l01 = luma(dof(uv + vec2<f32>(-t.x, 0.0)));
        let l21 = luma(dof(uv + vec2<f32>(t.x, 0.0)));
        let l02 = luma(dof(uv + vec2<f32>(-t.x, t.y)));
        let l12 = luma(dof(uv + vec2<f32>(0.0, t.y)));
        let l22 = luma(dof(uv + vec2<f32>(t.x, t.y)));
        let sx = (l20 + 2.0 * l21 + l22) - (l00 + 2.0 * l01 + l02);
        let sy = (l02 + 2.0 * l12 + l22) - (l00 + 2.0 * l10 + l20);
        let g = sqrt(sx * sx + sy * sy);
        let edge = smoothstep(u.p3.y, u.p3.y + 0.3, g) * clamp(amt, 0.0, 1.0);
        return base * (1.0 - edge);
    } else if (style < 3.5) {
        // Halftone: a rotated dot screen modulating darkness by luminance.
        let cell = max(amt, 2.0);
        let grid = uv * u.texel.zw / cell;
        let c = fract(grid) - 0.5;
        let dist = length(c);
        let l = luma(base);
        let dot_r = sqrt(clamp(1.0 - l, 0.0, 1.0)) * 0.5;
        let ink = smoothstep(dot_r, dot_r + 0.1, dist); // 1 outside the dot
        return base * mix(1.0, ink, 0.85);
    } else if (style < 4.5) {
        // Ordered dither: posterise to few levels with a Bayer threshold.
        let levels = max(round(amt), 2.0);
        let px = vec2<i32>(uv * u.texel.zw);
        let thr = (bayer4(px) - 0.5) / levels;
        return floor((base + thr) * levels + 0.5) / levels;
    }
    return base; // Pixelate handled at the uv stage
}

// Lift/gamma/gain-lite colour grade: saturation, contrast (about 0.18 grey),
// temperature (warm/cool), and a gain multiplier. All neutral at their defaults.
fn grade(c_in: vec3<f32>) -> vec3<f32> {
    var c = max(c_in, vec3<f32>(0.0));
    // Temperature: push R up / B down (warm) or the reverse (cool).
    let temp = u.p2.z;
    c = c * vec3<f32>(1.0 + temp * 0.2, 1.0, 1.0 - temp * 0.2);
    // Gain.
    c = c * max(u.p2.w, 0.0);
    // Saturation about luminance.
    let l = luma(c);
    c = mix(vec3<f32>(l), c, max(u.p2.x, 0.0));
    // Contrast about mid-grey.
    c = mix(vec3<f32>(0.18), c, max(u.p2.y, 0.0));
    return max(c, vec3<f32>(0.0));
}

// Halation (#167 T1): the red-sensitive film layer scattering light back through the
// emulsion AROUND bright highlights. Distinct from bloom in two physical ways this
// models: (1) it's a WIDE, SOFT radial bleed — a smooth Gaussian-ish falloff over
// several rings, not a bright core or a hard annulus; and (2) it's CHROMATIC — red
// penetrates deepest and reflects off the film backing, so it scatters FURTHEST,
// while green/blue stay tight. That red-fringed outer halo is what reads as "shot on
// film" rather than "glowing." Each channel is emphasised at a different radius (red
// toward the far taps, blue toward the near) from one bright-passed gather.
// amount 0 → returns 0 (no change); `warmth` widens the red↔blue separation.
fn halation(uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p4.x;
    if (amount <= 0.0) {
        return vec3<f32>(0.0);
    }
    let threshold = u.p4.y;
    let width = max(u.p4.z, 1e-3);
    let warmth = clamp(u.p4.w, 0.0, 1.0);
    let TAU = 6.2831853;
    let max_rad = width * 0.09;
    // Per-channel radial centres: red peaks at the outer ring (widest scatter), blue
    // at the inner (tightest). warmth spreads them apart; at 0 they collapse to a
    // neutral wide blur. Aspect-corrected so the halo is round, not oval.
    let aspect = u.texel.z / max(u.texel.w, 1.0);
    let asp = vec2<f32>(1.0, aspect);
    let cr = 1.0;
    let cg = mix(0.85, 0.55, warmth);
    let cb = mix(0.7, 0.18, warmth);
    let RINGS = 4;
    let TAPS = 12;
    var acc = vec3<f32>(0.0);
    var wsum = vec3<f32>(0.0);
    for (var ring = 1; ring <= RINGS; ring = ring + 1) {
        let t = f32(ring) / f32(RINGS); // 0..1 along the radius
        // Golden-angle stagger per ring so taps don't line up into spokes.
        let a0 = 2.399963 * f32(ring);
        for (var i = 0; i < TAPS; i = i + 1) {
            let a = a0 + TAU * f32(i) / f32(TAPS);
            let dir = vec2<f32>(cos(a), sin(a)) * asp;
            let s = sample_src(uv + dir * max_rad * t);
            let b = max(luma(s) - threshold, 0.0); // soft bright-pass weight
            // Per-channel Gaussian weight over radius → chromatic spread.
            let dr = t - cr;
            let dg = t - cg;
            let db = t - cb;
            let w = vec3<f32>(
                exp(-dr * dr * 3.5),
                exp(-dg * dg * 3.5),
                exp(-db * db * 3.5),
            );
            acc = acc + s * b * w;
            wsum = wsum + w;
        }
    }
    let h = acc / max(wsum, vec3<f32>(1e-3));
    // Final warm bias (the classic orange-red halation cast).
    let tint = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.45, 0.22), warmth);
    return h * tint * amount;
}

// A soft radial sprite (disc) centred at p with radius r, in aspect-corrected space.
fn flare_disc(uv: vec2<f32>, p: vec2<f32>, r: f32) -> f32 {
    let d = distance(uv, p);
    return pow(clamp(1.0 - d / r, 0.0, 1.0), 2.0);
}

// Lens flares (#167 T1), anchored to the KEY LIGHT'S screen position (`u.p6.xy`),
// not to the bright pixels of the image. `u.p6.z` is the light's visibility (0 when
// it's off-frame / behind the camera, scaled by key intensity). Ghosts march as
// discrete chromatic sprites along the light→centre optical axis (both sides of
// centre); a halo rings the optical centre; an anamorphic streak runs horizontally
// through the light. So the whole flare is a function of camera-view × light-
// direction — it sweeps across the frame as the camera orbits, and vanishes when
// you look away from the light. amount 0 or vis 0 → returns 0 (free when unused).
fn lensflare(uv: vec2<f32>) -> vec3<f32> {
    let amount = u.p5.x;
    let vis = u.p6.z;
    if (amount <= 0.0 || vis <= 0.0) {
        return vec3<f32>(0.0);
    }
    let lpos = u.p6.xy;              // key light's screen position (uv)
    let center = vec2<f32>(0.5);
    let axis = center - lpos;        // the optical axis on screen (light → centre)
    // Aspect-correct so discs/rings are round and distances read in screen space.
    let aspect = u.texel.z / max(u.texel.w, 1.0);
    let asp = vec2<f32>(aspect, 1.0);
    var acc = vec3<f32>(0.0);

    // Ghosts: discrete chromatic sprites at fixed scales along the axis (a scale of
    // 0 sits on the light, 1 on the centre, >1 past it to the far side).
    let ghosts = u.p5.y;
    if (ghosts > 0.0) {
        var scales = array<f32, 6>(-0.35, 0.25, 0.5, 0.75, 1.15, 1.45);
        var radii = array<f32, 6>(0.11, 0.05, 0.08, 0.04, 0.15, 0.07);
        let ca = axis * 0.015;       // chromatic fringe: split RGB along the axis
        for (var i = 0; i < 6; i = i + 1) {
            let p = lpos + axis * scales[i];
            let r = flare_disc(uv * asp, (p + ca) * asp, radii[i]);
            let g = flare_disc(uv * asp, p * asp, radii[i]);
            let b = flare_disc(uv * asp, (p - ca) * asp, radii[i]);
            // Fade ghosts that stray toward the frame corners.
            let ef = clamp(1.0 - length(p - center) / 0.9, 0.0, 1.0);
            acc = acc + vec3<f32>(r, g, b) * ef;
        }
        acc = acc * ghosts;
    }

    // Halo: a chromatic ring around the optical centre.
    let halo = u.p5.z;
    if (halo > 0.0) {
        let hr = 0.28;
        let d = distance(uv * asp, center * asp);
        let ring = pow(clamp(1.0 - abs(d - hr) / 0.10, 0.0, 1.0), 2.0);
        let t = clamp((d - (hr - 0.10)) / 0.20, 0.0, 1.0);
        let tint = mix(vec3<f32>(0.4, 0.6, 1.0), vec3<f32>(1.0, 0.6, 0.4), t);
        acc = acc + ring * tint * halo;
    }

    // Anamorphic streak: a horizontal blue-tinted smear through the light position.
    let streak = u.p5.w;
    if (streak > 0.0) {
        let dy = abs(uv.y - lpos.y);
        let dx = abs(uv.x - lpos.x);
        let s = pow(clamp(1.0 - dy / 0.012, 0.0, 1.0), 2.0)
              * clamp(1.0 - dx / 0.5, 0.0, 1.0);
        acc = acc + s * vec3<f32>(0.55, 0.72, 1.0) * streak;
    }

    return acc * vis * amount;
}

// ─────────────────────────────────────────────────────────────────────────────
// organon#217 T15 — the scatter: velocity-keyed motion streaks with an RGB split.
//
// WHERE THE VELOCITY COMES FROM. Not from the camera. `temporal.rs` reconstructs
// velocity by reprojecting the previous camera, which describes how the WORLD moved
// under a moving eye — and a glyph does not move that way: it teleports cell to cell
// while the camera is held still (§8's held camera is the whole point of T3/T5). So the
// flow is MEASURED from the image, by the standard normal-flow relation
//
//     dI/dt + v·grad(I) = 0   =>   v = -(dI/dt) * grad(I) / |grad(I)|^2
//
// which is the only component of v an image can give you (the aperture problem): the
// part along the local gradient, i.e. perpendicular to the edge that moved. That is
// exactly the direction a streak should run — a smeared edge smears across itself.
//
// WHERE THE PREVIOUS FRAME COMES FROM. `hist_tex` already holds last frame's output for
// the feedback trail, and the trail reads `.rgb` only — so its ALPHA has been a lane
// written as the literal 1.0 and read by nothing since #152. The scatter stores last
// frame's `luma(base)` there. No new attachment, no new texture, no second pipeline, and
// with the scatter off the alpha is written 1.0 exactly as before, so the feedback path
// and the presented image are both untouched byte for byte.
//
// WHY IT DOES NOT COMPOUND. The reference is the picture BEFORE the streak is mixed in,
// so this frame's streak never feeds the next frame's velocity estimate. A reference
// taken after the mix would see its own smear as motion and grow it every frame.
//
// §9 LAW 1 — "the cell's energy stays in the cell". A streak moves energy out of a cell
// by definition, so it is bounded twice. (1) The reach is expressed in CELL WIDTHS with
// a range that stops at one cell, converted to pixels CPU-side against the live grid's
// measured on-screen cell width (`fx.rs::scatter_max_px`), so no setting can throw a
// stroke past its neighbour's centre. (2) It is transient by construction: dI/dt is zero
// on a settled frame, so the streak vanishes the moment the effect stops — and the
// settled frame is the one §9's harness scores and the one T5's tracer converges to.

// Below this the gradient carries no usable direction (a flat region, or grain), and a
// division by |grad|^2 would turn noise into a long streak pointing anywhere.
const SCATTER_GRAD_FLOOR: f32 = 4.0e-4;   // |grad(luma)| >= 0.02 per texel
// Temporal deadband: |dI/dt| under this is not motion. Film grain lands here — it is
// added to `col` before the history is written, so it is present on BOTH sides of the
// difference as uncorrelated noise of its own amplitude.
const SCATTER_DT_FLOOR: f32 = 0.01;
const SCATTER_TAPS: i32 = 10;

fn hist_luma(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(hist_tex, samp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a;
}

// The streaked colour at `uv`, given this pixel's un-streaked `base` and its luminance.
// Returns `base` unchanged whenever the estimate is not trustworthy — which is every
// pixel of a settled frame, the first frame after the scatter is switched on, and every
// pixel at all when `amount` is 0.
fn scatter(uv: vec2<f32>, base: vec3<f32>, base_luma: f32) -> vec3<f32> {
    let amount = clamp(u.p8.x, 0.0, 1.0);
    let max_px = u.p8.y;
    if (amount <= 0.0 || max_px < 0.5 || u.p8.w < 0.5) {
        return base;
    }
    let t = u.texel.xy;
    let prev = hist_luma(uv);
    let dt = base_luma - prev;
    if (abs(dt) < SCATTER_DT_FLOOR) {
        return base; // nothing moved here
    }
    // grad of the PREVIOUS frame, central differences, per texel. Using the previous
    // frame rather than this one is what makes it one texture fetch pattern instead of
    // re-running the whole chain at four neighbours; for the small displacements this
    // effect is bounded to, grad(I_prev) ~ grad(I_cur).
    let g = vec2<f32>(
        (hist_luma(uv + vec2<f32>(t.x, 0.0)) - hist_luma(uv - vec2<f32>(t.x, 0.0))) * 0.5,
        (hist_luma(uv + vec2<f32>(0.0, t.y)) - hist_luma(uv - vec2<f32>(0.0, t.y))) * 0.5,
    );
    let g2 = dot(g, g);
    if (g2 < SCATTER_GRAD_FLOOR) {
        return base; // no edge here: the direction is not recoverable
    }
    let v = -dt * g / g2;             // texels per frame, along the gradient
    let speed = length(v);
    if (speed < 0.5) {
        return base; // sub-pixel travel: there is nothing to smear
    }
    let len_px = min(speed, max_px);
    let dir = v / speed;
    // Gather back along the travel — where this pixel's light came from.
    let split = clamp(u.p8.z, 0.0, 1.0);
    var acc = base;
    var wsum = vec3<f32>(1.0);
    for (var i = 1; i <= SCATTER_TAPS; i = i + 1) {
        let k = f32(i) / f32(SCATTER_TAPS);   // 0 < k <= 1 along the streak
        let s = sample_src(uv - dir * (len_px * k) * t);
        // The RGB split. At `split` = 0 every channel weight is exactly 1, so the result
        // is a plain achromatic directional blur — the dispersion cannot leak in at the
        // off position. Above 0, red is pulled toward the tail and blue toward the head,
        // symmetrically about the middle of the streak, so the mean travel is unchanged.
        let d = split * (k - 0.5) * 2.0;
        let w = max(vec3<f32>(1.0 + d, 1.0, 1.0 - d), vec3<f32>(0.0));
        acc = acc + s * w;
        wsum = wsum + w;
    }
    let streak = acc / wsum;
    // A convex mix, never an addition: the streak is a weighted mean of source samples,
    // so mixing preserves the cell's integrated luminance (§9 law 2) where adding would
    // brighten whatever moves.
    let confidence = smoothstep(SCATTER_DT_FLOOR, SCATTER_DT_FLOOR * 2.0, abs(dt));
    return mix(base, streak, amount * confidence);
}

struct FragOut {
    @location(0) view: vec4<f32>,
    @location(1) hist: vec4<f32>,
};

@fragment
fn fs_fx(in: VsOut) -> FragOut {
    var uv = in.uv;

    // Pixelate (style 5): snap uv to blocks before any sampling.
    if (u.p0.x > 4.5) {
        let block = max(u.p0.y, 1.0);
        let grid = u.texel.zw / block;
        uv = (floor(uv * grid) + 0.5) / grid;
    }

    // Chromatic aberration: spread the channels radially from the centre.
    let chroma = u.p1.y;
    var base: vec3<f32>;
    if (chroma > 0.0) {
        let dir = (uv - vec2<f32>(0.5));
        let off = dir * chroma * 0.02;
        base = vec3<f32>(
            dof(uv + off).r,
            dof(uv).g,
            dof(uv - off).b,
        );
    } else {
        base = dof(uv);
    }

    // organon#217 T15 — the scatter, on `base`: the composited picture as DoF and the
    // aberration left it, and BEFORE the history reference is taken, so the streak never
    // feeds its own estimate. Everything below (style, grade, halation, flare, vignette,
    // grain, feedback) then runs over the streaked picture exactly as it always has.
    let base_luma = luma(base);
    base = scatter(uv, base, base_luma);

    // NPR style.
    var col = apply_style(base, uv);

    // Colour grade.
    col = grade(col);

    // Cinematic finishing (#167 T1): halation + lens flares, additive on top of the
    // graded image. Both inert at amount 0, so they're free when unused.
    col = col + halation(in.uv);
    col = col + lensflare(in.uv);

    // Vignette.
    let vig = u.p1.z;
    if (vig > 0.0) {
        let d = length(in.uv - vec2<f32>(0.5)) * 1.41421356;
        col = col * (1.0 - vig * smoothstep(0.3, 1.0, d));
    }

    // Film grain.
    let grain = u.p1.w;
    if (grain > 0.0) {
        let n = hash21(in.uv * u.texel.zw + vec2<f32>(u.p3.z * 60.0));
        col = col + (n - 0.5) * grain;
    }

    col = max(col, vec3<f32>(0.0));

    // Feedback / echo trail: a persistence blend toward last frame's stored result
    // (an exponential moving average). `fb` is the persistence weight — 0 = no trail,
    // higher = longer smear that decays smoothly over frames. (A plain max() only kept
    // the brighter of the two, so trails never smeared motion and decayed unevenly.)
    let fb = clamp(u.p3.x, 0.0, 0.97);
    if (fb > 0.0) {
        let prev = textureSampleLevel(hist_tex, samp, in.uv, 0.0).rgb;
        col = mix(col, prev, fb);
    }

    var o: FragOut;
    o.view = vec4<f32>(col, 1.0);
    // The history's alpha is the scatter's motion reference (see the block above): this
    // frame's un-streaked `luma(base)` while the scatter is on, and otherwise the literal
    // 1.0 it has been written as since #152 — so with the scatter off this texture is
    // byte-identical to what it always held, and the feedback trail (which reads `.rgb`)
    // cannot tell the difference either way.
    let ref_a = select(1.0, base_luma, u.p8.x > 0.0);
    o.hist = vec4<f32>(col, ref_a);
    return o;
}

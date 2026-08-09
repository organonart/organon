// #472 Tier 2 — procedural material bake.
//
// A compute pass that evaluates a single noise/pattern layer per texel and writes
// it into the routed channel's Rgba16Float storage texture, which the Tier-1 cube
// material path (group 5) then samples like any other channel map. One layer for
// now (Tier 3 stacks + blends N).
//
// The noise library is a curated ~16-entry set (the C4D palette is the north star
// for *feel*). Everything returns a 0..1 scalar the baker remaps / tints. Periodic
// variants wrap the lattice at `period = round(scale)` so an un-rotated bake tiles
// seamlessly; rotation / fractional scale trade some seam for freedom (a Tier-3
// polish is a true toroidal eval). Compute-stage, so no derivative/uniformity
// constraints — dynamic octave loops are fine.

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;

// MatNoise ordinals (mirror params::MatNoise).
const N_VALUE: u32 = 0u;
const N_PERLIN: u32 = 1u;
const N_SIMPLEX: u32 = 2u;
const N_FBM: u32 = 3u;
const N_TURB: u32 = 4u;
const N_RIDGED: u32 = 5u;
const N_WORLEY: u32 = 6u;
const N_CELLS: u32 = 7u;
const N_GABOR: u32 = 8u;
const N_CURL: u32 = 9u;
const N_WARP: u32 = 10u;
const N_CHECKER: u32 = 11u;
const N_STRIPES: u32 = 12u;
const N_HEX: u32 = 13u;
const N_BRICK: u32 = 14u;
const N_VEINS: u32 = 15u;

// One layer's parameters (#472 Tier 3 makes the bake a per-channel COMPOSITE over a
// small stack; Tier 2 was a single layer). `meta = [enabled, blend_mode, _, _]`.
struct LayerU {
    p0: vec4<f32>,      // kind, channel (MatChannel), scale, rotation
    p1: vec4<f32>,      // offset_x, offset_y, octaves, lacunarity
    p2: vec4<f32>,      // gain, warp, contrast, gamma
    p3: vec4<f32>,      // remap_lo, remap_hi, invert, seed
    mflags: vec4<f32>,  // enabled, blend_mode (BlendMode), _, _
    grad_lo: vec4<f32>, // linear RGB, noise-0 end
    grad_hi: vec4<f32>, // linear RGB, noise-1 end
};

const MAX_LAYERS: u32 = 2u;

// Composite bake — group(0). Bakes ONE output channel (`ctrl.x` = MatChannel) by
// compositing every enabled layer that targets it, in order, with its blend mode.
struct BakeU {
    layers: array<LayerU, MAX_LAYERS>,
    ctrl: vec4<f32>,    // target_channel (MatChannel), res, num_layers, _
};
@group(0) @binding(0) var<uniform> B: BakeU;
@group(0) @binding(1) var bake_out: texture_storage_2d<rgba16float, write>;

// Derive pass — distinct group(0) bindings (a separate bind group / pipeline layout
// from the composite, so the two never bind together). Reads a baked input map
// (height or albedo) and writes a derived normal / AO map.
// `ctrl = [res, strength, radius, source(0 height/1 albedo)]`.
struct DeriveU {
    ctrl: vec4<f32>,
};
@group(0) @binding(2) var<uniform> D: DeriveU;
@group(0) @binding(3) var derive_in: texture_2d<f32>;
@group(0) @binding(4) var derive_samp: sampler;
@group(0) @binding(5) var derive_out: texture_storage_2d<rgba16float, write>;

// ---- hashing ----------------------------------------------------------------
fn hash1(p: vec2<f32>, seed: f32) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7)) + seed * 13.131;
    return fract(sin(h) * 43758.5453123);
}
fn hash2(p: vec2<f32>, seed: f32) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q + seed * 13.131) * 43758.5453);
}
// Wrap a lattice coord to an integer period so the field tiles.
fn wrap(p: vec2<f32>, period: f32) -> vec2<f32> {
    let m = max(period, 1.0);
    return p - floor(p / m) * m;
}

// ---- base noises (all return ~0..1) -----------------------------------------
fn value_noise(p: vec2<f32>, period: f32, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash1(wrap(i + vec2<f32>(0.0, 0.0), period), seed);
    let b = hash1(wrap(i + vec2<f32>(1.0, 0.0), period), seed);
    let c = hash1(wrap(i + vec2<f32>(0.0, 1.0), period), seed);
    let d = hash1(wrap(i + vec2<f32>(1.0, 1.0), period), seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn grad2(ip: vec2<f32>, period: f32, seed: f32) -> vec2<f32> {
    let a = hash1(wrap(ip, period), seed) * TAU;
    return vec2<f32>(cos(a), sin(a));
}
// Perlin gradient noise, remapped to 0..1.
fn perlin_noise(p: vec2<f32>, period: f32, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let ga = grad2(i + vec2<f32>(0.0, 0.0), period, seed);
    let gb = grad2(i + vec2<f32>(1.0, 0.0), period, seed);
    let gc = grad2(i + vec2<f32>(0.0, 1.0), period, seed);
    let gd = grad2(i + vec2<f32>(1.0, 1.0), period, seed);
    let va = dot(ga, f - vec2<f32>(0.0, 0.0));
    let vb = dot(gb, f - vec2<f32>(1.0, 0.0));
    let vc = dot(gc, f - vec2<f32>(0.0, 1.0));
    let vd = dot(gd, f - vec2<f32>(1.0, 1.0));
    let n = mix(mix(va, vb, u.x), mix(vc, vd, u.x), u.y);
    return clamp(n * 0.7071 + 0.5, 0.0, 1.0);
}

// Simplex-ish (2-D gradient noise on a skewed grid), remapped to 0..1. Not
// periodic (used un-tiled); good for organic marble.
fn simplex_noise(p: vec2<f32>, seed: f32) -> f32 {
    let K1 = 0.366025404;
    let K2 = 0.211324865;
    let i = floor(p + (p.x + p.y) * K1);
    let a = p - i + (i.x + i.y) * K2;
    let o = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), a.x > a.y);
    let b = a - o + K2;
    let c = a - 1.0 + 2.0 * K2;
    var h = max(vec3<f32>(0.5) - vec3<f32>(dot(a, a), dot(b, b), dot(c, c)), vec3<f32>(0.0));
    h = h * h * h * h;
    let ga = 2.0 * hash1(i, seed) - 1.0;
    let gb = 2.0 * hash1(i + o, seed) - 1.0;
    let gc = 2.0 * hash1(i + 1.0, seed) - 1.0;
    let n = h.x * ga * dot(a, a) + h.y * gb * dot(b, b) + h.z * gc * dot(c, c);
    return clamp(70.0 * n * 0.5 + 0.5, 0.0, 1.0);
}

// Worley: returns vec2(F1, F2) as nearest / second-nearest feature distances,
// periodic. Distances normalised so ~0..1.
fn worley(p: vec2<f32>, period: f32, seed: f32) -> vec2<f32> {
    let ip = floor(p);
    let fp = fract(p);
    var f1 = 8.0;
    var f2 = 8.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let g = vec2<f32>(f32(x), f32(y));
            let feat = g + hash2(wrap(ip + g, period), seed) - fp;
            let d = dot(feat, feat);
            if (d < f1) {
                f2 = f1;
                f1 = d;
            } else if (d < f2) {
                f2 = d;
            }
        }
    }
    return vec2<f32>(clamp(sqrt(f1), 0.0, 1.0), clamp(sqrt(f2), 0.0, 1.0));
}

// Gabor-ish: a sum of oriented cosine wavelets with per-cell random orientation.
fn gabor_noise(p: vec2<f32>, period: f32, seed: f32) -> f32 {
    var acc = 0.0;
    let ip = floor(p);
    let fp = fract(p);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let g = vec2<f32>(f32(x), f32(y));
            let cell = wrap(ip + g, period);
            let r = hash2(cell, seed);
            let center = g + r - fp;
            let d2 = dot(center, center);
            let env = exp(-3.5 * d2);
            let ang = hash1(cell, seed + 7.0) * TAU;
            let dir = vec2<f32>(cos(ang), sin(ang));
            let freq = 6.0 + 6.0 * hash1(cell, seed + 19.0);
            acc = acc + env * cos(dot(center, dir) * freq);
        }
    }
    return clamp(acc * 0.5 + 0.5, 0.0, 1.0);
}

// ---- fractal stacks ---------------------------------------------------------
fn fbm(p: vec2<f32>, period: f32, oct: i32, lac: f32, gain: f32, seed: f32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var per = period;
    var q = p;
    for (var i = 0; i < oct; i = i + 1) {
        sum = sum + amp * (perlin_noise(q, per, seed + f32(i) * 3.7) * 2.0 - 1.0);
        norm = norm + amp;
        q = q * lac;
        per = round(per * lac);
        amp = amp * gain;
    }
    return clamp(sum / max(norm, 1e-4) * 0.5 + 0.5, 0.0, 1.0);
}
fn turbulence(p: vec2<f32>, period: f32, oct: i32, lac: f32, gain: f32, seed: f32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var per = period;
    var q = p;
    for (var i = 0; i < oct; i = i + 1) {
        sum = sum + amp * abs(perlin_noise(q, per, seed + f32(i) * 3.7) * 2.0 - 1.0);
        norm = norm + amp;
        q = q * lac;
        per = round(per * lac);
        amp = amp * gain;
    }
    return clamp(sum / max(norm, 1e-4), 0.0, 1.0);
}
fn ridged(p: vec2<f32>, period: f32, oct: i32, lac: f32, gain: f32, seed: f32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var per = period;
    var q = p;
    for (var i = 0; i < oct; i = i + 1) {
        let r = 1.0 - abs(perlin_noise(q, per, seed + f32(i) * 3.7) * 2.0 - 1.0);
        sum = sum + amp * r * r;
        norm = norm + amp;
        q = q * lac;
        per = round(per * lac);
        amp = amp * gain;
    }
    return clamp(sum / max(norm, 1e-4), 0.0, 1.0);
}

// ---- structured patterns ----------------------------------------------------
fn checker(p: vec2<f32>) -> f32 {
    let c = floor(p.x) + floor(p.y);
    return c - 2.0 * floor(c * 0.5); // 0 / 1
}
fn stripes(p: vec2<f32>) -> f32 {
    return sin(p.x * PI) * 0.5 + 0.5;
}
fn hex(p: vec2<f32>) -> f32 {
    // Distance to the nearest hex-cell centre → 0..1 (bevelled tiles).
    let s = vec2<f32>(1.0, 1.7320508);
    let a = abs(fract(p / s) * s - s * 0.5);
    let b = abs(fract((p - s * 0.5) / s) * s - s * 0.5);
    let da = max(a.x * 0.8660254 + a.y * 0.5, a.y);
    let db = max(b.x * 0.8660254 + b.y * 0.5, b.y);
    return clamp(min(da, db) * 2.0, 0.0, 1.0);
}
fn brick(p: vec2<f32>) -> f32 {
    // Running-bond brick height: raised bricks (1) with recessed mortar lines (0).
    let row = floor(p.y);
    let q = vec2<f32>(p.x + select(0.0, 0.5, (row - 2.0 * floor(row * 0.5)) > 0.5), p.y);
    let c = fract(q);
    let mortar = 0.06;
    let bx = smoothstep(0.0, mortar, c.x) * smoothstep(0.0, mortar, 1.0 - c.x);
    let by = smoothstep(0.0, mortar, c.y) * smoothstep(0.0, mortar, 1.0 - c.y);
    return bx * by;
}

// ---- dispatcher: kind → 0..1 field ------------------------------------------
fn noise_field(kind: u32, p: vec2<f32>, period: f32, oct: i32, lac: f32, gain: f32, warp: f32, seed: f32) -> f32 {
    // Optional domain warp by a low-frequency perlin vector.
    var q = p;
    if (warp > 1e-4) {
        let wx = perlin_noise(p + vec2<f32>(3.1, 1.7), period, seed + 41.0) * 2.0 - 1.0;
        let wy = perlin_noise(p + vec2<f32>(8.3, 2.9), period, seed + 71.0) * 2.0 - 1.0;
        q = p + warp * vec2<f32>(wx, wy);
    }
    switch (kind) {
        case 0u: { return value_noise(q, period, seed); }
        case 1u: { return perlin_noise(q, period, seed); }
        case 2u: { return simplex_noise(q, seed); }
        case 3u: { return fbm(q, period, oct, lac, gain, seed); }
        case 4u: { return turbulence(q, period, oct, lac, gain, seed); }
        case 5u: { return ridged(q, period, oct, lac, gain, seed); }
        case 6u: { return worley(q, period, seed).x; }
        case 7u: {
            let w = worley(q, period, seed);
            return clamp(w.y - w.x, 0.0, 1.0); // cells / cracks
        }
        case 8u: { return gabor_noise(q, period, seed); }
        case 9u: {
            // Curl of a perlin potential: magnitude of the rotated gradient.
            let e = 0.5 / max(period, 1.0);
            let n1 = perlin_noise(q + vec2<f32>(0.0, e), period, seed);
            let n2 = perlin_noise(q - vec2<f32>(0.0, e), period, seed);
            let n3 = perlin_noise(q + vec2<f32>(e, 0.0), period, seed);
            let n4 = perlin_noise(q - vec2<f32>(e, 0.0), period, seed);
            let curl = vec2<f32>((n1 - n2), -(n3 - n4)) / (2.0 * e);
            return clamp(length(curl) * 0.5, 0.0, 1.0);
        }
        case 10u: {
            // Domain-warped perlin (a second warp on top for churn).
            let wx = perlin_noise(q + vec2<f32>(0.0, 0.0), period, seed + 5.0) * 2.0 - 1.0;
            let wy = perlin_noise(q + vec2<f32>(5.2, 1.3), period, seed + 9.0) * 2.0 - 1.0;
            return perlin_noise(q + 0.8 * vec2<f32>(wx, wy), period, seed);
        }
        case 11u: { return checker(q); }
        case 12u: { return stripes(q); }
        case 13u: { return hex(q); }
        case 14u: { return brick(q); }
        case 15u: {
            // Veins: sharp ridges along the cell cracks (F2-F1), inverted.
            let w = worley(q, period, seed);
            return clamp(1.0 - smoothstep(0.0, 0.35, w.y - w.x), 0.0, 1.0);
        }
        default: { return perlin_noise(q, period, seed); }
    }
}

// ---- output shaping ---------------------------------------------------------
fn remap01(v: f32, lo: f32, hi: f32) -> f32 {
    let d = max(hi - lo, 1e-4);
    return clamp((v - lo) / d, 0.0, 1.0);
}
fn shape(v_in: f32, lo: f32, hi: f32, contrast: f32, gamma: f32, invert: f32) -> f32 {
    var v = remap01(v_in, lo, hi);
    // Contrast around 0.5, then gamma.
    v = clamp((v - 0.5) * contrast + 0.5, 0.0, 1.0);
    v = pow(v, max(gamma, 1e-3));
    v = mix(v, 1.0 - v, clamp(invert, 0.0, 1.0));
    return v;
}

// Evaluate one layer at a base UV → its channel colour (albedo layers map the noise
// scalar through the gradient; scalar channels replicate it into RGB).
fn eval_layer(L: LayerU, uv: vec2<f32>) -> vec3<f32> {
    let period = max(round(L.p0.z), 1.0);
    // Rotate about the centre, offset, scale to the tile lattice.
    let rot = L.p0.w;
    let cs = cos(rot);
    let sn = sin(rot);
    var t = uv - 0.5;
    t = vec2<f32>(t.x * cs - t.y * sn, t.x * sn + t.y * cs) + 0.5;
    t = t + vec2<f32>(L.p1.x, L.p1.y);
    let p = t * period;
    let raw = noise_field(u32(L.p0.x), p, period, max(i32(L.p1.z), 1), L.p1.w, L.p2.x, L.p2.y, L.p3.w);
    let v = shape(raw, L.p3.x, L.p3.y, L.p2.z, L.p2.w, L.p3.z);
    if (u32(L.p0.y) == 0u) {
        return mix(L.grad_lo.rgb, L.grad_hi.rgb, v); // Albedo → gradient
    }
    return vec3<f32>(v);
}

// Composite `top` onto `base` by BlendMode. Operates per-channel; scalar channels
// pass vec3(v) so the mode acts identically on all three.
fn blend(base: vec3<f32>, top: vec3<f32>, mode: u32) -> vec3<f32> {
    switch (mode) {
        case 1u: { return base + top; }                                  // Add
        case 2u: { return base * top; }                                  // Multiply
        case 3u: {                                                       // Overlay
            let lo = 2.0 * base * top;
            let hi = 1.0 - 2.0 * (1.0 - base) * (1.0 - top);
            return select(hi, lo, base < vec3<f32>(0.5));
        }
        case 4u: { return 1.0 - (1.0 - base) * (1.0 - top); }            // Screen
        case 5u: { return min(base, top); }                             // Min
        case 6u: { return max(base, top); }                             // Max
        case 7u: {                                                       // Height (taller wins, soft)
            let hb = dot(base, vec3<f32>(0.333));
            let ht = dot(top, vec3<f32>(0.333));
            return mix(base, top, smoothstep(-0.08, 0.08, ht - hb));
        }
        default: { return top; }                                        // Normal (replace)
    }
}

@compute @workgroup_size(8, 8, 1)
fn bake(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = max(u32(B.ctrl.y), 1u);
    if (gid.x >= res || gid.y >= res) {
        return;
    }
    let uv = (vec2<f32>(f32(gid.x), f32(gid.y)) + 0.5) / f32(res);
    let tgt = u32(B.ctrl.x);
    let n = min(u32(B.ctrl.z), MAX_LAYERS);

    var acc = vec3<f32>(0.0);
    var hit = false;
    for (var i = 0u; i < n; i = i + 1u) {
        let L = B.layers[i];
        if (L.mflags.x < 0.5) { continue; }        // disabled
        if (u32(L.p0.y) != tgt) { continue; } // not this channel
        let col = eval_layer(L, uv);
        if (!hit) {
            acc = col;                            // first layer = base
        } else {
            acc = blend(acc, col, u32(L.mflags.y));
        }
        hit = true;
    }
    textureStore(bake_out, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(clamp(acc, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}

// Sample the derive input as a scalar height: red channel, or luminance if reading
// albedo. Explicit-LOD sample → valid in a compute stage.
fn derive_h(uv: vec2<f32>, use_albedo: bool) -> f32 {
    let c = textureSampleLevel(derive_in, derive_samp, uv, 0.0);
    return select(c.r, dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114)), use_albedo);
}

// Derive a tangent-space normal map from the input field's gradient (Sobel). The
// input is height (or albedo luminance); `strength` scales the bump.
@compute @workgroup_size(8, 8, 1)
fn derive_normal(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = max(u32(D.ctrl.x), 1u);
    if (gid.x >= res || gid.y >= res) {
        return;
    }
    let px = 1.0 / f32(res);
    let uv = (vec2<f32>(f32(gid.x), f32(gid.y)) + 0.5) * px;
    let use_albedo = D.ctrl.w > 0.5;
    let dx = (derive_h(uv + vec2<f32>(px, 0.0), use_albedo) - derive_h(uv - vec2<f32>(px, 0.0), use_albedo)) * D.ctrl.y;
    let dy = (derive_h(uv + vec2<f32>(0.0, px), use_albedo) - derive_h(uv - vec2<f32>(0.0, px), use_albedo)) * D.ctrl.y;
    let nrm = normalize(vec3<f32>(-dx, -dy, 1.0));
    textureStore(derive_out, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(nrm * 0.5 + 0.5, 1.0));
}

// Derive an AO map from the input height's cavity: darker where the centre sits
// below its neighbourhood. `radius` (texels) sets the sample ring; `strength` the
// darkening.
@compute @workgroup_size(8, 8, 1)
fn derive_ao(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = max(u32(D.ctrl.x), 1u);
    if (gid.x >= res || gid.y >= res) {
        return;
    }
    let px = 1.0 / f32(res);
    let uv = (vec2<f32>(f32(gid.x), f32(gid.y)) + 0.5) * px;
    let r = max(D.ctrl.z, 1.0) * px;
    let c = textureSampleLevel(derive_in, derive_samp, uv, 0.0).r;
    var nb = 0.0;
    nb = nb + textureSampleLevel(derive_in, derive_samp, uv + vec2<f32>(r, 0.0), 0.0).r;
    nb = nb + textureSampleLevel(derive_in, derive_samp, uv - vec2<f32>(r, 0.0), 0.0).r;
    nb = nb + textureSampleLevel(derive_in, derive_samp, uv + vec2<f32>(0.0, r), 0.0).r;
    nb = nb + textureSampleLevel(derive_in, derive_samp, uv - vec2<f32>(0.0, r), 0.0).r;
    let occ = max(0.0, nb * 0.25 - c);
    let ao = clamp(1.0 - occ * 4.0 * D.ctrl.y, 0.0, 1.0);
    textureStore(derive_out, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(ao, ao, ao, 1.0));
}

// Neural denoiser (#200 Tier 5a) — a kernel-predicting filter over the RT
// reflection / GI buffers, the neural sibling of Tier 4½ part 2's classical
// à-trous (`rt_denoise.wgsl`). The classical joint-bilateral weight is the
// *base*; a tiny seeded MLP (the Tier 0 network, regenerated inline from a seed)
// predicts a bounded per-tap multiplicative modulation of it from local edge
// features. The network REFINES a filter already known to be well-behaved
// (KPCN-lite / residual) rather than replacing it, which buys two guarantees:
//   • `net = 0` → modulation ≡ 1 → byte-identical to `rt_denoise.wgsl` ("off =
//     classical (4½)", the tier's fallback contract);
//   • the modulation is `exp` of a CLAMPED argument, so an untrained / arbitrary
//     procedural weight set can never blow a tap up or drive it negative.
// The Rust mirror is `math::nd_tap_weight` / `nd_modulation`; both regenerate
// the SAME weights from the seed and agree bit-for-bit (the offline golden bar).
//
// Everything else matches the classical pass: same 3×3 B3-spline à-trous kernel
// at a step, same premultiplied-colour operator for reflections, same per-step
// `strength` blend, same scale-invariant (camera-distance-normalized) edge-stops.
// The renderer runs it twice (step 1 then 2) ping-ponging through the scratch
// buffer, exactly like the classical denoiser.

struct NdU {
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,  // xyz = camera world pos
    params: vec4<f32>,   // texel.x, texel.y, step (à-trous), strength
    params2: vec4<f32>,  // pos_sigma (rel), lum_sigma, net (network influence), _
    net: vec4<f32>,      // seed, omega, _, _
};
@group(0) @binding(0) var<uniform> u: NdU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var src_tex: texture_2d<f32>;

// Max |log-modulation| — must equal `math::ND_MOD_CLAMP`.
const ND_MOD_CLAMP: f32 = 2.0;

// Fixed MLP topology — MUST match MLP_IN / MLP_H / MLP_OUT in math.rs / mlp.wgsl.
const MLP_IN: u32 = 4u;
const MLP_H: u32 = 8u;
const MLP_OUT: u32 = 4u;

// Murmur3-style fmix avalanche — identical to `mlp_hash` in math.rs / mlp.wgsl.
fn mlp_hash(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}

// Pseudo-random weight in [-1, 1] for (seed, index). 2^32 divisor exactly as the
// Rust mirror, so the two evaluators agree bit-for-bit.
fn mlp_rand(seed: u32, idx: u32) -> f32 {
    let h = mlp_hash(seed ^ mlp_hash(idx));
    return f32(h) / 4294967296.0 * 2.0 - 1.0;
}

// Forward pass from a single seed (walk t = 0 → seed A weights): two SIREN hidden
// layers + a linear output. Returns the first output only — the log-modulation
// scalar. Weight-block index order matches `mlp_eval` (math.rs / mlp.wgsl).
fn mlp_eval0(seed: u32, input: vec4<f32>, omega: f32) -> f32 {
    var in_arr = array<f32, 4>(input.x, input.y, input.z, input.w);

    var h0: array<f32, 8>;
    let b0_base = MLP_H * MLP_IN;
    for (var j = 0u; j < MLP_H; j = j + 1u) {
        var acc = 0.0;
        for (var i = 0u; i < MLP_IN; i = i + 1u) {
            acc += mlp_rand(seed, j * MLP_IN + i) * in_arr[i];
        }
        acc += mlp_rand(seed, b0_base + j);
        h0[j] = sin(omega * acc);
    }
    var o = MLP_H * MLP_IN + MLP_H;

    var h1: array<f32, 8>;
    let b1_base = o + MLP_H * MLP_H;
    for (var j = 0u; j < MLP_H; j = j + 1u) {
        var acc = 0.0;
        for (var k = 0u; k < MLP_H; k = k + 1u) {
            acc += mlp_rand(seed, o + j * MLP_H + k) * h0[k];
        }
        acc += mlp_rand(seed, b1_base + j);
        h1[j] = sin(acc);
    }
    o = o + MLP_H * MLP_H + MLP_H;

    // Output 0 only.
    var acc = 0.0;
    for (var k = 0u; k < MLP_H; k = k + 1u) {
        acc += mlp_rand(seed, o + 0u * MLP_H + k) * h1[k];
    }
    let b2_base = o + MLP_OUT * MLP_H;
    acc += mlp_rand(seed, b2_base + 0u);
    return acc;
}

// The bounded per-tap modulation — identical to `math::nd_modulation`.
fn nd_modulation(net: f32, mlp_out: f32) -> f32 {
    let arg = clamp(net * mlp_out, -ND_MOD_CLAMP, ND_MOD_CLAMP);
    return exp(arg);
}

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
    let net = u.params2.z;
    let seed = u32(max(u.net.x, 0.0));
    let omega = u.net.y;
    let step = i32(u.params.z);

    // Center-pixel local luminance variance (a "how noisy is here" feature the
    // network can key off — noisy pixels want a wider kernel). Computed once.
    var s1 = 0.0;
    var s2 = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let tp = clamp(px + vec2<i32>(x, y), vec2<i32>(0), dimsu - vec2<i32>(1));
            let cl = lum(textureLoad(src_tex, tp, 0).rgb);
            s1 = s1 + cl;
            s2 = s2 + cl * cl;
        }
    }
    let lmean = s1 / 9.0;
    let lvar = max(s2 / 9.0 - lmean * lmean, 0.0);

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
                // Classical edge-stop arguments (same as rt_denoise.wgsl).
                let dpos = length(wp - wp0) / pos_sigma;
                let dluma = abs(lum(cc.rgb) - l0) / (l0 + lum_sigma);
                var base = w * exp(-dpos) * exp(-dluma);
                // Neural modulation: MLP(features) → bounded log-multiplier.
                // net = 0 skips the network entirely (byte-identical classical).
                if (net > 0.0) {
                    let radius = sqrt(f32(x * x + y * y));
                    let feats = vec4<f32>(dpos, dluma, radius, lvar);
                    base = base * nd_modulation(net, mlp_eval0(seed, feats, omega));
                }
                sum = sum + cc * base;
                wsum = wsum + base;
                continue;
            }
        }
    }
    let filtered = select(c0, sum / max(wsum, 1e-4), wsum > 1e-4);
    return mix(c0, filtered, strength);
}

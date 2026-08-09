// Neural shading foundation (#200 Tier 0) — the WGSL half of the tiny MLP whose
// Rust reference lives in `math.rs` (`mlp_eval` / `mlp_eval_seed`). This is an
// include-able *pattern*, not a full shader: later neural tiers paste these
// functions into their own entry points (Tier 1's neural-field generator is the
// first). It ships dark — no entry point renders anything here; `tests/wgsl.rs`
// wraps it in a trivial fragment shell so naga still validates the arithmetic.
//
// Weights are regenerated inline from a compact integer seed by the SAME
// deterministic hash as the CPU mirror, so a whole network travels as a `u32`
// (plus a second seed + walk `t` for the latent walk) — no large buffer upload,
// and the GPU output matches the CPU reference bit-for-bit in structure (the
// on-Mac agreement test compares them). u32 arithmetic wraps in WGSL exactly
// like Rust's `wrapping_*`, and `sin` is the shared SIREN activation.

// Fixed topology — MUST match MLP_IN / MLP_H / MLP_OUT in math.rs.
const MLP_IN: u32 = 4u;
const MLP_H: u32 = 8u;
const MLP_OUT: u32 = 4u;

// Murmur3-style fmix avalanche — identical to `mlp_hash` in math.rs.
fn mlp_hash(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}

// Pseudo-random weight in [-1, 1] for (seed, index).
fn mlp_rand(seed: u32, idx: u32) -> f32 {
    let h = mlp_hash(seed ^ mlp_hash(idx));
    // 2^32 — the identical divisor math.rs::mlp_rand uses; exactly
    // representable in f32, so the two evaluators agree bit-for-bit.
    return f32(h) / 4294967296.0 * 2.0 - 1.0;
}

// One latent-walk weight: lerp seed A's and seed B's weight `idx` by `t`.
fn mlp_w(seed_a: u32, seed_b: u32, t: f32, idx: u32) -> f32 {
    return mix(mlp_rand(seed_a, idx), mlp_rand(seed_b, idx), t);
}

// Forward pass straight from seed(s), regenerating weights inline. Two SIREN
// hidden layers (`sin`, first layer scaled by `omega`) + a linear output.
// Weight-block layout walked in order: W0(H·IN), b0(H), W1(H·H), b1(H),
// W2(OUT·H), b2(OUT) — identical index order to math.rs's `mlp_eval`.
fn mlp_eval(seed_a: u32, seed_b: u32, t: f32, input: vec4<f32>, omega: f32) -> vec4<f32> {
    var in_arr = array<f32, 4>(input.x, input.y, input.z, input.w);

    // Layer 0: in → H, sin(omega · (W0·in + b0)).
    var h0: array<f32, 8>;
    let b0_base = MLP_H * MLP_IN;
    for (var j = 0u; j < MLP_H; j = j + 1u) {
        var acc = 0.0;
        for (var i = 0u; i < MLP_IN; i = i + 1u) {
            acc += mlp_w(seed_a, seed_b, t, j * MLP_IN + i) * in_arr[i];
        }
        acc += mlp_w(seed_a, seed_b, t, b0_base + j);
        h0[j] = sin(omega * acc);
    }
    var o = MLP_H * MLP_IN + MLP_H;

    // Layer 1: H → H, sin(W1·h0 + b1).
    var h1: array<f32, 8>;
    let b1_base = o + MLP_H * MLP_H;
    for (var j = 0u; j < MLP_H; j = j + 1u) {
        var acc = 0.0;
        for (var k = 0u; k < MLP_H; k = k + 1u) {
            acc += mlp_w(seed_a, seed_b, t, o + j * MLP_H + k) * h0[k];
        }
        acc += mlp_w(seed_a, seed_b, t, b1_base + j);
        h1[j] = sin(acc);
    }
    o = o + MLP_H * MLP_H + MLP_H;

    // Layer 2: H → OUT, linear.
    var out_arr: array<f32, 4>;
    let b2_base = o + MLP_OUT * MLP_H;
    for (var oi = 0u; oi < MLP_OUT; oi = oi + 1u) {
        var acc = 0.0;
        for (var k = 0u; k < MLP_H; k = k + 1u) {
            acc += mlp_w(seed_a, seed_b, t, o + oi * MLP_H + k) * h1[k];
        }
        acc += mlp_w(seed_a, seed_b, t, b2_base + oi);
        out_arr[oi] = acc;
    }
    return vec4<f32>(out_arr[0], out_arr[1], out_arr[2], out_arr[3]);
}

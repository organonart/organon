// Neural radiance cache — WGSL query (#256 Tier 0). The GPU half of the trained
// SIREN whose Rust reference is `math.rs` (`RadianceCache` / `nrc_forward` /
// `oct_encode` / `RadianceCache::encode`). Like `mlp.wgsl` this is an INCLUDE-able
// library: it declares no bindings and no entry point — a consumer concatenates it
// ahead of its own shader (the path tracer's early-termination in `rt_pathtrace.wgsl`;
// the cube-shader ambient in Tier 2) and calls `nrc_query` with a pointer to its own
// weight storage buffer. `tests/wgsl.rs` wraps it in a fragment shell so naga still
// validates the arithmetic offline.
//
// Unlike `mlp.wgsl` (weights regenerated inline from a seed), the cache carries
// TRAINED weights, so they travel as a 419-float storage buffer the visual uploads
// each frame. Everything else — the octahedral direction encode, the AABB position
// normalize, the two-`sin` SIREN forward pass, the weight-block index order —
// matches the CPU reference bit-for-bit in structure (the on-Mac agreement test
// compares a query against `RadianceCache::query`).
//
// INCLUDE CONTRACT: the consumer MUST declare the weight buffer as a module global
// named `nrc_w` before use (any group/binding), e.g.
//   @group(0) @binding(6) var<storage, read> nrc_w: array<f32, NRC_WEIGHTS>;
// `nrc_query` reads it directly. (WGSL forbids `ptr<storage,…>` function params, and
// since the library is CONCATENATED into the consumer's module, a module-scope
// forward reference to `nrc_w` is the portable way to share the buffer.)

// Topology — MUST match NRC_IN / NRC_H / NRC_OUT in math.rs.
const NRC_IN: u32 = 5u;
const NRC_H: u32 = 16u;
const NRC_OUT: u32 = 3u;
// Section offsets into the flat weight block (W0, b0, W1, b1, W2, b2) — identical
// to math.rs's NRC_B0 / NRC_W1 / NRC_B1 / NRC_W2 / NRC_B2.
const NRC_B0: u32 = NRC_H * NRC_IN;         // 80
const NRC_W1: u32 = NRC_B0 + NRC_H;         // 96
const NRC_B1: u32 = NRC_W1 + NRC_H * NRC_H; // 352
const NRC_W2: u32 = NRC_B1 + NRC_H;         // 368
const NRC_B2: u32 = NRC_W2 + NRC_OUT * NRC_H; // 416
const NRC_WEIGHTS: u32 = NRC_B2 + NRC_OUT;  // 419

// Octahedral encode of a (not-necessarily-unit) direction → [0,1]². The exact
// concentric-oct map math.rs::oct_encode uses; the zero vector → the centre.
fn nrc_oct_encode(dir: vec3<f32>) -> vec2<f32> {
    let n = max(abs(dir.x) + abs(dir.y) + abs(dir.z), 1e-8);
    var px = dir.x / n;
    var py = dir.y / n;
    if (dir.z < 0.0) {
        let sx = select(-1.0, 1.0, px >= 0.0);
        let sy = select(-1.0, 1.0, py >= 0.0);
        let ox = (1.0 - abs(py)) * sx;
        let oy = (1.0 - abs(px)) * sy;
        px = ox;
        py = oy;
    }
    return vec2<f32>(px * 0.5 + 0.5, py * 0.5 + 0.5);
}

// Encode (world position normalized to [-1,1] over the field AABB, octahedral
// direction remapped to [-1,1]) → the 5-D network input. Mirrors
// RadianceCache::encode.
fn nrc_encode(pos: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>, dir: vec3<f32>) -> array<f32, 5> {
    let span = max(bmax - bmin, vec3<f32>(1e-6));
    let np = ((pos - bmin) / span) * 2.0 - 1.0;
    let o = nrc_oct_encode(dir);
    return array<f32, 5>(np.x, np.y, np.z, o.x * 2.0 - 1.0, o.y * 2.0 - 1.0);
}

// Evaluate the cache: encoded input → radiance (RGB). Reads the module-global
// weight buffer `nrc_w` (see the include contract above). Two SIREN hidden layers
// (sin, the first scaled by `omega`) + a linear output — identical to nrc_forward.
fn nrc_query(x: array<f32, 5>, omega: f32) -> vec3<f32> {
    var xin = x;
    // Layer 0: IN → H, sin(omega·(W0·x + b0)).
    var h0: array<f32, 16>;
    for (var j = 0u; j < NRC_H; j = j + 1u) {
        var acc = nrc_w[NRC_B0 + j];
        for (var i = 0u; i < NRC_IN; i = i + 1u) {
            acc += nrc_w[j * NRC_IN + i] * xin[i];
        }
        h0[j] = sin(omega * acc);
    }
    // Layer 1: H → H, sin(W1·h0 + b1).
    var h1: array<f32, 16>;
    for (var k = 0u; k < NRC_H; k = k + 1u) {
        var acc = nrc_w[NRC_B1 + k];
        for (var j = 0u; j < NRC_H; j = j + 1u) {
            acc += nrc_w[NRC_W1 + k * NRC_H + j] * h0[j];
        }
        h1[k] = sin(acc);
    }
    // Layer 2: H → OUT, linear.
    var out = vec3<f32>(0.0);
    for (var o = 0u; o < NRC_OUT; o = o + 1u) {
        var acc = nrc_w[NRC_B2 + o];
        for (var k = 0u; k < NRC_H; k = k + 1u) {
            acc += nrc_w[NRC_W2 + o * NRC_H + k] * h1[k];
        }
        if (o == 0u) { out.x = acc; }
        else if (o == 1u) { out.y = acc; }
        else { out.z = acc; }
    }
    return out;
}

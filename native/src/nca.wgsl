// Organic Math — Neural Cellular Automaton step (learned surrogate, Tier B #407).
//
// ONE autoregressive NCA update over a periodic 2-D grid: for every cell we gather
// a fixed perception per channel — [identity, sobel_x, sobel_y, laplacian] — then
// run a tiny two-layer MLP (relu hidden → linear delta), integrate, and tanh-squash
// every channel so the rollout is provably bounded (|state| ≤ 1) for ANY weights.
// This mirrors `math::NeuralCA::step` exactly.
//
// STATUS: validated-but-not-yet-dispatched. The **CPU rollout in `math.rs` is
// authoritative** — this GPU compute path is registered in `tests/wgsl.rs` so naga
// parse-validates it offline, but its dispatch is DEFERRED, exactly as `FieldSim`
// deferred its GPU route (`rd.wgsl` shipped ahead of the CPU sim likewise). When a
// grid outgrows the CPU budget, the host can bind these buffers and dispatch
// `cs_step` ping-ponging `state_in`/`state_out`.
//
// State is stored **planar** in a storage buffer: channel c occupies
// [c*w*h .. (c+1)*w*h], matching the CPU layout. Weights are one flat storage buffer
// laid out [w1 | b1 | w2 | b2] (row-major matrices), addressed via the `Dims` header.

// Scratch caps (local arrays need constant bounds). channels ≤ 16, hidden ≤ 64.
const MAX_CHANNELS: u32 = 16u;
const MAX_PERCEPT: u32 = 64u;   // 4 * MAX_CHANNELS
const MAX_HIDDEN: u32 = 64u;
const NCA_PERCEPT: u32 = 4u;    // id, sobel_x, sobel_y, laplacian

struct Dims {
    // channels, hidden, grid_w, grid_h
    shape: vec4<u32>,
    // dt, _, _, _
    misc: vec4<f32>,
};

@group(0) @binding(0) var<storage, read> state_in: array<f32>;
@group(0) @binding(1) var<storage, read_write> state_out: array<f32>;
@group(0) @binding(2) var<storage, read> weights: array<f32>;
@group(0) @binding(3) var<uniform> dims: Dims;

// Periodic wrap of an integer coordinate into [0, n).
fn wrap(i: i32, n: i32) -> i32 {
    return ((i % n) + n) % n;
}

// state_in index for channel c at cell (x,y) — planar layout.
fn sidx(c: u32, x: i32, y: i32, w: i32, h: i32) -> u32 {
    let cells = u32(w) * u32(h);
    return c * cells + u32(wrap(y, h)) * u32(w) + u32(wrap(x, w));
}

@compute @workgroup_size(8, 8)
fn cs_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let channels = dims.shape.x;
    let hidden = dims.shape.y;
    let w = i32(dims.shape.z);
    let h = i32(dims.shape.w);
    let dt = dims.misc.x;
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= w || y >= h) {
        return;
    }

    // --- perception: [id, sobel_x, sobel_y, laplacian] per channel ---
    var percept: array<f32, 64>; // MAX_PERCEPT
    var c: u32 = 0u;
    loop {
        if (c >= channels || c >= MAX_CHANNELS) { break; }
        let cc = state_in[sidx(c, x, y, w, h)];
        let l = state_in[sidx(c, x - 1, y, w, h)];
        let r = state_in[sidx(c, x + 1, y, w, h)];
        let up = state_in[sidx(c, x, y - 1, w, h)];
        let dn = state_in[sidx(c, x, y + 1, w, h)];
        let ul = state_in[sidx(c, x - 1, y - 1, w, h)];
        let ur = state_in[sidx(c, x + 1, y - 1, w, h)];
        let dl = state_in[sidx(c, x - 1, y + 1, w, h)];
        let dr = state_in[sidx(c, x + 1, y + 1, w, h)];
        let sx = (r - l) * 0.25 + (ur - ul + dr - dl) * 0.125;
        let sy = (dn - up) * 0.25 + (dl - ul + dr - ur) * 0.125;
        let lap = l + r + up + dn - 4.0 * cc;
        let base = c * NCA_PERCEPT;
        percept[base] = cc;
        percept[base + 1u] = sx;
        percept[base + 2u] = sy;
        percept[base + 3u] = lap;
        c = c + 1u;
    }

    let pin = NCA_PERCEPT * channels;
    // Weight buffer offsets: [w1 (hidden*pin) | b1 (hidden) | w2 (channels*hidden) | b2 (channels)]
    let w1_off = 0u;
    let b1_off = w1_off + hidden * pin;
    let w2_off = b1_off + hidden;
    let b2_off = w2_off + channels * hidden;

    // --- hidden = relu(w1·percept + b1) ---
    var hid: array<f32, 64>; // MAX_HIDDEN
    var j: u32 = 0u;
    loop {
        if (j >= hidden || j >= MAX_HIDDEN) { break; }
        var acc = weights[b1_off + j];
        var k: u32 = 0u;
        loop {
            if (k >= pin || k >= MAX_PERCEPT) { break; }
            acc = acc + weights[w1_off + j * pin + k] * percept[k];
            k = k + 1u;
        }
        hid[j] = max(acc, 0.0);
        j = j + 1u;
    }

    // --- delta = w2·hidden + b2 ; state += dt·delta ; tanh squash ---
    let cells = u32(w) * u32(h);
    let cell = u32(y) * u32(w) + u32(x);
    var oc: u32 = 0u;
    loop {
        if (oc >= channels || oc >= MAX_CHANNELS) { break; }
        var acc = weights[b2_off + oc];
        var jj: u32 = 0u;
        loop {
            if (jj >= hidden || jj >= MAX_HIDDEN) { break; }
            acc = acc + weights[w2_off + oc * hidden + jj] * hid[jj];
            jj = jj + 1u;
        }
        let prev = state_in[oc * cells + cell];
        state_out[oc * cells + cell] = tanh(prev + dt * acc);
        oc = oc + 1u;
    }
}

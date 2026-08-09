// Organic Math — Gray–Scott reaction–diffusion (Turing patterns), compute.
//
// Two virtual chemicals U and V diffuse and react on a 2-D grid; the nonlinear
// reaction U + 2V → 3V plus a feed/kill balance spontaneously breaks the uniform
// state into spots, stripes, or labyrinthine mazes (Turing's morphogenesis). We
// step it on a torus (wrapped Laplacian) so the field tiles seamlessly, then the
// lit shaders sample V triplanar-projected onto the surface as crawling emissive
// dapple. State is stored in an `rgba16float` texture: r = U, g = V.
//
// Ping-pong: `front` is read (textureLoad, no sampler) and `back` is written; the
// host alternates the two textures each step.

struct RdSim {
    rates: vec4<f32>, // feed, kill, diffuse_u, diffuse_v
    misc: vec4<f32>,  // dt, size, _, _
};

@group(0) @binding(0) var front: texture_2d<f32>;
@group(0) @binding(1) var back: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: RdSim;

// Integer hash → [0,1), for scattering seed spots.
fn hash(p: vec2<i32>) -> f32 {
    var h = u32(p.x) * 374761393u + u32(p.y) * 668265263u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32(h & 0xffffffu) / f32(0xffffffu);
}

// Seed the field: U = 1 everywhere, with scattered coarse cells of V = 1 so the
// reaction has nuclei to grow from. Writes `back` (the host points it at the
// texture the scene will read).
@compute @workgroup_size(8, 8)
fn cs_seed(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = i32(p.misc.y);
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= size || y >= size) {
        return;
    }
    let cell = max(size / 24, 1);
    let v = select(0.0, 1.0, hash(vec2<i32>(x / cell, y / cell)) > 0.6);
    textureStore(back, vec2<i32>(x, y), vec4<f32>(1.0, v, 0.0, 1.0));
}

fn at(x: i32, y: i32, size: i32) -> vec2<f32> {
    let xm = (x + size) % size;
    let ym = (y + size) % size;
    return textureLoad(front, vec2<i32>(xm, ym), 0).rg;
}

@compute @workgroup_size(8, 8)
fn cs_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = i32(p.misc.y);
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (x >= size || y >= size) {
        return;
    }

    let c = at(x, y, size);
    // 9-point Laplacian (orthogonal 0.2, diagonal 0.05) — the standard Gray–Scott
    // stencil; isotropic enough to avoid grid-aligned artifacts.
    let lap = at(x - 1, y, size) * 0.2
        + at(x + 1, y, size) * 0.2
        + at(x, y - 1, size) * 0.2
        + at(x, y + 1, size) * 0.2
        + at(x - 1, y - 1, size) * 0.05
        + at(x + 1, y - 1, size) * 0.05
        + at(x - 1, y + 1, size) * 0.05
        + at(x + 1, y + 1, size) * 0.05
        - c;

    let u = c.x;
    let v = c.y;
    let feed = p.rates.x;
    let kill = p.rates.y;
    let reaction = u * v * v;
    let du = p.rates.z * lap.x - reaction + feed * (1.0 - u);
    let dv = p.rates.w * lap.y + reaction - (feed + kill) * v;
    let nu = clamp(u + du * p.misc.x, 0.0, 1.0);
    let nv = clamp(v + dv * p.misc.x, 0.0, 1.0);
    textureStore(back, vec2<i32>(x, y), vec4<f32>(nu, nv, 0.0, 1.0));
}

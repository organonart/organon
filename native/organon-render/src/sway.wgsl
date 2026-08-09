// Two-way coupling (#182 Tier 4) — the fluid pushes back on the generator.
//
// One thread per instance: sample the fluid velocity grid at the node's world
// position, integrate a damped displacement spring (offset pulled by the flow,
// restored toward the procedural position), and add the offset to the instance
// matrix's translation column IN PLACE — after the CPU upload, before the
// depth/shadow/scene passes, so every pass sees the swayed structure. No
// GPU→CPU readback: the spring state lives in a persistent GPU buffer keyed by
// instance index (procedural node order is stable frame-to-frame; the host
// zeroes the state whenever the count changes). CPU mirror: math::sway_step.

struct SwayU {
    box0: vec4<f32>, // velocity-grid AABB min, w = instance count
    box1: vec4<f32>, // velocity-grid AABB max, w = dt (frame seconds)
    grid: vec4<f32>, // xyz = velocity grid res, w = velocity scale (grid→world)
    ctl: vec4<f32>,  // x = drive, y = spring, z = damping, w = max offset
};

@group(0) @binding(0) var<uniform> u: SwayU;
// Instance matrices as raw vec4 columns (4 per instance; translation = i*4+3).
@group(0) @binding(1) var<storage, read_write> inst: array<vec4<f32>>;
// Spring state: 2 vec4 per instance — [offset.xyz, _], [dvel.xyz, _].
@group(0) @binding(2) var<storage, read_write> state: array<vec4<f32>>;
// The fluid velocity grid (vec4 per cell, xyz = velocity) — either the
// Navier–Stokes vel buffer (world units/s, scale 1) or the MLS-MPM grid_v
// (grid units/s, scale = cell size).
@group(0) @binding(3) var<storage, read> vel: array<vec4<f32>>;

fn vel_at(x: i32, y: i32, z: i32) -> vec3<f32> {
    let r = vec3<i32>(u.grid.xyz);
    let c = clamp(vec3<i32>(x, y, z), vec3<i32>(0), r - vec3<i32>(1));
    return vel[u32((c.z * r.y + c.y) * r.x + c.x)].xyz;
}

// Trilinear fluid velocity at a world position (cell centres at integers, the
// solver grids' own convention).
fn sample_vel(p: vec3<f32>) -> vec3<f32> {
    let ext = max(u.box1.xyz - u.box0.xyz, vec3<f32>(1e-5));
    let c = clamp((p - u.box0.xyz) / ext, vec3<f32>(0.0), vec3<f32>(1.0))
        * (u.grid.xyz - 1.0);
    let b = floor(c);
    let f = c - b;
    let ix = i32(b.x);
    let iy = i32(b.y);
    let iz = i32(b.z);
    let v00 = mix(vel_at(ix, iy, iz), vel_at(ix + 1, iy, iz), f.x);
    let v10 = mix(vel_at(ix, iy + 1, iz), vel_at(ix + 1, iy + 1, iz), f.x);
    let v01 = mix(vel_at(ix, iy, iz + 1), vel_at(ix + 1, iy, iz + 1), f.x);
    let v11 = mix(vel_at(ix, iy + 1, iz + 1), vel_at(ix + 1, iy + 1, iz + 1), f.x);
    return mix(mix(v00, v10, f.y), mix(v01, v11, f.y), f.z) * u.grid.w;
}

@compute @workgroup_size(64)
fn cs_sway(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u32(u.box0.w)) {
        return;
    }
    let base = i * 4u;
    let pos = inst[base + 3u].xyz; // the procedural (un-swayed) position
    let dt = clamp(u.box1.w, 0.0, 0.1);

    var off = state[i * 2u + 0u].xyz;
    var dv = state[i * 2u + 1u].xyz;
    let vf = sample_vel(pos + off);

    // Damped spring: driven by the local flow, restored to the procedural
    // position. Semi-implicit Euler — mirrors math::sway_step exactly.
    let acc = vf * u.ctl.x - off * u.ctl.y - dv * u.ctl.z;
    dv = dv + acc * dt;
    off = off + dv * dt;
    let len = length(off);
    if (len > u.ctl.w && len > 1e-6) {
        off = off * (u.ctl.w / len);
    }

    state[i * 2u + 0u] = vec4<f32>(off, 0.0);
    state[i * 2u + 1u] = vec4<f32>(dv, 0.0);
    inst[base + 3u] = vec4<f32>(pos + off, inst[base + 3u].w);
}

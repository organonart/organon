// Aura-Fluid (#81 showpiece) — a GPU Navier–Stokes solver on a 3-D grid.
//
// Stable Fluids (Stam 1999): semi-Lagrangian advection (unconditionally stable) +
// a Jacobi pressure projection that makes the field divergence-free — which is what
// manufactures swirl out of the stirring. Vorticity confinement (Fedkiw et al.)
// re-injects the small eddies numerical diffusion eats. The generator does NOT
// become the field — it STIRS it: node motion is splatted into a coarse grid on the
// CPU (`math::VelGrid`) and injected here as a momentum source, so vortices shed off
// the moving structure and persist in its wake. The evolved velocity field is the
// same buffer the particle system samples (so the motes ride the real fluid).
//
// The grid is a CUBE (uniform cell size `h`), so the finite-difference operators
// below are the textbook ones. Velocity is stored in WORLD units (the motes read it
// directly). Everything is one storage-buffer set; each pass binds the subset it
// needs (see fluid.rs). The PDE math here is mirrored on the CPU in `math.rs`
// (`fluid_project`) and unit-tested (projection drives divergence toward zero).

struct FluidU {
    res: vec4<u32>,  // xyz grid resolution, w = cell count (x*y*z)
    gmin: vec4<f32>, // xyz grid AABB min, w = dt (seconds)
    gmax: vec4<f32>, // xyz grid AABB max, w = time (seconds)
    p0: vec4<f32>,   // force, vorticity, dissipation, inflow_decay
    p1: vec4<f32>,   // cell size h (world), dye rate, dye dissipation, boundaries (0/1)
    // #182 Tier 2: buoyancy (signed; y-force per heat unit), heat decay (1/s),
    // beat splash impulse (world units/s², scaled by the beat envelope), beat env.
    p2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> U: FluidU;
@group(0) @binding(1) var<storage, read_write> vel_a: array<vec4<f32>>; // canonical velocity
@group(0) @binding(2) var<storage, read_write> vel_b: array<vec4<f32>>; // advection scratch
@group(0) @binding(3) var<storage, read> source: array<vec4<f32>>;      // injected stir
@group(0) @binding(4) var<storage, read_write> divg: array<f32>;        // ∇·v term
@group(0) @binding(5) var<storage, read_write> curlb: array<vec4<f32>>; // vorticity (xyz, |ω| in w)
@group(0) @binding(6) var<storage, read> p_src: array<f32>;             // pressure (read)
@group(0) @binding(7) var<storage, read_write> p_dst: array<f32>;       // pressure (write)
// #182 Tier 1 — the RGB dye field the ink-volume pass renders. Each pass binds
// only the subset it needs (see fluid.rs), same as the velocity buffers above.
@group(0) @binding(8) var<storage, read_write> dye_a: array<vec4<f32>>;   // canonical dye
@group(0) @binding(9) var<storage, read_write> dye_b: array<vec4<f32>>;   // advection scratch
@group(0) @binding(10) var<storage, read> dye_src: array<vec4<f32>>;      // injected dye (CPU splat)
@group(0) @binding(11) var<storage, read_write> dye_c: array<vec4<f32>>;  // MacCormack scratch

fn count() -> u32 { return U.res.w; }
fn hh() -> f32 { return max(U.p1.x, 1e-6); }

// #182 Tier 2 — solid boundaries: the CPU marks node-occupied cells in
// `source[i].w`; when the Boundaries toggle is on those cells are walls moving
// at the node's (unscaled) velocity — no-slip against the generator's geometry,
// so wakes shed off the structure and flow channels through lattices.
fn solid(i: u32) -> bool {
    return U.p1.w > 0.5 && source[i].w > 0.5;
}
fn solid_at(x: i32, y: i32, z: i32) -> bool {
    return solid(cidx(x, y, z));
}
// World-space centre of cell `i` (for the radial beat splash).
fn cell_world(xyz: vec3<i32>) -> vec3<f32> {
    let f = (vec3<f32>(xyz) + vec3<f32>(0.5)) / vec3<f32>(U.res.xyz);
    return U.gmin.xyz + f * (U.gmax.xyz - U.gmin.xyz);
}

fn cidx(x: i32, y: i32, z: i32) -> u32 {
    let rx = i32(U.res.x);
    let ry = i32(U.res.y);
    let rz = i32(U.res.z);
    let cx = clamp(x, 0, rx - 1);
    let cy = clamp(y, 0, ry - 1);
    let cz = clamp(z, 0, rz - 1);
    return u32((cz * ry + cy) * rx + cx);
}

fn decode(i: u32) -> vec3<i32> {
    let rx = U.res.x;
    let ry = U.res.y;
    let x = i % rx;
    let y = (i / rx) % ry;
    let z = i / (rx * ry);
    return vec3<i32>(i32(x), i32(y), i32(z));
}

fn vel_at(x: i32, y: i32, z: i32) -> vec3<f32> {
    return vel_a[cidx(x, y, z)].xyz;
}

// Trilinear velocity at cell-space coordinate `c` (cell centres at integer coords).
fn sample_vel(c: vec3<f32>) -> vec3<f32> {
    let b = floor(c);
    let f = c - b;
    let ix = i32(b.x);
    let iy = i32(b.y);
    let iz = i32(b.z);
    let v000 = vel_at(ix, iy, iz);
    let v100 = vel_at(ix + 1, iy, iz);
    let v010 = vel_at(ix, iy + 1, iz);
    let v110 = vel_at(ix + 1, iy + 1, iz);
    let v001 = vel_at(ix, iy, iz + 1);
    let v101 = vel_at(ix + 1, iy, iz + 1);
    let v011 = vel_at(ix, iy + 1, iz + 1);
    let v111 = vel_at(ix + 1, iy + 1, iz + 1);
    let c00 = mix(v000, v100, f.x);
    let c10 = mix(v010, v110, f.x);
    let c01 = mix(v001, v101, f.x);
    let c11 = mix(v011, v111, f.x);
    let c0 = mix(c00, c10, f.y);
    let c1 = mix(c01, c11, f.y);
    return mix(c0, c1, f.z);
}

// 1) Inject the stir + relax. The generator's splatted node motion (`source`) adds
// momentum; `inflow_decay` eases the field toward the stir where it's strong and
// back toward rest in still regions (so the wake fades on its own clock).
// #182 Tier 2: solid cells are pinned to the wall (node) velocity; fluid cells
// additionally get buoyancy (the heat scalar riding dye_a.w drives a vertical
// force — smoke rises / ink sinks by sign) and the radial beat-splash impulse.
@compute @workgroup_size(64)
fn cs_inject(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let dt = U.gmin.w;
    if (solid(i)) {
        // Moving no-slip: the fluid at the wall moves WITH the wall (the node's
        // unscaled velocity, not the force-amplified stir).
        vel_a[i] = vec4<f32>(source[i].xyz, 0.0);
        return;
    }
    var v = vel_a[i].xyz;
    // #248 force-drive fills `source` with the field's solenoidal azimuthal
    // circulation (the oscillating dipole's B field) — a real rotational stir the
    // projection keeps, unlike the conservative E which it would cancel. The normal
    // relax-toward-source then swirls the fluid toward it (strong near the core,
    // reversing as the field oscillates).
    let stir = source[i].xyz * U.p0.x;
    v = v + stir * dt;
    v = mix(v, stir, clamp(U.p0.w * dt, 0.0, 1.0));
    // Buoyancy: vertical force proportional to the local heat (dye_a.w).
    v.y = v.y + U.p2.x * dye_a[i].w * dt;
    // Beat splash: a radial momentum impulse from the field centre, scaled by
    // the decaying beat envelope (0 while Pulse is off).
    let splash = U.p2.z * U.p2.w;
    if (splash != 0.0) {
        let xyz = decode(i);
        let ctr = 0.5 * (U.gmin.xyz + U.gmax.xyz);
        let d = cell_world(xyz) - ctr;
        let len = length(d);
        if (len > 1e-4) {
            v = v + (d / len) * (splash * dt);
        }
    }
    vel_a[i] = vec4<f32>(v, 0.0);
}

// 2) Semi-Lagrangian advection: backtrace each cell along the velocity and resample.
// Dissipation fades the field (viscosity-ish stability + wake decay). Writes vel_b;
// the host copies it back to vel_a.
@compute @workgroup_size(64)
fn cs_advect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let dt = U.gmin.w;
    // #182 Tier 2: walls keep the wall velocity through the advection.
    if (solid(i)) {
        vel_b[i] = vec4<f32>(source[i].xyz, 0.0);
        return;
    }
    let xyz = decode(i);
    let v = vel_a[i].xyz;
    // World displacement v*dt → cells (divide by the cell size).
    let c_prev = vec3<f32>(xyz) - v * (dt / hh());
    var nv = sample_vel(c_prev);
    nv = nv * clamp(1.0 - U.p0.z * dt, 0.0, 1.0);
    vel_b[i] = vec4<f32>(nv, 0.0);
}

// 3a) Curl ω = ∇×v (central differences). |ω| in .w for the confinement gradient.
@compute @workgroup_size(64)
fn cs_curl(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let xyz = decode(i);
    let x = xyz.x; let y = xyz.y; let z = xyz.z;
    let wx = (vel_at(x, y + 1, z).z - vel_at(x, y - 1, z).z)
           - (vel_at(x, y, z + 1).y - vel_at(x, y, z - 1).y);
    let wy = (vel_at(x, y, z + 1).x - vel_at(x, y, z - 1).x)
           - (vel_at(x + 1, y, z).z - vel_at(x - 1, y, z).z);
    let wz = (vel_at(x + 1, y, z).y - vel_at(x - 1, y, z).y)
           - (vel_at(x, y + 1, z).x - vel_at(x, y - 1, z).x);
    let w = vec3<f32>(wx, wy, wz) / (2.0 * hh());
    curlb[i] = vec4<f32>(w, length(w));
}

// 3b) Vorticity confinement: push velocity along N×ω, where N points up the |ω|
// gradient — re-sharpening the eddies (the "more curl / more whorls" dial).
@compute @workgroup_size(64)
fn cs_vorticity(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let eps = U.p0.y;
    if (eps <= 0.0) { return; }
    // #182 Tier 2: walls are pinned — confinement must not push them off the
    // wall velocity (inject/advect re-pin, but the drift would reach the
    // pressure solve within the same substep).
    if (solid(i)) { return; }
    let xyz = decode(i);
    let x = xyz.x; let y = xyz.y; let z = xyz.z;
    let gx = curlb[cidx(x + 1, y, z)].w - curlb[cidx(x - 1, y, z)].w;
    let gy = curlb[cidx(x, y + 1, z)].w - curlb[cidx(x, y - 1, z)].w;
    let gz = curlb[cidx(x, y, z + 1)].w - curlb[cidx(x, y, z - 1)].w;
    let grad = vec3<f32>(gx, gy, gz) / (2.0 * hh());
    let gl = length(grad);
    if (gl > 1e-6) {
        let n = grad / gl;
        let f = eps * hh() * cross(n, curlb[i].xyz);
        vel_a[i] = vec4<f32>(vel_a[i].xyz + f * U.gmin.w, 0.0);
    }
}

// 3c) Flow energy density for the Particle-Aura energization (#247 in the Fluid tier).
// Writes the flow's **total quadratic energy density** into the velocity grid's `w`
// channel — kinetic energy density ½|u|² (bright in the jets/plumes/wake) + enstrophy
// density ½|ω|² (bright in the vortex cores) — so the motes glow by it exactly as the
// Maxwell path glows by EM energy density (both ride the grid's `w`; `cs_advect` samples
// it). The velocity xyz is untouched. `curlb.w` already carries |ω| (cs_curl, re-run on
// the projected field just before this). Only dispatched when energization is on, so the
// non-energized path leaves `w = 0` (byte-identical).
@compute @workgroup_size(64)
fn cs_energy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let v = vel_a[i].xyz;
    let wmag = curlb[i].w; // |ω|
    let e = 0.5 * dot(v, v) + 0.5 * wmag * wmag;
    vel_a[i] = vec4<f32>(v, e);
}

// 4) Divergence term for the pressure solve: 0.5·h·(∂vx/∂x + ∂vy/∂y + ∂vz/∂z) raw.
// #182 Tier 2: solid cells carry no divergence (the wall isn't a source/sink the
// solve should try to correct).
@compute @workgroup_size(64)
fn cs_divergence(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    if (solid(i)) {
        divg[i] = 0.0;
        return;
    }
    let xyz = decode(i);
    let x = xyz.x; let y = xyz.y; let z = xyz.z;
    let raw = (vel_at(x + 1, y, z).x - vel_at(x - 1, y, z).x)
            + (vel_at(x, y + 1, z).y - vel_at(x, y - 1, z).y)
            + (vel_at(x, y, z + 1).z - vel_at(x, y, z - 1).z);
    divg[i] = 0.5 * hh() * raw;
}

// Neighbour pressure with the Neumann (no-flux) wall condition (#182 Tier 2):
// a solid neighbour mirrors the centre pressure, so no gradient forms across a
// wall face and the projection can't push flow into the geometry.
fn p_at(x: i32, y: i32, z: i32, center: f32) -> f32 {
    let j = cidx(x, y, z);
    if (solid(j)) {
        return center;
    }
    return p_src[j];
}

// 5) Jacobi iteration for ∇²p = ∇·v: p = (Σ neighbour p − div) / 6. Ping-ponged
// between p_src and p_dst by the host (the buffers swap each pass).
@compute @workgroup_size(64)
fn cs_jacobi(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let xyz = decode(i);
    let x = xyz.x; let y = xyz.y; let z = xyz.z;
    let c = p_src[i];
    let s = p_at(x + 1, y, z, c) + p_at(x - 1, y, z, c)
          + p_at(x, y + 1, z, c) + p_at(x, y - 1, z, c)
          + p_at(x, y, z + 1, c) + p_at(x, y, z - 1, c);
    p_dst[i] = (s - divg[i]) / 6.0;
}

// 6) Projection: subtract the pressure gradient → the field is divergence-free.
// #182 Tier 2: walls keep their wall velocity; wall-adjacent gradients use the
// Neumann-mirrored pressure so nothing is pushed through a wall face.
@compute @workgroup_size(64)
fn cs_project(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    if (solid(i)) {
        return;
    }
    let xyz = decode(i);
    let x = xyz.x; let y = xyz.y; let z = xyz.z;
    let c = p_src[i];
    let gx = (p_at(x + 1, y, z, c) - p_at(x - 1, y, z, c)) / (2.0 * hh());
    let gy = (p_at(x, y + 1, z, c) - p_at(x, y - 1, z, c)) / (2.0 * hh());
    let gz = (p_at(x, y, z + 1, c) - p_at(x, y, z - 1, c)) / (2.0 * hh());
    vel_a[i] = vec4<f32>(vel_a[i].xyz - vec3<f32>(gx, gy, gz), 0.0);
}

// ===========================================================================
// #182 Tier 1 — dye transport (run after the velocity solve, on the projected
// field). The RGB dye is what the Fluid Ink volume pass renders; density is
// derived from the colour (max component), so no 4th channel is carried.
// CPU mirrors: math::fluid_dye_advect / math::fluid_dye_maccormack.
// ===========================================================================

// Open domain boundary for the dye: a backtrace that lands outside the box
// samples clean water (zero) instead of the clamped edge cell. Without this,
// dye pushed against a domain face sticks and accumulates — the whole fluid
// AABB grows visible opaque walls of ink. (Half-cell tolerance: cell centres
// sit on 0..res−1, the physical faces at ±0.5 beyond.)
fn dye_outside(c: vec3<f32>) -> bool {
    let hi = vec3<f32>(U.res.xyz) - vec3<f32>(0.5);
    return any(c < vec3<f32>(-0.5)) || any(c > hi);
}

// Trilinear dye sample at cell-space coordinate `c` (clamped stencil, zero
// outside the open boundary). vec4: rgb = the dye colour, w = the heat scalar
// (#182 Tier 2 buoyancy) riding along.
fn sample_dye_a(c: vec3<f32>) -> vec4<f32> {
    if (dye_outside(c)) {
        return vec4<f32>(0.0);
    }
    let b = floor(c);
    let f = c - b;
    let ix = i32(b.x);
    let iy = i32(b.y);
    let iz = i32(b.z);
    let c00 = mix(dye_a[cidx(ix, iy, iz)], dye_a[cidx(ix + 1, iy, iz)], f.x);
    let c10 = mix(dye_a[cidx(ix, iy + 1, iz)], dye_a[cidx(ix + 1, iy + 1, iz)], f.x);
    let c01 = mix(dye_a[cidx(ix, iy, iz + 1)], dye_a[cidx(ix + 1, iy, iz + 1)], f.x);
    let c11 = mix(dye_a[cidx(ix, iy + 1, iz + 1)], dye_a[cidx(ix + 1, iy + 1, iz + 1)], f.x);
    let c0 = mix(c00, c10, f.y);
    let c1 = mix(c01, c11, f.y);
    return mix(c0, c1, f.z);
}
fn sample_dye_b(c: vec3<f32>) -> vec4<f32> {
    if (dye_outside(c)) {
        return vec4<f32>(0.0);
    }
    let b = floor(c);
    let f = c - b;
    let ix = i32(b.x);
    let iy = i32(b.y);
    let iz = i32(b.z);
    let c00 = mix(dye_b[cidx(ix, iy, iz)], dye_b[cidx(ix + 1, iy, iz)], f.x);
    let c10 = mix(dye_b[cidx(ix, iy + 1, iz)], dye_b[cidx(ix + 1, iy + 1, iz)], f.x);
    let c01 = mix(dye_b[cidx(ix, iy, iz + 1)], dye_b[cidx(ix + 1, iy, iz + 1)], f.x);
    let c11 = mix(dye_b[cidx(ix, iy + 1, iz + 1)], dye_b[cidx(ix + 1, iy + 1, iz + 1)], f.x);
    let c0 = mix(c00, c10, f.y);
    let c1 = mix(c01, c11, f.y);
    return mix(c0, c1, f.z);
}

// Cap on per-cell dye so continuous injection can't run away; the cap preserves
// hue (uniform scale), extinction handles the rest visually.
const MAX_DYE: f32 = 10.0;

// D1) Inject: the CPU-splatted node dye (colour × ball kernel) accumulates at
// `rate` per second. Additive, then hue-preserving cap. #182 Tier 2: heat is
// injected with the dye (w += the injected density) so hot fresh ink drives
// buoyancy, then cools on its own decay clock.
@compute @workgroup_size(64)
fn cs_dye_inject(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let dt = U.gmin.w;
    let rate = U.p1.y;
    let inj = dye_src[i].xyz * (rate * dt);
    var d = dye_a[i].xyz + inj;
    let m = max(d.x, max(d.y, d.z));
    if (m > MAX_DYE) {
        d = d * (MAX_DYE / m);
    }
    let heat = min(dye_a[i].w + max(inj.x, max(inj.y, inj.z)), MAX_DYE);
    dye_a[i] = vec4<f32>(d, heat);
}

// D2) Semi-Lagrangian dye advection along the PROJECTED velocity, with its own
// dissipation clock (independent of the velocity's). Writes dye_b; the host
// copies back (or runs the MacCormack correction first). The heat scalar (w)
// advects identically but fades on the heat-decay clock (#182 Tier 2).
@compute @workgroup_size(64)
fn cs_dye_advect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let dt = U.gmin.w;
    let xyz = decode(i);
    let v = vel_a[i].xyz;
    let c_prev = vec3<f32>(xyz) - v * (dt / hh());
    let fade = clamp(1.0 - U.p1.z * dt, 0.0, 1.0);
    let fade_heat = clamp(1.0 - U.p2.y * dt, 0.0, 1.0);
    let s = sample_dye_a(c_prev);
    dye_b[i] = vec4<f32>(s.xyz * fade, s.w * fade_heat);
}

// D3) MacCormack / BFECC error correction (quality toggle): reverse-advect the
// semi-Lagrangian result (dye_b) forward, take half the round-trip error as a
// correction, and clamp to the 8-cell source neighbourhood (no new extrema —
// sharp filaments instead of trilinear mush, and it can't ring or go negative).
// Reads dye_a (φⁿ) + dye_b (φ̂ⁿ⁺¹), writes dye_c; the host copies dye_c → dye_a.
@compute @workgroup_size(64)
fn cs_dye_mac(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let dt = U.gmin.w;
    let xyz = decode(i);
    let here = vec3<f32>(xyz);
    let v = vel_a[i].xyz;
    let scale = dt / hh();
    let c_back = here - v * scale;
    let c_fwd = here + v * scale;
    let back_again = sample_dye_b(c_fwd);
    // vec4: the heat scalar (w) is corrected + limited the same way (#182 T2).
    var d = dye_b[i] + 0.5 * (dye_a[i] - back_again);
    // Limiter: min/max of the 8 source cells around the backtrace point.
    // Open boundary here too: a stencil cell OUTSIDE the box is clean water
    // (zero), not the clamped edge cell — otherwise the limiter window at an
    // inflow face never contains 0 and pins dye back onto the wall, partially
    // recreating the "ink walls" the open-boundary advection removed.
    let b = floor(c_back);
    let ix = i32(b.x);
    let iy = i32(b.y);
    let iz = i32(b.z);
    var lo = vec4<f32>(1e30);
    var hi = vec4<f32>(-1e30);
    for (var dz = 0; dz < 2; dz = dz + 1) {
        for (var dy = 0; dy < 2; dy = dy + 1) {
            for (var dx = 0; dx < 2; dx = dx + 1) {
                let cx = ix + dx;
                let cy = iy + dy;
                let cz = iz + dz;
                var s = vec4<f32>(0.0);
                if (cx >= 0 && cy >= 0 && cz >= 0
                    && cx < i32(U.res.x) && cy < i32(U.res.y) && cz < i32(U.res.z)) {
                    s = dye_a[cidx(cx, cy, cz)];
                }
                lo = min(lo, s);
                hi = max(hi, s);
            }
        }
    }
    d = clamp(d, lo, hi);
    dye_c[i] = d;
}

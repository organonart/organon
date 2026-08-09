// MLS-MPM liquid (#182 Tier 3) — the GPU port of `math::MpmSim` (the CPU
// mirror carries the unit tests; keep the arithmetic in lockstep).
//
// Moving-Least-Squares Material Point Method (Hu et al. 2018), weakly
// compressible fluid. Per substep:
//   cs_p2g  — particles scatter momentum + mass to the grid (quadratic
//             B-spline weights) via FIXED-POINT INTEGER atomics (the VXGI
//             pattern; integer adds commute exactly, so the sim stays
//             deterministic despite the scatter).
//   cs_grid — nodes normalise momentum, apply gravity, node colliders (the
//             generator as moving no-slip obstacles) and container walls,
//             consuming + re-zeroing the atomics (atomicExchange).
//   cs_g2p  — particles gather velocity + the APIC affine C, advect, update
//             the volume ratio J, and clamp into the container.
// Then once per frame:
//   cs_splat_density / cs_resolve_field — splat particle density into the
//             liquid's metaball-style 3D field texture (rgb = liquid colour,
//             a = density); the EXISTING metaball isosurface raymarch
//             (metaball.wgsl) draws it with the full material stack — set
//             Material = Glass for water.
//
// The sim works in GRID UNITS (dx = 1, positions in [0, res)); world mapping
// happens only at the edges (colliders arrive in world velocity and are
// divided by the cell size h; the density field shares the container AABB).

struct LiquidU {
    res: vec4<u32>,  // xyz = sim grid res, w = particle count
    fres: vec4<u32>, // xyz = density field res, w unused
    box0: vec4<f32>, // container min (world), w = cell size h (world)
    box1: vec4<f32>, // container max (world), w = dt (one substep, seconds)
    phys: vec4<f32>, // gravity (grid units/s²), stiffness, viscosity, open_top
    misc: vec4<f32>, // stir gain, collide_on, splat scale, container shape
    color: vec4<f32>,// liquid rgb (the field's albedo), w = render reveal
};

struct Particle {
    pos_j: vec4<f32>, // xyz = position (grid units), w = volume ratio J
    vel: vec4<f32>,   // xyz = velocity (grid units / s)
    c0: vec4<f32>,    // APIC affine C, columns (matches glam Mat3 from_cols)
    c1: vec4<f32>,
    c2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> U: LiquidU;
@group(0) @binding(1) var<storage, read_write> parts: array<Particle>;
// 4 fixed-point channels per node: momentum xyz + mass.
@group(0) @binding(2) var<storage, read_write> grid_a: array<atomic<i32>>;
// Resolved node velocity (xyz) + mass (w).
@group(0) @binding(3) var<storage, read_write> grid_v: array<vec4<f32>>;
// Node colliders: xyz = collider velocity (WORLD units), w > 0.5 = solid.
@group(0) @binding(4) var<storage, read> colliders: array<vec4<f32>>;
// Density splat accumulator (fixed-point u32, per FIELD cell).
@group(0) @binding(5) var<storage, read_write> dens_a: array<atomic<u32>>;
// The liquid's metaball-style field texture the isosurface raymarch samples.
@group(0) @binding(6) var field_out: texture_storage_3d<rgba16float, write>;
// #247 Tier 3 (energy → liquid): a per-field-cell HDR ember-glow grid the Maxwell
// energization splats into (bright where the field energy is high). Added into the
// field's rgb at resolve so the surface albedo carries the energy pattern; the
// metaball raymarch then emits the HDR excess. Only read when `U.fres.w != 0`.
@group(0) @binding(7) var<storage, read> energy_glow: array<vec4<f32>>;

// Fixed-point scales. Momentum: ±32k units of headroom in an i32; weights are
// ≤ 0.75³ so per-particle contributions are small and sums stay far from the
// edge at realistic densities. Density: 256 = 8-bit fraction.
const FP: f32 = 32768.0;
const DFP: f32 = 256.0;
// Wall margin in cells — mirrors math::MPM_BOUND.
const BOUND: f32 = 2.0;
// Boundless-shell inward pull (grid cells/s²) — mirrors math::MPM_SOFT_PULL.
const SOFT_PULL: f32 = 30.0;

fn count() -> u32 { return U.res.w; }
fn ncells() -> u32 { return U.res.x * U.res.y * U.res.z; }
fn nfield() -> u32 { return U.fres.x * U.fres.y * U.fres.z; }

fn gidx(x: i32, y: i32, z: i32) -> u32 {
    let r = vec3<i32>(U.res.xyz);
    let c = clamp(vec3<i32>(x, y, z), vec3<i32>(0), r - vec3<i32>(1));
    return u32((c.z * r.y + c.y) * r.x + c.x);
}

// Quadratic B-spline weights (per component) around fx ∈ [0.5, 1.5].
fn bspline(fx: vec3<f32>, k: i32) -> vec3<f32> {
    if (k == 0) {
        let t = vec3<f32>(1.5) - fx;
        return 0.5 * t * t;
    }
    if (k == 1) {
        let t = fx - vec3<f32>(1.0);
        return vec3<f32>(0.75) - t * t;
    }
    let t = fx - vec3<f32>(0.5);
    return 0.5 * t * t;
}

// ===================== P2G: scatter momentum + mass =====================
@compute @workgroup_size(64)
fn cs_p2g(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    let p = parts[i];
    let x = p.pos_j.xyz;
    let base = floor(x - vec3<f32>(0.5));
    let fx = x - base;
    let dt = U.box1.w;
    // mpm88 EOS: linear in (J − 1); dt + the 4/dx² MLS factor folded in.
    // p_vol = (dx/2)³ = 0.125 at dx = 1; p_mass = 1.
    let stress = -dt * 4.0 * U.phys.y * 0.125 * (p.pos_j.w - 1.0);
    for (var dz = 0; dz < 3; dz = dz + 1) {
        for (var dy = 0; dy < 3; dy = dy + 1) {
            for (var dx = 0; dx < 3; dx = dx + 1) {
                let node = base + vec3<f32>(f32(dx), f32(dy), f32(dz));
                let dpos = node - x;
                let wt = bspline(fx, dx).x * bspline(fx, dy).y * bspline(fx, dz).z;
                // affine·dpos with affine = stress·I + C (mass 1).
                let adp = stress * dpos
                    + p.c0.xyz * dpos.x + p.c1.xyz * dpos.y + p.c2.xyz * dpos.z;
                let mv = p.vel.xyz + adp;
                let gi4 = 4u * gidx(i32(node.x), i32(node.y), i32(node.z));
                atomicAdd(&grid_a[gi4 + 0u], i32(mv.x * wt * FP));
                atomicAdd(&grid_a[gi4 + 1u], i32(mv.y * wt * FP));
                atomicAdd(&grid_a[gi4 + 2u], i32(mv.z * wt * FP));
                atomicAdd(&grid_a[gi4 + 3u], i32(wt * FP));
            }
        }
    }
}

// ============ grid: normalise, gravity, colliders, walls (+ reset) ============
@compute @workgroup_size(64)
fn cs_grid(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gi = gid.x;
    if (gi >= ncells()) { return; }
    let gi4 = 4u * gi;
    // Consume + re-zero the fixed-point accumulators for the next substep.
    let mvx = f32(atomicExchange(&grid_a[gi4 + 0u], 0)) / FP;
    let mvy = f32(atomicExchange(&grid_a[gi4 + 1u], 0)) / FP;
    let mvz = f32(atomicExchange(&grid_a[gi4 + 2u], 0)) / FP;
    let m = f32(atomicExchange(&grid_a[gi4 + 3u], 0)) / FP;
    if (m <= 0.0) {
        grid_v[gi] = vec4<f32>(0.0);
        return;
    }
    var v = vec3<f32>(mvx, mvy, mvz) / m;
    let dt = U.box1.w;
    v.y = v.y - dt * U.phys.x;
    // Node colliders: the generator's nodes are moving no-slip obstacles that
    // churn the liquid. Collider velocity arrives in WORLD units → grid units.
    if (U.misc.y > 0.5) {
        let occ = colliders[gi];
        if (occ.w > 0.5) {
            v = occ.xyz * (U.misc.x / max(U.box0.w, 1e-5));
        }
    }
    // Container walls, by shape (misc.w) — mirrors math::mpm_wall 1:1.
    let rx = U.res.x;
    let pos = vec3<f32>(
        f32(gi % rx),
        f32((gi / rx) % U.res.y),
        f32(gi / (rx * U.res.y)),
    );
    let hi = f32(rx) - 1.0 - BOUND;
    let c = vec3<f32>((f32(rx) - 1.0) * 0.5);
    let rb = (f32(rx) - 1.0) * 0.5 - BOUND;
    let shape = u32(U.misc.w);
    let open_top = U.phys.w > 0.5;
    if (shape == 1u) {
        // Sphere: free-slip radial shell — kill only the outward component so
        // the liquid swirls along the bowl. Nodes BEYOND the shell also get a
        // steady inward push, so material that starts outside (the box-seeded
        // pool's corners, or a live shape switch) migrates in even at zero
        // gravity. open_top opens the upper cap.
        let d = pos - c;
        let r = length(d);
        if (r > rb && r > 1e-5) {
            let n = d / r;
            if (!(open_top && n.y > 0.5)) {
                v = v - n * (max(dot(v, n), 0.0) + SOFT_PULL * dt);
            }
        }
    } else if (shape == 2u) {
        // Cylinder (vertical axis): free-slip round wall + box floor/ceiling,
        // with the same inward push beyond the wall as the sphere.
        let dx = pos.x - c.x;
        let dz = pos.z - c.z;
        let r = sqrt(dx * dx + dz * dz);
        if (r > rb && r > 1e-5) {
            let nx = dx / r;
            let nz = dz / r;
            let vn = max(v.x * nx + v.z * nz, 0.0) + SOFT_PULL * dt;
            v.x = v.x - nx * vn;
            v.z = v.z - nz * vn;
        }
        if ((pos.y < BOUND && v.y < 0.0) || (pos.y > hi && v.y > 0.0 && !open_top)) {
            v.y = 0.0;
        }
    } else if (shape == 3u) {
        // Boundless: no hard wall — an absorbing shell fades the outward
        // component to zero by rb and gently pulls strays back inward, so the
        // density trails off into space with no face to plate against.
        let d = pos - c;
        let r = length(d);
        let r0 = rb * 0.55;
        if (r > r0 && r > 1e-5) {
            let n = d / r;
            let t = clamp((r - r0) / max(rb - r0, 1e-5), 0.0, 1.0);
            let w = t * t * (3.0 - 2.0 * t);
            let vn = max(dot(v, n), 0.0);
            v = v - n * (vn * w + SOFT_PULL * dt * w);
        }
    } else {
        // Box (default): per-axis clamps at each face (+y skipped when open).
        if ((pos.x < BOUND && v.x < 0.0) || (pos.x > hi && v.x > 0.0)) { v.x = 0.0; }
        if ((pos.y < BOUND && v.y < 0.0) || (pos.y > hi && v.y > 0.0 && !open_top)) {
            v.y = 0.0;
        }
        if ((pos.z < BOUND && v.z < 0.0) || (pos.z > hi && v.z > 0.0)) { v.z = 0.0; }
    }
    // Backstop for the round/soft shapes: never push through the grid edge
    // itself (particle positions clamp there in G2P).
    if (shape != 0u) {
        if ((pos.x < BOUND && v.x < 0.0) || (pos.x > hi && v.x > 0.0)) { v.x = 0.0; }
        if (pos.y < BOUND && v.y < 0.0) { v.y = 0.0; }
        if ((pos.z < BOUND && v.z < 0.0) || (pos.z > hi && v.z > 0.0)) { v.z = 0.0; }
    }
    grid_v[gi] = vec4<f32>(v, m);
}

// ===================== G2P: gather, advect, update J =====================
@compute @workgroup_size(64)
fn cs_g2p(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    var p = parts[i];
    let x = p.pos_j.xyz;
    let base = floor(x - vec3<f32>(0.5));
    let fx = x - base;
    var v_new = vec3<f32>(0.0);
    var b0 = vec3<f32>(0.0);
    var b1 = vec3<f32>(0.0);
    var b2 = vec3<f32>(0.0);
    for (var dz = 0; dz < 3; dz = dz + 1) {
        for (var dy = 0; dy < 3; dy = dy + 1) {
            for (var dx = 0; dx < 3; dx = dx + 1) {
                let node = base + vec3<f32>(f32(dx), f32(dy), f32(dz));
                let dpos = node - x;
                let wt = bspline(fx, dx).x * bspline(fx, dy).y * bspline(fx, dz).z;
                let gv = grid_v[gidx(i32(node.x), i32(node.y), i32(node.z))].xyz;
                v_new = v_new + gv * wt;
                // B += wt · gv ⊗ dpos (accumulated as columns).
                b0 = b0 + gv * (dpos.x * wt);
                b1 = b1 + gv * (dpos.y * wt);
                b2 = b2 + gv * (dpos.z * wt);
            }
        }
    }
    let dt = U.box1.w;
    let damp = 4.0 * (1.0 - clamp(U.phys.z, 0.0, 1.0));
    let c0 = b0 * damp;
    let c1 = b1 * damp;
    let c2 = b2 * damp;
    // Volume ratio integrates the velocity divergence (trace of C).
    let tr = c0.x + c1.y + c2.z;
    let j = clamp(p.pos_j.w * (1.0 + dt * tr), 0.2, 5.0);
    let lo = vec3<f32>(BOUND);
    var hi = vec3<f32>(f32(U.res.x) - 1.0 - BOUND);
    // Open top: raise the +y ceiling to the grid edge so particles splash out
    // past the interior wall (velocity clamp already lets +y through). gidx
    // clamps indices, so this stays in range; other faces keep the wall.
    if (U.phys.w > 0.5) { hi.y = f32(U.res.x) - 1.0; }
    let np = clamp(x + v_new * dt, lo, hi);
    p.pos_j = vec4<f32>(np, j);
    p.vel = vec4<f32>(v_new, 0.0);
    p.c0 = vec4<f32>(c0, 0.0);
    p.c1 = vec4<f32>(c1, 0.0);
    p.c2 = vec4<f32>(c2, 0.0);
    parts[i] = p;
}

// ============ density splat: particles → the isosurface field ============
@compute @workgroup_size(64)
fn cs_splat_density(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= count()) { return; }
    // Sim grid units → field cells (both span the container AABB).
    let scale = vec3<f32>(U.fres.xyz) / vec3<f32>(U.res.xyz);
    let pf = parts[i].pos_j.xyz * scale;
    let base = floor(pf - vec3<f32>(0.5));
    let fx = pf - base - vec3<f32>(0.5); // trilinear fraction to the 2³ corners
    let fr = vec3<i32>(U.fres.xyz);
    for (var dz = 0; dz < 2; dz = dz + 1) {
        for (var dy = 0; dy < 2; dy = dy + 1) {
            for (var dx = 0; dx < 2; dx = dx + 1) {
                let c = clamp(
                    vec3<i32>(base) + vec3<i32>(dx, dy, dz),
                    vec3<i32>(0),
                    fr - vec3<i32>(1),
                );
                let w = (select(1.0 - fx.x, fx.x, dx == 1))
                    * (select(1.0 - fx.y, fx.y, dy == 1))
                    * (select(1.0 - fx.z, fx.z, dz == 1));
                let fi = u32((c.z * fr.y + c.y) * fr.x + c.x);
                atomicAdd(&dens_a[fi], u32(max(w, 0.0) * DFP));
            }
        }
    }
}

@compute @workgroup_size(64)
fn cs_resolve_field(@builtin(global_invocation_id) gid: vec3<u32>) {
    let fi = gid.x;
    if (fi >= nfield()) { return; }
    var d = f32(atomicExchange(&dens_a[fi], 0u)) / DFP * U.misc.z;
    let fr = U.fres.xyz;
    let x = fi % fr.x;
    let y = (fi / fr.x) % fr.y;
    let z = fi / (fr.x * fr.y);
    // Render reveal (color.w, 0..1): a soft SPHERICAL window on the density —
    // the ink's reveal, spatially. Fading the field (not clipping the march)
    // means the isosurface closes into smooth blobby trailing edges instead
    // of wall-flattened planes, so the wall geometry never reads. 0 = off.
    let reveal = U.color.w;
    if (reveal > 0.0) {
        let fc = (vec3<f32>(fr) - 1.0) * 0.5;
        // Normalised radius: 1 at the face centres, ~1.73 at the box corners.
        let q = length((vec3<f32>(vec3<u32>(x, y, z)) - fc) / max(fc, vec3<f32>(1e-5)));
        let q_out = mix(1.75, 0.35, clamp(reveal, 0.0, 1.0));
        d = d * (1.0 - smoothstep(q_out * 0.7, q_out, q));
    }
    // #247 Tier 3: fold in the HDR energy-ember glow (bright at strong-field cells),
    // so the surface albedo carries the field pattern. Off (fres.w = 0) → byte-identical.
    var rgb = U.color.rgb;
    if (U.fres.w != 0u) {
        rgb = rgb + energy_glow[fi].rgb;
    }
    textureStore(field_out, vec3<i32>(i32(x), i32(y), i32(z)),
                 vec4<f32>(rgb, d));
}

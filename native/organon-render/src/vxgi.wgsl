// Organic Math — voxel global illumination (#152 Tier 3, #10).
//
// Two passes, both self-contained (this never touches cube.wgsl or the composite):
//
//   1. cs_voxelize — a compute pass that gathers the (capped) node set into a small
//      3D colour+occupancy volume (a Gaussian splat per cell, like the metaball
//      field). This is the world-space scene proxy the GI marches — because it holds
//      the WHOLE field (not just what's on screen), the bounce captures light from
//      emissive/coloured cubes even when they're off-camera or occluded (SSGI can't).
//
//   2. fs_vxgi — a fullscreen pass that reconstructs each pixel's WORLD position from
//      the depth prepass, marches a few hemisphere rays through the volume, and
//      accumulates emission·occupancy with front-to-back transmittance (a poor-man's
//      cone trace — single LOD, no mip pyramid). The result is added straight into the
//      HDR scene buffer (additive blend, RGB-only write) so it blooms + tonemaps with
//      the scene. Noisy at low ray counts → pairs with TAA (#152 Tier 2).
//
// Off (intensity 0 / not enabled) → the pass isn't run, so the image is unchanged.

// ============================ 1) voxelize (compute) ============================

struct VoxU {
    bmin: vec4<f32>, // xyz = volume min, w = injected radiance scale (glow + key estimate)
    bmax: vec4<f32>, // xyz = volume max
    grid: vec4<f32>, // xyz = resolution, w = node count
    // Fluid → GI (#182 T4): the ink dye field and the MLS-MPM liquid density
    // are extra injection sources sampled per cell in cs_resolve. w lanes:
    // dye0.w = dye inject gain (0 = off), dye1.w = liquid inject gain.
    dye0: vec4<f32>, // xyz = dye AABB min
    dye1: vec4<f32>, // xyz = dye AABB max
    liq0: vec4<f32>, // xyz = liquid AABB min
    liq1: vec4<f32>, // xyz = liquid AABB max
};
struct Node {
    pos: vec4<f32>,   // xyz = world position, w = influence radius (unused here)
    color: vec4<f32>, // rgb = emitter colour
};
@group(0) @binding(0) var<uniform> vu: VoxU;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var vol_out: texture_storage_3d<rgba16float, write>;
// Scatter accumulator (#174 T2): 4 fixed-point u32 per cell (r, g, b, weight).
@group(0) @binding(3) var<storage, read_write> accum: array<atomic<u32>>;
// Fluid → GI (#182 T4) sample sources (always bound; gains gate the reads).
@group(0) @binding(4) var dye_tex: texture_3d<f32>;
@group(0) @binding(5) var liq_tex: texture_3d<f32>;
@group(0) @binding(6) var fl_samp: sampler;

// Fixed-point scale for the atomic accumulation.
const FIXED: f32 = 1024.0;
// Splat window half-width in cells (σ ≈ 1.56 cells, so ±3 covers ~2σ; the tail
// beyond contributes < e^-1.85 ≈ 0.16 per-node weight — negligible after the
// soft normalisation).
const SPLAT_R: i32 = 3;

// #174 T2: the voxelizer was a GATHER — every cell iterated every node
// (32³ × 2048 ≈ 67M exp() per frame). It is now a two-dispatch SCATTER:
// `cs_scatter` runs one thread per NODE, splatting its Gaussian into the ±3-cell
// window around it via fixed-point atomics (≈ 0.7M exp at the cap), then
// `cs_resolve` runs one thread per CELL, normalising exactly like the old gather
// (soft `Σwc·scale / (1 + Σw)` + occupancy) into the volume texture — and
// zeroing the accumulator for the next frame via atomicExchange.
@compute @workgroup_size(64)
fn cs_scatter(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = u32(vu.grid.w);
    if (gid.x >= n) {
        return;
    }
    let res = vec3<i32>(vu.grid.xyz);
    let pos = nodes[gid.x].pos.xyz;
    let col = max(nodes[gid.x].color.rgb, vec3<f32>(0.0));
    let ext = max(vu.bmax.xyz - vu.bmin.xyz, vec3<f32>(1e-6));
    let cell = ext / vu.grid.xyz;
    let sigma = max(length(cell) * 0.9, 1e-4);
    let inv_2s2 = 1.0 / (2.0 * sigma * sigma);
    // The node's continuous cell coordinate (cell centres at integer coords).
    let gc = (pos - vu.bmin.xyz) / ext * vu.grid.xyz - vec3<f32>(0.5);
    let ctr = vec3<i32>(round(gc));
    for (var dz = -SPLAT_R; dz <= SPLAT_R; dz = dz + 1) {
        for (var dy = -SPLAT_R; dy <= SPLAT_R; dy = dy + 1) {
            for (var dx = -SPLAT_R; dx <= SPLAT_R; dx = dx + 1) {
                let c = ctr + vec3<i32>(dx, dy, dz);
                if (any(c < vec3<i32>(0)) || any(c >= res)) {
                    continue;
                }
                let cp = mix(vu.bmin.xyz, vu.bmax.xyz, (vec3<f32>(c) + vec3<f32>(0.5)) / vu.grid.xyz);
                let d = cp - pos;
                let w = exp(-dot(d, d) * inv_2s2);
                if (w < 1e-3) {
                    continue;
                }
                let idx = ((u32(c.z) * u32(res.y) + u32(c.y)) * u32(res.x) + u32(c.x)) * 4u;
                atomicAdd(&accum[idx + 0u], u32(col.r * w * FIXED));
                atomicAdd(&accum[idx + 1u], u32(col.g * w * FIXED));
                atomicAdd(&accum[idx + 2u], u32(col.b * w * FIXED));
                atomicAdd(&accum[idx + 3u], u32(w * FIXED));
            }
        }
    }
}

@compute @workgroup_size(4, 4, 4)
fn cs_resolve(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = vec3<u32>(vu.grid.xyz);
    if (gid.x >= res.x || gid.y >= res.y || gid.z >= res.z) {
        return;
    }
    let idx = ((gid.z * res.y + gid.y) * res.x + gid.x) * 4u;
    // Exchange-to-zero: read this frame's sums and clear for the next frame.
    let r = f32(atomicExchange(&accum[idx + 0u], 0u)) / FIXED;
    let g = f32(atomicExchange(&accum[idx + 1u], 0u)) / FIXED;
    let b = f32(atomicExchange(&accum[idx + 2u], 0u)) / FIXED;
    let wsum = f32(atomicExchange(&accum[idx + 3u], 0u)) / FIXED;
    // Soft-normalised colour (empty space → ~0) + an occupancy in alpha (how much
    // "stuff" is in this cell, for the march's absorption). The colour is scaled by
    // the injected radiance estimate (bmin.w = glow + a key-light term, from the
    // CPU): a node's tint is its ALBEDO, not its emission — injecting it raw made a
    // completely unlit, non-emissive field "bounce" at full strength.
    var col = vec3<f32>(r, g, b) * vu.bmin.w / (1.0 + wsum);
    var occ = clamp(wsum, 0.0, 1.0);
    // Fluid → GI (#182 T4): sample the ink dye + the liquid density at this
    // cell's world centre and fold them in — glowing ink tints the bounce
    // light; the liquid occludes and colours it. Gains 0 → byte-identical.
    if (vu.dye0.w > 0.0 || vu.dye1.w > 0.0) {
        let cp = mix(vu.bmin.xyz, vu.bmax.xyz,
                     (vec3<f32>(gid) + vec3<f32>(0.5)) / max(vu.grid.xyz, vec3<f32>(1.0)));
        if (vu.dye0.w > 0.0) {
            let ext = max(vu.dye1.xyz - vu.dye0.xyz, vec3<f32>(1e-5));
            let uvw = (cp - vu.dye0.xyz) / ext;
            if (all(uvw >= vec3<f32>(0.0)) && all(uvw <= vec3<f32>(1.0))) {
                // rgb = dye colour × amount (radiance-ish already); density =
                // the channel max (the ink march's own convention).
                let d = textureSampleLevel(dye_tex, fl_samp, uvw, 0.0);
                let dens = max(d.r, max(d.g, d.b));
                col += d.rgb * vu.dye0.w;
                occ = clamp(occ + min(dens, 1.0) * vu.dye0.w * 0.5, 0.0, 1.0);
            }
        }
        if (vu.dye1.w > 0.0) {
            let ext = max(vu.liq1.xyz - vu.liq0.xyz, vec3<f32>(1e-5));
            let uvw = (cp - vu.liq0.xyz) / ext;
            if (all(uvw >= vec3<f32>(0.0)) && all(uvw <= vec3<f32>(1.0))) {
                // rgb = liquid albedo, a = splatted density (iso-field units,
                // surface sits around ~0.35): mostly an occluder that tints.
                let l = textureSampleLevel(liq_tex, fl_samp, uvw, 0.0);
                let dens = clamp(l.a, 0.0, 1.0);
                col += l.rgb * dens * vu.dye1.w * vu.bmin.w * 0.5;
                occ = clamp(occ + dens * vu.dye1.w, 0.0, 1.0);
            }
        }
    }
    textureStore(vol_out, vec3<i32>(gid), vec4<f32>(col, occ));
}

// ============================ 2) gather (fullscreen) ============================

struct GiU {
    inv_vp: mat4x4<f32>, // clip → world (the same matrix the depth was rendered with)
    bmin: vec4<f32>,     // xyz = volume min
    bmax: vec4<f32>,     // xyz = volume max
    params: vec4<f32>,   // intensity, rays, steps, max_dist (world units)
    texel: vec4<f32>,    // 1/w, 1/h, w, h
    cam: vec4<f32>,      // #163 T3: xyz = camera world position (reflection ray)
    spec: vec4<f32>,     // #163 T3: strength, aperture, max_dist, steps (0 strength → off)
};
@group(0) @binding(0) var<uniform> g: GiU;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var samp_n: sampler;          // nearest (depth)
@group(0) @binding(3) var vol_tex: texture_3d<f32>; // the voxelized volume
@group(0) @binding(4) var samp_l: sampler;          // linear (volume)

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 345.45));
    q = q + dot(q, q + 34.345);
    return fract(q.x * q.y);
}

fn world_from_depth(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, d, 1.0);
    let w = g.inv_vp * ndc;
    return w.xyz / w.w;
}

// Sample the volume at a world point; 0 outside the box.
fn sample_vol(p: vec3<f32>) -> vec4<f32> {
    let uvw = (p - g.bmin.xyz) / max(g.bmax.xyz - g.bmin.xyz, vec3<f32>(1e-3));
    if (uvw.x < 0.0 || uvw.x > 1.0 || uvw.y < 0.0 || uvw.y > 1.0 || uvw.z < 0.0 || uvw.z > 1.0) {
        return vec4<f32>(0.0);
    }
    return textureSampleLevel(vol_tex, samp_l, uvw, 0.0);
}

@fragment
fn fs_vxgi(in: VsOut) -> @location(0) vec4<f32> {
    let d = textureSampleLevel(depth_tex, samp_n, in.uv, 0);
    if (d >= 1.0) {
        return vec4<f32>(0.0); // sky — nothing to light
    }
    let p = world_from_depth(in.uv, d);
    // Normal from world-position derivatives (no G-buffer normal available). wgpu
    // framebuffer origin is TOP-left, so the correct winding is dpdy×dpdx — the GL
    // order flipped the gather hemisphere INTO the cube and turned the
    // anti-self-occlusion offset below into a self-ILLUMINATION offset.
    let n = normalize(cross(dpdy(p), dpdx(p)));

    // Hemisphere basis about n.
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.95) { up = vec3<f32>(1.0, 0.0, 0.0); }
    let tang = normalize(cross(up, n));
    let bit = cross(n, tang);

    let rays = i32(clamp(g.params.y, 1.0, 8.0));
    let steps = i32(clamp(g.params.z, 1.0, 32.0));
    let max_dist = max(g.params.w, 1e-3);
    let dt = max_dist / f32(steps);
    let base = hash21(in.uv * g.texel.zw);
    // Extinction per WORLD unit, normalized so one voxel cell of occupancy 1 is
    // roughly one optical depth (32 = the voxel grid resolution, vxgi.rs::VXGI_RES).
    // Treating occupancy as a per-STEP opacity made the accumulated energy scale
    // with the step count — the "steps" quality slider was secretly a brightness
    // slider, and the diffuse/specular marches (independent step counts) could
    // never be energy-matched.
    let ext_k = 32.0 / max(length(g.bmax.xyz - g.bmin.xyz), 1e-3);

    var gi = vec3<f32>(0.0);
    for (var r = 0; r < rays; r = r + 1) {
        // Cosine-ish hemisphere direction (jittered per ray + pixel; TAA denoises).
        let a = (base + f32(r) / f32(rays)) * 6.2831853;
        let rr = sqrt(fract(base * 13.0 + f32(r) * 0.37));
        let dl = vec3<f32>(cos(a) * rr, sin(a) * rr, sqrt(max(1.0 - rr * rr, 0.0)));
        let dir = normalize(tang * dl.x + bit * dl.y + n * dl.z);

        var trans = 1.0;
        var t = dt; // start one step off the surface
        for (var s = 0; s < steps; s = s + 1) {
            let sp = p + n * 0.02 + dir * t; // small normal offset kills self-occlusion
            let v = sample_vol(sp);
            let occ = clamp(v.a, 0.0, 1.0);
            // In-scatter with a step-length-integrated opacity (Beer–Lambert), so
            // total energy depends on the marched DISTANCE, not the step count.
            let alpha = 1.0 - exp(-occ * ext_k * dt);
            gi = gi + trans * v.rgb * alpha;
            trans = trans * (1.0 - alpha);
            if (trans < 0.02) {
                break;
            }
            t = t + dt;
        }
    }
    gi = gi / f32(rays) * g.params.x;

    // ---- specular reflection cone (#163 Tier 3) ----
    // Reflect the view ray about the surface normal and cone-march the SAME volume, so
    // the pixel picks up the actual scene behind/around it (other cubes, off-screen
    // emitters) — a world-space reflection with no screen-edge dropout. Added on top of
    // the diffuse bounce. strength 0 → skipped (today's look).
    var spec = vec3<f32>(0.0);
    let spec_strength = g.spec.x;
    if (spec_strength > 0.0) {
        let v = normalize(g.cam.xyz - p);
        let refl = reflect(-v, n);
        let ssteps = i32(clamp(g.spec.w, 1.0, 64.0));
        let aperture = clamp(g.spec.y, 0.0, 1.0);
        let step0 = max(g.spec.z, 1e-3) / f32(ssteps);
        var strans = 1.0;
        var st = step0; // one step off the surface
        var sacc = vec3<f32>(0.0);
        for (var s = 0; s < ssteps; s = s + 1) {
            let sp = p + n * 0.02 + refl * st;
            let sv = sample_vol(sp);
            let socc = clamp(sv.a, 0.0, 1.0);
            // Widening cone without a mip pyramid: grow the step with distance, scaled
            // by aperture (0 = tight/mirror-ish, 1 = fast-widening/glossy).
            let seg = step0 * (1.0 + aperture * f32(s) * 0.5);
            // Same step-length-integrated opacity as the diffuse march, using each
            // segment's ACTUAL length, so the widening steps stay energy-consistent.
            let alpha = 1.0 - exp(-socc * ext_k * seg);
            sacc = sacc + strans * sv.rgb * alpha;
            strans = strans * (1.0 - alpha);
            if (strans < 0.02) {
                break;
            }
            st = st + seg;
        }
        spec = sacc * spec_strength;
    }

    return vec4<f32>(gi + spec, 0.0); // RGB added into the HDR buffer; alpha write masked off
}

// Organic Math — voxel field splatter (compute).
//
// Eulerian counterpart to the Lagrangian cube field: instead of drawing a cube
// per node at its continuous position, we SPLAT the node point-set into a fixed
// 3D lattice (occupancy + colour), then DDA-raymarch crisp grid-snapped voxels
// from it (voxel.wgsl). One splat path serves every generator, because every
// generator already reduces to nodes (position + colour).
//
// Each voxel gathers the nodes' falloff contributions: density = Σ wᵢ (a smooth
// kernel of radius rᵢ), colour = Σ wᵢ·colourᵢ / Σ wᵢ (the density-weighted blend,
// so the palette / HSV sweep carries straight through). The raymarch then keeps
// cells whose density crosses the fill threshold — the snapping to a shared grid
// is what makes neighbours line up into faces (the whole "voxel" charm).
//
// Output is an `rgba16float` 3D texture: rgb = blended albedo, a = density.

struct FieldU {
    bmin: vec4<f32>, // xyz = grid min (world), w unused
    bmax: vec4<f32>, // xyz = grid max (world), w = edge sharpness
    grid: vec4<u32>, // xyz = resolution, w = node count
};

struct Node {
    pos: vec4<f32>,   // xyz = world position, w = influence radius
    color: vec4<f32>, // rgb = colour, a unused
};

@group(0) @binding(0) var<uniform> fu: FieldU;
@group(0) @binding(1) var<storage, read> nodes: array<Node>;
@group(0) @binding(2) var field_out: texture_storage_3d<rgba16float, write>;

@compute @workgroup_size(4, 4, 4)
fn cs_splat(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = fu.grid.xyz;
    if (gid.x >= res.x || gid.y >= res.y || gid.z >= res.z) {
        return;
    }

    let bmin = fu.bmin.xyz;
    let bmax = fu.bmax.xyz;
    // Voxel centre in world space.
    let uvw = (vec3<f32>(gid) + vec3<f32>(0.5)) / vec3<f32>(res);
    let wpos = bmin + uvw * (bmax - bmin);

    let n = fu.grid.w;
    let sharp = max(fu.bmax.w, 0.05);

    var dens = 0.0;
    var col = vec3<f32>(0.0);
    let exponent = 1.0 + sharp * 3.0;
    for (var i = 0u; i < n; i = i + 1u) {
        let nd = nodes[i];
        let r = max(nd.pos.w, 1e-4);
        let dd = wpos - nd.pos.xyz;
        let q = dot(dd, dd) / (r * r); // (distance / radius)²
        // Out of this node's radius → zero contribution. Skip the pow() before it
        // even runs: most nodes are far from any given voxel, and a 4×4×4 voxel
        // workgroup is spatially tight, so the reject is subgroup-coherent — this
        // turns the O(voxels·nodes) splat from "pow() per pair" into "pow() only
        // for the few nodes that actually touch the cell".
        if (q >= 1.0) {
            continue;
        }
        let x = 1.0 - q;
        // Wyvill-style smooth kernel; `sharp` tightens the falloff toward the rim
        // (crisper voxel walls) without changing the radius the strand fills.
        let w = pow(x, exponent);
        dens = dens + w;
        col = col + nd.color.rgb * w;
    }

    var outc = vec3<f32>(0.0);
    if (dens > 1e-5) {
        outc = col / dens;
    }
    textureStore(field_out, vec3<i32>(gid), vec4<f32>(outc, dens));
}

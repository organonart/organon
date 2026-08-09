// Organic Math — metaball field voxelizer (compute).
//
// Bakes the node point-set into a 3D scalar+colour field so the raymarch
// (metaball.wgsl) is O(pixels·steps) instead of O(pixels·steps·nodes). Each
// voxel gathers the nodes' metaball contributions: density = Σ wᵢ (a smooth
// falloff kernel), colour = Σ wᵢ·colourᵢ. The stored colour is the density-
// weighted average (colour / density), so nearby nodes blend their tints — the
// HSV / palette sweep carries straight through into the merged skin.
//
// Output is an `rgba16float` 3D texture: rgb = blended colour, a = density.

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
fn cs_voxelize(@builtin(global_invocation_id) gid: vec3<u32>) {
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
    for (var i = 0u; i < n; i = i + 1u) {
        let nd = nodes[i];
        let r = max(nd.pos.w, 1e-4);
        let dd = wpos - nd.pos.xyz;
        let q = dot(dd, dd) / (r * r); // (distance / radius)²
        let x = clamp(1.0 - q, 0.0, 1.0);
        // Wyvill-style smooth kernel; `sharp` tightens the falloff toward the rim.
        let w = pow(x, 1.0 + sharp * 3.0);
        dens = dens + w;
        col = col + nd.color.rgb * w;
    }

    var outc = vec3<f32>(0.0);
    if (dens > 1e-5) {
        outc = col / dens;
    }
    textureStore(field_out, vec3<i32>(gid), vec4<f32>(outc, dens));
}

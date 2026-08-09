// Organic Math — voxel GI mip downsample (#89).
//
// Builds the mip pyramid of the splatted voxel field (voxelize.wgsl's output:
// rgb = density-weighted albedo, a = density/coverage) so the raymarch can
// cone-trace it for bounced colour (voxel.wgsl). One dispatch per mip level,
// each averaging the 2×2×2 children of the level below — averaged albedo = the
// region's mean colour, averaged density = its coverage, which is exactly what a
// voxel cone needs at each step. No uniform: the level sizes come from
// `textureDimensions`, so the same pipeline drives every level.

@group(0) @binding(0) var src: texture_3d<f32>;            // previous (finer) mip
@group(0) @binding(1) var dst: texture_storage_3d<rgba16float, write>; // this (coarser) mip

@compute @workgroup_size(4, 4, 4)
fn cs_downsample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dres = textureDimensions(dst);
    if (gid.x >= dres.x || gid.y >= dres.y || gid.z >= dres.z) {
        return;
    }
    let sres = vec3<i32>(textureDimensions(src));
    let b = vec3<i32>(gid) * 2;

    var acc = vec4<f32>(0.0);
    for (var dz = 0; dz < 2; dz = dz + 1) {
        for (var dy = 0; dy < 2; dy = dy + 1) {
            for (var dx = 0; dx < 2; dx = dx + 1) {
                let c = clamp(b + vec3<i32>(dx, dy, dz), vec3<i32>(0), sres - vec3<i32>(1));
                acc = acc + textureLoad(src, c, 0);
            }
        }
    }
    textureStore(dst, vec3<i32>(gid), acc * 0.125);
}

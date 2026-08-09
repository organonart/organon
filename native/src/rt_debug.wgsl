// Hardware-RT debug view (#195 Tier 0). A fullscreen ray query against the
// TLAS built from the instanced field, drawn OVER the final frame so the
// acceleration structure can be verified against the raster scene: if the
// silhouettes line up, the TLAS matches what the raster pipelines drew.
// Views: 1 = geometric normals (screen-derivative of the hit point),
// 2 = instance-index hash colour, 3 = hit distance. Miss = a dim checker so
// it reads unambiguously as a debug overlay, not a broken render.

enable wgpu_ray_query;

struct DebugU {
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>, // xyz = camera world pos (w unused)
    params: vec4<f32>,  // x = view mode (1/2/3), y = t_max, z/w unused
};

@group(0) @binding(0) var<uniform> u: DebugU;
@group(0) @binding(1) var tlas: acceleration_structure;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

// Stable pseudo-random colour per instance index, so neighbouring instances
// contrast (a plain ramp makes adjacent grid nodes near-identical).
fn hash_color(i: u32) -> vec3<f32> {
    var h = i * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    let r = f32(h & 1023u) / 1023.0;
    let g = f32((h >> 10u) & 1023u) / 1023.0;
    let b = f32((h >> 20u) & 1023u) / 1023.0;
    return vec3<f32>(0.15) + 0.85 * vec3<f32>(r, g, b);
}

// The hit world position for this fragment; the ray parameters are smooth
// across the screen, so dpdx/dpdy of this give the surface tangent plane and
// their cross is the geometric normal — no vertex fetch needed.
fn hit_pos(uv: vec2<f32>) -> vec4<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    let p0 = u.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let p1 = u.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let origin = p0.xyz / p0.w;
    let dir = normalize(p1.xyz / p1.w - origin);
    var rq: ray_query;
    rayQueryInitialize(&rq, tlas, RayDesc(0x1u /* FORCE_OPAQUE */, 0xFFu, 1e-3, u.params.y, origin, dir));
    loop {
        if (!rayQueryProceed(&rq)) { break; }
    }
    let hit = rayQueryGetCommittedIntersection(&rq);
    if (hit.kind == RAY_QUERY_INTERSECTION_TRIANGLE) {
        return vec4<f32>(origin + dir * hit.t, f32(hit.instance_custom_data) + 0.5);
    }
    return vec4<f32>(0.0, 0.0, 0.0, -1.0); // w < 0 = miss
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Evaluate the hit unconditionally (uniform control flow) so dpdx/dpdy of
    // the hit position are well-defined for the normals view.
    let hp = hit_pos(in.uv);
    let nrm = normalize(cross(dpdy(hp.xyz), dpdx(hp.xyz)));
    if (hp.w < 0.0) {
        // Miss: a dim checker keyed off the pixel grid.
        let c = f32((u32(in.pos.x / 16.0) + u32(in.pos.y / 16.0)) & 1u);
        return vec4<f32>(vec3<f32>(0.03 + 0.02 * c), 1.0);
    }
    let mode = u32(u.params.x);
    var col = vec3<f32>(1.0, 0.0, 1.0);
    if (mode == 1u) {
        col = nrm * 0.5 + 0.5;
    } else if (mode == 2u) {
        col = hash_color(u32(hp.w));
    } else {
        // Distance: near = bright, fading with world distance from the camera.
        let d = length(hp.xyz - u.cam_pos.xyz);
        col = vec3<f32>(1.0 / (1.0 + d * 0.08));
    }
    return vec4<f32>(col, 1.0);
}

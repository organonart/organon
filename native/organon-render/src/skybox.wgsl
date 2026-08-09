// Background skybox: fullscreen triangle, reconstruct world view ray from
// inverse(view_proj), sample the env equirect with rotation + exposure, ACES
// tonemap, output LINEAR (surface is sRGB → hardware encodes). Writes far depth
// so the cube pass (depth_compare Less) draws over it.

struct SkyUniforms {
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,   // xyz = camera world position
    params: vec4<f32>,    // exposure, env_intensity, env_rotation_radians, bg_brightness
    env_tint: vec4<f32>,  // xyz = background tint colour (white = none), w unused
};
@group(0) @binding(0) var<uniform> sky: SkyUniforms;
@group(1) @binding(0) var env_tex: texture_2d<f32>;
@group(1) @binding(1) var env_samp: sampler;

const INV_ATAN: vec2<f32> = vec2<f32>(0.15915494, 0.31830989); // (1/2π, 1/π)

fn rotate_y(d: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a); let s = sin(a);
    return vec3<f32>(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
}
fn dir_to_equirect_uv(d: vec3<f32>) -> vec2<f32> {
    let nd = normalize(d);
    var uv = vec2<f32>(atan2(nd.z, nd.x), asin(clamp(nd.y, -1.0, 1.0)));
    uv = uv * INV_ATAN + vec2<f32>(0.5, 0.5);
    return uv;
}

struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) ndc: vec2<f32> };
@vertex
fn vs_sky(@builtin(vertex_index) vid: u32) -> VsOut {
    let uv = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    let p = uv * 2.0 - vec2<f32>(1.0, 1.0);
    var out: VsOut;
    out.clip = vec4<f32>(p, 1.0, 1.0); // far plane (clip z=1) → behind cubes
    out.ndc = p;
    return out;
}

@fragment
fn fs_sky(in: VsOut) -> @location(0) vec4<f32> {
    let near_h = sky.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far_h  = sky.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let near_w = near_h.xyz / near_h.w;
    let far_w  = far_h.xyz / far_h.w;
    var dir = normalize(far_w - near_w);

    dir = rotate_y(dir, sky.params.z);                       // env_rotation
    let hdr = textureSampleLevel(env_tex, env_samp, dir_to_equirect_uv(dir), 0.0).rgb;
    // LINEAR HDR into the scene buffer; exposure + tonemap happen in composite.
    // env_intensity (master) × bg_brightness (background only) × tint colour.
    // Alpha = 0 marks "background" so the composite can give the environment its
    // own tone-map (geometry overpaints alpha = 1). Destination alpha doesn't
    // affect any blended colour, so this is purely a coverage tag.
    return vec4<f32>(hdr * sky.params.y * sky.params.w * sky.env_tint.rgb, 0.0);
}

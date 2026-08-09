// Capture / production-frame letterbox blit (#135 Phase 1).
//
// Samples the fixed-resolution production texture and writes it straight to the
// swapchain (the viewport/scissor confine it to the centred fit rect; the bars are
// the render pass's clear colour). Pure pass-through — no tonemap, no clamp — so
// EDR/HDR (Rgba16Float) content survives. An optional thin border marks the frame
// edge as a safe-area guide for lining up an OBS crop.

struct BlitU {
    thick: vec2<f32>,        // guide border half-thickness in UV per axis (0 = off)
    _pad: vec2<f32>,
    guide_color: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> u: BlitU;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Fullscreen triangle; uv has its origin at the top-left to match the swapchain.
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let xy = p[vi];
    var o: VsOut;
    o.pos = vec4<f32>(xy, 0.0, 1.0);
    o.uv = vec2<f32>(xy.x * 0.5 + 0.5, 0.5 - xy.y * 0.5);
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_samp, in.uv);
    if (u.thick.x > 0.0) {
        let e = in.uv;
        if (e.x < u.thick.x || e.x > 1.0 - u.thick.x ||
            e.y < u.thick.y || e.y > 1.0 - u.thick.y) {
            return u.guide_color;
        }
    }
    return c;
}

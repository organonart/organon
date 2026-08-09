// Organic Math — Creature anatomy overlay (GitHub #476 Tier 2c).
//
// A thin "diagram over the creature" drawn in world space and projected to
// screen: the spine, a cross-section ring around each body segment, and a vector
// per limb. The lines are additive glowing strokes, drawn TWICE against the
// raymarched creature's shared depth buffer: once with depth-compare Always (so
// occluded lines still read, faintly) and once with depth-compare Less (so lines
// in front of the surface read bright). The sum is the artist's depth-aware
// look — landmarks dim where they pass behind the translucent body. The line
// geometry is built CPU-side in `creature_overlay.rs` from the same body plan the
// raymarch uses. Outputs LINEAR HDR radiance (bloom happens later).

struct OverlayU {
    view_proj: mat4x4<f32>, // the scene camera (UNSCALED world → clip, like axes)
    params: vec4<f32>,      // x = brightness, y = alpha, z/w reserved
};
@group(0) @binding(0) var<uniform> u: OverlayU;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> VOut {
    var out: VOut;
    out.clip = u.view_proj * vec4<f32>(pos, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_line(in: VOut) -> @location(0) vec4<f32> {
    // Additive glow: the pipeline blends src·1 + dst·1, so the RGB is the stroke
    // colour scaled by this pass's brightness and the per-vertex alpha; the alpha
    // channel is carried for completeness but the additive blend ignores it.
    let a = u.params.y * in.color.a;
    return vec4<f32>(in.color.rgb * (u.params.x * a), a);
}

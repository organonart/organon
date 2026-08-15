// Capture overlay (#135 Phase 2): one alpha-blended pass for the whole overlay —
// solid quads (panel/rules), atlas text, and the formula image all share this
// pipeline. Vertices arrive in NDC with a per-vertex colour; the bound texture is
// the glyph atlas / formula image / a 1×1 white (for solids).
//
//   out = vec4(color.rgb * tex.rgb, color.a * tex.a)
//
// Atlas glyphs store coverage in alpha (rgb = 1), so text = colour × coverage.
// A 1×1 white texture makes solids = colour. The formula image carries its own
// colours, drawn with colour = (1,1,1, opacity). Straight (non-premultiplied) alpha.

struct VsIn {
    @location(0) pos: vec2<f32>,   // NDC
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(in.pos, 0.0, 1.0);
    o.uv = in.uv;
    o.color = in.color;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    return vec4<f32>(in.color.rgb * t.rgb, in.color.a * t.a);
}

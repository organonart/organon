// HDR bloom passes (run on the linear Rgba16Float scene buffer, before tonemap).
// Down = 13-tap filter (Jimenez/CoD) with an optional soft-knee bright prefilter
// on the first level; Up = 3x3 tent, blended additively by the pipeline. All
// LINEAR HDR — no tonemap here (that's composite.wgsl).

struct DownU {
    texel: vec2<f32>,   // 1 / source size
    threshold: f32,
    knee: f32,
    prefilter: f32,     // 1.0 only on the first (bright-pass) downsample
    exposure: f32,      // applied before the bright-pass so threshold is post-exposure
    _p0: f32,
    _p1: f32,
};
struct UpU {
    texel: vec2<f32>,   // 1 / source (smaller mip) size
    radius: f32,
    _p: f32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> du: DownU;
@group(0) @binding(2) var<uniform> uu: UpU;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

fn s(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, src_samp, uv, 0.0).rgb;
}

// Soft-knee bright-pass (Unity/CoD): keeps HDR energy above the threshold with a
// smooth knee so there's no hard cutoff.
fn prefilter(c: vec3<f32>) -> vec3<f32> {
    let br = max(c.r, max(c.g, c.b));
    var soft = br - du.threshold + du.knee;
    soft = clamp(soft, 0.0, 2.0 * du.knee);
    soft = soft * soft / (4.0 * du.knee + 1e-5);
    let contrib = max(soft, br - du.threshold) / max(br, 1e-5);
    return c * contrib;
}

@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    let t = du.texel;
    let uv = in.uv;
    let a = s(uv + t * vec2<f32>(-2.0, -2.0));
    let b = s(uv + t * vec2<f32>( 0.0, -2.0));
    let c = s(uv + t * vec2<f32>( 2.0, -2.0));
    let d = s(uv + t * vec2<f32>(-2.0,  0.0));
    let e = s(uv + t * vec2<f32>( 0.0,  0.0));
    let f = s(uv + t * vec2<f32>( 2.0,  0.0));
    let g = s(uv + t * vec2<f32>(-2.0,  2.0));
    let h = s(uv + t * vec2<f32>( 0.0,  2.0));
    let i = s(uv + t * vec2<f32>( 2.0,  2.0));
    let j = s(uv + t * vec2<f32>(-1.0, -1.0));
    let k = s(uv + t * vec2<f32>( 1.0, -1.0));
    let l = s(uv + t * vec2<f32>(-1.0,  1.0));
    let m = s(uv + t * vec2<f32>( 1.0,  1.0));

    var res: vec3<f32>;
    if (du.prefilter > 0.5) {
        // Karis average on the FIRST downsample (#174 T3): weight each 2x2 group
        // by 1/(1 + maxRGB) before combining. Sub-pixel HDR fireflies -- chrome
        // mirror-hits of the sun are thousands of nits in one pixel -- otherwise
        // flicker frame to frame and the bloom chain amplifies them into pulsing
        // blobs; the luma weight suppresses exactly those single-sample spikes.
        let g0 = (j + k + l + m) * 0.25;
        let g1 = (a + b + d + e) * 0.25;
        let g2 = (b + c + e + f) * 0.25;
        let g3 = (d + e + g + h) * 0.25;
        let g4 = (e + f + h + i) * 0.25;
        let w0 = 0.5 / (1.0 + max(g0.r, max(g0.g, g0.b)));
        let w1 = 0.125 / (1.0 + max(g1.r, max(g1.g, g1.b)));
        let w2 = 0.125 / (1.0 + max(g2.r, max(g2.g, g2.b)));
        let w3 = 0.125 / (1.0 + max(g3.r, max(g3.g, g3.b)));
        let w4 = 0.125 / (1.0 + max(g4.r, max(g4.g, g4.b)));
        res = (g0 * w0 + g1 * w1 + g2 * w2 + g3 * w3 + g4 * w4)
            / max(w0 + w1 + w2 + w3 + w4, 1e-6);
        // Expose first, so the threshold is in the same space as the final image.
        res = prefilter(res * du.exposure);
    } else {
        res = (j + k + l + m) * (0.5 * 0.25);
        res = res + (a + b + d + e) * (0.125 * 0.25);
        res = res + (b + c + e + f) * (0.125 * 0.25);
        res = res + (d + e + g + h) * (0.125 * 0.25);
        res = res + (e + f + h + i) * (0.125 * 0.25);
    }
    return vec4<f32>(res, 1.0);
}

@fragment
fn fs_up(in: VsOut) -> @location(0) vec4<f32> {
    let t = uu.texel * uu.radius;
    let uv = in.uv;
    var sum = s(uv + t * vec2<f32>(-1.0, -1.0)) * 1.0;
    sum = sum + s(uv + t * vec2<f32>( 0.0, -1.0)) * 2.0;
    sum = sum + s(uv + t * vec2<f32>( 1.0, -1.0)) * 1.0;
    sum = sum + s(uv + t * vec2<f32>(-1.0,  0.0)) * 2.0;
    sum = sum + s(uv + t * vec2<f32>( 0.0,  0.0)) * 4.0;
    sum = sum + s(uv + t * vec2<f32>( 1.0,  0.0)) * 2.0;
    sum = sum + s(uv + t * vec2<f32>(-1.0,  1.0)) * 1.0;
    sum = sum + s(uv + t * vec2<f32>( 0.0,  1.0)) * 2.0;
    sum = sum + s(uv + t * vec2<f32>( 1.0,  1.0)) * 1.0;
    return vec4<f32>(sum * (1.0 / 16.0), 1.0); // additive blend done by the pipeline
}

// Final HDR composite: read the linear scene HDR buffer + the bloom result,
// apply exposure, add bloom, tonemap, output LINEAR. This is the ONLY place the
// 128-bit scene radiance is collapsed to display range, so highlights roll off
// instead of clipping per-fragment.
//
// Two output modes, picked by `hdr_max`:
//   - SDR (hdr_max <= 1): ACES filmic clamped to [0,1]. The surface is sRGB, so
//     the hardware applies the gamma OETF. (The original, default path.)
//   - HDR (hdr_max  > 1): the SAME tone-map operator as SDR (so the diffuse range
//     looks identical to SDR — the curve gives us its filmic contrast + per-channel
//     saturation), then highlights past the knee are re-expanded into the display's
//     EDR headroom instead of clamping at white. The surface is Rgba16Float in an
//     extended-linear colorspace, so we emit linear radiance up to the headroom and
//     the compositor PQ-encodes it.
//
//   ⚠️ History (#119): HDR used to be a near-identity "shoulder" (linear below the
//   knee), which threw away the ACES filmic curve — so HDR looked muted/washed out
//   vs SDR (linear = no toe contrast, no per-channel saturation boost). The fix is
//   to tone-map exactly like SDR for the diffuse range and only re-expand highlights.

struct CompU {
    exposure: f32,
    bloom_intensity: f32,
    // EDR headroom. <= 1.0 → SDR output (ACES); > 1.0 → HDR output (highlights
    // roll off toward this value, in SDR-white units, instead of clamping to 1).
    hdr_max: f32,
    // HDR highlight knee (input-linear, in SDR-white units): pixels dimmer than
    // this tone-map exactly like SDR; brighter pixels are re-expanded toward
    // `hdr_max`. Lower = more of the image gains headroom (punchier highlights);
    // higher = only the very brightest specular/emissive extends. Only used in HDR.
    hdr_knee: f32,
    // SDR tone-map operator: 0 ACES (Narkowicz), 1 AgX, 2 Reinhard, 3 Neutral
    // (clip), 4 ACES Fitted (Hill, #174 T3). Also picks the HDR diffuse-range
    // curve (hdr_reexpand tone-maps with the same operator below the knee).
    tonemap: f32,
    // Ambient occlusion: enabled (0/1) + intensity. When disabled, AO = 1.
    ao_enabled: f32,
    ao_intensity: f32,
    // Tone-map operator for the ENVIRONMENT backdrop (same id space as `tonemap`).
    // The skybox is a photographic HDR panorama, so a contrasty filmic curve (ACES)
    // crushes it; this lets it use a gentler one (e.g. AgX) while geometry keeps
    // `tonemap` (SDR) / the headroom shoulder (HDR). Applied in BOTH modes, so the
    // backdrop looks consistent across displays. Equal SDR ids → no change.
    bg_tonemap: f32,
    // SSR (#80 A): when > 0.5, add the screen-space reflection buffer (premultiplied
    // by its Fresnel/roughness weight) on top of the lit scene before tonemapping.
    ssr_enabled: f32,
    // Wide-gamut output (#119). gamut > 0.5: the EDR surface is tagged Rec.2020, so
    // the final colour is converted/expanded from Rec.709 into it. `vivid` (0..1) is
    // the expansion amount: 0 = colour-accurate, 1 = full stretch to Rec.2020 primaries.
    // Only applied in HDR mode (hdr_max > 1).
    gamut: f32,
    vivid: f32,
    // SSGI (#152 Tier 2): when > 0.5, add the screen-space GI buffer (one diffuse
    // bounce gathered from neighbours) on top of the lit scene before tonemapping.
    ssgi_enabled: f32,
    // Frame counter for the SDR output dither (#174 T3) + explicit tail padding
    // (kept in lockstep with post.rs::CompU).
    frame: f32,
    // Learned upscaler (#200 Tier 5c) — repurposes the three tail scalar slots
    // (NOT a `vec3<f32>`: WGSL would 16-byte-align it and push the struct to 80
    // bytes vs the Rust `[f32;3]`'s 64, mismatching the bound buffer — three
    // scalars keep it at 64 bytes). `up_mode` > 0.5 replaces the plain bilinear
    // scene fetch with an HDR-safe content-adaptive sharpen reconstruction whose
    // per-pixel gain is `up_sharpen` × a Tier-0 seeded-MLP modulation (`up_seed`).
    // 0 = bilinear (byte-identical); gated on in the visual only when upscaling.
    up_mode: f32,
    up_sharpen: f32,
    up_seed: f32,
};

@group(0) @binding(0) var hdr_tex: texture_2d<f32>;
@group(0) @binding(1) var bloom_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<uniform> u: CompU;
@group(0) @binding(4) var ao_tex: texture_2d<f32>;
@group(0) @binding(5) var ssr_tex: texture_2d<f32>;
@group(0) @binding(6) var ssgi_tex: texture_2d<f32>;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> VsOut {
    let c = vec2<f32>(f32((vid << 1u) & 2u), f32(vid & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(c.x, 1.0 - c.y);
    return o;
}

// --- Learned upscaler (#200 Tier 5c) -------------------------------------------
// An HDR-safe content-adaptive sharpen reconstruction (CAS-style) that replaces
// the plain bilinear scene fetch when `render_scale < 1`, recovering apparent
// resolution so the auto-60fps DRS floor can drop further. The per-pixel sharpen
// gain rides a Tier-0 seeded MLP (regenerated inline from `up_seed`, bit-identical
// to `math::mlp_eval`); `up_mode = 0` returns the exact bilinear sample. The
// deterministic CAS base + the bounded gain + the flat-region dead zone are all
// mirrored + unit-tested in `math.rs` (`upscale_gain` / `upscale_adapt`).

const UP_GAIN_CLAMP: f32 = 1.0;      // matches math::UP_GAIN_CLAMP
const UP_OMEGA: f32 = 4.0;           // fixed SIREN feature scale (not a live param)
const MLP_IN_U: u32 = 4u;
const MLP_H_U: u32 = 8u;
const MLP_OUT_U: u32 = 4u;

fn up_hash(x: u32) -> u32 {
    var h = x;
    h ^= h >> 16u; h *= 0x7feb352du;
    h ^= h >> 15u; h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}
fn up_rand(seed: u32, idx: u32) -> f32 {
    let h = up_hash(seed ^ up_hash(idx));
    return f32(h) / 4294967296.0 * 2.0 - 1.0;
}
// Forward pass from a single seed (walk t = 0), first output only — the sharpen
// log-gain. Same weight-block order as math::mlp_eval / mlp.wgsl.
fn up_mlp0(seed: u32, input: vec4<f32>) -> f32 {
    var in_arr = array<f32, 4>(input.x, input.y, input.z, input.w);
    var h0: array<f32, 8>;
    let b0_base = MLP_H_U * MLP_IN_U;
    for (var j = 0u; j < MLP_H_U; j = j + 1u) {
        var acc = 0.0;
        for (var i = 0u; i < MLP_IN_U; i = i + 1u) {
            acc += up_rand(seed, j * MLP_IN_U + i) * in_arr[i];
        }
        acc += up_rand(seed, b0_base + j);
        h0[j] = sin(UP_OMEGA * acc);
    }
    var o = MLP_H_U * MLP_IN_U + MLP_H_U;
    var h1: array<f32, 8>;
    let b1_base = o + MLP_H_U * MLP_H_U;
    for (var j = 0u; j < MLP_H_U; j = j + 1u) {
        var acc = 0.0;
        for (var k = 0u; k < MLP_H_U; k = k + 1u) {
            acc += up_rand(seed, o + j * MLP_H_U + k) * h0[k];
        }
        acc += up_rand(seed, b1_base + j);
        h1[j] = sin(acc);
    }
    o = o + MLP_H_U * MLP_H_U + MLP_H_U;
    var acc = 0.0;
    for (var k = 0u; k < MLP_H_U; k = k + 1u) {
        acc += up_rand(seed, o + 0u * MLP_H_U + k) * h1[k];
    }
    acc += up_rand(seed, o + MLP_OUT_U * MLP_H_U + 0u);
    return acc;
}
fn upscale_gain(net: f32, mlp_out: f32) -> f32 {
    return exp(clamp(net * mlp_out, -UP_GAIN_CLAMP, UP_GAIN_CLAMP));
}
fn upscale_adapt(c: f32) -> f32 {
    return smoothstep(0.02, 0.25, c);
}
fn up_luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The scene fetch. `up_mode = 0` → the exact bilinear sample (byte-identical).
// Else: bilinear centre + a 4-tap cross at SOURCE-texel spacing (the low-res grid
// the upscale blurred), an unsharp `detail = centre − ring/4`, scaled by
// base × flat-region-adaptivity × MLP gain, keyed on the local luma contrast.
// Alpha (coverage) is the bilinear value — sharpening it would fringe silhouettes.
fn upsample_scene(uv: vec2<f32>) -> vec4<f32> {
    let base = textureSampleLevel(hdr_tex, samp, uv, 0.0);
    if (u.up_mode < 0.5 || u.up_sharpen <= 0.0) {
        return base;
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(hdr_tex));
    let l = textureSampleLevel(hdr_tex, samp, uv + vec2<f32>(-texel.x, 0.0), 0.0).rgb;
    let r = textureSampleLevel(hdr_tex, samp, uv + vec2<f32>( texel.x, 0.0), 0.0).rgb;
    let up = textureSampleLevel(hdr_tex, samp, uv + vec2<f32>(0.0, -texel.y), 0.0).rgb;
    let dn = textureSampleLevel(hdr_tex, samp, uv + vec2<f32>(0.0,  texel.y), 0.0).rgb;
    let c = base.rgb;
    let ring = (l + r + up + dn) * 0.25;
    let detail = c - ring;
    // Normalized local luma contrast (HDR-safe: relative, so it doesn't scale with
    // exposure) → flat regions get ~0 sharpen, edges get the full amount.
    let lc = up_luma(c);
    let lmn = min(min(up_luma(l), up_luma(r)), min(min(up_luma(up), up_luma(dn)), lc));
    let lmx = max(max(up_luma(l), up_luma(r)), max(max(up_luma(up), up_luma(dn)), lc));
    let contrast = (lmx - lmn) / (lmx + lmn + 1e-3);
    let feats = vec4<f32>(contrast, lc, detail.r + detail.g + detail.b, texel.x / max(texel.y, 1e-6));
    let amt = u.up_sharpen * upscale_adapt(contrast) * upscale_gain(u.up_sharpen, up_mlp0(u32(max(u.up_seed, 0.0)), feats));
    let sharp = max(c + detail * amt, vec3<f32>(0.0));
    return vec4<f32>(sharp, base.a);
}

// ACES filmic (Narkowicz fit). Linear HDR -> linear [0,1].
fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Reinhard (extended-less). Linear HDR -> linear [0,1).
fn reinhard(x: vec3<f32>) -> vec3<f32> {
    return x / (1.0 + x);
}

// Minimal AgX (Troy Sobotka / B. Wrensch). Outputs display-LINEAR [0,1] — the
// sigmoid polynomial below approximates AgX's 2.2-gamma DISPLAY-ENCODED output, so
// the reference's `agxEotf` linearization (pow 2.2) is applied at the end; the sRGB
// surface then applies the OETF once. (Omitting the pow handed gamma-encoded values
// to the sRGB surface — a double encode that washed AgX out, and fed the EDR path
// non-linear values it treated as linear.) AgX gently desaturates highlights,
// giving a filmic, less "neon" roll-off.
fn agx_contrast(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x
        + 0.4298 * x2 + 0.1191 * x - 0.00232;
}
fn agx(val: vec3<f32>) -> vec3<f32> {
    let m = mat3x3<f32>(
        0.842479062253094, 0.0423282422610123, 0.0423756549057051,
        0.0784335999999992, 0.878468636469772, 0.0784336000000000,
        0.0792237451477643, 0.0791661274605434, 0.879142973793104,
    );
    let m_inv = mat3x3<f32>(
        1.19687900512017, -0.0528968517574562, -0.0529716355144438,
        -0.0980208811401368, 1.15190312990417, -0.0980434501171241,
        -0.0990297440797205, -0.0989611768448433, 1.15107367264116,
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = m * max(val, vec3<f32>(0.0));
    v = clamp(log2(max(v, vec3<f32>(1e-10))), vec3<f32>(min_ev), vec3<f32>(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    v = agx_contrast(v);
    v = m_inv * v;
    // Linearize (the reference agxEotf): the contrast polynomial's output is
    // 2.2-gamma display-encoded. Clamp first — m_inv can go slightly negative,
    // and pow of a negative is NaN in WGSL.
    v = clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(v, vec3<f32>(2.2));
}

// ACES "fitted" (Stephen Hill): the RRT+ODT approximated with two 3x3 matrices
// around a rational fit (#174 T3). Unlike the per-channel Narkowicz fit above,
// the input/output matrices decorrelate the channels first, so saturated bright
// colours roll toward white instead of hue-skewing to neon at the top end -- the
// per-channel fit's signature artifact on this app's RGB-cube emissives.
// (Matrices are the sRGB<->ACEScg-ish fits from Hill's BakingLab, written as
// WGSL COLUMNS = the reference's transposed rows.)
fn rrt_odt_fit(v: vec3<f32>) -> vec3<f32> {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}
fn aces_fitted(x: vec3<f32>) -> vec3<f32> {
    let m_in = mat3x3<f32>(
        vec3<f32>(0.59719, 0.07600, 0.02840),
        vec3<f32>(0.35458, 0.90834, 0.13383),
        vec3<f32>(0.04823, 0.01566, 0.83777),
    );
    let m_out = mat3x3<f32>(
        vec3<f32>(1.60475, -0.10208, -0.00327),
        vec3<f32>(-0.53108, 1.10813, -0.07276),
        vec3<f32>(-0.07367, -0.00605, 1.07602),
    );
    var v = m_in * max(x, vec3<f32>(0.0));
    v = rrt_odt_fit(v);
    v = m_out * v;
    return clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Khronos PBR Neutral (Emmett Lalish, glTF Sample Viewer — MIT). The "Neutral"
// operator, purpose-built to keep SATURATED colours saturated while rolling the
// luminance off smoothly — the opposite of ACES, which bleaches bright colour to
// white. Only the very top desaturates (by `desaturation`), so bright emissive
// reads as coloured *light* (a glowing gem) instead of a flat clipped block, and
// it leaves an HDR gradient above the knee for the EDR re-expansion (a hard clip
// left none). Linear in → linear [0,1] out (the sRGB surface applies the OETF),
// matching the other operators here.
fn pbr_neutral(color_in: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;
    var color = color_in;
    let x = min(color.r, min(color.g, color.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    color = color - vec3<f32>(offset);
    let peak = max(color.r, max(color.g, color.b));
    if (peak < start_compression) {
        return color;
    }
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);
    color = color * (new_peak / peak);
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(color, vec3<f32>(new_peak), g);
}

// SDR operator dispatch.
fn tonemap_sdr(x: vec3<f32>, op: f32) -> vec3<f32> {
    if (op < 0.5) {
        return aces(x);
    } else if (op < 1.5) {
        return agx(x);
    } else if (op < 2.5) {
        return reinhard(x);
    } else if (op < 3.5) {
        return pbr_neutral(x); // 3 = Neutral (Khronos PBR Neutral — colour-preserving)
    }
    return aces_fitted(x); // 4 = ACES Fitted (Hill RRT+ODT fit)
}

// Cheap 2D hash for the output dither.
fn dhash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.0973));
    q = q + dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

// True-HDR geometry tone curve (#119): tone-map with the chosen SDR operator to
// reproduce the SDR look, then re-expand highlights into the EDR headroom.
//
// Below `knee` (pre-tonemap, input-linear brightness) the result is EXACTLY the SDR
// operator — so the diffuse range is identical to the SDR image (same filmic toe +
// per-channel saturation, the source of the "vivid" look). Above the knee, the
// tone-mapped pixel is scaled toward `peak` (the display headroom) so bright
// specular/emissive exceed SDR white instead of clamping at it. Hue-preserving (a
// single luminance-driven scalar boost), capped at `peak` so the EDR ceiling never
// clips. SDR white (everything below the knee) is left untouched, so HDR is "SDR
// look + brighter highlights", never dimmer or flatter.
fn hdr_reexpand(x: vec3<f32>, op: f32, peak: f32, knee: f32) -> vec3<f32> {
    let tm = tonemap_sdr(x, op);             // the SDR look, in [0,1]
    let m = max(x.r, max(x.g, x.b));         // pre-tonemap brightness (linear)
    let over = max(m - knee, 0.0);
    let span = max(peak - knee, 0.01);
    // boost ramps 1 -> peak as input brightness rises past the knee (asymptotic).
    let boost = 1.0 + (peak - 1.0) * (1.0 - exp(-over / span));
    let c = tm * boost;
    // Hue-preserving headroom cap: if any channel would exceed `peak`, scale the
    // WHOLE colour down by one factor (not a per-channel `min`, which would shift
    // hue / grey out the highlight at the ceiling). With tm ≤ 1 and boost < peak the
    // cap can't trigger today, but this keeps it correct if the curve ever changes.
    let hi = max(c.r, max(c.g, c.b));
    return select(c, c * (peak / hi), hi > peak);
}

// Rec.709 (sRGB primaries) linear → Rec.2020 linear (BT.2087, D65). Colour-preserving:
// the same physical colour, re-expressed in the wider container's coordinates. Since
// Rec.709 ⊂ Rec.2020, the result is always non-negative.
fn rec709_to_2020(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(vec3<f32>(0.6274, 0.3293, 0.0433), c),
        dot(vec3<f32>(0.0691, 0.9195, 0.0114), c),
        dot(vec3<f32>(0.0164, 0.0880, 0.8956), c),
    );
}

// Wide-gamut expansion (#119). The surface is tagged Rec.2020, so the OS reads our
// output as Rec.2020-linear. Two endpoints:
//   accurate = rec709_to_2020(c)  → displays as the original Rec.709 colour (no change)
//   stretched = c                 → our Rec.709 numbers read AS Rec.2020 → pushed out
//                                    toward the wide primaries (much more saturated)
// `vivid` dials between them, so 0 is colour-accurate and 1 is the full gamut stretch
// that makes the spectrum pop on a wide-gamut display (what SDR mode does for free).
fn gamut_expand(c: vec3<f32>, vivid: f32) -> vec3<f32> {
    return mix(rec709_to_2020(c), c, clamp(vivid, 0.0, 1.0));
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    // Learned upscaler (#200 Tier 5c): content-adaptive sharpen reconstruction
    // when upscaling (`up_mode`), else the exact bilinear fetch (byte-identical).
    let scene = upsample_scene(in.uv);
    let hdr = scene.rgb;
    // The scene pass writes coverage into alpha: the skybox clears it to 0, opaque
    // geometry writes 1 (glass/translucent write partial). So `content` = 1 over
    // cubes, 0 over the bare environment, and blends across silhouette edges.
    let content = clamp(scene.a, 0.0, 1.0);
    // Assembly order (identical to the old code when AO/SSR/SSGI are off, i.e. at
    // defaults): scene (+SSGI) → ×AO → SSR blend → +bloom. AO multiplies only the
    // scene radiance + diffuse bounce — multiplying it over the already-added bloom
    // carved dark holes into glow halos, and over SSR re-darkened the reflection.
    // SSR *blends* by its stored confidence weight instead of adding: the cube
    // shader already wrote a full env-specular for this pixel, so a pure add
    // counted the reflection energy twice everywhere SSR hit.
    let bloom = textureSampleLevel(bloom_tex, samp, in.uv, 0.0).rgb;
    var color = hdr * u.exposure;
    // Screen-space GI (#152 Tier 2): add the gathered one-bounce diffuse, exposed
    // to match the scene. Off → buffer unused (no change).
    if (u.ssgi_enabled > 0.5) {
        let g = textureSampleLevel(ssgi_tex, samp, in.uv, 0.0).rgb;
        color = color + g * u.exposure;
    }
    // Ambient occlusion darkens the scene radiance before tonemapping. Disabled →
    // AO = 1 (no change), so the default look is untouched.
    if (u.ao_enabled > 0.5) {
        let ao = textureSampleLevel(ao_tex, samp, in.uv, 0.0).r;
        color = color * clamp(1.0 - (1.0 - ao) * u.ao_intensity, 0.0, 1.0);
    }
    // Inter-cube reflections (#80 A): the buffer's rgb is premultiplied by the
    // Fresnel/roughness confidence weight, which is ALSO stored in alpha — blend
    // the pixel's existing radiance down by that weight so SSR replaces the env
    // reflection it supersedes instead of stacking on top of it.
    if (u.ssr_enabled > 0.5) {
        let ssr = textureSampleLevel(ssr_tex, samp, in.uv, 0.0);
        let w = clamp(ssr.a, 0.0, 1.0);
        color = color * (1.0 - w) + ssr.rgb * u.exposure;
    }
    color = color + bloom * u.bloom_intensity;
    if (u.hdr_max > 1.0) {
        // True-HDR output: geometry tone-maps like SDR for the diffuse range, then
        // re-expands highlights so cube specular/emissive exceed SDR white. The
        // environment backdrop keeps its own tone-map (e.g. AgX) and stays in SDR
        // range — the panorama looks the same as on an SDR display while the cubes
        // pop into the EDR headroom.
        let geo = hdr_reexpand(color, u.tonemap, u.hdr_max, u.hdr_knee);
        let bg = tonemap_sdr(color, u.bg_tonemap);
        color = mix(bg, geo, content);
    } else {
        // SDR: tone-map geometry and the environment backdrop separately, blended
        // by coverage. When the two operators match this is identical to one pass.
        let geo_tm = tonemap_sdr(color, u.tonemap);
        let bg_tm = tonemap_sdr(color, u.bg_tonemap);
        color = mix(bg_tm, geo_tm, content);
    }
    // Wide-gamut output (#119): only in HDR, where the surface is tagged Rec.2020.
    // Expand Rec.709 → Rec.2020 by `vivid` so saturated colours reach the display's
    // real primaries. In SDR (or gamut off) this is skipped → output stays Rec.709.
    if (u.hdr_max > 1.0 && u.gamut > 0.5) {
        color = gamut_expand(color, u.vivid);
    }
    // SDR output dither (#174 T3): ±½-LSB triangular noise before the 8-bit
    // quantize breaks up banding in slow gradients (vignettes, dark skies) on the
    // projector. Sub-visible everywhere else; HDR output is float — no dither.
    if (u.hdr_max <= 1.0) {
        let n1 = dhash(in.pos.xy + vec2<f32>(u.frame, u.frame * 0.618));
        let n2 = dhash(in.pos.yx * 1.371 + vec2<f32>(u.frame * 2.71, u.frame * 1.13));
        color = color + vec3<f32>((n1 + n2) * 0.5 - 0.5) * (1.0 / 255.0);
    }
    return vec4<f32>(color, 1.0);
}

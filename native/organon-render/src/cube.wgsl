// Organic Math cube shader — metallic-roughness Cook-Torrance PBR. The cubes are
// lit by BOTH an image-based environment (split-sum IBL: irradiance + prefiltered
// specular + BRDF LUT, all maps equirectangular Rgba16Float) AND two analytic
// directional lights (a key + a fill), ported from BrassLion/rust-pbr's
// pbr.frag — these give live, view-dependent specular highlights as the cubes
// rotate, which pure IBL can't. Albedo = per-vertex RGB-cube color; emissive
// "glow" preserved. Outputs LINEAR HDR (surface is sRGB — exposure/bloom/ACES
// happen later in the composite pass; do NOT pow(1/2.2) here).

// ----- group(0): uniforms -----
struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>, // xyz = camera world position
    mat: vec4<f32>,        // x=metallic, y=roughness, z=glow, w=prefilter_mip_count
    env: vec4<f32>,        // x=exposure, y=env_intensity, z=env_rotation(rad), w=opacity
    key_light: vec4<f32>,  // xyz = direction TO the key light (world, unit), w=intensity
    fill_light: vec4<f32>, // xyz = direction TO the fill light (world, unit), w=intensity
    amb: vec4<f32>,        // x=ambient/IBL multiplier, y=material_type, z=glass IOR, w reserved
    sss: vec4<f32>,        // translucency: x=amount, y=distortion, z=power, w reserved
    irid: vec4<f32>,       // iridescence: x=amount, y=scale, z=hue shift, w reserved
    env_tint: vec4<f32>,   // xyz = environment/IBL tint colour (white = none), w unused
    ripple: vec4<f32>,      // emissive ripple: x=intensity, y=phase, z=freq, w=sharpness
    ripple_ctr: vec4<f32>,  // xyz = field centre (world), w = field radius
    ripple_mode: vec4<f32>, // x = geom (0 radial / 1 axial), yzw = axial axis dir
    glassx: vec4<f32>,      // spectral glass (#80 C): x=dispersion, y=caustic, z=thin_film,
                            // w = specular-occlusion enable (#174 T3 — repurposed spare, was
                            // an unread "spectral_samples"; set by the visual when SSAO is on)
    reflect_ctl: vec4<f32>, // reflection (#163 T1): x=reflect_tint, y=chrome_purity, z=glass_clarity, w=f0_override
    refl_box_min: vec4<f32>, // reflection probe (#163 T2): xyz=box min (world), w=source_id (0 EnvOnly)
    refl_box_max: vec4<f32>, // xyz=box max (world), w=parallax blend (0..1)
    refr: vec4<f32>,        // Refractive optics: x=Beer–Lambert absorption strength
                            // (Refractive material + the overlay's murk),
                            // y=refraction-overlay enable (0/1) — weaves the same
                            // refracted transmission into Standard/Chrome/Glass,
                            // z=overlay blend (0..1), w reserved
    aniso: vec4<f32>,       // Anisotropy (#214 T1): x=amount (−1..1, along/across the
                            // brush), y=brush rotation around N (rad), z=overlay enable
                            // (0/1 — layer the elliptical lobe onto Standard/Chrome),
                            // w=overlay blend (0..1). All 0 → isotropic (byte-identical).
    coat: vec4<f32>,        // Surface lobes (#214 T2): x=clearcoat strength (0..1),
                            // y=clearcoat roughness, z=clearcoat overlay enable (0/1),
                            // w=sheen overlay enable (0/1).
    sheen: vec4<f32>,       // x=sheen amount (0..1), y=sheen roughness, z=sheen tint
                            // (0=white → 1=albedo), w reserved. All lobes off → identical.
    body: vec4<f32>,        // Body optics (#214 T3): x=SSS thickness amount (0..1 —
                            // drive translucency by the measured chord), y=SSS radius
                            // (Beer–Lambert penetration), z=interior in-scatter (0..1 —
                            // opal glow in the Glass/Refractive body), w reserved.
    micro: vec4<f32>,       // Microstructure (#214 T4): x=glitter amount (0..1),
                            // y=glitter density (world grid scale), z=glitter sharpness
                            // (0..1), w=diffraction amount (0..1 — rainbow grating orders).
    micro2: vec4<f32>,      // x=diffraction frequency, y=retroreflection amount (0..1),
                            // zw reserved. All amounts 0 → today's look (byte-identical).
    emit: vec4<f32>,        // Spectral emission (#214 T5 pt1): x=fluorescence (0..1 —
                            // absorb the env's short-wavelength light, re-emit at a hue),
                            // y=fluor hue (0..1), z=incandescence (0..1 — blackbody glow),
                            // w=temperature (Kelvin). Amounts 0 → today (byte-identical).
    thinfilm: vec4<f32>,    // Physical thin-film (#258 T1): x=base thickness (nm; 0 →
                            // model OFF, keep the cosine-hack path), y=thickness marbling,
                            // z=film IOR, w=gravity-drainage gradient. x=0 → byte-identical.
    demo_light_pos: vec4<f32>, // Demo point light (#288 T3): xyz=world pos, w=intensity
                            // (0 → OFF, adds nothing → byte-identical off the Demo scene).
    demo_light_col: vec4<f32>, // xyz=colour, w=radius (falloff scale).
    matcol: vec4<f32>,      // Material colour transform (#305 T1): x=effective hue offset
                            // (turns), y=saturation, z=value, w reserved. [0,1,1,_] = identity.
    skyrefl: vec4<f32>,     // Live-sky cloud reflections (#305 T2): x=enable, y=cover,
                            // z=drift phase, w=strength. enable 0 → identity.
    cal: vec4<f32>,         // Calibrated colour (#349 T1): x=mode (0 Aesthetic → identity,
                            // 1 Calibrated), y=lut (0 Turbo/1 Viridis/2 Inferno/3 Magma),
                            // z=amount (0..1 blend over the aesthetic albedo), w=cal_t
                            // (the CPU-computed db_to_colour_t coord). mode<0.5 → identity.
    shape: vec4<f32>,       // Node bevel: x=bevel (0 sharp cube → 1 sphere), yzw reserved.
                            // Rounds the (subdivided) cube mesh via a rounded-box morph in
                            // the vertex stages. Set nonzero only on the Original / Flow-
                            // Aligned cube draw (render() gates it). x=0 → sharp cube.
    mtl: vec4<f32>,         // Materials (#472 T1): x=material_on (0 → scalar PBR, identical),
                            // y=projection (0 triplanar / 1 world-planar XZ / 2 object-planar),
                            // z=scale (world→UV frequency), w=height displacement amount
                            // (#472 T5, from material_live[5]; was the removed normal_strength).
    mtl2: vec4<f32>,        // x=ao_strength, y=rough_scale, z=metal_scale, w=present_mask
                            // (runtime bits: 1 albedo|2 normal|4 rough|8 metal|16 AO|32 height
                            // — an absent map keeps the scalar-uniform value). x=0 → identical.
};
@group(0) @binding(0) var<uniform> u: Uniforms;

// ===================== #618 Tier 3: pipeline specialisation =================
// A pipeline-overridable constant, not a uniform. `u.mtl.x` already gates the
// material maps at RUNTIME, which keeps the picture right and costs the work
// anyway: the samples, the triplanar UV resolve and the derivative cotangent
// frame all execute on every fragment of every draw and are then multiplied
// through zero. The real price is not the five fetches (the neutral maps are
// 1x1 and cache-resident) — it is occupancy. A GPU allocates registers for the
// worst case across the whole shader, so live state you never use still caps
// how many wavefronts are in flight, which is what hides memory latency.
//
// Declared `true` so the UNSPECIALISED module is exactly today's shader: naga
// validates this file as-is, and a pipeline that forgets to set the constant
// gets the correct-but-slower path rather than silently losing its materials.
// `render.rs` sets it false for the variant it builds while no material folder
// is loaded, which is the default state.
override material_maps: bool = true;

// #472 Tier 1 — the material PBR texture set (group 5): six channel maps + one
// shared sampler. Sampled (triplanar or planar) only when `u.mtl.x` is on; the
// generator cube draw is the only draw that sets it (render() gates it).
@group(5) @binding(0) var mat_albedo_tex : texture_2d<f32>;
@group(5) @binding(1) var mat_normal_tex : texture_2d<f32>;
@group(5) @binding(2) var mat_rough_tex  : texture_2d<f32>;
@group(5) @binding(3) var mat_metal_tex  : texture_2d<f32>;
@group(5) @binding(4) var mat_ao_tex     : texture_2d<f32>;
@group(5) @binding(5) var mat_height_tex : texture_2d<f32>;
@group(5) @binding(6) var mat_samp       : sampler;

// Calibrated-colour LUT (#349 Tier 1) — MUST mirror `math::calibrated_colour`
// EXACTLY (same coefficients, same Horner order) so the CPU and GPU tint agree.
// `t` ∈ 0..1 is the `db_to_colour_t` coord; `lut` picks Turbo/Viridis/Inferno/Magma.
fn cal_p6(x: f32, c0: f32, c1: f32, c2: f32, c3: f32, c4: f32, c5: f32, c6: f32) -> f32 {
    // Degree-6 Horner: c0 + x(c1 + x(c2 + … + x·c6)) — matches Rust `calibrated_colour`.
    return c0 + x * (c1 + x * (c2 + x * (c3 + x * (c4 + x * (c5 + x * c6)))));
}
fn calibrated_colour(t: f32, lut: u32) -> vec3<f32> {
    let x = clamp(t, 0.0, 1.0);
    var rgb: vec3<f32>;
    if (lut == 1u) { // Viridis
        rgb = vec3<f32>(
            cal_p6(x, 0.27772733, 0.10509304, -0.33086183, -4.63423050, 6.22826994, 4.77638500, -5.43545586),
            cal_p6(x, 0.00540734, 1.40461353, 0.21484756, -5.79910097, 14.17993337, -13.74514538, 4.64585261),
            cal_p6(x, 0.33409981, 1.38459016, 0.09509516, -19.33244096, 56.69055260, -65.35303263, 26.31243525));
    } else if (lut == 2u) { // Inferno
        rgb = vec3<f32>(
            cal_p6(x, 0.00021894, 0.10651342, 11.60249308, -41.70399613, 77.16293570, -71.31942824, 25.13112622),
            cal_p6(x, 0.00165100, 0.56395644, -3.97285397, 17.43639888, -33.40235894, 32.62606426, -12.24266895),
            cal_p6(x, -0.01948090, 3.93271239, -15.94239411, 44.35414520, -81.80730926, 73.20951986, -23.07032500));
    } else if (lut == 3u) { // Magma
        rgb = vec3<f32>(
            cal_p6(x, -0.00213649, 0.25166054, 8.35371728, -27.66873309, 52.17613981, -50.76852536, 18.65570507),
            cal_p6(x, -0.00074966, 0.67752324, -3.57771951, 14.26473078, -27.94360607, 29.04658282, -11.48977352),
            cal_p6(x, -0.00538613, 2.49402660, 0.31446790, -13.64921319, 12.94416944, 4.23415299, -5.60196151));
    } else { // Turbo (default)
        rgb = vec3<f32>(
            cal_p6(x, 0.11408901, 6.71641950, -66.09402360, 228.76607915, -334.83515658, 218.76372184, -52.88903478),
            cal_p6(x, 0.06288341, 3.18228675, -4.92798270, 25.04986700, -69.31749713, 67.52150568, -21.54527365),
            cal_p6(x, 0.22483372, 7.57158159, -10.09439368, -91.54105330, 288.58588506, -305.20457722, 110.51746477));
    }
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Blend the calibrated tint over an aesthetic albedo (#349 T1). This is the SINGLE
// application point for the calibrated colour law — every surface type that goes
// through cube.wgsl (Original/FlowAligned/SweptTubes/Voxel instances + the shared
// per-fragment tint) inherits it here. `u.cal.x < 0.5` (Aesthetic) or amount 0 →
// returns `albedo` unchanged (byte-identical default).
fn apply_calibrated(albedo: vec3<f32>) -> vec3<f32> {
    if (u.cal.x < 0.5) {
        return albedo;
    }
    let cc = calibrated_colour(u.cal.w, u32(u.cal.y + 0.5));
    return mix(albedo, cc, clamp(u.cal.z, 0.0, 1.0));
}

// Material colour transform (#305 Tier 1) — recolour a resolved albedo by a per-
// material Hue / Saturation / Value. `hsv.x` = hue offset in TURNS (rotates the
// colour around the luma axis — a palette hue-cycle on the palette-derived albedo),
// `hsv.y` = saturation (1 = unchanged, 0 = greyscale), `hsv.z` = value (1 = unchanged,
// 0 = black). [0, 1, 1] is the identity (byte-identical default). Hue rotation uses
// the standard luma-preserving RGB rotation; saturation lerps toward luma.
fn apply_hsv(rgb: vec3<f32>, hsv: vec4<f32>) -> vec3<f32> {
    var c = rgb;
    let hue = hsv.x;
    if (hue != 0.0) {
        let a = hue * 6.2831853;
        let cs = cos(a);
        let sn = sin(a);
        // Rotation about the (1,1,1)/√3 axis (Rodrigues, luma-ish preserving).
        let k = 0.57735026;
        let one_c = 1.0 - cs;
        let m0 = vec3<f32>(cs + k * k * one_c,      k * k * one_c - k * sn, k * k * one_c + k * sn);
        let m1 = vec3<f32>(k * k * one_c + k * sn,  cs + k * k * one_c,     k * k * one_c - k * sn);
        let m2 = vec3<f32>(k * k * one_c - k * sn,  k * k * one_c + k * sn, cs + k * k * one_c);
        c = vec3<f32>(dot(m0, c), dot(m1, c), dot(m2, c));
    }
    let luma = dot(max(c, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, hsv.y); // saturation
    c = c * hsv.z;                      // value
    return max(c, vec3<f32>(0.0));
}

// ----- group(3): bounced-GI irradiance probe volume (#80 Part B) -----
// A coarse 6³ = 216 RGB probe grid (filled CPU-side from the node field) the cube
// shader trilinearly samples to add coloured diffuse bounce into the ambient term.
struct GiUniform {
    bbox_min: vec4<f32>,
    bbox_max: vec4<f32>,
    info: vec4<f32>, // x=enabled, y=intensity, z=grid_dim, w=falloff
    // #152 Tier 3 (DDGI/SH): 3 vec4 per probe — one per colour channel, each packing
    // `[L0, L1x, L1y, L1z]` (band-1 SH). 648 = math::GI_SLOTS (GI_CELLS·3, DIM=6).
    probes: array<vec4<f32>, 648>,
};
@group(3) @binding(0) var<uniform> gi: GiUniform;

// ----- group(3) binding(1): emissive cubes as real lights (#167 Tier 3) -----
// The visual picks the brightest N nodes and uploads them as point lights; the cube
// shader loops them and adds real Cook-Torrance direct lighting, so a glowing cube
// throws a crisp specular glint + a coloured diffuse pool onto its neighbours. Shares
// group(3) with the GI probes (cube-pipeline-only), so no new bind group. count 0 → off.
struct MLight { pos: vec4<f32>, color: vec4<f32> }; // pos.xyz + radius(w); color.rgb + w unused
struct LightsU {
    header: vec4<f32>, // x=count, y=intensity, z=reserved, w=reserved
    lights: array<MLight, 64>,
};
@group(3) @binding(1) var<uniform> mlights: LightsU;

// ----- group(3) bindings 2/3: blurred SSAO for specular occlusion (#174 T3) -----
// SSAO was only applied in the composite, where it darkens everything uniformly —
// the IBL specular had NO occlusion at shading time, so cubes deep inside a dense
// field reflected the full sky. When SSAO is on (`u.glassx.w`), the blurred AO is
// bound here (a 1×1 white dummy otherwise → no-op) and Lagarde's cheap specular-
// occlusion term scales the env-specular/mirror lobes. Rides the GI probe group —
// cube-pipeline-only, so no new bind group.
@group(3) @binding(2) var ao_tex: texture_2d<f32>;
@group(3) @binding(3) var ao_samp: sampler;

// ----- group(4): shadow map (#152 Tier 3) -----
// A single world-space depth map rendered from the key light. `shadow_factor`
// projects the fragment into the light's clip space and PCF-compares against it,
// darkening the KEY light's direct contribution where occluded. Off (params.x = 0)
// → returns 1 (no change). The instanced path only (the raymarch siblings keep
// their own shading); a 1×1 dummy map is bound when disabled.
struct ShadowU {
    light_vp: mat4x4<f32>, // world → light clip
    params: vec4<f32>,     // x=enabled, y=depth bias, z=strength, w=texel size
    // #182 T4 fluid light coupling: x = fluid-shadow amount, y = caustic
    // amount (both 0 whenever the fluidlight pass didn't run this frame).
    params2: vec4<f32>,
};
@group(4) @binding(0) var<uniform> shadow_u: ShadowU;
@group(4) @binding(1) var shadow_map: texture_depth_2d;
@group(4) @binding(2) var shadow_samp: sampler_comparison;
// #182 T4: the light-space fluid map — r = dye transmittance (the smoke casts
// shadow), g = caustic (key light focused through the liquid surface).
@group(4) @binding(3) var fluidlight_tex: texture_2d<f32>;
@group(4) @binding(4) var fluidlight_samp: sampler;
// #195 Tier 1: the SCREEN-SPACE hardware-RT shadow mask (r = key visibility,
// g = fill visibility), traced by rt_shadow.wgsl against the Tier-0 TLAS.
// Consulted instead of the PCF map when ShadowU.params2.z > 0; a 1×1 white
// dummy otherwise (visibility 1 = no change).
@group(4) @binding(5) var rt_shadow_tex: texture_2d<f32>;

// Sample the RT visibility mask at this fragment's screen position: reproject
// the world position through the SAME (jittered) view-proj the mask pass used
// to reconstruct it, so the lookup lands on this fragment's own mask texel.
// (1,1) outside the frustum or when the dummy is bound.
fn rt_shadow_vis(world: vec3<f32>) -> vec2<f32> {
    if (shadow_u.params2.z <= 0.0) {
        return vec2<f32>(1.0);
    }
    let clip = u.view_proj * vec4<f32>(world, 1.0);
    if (clip.w <= 0.0) {
        return vec2<f32>(1.0);
    }
    let ndc = clip.xy / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec2<f32>(1.0);
    }
    // Nearest-texel load (#198 review): a bilinear sample blends lit and
    // occluded mask texels at shadow edges into light leaks; the reprojected
    // uv lands on this fragment's own texel centre, so fetch exactly it.
    // (textureLoad also needs no sampler → fine in non-uniform control flow.)
    let dims = vec2<f32>(textureDimensions(rt_shadow_tex));
    let px = vec2<i32>(clamp(uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    return textureLoad(rt_shadow_tex, px, 0).rg;
}

// Key-light factor from the fluid medium (#182 T4): Beer–Lambert transmittance
// through the ink, plus the liquid's caustic shimmer, both projected from the
// key light's own map. 1.0 whenever both amounts are zero.
fn fluid_light(world: vec3<f32>) -> f32 {
    let amt_t = shadow_u.params2.x;
    let amt_c = shadow_u.params2.y;
    if (amt_t <= 0.0 && amt_c <= 0.0) {
        return 1.0;
    }
    let lc = shadow_u.light_vp * vec4<f32>(world, 1.0);
    if (lc.w <= 0.0) {
        return 1.0;
    }
    let ndc = lc.xyz / lc.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return 1.0;
    }
    let m = textureSampleLevel(fluidlight_tex, fluidlight_samp, uv, 0.0);
    return mix(1.0, m.r, clamp(amt_t, 0.0, 1.0)) * (1.0 + amt_c * m.g);
}

// `n_dot_l` drives a slope-scaled bias: surfaces near-parallel to the light need a
// larger offset to avoid acne, while a constant bias big enough for them
// peter-pans contact points. At n_dot_l = 1 the bias equals the user constant
// exactly (flat-on faces unchanged).
fn shadow_factor(world: vec3<f32>, n_dot_l: f32) -> f32 {
    if (shadow_u.params.x < 0.5) {
        return 1.0;
    }
    let lc = shadow_u.light_vp * vec4<f32>(world, 1.0);
    if (lc.w <= 0.0) {
        return 1.0;
    }
    let ndc = lc.xyz / lc.w;
    if (ndc.z <= 0.0 || ndc.z >= 1.0) {
        return 1.0; // outside the light's depth range → unshadowed
    }
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return 1.0; // outside the map → unshadowed
    }
    // Reference depth, biased to kill acne; the slope term grows the bias up to 3×
    // toward grazing incidence (where the depth gradient per texel is steepest).
    let slope = clamp(1.0 - n_dot_l, 0.0, 1.0);
    let cmp = ndc.z - shadow_u.params.y * (1.0 + 2.0 * slope);
    let texel = shadow_u.params.w;
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let off = vec2<f32>(f32(dx), f32(dy)) * texel;
            // → 1 where the reference passes (lit), 0 occluded. Use the *Level* variant
            // (explicit mip 0, no implicit derivatives): shadow_factor has per-fragment
            // early-returns above, so this runs in non-uniform control flow, where the
            // derivative-using textureSampleCompare is a WGSL uniformity violation
            // (UB / invalid on strict backends). The shadow map has no mips anyway.
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_samp, uv + off, cmp);
        }
    }
    let vis = sum / 9.0;
    return mix(1.0, vis, clamp(shadow_u.params.z, 0.0, 1.0));
}

// One probe's SH: the three per-channel `[L0, L1x, L1y, L1z]` vec4s (#152 Tier 3 DDGI/SH).
struct ShCell { r: vec4<f32>, g: vec4<f32>, b: vec4<f32> };

fn sh_fetch(ix: i32, iy: i32, iz: i32, dim: i32) -> ShCell {
    let x = clamp(ix, 0, dim - 1);
    let y = clamp(iy, 0, dim - 1);
    let z = clamp(iz, 0, dim - 1);
    let base = (z * dim * dim + y * dim + x) * 3;
    var c: ShCell;
    c.r = gi.probes[base];
    c.g = gi.probes[base + 1];
    c.b = gi.probes[base + 2];
    return c;
}
fn sh_mix(a: ShCell, b: ShCell, t: f32) -> ShCell {
    var c: ShCell;
    c.r = mix(a.r, b.r, t);
    c.g = mix(a.g, b.g, t);
    c.b = mix(a.b, b.b, t);
    return c;
}

// Directional bounce irradiance (band-1 SH) at a world position + surface normal,
// scaled by intensity. `L0` (slot `.x`) is the flat term — identical to the old
// probe; the `L1` gradient (`.yzw`) adds directionality (a red node to +X brightens
// +X-facing surfaces more), and is ~0 for a symmetric neighbourhood, so the flat
// look is preserved. Returns 0 when GI is off. Layout matches
// `math::compute_gi_probes` ((z·DIM² + y·DIM + x)·3 + channel).
fn gi_irradiance(world_pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if (gi.info.x < 0.5 || gi.info.y <= 0.0) {
        return vec3<f32>(0.0);
    }
    let dim = i32(gi.info.z);
    let ext = max(gi.bbox_max.xyz - gi.bbox_min.xyz, vec3<f32>(1e-3));
    let g = (world_pos - gi.bbox_min.xyz) / ext * f32(dim) - vec3<f32>(0.5);
    let gc = clamp(g, vec3<f32>(0.0), vec3<f32>(f32(dim - 1)));
    let i0 = vec3<i32>(floor(gc));
    let f = fract(gc);
    let c000 = sh_fetch(i0.x,     i0.y,     i0.z,     dim);
    let c100 = sh_fetch(i0.x + 1, i0.y,     i0.z,     dim);
    let c010 = sh_fetch(i0.x,     i0.y + 1, i0.z,     dim);
    let c110 = sh_fetch(i0.x + 1, i0.y + 1, i0.z,     dim);
    let c001 = sh_fetch(i0.x,     i0.y,     i0.z + 1, dim);
    let c101 = sh_fetch(i0.x + 1, i0.y,     i0.z + 1, dim);
    let c011 = sh_fetch(i0.x,     i0.y + 1, i0.z + 1, dim);
    let c111 = sh_fetch(i0.x + 1, i0.y + 1, i0.z + 1, dim);
    let x00 = sh_mix(c000, c100, f.x);
    let x10 = sh_mix(c010, c110, f.x);
    let x01 = sh_mix(c001, c101, f.x);
    let x11 = sh_mix(c011, c111, f.x);
    let y0 = sh_mix(x00, x10, f.y);
    let y1 = sh_mix(x01, x11, f.y);
    let cell = sh_mix(y0, y1, f.z);
    // irr_c = L0_c + SH_DIR · dot(L1_c, n).
    let SH_DIR = 2.0;
    let l0 = vec3<f32>(cell.r.x, cell.g.x, cell.b.x);
    let l1 = vec3<f32>(dot(cell.r.yzw, n), dot(cell.g.yzw, n), dot(cell.b.yzw, n));
    // The directional term is an additive boost only (clamped ≥ 0), so a face turned
    // TOWARD a coloured cluster brightens, but a face turned AWAY keeps the flat
    // ambient bleed L0 rather than losing all GI. (Clamping the *combined* sum to 0
    // would let a strongly-negative L1 zero out the still-positive L0.) L0 ≥ 0, so the
    // outer max only guards tiny negative interpolation noise.
    let irr = max(l0 + max(l1 * SH_DIR, vec3<f32>(0.0)), vec3<f32>(0.0));
    return irr * gi.info.y;
}

// Bioluminescent ripple: a travelling band of HDR emission through world space.
// Radial = expanding shells from the field centre; axial = a wavefront along the
// axis. The band brightens the LOCAL albedo so it glows in the palette colour;
// `intensity` is in linear HDR units (push > 1 to bloom / exceed SDR white).
fn ripple_emission(world_pos: vec3<f32>, albedo: vec3<f32>) -> vec3<f32> {
    let intensity = u.ripple.x;
    if (intensity <= 0.0) {
        return vec3<f32>(0.0);
    }
    let phase = u.ripple.y;
    let freq = max(u.ripple.z, 0.0);
    let sharp = max(u.ripple.w, 1.0);
    let center = u.ripple_ctr.xyz;
    let radius = max(u.ripple_ctr.w, 1e-3);
    var coord: f32;
    if (u.ripple_mode.x >= 0.5) {
        let axis = normalize(u.ripple_mode.yzw);
        coord = dot(world_pos - center, axis) / radius * 0.5 + 0.5; // axial, remapped
    } else {
        coord = length(world_pos - center) / radius; // radial: 0 at core, ~1 at edge
    }
    let band = pow(0.5 + 0.5 * cos(6.2831853 * (coord * freq - phase)), sharp);
    return albedo * (intensity * band);
}

// ----- group(1): IBL maps + shared filtering sampler -----
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

// ----- group(2): reaction–diffusion (Turing) surface field -----
@group(2) @binding(0) var rd_tex  : texture_2d<f32>;
@group(2) @binding(1) var rd_samp : sampler;
struct RdLook { params: vec4<f32> }; // x=intensity, y=scale, z=albedo_mix
@group(2) @binding(2) var<uniform> rd: RdLook;

// Triplanar sample of the RD field's V channel, projected onto the surface in
// world space (so it works on every surface mode + tiles seamlessly). Returns a
// 0..1 dapple after a little contrast.
fn rd_dapple(world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    if (rd.params.x <= 0.0 && rd.params.z <= 0.0) {
        return 0.0; // RD fully off (no emissive, no pigment)
    }
    let scale = rd.params.y;
    let an = abs(n);
    let w = an / max(an.x + an.y + an.z, 1e-4);
    let vx = textureSampleLevel(rd_tex, rd_samp, world_pos.yz * scale, 0.0).g;
    let vy = textureSampleLevel(rd_tex, rd_samp, world_pos.xz * scale, 0.0).g;
    let vz = textureSampleLevel(rd_tex, rd_samp, world_pos.xy * scale, 0.0).g;
    let v = vx * w.x + vy * w.y + vz * w.z;
    return clamp((v - 0.2) * 2.5, 0.0, 1.0);
}

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.15915494, 0.31830989); // (1/2π, 1/π)

fn rotate_y(d: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
}

// Unit direction -> equirect UV. U wraps (sampler Repeat); V clamped.
fn dir_to_equirect_uv(dir: vec3<f32>) -> vec2<f32> {
    let d = normalize(dir);
    var uv = vec2<f32>(atan2(d.z, d.x), asin(clamp(d.y, -1.0, 1.0)));
    uv = uv * INV_ATAN + vec2<f32>(0.5, 0.5);
    return uv;
}

// Nudge a near-zero value away from 0 (keeping its sign) so 1/x stays finite.
fn nz(x: f32) -> f32 {
    if (abs(x) < 1e-5) {
        return select(1e-5, -1e-5, x < 0.0);
    }
    return x;
}
// Parallax-corrected reflection direction (#163 Tier 2, box-projected IBL,
// Lagarde/Bouvier). The env map is infinitely far, so a plain `reflect(-v,n)` lookup
// depends only on orientation → the reflection is "painted on". Intersecting the ray
// (p, r) with the field's AABB and re-aiming from the box CENTRE to the hit makes the
// reflection depend on the fragment's *position* too, so it slides across faces as
// cubes move — the depth cue a distant env can't give. `source_id = 0` never calls
// this (today's look). Falls back to `r` on a degenerate/backward hit.
fn parallax_correct(r: vec3<f32>, p: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec3<f32> {
    // Guard axis-aligned rays (|component| ~ 0) so 1/r can't produce inf/NaN.
    let inv = 1.0 / vec3<f32>(nz(r.x), nz(r.y), nz(r.z));
    let t1 = (bmax - p) * inv;
    let t2 = (bmin - p) * inv;
    // Full slab test: `tmin` = entry distance, `tmax` = exit. The AABB is built from
    // node CENTRES, so a fragment on a scaled cube/tube surface is often just OUTSIDE
    // the box — where the correct forward hit is the ENTRY face, not the exit. (Using
    // the exit unconditionally, as before, aimed at the far wall for exterior points and
    // mirrored the reflection on outer faces.)
    let tmin = max(max(min(t1.x, t2.x), min(t1.y, t2.y)), min(t1.z, t2.z));
    let tmax = min(min(max(t1.x, t2.x), max(t1.y, t2.y)), max(t1.z, t2.z));
    if (tmax < max(tmin, 0.0)) {
        return r; // ray misses the box (or the box is behind) → no correction
    }
    // Inside (tmin < 0) → exit hit; outside (tmin > 0) → the near/entry hit.
    let t_hit = select(tmax, tmin, tmin > 0.0);
    if (t_hit <= 0.0) {
        return r;
    }
    let hit = p + r * t_hit;
    let center = (bmin + bmax) * 0.5;
    return hit - center; // caller normalizes (dir_to_equirect_uv does)
}

// ===================== node bevel (rounded-box morph) =====================
// Round a local cube-surface position (half-extent 0.5) toward a rounded box, growing
// with `bevel` ∈ [0,1]: at 0 the sharp cube is returned unchanged; at 0.5 a wide
// rounded cube (flat faces, rounded edges/corners of radius 0.25); at 1 a full sphere.
// Classic SDF rounded-box surface map: clamp the point into an inner box of half-size
// `0.5·(1−bevel)`, then push out by `0.5·bevel` along the direction to the clamped
// point. When bevel = 1 the inner box collapses to the origin → `normalize(p)·0.5`, a
// sphere. Needs a SUBDIVIDED cube mesh (interior face vertices) to actually bulge.
// The morph is applied in LOCAL space (before the instance matrix), so a stretched
// Flow-Aligned rod rounds into a smooth capsule/ellipsoid naturally. Returns the
// rounded position; the outward normal in the rounded region is `normalize(d)` (which
// equals the flat face normal where the point is still on a face), so callers derive
// the shaded normal from the same `d`.
struct Rounded { pos: vec3<f32>, has_normal: bool, normal: vec3<f32> };
fn round_local(p: vec3<f32>, bevel: f32) -> Rounded {
    var out: Rounded;
    if (bevel <= 0.0) {
        out.pos = p;
        out.has_normal = false;
        out.normal = vec3<f32>(0.0, 0.0, 1.0);
        return out;
    }
    let h = 0.5;
    let inner = h * (1.0 - bevel);
    let c = clamp(p, vec3<f32>(-inner), vec3<f32>(inner));
    let d = p - c;
    let dl = length(d);
    if (dl < 1e-6) {
        // Deep interior (only possible at bevel→1 for a near-axis point): leave it.
        out.pos = p;
        out.has_normal = false;
        out.normal = vec3<f32>(0.0, 0.0, 1.0);
        return out;
    }
    let dir = d / dl;
    out.pos = c + dir * (h * bevel);
    out.has_normal = true;
    out.normal = dir;
    return out;
}

// ===================== vertex =====================
struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) tint: vec4<f32>, // per-instance colour (×albedo); white = no change
};

struct VsOut {
    // `@invariant`: guarantees this position is computed identically here and in
    // the depth-only prepass (same shader), so the opaque scene pass can use a
    // `depth_compare: Equal` test against the prepass depth without z-fighting.
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    // Refractive optics (the Refractive material + the refraction overlay): the
    // fragment's mesh-local position plus
    // the instance frame's inverse columns, so the fragment stage can measure the
    // world-space chord through the instance's unit-cube body along the refracted
    // ray (see `instance_thickness`). Constant-per-instance (inv*) or affine in
    // world position (local_pos), so plain interpolation is exact.
    @location(3) local_pos: vec3<f32>,
    @location(4) inv0: vec3<f32>,
    @location(5) inv1: vec3<f32>,
    @location(6) inv2: vec3<f32>,
    // Anisotropy (#214 T1): the instance's local +Z axis in world space — the
    // natural brush/streak direction (the rod/tube long axis; `cyl_mesh` spans
    // local +Z, and the per-segment matrices stretch it node→node). Constant per
    // instance, so it interpolates exactly; the fragment normalizes it and builds
    // the tangent frame. Only read when anisotropy is active.
    @location(7) brush: vec3<f32>,
};

// #472 Tier 5 — height→vertex displacement. Offsets the world vertex along its
// world normal by the baked height field. Called IDENTICALLY from `vs_main` and
// `vs_depth` (same inputs, pure function) so the `@invariant` prepass position
// still matches bit-for-bit and the opaque pass's `depth_compare: Equal` holds.
// Inert (returns `world` unchanged) unless the material texture set is on
// (`u.mtl.x`), the displace amount (`u.mtl.w`, from `material_live[5]`) is > 0,
// AND a height map is present (present-mask bit 32). `textureSampleLevel` (mip 0)
// because the vertex stage has no implicit derivatives. The triplanar UV mirrors
// the fragment's `mat_uv` (same `u.mtl.y` projection / `u.mtl.z` scale), so the
// displacement height lookup aligns with the shaded height/derived maps.
fn mat_displace_world(world: vec3<f32>, base_normal: vec3<f32>, nm: mat3x3<f32>) -> vec3<f32> {
    let amt = u.mtl.w;
    let has_height = (u32(round(u.mtl2.w)) & 32u) != 0u;
    if (u.mtl.x <= 0.5 || amt <= 0.0 || !has_height) { return world; }
    let dir = nm * base_normal;
    let dl = length(dir);
    if (dl < 1e-6) { return world; }
    let scale = max(u.mtl.z, 1e-4);
    var uv: vec2<f32>;
    if (u.mtl.y >= 1.5) {
        uv = world.xy * scale;        // object-planar (fragment passes world_pos)
    } else if (u.mtl.y >= 0.5) {
        uv = world.xz * scale;        // world-planar XZ
    } else {
        let an = abs(dir / dl);       // triplanar → dominant axis
        if (an.x >= an.y && an.x >= an.z) {
            uv = world.zy * scale;
        } else if (an.y >= an.z) {
            uv = world.xz * scale;
        } else {
            uv = world.xy * scale;
        }
    }
    let h = textureSampleLevel(mat_height_tex, mat_samp, uv, 0.0).r;
    return world + (dir / dl) * ((h - 0.5) * amt);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    // Node bevel: round the local cube position (and its normal) before the instance
    // transform. bevel = 0 → `rounded.pos == in.position` and `has_normal == false`,
    // so this is byte-identical to the sharp cube. MUST match `vs_depth` exactly (the
    // opaque pass's `depth_compare: Equal` relies on the invariant prepass position).
    let rounded = round_local(in.position, u.shape.x);
    let local = rounded.pos;
    let base_normal = select(in.normal, rounded.normal, rounded.has_normal);
    let nm = mat3x3<f32>(in.m0.xyz, in.m1.xyz, in.m2.xyz);
    // #472 Tier 5: height→vertex displacement (inert at amount 0 / no height map).
    // Applied here and in `vs_depth` via the SAME helper + inputs so `@invariant` holds.
    let world = vec4<f32>(mat_displace_world((model * vec4<f32>(local, 1.0)).xyz, base_normal, nm), 1.0);

    var out: VsOut;
    out.clip = u.view_proj * world;
    out.world_pos = world.xyz;
    // Inverse of the instance's 3×3 frame via the adjugate (WGSL has no
    // inverse()). Degenerate (zero-scale) instances get a zero matrix — the
    // thickness helper rejects it. Read by the Refractive branch and by any
    // branch with the refraction overlay on.
    let adj0 = cross(in.m1.xyz, in.m2.xyz);
    let det = dot(in.m0.xyz, adj0);
    let inv_det = select(0.0, 1.0 / det, abs(det) > 1e-12);
    let inv_nm = transpose(mat3x3<f32>(
        adj0,
        cross(in.m2.xyz, in.m0.xyz),
        cross(in.m0.xyz, in.m1.xyz),
    ));
    out.local_pos = in.position;
    out.inv0 = inv_nm[0] * inv_det;
    out.inv1 = inv_nm[1] * inv_det;
    out.inv2 = inv_nm[2] * inv_det;
    // Inverse-scale the normal (#174 T3): for M = R·S the correct normal transform
    // divides by the squared per-axis scale (∝ the inverse-transpose). Identical
    // direction for the cube (axis-aligned normals) and the cylinder (uniform xy
    // cross-section), but revolution/creature meshes with th≠len stretch had their
    // normals skewed toward the long axis.
    let inv2 = vec3<f32>(
        1.0 / max(dot(in.m0.xyz, in.m0.xyz), 1e-12),
        1.0 / max(dot(in.m1.xyz, in.m1.xyz), 1e-12),
        1.0 / max(dot(in.m2.xyz, in.m2.xyz), 1e-12),
    );
    out.world_normal = normalize(nm * (base_normal * inv2));
    // amb.w = palette-active: an explicit LUT replaces the mesh's RGB-cube colour
    // with the per-instance tint; otherwise the tint multiplies it (white = none).
    // tint.w < 0.5 opts the instance OUT of the replace (the Membrane mesh sets
    // w=0 so its per-vertex colour is used even when a palette is active).
    let use_tint = u.amb.w > 0.5 && in.tint.w > 0.5;
    out.color = select(in.color * in.tint.rgb, in.tint.rgb, use_tint);
    // Brush direction for anisotropy: the instance's local +Z basis in world.
    out.brush = in.m2.xyz;
    return out;
}

// Depth-only vertex stage for the prepass / shadow pipelines (#174 T2): those
// pipelines have NO fragment stage, yet ran the full `vs_main` — normal
// transform, palette/tint colour select, world-position varyings, all dead work
// multiplied across up to three depth-only rasterizations of the whole field per
// frame. This computes ONLY the clip position. The position expression mirrors
// `vs_main`'s exactly and both are `@invariant`, so the opaque scene pass's
// `depth_compare: Equal` against prepass depth still matches bit-for-bit.
struct VsDepthIn {
    @location(0) position: vec3<f32>,
    // #472 Tier 5: the mesh normal (the prepass vertex layout already binds loc 1 —
    // it was declared but unread). Needed so displacement picks the same world
    // direction `vs_main` does, keeping the `@invariant` prepass position matched.
    @location(1) normal: vec3<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
};
struct VsDepthOut {
    @invariant @builtin(position) clip: vec4<f32>,
};
@vertex
fn vs_depth(in: VsDepthIn) -> VsDepthOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    // Same rounded-box morph + #472 Tier 5 height displacement as `vs_main`. This
    // MUST mirror `vs_main`'s position expression bit-for-bit — both are `@invariant`
    // and the opaque pass tests `depth_compare: Equal` against this prepass depth — so
    // it calls the SAME `mat_displace_world` helper with the same inputs.
    let rounded = round_local(in.position, u.shape.x);
    let local = rounded.pos;
    let base_normal = select(in.normal, rounded.normal, rounded.has_normal);
    let nm = mat3x3<f32>(in.m0.xyz, in.m1.xyz, in.m2.xyz);
    let world = vec4<f32>(mat_displace_world((model * vec4<f32>(local, 1.0)).xyz, base_normal, nm), 1.0);
    var out: VsDepthOut;
    out.clip = u.view_proj * world;
    return out;
}

// ===================== BRDF helpers =====================
fn fresnel_schlick_roughness(cos_theta: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let one_minus_r = vec3<f32>(1.0 - roughness);
    return f0 + (max(one_minus_r, f0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// --- Analytic (direct-light) Cook-Torrance terms, ported from rust-pbr ---
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// GGX/Trowbridge-Reitz normal distribution. α is floored (≙ roughness 0.02, the
// same floor Chrome/Glass apply to their whole direct term): at roughness 0 and
// h == n the raw quotient is 0/0 = NaN, and one NaN pixel snowballs through the
// bloom downsample chain into a growing blotch.
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = max(roughness * roughness, 4e-4);
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

// Schlick-GGX geometry term with the direct-lighting `k` (Disney remap).
fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let g_v = geometry_schlick_ggx(max(dot(n, v), 0.0), roughness);
    let g_l = geometry_schlick_ggx(max(dot(n, l), 0.0), roughness);
    return g_v * g_l;
}

// Reflected radiance from one directional light. `l` is the unit direction from
// the surface toward the light; `radiance` is the light colour × intensity.
fn direct_light(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let f = fresnel_schlick(max(dot(h, v), 0.0), f0);
    let d = distribution_ggx(n, h, roughness);
    let g = geometry_smith(n, v, l, roughness);
    // Clamp N·V away from 0 instead of leaning on a large additive epsilon: the
    // Smith G term already goes to zero with N·V, so with the clamp the quotient
    // stays bounded at grazing angles without the epsilon biasing every pixel.
    let specular = (d * f * g) / (4.0 * max(dot(n, v), 1e-4) * n_dot_l + 1e-4);
    // Energy split: metals have no diffuse; Fresnel steals from the diffuse lobe.
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    return (kd * albedo / PI + specular) * radiance * n_dot_l;
}

// Demo point light (#288 Tier 3): a single placeable emitter (the Cornell ceiling
// panel / a light-stage rig light) with inverse-square falloff, softened by the
// light radius (a crude area-light approximation). `u.demo_light_pos.w` (intensity)
// = 0 → returns 0, so every non-Demo frame is byte-identical. Cook-Torrance via the
// shared `direct_light`, so it lights Standard/Chrome consistently with key+fill.
fn demo_point_light(world: vec3<f32>, n: vec3<f32>, v: vec3<f32>,
                    albedo: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let intensity = u.demo_light_pos.w;
    if (intensity <= 0.0) {
        return vec3<f32>(0.0);
    }
    let to = u.demo_light_pos.xyz - world;
    let dist = length(to);
    let l = to / max(dist, 1e-3);
    let r = max(u.demo_light_col.w, 1e-3);
    let atten = 1.0 / (1.0 + (dist * dist) / (r * r));
    let radiance = u.demo_light_col.xyz * intensity * atten;
    return direct_light(n, v, l, albedo, metallic, roughness, f0, radiance);
}

// ===================== anisotropy (#214 Tier 1) =====================
// Elliptical GGX instead of round: brushed metal, satin, hair. The one input a
// standard PBR shader lacks is a consistent tangent — here it comes free from the
// instance frame (the local +Z long axis, passed as `brush`). All of this is
// inert unless the material is Anisotropic (type 4) or the anisotropy overlay is
// on Standard/Chrome; at amount 0 the frame is unused and shading is byte-identical.

struct AnisoFrame { t: vec3<f32>, b: vec3<f32> };

// Orthonormal tangent/bitangent on the surface: project the world-space brush into
// the tangent plane, rotate around N by `rot`, complete the basis. Falls back to an
// arbitrary perpendicular when the brush is parallel to N (a cube face whose normal
// IS the long axis) so the frame is always well-defined.
fn aniso_frame(n: vec3<f32>, brush_world: vec3<f32>, rot: f32) -> AnisoFrame {
    var t = brush_world - n * dot(n, brush_world);
    if (dot(t, t) < 1e-8) {
        let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
        t = a - n * dot(n, a);
    }
    t = normalize(t);
    let b = cross(n, t);
    let c = cos(rot);
    let s = sin(rot);
    var out: AnisoFrame;
    out.t = normalize(t * c + b * s);
    out.b = normalize(b * c - t * s);
    return out;
}

// Split a scalar roughness into tangent/bitangent GGX roughnesses (Disney aspect).
// Sign of `amount` flips which axis stretches; 0 → isotropic (at == ab).
fn aniso_alpha(roughness: f32, amount: f32) -> vec2<f32> {
    let a_abs = clamp(abs(amount), 0.0, 1.0);
    let aspect = sqrt(1.0 - 0.9 * a_abs);
    let a = max(roughness * roughness, 4e-4);
    var at = a / aspect;
    var ab = a * aspect;
    if (amount < 0.0) {
        let tmp = at;
        at = ab;
        ab = tmp;
    }
    return vec2<f32>(at, ab);
}

// Anisotropic Cook-Torrance from one directional light — Burley GGX NDF + Heitz
// height-correlated Smith visibility (the visibility already folds in 1/(4·nv·nl)).
// `at`/`ab` are the tangent/bitangent roughnesses. Diffuse is the isotropic lobe.
fn direct_light_aniso(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, t: vec3<f32>, b: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, at: f32, ab: f32, f0: vec3<f32>,
    radiance: vec3<f32>,
) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let t_dot_h = dot(t, h);
    let b_dot_h = dot(b, h);
    // Burley anisotropic GGX distribution.
    let dd = t_dot_h * t_dot_h / (at * at) + b_dot_h * b_dot_h / (ab * ab) + n_dot_h * n_dot_h;
    let dist = 1.0 / (PI * at * ab * dd * dd);
    // Heitz anisotropic Smith visibility (= G / (4·nv·nl)).
    let lv = n_dot_l * length(vec3<f32>(at * dot(t, v), ab * dot(b, v), n_dot_v));
    let ll = n_dot_v * length(vec3<f32>(at * dot(t, l), ab * dot(b, l), n_dot_l));
    let vis = 0.5 / max(lv + ll, 1e-5);
    let fres = fresnel_schlick(max(dot(h, v), 0.0), f0);
    let spec = dist * vis * fres;
    let kd = (vec3<f32>(1.0) - fres) * (1.0 - metallic);
    return (kd * albedo / PI + spec) * radiance * n_dot_l;
}

// Bent reflection vector for the anisotropic ENV lobe (Filament's approximation):
// warp the reflection toward the streak axis so the prefiltered env smears into an
// elongated highlight. `amount ≥ 0` stretches along the tangent (brush), `< 0`
// across it. Reuses the existing prefiltered mips (no new textures).
fn aniso_reflect(n: vec3<f32>, v: vec3<f32>, t: vec3<f32>, b: vec3<f32>,
                 amount: f32, roughness: f32) -> vec3<f32> {
    let dir = select(t, b, amount >= 0.0);
    let a_t = cross(dir, v);
    let a_n = cross(a_t, dir);
    let bend = abs(amount) * clamp(5.0 * roughness, 0.0, 1.0);
    let bent = normalize(mix(n, a_n, bend));
    return reflect(-v, bent);
}

// ===================== surface lobes (#214 Tier 2) =====================
// Clearcoat: a thin smooth dielectric layer (F0 = 0.04, ≙ IOR 1.5) over the base —
// car paint / lacquer / ceramic / wet. Sheen: a retroreflective grazing-angle fuzz
// (Charlie NDF + Neubelt visibility) — velvet / dust / moss. Both are the pure
// `Clearcoat`/`Velvet` materials AND opt-in overlays on Standard/Chrome; all inert
// at 0 (byte-identical). The lobes layer on top of whatever the base branch shades.

// Charlie sheen NDF (Estevez & Kulla) — the inverted-Gaussian microflake lobe that
// blooms at grazing angles instead of head-on like GGX.
fn sheen_d_charlie(roughness: f32, n_dot_h: f32) -> f32 {
    let inv_a = 1.0 / max(roughness, 1e-3);
    let sin2 = max(1.0 - n_dot_h * n_dot_h, 0.0);
    return (2.0 + inv_a) * pow(sin2, inv_a * 0.5) / (2.0 * PI);
}

// One directional light's sheen response (scalar; caller weights by radiance ×
// sheen colour). Neubelt visibility keeps the grazing fuzz soft and bounded.
fn sheen_lobe(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return 0.0;
    }
    let n_dot_v = max(dot(n, v), 1e-4);
    let h = normalize(v + l);
    let n_dot_h = clamp(dot(n, h), 0.0, 1.0);
    let d = sheen_d_charlie(roughness, n_dot_h);
    let vis = 1.0 / (4.0 * (n_dot_l + n_dot_v - n_dot_l * n_dot_v)); // Neubelt
    return d * vis * n_dot_l;
}

// ===================== microstructure (#214 Tier 4) =====================
// Glitter, diffraction, retroreflection — high-frequency detail the stochastic-glass
// + #200 blue-noise + TAA machinery already knows how to resolve. All additive /
// multiplicative and inert at 0 (byte-identical). Woven into Standard + Chrome.

// Dave Hoskins hash (hash-without-sine): one and three floats from a cell coord.
fn hash13(p3in: vec3<f32>) -> f32 {
    var p3 = fract(p3in * 0.1031);
    p3 += dot(p3, p3.zyx + 31.32);
    return fract((p3.x + p3.y) * p3.z);
}
fn hash33(p3in: vec3<f32>) -> vec3<f32> {
    var p3 = fract(p3in * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yxz + 33.33);
    return fract((p3.xxy + p3.yxx) * p3.zyx);
}

// Procedural glitter: partition world space into cells, give each a random
// microfacet, and let a cell flash when that facet's half-vector aligns with the
// light. A slow per-cell phase (advanced by the frame seed) twinkles flakes in and
// out; sparse activation keeps them as discrete sparks. TAA + blue noise average the
// per-frame shimmer into clean glitter (grainy with TAA off, like stochastic glass).
fn glitter_glint(world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
                 radiance: vec3<f32>, density: f32, sharpness: f32, seed: f32) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) {
        return vec3<f32>(0.0);
    }
    let cell = floor(world * max(density, 0.1));
    let rnd = hash33(cell);
    let fnrm = normalize(n + (rnd - vec3<f32>(0.5)) * 1.4); // jittered facet
    let h = normalize(v + l);
    let spec = pow(max(dot(fnrm, h), 0.0), mix(80.0, 2000.0, clamp(sharpness, 0.0, 1.0)));
    // Sparse + twinkling: a per-cell phase gives a slow on/off; only the upper hash
    // band ever sparkles, so most cells stay dark (discrete flakes, not a sheen).
    let phase = hash13(cell) * 6.2831853 + seed * 0.5;
    let flake = smoothstep(0.55, 1.0, (0.5 + 0.5 * sin(phase)) * hash13(cell + 4.0) * 1.6);
    return radiance * (spec * flake * n_dot_l);
}

// Diffraction grating: a periodic microstructure splits the reflection into
// wavelength-dependent orders — a rainbow that shifts with view angle (CD / holo-
// foil, especially over Chrome). Closed form: cycle an IQ cosine palette by the
// angle × frequency. Returns a tint (white-ish energy) to multiply the env specular.
fn diffraction_tint(cos_ang: f32, freq: f32) -> vec3<f32> {
    let phase = cos_ang * freq;
    return 0.5 + 0.5 * cos(6.2831853 * (phase + vec3<f32>(0.0, 0.33, 0.67)));
}

// Retroreflection: light bounced back toward its source — bright when the camera
// looks along the light (road sign / cat's-eye). `dot(v, l)` peaks when view ≈ light.
fn retro_glow(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>,
              radiance: vec3<f32>, amount: f32) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    let back = pow(max(dot(v, l), 0.0), 3.0);
    return albedo * radiance * (back * n_dot_l * amount);
}

// Emissive cubes as real lights (#167 Tier 3): loop the uploaded point lights and add
// Cook-Torrance direct lighting from each (smooth inverse-square falloff). count 0 → 0
// (today's look). A per-emitter dead-zone stops a cube blowing itself out.
fn many_lights(world: vec3<f32>, n: vec3<f32>, v: vec3<f32>, albedo: vec3<f32>,
               metallic: f32, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let count = min(i32(mlights.header.x), 64);
    if (count <= 0) {
        return vec3<f32>(0.0);
    }
    let intensity = mlights.header.y;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < count; i = i + 1) {
        let lp = mlights.lights[i].pos.xyz;
        let radius = max(mlights.lights[i].pos.w, 1e-3);
        let to_l = lp - world;
        let dist = length(to_l);
        if (dist < radius * 0.08) {
            continue; // the emitter's own surface — skip
        }
        let l = to_l / max(dist, 1e-4);
        // Windowed falloff (UE4-style): the bare 1/(1+d²/r²) never reaches zero, so
        // every light lit every fragment forever — an ambient floor, and 64 full
        // Cook-Torrance evaluations per fragment with no early-out. The window
        // takes the light smoothly to exactly 0 at 4×radius, and the `continue`
        // then skips the BRDF for every out-of-range light.
        let window = pow(clamp(1.0 - pow(dist / (radius * 4.0), 4.0), 0.0, 1.0), 2.0);
        let atten = window / (1.0 + (dist * dist) / (radius * radius));
        if (atten < 1e-4) {
            continue;
        }
        let radiance = mlights.lights[i].color.rgb * intensity * atten;
        acc = acc + direct_light(n, v, l, albedo, metallic, roughness, f0, radiance);
    }
    return acc;
}

fn sample_irradiance(n_env: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(irradiance_tex, ibl_samp, dir_to_equirect_uv(n_env), 0.0).rgb;
}

// ===================== live-sky cloud reflections (#305 Tier 2) =====================
// A drifting procedural cloud layer that modulates the SHARP environment reflection,
// so chrome/glass materials (and the beads) show moving clouds instead of a frozen
// sky. Cheap value-noise fBm on the reflection direction projected onto the sky dome,
// scrolled by `skyrefl.z` (the beat-accumulated drift phase). Returns a brightness
// MULTIPLIER (1 = unchanged); gated to the upper hemisphere + sharp reflections, and
// off entirely when `skyrefl.x < 0.5` (byte-identical default).
fn cloud_hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn cloud_vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u2 = f * f * (3.0 - 2.0 * f);
    let a = cloud_hash2(i);
    let b = cloud_hash2(i + vec2<f32>(1.0, 0.0));
    let c = cloud_hash2(i + vec2<f32>(0.0, 1.0));
    let d = cloud_hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u2.x), mix(c, d, u2.x), u2.y);
}
fn cloud_fbm(p: vec2<f32>) -> f32 {
    var s = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        s = s + amp * cloud_vnoise(q);
        q = q * 2.03;
        amp = amp * 0.5;
    }
    return s;
}
fn cloud_bright(dir: vec3<f32>, roughness: f32, sky: vec4<f32>) -> f32 {
    if (sky.x < 0.5) {
        return 1.0;
    }
    let up = smoothstep(-0.02, 0.35, dir.y); // sky dome only; fade at the horizon
    let sharp = clamp(1.0 - roughness, 0.0, 1.0); // sharp reflections carry crisp clouds
    let amt = up * sharp * clamp(sky.w, 0.0, 1.0);
    if (amt <= 0.0) {
        return 1.0;
    }
    // Project the reflection dir onto a sky plane and scroll it by the drift phase.
    let uv = dir.xz / max(dir.y + 0.25, 0.15) * 1.2 + vec2<f32>(sky.z, sky.z * 0.5);
    let n = cloud_fbm(uv);
    let cover = clamp(sky.y, 0.0, 1.0);
    let mask = smoothstep(1.0 - cover - 0.15, 1.0 - cover + 0.15, n);
    let bright = mix(0.65, 1.7, mask); // gaps darker, sunlit clouds brighter
    return mix(1.0, bright, amt);
}

fn sample_prefiltered(r_env: vec3<f32>, roughness: f32) -> vec3<f32> {
    let mip_count = u.mat.w;
    let lod = roughness * max(mip_count - 1.0, 0.0);
    let base = textureSampleLevel(prefilter_tex, ibl_samp, dir_to_equirect_uv(r_env), lod).rgb;
    // #305 Tier 2: drifting cloud modulation on the sharp env reflection (identity off).
    return base * cloud_bright(r_env, roughness, u.skyrefl);
}

// ===================== additive surface modifiers =====================
// These layer ON TOP of whatever material_type is active — they don't replace
// it. Both are no-ops at amount 0, so the base look is untouched until dialed up.

// View-angle thin-film iridescence. The optical path difference of a thin film
// grows toward grazing angles (∝ thickness · (1 − cosθ)); converting that path
// length to a hue gives the soap-bubble / beetle-shell spectral shift that
// sweeps as the surface rotates. `scale` ≈ film thickness (number of colour
// bands), `shift` offsets the starting hue. Returns an RGB reflectance tint.
fn iridescent_tint(cos_theta: f32, scale: f32, shift: f32) -> vec3<f32> {
    let opd = scale * (1.0 - clamp(cos_theta, 0.0, 1.0));
    let phase = (opd + shift) * 2.0 * PI;
    // Three phase-offset cosines approximate the R/G/B spectral interference.
    return 0.5 + 0.5 * cos(vec3<f32>(phase) + vec3<f32>(0.0, 2.0944, 4.1888));
}

// Back-scatter translucency (Barré-Brisebois fast subsurface). The light
// direction is bent around the surface by `distortion`; where the view aligns
// with that transmitted light the surface glows through. Returns a scalar lobe
// to scale by albedo × light radiance × amount.
fn translucency_lobe(v: vec3<f32>, l: vec3<f32>, n: vec3<f32>, distortion: f32, power: f32) -> f32 {
    let lt = normalize(l + n * distortion);
    return pow(clamp(dot(v, -lt), 0.0, 1.0), power);
}

// ===================== spectral glass (#80 Part C) =====================
// Thin-film interference tint — a few spectral samples of the optical path
// difference give a physically-motivated oil-slick/soap colour that shifts with
// view angle (richer than `iridescent_tint`). Used by the Glass branch when
// `thin_film > 0`; returns an RGB reflectance multiplier centred on white (so 0 =
// no change).
fn thin_film_tint(cos_theta: f32, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return vec3<f32>(1.0);
    }
    let opd = (1.0 - clamp(cos_theta, 0.0, 1.0)) * 6.2831853 * (2.0 + amount * 6.0);
    let tint = 0.5 + 0.5 * cos(vec3<f32>(opd) + vec3<f32>(0.0, 2.0944, 4.1888));
    return mix(vec3<f32>(1.0), tint, clamp(amount, 0.0, 1.0));
}

// ============ physical thin-film interference (soap film / bubbles, #258 T1) ============
// A real wavelength-resolved Airy interference model, gated behind
// `u.thinfilm.x` (base thickness in nm) > 0. Two reflections — top (air→film) and
// bottom (film→air) — interfere with a phase difference
//   Δφ = 4π·n·d·cosθ_t / λ
// that depends on the ACTUAL film thickness `d` (nm), the film index `n`, and the
// refraction angle inside the film. Evaluated at three wavelengths (R/G/B) → the
// spectral reflectance the highlight is tinted by. Unlike `thin_film_tint` /
// `iridescent_tint` (a view-angle cosine hack), the colour here tracks a real
// thickness field, so gravity drainage + noise marbling paint moving bands.

// Unpolarized Fresnel reflectance of a single dielectric interface.
fn fresnel_dielectric(cos_i: f32, n1: f32, n2: f32) -> f32 {
    let s2 = (n1 / n2) * (n1 / n2) * max(1.0 - cos_i * cos_i, 0.0);
    if (s2 >= 1.0) { return 1.0; } // total internal reflection
    let cos_t = sqrt(1.0 - s2);
    let rs = (n1 * cos_i - n2 * cos_t) / (n1 * cos_i + n2 * cos_t);
    let rp = (n1 * cos_t - n2 * cos_i) / (n1 * cos_t + n2 * cos_i);
    return clamp(0.5 * (rs * rs + rp * rp), 0.0, 1.0);
}

// Physical film thickness (nm) at a surface point: base thickness, thinned toward
// the top / thickened toward the bottom by gravity drainage (world up = +y), and
// marbled by value noise. Each extra term folds to the base when its amount is 0.
fn film_thickness_at(world_pos: vec3<f32>) -> f32 {
    let base = u.thinfilm.x;
    let drainage = u.thinfilm.w;
    let marble = u.thinfilm.y;
    // Thin at the top (high y) → thick at the bottom (low y): a smooth signed ramp.
    let grad = -tanh(world_pos.y * 0.12);
    var d = base * (1.0 + drainage * grad);
    // Value-noise marbling: swirl the thickness so the bands aren't perfectly regular.
    let n0 = hash13(floor(world_pos * 1.7));
    let n1 = hash13(floor(world_pos * 0.6) + vec3<f32>(11.0));
    d = d * (1.0 + marble * (n0 * 0.7 + n1 * 0.3 - 0.5));
    return max(d, 0.0);
}

// Low-finesse Airy reflectance of the film at one wavelength.
fn film_airy(cos_t: f32, d: f32, n_film: f32, lambda: f32, r_top: f32) -> f32 {
    // Δφ = 4π·n·d·cosθ_t / λ. The extra π between the two interfaces is folded into
    // the sin²(φ/2) low-finesse form (reflectance peaks at odd half-waves).
    let phase = (4.0 * PI * n_film * d * cos_t) / max(lambda, 1.0);
    let s = sin(phase * 0.5);
    let s2 = s * s;
    let r = clamp(r_top, 0.0, 0.999);
    return (4.0 * r * s2) / ((1.0 - r) * (1.0 - r) + 4.0 * r * s2);
}

// Spectral reflectance (R/G/B) of the physical thin film at a surface point.
fn thin_film_physical(cos_theta: f32, world_pos: vec3<f32>) -> vec3<f32> {
    let n_film = max(u.thinfilm.z, 1.0);
    let d = film_thickness_at(world_pos);
    let cos_i = clamp(cos_theta, 0.02, 1.0);
    // Refraction angle inside the film (air n0 = 1).
    let s2 = (1.0 / n_film) * (1.0 / n_film) * max(1.0 - cos_i * cos_i, 0.0);
    let cos_t = sqrt(max(1.0 - s2, 0.0));
    let r_top = fresnel_dielectric(cos_i, 1.0, n_film);
    // Dominant wavelengths (nm) for the sRGB R/G/B primaries.
    return vec3<f32>(
        film_airy(cos_t, d, n_film, 680.0, r_top),
        film_airy(cos_t, d, n_film, 550.0, r_top),
        film_airy(cos_t, d, n_film, 440.0, r_top),
    );
}

// Refract one wavelength's channel through the environment at a given IOR.
// Total-internal-reflection falls back to the reflection (so edges never go black).
fn sample_refract_chan(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32) -> vec3<f32> {
    let refr_dir = refract(-v, n, 1.0 / max(ior, 1.0));
    if (dot(refr_dir, refr_dir) < 1e-6) {
        return sample_prefiltered(rotate_y(reflect(-v, n), env_rot), roughness);
    }
    return sample_prefiltered(rotate_y(refr_dir, env_rot), roughness);
}

// Chromatic refraction of the environment through glass: refract at three
// wavelength-offset IORs around the base `ior` so edges fringe into a rainbow.
// `dispersion = 0` → all three IORs equal → identical to a single refraction
// (today's glass). `caustic` brightens focused bright spots seen through the body
// (a cheap stand-in for projected caustics; a true light-space pass is a follow-up).
fn glass_dispersion(v: vec3<f32>, n: vec3<f32>, ior: f32, roughness: f32, env_rot: f32,
                    dispersion: f32, caustic: f32) -> vec3<f32> {
    let spread = dispersion * 0.06; // fractional IOR offset between R and B
    let cr = sample_refract_chan(v, n, ior * (1.0 - spread), roughness, env_rot);
    let cg = sample_refract_chan(v, n, ior,                  roughness, env_rot);
    let cb = sample_refract_chan(v, n, ior * (1.0 + spread), roughness, env_rot);
    var col = vec3<f32>(cr.r, cg.g, cb.b);
    if (caustic > 0.0) {
        let lum = max(max(col.r, col.g), col.b);
        col += col * smoothstep(0.6, 2.0, lum) * caustic * 2.0;
    }
    return col;
}

// ===================== refractive body thickness =====================
// World-space chord through THIS instance's unit-cube body along a unit world
// direction, starting at the fragment (the Refractive material's Beer–Lambert
// path length — the instanced analog of the liquid surface's marched thickness).
// The local ray direction is the instance-inverse-transformed world direction
// left UNNORMALIZED: for local p(t) = lp + (inv·dir)·t the world point is exactly
// `world_pos + dir·t`, so the slab parameter t IS world distance (no forward
// transform needed). The unit box is exact for the cube mesh, a close
// over-estimate for the cylinder/creature meshes; geometry that isn't a closed
// instanced body is rejected by the `lp` bound (the membrane sheet draws with an
// identity instance, so its "local" coords are world-scale and land outside).
fn instance_thickness(lp: vec3<f32>, inv_nm: mat3x3<f32>, dir: vec3<f32>) -> f32 {
    if (any(abs(lp) > vec3<f32>(0.85))) {
        return 0.0;
    }
    let ld = inv_nm * dir;
    if (dot(ld, ld) < 1e-12) {
        return 0.0; // degenerate (zero-scale) instance
    }
    // Robust slab test vs the ±0.5 box; the near-zero clamp keeps sign and
    // avoids the 0·∞ NaN when the fragment sits exactly on an axis plane.
    let ld_safe = select(ld, vec3<f32>(1e-20), abs(ld) < vec3<f32>(1e-20));
    let inv = 1.0 / ld_safe;
    let ta = (vec3<f32>(-0.5) - lp) * inv;
    let tb = (vec3<f32>(0.5) - lp) * inv;
    let tsm = min(ta, tb);
    let tbg = max(ta, tb);
    let t0 = max(max(tsm.x, tsm.y), max(tsm.z, 0.0));
    let t1 = min(min(tbg.x, tbg.y), tbg.z);
    return max(t1 - t0, 0.0);
}

// Refraction overlay (u.refr.y/z): the Refractive material's transmission as an
// optional layer ON the other material types. Refract the view ray at the live
// IOR, sample the env along it (the caller's roughness keeps each material's
// character — frosted on rough Standard, polished on Chrome), and attenuate by
// Beer–Lambert over the measured chord through this instance's body (σ per
// channel = (1 − albedo) × u.refr.x — the node's own colour survives). Each
// branch weaves the result into its own shading, so the overlay builds on the
// material instead of replacing it.
fn refr_overlay_trans(lp: vec3<f32>, inv_nm: mat3x3<f32>, v: vec3<f32>, n: vec3<f32>,
                      ior: f32, rough: f32, env_rot: f32, albedo: vec3<f32>) -> vec3<f32> {
    var tdir = refract(-v, n, 1.0 / ior);
    if (dot(tdir, tdir) < 1e-6) {
        tdir = reflect(-v, n); // total internal reflection: the ray still bounces through
    }
    tdir = normalize(tdir);
    return sample_prefiltered(rotate_y(tdir, env_rot), rough)
        * refr_overlay_absorb(lp, inv_nm, tdir, albedo);
}

// The Beer–Lambert body attenuation alone (also used by the Glass branch's
// murk). `albedo` is passed separately so the caller controls the surviving
// colour; strength 0 → vec3(1) (no change).
fn refr_overlay_absorb(lp: vec3<f32>, inv_nm: mat3x3<f32>, tdir: vec3<f32>,
                       albedo: vec3<f32>) -> vec3<f32> {
    if (u.refr.x <= 0.0) {
        return vec3<f32>(1.0);
    }
    let thickness = instance_thickness(lp, inv_nm, tdir);
    let sigma = (vec3<f32>(1.0) - clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0))) * u.refr.x;
    return exp(-sigma * thickness);
}

// ===================== spectral emission (#214 Tier 5 pt 1) =====================
// Fluorescence + blackbody incandescence — additive emissive terms woven into every
// material branch via `emissive`. Both inert at 0 (byte-identical).

// Pure-hue → RGB (h in 0..1), for the fluorescence emit colour.
fn hue2rgb(h: f32) -> vec3<f32> {
    let r = abs(h * 6.0 - 3.0) - 1.0;
    let g = 2.0 - abs(h * 6.0 - 2.0);
    let b = 2.0 - abs(h * 6.0 - 4.0);
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Blackbody-locus colour for a temperature in Kelvin (Tanner Helland approximation,
// ~1000–40000K). Normalized-ish RGB; the caller scales by the incandescence amount.
fn blackbody(kelvin: f32) -> vec3<f32> {
    let t = clamp(kelvin, 1000.0, 40000.0) / 100.0;
    var r: f32;
    var g: f32;
    var b: f32;
    if (t <= 66.0) {
        r = 1.0;
        g = clamp(0.3900816 * log(max(t, 1.0)) - 0.6318414, 0.0, 1.0);
    } else {
        r = clamp(1.2929362 * pow(t - 60.0, -0.1332047), 0.0, 1.0);
        g = clamp(1.1298909 * pow(t - 60.0, -0.0755148), 0.0, 1.0);
    }
    if (t >= 66.0) {
        b = 1.0;
    } else if (t <= 19.0) {
        b = 0.0;
    } else {
        b = clamp(0.5432068 * log(t - 10.0) - 1.1962541, 0.0, 1.0);
    }
    return vec3<f32>(r, g, b);
}

// ===================== materials (#472 Tier 1) =====================
// A single planar UV for the material maps, chosen by projection mode. Most
// Organon geometry has no UVs, so Triplanar picks the DOMINANT axis plane (the one
// the surface most faces) — a coherent single UV cheap enough for the cotangent
// normal frame, without a full 3-plane blend (that is a later polish). `scale` is
// world units → tiles.
fn mat_uv(world_pos: vec3<f32>, obj_pos: vec3<f32>, nrm: vec3<f32>, projection: f32, scale: f32) -> vec2<f32> {
    let s = max(scale, 1e-4);
    if (projection >= 1.5) {
        // Object-planar XY (rides the geometry).
        return obj_pos.xy * s;
    } else if (projection >= 0.5) {
        // World-planar XZ (a flat ground-style projection).
        return world_pos.xz * s;
    }
    // Triplanar → dominant axis.
    let an = abs(nrm);
    if (an.x >= an.y && an.x >= an.z) {
        return world_pos.zy * s;
    } else if (an.y >= an.z) {
        return world_pos.xz * s;
    }
    return world_pos.xy * s;
}

// Perturb a geometric normal by a tangent-space normal map with NO mesh UVs, via
// the derivative cotangent frame (Mikkelsen). `nmap` is the raw [0,1] texel; the
// caller gates this on uniform control flow (u.mtl.x), so the derivatives are legal.
fn mat_perturb_normal(n: vec3<f32>, world_pos: vec3<f32>, uv: vec2<f32>, nmap: vec3<f32>, strength: f32) -> vec3<f32> {
    let ts = vec3<f32>((nmap.xy * 2.0 - 1.0) * strength, max(nmap.z * 2.0 - 1.0, 1e-3));
    let dp1 = dpdx(world_pos);
    let dp2 = dpdy(world_pos);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let denom = max(dot(t, t), dot(b, b));
    if (denom < 1e-12) { return n; }
    let invmax = inverseSqrt(denom);
    return normalize(t * (invmax * ts.x) + b * (invmax * ts.y) + n * ts.z);
}

// ===================== fragment =====================
// Outputs LINEAR HDR radiance into the scene's Rgba16Float buffer; exposure,
// bloom, and ACES tonemapping all happen later in the composite pass.
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var metallic      = clamp(u.mat.x, 0.0, 1.0);
    var roughness     = clamp(u.mat.y, 0.0, 1.0);
    let glow          = u.mat.z;
    let env_intensity = u.env.y;
    let env_rot       = u.env.z;
    let opacity       = u.env.w;
    var ambient_mul   = u.amb.x;
    let mat_type      = u.amb.y; // 0=Standard, 1=Chrome, 2=Glass, 3=Refractive
    let ior           = max(u.amb.z, 1.0);
    // Environment tint: colours the light the env contributes (white = none).
    let etint         = u.env_tint.rgb;
    // Reflection controls (#163 Tier 1). All 0 → today's look byte-identical.
    let reflect_tint  = u.reflect_ctl.x;                 // palette influence on reflection
    let chrome_purity = clamp(u.reflect_ctl.y, 0.0, 1.0); // Chrome → pure neutral mirror
    let glass_clarity = clamp(u.reflect_ctl.z, 0.0, 1.0); // Glass → colourless clear glass
    let f0_override   = clamp(u.reflect_ctl.w, 0.0, 1.0); // Standard reflectance lift

    var n = normalize(in.world_normal);
    let v = normalize(u.camera_pos.xyz - in.world_pos);
    // Double-sided lighting: flip the normal toward the viewer. A no-op on the
    // closed cubes/tubes (visible faces already face the camera), but it lets the
    // thin Membrane sheets light correctly from both sides.
    if (dot(n, v) < 0.0) { n = -n; }

    // ---- Material texture set (#472 Tier 1) --------------------------------
    // Sampled into a coherent UV (triplanar dominant-axis or planar). When
    // materials are off (u.mtl.x = 0) or a channel map is absent (present_mask),
    // every override collapses to the scalar-uniform value → byte-identical.
    // `m_albedo`/`has_alb` outlive the branch (the base-colour resolve below reads
    // them). Their defaults are the identity for that resolve: has_alb = 0 makes
    // `mix(in.color, m_albedo.rgb, has_alb)` return `in.color` exactly, so the
    // specialised-off variant is bit-identical rather than merely close.
    var m_albedo = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    var has_alb  = 0.0;
    if (material_maps) {
    let mat_on   = select(0.0, 1.0, u.mtl.x > 0.5);
    let mat_mask = u32(round(u.mtl2.w));
    let m_uv     = mat_uv(in.world_pos, in.world_pos, n, u.mtl.y, u.mtl.z);
    // Sample every channel unconditionally (textures are always bound — neutral
    // when nothing is loaded), then blend by (mat_on × present-bit) so an off frame
    // multiplies through zero and stays bit-identical.
    m_albedo     = textureSample(mat_albedo_tex, mat_samp, m_uv);
    let m_normal = textureSample(mat_normal_tex, mat_samp, m_uv).xyz;
    let m_rough  = textureSample(mat_rough_tex,  mat_samp, m_uv).r;
    let m_metal  = textureSample(mat_metal_tex,  mat_samp, m_uv).r;
    let m_ao     = textureSample(mat_ao_tex,     mat_samp, m_uv).r;
    has_alb      = f32((mat_mask & 1u) != 0u) * mat_on;
    let has_nrm  = f32((mat_mask & 2u) != 0u) * mat_on;
    let has_rgh  = f32((mat_mask & 4u) != 0u) * mat_on;
    let has_met  = f32((mat_mask & 8u) != 0u) * mat_on;
    let has_ao   = f32((mat_mask & 16u) != 0u) * mat_on;
    // The maps feed the ONE material pipeline directly — no separate quality knobs.
    // A present roughness/metallic channel replaces the scalar value the main Material
    // card drives; where a channel is absent, that scalar stands. Every surface type
    // (Standard / Chrome / Glass / Subsurface / …), the GI bounce, and SSR then consume
    // these shared values exactly as they do for a scalar material.
    roughness = clamp(mix(roughness, m_rough, has_rgh), 0.0, 1.0);
    metallic  = clamp(mix(metallic,  m_metal, has_met), 0.0, 1.0);
    // AO map darkens the ambient/IBL term at full strength (mix→1 when off/absent).
    ambient_mul = ambient_mul * mix(1.0, m_ao, has_ao);
    // Perturb the normal via the derivative cotangent frame at full strength; select
    // keeps `n` bit-identical when the normal map is off/absent.
    let n_bumped = mat_perturb_normal(n, in.world_pos, m_uv, m_normal, 1.0);
    n = select(n, n_bumped, has_nrm > 0.5);
    }
    // `m_albedo`/`has_alb` feed the base-colour resolve below.
    // ------------------------------------------------------------------------

    let n_dot_v = max(dot(n, v), 1e-4);

    // Specular occlusion (#174 T3, Lagarde): derive an occlusion factor for the
    // env-specular/mirror lobes from the blurred SSAO — sharper than diffuse AO at
    // low roughness (a mirror is only occluded by what blocks its one reflected
    // ray), converging to plain AO as roughness rises. 1 (no-op) when SSAO is off
    // or the bound AO is the white dummy.
    var spec_occ = 1.0;
    if (u.glassx.w > 0.5) {
        let ao_dims = vec2<f32>(textureDimensions(ao_tex));
        let ao = textureSampleLevel(ao_tex, ao_samp, in.clip.xy / max(ao_dims, vec2<f32>(1.0)), 0.0).r;
        spec_occ = clamp(pow(n_dot_v + ao, exp2(-16.0 * roughness - 1.0)) - 1.0 + ao, 0.0, 1.0);
    }

    let l_key  = normalize(u.key_light.xyz);
    let l_fill = normalize(u.fill_light.xyz);

    // Cast shadows (#152 Tier 3): darken the KEY light's direct term where the
    // fragment is occluded from the light. The key radiance below is scaled by this.
    // N·L feeds the slope-scaled bias.
    var shadow = shadow_factor(in.world_pos, max(dot(n, l_key), 0.0));
    // #195 Tier 1: hardware-RT shadows supersede the PCF map when on — ground-
    // truth occlusion from the screen-space ray mask, no bias/frustum tuning.
    // The fill light gets its own visibility channel (strength in params2.w).
    let rt_vis = rt_shadow_vis(in.world_pos);
    if (shadow_u.params2.z > 0.0) {
        shadow = mix(1.0, rt_vis.x, shadow_u.params2.z);
    }
    let fill_w = u.fill_light.w * mix(1.0, rt_vis.y, shadow_u.params2.w);
    let key_rad = vec3<f32>(u.key_light.w) * shadow * fluid_light(in.world_pos);

    // Reaction–diffusion dapple (0..1, triplanar world-space). `albedo_mix` carves
    // pigment into the albedo (spots/stripes); `intensity` adds HDR emissive glow
    // (woven into `emissive` below). The base colour is kept for the glow tint.
    let rd_d = rd_dapple(in.world_pos, n);
    // #305 Tier 1: recolour the resolved albedo by the per-material Hue/Saturation/
    // Value (identity [0,1,1] → byte-identical). Applied here so every material branch
    // + the emissive glow + the GI bounce all track the same recoloured base.
    // #349 T1: the SINGLE application point for the calibrated colour law. The
    // aesthetic albedo (RGB-cube × per-instance tint, recoloured by HSV) is resolved
    // first, then `apply_calibrated` mixes in the measured-level LUT colour by
    // `u.cal.z` (a no-op → byte-identical when mode = Aesthetic or amount = 0). Doing
    // it here means every material branch + the emissive glow + the GI bounce all
    // track the same base, exactly once.
    // #472 Tier 1: the albedo map replaces the RGB-cube base colour where present
    // (has_alb = 0 → in.color, byte-identical), then the RD dapple + HSV +
    // calibrated law all track that base exactly as before.
    let base_col = mix(in.color, m_albedo.rgb, has_alb);
    let albedo = apply_calibrated(apply_hsv(mix(base_col, base_col * rd_d, clamp(rd.params.z, 0.0, 1.0)), u.matcol));
    // #163 T1: palette influence on the reflection. 1 = neutral (no palette colour);
    // → albedo as `reflect_tint` rises; >1 pushes past (clamped ≥ 0 so it can't go
    // negative). At reflect_tint = 0 this is exactly vec3(1) → reflections unchanged.
    let refl_palette = max(vec3<f32>(0.0), mix(vec3<f32>(1.0), albedo, reflect_tint));
    var r = reflect(-v, n);

    // ----- Anisotropy (#214 Tier 1) -----
    // Active on the pure Anisotropic material (type 4) OR the overlay on
    // Standard/Chrome. Never on Glass/Refractive (2/3 — their transmission owns
    // the look). `aniso_amt` folds in the overlay blend; 0 → isotropic, so the
    // frame/αt/αb below stay at their neutral defaults and nothing changes.
    let aniso_is_type = mat_type > 3.5 && mat_type < 4.5; // exactly Anisotropic (4)
    let is_glassy = mat_type >= 1.5 && mat_type < 3.5;
    let aniso_amt = select(
        select(0.0, clamp(u.aniso.x, -1.0, 1.0) * clamp(u.aniso.w, 0.0, 1.0), u.aniso.z > 0.5),
        clamp(u.aniso.x, -1.0, 1.0),
        aniso_is_type,
    );
    let aniso_on = abs(aniso_amt) > 1e-4 && !is_glassy;
    var af_t = vec3<f32>(1.0, 0.0, 0.0);
    var af_b = vec3<f32>(0.0, 1.0, 0.0);
    var a_t = max(roughness * roughness, 4e-4);
    var a_b = a_t;
    if (aniso_on) {
        let af = aniso_frame(n, normalize(in.brush), u.aniso.y);
        af_t = af.t;
        af_b = af.b;
        // Split roughness into along/across the streak (Standard uses `roughness`;
        // Chrome recomputes from its own polish below).
        let ab = aniso_alpha(roughness, aniso_amt);
        a_t = ab.x;
        a_b = ab.y;
        // Bend the ENV reflection into the streak (the prefiltered mips smear it);
        // the parallax block below then re-aims this bent ray exactly like the
        // isotropic one, so anisotropy + parallax compose. Size the bend by the
        // roughness the sampling branch will actually use: Chrome samples its env
        // at `roughness * (1 - chrome_purity)`, so a high-purity mirror gets ~0
        // bend (no smear on a sharp reflection) instead of a bend sized for the
        // base roughness but sampled sharp (#220 review).
        let is_chrome = mat_type > 0.5 && mat_type < 1.5;
        let bend_rough = select(roughness, roughness * (1.0 - chrome_purity), is_chrome);
        r = aniso_reflect(n, v, af_t, af_b, aniso_amt, bend_rough);
    }

    // #163 Tier 2: parallax box-projection. When the reflection source is Parallax
    // (refl_box_min.w > 0), re-aim the reflection ray through the field's AABB so it
    // depends on position, then blend with the infinite direction. All material
    // branches sample via `r_env`, so this one correction covers Standard/Chrome/Glass.
    // source 0 → untouched (today's look). Diffuse irradiance (`n_env`) is left as-is.
    if (u.refl_box_min.w > 0.5) {
        let corrected = normalize(parallax_correct(r, in.world_pos,
                                                   u.refl_box_min.xyz, u.refl_box_max.xyz));
        r = normalize(mix(r, corrected, clamp(u.refl_box_max.w, 0.0, 1.0)));
    }

    // Rotate sampling directions into env space (Y-axis env_rotation).
    let n_env = rotate_y(n, env_rot);
    let r_env = rotate_y(r, env_rot);

    // Constant glow + the travelling bioluminescent ripple (inert at intensity 0),
    // added into every material branch via `emissive`. `env_tint.w` carries the
    // Material **Emissive** (HDR): the resolved albedo emitted in its OWN colour,
    // pushed past 1 so the surface blooms in its hue instead of washing to white.
    // 0 → glow-only, byte-identical.
    var emissive = albedo * (glow + u.env_tint.w) + ripple_emission(in.world_pos, albedo)
        + base_col * (rd_d * rd.params.x);
    // Spectral emission (#214 T5 pt 1), woven in here so it applies on every material.
    // Fluorescence: the surface absorbs the environment's short-wavelength (blue)
    // light and re-emits it at a chosen hue — bright under a blue/UV-ish env
    // (blacklight-poster glow). Incandescence: a blackbody glow by temperature
    // (embers → white-hot) that replaces the flat albedo glow with a physical locus.
    // Both 0 → no change.
    if (u.emit.x > 0.0) {
        let excite = sample_irradiance(n_env).b * env_intensity * ambient_mul;
        emissive += hue2rgb(u.emit.y) * (excite * u.emit.x);
    }
    if (u.emit.z > 0.0) {
        emissive += blackbody(u.emit.w) * u.emit.z;
    }

    // Bounced GI (#80 Part B): coloured diffuse light bleeding from neighbouring
    // cubes, sampled from the probe volume and tinted by this surface's albedo.
    // Zero when GI is off, so it's added freely into every material branch below.
    let gi_term = gi_irradiance(in.world_pos, n) * albedo;

    // Emissive cubes as real lights (#167 Tier 3): direct illumination from the brightest
    // neighbouring cubes. Uses the Standard dielectric/metal F0 (one term shared across
    // all material branches, like gi_term). Zero when the light count is 0 (today's look).
    let ml_f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let ml_term = many_lights(in.world_pos, n, v, albedo, metallic, roughness, ml_f0);

    // ===== Water (#206): a dedicated, physically-real water surface =====
    // The channel water is drawn with its OWN uniform; render.rs flags it with
    // `sss.w = 3` and packs [absorption, glitter, reflectivity] into sss.xyz.
    // Returns early — bypassing the generic material/lobe path — so the water
    // reads as WATER regardless of the tunnel's material: Fresnel-weighted sky
    // reflection (the walls arrive via SSR in the composite), Beer–Lambert depth
    // absorption toward a deep colour (clear at grazing shallows → deep in the
    // channel body), and a sharp sun-glitter streak riding the ripples.
    if (u.sss.w > 2.5) {
        let w_absorb  = u.sss.x;
        let w_glitter = u.sss.y;
        let w_reflect = u.sss.z;
        let f0s = (ior - 1.0) / (ior + 1.0);
        let f0 = f0s * f0s;                                   // reflectance from IOR
        var fr = f0 + (1.0 - f0) * pow(1.0 - n_dot_v, 5.0);   // Schlick Fresnel
        fr = clamp(fr + w_reflect * pow(1.0 - n_dot_v, 2.0), 0.0, 1.0);
        let refl = sample_prefiltered(r_env, roughness) * env_intensity * ambient_mul * etint;
        // Beer–Lambert body: the water colour absorbs toward a deep colour with the
        // optical path length (longer at grazing → the far channel reads deep).
        let deep = albedo * albedo * 0.6;
        let path = clamp(w_absorb * (0.3 + 0.9 * (1.0 - n_dot_v)), 0.0, 1.0);
        let body = mix(albedo, deep, path) * env_intensity * ambient_mul * etint;
        // Sharp sun glitter from the key light on the rippled normal.
        let h_spec = normalize(l_key + v);
        let glint = pow(max(dot(n, h_spec), 0.0), 380.0) * w_glitter * u.key_light.w * shadow;
        let w_color = mix(body, refl, fr) + vec3<f32>(glint) + emissive + gi_term + ml_term;
        // Premultiplied output (pipeline_skin blends One / OneMinusSrcAlpha); grazing
        // rims read more solid/mirror via the Fresnel-lifted coverage.
        let w_alpha = clamp(opacity + (1.0 - opacity) * fr, 0.0, 1.0);
        return vec4<f32>(w_color * w_alpha, w_alpha);
    }

    // ----- additive surface modifiers (computed once, woven into each branch) -----
    // Body optics (#214 T3): the pure Subsurface material forces the translucency
    // lobe on with the thickness model, so picking it gives honest wax/jade without
    // also dialing the Surface-FX translucency. Standard/others keep their explicit
    // `subsurface` amount; a 0 amount stays inert. (Enum id 7 after the main re-merge —
    // the Tier-2 Clearcoat/Velvet types took 5/6.)
    let is_subsurface_type = mat_type > 6.5 && mat_type < 7.5;
    let sss_amount     = select(u.sss.x, select(1.0, u.sss.x, u.sss.x > 0.0), is_subsurface_type);
    let sss_distortion = u.sss.y;
    let sss_power      = max(u.sss.z, 1.0);
    // SSS thickness drive (0 = today's distortion-only translucency, byte-identical).
    // Subsurface forces the thickness model on the same way `sss_amount` does — a set
    // dial wins, else default to full (u.body.x is CPU-clamped to 0..1, so a plain
    // `max(.,1)` would peg it at 1 and kill the slider).
    let sss_thickness  = select(u.body.x, select(1.0, u.body.x, u.body.x > 0.0), is_subsurface_type);
    let irid_amount    = u.irid.x;

    // Iridescence: a spectral tint that multiplies the reflection (white when
    // amount = 0 → no change) plus an additive oil-slick rim, strongest grazing.
    let irid_raw   = iridescent_tint(n_dot_v, u.irid.y, u.irid.z);
    let irid_tint  = mix(vec3<f32>(1.0), irid_raw, irid_amount);
    let irid_sheen = irid_raw * (irid_amount * pow(1.0 - n_dot_v, 3.0));

    // Translucency: back-glow from both analytic lights, tinted by albedo.
    // #214 T3 real-thickness SSS: the old lobe faked depth with `sss_distortion`;
    // now the back-glow is attenuated per channel by Beer–Lambert over the MEASURED
    // chord through the instance body (`instance_thickness` along the view-through
    // direction −v — how much material sits behind the visible point). σ = (1 −
    // albedo)/radius, so the node's own colour survives: thin silhouette edges glow,
    // thick centres go deep/absorbed — honest wax/jade/marble. `sss_thickness`
    // blends from the flat lobe (0 = byte-identical) to fully thickness-driven.
    var sss = vec3<f32>(0.0);
    if (sss_amount > 0.0) {
        // Back-scatter lobe (Barré-Brisebois): the through-glow, only bright BACKLIT.
        let back = translucency_lobe(v, l_key, n, sss_distortion, sss_power) * u.key_light.w
                 + translucency_lobe(v, l_fill, n, sss_distortion, sss_power) * fill_w;
        var trans = vec3<f32>(1.0);
        var front = 0.0;
        if (sss_thickness > 0.0) {
            let inv_nm = mat3x3<f32>(in.inv0, in.inv1, in.inv2);
            let thick = instance_thickness(in.local_pos, inv_nm, -v);
            let sigma = (vec3<f32>(1.0) - clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0)))
                / max(u.body.y, 0.05);
            trans = mix(vec3<f32>(1.0), exp(-sigma * thick), clamp(sss_thickness, 0.0, 1.0));
            // Front-lit wrap: the back lobe alone reads opaque when the key is in
            // FRONT, so add a soft wrapped-diffuse bleed (light that enters and
            // scatters back out the same side). `sss_distortion` widens the wrap;
            // scaled by the thickness drive and multiplied by `trans` below, so the
            // Thickness/Radius sliders sculpt the FRONT-lit look, not only the backlit
            // rim. Gated on sss_thickness>0, so back-only translucency (thickness 0)
            // stays byte-identical. `0.6` is a look constant (tune on-Mac).
            let wrap = clamp(sss_distortion, 0.0, 1.0);
            let fk = max((dot(n, l_key) + wrap) / (1.0 + wrap), 0.0) * u.key_light.w;
            let ff = max((dot(n, l_fill) + wrap) / (1.0 + wrap), 0.0) * fill_w;
            front = (fk + ff) * clamp(sss_thickness, 0.0, 1.0) * 0.6;
        }
        sss = albedo * (back + front) * sss_amount * trans;
    }

    // ----- Surface lobes (#214 Tier 2): clearcoat + sheen -----
    // Woven into the opaque branches (Standard + Chrome). The pure Clearcoat (5) /
    // Velvet (6) materials force their lobe on and land in the Standard branch (they
    // are > 3.5 and not glassy, so they skip Chrome/Glass); Standard/Chrome also get
    // them as opt-in overlays. Glass/Refractive are excluded (their transmission owns
    // the look — a coated-glass overlay is a follow-up). `coat_spec`/`sheen_add` are
    // additive and `base_scale` is 1 when both are off → byte-identical.
    let is_clearcoat_type = mat_type > 4.5 && mat_type < 5.5;
    let is_velvet_type    = mat_type > 5.5 && mat_type < 6.5;
    let coat_amt  = select(select(0.0, u.coat.x, u.coat.z > 0.5), u.coat.x, is_clearcoat_type);
    let sheen_amt = select(select(0.0, u.sheen.x, u.coat.w > 0.5), u.sheen.x, is_velvet_type);
    let coat_on  = coat_amt > 1e-4 && !is_glassy;
    let sheen_on = sheen_amt > 1e-4 && !is_glassy;
    var coat_spec = vec3<f32>(0.0);
    var sheen_add = vec3<f32>(0.0);
    var base_scale = 1.0; // energy the lobes steal from the base beneath them
    if (coat_on) {
        let cc_rough = clamp(u.coat.y, 0.02, 1.0);
        // Clearcoat Fresnel (dielectric F0 0.04 ≙ IOR 1.5) × strength — the fraction
        // reflected by the coat before light reaches the base.
        let fc = (0.04 + 0.96 * pow(1.0 - n_dot_v, 5.0)) * coat_amt;
        // Env at the coat's smoothness, along the ISOTROPIC reflection (the coat is
        // smooth even over a brushed/parallax base — its own straight mirror).
        let coat_r = rotate_y(reflect(-v, n), env_rot);
        let coat_env = sample_prefiltered(coat_r, cc_rough) * fc
            * env_intensity * ambient_mul * etint * spec_occ;
        // Direct: a sharp isotropic glint (albedo 0, metallic 0 → specular only).
        let coat_dir = (direct_light(n, v, l_key, vec3<f32>(0.0), 0.0, cc_rough, vec3<f32>(0.04), key_rad)
                      + direct_light(n, v, l_fill, vec3<f32>(0.0), 0.0, cc_rough, vec3<f32>(0.04), vec3<f32>(fill_w))) * coat_amt;
        coat_spec = coat_env + coat_dir;
        base_scale = base_scale * (1.0 - fc);
    }
    if (sheen_on) {
        let sh_rough = clamp(u.sheen.y, 0.02, 1.0);
        // Sheen colour: white → the node's own albedo as the tint dial rises (the
        // fuzz retroreflects the body colour). Scaled by amount.
        let sheen_col = mix(vec3<f32>(1.0), albedo, clamp(u.sheen.z, 0.0, 1.0)) * sheen_amt;
        let sh = sheen_lobe(n, v, l_key, sh_rough) * key_rad
               + sheen_lobe(n, v, l_fill, sh_rough) * vec3<f32>(fill_w);
        // A soft grazing env rim so the fuzz reads under IBL too, not only direct.
        let rim = pow(1.0 - n_dot_v, 2.0) * env_intensity * ambient_mul;
        sheen_add = sheen_col * (sh + rim * 0.5);
        // Energy compensation: the fuzz scatters light that would otherwise reach the
        // base — darken it gently by the sheen albedo.
        base_scale = base_scale * (1.0 - 0.25 * sheen_amt
            * max(max(sheen_col.r, sheen_col.g), sheen_col.b));
    }

    // ----- Microstructure (#214 Tier 4): glitter + retro (additive) + diffraction
    // (a spectral tint on the env reflection). Computed once, woven into the Standard
    // + Chrome returns. All amounts 0 → micro_add 0 + diffr_tint white (identical).
    let glitter_amt     = u.micro.x;
    let diffraction_amt = u.micro.w;
    let retro_amt       = u.micro2.y;
    var micro_add  = vec3<f32>(0.0);
    var diffr_tint = vec3<f32>(1.0);
    if (glitter_amt > 0.0) {
        let gseed = u.irid.w; // per-frame counter (0..63) — TAA/blue-noise resolve
        micro_add += (glitter_glint(in.world_pos, n, v, l_key, key_rad, u.micro.y, u.micro.z, gseed)
                   +  glitter_glint(in.world_pos, n, v, l_fill, vec3<f32>(fill_w), u.micro.y, u.micro.z, gseed + 13.0))
                   * glitter_amt;
    }
    if (retro_amt > 0.0) {
        micro_add += retro_glow(n, v, l_key, albedo, key_rad, retro_amt)
                   + retro_glow(n, v, l_fill, albedo, vec3<f32>(fill_w), retro_amt);
    }
    if (diffraction_amt > 0.0) {
        diffr_tint = mix(vec3<f32>(1.0), diffraction_tint(n_dot_v, max(u.micro2.x, 1.0)),
                         clamp(diffraction_amt, 0.0, 1.0));
    }

    // ===== Chrome: a polished mirror reflecting the environment =====
    if (mat_type > 0.5 && mat_type < 1.5) {
        // roughness acts as "polish" (0 = sharp mirror); reflectivity is high.
        // #163 `chrome_purity`: lerp F0 toward a neutral white mirror and sharpen the
        // reflection (roughness → 0) so a full-purity chrome is a pure, untinted mirror
        // regardless of palette/metallic. At purity 0 this is exactly today's chrome.
        let f0c_base = mix(vec3<f32>(0.85), albedo, metallic);
        let f0c = mix(f0c_base, vec3<f32>(1.0), chrome_purity);
        let refl_rough = roughness * (1.0 - chrome_purity);
        // #174 T3: shade through the split-sum BRDF LUT like the Standard branch
        // (was a bare Fresnel — skipping the Smith geometry integral made rough
        // chrome too bright at grazing angles). At roughness ≈ 0 the LUT gives
        // A ≈ 1, B ≈ 0 → the mirror is unchanged.
        let brdf_c = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                        vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0),
                                                  max(refl_rough, 1e-3)), 0.0).rg;
        let refl = sample_prefiltered(r_env, refl_rough);
        // Env tint × iridescence × palette-influence tint the mirror reflection;
        // specular occlusion darkens it where SSAO says the sky is blocked.
        // #214 T4: `diffr_tint` splits the mirror into diffraction orders (holo-foil
        // over Chrome); white when off.
        var mirror = refl * (f0c * brdf_c.x + vec3<f32>(brdf_c.y))
            * env_intensity * ambient_mul * etint * irid_tint * refl_palette * spec_occ * diffr_tint;
        // Refraction overlay on Chrome: the mirror yields to a refracted core by
        // the IOR Fresnel — grazing stays pure mirror, face-on opens into the
        // absorbing glassy body. blend 0 / overlay off → today's chrome.
        var chrome_core = vec3<f32>(0.0);
        if (u.refr.y > 0.5) {
            let inv_nm = mat3x3<f32>(in.inv0, in.inv1, in.inv2);
            let f0i = (ior - 1.0) / (ior + 1.0);
            let fri = fresnel_schlick_roughness(n_dot_v, vec3<f32>(f0i * f0i), refl_rough).x;
            let open = clamp(u.refr.z, 0.0, 1.0) * (1.0 - fri);
            chrome_core = refr_overlay_trans(in.local_pos, inv_nm, v, n, ior,
                                             refl_rough, env_rot, albedo)
                * env_intensity * ambient_mul * etint * open;
            mirror = mirror * (1.0 - open);
        }
        let rough_g = max(refl_rough, 0.02);
        // Anisotropy overlay on Chrome: the mirror highlight elongates along the
        // brush (env already bent via `r`). Off → today's isotropic chrome.
        var direct: vec3<f32>;
        if (aniso_on) {
            let cab = aniso_alpha(rough_g, aniso_amt);
            direct = direct_light_aniso(n, v, l_key, af_t, af_b, albedo, 1.0, cab.x, cab.y, f0c, key_rad)
                   + direct_light_aniso(n, v, l_fill, af_t, af_b, albedo, 1.0, cab.x, cab.y, f0c, vec3<f32>(fill_w));
        } else {
            direct = direct_light(n, v, l_key, albedo, 1.0, rough_g, f0c, key_rad)
                   + direct_light(n, v, l_fill, albedo, 1.0, rough_g, f0c, vec3<f32>(fill_w));
        }
        // Clearcoat/sheen overlays (#214 T2): attenuate the mirror by the coat
        // Fresnel and add the coat glint + sheen fuzz on top. Off → identical.
        // #214 T4: glitter + retro (`micro_add`) sparkle on top.
        // #288 T3: the Demo point light (inert off the Demo scene).
        let chrome_demo_pt = demo_point_light(in.world_pos, n, v, albedo, 1.0, rough_g, f0c);
        let chrome_col = (mirror + chrome_core + direct + emissive + irid_sheen + gi_term + ml_term + chrome_demo_pt)
            * base_scale + coat_spec + sheen_add + micro_add;
        return vec4<f32>(chrome_col, 1.0);
    }

    // ===== Glass / Refractive: Fresnel-blended reflection + refraction =====
    // Refractive (mat_type 3) is the Glass path plus Beer–Lambert absorption over
    // the measured chord through the instance body (the `absorb` block below), so
    // it inherits every glass dial (clarity, dispersion, thin-film, stochastic OIT).
    // Bounded < 3.5 so the Anisotropic material (type 4) falls through to the
    // Standard branch, where it shades as PBR + an anisotropic specular lobe.
    if (mat_type >= 1.5 && mat_type < 3.5) {
        // Spectral glass (#80 Part C): dispersion splits the refraction into
        // wavelength-offset IORs (rainbow fringing) and caustic boosts focused
        // bright spots; both 0 → identical to the single-IOR refraction below.
        let dispersion = u.glassx.x;
        let caustic    = u.glassx.y;
        let thin_film  = u.glassx.z;
        // #163 `glass_clarity`: sharpen the refraction/reflection (roughness → 0) so
        // clear glass reads crisp. At clarity 0 this is exactly today's roughness.
        let g_rough = roughness * (1.0 - glass_clarity);
        var thru: vec3<f32>;
        if (dispersion > 0.0 || caustic > 0.0) {
            thru = glass_dispersion(v, n, ior, g_rough, env_rot, dispersion, caustic);
        } else {
            let refr_dir = refract(-v, n, 1.0 / ior);
            if (dot(refr_dir, refr_dir) < 1e-6) {
                thru = sample_prefiltered(r_env, g_rough); // total internal reflection
            } else {
                thru = sample_prefiltered(rotate_y(refr_dir, env_rot), g_rough);
            }
        }
        let reflected = sample_prefiltered(r_env, g_rough);
        // Scalar Fresnel for the reflect/refract mix, with F0 derived from the live
        // IOR — ((ior−1)/(ior+1))² — instead of a hard-coded 0.04: refraction and
        // dispersion already track the IOR slider, so a fixed F0 gave high-IOR glass
        // diamond-like bending with water-like reflectance. (0.04 ≙ ior 1.5; the
        // default 1.45 gives 0.0337 — a hair less face-on reflection.)
        let f0s = (ior - 1.0) / (ior + 1.0);
        let fr = fresnel_schlick_roughness(n_dot_v, vec3<f32>(f0s * f0s), g_rough).x;
        // ===== Refractive (mat_type 3): Glass + Beer–Lambert absorption =====
        // The liquid's see-through-water optics on the instanced generators: the
        // transmission is attenuated over the measured chord through this node's
        // body along the refracted ray. σ per channel = (1 − albedo) × strength,
        // so the node's own colour is what SURVIVES (thin edges stay clear, thick
        // bodies go murky in the node's colour — the `liq_absorb` convention).
        // `absorb` stays 1 on Glass (mat_type 2) or at strength 0 → byte-identical.
        // The refraction OVERLAY (u.refr.y) opens the same murk on plain Glass,
        // scaled by the blend dial (Refractive keeps full strength).
        var absorb = vec3<f32>(1.0);
        if ((mat_type >= 2.5 || u.refr.y > 0.5) && u.refr.x > 0.0) {
            var tdir = refract(-v, n, 1.0 / ior);
            if (dot(tdir, tdir) < 1e-6) {
                tdir = reflect(-v, n); // TIR: the bounced ray still crosses the body
            }
            let inv_nm = mat3x3<f32>(in.inv0, in.inv1, in.inv2);
            absorb = refr_overlay_absorb(in.local_pos, inv_nm, normalize(tdir), albedo);
            if (mat_type < 2.5) {
                absorb = mix(vec3<f32>(1.0), absorb, clamp(u.refr.z, 0.0, 1.0));
            }
        }
        // Interior in-scatter (#214 T3, the liquid-murk term on the instances):
        // where absorption removed the transmission, the murky body glows instead of
        // going dark — a single scatter of the scene's ambient + bounced GI at the
        // body midpoint, tinted by the node's own colour. Gated by (1 − absorb), so
        // crystal-clear glass (absorb → 1) stays GI-free exactly as optics wants, and
        // by the `interior_scatter` dial (0 → byte-identical). Opal / nebula-in-a-cube.
        var interior = vec3<f32>(0.0);
        if (u.body.z > 0.0) {
            var idir = refract(-v, n, 1.0 / ior);
            if (dot(idir, idir) < 1e-6) {
                idir = reflect(-v, n);
            }
            idir = normalize(idir);
            let inv_nm = mat3x3<f32>(in.inv0, in.inv1, in.inv2);
            let thick = instance_thickness(in.local_pos, inv_nm, idir);
            let midp = in.world_pos + idir * (thick * 0.5);
            // Ambient environment scattered inside + the probe-GI bounce, both tinted
            // by the node colour (the single-scatter albedo).
            let ambient_in = sample_irradiance(rotate_y(-idir, env_rot)) * env_intensity * ambient_mul;
            let scatter = (ambient_in + gi_irradiance(midp, -idir)) * albedo;
            interior = scatter * u.body.z * (vec3<f32>(1.0) - absorb);
        }
        // Body tint: today's 50% albedo tint, lerped toward colourless white as
        // `glass_clarity` rises (1 = clear glass). At clarity 0 → mix(1, albedo, 0.5).
        let tint = mix(mix(vec3<f32>(1.0), albedo, 0.5), vec3<f32>(1.0), glass_clarity);
        // Iridescence + thin-film + palette-influence tint the reflected component
        // (films on the surface); env tint multiplies the whole environment contribution.
        // Split into the TRANSMITTED body (attenuated by coverage below) and the
        // SURFACE reflection (composited at full strength) — mix(a,b,fr) = the sum
        // of these two, so the split itself changes nothing.
        // Thin-film sheen tinting the reflected component. `u.thinfilm.x` (base
        // thickness in nm) > 0 switches on the PHYSICAL wavelength-resolved Airy
        // model (soap film / bubble, thickness-driven with gravity drainage +
        // marbling); its spectral reflectance is normalized to a mean-1 colour
        // ratio so it tints the reflection like the legacy multiplier. Thickness 0
        // → the existing cosine-hack `thin_film_tint` path (byte-identical default).
        var film = thin_film_tint(n_dot_v, thin_film);
        if (u.thinfilm.x > 0.0) {
            let refl = thin_film_physical(n_dot_v, in.world_pos);
            let lum = max((refl.r + refl.g + refl.b) * 0.3333333, 1e-3);
            film = refl / lum;
        }
        let env_mul = tint * env_intensity * ambient_mul * etint;
        // `absorb` (Refractive only) darkens the refracted env per channel; 1 on Glass.
        // `interior` re-adds glow where absorption removed it (0 → unchanged).
        let glass_body = thru * (1.0 - fr) * env_mul * absorb + interior;
        let glass_surf = reflected * irid_tint * film * refl_palette * fr * env_mul;
        // Sharpen the analytic key/fill highlights with the same `g_rough` the env
        // refraction/reflection uses (floored like Chrome), so clear glass isn't crisp
        // in the IBL yet soft in the direct specular. At clarity 0, g_rough == roughness.
        let dg_rough = max(g_rough, 0.02);
        // The analytic highlights share the IOR-derived F0 with the env Fresnel mix.
        let glass_f0 = vec3<f32>(f0s * f0s);
        let direct = direct_light(n, v, l_key, albedo, metallic, dg_rough, glass_f0, key_rad)
                   + direct_light(n, v, l_fill, albedo, metallic, dg_rough, glass_f0, vec3<f32>(fill_w));
        // Edge-on faces (low n·v) look more opaque (more reflective); face-on is
        // clearer. Alpha = opacity, lifted by Fresnel so rims read as glass.
        var alpha = clamp(opacity + (1.0 - opacity) * fr, 0.0, 1.0);
        // Refractive murk occludes: where absorption removed the transmission,
        // the scene BEHIND is blocked too — lift coverage by the mean absorbed
        // fraction (scalar; the per-channel colour rides the refracted env in
        // `glass_body`). absorb = 1 → alpha unchanged (Glass byte-identical).
        alpha = 1.0 - (1.0 - alpha) * dot(absorb, vec3<f32>(1.0 / 3.0));
        // Stochastic transparency (#152 Tier 2, the order-independent-transparency
        // item): when enabled (`u.sss.w`), dither-DISCARD by coverage instead of
        // alpha-blending — kept fragments are opaque and depth-sorted, so stacked
        // glass composites correctly regardless of draw order. The threshold is
        // animated per frame by the seed (`u.irid.w`) so TAA averages the dither
        // into smooth glass (needs TAA on, else it looks grainy). 0 → classic blend.
        if (u.sss.w > 0.5) {
            let seed = u.irid.w;
            let h = fract(sin(dot(in.clip.xy + vec2<f32>(seed * 7.13, seed * 3.71),
                                  vec2<f32>(12.9898, 78.233))) * 43758.5453);
            if (h > alpha) {
                discard;
            }
            return vec4<f32>(glass_body + glass_surf + direct + emissive + sss
                                 + irid_sheen + gi_term + ml_term, 1.0);
        }
        // PREMULTIPLIED output (the pipeline blends One / OneMinusSrcAlpha):
        // transparency attenuates only what passes THROUGH the body — the refracted
        // env, the subsurface glow, and the diffuse GI bounce — while the surface
        // terms (Fresnel reflection, direct specular, emissive, iridescent sheen)
        // composite at full strength. Straight alpha multiplied the WHOLE sum by
        // opacity, halving the mirror rim that makes glass read as glass.
        let through = (glass_body + sss + gi_term) * alpha;
        return vec4<f32>(through + glass_surf + direct + emissive + irid_sheen + ml_term,
                         alpha);
    }

    // ===== Standard: metallic-roughness PBR (IBL + direct) =====
    // #163 `f0_override`: lift the *dielectric* reflectance toward a neutral mirror
    // without forcing metallic = 1 (albedo is kept for the diffuse term). Lift the
    // dielectric base first, THEN blend to albedo by metallic — so coloured metal
    // (metallic ≈ 1) keeps its albedo-driven specular instead of being pulled to grey.
    // At f0_override 0 this is exactly today's metallic-driven F0.
    let f0_dielectric = mix(vec3<f32>(0.04), vec3<f32>(0.9), f0_override);
    let f0 = mix(f0_dielectric, albedo, metallic);
    let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let ks = f;
    let kd = (vec3<f32>(1.0) - ks) * (1.0 - metallic);

    let irradiance = sample_irradiance(n_env);
    let diffuse = irradiance * albedo;

    let prefiltered = sample_prefiltered(r_env, roughness);
    // The shared IBL sampler wraps in U (equirect longitude); at N·V → 1 bilinear
    // wrapping blends the LUT's N·V≈1 column with the N·V≈0 column (where the
    // scale term collapses), corrupting the split-sum exactly at mirror-facing
    // pixels. Clamp to half a texel inside the edge (LUT is 256², env.rs::LUT_SIZE).
    let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                  vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0),
                                            max(roughness, 1e-3)), 0.0).rg;
    // Multiple-scattering energy compensation (#174 T3, Fdez-Agüera 2019): the
    // split-sum term is single-scatter — at high roughness the energy of second+
    // microfacet bounces was simply lost, visibly darkening rough metals. The
    // correction reuses the LUT values already sampled: Ess = A + B is the
    // single-scatter directional albedo; the missing (1 − Ess) is returned
    // through the average-Fresnel multiple-scatter term. No-op as Ess → 1.
    let fss_ess = f0 * brdf.x + vec3<f32>(brdf.y);
    let ems = 1.0 - (brdf.x + brdf.y);
    let favg = f0 + (vec3<f32>(1.0) - f0) * (1.0 / 21.0);
    let fms = fss_ess * favg / (vec3<f32>(1.0) - ems * favg);
    // Iridescence + palette-influence tint the environment specular reflection;
    // specular occlusion darkens it where SSAO says the sky is blocked.
    // #214 T4: `diffr_tint` splits the env specular into diffraction orders (white
    // when off — byte-identical).
    let specular = prefiltered * (fss_ess + fms * ems) * irid_tint * refl_palette * spec_occ * diffr_tint;

    // Refraction overlay on Standard: the diffuse body opens into the refracted
    // transmission on the same (1 − ks) energy split the irradiance uses — the
    // PBR specular lobe, direct lights, and emissive stay, so it reads as this
    // material gone glassy (roughness frosts the refraction). Off → unchanged.
    var body = kd * diffuse;
    if (u.refr.y > 0.5) {
        let inv_nm = mat3x3<f32>(in.inv0, in.inv1, in.inv2);
        let trans = refr_overlay_trans(in.local_pos, inv_nm, v, n, ior,
                                       roughness, env_rot, albedo)
            * (vec3<f32>(1.0) - ks);
        body = mix(body, trans, clamp(u.refr.z, 0.0, 1.0));
    }
    let ambient = (body + specular) * env_intensity * ambient_mul * etint;

    // Distant/directional analytic lights add crisp specular glints IBL can't.
    // Anisotropy (#214 T1): the pure Anisotropic material (type 4) lands here and
    // the overlay on Standard also flips this on — an elliptical GGX highlight
    // stretched along the brush (the env lobe rode the bent `r` above). The env
    // specular keeps its isotropic-roughness split-sum (bent-vector approximation);
    // only the crisp analytic lobe goes elliptical. Off → today's isotropic PBR.
    var direct: vec3<f32>;
    if (aniso_on) {
        direct = direct_light_aniso(n, v, l_key, af_t, af_b, albedo, metallic, a_t, a_b, f0, key_rad)
               + direct_light_aniso(n, v, l_fill, af_t, af_b, albedo, metallic, a_t, a_b, f0, vec3<f32>(fill_w));
    } else {
        direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, key_rad)
               + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(fill_w));
    }

    // The probe bounce is DIFFUSE bleed, so it rides the same kd energy split as
    // the IBL irradiance — a pure metal (kd = 0) no longer receives it.
    // Demo point light (#288 T3): inert unless a Demo emitter drives it.
    let demo_pt = demo_point_light(in.world_pos, n, v, albedo, metallic, roughness, f0);
    let color = ambient + direct + emissive + sss + irid_sheen + gi_term * kd + ml_term + demo_pt;
    // Clearcoat/sheen (#214 T2): the pure Clearcoat (5) / Velvet (6) materials and
    // the Standard overlays. `base_scale` attenuates the base under the coat; the
    // coat glint + sheen fuzz add on top. #214 T4 adds glitter + retro (`micro_add`).
    // All off → color2 == color (identical).
    let color2 = color * base_scale + coat_spec + sheen_add + micro_add;
    // Premultiplied output (pipeline blends One / OneMinusSrcAlpha): rgb × opacity
    // reproduces the old SrcAlpha blend exactly, at any opacity.
    return vec4<f32>(color2 * opacity, opacity);
}

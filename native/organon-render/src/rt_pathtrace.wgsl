// Hardware-RT progressive path tracer (#200 Tier 4 — the ground-truth substrate).
//
// A full path tracer over the #195 Tier-0 TLAS: per pixel it casts a CAMERA ray
// (jittered for AA), traces up to `bounces` diffuse bounces against the scene,
// does next-event estimation toward the key + fill lights (a traced shadow ray
// per hit) plus each surface's own emissive glow, and misses to a simple
// analytic sky. One sample/pixel/frame is progressively averaged into an
// accumulation buffer whenever the camera is still (the visual resets the sample
// count on any camera move) — so a still frame converges to reference over
// seconds, the "ships beautiful before it ships fast" of the tier. Output is
// LINEAR HDR radiance into the same scene buffer bloom + the composite tonemap
// already consume, so exposure / tone-map / EDR all apply unchanged.
//
// Hits are shaded from the hit instance's transform + tint (the #195 local-space
// reconstruction: for the unit cube the local hit position IS both the RGB-cube
// albedo and the face normal) — and, since organon#217 T8, from its per-instance
// emission (`emits`, the cube pipeline's @location(8) buffer): an emissive hit adds
// `emit.rgb * emit.w` and terminates the path. Diffuse is the default; when the #258 Tier-2
// dielectric enable (`p.params2.x`) is on the bounce loop grows a real material:
// Glass/Refractive (`u.amb.y` 2/3) shade as a STOCHASTIC two-interface dielectric
// (exact-Fresnel reflect/transmit split, `refract` on entry AND exit, total-
// internal-reflection, Beer–Lambert body absorption over the traversed segment
// with σ = (1 − albedo)·absorption), and Chrome (1) as a perfect mirror. Off →
// the loop is byte-identical to the diffuse-only tracer. Monochromatic (single
// IOR) at this tier — true spectral dispersion (wavelengths), lens DoF and photon
// caustics are documented follow-ups. Every experimental-API call site stays in
// this module (the `rt_*` discipline).

// NOTE: `enable wgpu_ray_query;` is prepended by the module builder
// (`rt_pathtrace.rs`) / the wgsl test so it sits ahead of `nrc.wgsl`'s global
// declarations — WGSL requires all `enable` directives to precede any global decl.

// Mirror of render.rs::Uniforms (the scene's group-0 block, written verbatim).
struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    mat: vec4<f32>,        // x=metallic, y=roughness, z=glow, w=prefilter_mip_count
    env: vec4<f32>,        // x=exposure, y=env_intensity, z=env_rotation, w=opacity
    key_light: vec4<f32>,  // xyz = dir TO key light (unit), w = intensity
    fill_light: vec4<f32>, // xyz = dir TO fill light (unit), w = intensity
    amb: vec4<f32>,        // x=ambient/IBL mult, y=material_type, z=glass IOR
    sss: vec4<f32>,
    irid: vec4<f32>,
    env_tint: vec4<f32>,   // xyz = environment tint colour
    ripple: vec4<f32>,
    ripple_ctr: vec4<f32>,
    ripple_mode: vec4<f32>,
    glassx: vec4<f32>,
    reflect_ctl: vec4<f32>,
    refl_box_min: vec4<f32>,
    refl_box_max: vec4<f32>,
};

struct PtU {
    inv_view_proj: mat4x4<f32>, // unjittered current VP inverse (camera rays)
    cam_pos: vec4<f32>,         // xyz = camera world pos, w = accumulated sample count (spp)
    params: vec4<f32>,          // x = bounces, y = tube flag, z = ray reach, w = frame index
    params2: vec4<f32>,         // #258 T2: x = dielectric enable, y = absorption, z = composite, w = augment
    lens0: vec4<f32>,           // #258 T3: lens world centre (xyz) + active flag (w)
    lens1: vec4<f32>,           // #258 T3: lens shape (r, dz, aperture, plano)
    spectral: vec4<f32>,        // #258 T4: x = spectral_on, y = Abbe number, z = secondaries, w = _
    caustic: vec4<f32>,         // #258 T5: x = photon-caustic map live (add it in)
    nrc0: vec4<f32>,            // #256 T0: x = enable, y = confidence, z = omega, w = terminate_bounce
    nrc_bmin: vec4<f32>,        // #256 T0: field AABB min (xyz) for the cache position encode
    nrc_bmax: vec4<f32>,        // #256 T0: field AABB max (xyz)
    nrc1: vec4<f32>,            // #256 T1: x = guide_on, y = guide_candidates, z = firefly_on, w = firefly_clamp
    nrc_vol: vec4<f32>,         // #256 T3: x = volume_on, y = density, z = steps, w = strength
    nrc_caus: vec4<f32>,        // #256 T3: x = caustic_on, y = caustic_gain
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<uniform> p: PtU;
@group(0) @binding(2) var accum_tex: texture_2d<f32>; // previous accumulation
@group(0) @binding(3) var<storage, read> insts: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read> tints: array<vec4<f32>>;
// #258 T5: the resolved photon-caustic map (rt_caustic.wgsl); a zero 1×1 when off.
@group(0) @binding(5) var caustic_tex: texture_2d<f32>;
// #256 T0: the live radiance cache's trained SIREN weights (419 floats); zeroed +
// unread when the cache is off (`p.nrc0.x == 0`). `NRC_WEIGHTS` comes from nrc.wgsl.
@group(0) @binding(6) var<storage, read> nrc_w: array<f32, NRC_WEIGHTS>;
// organon#217 T8 — the per-instance EMISSION the cube pipeline reads at @location(8):
// linear radiance in rgb, gain in w. The same `emit_buf` the raster path binds at vertex
// slot 3, indexed by the same instance id the hit reports (`instance_custom_data`).
@group(0) @binding(7) var<storage, read> emits: array<vec4<f32>>;
@group(1) @binding(0) var tlas: acceleration_structure;

const PI: f32 = 3.14159265359;

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

// --- hash RNG (PCG-ish); a fresh stream per (pixel, sample, dimension) ---
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn rand(seed: ptr<function, u32>) -> f32 {
    *seed = pcg(*seed);
    return f32(*seed) / 4294967296.0;
}

// Cosine-weighted hemisphere direction around `n`.
fn cosine_dir(n: vec3<f32>, u0: f32, u1: f32) -> vec3<f32> {
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    let r = sqrt(u0);
    let a = u1 * 2.0 * PI;
    let z = sqrt(max(0.0, 1.0 - u0));
    return normalize(t * (r * cos(a)) + b * (r * sin(a)) + n * z);
}

fn inv3(m: mat4x4<f32>) -> mat3x3<f32> {
    let a = vec3<f32>(m[0].xyz);
    let b = vec3<f32>(m[1].xyz);
    let c = vec3<f32>(m[2].xyz);
    let r0 = cross(b, c);
    let r1 = cross(c, a);
    let r2 = cross(a, b);
    let det = dot(a, r0);
    let inv_det = 1.0 / select(det, 1e-8, abs(det) < 1e-12);
    return mat3x3<f32>(
        vec3<f32>(r0.x, r1.x, r2.x) * inv_det,
        vec3<f32>(r0.y, r1.y, r2.y) * inv_det,
        vec3<f32>(r0.z, r1.z, r2.z) * inv_det,
    );
}

// A cheap analytic sky (miss radiance): a horizon→zenith gradient tinted by the
// environment tint, plus a soft key-light disc, all pre-exposure linear HDR.
fn sky(dir: vec3<f32>) -> vec3<f32> {
    let up = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
    let horizon = vec3<f32>(0.55, 0.62, 0.72);
    let zenith = vec3<f32>(0.10, 0.16, 0.34);
    var col = mix(horizon, zenith, up) * u.env.y;
    let sun = max(dot(dir, normalize(u.key_light.xyz)), 0.0);
    col += vec3<f32>(1.0, 0.95, 0.85) * pow(sun, 220.0) * u.key_light.w;
    return col * u.env_tint.rgb;
}

// Shade a committed hit into (albedo, world normal facing the ray, world pos).
// organon#217 T8 — the per-instance emission at a hit: THE SAME EXPRESSION `cube.wgsl` adds
// into its emissive term from @location(8) (`emit.rgb * emit.w`), so raster and traced agree
// on what a lit cell is worth (§9's second law). The all-zero buffer every non-glyph draw
// binds makes this exactly vec3(0.0) — invariant #4.
fn instance_emission(idx: u32) -> vec3<f32> {
    let e = emits[idx];
    return e.rgb * e.w;
}

struct Hit {
    albedo: vec3<f32>,
    n: vec3<f32>,
    pos: vec3<f32>,
    emit: vec3<f32>, // organon#217 T8: the instance's own radiance (0 for the lens / no glyph)
};
fn shade_hit(origin: vec3<f32>, dir: vec3<f32>, idx: u32, t: f32, tube: bool) -> Hit {
    let m = insts[idx];
    let ainv = inv3(m);
    let hp = origin + dir * t;
    let local = ainv * (hp - m[3].xyz);
    var n_loc: vec3<f32>;
    var albedo_loc: vec3<f32>;
    if (tube) {
        n_loc = normalize(vec3<f32>(local.xy, 0.0));
        albedo_loc = vec3<f32>(1.0);
    } else {
        let al = abs(local);
        if (al.x >= al.y && al.x >= al.z) {
            n_loc = vec3<f32>(sign(local.x), 0.0, 0.0);
        } else if (al.y >= al.z) {
            n_loc = vec3<f32>(0.0, sign(local.y), 0.0);
        } else {
            n_loc = vec3<f32>(0.0, 0.0, sign(local.z));
        }
        albedo_loc = clamp(local + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    var hn = normalize(transpose(ainv) * n_loc);
    if (dot(hn, dir) > 0.0) { hn = -hn; }
    var o: Hit;
    o.albedo = albedo_loc * tints[idx].rgb;
    o.n = hn;
    o.pos = hp;
    o.emit = instance_emission(idx);
    return o;
}

// Exact (unpolarized) dielectric Fresnel reflectance (#258 T2). `cos_i` is the
// cosine of the incidence angle (≥ 0), `eta` = n_incident / n_transmitted. Returns
// 1.0 on total internal reflection (sin²θ_t ≥ 1), matching WGSL `refract`'s
// zero-vector TIR result so the caller's reflect/transmit split stays consistent.
fn fresnel_dielectric(cos_i: f32, eta: f32) -> f32 {
    let sin_t2 = eta * eta * max(0.0, 1.0 - cos_i * cos_i);
    if (sin_t2 >= 1.0) {
        return 1.0; // total internal reflection
    }
    let cos_t = sqrt(1.0 - sin_t2);
    let rs = (eta * cos_i - cos_t) / (eta * cos_i + cos_t);
    let rp = (eta * cos_t - cos_i) / (eta * cos_t + cos_i);
    return clamp(0.5 * (rs * rs + rp * rp), 0.0, 1.0);
}

// Two outputs, same value: loc0 → the ping-pong accumulation (read next frame),
// loc1 → the HDR scene buffer bloom + the composite tonemap already consume. One
// pass, no copy, and post/exposure/EDR all apply to the traced result unchanged.
// ===================== Analytic lens intersection (#258 Tier 3) =====================
// The lens generator is a raymarched SDF, NOT in the TLAS, so the path tracer can't
// see it via ray queries. Intersect it analytically instead — the lens body is a
// convex CSG (front sphere ∩ back sphere OR half-space ∩ aperture cylinder), so the
// ray's inside-interval is the intersection of each primitive's interval, and the
// nearest boundary crossing is the surface hit. Matches `lens.wgsl::lens_sdf`
// (recentred to `lens0.xyz`, optical axis = z). Returns the ray-facing normal.
struct LensHit { hit: bool, t: f32, n: vec3<f32> };

// (t_enter, t_exit) where the ray o+t*d is INSIDE the sphere; empty if it misses.
fn ray_sphere_in(o: vec3<f32>, d: vec3<f32>, c: vec3<f32>, r: f32) -> vec2<f32> {
    let oc = o - c;
    let a = dot(d, d);
    let b = 2.0 * dot(oc, d);
    let cc = dot(oc, oc) - r * r;
    let disc = b * b - 4.0 * a * cc;
    if (disc < 0.0) { return vec2<f32>(1e30, -1e30); }
    let s = sqrt(disc);
    let t0 = (-b - s) / (2.0 * a);
    let t1 = (-b + s) / (2.0 * a);
    return vec2<f32>(min(t0, t1), max(t0, t1));
}

// Outward geometric normal of primitive `prim` at local point `hp` (front=0, back=1,
// half-space=2, cylinder=3).
fn lens_prim_normal(prim: i32, hp: vec3<f32>, dz: f32) -> vec3<f32> {
    if (prim == 0) { return normalize(hp - vec3<f32>(0.0, 0.0, dz)); }
    if (prim == 1) { return normalize(hp - vec3<f32>(0.0, 0.0, -dz)); }
    if (prim == 2) { return vec3<f32>(0.0, 0.0, 1.0); }
    return normalize(vec3<f32>(hp.xy, 0.0));
}

fn lens_hit(ro: vec3<f32>, rd: vec3<f32>, tmin: f32) -> LensHit {
    var res: LensHit;
    res.hit = false;
    if (p.lens0.w < 0.5) { return res; }              // no lens active
    let r = p.lens1.x;
    let dz = p.lens1.y;
    let aper = p.lens1.z;
    let plano = p.lens1.w > 0.5;
    let o = ro - p.lens0.xyz;                          // lens-local (axis z)

    // Front spherical cap.
    let sf = ray_sphere_in(o, rd, vec3<f32>(0.0, 0.0, dz), r);
    var enter = sf.x; var exit = sf.y;
    var enter_prim = 0; var exit_prim = 0;

    // Back: biconvex = mirrored sphere; plano-convex = flat half-space z <= 0.
    if (plano) {
        var he = -1e30; var hx = 1e30;
        if (abs(rd.z) < 1e-8) {
            if (o.z > 0.0) { he = 1e30; hx = -1e30; }  // outside the half-space forever
        } else {
            let tz = -o.z / rd.z;
            if (rd.z > 0.0) { hx = tz; } else { he = tz; }
        }
        if (he > enter) { enter = he; enter_prim = 2; }
        if (hx < exit) { exit = hx; exit_prim = 2; }
    } else {
        let sb = ray_sphere_in(o, rd, vec3<f32>(0.0, 0.0, -dz), r);
        if (sb.x > enter) { enter = sb.x; enter_prim = 1; }
        if (sb.y < exit) { exit = sb.y; exit_prim = 1; }
    }

    // Aperture stop: axial cylinder of radius `aper`.
    let od = o.xy; let dd = rd.xy;
    let ca = dot(dd, dd); let cb = 2.0 * dot(od, dd); let cc = dot(od, od) - aper * aper;
    var ce = -1e30; var cx = 1e30;
    if (ca < 1e-12) {
        if (cc > 0.0) { ce = 1e30; cx = -1e30; }        // parallel + outside → never inside
    } else {
        let disc = cb * cb - 4.0 * ca * cc;
        if (disc < 0.0) { ce = 1e30; cx = -1e30; } else {
            let s = sqrt(disc);
            ce = (-cb - s) / (2.0 * ca); cx = (-cb + s) / (2.0 * ca);
        }
    }
    if (ce > enter) { enter = ce; enter_prim = 3; }
    if (cx < exit) { exit = cx; exit_prim = 3; }

    if (enter >= exit) { return res; }                  // empty intersection

    var t: f32; var prim: i32;
    if (enter > tmin) { t = enter; prim = enter_prim; }
    else if (exit > tmin) { t = exit; prim = exit_prim; }
    else { return res; }

    var n = lens_prim_normal(prim, o + rd * t, dz);
    if (dot(n, rd) > 0.0) { n = -n; }                   // face the incoming ray
    res.hit = true; res.t = t; res.n = n;
    return res;
}

// ===================== Spectral light transport (#258 Tier 4) =====================
// When `spectral_on`, each path carries ONE wavelength λ and glass refracts at a
// per-λ Cauchy IOR (a prism / dispersive lens throws a real spectrum). The scalar
// per-λ radiance is reconstructed to RGB via the CIE colour-matching functions; a
// white non-dispersive path integrates back to neutral.

// Cauchy two-term dispersion n(λ) = A + B/λ². `abbe` (Vd) → ∞ ⇒ B → 0 ⇒ n(λ) = nd
// (no dispersion → the Tier-2 monochromatic dielectric). λ in nanometres.
fn cauchy_ior(nd: f32, abbe: f32, lambda_nm: f32) -> f32 {
    let lum = lambda_nm * 1e-3; // → micrometres
    let fc = 1.0 / (0.48613 * 0.48613) - 1.0 / (0.65627 * 0.65627); // 1/λF² − 1/λC²
    let b = (nd - 1.0) / (max(abbe, 1e-3) * fc);
    let a = nd - b / (0.58929 * 0.58929); // subtract B/λd² so n(λd)=nd
    return a + b / (lum * lum);
}

// CIE 1931 2° colour-matching functions (Wyman/Sloan/Shirley 2013 multi-lobe fit).
fn gauss(x: f32, mu: f32, s1: f32, s2: f32) -> f32 {
    let s = select(s2, s1, x < mu);
    let t = (x - mu) * s;
    return exp(-0.5 * t * t);
}
fn cie_xyz(l: f32) -> vec3<f32> {
    let x = 1.056 * gauss(l, 599.8, 0.0264, 0.0323)
          + 0.362 * gauss(l, 442.0, 0.0624, 0.0374)
          - 0.065 * gauss(l, 501.1, 0.0490, 0.0382);
    let y = 0.821 * gauss(l, 568.8, 0.0213, 0.0247)
          + 0.286 * gauss(l, 530.9, 0.0613, 0.0322);
    let z = 1.217 * gauss(l, 437.0, 0.0845, 0.0278)
          + 0.681 * gauss(l, 459.0, 0.0385, 0.0725);
    return vec3<f32>(x, y, z);
}
fn xyz_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        3.2406 * c.x - 1.5372 * c.y - 0.4986 * c.z,
        -0.9689 * c.x + 1.8758 * c.y + 0.0415 * c.z,
        0.0557 * c.x - 0.2040 * c.y + 1.0570 * c.z,
    );
}
// Reconstruct a hero-wavelength scalar radiance to linear-sRGB, normalised so a
// spectrally flat radiance L reconstructs to L·(1,1,1). Out-of-gamut lobes clamped.
fn spectral_to_rgb(radiance: f32, lambda_nm: f32) -> vec3<f32> {
    let rgb_white = vec3<f32>(1.2048, 0.9484, 0.9087); // M·(1,1,1)
    let ybar_norm = 0.30554;                            // mean ȳ over 380–730 nm
    let rgb = xyz_to_srgb(cie_xyz(lambda_nm) * radiance) / (rgb_white * ybar_norm);
    return max(rgb, vec3<f32>(0.0));
}
// Spectral reflectance of an RGB value at λ (three primary lobes). White → 1 at all λ.
fn spectral_response(rgb: vec3<f32>, lambda_nm: f32) -> f32 {
    let wr = gauss(lambda_nm, 600.0, 0.012, 0.012);
    let wg = gauss(lambda_nm, 550.0, 0.012, 0.012);
    let wb = gauss(lambda_nm, 450.0, 0.012, 0.012);
    let sum = wr + wg + wb;
    return (rgb.r * wr + rgb.g * wg + rgb.b * wb) / max(sum, 1e-4);
}

struct Out {
    @location(0) accum: vec4<f32>,
    @location(1) scene: vec4<f32>,
};

@fragment
fn fs_main(in: VsOut) -> Out {
    let dims = vec2<f32>(textureDimensions(accum_tex));
    let px = vec2<u32>(clamp(in.pos.xy, vec2<f32>(0.0), dims - 1.0));
    let spp = u32(p.cam_pos.w);
    let frame = u32(p.params.w);
    let bounces = clamp(u32(p.params.x), 1u, 12u);
    let reach = max(p.params.z, 1e-3);
    let tube = p.params.y > 0.5;
    let l_key = normalize(u.key_light.xyz);
    let l_fill = normalize(u.fill_light.xyz);

    // Per-pixel/per-sample RNG stream.
    var seed = pcg(px.x * 1973u + px.y * 9277u + (spp + frame * 26699u) * 26699u + 1u);

    // Jittered camera ray (box filter AA across the accumulated samples).
    let jit = vec2<f32>(rand(&seed), rand(&seed)) - 0.5;
    let uv = (in.pos.xy + jit) / dims;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let near = p.inv_view_proj * vec4<f32>(ndc, 0.0, 1.0);
    let far = p.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    var origin = u.camera_pos.xyz;
    var dir = normalize(far.xyz / far.w - near.xyz / near.w);

    var radiance = vec3<f32>(0.0);
    // #256 T3 — cached caustics accumulate here, OUT of `radiance`, so (like the #258
    // photon-caustic map) they escape the GI-add indirect-only average + `augment`
    // scaling and are added at full DIRECT-style weight in the composite below.
    var nrc_caustic = vec3<f32>(0.0);
    var throughput = vec3<f32>(1.0);
    var rq: ray_query; // hoisted (the #195 T3 loop-local ray_query crash)

    // #258 T2 dielectric BTDF. When `dielectric` is off every branch below is
    // skipped and the loop is byte-identical to the diffuse-only tracer (no extra
    // RNG draws either, so the sample stream matches). `inside` tracks whether the
    // ray is currently INSIDE a dielectric body (a single-medium approximation — no
    // nested-glass stack at this tier); `medium_sigma` is that body's Beer–Lambert
    // σ per channel, applied over the traversed segment on the next hit.
    let dielectric = p.params2.x > 0.5;
    let absorb_strength = max(p.params2.y, 0.0);
    // Composite mode (params2.z): 2 = GI-add → the tracer contributes INDIRECT light
    // only, so at the PRIMARY hit (b == 0) skip the terms the raster already has
    // (direct sky, emissive, direct key/fill) and accumulate only what arrives via
    // bounces. Modes 0/1 accumulate the full radiance as before.
    let gi_only = p.params2.z > 1.5;
    let mat_type = u.amb.y;
    let ior = max(u.amb.z, 1.0001);
    var inside = false;
    var medium_sigma = vec3<f32>(0.0);
    // #256 T1 firefly clamp: the cache's expected outgoing luminance at the PRIMARY
    // hit, captured on the first diffuse bounce (−1 = not captured / clamp inactive).
    var ff_ref = -1.0;

    let spectral_on = p.spectral.x > 0.5;
    let abbe = p.spectral.y;
    if (spectral_on) {
        // === Hero-wavelength spectral integrator (#258 Tier 4) ===
        // Sample 1 + `secondaries` stratified wavelengths across 380–730 nm; each
        // traces a monochromatic path (glass + the lens refract at a per-λ Cauchy
        // IOR — a prism), its scalar radiance reconstructed to RGB and averaged.
        let cam_o = origin;
        let cam_d = dir;
        let nwl = 1u + min(u32(p.spectral.z), 8u);
        var spec_rgb = vec3<f32>(0.0);
        for (var w = 0u; w < nwl; w = w + 1u) {
            let strat = (f32(w) + rand(&seed)) / f32(nwl);
            let lambda = 380.0 + strat * 350.0;
            var so = cam_o;
            var sd = cam_d;
            var tp = 1.0;       // scalar throughput at λ
            var l_rad = 0.0;    // scalar radiance at λ
            var s_in = false;   // inside a dielectric body
            var medium_refl = 1.0;
            for (var b = 0u; b < bounces; b = b + 1u) {
                let was_in = s_in;
                rayQueryInitialize(&rq, tlas, RayDesc(0x1u, 0xFFu, 1e-3, reach, so, sd));
                loop { if (!rayQueryProceed(&rq)) { break; } }
                let hit = rayQueryGetCommittedIntersection(&rq);
                let tlas_hit = hit.kind == RAY_QUERY_INTERSECTION_TRIANGLE;
                let tlas_t = select(1e30, hit.t, tlas_hit);
                let lh = lens_hit(so, sd, 1e-3);           // the lens disperses light
                let use_lens = lh.hit && lh.t < tlas_t;
                if (!tlas_hit && !use_lens) {
                    // GI-add: skip the primary miss (the raster shows the background).
                    if (!(gi_only && b == 0u)) {
                        l_rad += tp * spectral_response(sky(sd), lambda); // miss → sky
                    }
                    break;
                }
                let seg_t = select(hit.t, lh.t, use_lens);
                if (was_in) { tp *= exp(-(1.0 - medium_refl) * absorb_strength * seg_t); }
                var h: Hit;
                if (use_lens) {
                    h.pos = so + sd * lh.t; h.n = lh.n; h.albedo = vec3<f32>(1.0);
                } else {
                    let idx = min(hit.instance_custom_data, arrayLength(&insts) - 1u);
                    h = shade_hit(so, sd, idx, hit.t, tube);
                }
                // organon#217 T8 — the per-instance emission's response at λ (all
                // materials; the primary hit is skipped in GI-add like the RGB path), and
                // an emitter TERMINATES the path — see the RGB loop for the reasoning.
                // `spectral_response(0, λ)` is exactly 0 and `+ 0.0` is exact, so with the
                // all-zero buffer the sum and the RNG stream are byte-identical.
                if (!(gi_only && b == 0u)) { l_rad += tp * spectral_response(h.emit, lambda); }
                if (any(h.emit > vec3<f32>(0.0))) { break; }
                if (dielectric && mat_type >= 0.5 && mat_type < 1.5 && !use_lens) {
                    // Chrome: perfect specular mirror (matches the RGB path — no colour,
                    // no NEE); dispersion only shows on refractive surfaces.
                    sd = reflect(sd, h.n); so = h.pos + h.n * 1e-2;
                } else if (dielectric && (use_lens || (mat_type >= 1.5 && mat_type < 3.5))) {
                    // Dielectric interface at the per-λ Cauchy IOR (the dispersion). Gated
                    // on `dielectric` like the RGB path, so glass/lens only refract (+
                    // Beer–Lambert absorb) when PT dielectric is on — else it's diffuse.
                    let n_l = cauchy_ior(ior, abbe, lambda);
                    let eta = select(1.0 / n_l, n_l, was_in);  // n_incident / n_transmitted
                    let cosi = clamp(dot(-sd, h.n), 0.0, 1.0);
                    let fr = fresnel_dielectric(cosi, eta);
                    let rr = refract(sd, h.n, eta);
                    if (rand(&seed) < fr || dot(rr, rr) < 1e-8) {
                        sd = reflect(sd, h.n); so = h.pos + h.n * 1e-2;   // reflect / TIR
                    } else {
                        sd = normalize(rr); so = h.pos - h.n * 1e-2;      // transmit
                        if (!was_in) { medium_refl = spectral_response(h.albedo, lambda); }
                        s_in = !was_in;
                    }
                } else {
                    // Diffuse (scalar hero-wavelength): emissive + NEE + cosine bounce.
                    let refl = spectral_response(h.albedo, lambda);
                    // GI-add: skip the primary-hit emissive (the raster already shows it).
                    if (!(gi_only && b == 0u)) { l_rad += tp * refl * u.mat.z; }
                    let ox = h.pos + h.n * 1e-2;
                    var direct = 0.0;
                    let nk = max(dot(h.n, l_key), 0.0);
                    if (nk > 0.0) {
                        rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 1e-3, reach, ox, l_key));
                        loop { if (!rayQueryProceed(&rq)) { break; } }
                        let s = rayQueryGetCommittedIntersection(&rq);
                        if (s.kind != RAY_QUERY_INTERSECTION_TRIANGLE) { direct += u.key_light.w * nk; }
                    }
                    let nf = max(dot(h.n, l_fill), 0.0);
                    if (nf > 0.0) {
                        rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 1e-3, reach, ox, l_fill));
                        loop { if (!rayQueryProceed(&rq)) { break; } }
                        let s = rayQueryGetCommittedIntersection(&rq);
                        if (s.kind != RAY_QUERY_INTERSECTION_TRIANGLE) { direct += u.fill_light.w * nf; }
                    }
                    // GI-add: skip primary-hit direct key/fill (raster covers it); at
                    // bounces it's indirect illumination.
                    if (!(gi_only && b == 0u)) { l_rad += tp * refl * direct / PI; }
                    tp *= refl;
                    sd = cosine_dir(h.n, rand(&seed), rand(&seed));
                    so = ox;
                    // #256 T0 — cache early-termination on the spectral hero-wavelength
                    // path too (mirrors the RGB path). The cache stores RGB radiance;
                    // project it onto this λ with `spectral_response` so it enters the
                    // scalar accumulator consistently. Gated on `nrc0.x`; off → skipped.
                    if (p.nrc0.x > 0.5 && f32(b) + 1.0 >= p.nrc0.w) {
                        let xq = nrc_encode(so, p.nrc_bmin.xyz, p.nrc_bmax.xyz, sd);
                        let cached = max(nrc_query(xq, p.nrc0.z), vec3<f32>(0.0));
                        l_rad += tp * spectral_response(cached, lambda) * clamp(p.nrc0.y, 0.0, 1.0);
                        break;
                    }
                }
                if (b >= 2u) {
                    let q = clamp(tp, 0.05, 0.95);
                    if (rand(&seed) > q) { break; }
                    tp /= q;
                }
            }
            spec_rgb += spectral_to_rgb(l_rad, lambda);
        }
        radiance = spec_rgb / f32(nwl);
    } else {
    // #256 T3 — volumetric in-scattering (god-rays): march the primary camera ray
    // through a participating medium, querying the cache for the in-scattered radiance
    // at each step (attenuated by transmittance) — a single-scatter glow/haze that
    // feeds the bloom + HDR chain (so it pulses with the beat for free). The first-hit
    // distance bounds the march to the haze IN FRONT of the surface. Gated on
    // volume_on + the cache; off → skipped, byte-identical (no state touched before
    // the bounce loop's own first query overwrites `rq`).
    if (p.nrc_vol.x > 0.5 && p.nrc0.x > 0.5) {
        rayQueryInitialize(&rq, tlas, RayDesc(0x1u, 0xFFu, 1e-3, reach, origin, dir));
        loop { if (!rayQueryProceed(&rq)) { break; } }
        let vh = rayQueryGetCommittedIntersection(&rq);
        // Bound the march to the nearest surface IN FRONT: the TLAS hit (or half of
        // `reach` on a miss) AND the analytic lens (#258 T3) the bounce loop also
        // intersects — else haze extends through an in-front lens / uses the wrong
        // bound when the lens is the first visible surface.
        var vend = select(reach * 0.5, vh.t, vh.kind == RAY_QUERY_INTERSECTION_TRIANGLE);
        let vlh = lens_hit(origin, dir, 1e-3);
        if (vlh.hit && vlh.t < vend) { vend = vlh.t; }
        let vsteps = clamp(u32(p.nrc_vol.z), 1u, 64u);
        let vsigma = max(p.nrc_vol.y, 0.0);
        let vdstep = vend / f32(vsteps);
        let vseg = exp(-vsigma * vdstep);
        var vtrans = 1.0;
        for (var vi = 0u; vi < vsteps; vi = vi + 1u) {
            let vp = origin + dir * (vdstep * (f32(vi) + 0.5));
            let inl = max(nrc_query(nrc_encode(vp, p.nrc_bmin.xyz, p.nrc_bmax.xyz, -dir), p.nrc0.z), vec3<f32>(0.0));
            radiance += vtrans * inl * vsigma * vdstep * p.nrc_vol.w;
            vtrans *= vseg;
        }
    }
    for (var b = 0u; b < bounces; b = b + 1u) {
        rayQueryInitialize(&rq, tlas, RayDesc(0x1u, 0xFFu, 1e-3, reach, origin, dir));
        loop { if (!rayQueryProceed(&rq)) { break; } }
        let hit = rayQueryGetCommittedIntersection(&rq);
        let tlas_hit = hit.kind == RAY_QUERY_INTERSECTION_TRIANGLE;
        let tlas_t = select(1e30, hit.t, tlas_hit);
        // Analytic lens (#258 T3): the raymarched lens isn't in the TLAS — intersect
        // it directly and take whichever surface (lens or instanced) is nearer.
        let lh = lens_hit(origin, dir, 1e-3);
        let use_lens = lh.hit && lh.t < tlas_t;
        if (!tlas_hit && !use_lens) {
            // GI-add: the primary miss is the background the raster already shows —
            // skip it; a bounce ray reaching the sky IS indirect illumination — keep.
            if (!(gi_only && b == 0u)) {
                radiance += throughput * sky(dir); // miss → sky, terminate
            }
            break;
        }
        let seg_t = select(hit.t, lh.t, use_lens);
        // Beer–Lambert absorption over the segment just travelled inside a body.
        if (inside) {
            throughput *= exp(-medium_sigma * seg_t);
        }
        var h: Hit;
        if (use_lens) {
            // Clear-glass lens surface: white albedo (no absorption tint), the analytic
            // ray-facing normal. The dielectric branch below refracts it → the lens
            // converges light to a focus when Material = Glass + dielectric are on.
            h.pos = origin + dir * lh.t;
            h.n = lh.n;
            h.albedo = vec3<f32>(1.0);
        } else {
            let idx = min(hit.instance_custom_data, arrayLength(&insts) - 1u);
            h = shade_hit(origin, dir, idx, hit.t, tube);
        }

        // Emissive: the surface's own glow (all materials), plus its per-instance
        // emission (organon#217 T8 — the glyph ring's phosphor, `cube.wgsl`'s exact
        // term). Skipped at the primary hit in GI-add mode (the raster already shows
        // both). Two products, not one factored one: `throughput * 0.0` then `+ 0.0`
        // is exact, so the all-zero buffer leaves the sum byte-identical.
        if (!(gi_only && b == 0u)) {
            radiance += throughput * h.albedo * u.mat.z + throughput * h.emit;
        }
        // organon#217 T8 — an emissive instance is a LIGHT: its radiance is in, and the
        // path terminates here (the "lights are emitters" simplification). A lit tile's
        // tint is the near-black faceplate (§4), so what the continuation would have
        // added is ≤ albedo × incident — under 4 % — and a fullscreen grid then costs
        // one ray per pixel instead of `bounces`. What is given up is the faceplate's own
        // sheen over a LIT cell. Gated on the emission's VALUE, never on "is this a glyph
        // instance": a dark tile with emit == 0 keeps bouncing and shows the room. With
        // the all-zero buffer every non-glyph draw binds this is never taken —
        // byte-identical, invariant #4.
        if (any(h.emit > vec3<f32>(0.0))) { break; }

        var did_specular = false;
        if (dielectric) {
            if (mat_type >= 0.5 && mat_type < 1.5) {
                // Chrome: perfect (colourless) specular mirror — no NEE for a delta BSDF.
                dir = reflect(dir, h.n);
                origin = h.pos + h.n * 1e-2;
                did_specular = true;
            } else if (mat_type >= 1.5 && mat_type < 3.5) {
                // Glass / Refractive: stochastic two-interface dielectric. `h.n` faces
                // the incoming ray (shade_hit flips it), so it is the incident-side
                // normal on BOTH entry and exit; `eta` = n_i/n_t flips with `inside`.
                let n = h.n;
                let eta = select(1.0 / ior, ior, inside);
                let cos_i = clamp(dot(-dir, n), 0.0, 1.0);
                let fr = fresnel_dielectric(cos_i, eta);
                let refr = refract(dir, n, eta);
                let tir = dot(refr, refr) < 1e-8;
                if (tir || rand(&seed) < fr) {
                    // Reflect (specular or TIR): stay on the same side of the surface.
                    dir = reflect(dir, n);
                    origin = h.pos + n * 1e-2;
                } else {
                    // Transmit through the interface: cross to the other side.
                    dir = normalize(refr);
                    origin = h.pos - n * 1e-2;
                    if (!inside) {
                        // Entering the body: its colour sets the surviving transmission.
                        medium_sigma = max(vec3<f32>(0.0), (vec3<f32>(1.0) - h.albedo) * absorb_strength);
                    }
                    inside = !inside;
                }
                did_specular = true;
            }
        }

        // #256 T2 — cache-lit reflections: a specular (Chrome/Glass) ray terminates
        // into a cache query along its reflected/refracted direction, so it reflects
        // the LIT neighbours + off-screen light instead of tracing on to the env only.
        // Gated on the reflect flag (nrc_bmin.w) + the cache being live; off → the
        // specular ray continues as before, byte-identical. NOT gated on the diffuse
        // terminate-bounce (nrc0.w): a specular event terminates into the cache the
        // moment it happens (a primary Chrome/Glass hit is bounce 0), else the default
        // terminate bounce of 2 would leave primary specular rays tracing unchanged.
        if (did_specular && p.nrc0.x > 0.5 && p.nrc_bmin.w > 0.5) {
            let xr = nrc_encode(origin, p.nrc_bmin.xyz, p.nrc_bmax.xyz, dir);
            let cr = max(nrc_query(xr, p.nrc0.z), vec3<f32>(0.0));
            radiance += throughput * cr * clamp(p.nrc0.y, 0.0, 1.0);
            break;
        }

        if (!did_specular) {
            // Diffuse (today's exact path): NEE toward the key + fill lights + a
            // cosine-sampled bounce.
            let ox = h.pos + h.n * 1e-2;
            // #256 T1 — firefly reference: at the CAMERA primary hit (b == 0), cache the
            // expected outgoing luminance here (queried along the surface normal) so
            // the post-loop clamp has an anchor. Gated on b == 0 so a specular primary
            // hit (glass/chrome/lens, which skips this diffuse block) leaves ff_ref
            // unset → no clamp, rather than anchoring on a deeper diffuse surface.
            // Only when firefly + the cache are on.
            if (p.nrc1.z > 0.5 && p.nrc0.x > 0.5 && b == 0u) {
                let er = max(nrc_query(nrc_encode(ox, p.nrc_bmin.xyz, p.nrc_bmax.xyz, h.n), p.nrc0.z), vec3<f32>(0.0));
                ff_ref = dot(er, vec3<f32>(0.2126, 0.7152, 0.0722));
            }
            // #256 T3 — cached caustics: the focused high-energy light a camera-first
            // path can't find through a specular chain lives in the cache. At the
            // primary hit add `caustic_gain ×` the cache's radiance arriving along the
            // MIRROR of the view (the direction focused caustic light concentrates), so
            // it feeds bloom as a bright highlight. Gated on caustic_on + the cache;
            // primary hit only; off → skipped, byte-identical.
            if (p.nrc_caus.x > 0.5 && p.nrc0.x > 0.5 && b == 0u) {
                let mdir = reflect(dir, h.n);
                let cf = max(nrc_query(nrc_encode(ox, p.nrc_bmin.xyz, p.nrc_bmax.xyz, mdir), p.nrc0.z), vec3<f32>(0.0));
                // Into nrc_caustic, not radiance — kept out of the GI-add average (see
                // the composite): caustics are focused DIRECT-style light, so `augment`
                // must not dim them nor count them as indirect over the raster's direct.
                nrc_caustic += throughput * cf * p.nrc_caus.y;
            }
            var direct = vec3<f32>(0.0);
            let nk = max(dot(h.n, l_key), 0.0);
            if (nk > 0.0) {
                rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 1e-3, reach, ox, l_key));
                loop { if (!rayQueryProceed(&rq)) { break; } }
                let s = rayQueryGetCommittedIntersection(&rq);
                if (s.kind != RAY_QUERY_INTERSECTION_TRIANGLE) {
                    direct += u.key_light.w * nk * vec3<f32>(1.0);
                }
            }
            let nf = max(dot(h.n, l_fill), 0.0);
            if (nf > 0.0) {
                rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 1e-3, reach, ox, l_fill));
                loop { if (!rayQueryProceed(&rq)) { break; } }
                let s = rayQueryGetCommittedIntersection(&rq);
                if (s.kind != RAY_QUERY_INTERSECTION_TRIANGLE) {
                    direct += u.fill_light.w * nf * vec3<f32>(1.0);
                }
            }
            // GI-add: skip direct key/fill at the primary hit (the raster's direct
            // lighting already covers it); at bounces it's indirect illumination.
            if (!(gi_only && b == 0u)) {
                radiance += throughput * h.albedo * direct / PI;
            }

            // Diffuse bounce: cosine-sampled, so the throughput just carries albedo.
            throughput *= h.albedo;
            // #256 T1 — NRC-guided importance sampling: choose the bounce by resampled
            // importance sampling (RIS) over `guide_candidates` cosine candidates,
            // weighted by the cache's predicted luminance in each — the path stops
            // wasting itself on dark directions (faster convergence at equal quality).
            // Unbiased: the one-sample RIS reweight (mean weight / picked weight) folds
            // into throughput. Gated on guide_on AND the cache being live; off → the
            // single cosine draw below, byte-identical (same two rand draws).
            if (p.nrc1.x > 0.5 && p.nrc0.x > 0.5) {
                let kc = clamp(u32(p.nrc1.y), 1u, 8u);
                var cand: array<vec3<f32>, 8>;
                var cw: array<f32, 8>;
                var wsum = 0.0;
                for (var c = 0u; c < kc; c = c + 1u) {
                    let dc = cosine_dir(h.n, rand(&seed), rand(&seed));
                    cand[c] = dc;
                    let lq = max(nrc_query(nrc_encode(ox, p.nrc_bmin.xyz, p.nrc_bmax.xyz, dc), p.nrc0.z), vec3<f32>(0.0));
                    let wl = dot(lq, vec3<f32>(0.2126, 0.7152, 0.0722)) + 1e-3;
                    cw[c] = wl;
                    wsum += wl;
                }
                let pick = rand(&seed) * wsum;
                var acc = 0.0;
                var j = 0u;
                for (var c = 0u; c < kc; c = c + 1u) {
                    acc += cw[c];
                    j = c;
                    if (pick <= acc) { break; }
                }
                dir = cand[j];
                throughput *= (wsum / f32(kc)) / max(cw[j], 1e-6);
            } else {
                dir = cosine_dir(h.n, rand(&seed), rand(&seed));
            }
            origin = ox;

            // #256 T0 — cache early-termination: once the path has taken
            // `terminate_bounce` diffuse bounces, stop tracing and TERMINATE into a
            // cache query of the incoming radiance along the chosen bounce direction
            // (infinite-bounce GI at short-path cost). Added at `confidence` weight —
            // the confidence blend, so a cold / mis-trained cache can only *lose* GI,
            // never corrupt the image (the raw trace above is the fallback). Gated on
            // `nrc0.x`; off → skipped with no extra RNG draws, byte-identical.
            if (p.nrc0.x > 0.5 && f32(b) + 1.0 >= p.nrc0.w) {
                let xq = nrc_encode(origin, p.nrc_bmin.xyz, p.nrc_bmax.xyz, dir);
                let cached = max(nrc_query(xq, p.nrc0.z), vec3<f32>(0.0));
                radiance += throughput * cached * clamp(p.nrc0.y, 0.0, 1.0);
                break;
            }
        }

        // Russian roulette after a couple of bounces (unbiased path termination).
        if (b >= 2u) {
            let q = clamp(max(throughput.r, max(throughput.g, throughput.b)), 0.05, 0.95);
            if (rand(&seed) > q) { break; }
            throughput /= q;
        }
    }
    } // end spectral_on / RGB path

    // #256 T1 — firefly suppression at the source: the cache IS the expected value, so
    // clamp this sample's luminance to `firefly_clamp × expected` (hue-preserving) —
    // a single bright outlier can't survive the progressive average as a firefly.
    // Gated on firefly_on + a positive clamp factor + a captured reference (RGB
    // diffuse paths only); off → unchanged. The `nrc1.w > 0.0` guard matches
    // `math::nrc_firefly_clamp`, which treats a non-positive factor as disabled
    // (else a zero dial would still impose the +0.05 luminance floor as a ceiling).
    if (p.nrc1.z > 0.5 && p.nrc1.w > 0.0 && ff_ref >= 0.0) {
        let lr = dot(radiance, vec3<f32>(0.2126, 0.7152, 0.0722));
        let cap = p.nrc1.w * ff_ref + 0.05;
        if (lr > cap && lr > 0.0) {
            radiance *= cap / lr;
        }
    }

    // #258 T5: photon-mapped caustics — the light-traced splat map carries the
    // focused specular-transmission light (LS+D paths) this camera-first tracer
    // can't find through a delta chain. Fresh photons land every frame, so the
    // progressive average below converges their noise exactly like the paths'.
    var caustic = vec3<f32>(0.0);
    if (p.caustic.x > 0.5) {
        caustic = textureLoad(caustic_tex, vec2<i32>(px), 0).rgb;
    }
    // #256 T3 — the cached-caustic term joins the photon caustics here so it gets the
    // identical DIRECT-style treatment (folded into the average in Replace/Blend, held
    // out of the GI-add average and added at full weight). It's deterministic at the
    // primary hit (no per-frame photon noise), so averaging it is a harmless no-op.
    caustic += nrc_caustic;
    // In Replace/Blend the presented image IS the average, so fold caustics into the
    // accumulated radiance (the running mean also converges their per-frame photon
    // noise). In GI-add the average is INDIRECT-only (gi_only) and gets scaled by
    // `augment` at composite; caustics are focused DIRECT-style light, so keep them
    // OUT of that average and add them at full weight below — otherwise `augment`
    // would dim them and they'd be mis-counted as indirect on top of the raster's
    // own direct lighting (the flagged double/mis-scale).
    let gi_add = p.params2.z > 1.5;
    if (!gi_add) { radiance += caustic; }

    // Progressive average: running mean over the accumulated samples. `spp` is the
    // count already integrated; the visual resets it to 0 on any camera move.
    let prev = textureLoad(accum_tex, vec2<i32>(px), 0).rgb;
    let n = f32(spp);
    let avg = select(radiance, (prev * n + radiance) / (n + 1.0), spp > 0u);
    var o: Out;
    o.accum = vec4<f32>(avg, 1.0);
    // Present the accumulation to the HDR scene per composite mode (params2.z):
    //   0 Replace  → overwrite (blend None, the pass clears the raster first).
    //   1 Blend    → alpha = augment; the pass keeps the raster + alpha-blends this
    //                over it (mix(raster, avg, augment)).
    //   2 GI add   → additive over the raster; `avg` is indirect-only (gi_only), so
    //                hdr = raster + augment·indirect.
    let composite = p.params2.z;
    let augment = clamp(p.params2.w, 0.0, 1.0);
    if (composite < 0.5) {
        o.scene = vec4<f32>(avg, 1.0);
    } else if (composite < 1.5) {
        o.scene = vec4<f32>(avg, augment);
    } else {
        o.scene = vec4<f32>(avg * augment + caustic, 1.0);
    }
    return o;
}

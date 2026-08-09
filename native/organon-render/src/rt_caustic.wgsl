// Photon-mapped caustics (#258 Tier 5 — "caustics that converge").
//
// The camera-first path tracer discovers focused specular-transmission light
// (a lens focal spot, a prism spectrum on a surface, glass concentrating the
// key light) only by blind luck — LS+D paths have no next-event estimation
// through a delta chain, so projected caustics resolve over thousands of
// samples. This pass traces the SAME light transport from the OTHER end:
//
//   cs_photon — one photon per invocation, emitted from the key light over a
//     disc covering the scene's bounding sphere. Each photon follows the exact
//     dielectric chain the tracer uses (`rt_pathtrace.wgsl`): Fresnel
//     reflect/transmit split, refract on entry AND exit, TIR, Beer–Lambert
//     absorption, the analytic lens body, and — when spectral dispersion is on
//     — a per-λ Cauchy IOR so a prism throws a real rainbow ONTO the floor.
//     When a photon that has been redirected by ≥ 1 specular event lands on a
//     receiving surface (and the landing point is visible from the camera —
//     one traced visibility ray), its flux is splatted into a per-pixel
//     fixed-point accumulator (atomics; the MLS-MPM P2G pattern).
//   cs_resolve — normalized disc blur over the splat buffer (a screen-space
//     kernel-density estimate: the photon "gather radius") into an HDR map the
//     path tracer adds to its radiance, so the progressive accumulation +
//     denoiser average the per-frame photon noise like everything else.
//
// Honest approximations, documented: photons that land on a DIFFUSE surface
// deposit fully and die (the classic caustic photon map — direct light is
// already handled by the tracer's NEE, so first-bounce photons never deposit);
// photons landing on glass/chrome deposit a small surface-scatter fraction
// (`CATCH`) and continue — perfect dielectrics have no diffuse lobe, but the
// all-glass scenes this product actually shows need received caustics to READ,
// so a dusty-surface fraction is the pragmatic choice. The tracer's own rare
// caustic paths still exist (slight double count) — they are exactly the noise
// this pass replaces, and the blend is visually negligible. Every experimental
// ray-query call site stays in an `rt_*` module (the discipline).

enable wgpu_ray_query;

// Mirror of render.rs::Uniforms (the scene's group-0 block, written verbatim —
// the same buffer the path tracer binds).
struct Uniforms {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    mat: vec4<f32>,        // x=metallic, y=roughness, z=glow, w=prefilter_mip_count
    env: vec4<f32>,        // x=exposure, y=env_intensity, z=env_rotation, w=opacity
    key_light: vec4<f32>,  // xyz = dir TO key light (unit), w = intensity
    fill_light: vec4<f32>,
    amb: vec4<f32>,        // x=ambient/IBL mult, y=material_type, z=glass IOR
    sss: vec4<f32>,
    irid: vec4<f32>,
    env_tint: vec4<f32>,
    ripple: vec4<f32>,
    ripple_ctr: vec4<f32>,
    ripple_mode: vec4<f32>,
    glassx: vec4<f32>,
    reflect_ctl: vec4<f32>,
    refl_box_min: vec4<f32>,
    refl_box_max: vec4<f32>,
};

struct CaU {
    view_proj: mat4x4<f32>, // UNJITTERED forward VP (project deposits to screen)
    sphere: vec4<f32>,      // xyz = scene bounding-sphere centre, w = radius
    lens0: vec4<f32>,       // #258 T3 analytic lens: world centre (xyz) + active (w)
    lens1: vec4<f32>,       // lens shape (r, dz, aperture, plano)
    spectral: vec4<f32>,    // x = spectral_on, y = Abbe number, z = tube flag
    params_a: vec4<f32>,    // x = absorption, y = photon count N, z = frame seed, w = pixel scale
    params_b: vec4<f32>,    // x = intensity, y = gather radius (px), z = width, w = height
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<uniform> c: CaU;
// Fixed-point splat accumulator: 3 × u32 (RGB) per pixel, cleared each frame.
@group(0) @binding(2) var<storage, read_write> photons: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read> insts: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read> tints: array<vec4<f32>>;
@group(0) @binding(5) var caustic_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(0) var tlas: acceleration_structure;

const PI: f32 = 3.14159265359;
// Fixed-point scale for the u32 splat accumulator.
const SCALE: f32 = 4096.0;

// Saturating fixed-point splat: add `v` into `photons[idx]`, clamping at
// u32::MAX instead of wrapping modulo 2^32. Focal spots concentrate thousands
// of photons into a few pixels, so a plain `atomicAdd` can overflow and wrap the
// brightest caustic pixels back to (near) black. A compare-exchange loop pins the
// accumulator to the ceiling; the resolve just reads a saturated (very bright)
// value there, which the tone-map rolls off — far better than a black hole.
fn splat_sat(idx: u32, v: u32) {
    if (v == 0u) { return; }
    var old = atomicLoad(&photons[idx]);
    loop {
        // Detect overflow: the wrapped sum is smaller than the existing value.
        let sum = old + v;
        let want = select(sum, 0xFFFFFFFFu, sum < old);
        if (want == old) { return; } // already saturated
        let r = atomicCompareExchangeWeak(&photons[idx], old, want);
        if (r.exchanged) { return; }
        old = r.old_value;
    }
}
// Surface-scatter fraction a specular receiver keeps of an arriving caustic
// (perfect dielectrics have none; all-glass scenes need caustics to read).
const CATCH: f32 = 0.2;
// Max specular-chain segments per photon.
const MAX_SEG: u32 = 8u;

// --- hash RNG (PCG-ish), the tracer's exact stream machinery ---
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn rand(seed: ptr<function, u32>) -> f32 {
    *seed = pcg(*seed);
    return f32(*seed) / 4294967296.0;
}

fn inv3(m: mat4x4<f32>) -> mat3x3<f32> {
    let a = vec3<f32>(m[0].xyz);
    let b = vec3<f32>(m[1].xyz);
    let cc = vec3<f32>(m[2].xyz);
    let r0 = cross(b, cc);
    let r1 = cross(cc, a);
    let r2 = cross(a, b);
    let det = dot(a, r0);
    let inv_det = 1.0 / select(det, 1e-8, abs(det) < 1e-12);
    return mat3x3<f32>(
        vec3<f32>(r0.x, r1.x, r2.x) * inv_det,
        vec3<f32>(r0.y, r1.y, r2.y) * inv_det,
        vec3<f32>(r0.z, r1.z, r2.z) * inv_det,
    );
}

struct Hit {
    albedo: vec3<f32>,
    n: vec3<f32>,
    pos: vec3<f32>,
};
// The tracer's local-space hit reconstruction (RGB-cube albedo + face normal).
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
    return o;
}

// Exact (unpolarized) dielectric Fresnel; 1.0 on TIR (rt_pathtrace.wgsl's).
fn fresnel_dielectric(cos_i: f32, eta: f32) -> f32 {
    let sin_t2 = eta * eta * max(0.0, 1.0 - cos_i * cos_i);
    if (sin_t2 >= 1.0) { return 1.0; }
    let cos_t = sqrt(1.0 - sin_t2);
    let rs = (eta * cos_i - cos_t) / (eta * cos_i + cos_t);
    let rp = (eta * cos_t - cos_i) / (eta * cos_t + cos_i);
    return clamp(0.5 * (rs * rs + rp * rp), 0.0, 1.0);
}

// Cauchy two-term dispersion n(λ) — rt_pathtrace.wgsl's (#258 T4).
fn cauchy_ior(nd: f32, abbe: f32, lambda_nm: f32) -> f32 {
    let lum = lambda_nm * 1e-3;
    let fc = 1.0 / (0.48613 * 0.48613) - 1.0 / (0.65627 * 0.65627);
    let b = (nd - 1.0) / (max(abbe, 1e-3) * fc);
    let a = nd - b / (0.58929 * 0.58929);
    return a + b / (lum * lum);
}

// CIE reconstruction (rt_pathtrace.wgsl's #258 T4 kit).
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
fn xyz_to_srgb(cxyz: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        3.2406 * cxyz.x - 1.5372 * cxyz.y - 0.4986 * cxyz.z,
        -0.9689 * cxyz.x + 1.8758 * cxyz.y + 0.0415 * cxyz.z,
        0.0557 * cxyz.x - 0.2040 * cxyz.y + 1.0570 * cxyz.z,
    );
}
fn spectral_to_rgb(radiance: f32, lambda_nm: f32) -> vec3<f32> {
    let rgb_white = vec3<f32>(1.2048, 0.9484, 0.9087);
    let ybar_norm = 0.30554;
    let rgb = xyz_to_srgb(cie_xyz(lambda_nm) * radiance) / (rgb_white * ybar_norm);
    return max(rgb, vec3<f32>(0.0));
}
fn spectral_response(rgb: vec3<f32>, lambda_nm: f32) -> f32 {
    let wr = gauss(lambda_nm, 600.0, 0.012, 0.012);
    let wg = gauss(lambda_nm, 550.0, 0.012, 0.012);
    let wb = gauss(lambda_nm, 450.0, 0.012, 0.012);
    let sum = wr + wg + wb;
    return (rgb.r * wr + rgb.g * wg + rgb.b * wb) / max(sum, 1e-4);
}

// ===================== Analytic lens intersection (#258 Tier 3) =====================
// rt_pathtrace.wgsl's exact lens CSG, so a photon refracts through the identical body.
struct LensHit { hit: bool, t: f32, n: vec3<f32> };

fn ray_sphere_in(o: vec3<f32>, d: vec3<f32>, ctr: vec3<f32>, r: f32) -> vec2<f32> {
    let oc = o - ctr;
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

fn lens_prim_normal(prim: i32, hp: vec3<f32>, dz: f32) -> vec3<f32> {
    if (prim == 0) { return normalize(hp - vec3<f32>(0.0, 0.0, dz)); }
    if (prim == 1) { return normalize(hp - vec3<f32>(0.0, 0.0, -dz)); }
    if (prim == 2) { return vec3<f32>(0.0, 0.0, 1.0); }
    return normalize(vec3<f32>(hp.xy, 0.0));
}

fn lens_hit(ro: vec3<f32>, rd: vec3<f32>, tmin: f32) -> LensHit {
    var res: LensHit;
    res.hit = false;
    if (c.lens0.w < 0.5) { return res; }
    let r = c.lens1.x;
    let dz = c.lens1.y;
    let aper = c.lens1.z;
    let plano = c.lens1.w > 0.5;
    let o = ro - c.lens0.xyz;

    let sf = ray_sphere_in(o, rd, vec3<f32>(0.0, 0.0, dz), r);
    var enter = sf.x; var exit = sf.y;
    var enter_prim = 0; var exit_prim = 0;

    if (plano) {
        var he = -1e30; var hx = 1e30;
        if (abs(rd.z) < 1e-8) {
            if (o.z > 0.0) { he = 1e30; hx = -1e30; }
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

    let od = o.xy; let dd = rd.xy;
    let ca = dot(dd, dd); let cb = 2.0 * dot(od, dd); let cc = dot(od, od) - aper * aper;
    var ce = -1e30; var cx = 1e30;
    if (ca < 1e-12) {
        if (cc > 0.0) { ce = 1e30; cx = -1e30; }
    } else {
        let disc = cb * cb - 4.0 * ca * cc;
        if (disc < 0.0) { ce = 1e30; cx = -1e30; } else {
            let s = sqrt(disc);
            ce = (-cb - s) / (2.0 * ca); cx = (-cb + s) / (2.0 * ca);
        }
    }
    if (ce > enter) { enter = ce; enter_prim = 3; }
    if (cx < exit) { exit = cx; exit_prim = 3; }

    if (enter >= exit) { return res; }

    var t: f32; var prim: i32;
    if (enter > tmin) { t = enter; prim = enter_prim; }
    else if (exit > tmin) { t = exit; prim = exit_prim; }
    else { return res; }

    var n = lens_prim_normal(prim, o + rd * t, dz);
    if (dot(n, rd) > 0.0) { n = -n; }
    res.hit = true; res.t = t; res.n = n;
    return res;
}

// ============================ Photon tracing ============================

@compute @workgroup_size(64)
fn cs_photon(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n_photons = u32(c.params_a.y);
    if (gid.x >= n_photons) { return; }
    var rq: ray_query; // hoisted (the #195 T3 loop-local ray_query crash)
    var seed = pcg(gid.x * 9781u + u32(c.params_a.z) * 26699u + 77u);

    // Emit from a disc covering the scene bounding sphere, facing down the key
    // light (a directional light: intensity is irradiance E, matching the
    // tracer's NEE `key.w · cosθ`, so photon flux = E · disc area / N).
    let kd = normalize(u.key_light.xyz); // dir TO the light
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(kd.y) > 0.9);
    let t1 = normalize(cross(up, kd));
    let t2 = cross(kd, t1);
    let radius = max(c.sphere.w, 1e-3);
    let dr = radius * sqrt(rand(&seed));
    let da = rand(&seed) * 2.0 * PI;
    var so = c.sphere.xyz + kd * (2.0 * radius) + (t1 * cos(da) + t2 * sin(da)) * dr;
    var sd = -kd;
    let flux = u.key_light.w * PI * radius * radius / f32(n_photons);

    let tube = c.spectral.z > 0.5;
    let absorb = max(c.params_a.x, 0.0);
    let mt = u.amb.y;
    let ior = max(u.amb.z, 1.0001);
    let spectral_on = c.spectral.x > 0.5;
    let abbe = c.spectral.y;
    // Hero wavelength: stratified across the photon population so the spectrum
    // fills evenly each frame (the prism rainbow ON the floor).
    let lambda = 380.0 + 350.0 * ((f32(gid.x % 64u) + rand(&seed)) / 64.0);

    let reach = radius * 8.0;
    var tp = vec3<f32>(1.0);          // path throughput (grey in spectral mode)
    var inside = false;
    var medium_sigma = vec3<f32>(0.0); // Beer–Lambert σ of the current body
    var spec_events = 0u;              // specular redirects so far (deposit gate)

    for (var b = 0u; b < MAX_SEG; b = b + 1u) {
        let was_in = inside;
        rayQueryInitialize(&rq, tlas, RayDesc(0x1u, 0xFFu, 1e-3, reach, so, sd));
        loop { if (!rayQueryProceed(&rq)) { break; } }
        let hit = rayQueryGetCommittedIntersection(&rq);
        let tlas_hit = hit.kind == RAY_QUERY_INTERSECTION_TRIANGLE;
        let tlas_t = select(1e30, hit.t, tlas_hit);
        let lh = lens_hit(so, sd, 1e-3);
        let use_lens = lh.hit && lh.t < tlas_t;
        if (!tlas_hit && !use_lens) { return; } // escaped the scene

        let seg_t = select(hit.t, lh.t, use_lens);
        if (was_in) { tp *= exp(-medium_sigma * seg_t); }

        var h: Hit;
        if (use_lens) {
            h.pos = so + sd * lh.t;
            h.n = lh.n;
            h.albedo = vec3<f32>(1.0);
        } else {
            let idx = min(hit.instance_custom_data, arrayLength(&insts) - 1u);
            h = shade_hit(so, sd, idx, hit.t, tube);
        }

        // Classify the surface: 0 diffuse receiver, 1 chrome mirror, 2 dielectric.
        var kind = 0;
        if (use_lens) { kind = 2; }
        else if (mt >= 0.5 && mt < 1.5) { kind = 1; }
        else if (mt >= 1.5 && mt < 3.5) { kind = 2; }

        // Deposit: full on a diffuse receiver (then die — the classic caustic
        // photon map; unredirected photons never deposit, NEE owns direct light),
        // a CATCH fraction on a specular receiver (then keep going).
        var dep = vec3<f32>(0.0);
        if (spec_events > 0u) {
            // In spectral mode the path is monochromatic: the receiver's albedo
            // acts through its response at λ (tp stays grey), and the deposit is
            // reconstructed to RGB via the CIE fit below.
            var alb = h.albedo;
            if (spectral_on) { alb = vec3<f32>(spectral_response(h.albedo, lambda)); }
            dep = tp * alb;
            if (kind != 0) { dep *= CATCH; }
        }
        if (dep.r + dep.g + dep.b > 0.0) {
            // Landing point must be visible from the camera: front-facing, an
            // unobstructed traced ray, inside the frame.
            let toc = u.camera_pos.xyz - h.pos;
            let dist = length(toc);
            let v = toc / max(dist, 1e-4);
            let ct = dot(h.n, v);
            if (ct > 0.02 && dist > 1e-3) {
                var vis = true;
                rayQueryInitialize(&rq, tlas, RayDesc(0x5u, 0xFFu, 1e-3, dist * 0.999, u.camera_pos.xyz, -v));
                loop { if (!rayQueryProceed(&rq)) { break; } }
                let s = rayQueryGetCommittedIntersection(&rq);
                if (s.kind == RAY_QUERY_INTERSECTION_TRIANGLE) { vis = false; }
                if (vis) {
                    let vl = lens_hit(u.camera_pos.xyz, -v, 1e-3);
                    if (vl.hit && vl.t < dist * 0.999) { vis = false; }
                }
                let clip = c.view_proj * vec4<f32>(h.pos, 1.0);
                if (vis && clip.w > 1e-4) {
                    let ndc = clip.xy / clip.w;
                    if (abs(ndc.x) <= 1.0 && abs(ndc.y) <= 1.0) {
                        let w = c.params_b.z;
                        let hgt = c.params_b.w;
                        let px = u32(clamp((ndc.x * 0.5 + 0.5) * w, 0.0, w - 1.0));
                        let py = u32(clamp((0.5 - ndc.y * 0.5) * hgt, 0.0, hgt - 1.0));
                        // Pixel footprint on the surface → irradiance → Lambertian
                        // outgoing radiance (E · albedo / π; albedo is in `dep`).
                        let px_w = max(c.params_a.w * dist, 1e-5);
                        let area = px_w * px_w / max(ct, 0.05);
                        let l = dep * flux / (area * PI);
                        var rgb = l;
                        // Grey scalar in spectral mode → the CIE reconstruction
                        // (a white non-dispersive deposit integrates to neutral).
                        if (spectral_on) { rgb = spectral_to_rgb(l.r, lambda); }
                        let base = (py * u32(w) + px) * 3u;
                        splat_sat(base, u32(clamp(rgb.r, 0.0, 3e4) * SCALE));
                        splat_sat(base + 1u, u32(clamp(rgb.g, 0.0, 3e4) * SCALE));
                        splat_sat(base + 2u, u32(clamp(rgb.b, 0.0, 3e4) * SCALE));
                    }
                }
            }
            if (kind == 0) { return; } // absorbed by the diffuse receiver
        } else if (kind == 0) {
            return; // un-redirected photon on diffuse: direct light, NEE owns it
        }

        // Specular interaction (kind 1/2) — the tracer's exact chain.
        if (kind == 1) {
            sd = reflect(sd, h.n);
            so = h.pos + h.n * 1e-2;
            spec_events += 1u;
        } else {
            var n_i = ior;
            if (spectral_on) { n_i = cauchy_ior(ior, abbe, lambda); }
            let eta = select(1.0 / n_i, n_i, was_in);
            let cos_i = clamp(dot(-sd, h.n), 0.0, 1.0);
            let fr = fresnel_dielectric(cos_i, eta);
            let refr = refract(sd, h.n, eta);
            let tir = dot(refr, refr) < 1e-8;
            if (tir || rand(&seed) < fr) {
                sd = reflect(sd, h.n);
                so = h.pos + h.n * 1e-2;
            } else {
                sd = normalize(refr);
                so = h.pos - h.n * 1e-2;
                if (!inside) {
                    if (spectral_on) {
                        let resp = spectral_response(h.albedo, lambda);
                        medium_sigma = vec3<f32>((1.0 - resp) * absorb);
                    } else {
                        medium_sigma = max(vec3<f32>(0.0), (vec3<f32>(1.0) - h.albedo) * absorb);
                    }
                }
                inside = !inside;
            }
            spec_events += 1u;
        }

        // Russian roulette after a few segments (unbiased termination).
        if (b >= 3u) {
            let q = clamp(max(tp.r, max(tp.g, tp.b)), 0.05, 0.95);
            if (rand(&seed) > q) { return; }
            tp /= q;
        }
    }
}

// ============================ KDE resolve ============================
// Normalized disc blur over the splat buffer — a screen-space kernel-density
// estimate (the photon gather radius). Mean over the kernel conserves energy;
// radius 0 = the raw single-pixel splats.

@compute @workgroup_size(8, 8)
fn cs_resolve(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(c.params_b.z);
    let h = u32(c.params_b.w);
    if (gid.x >= w || gid.y >= h) { return; }
    let r = i32(clamp(c.params_b.y, 0.0, 8.0));
    var sum = vec3<f32>(0.0);
    var count = 0.0;
    for (var dy = -r; dy <= r; dy = dy + 1) {
        for (var dx = -r; dx <= r; dx = dx + 1) {
            if (dx * dx + dy * dy > r * r) { continue; }
            let sx = i32(gid.x) + dx;
            let sy = i32(gid.y) + dy;
            if (sx < 0 || sy < 0 || sx >= i32(w) || sy >= i32(h)) { continue; }
            let base = (u32(sy) * w + u32(sx)) * 3u;
            sum += vec3<f32>(
                f32(atomicLoad(&photons[base])),
                f32(atomicLoad(&photons[base + 1u])),
                f32(atomicLoad(&photons[base + 2u])),
            );
            count += 1.0;
        }
    }
    let intensity = max(c.params_b.x, 0.0);
    let out = sum * intensity / (max(count, 1.0) * SCALE);
    textureStore(caustic_out, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(out, 1.0));
}

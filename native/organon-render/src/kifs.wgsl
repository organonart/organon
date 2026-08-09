// Organic Math — Kaleidoscopic Fractal (KIFS) generator (2-D field).
//
// A fullscreen per-pixel field: N-fold kaleidoscopic symmetry feeding a choice of
// fractal "engines" (circle inversion, Mandelbox, Sierpinski, log-spiral, Kleinian),
// with concentric rings, radial rays, an optional domain warp, a selectable colour
// palette cycled at its own rate, and a flat OR a true receding-tunnel projection.
// Paints LINEAR HDR radiance and lets the shared bloom + tonemap composite finish
// it (no fake inverse-distance bloom substitute beyond the emission profile, no
// local tonemap).
//
// Clean-room from the maths (kaleidoscope fold + assorted IFS engines), not a copy
// of any specific shader.

struct KifsU {
    p0: vec4<f32>, // aspect, time, zoom, sectors
    p1: vec4<f32>, // fold_c, iter_rot, iterations, tunnel(0/1)
    p2: vec4<f32>, // ring_gain, ray_count, spin, glow_gain
    p3: vec4<f32>, // hue, breathe, pattern_id, palette_id
    p4: vec4<f32>, // color_speed, warp, flow, petals
    p5: vec4<f32>, // contrast, sharp, invert, dispersion
    p6: vec4<f32>, // space_id (0 Eucl / 1 Hyp / 2 Quasi / 3 Weyl / 4 E8), churn, e8_flow, view(3D mode)
    p7: vec4<f32>, // relief_height, 3D_elev, 3D_steps, 3D_shine (shared by Relief + Conformal)
    e8: array<vec4<f32>, 4>,    // E8 Petrie rings: 8×(radius, phase) packed 2-per-vec4
    e8pts: array<vec4<f32>, 120>, // 240 re-projected roots (8-D rotation), 2-per-vec4
    e8pts3: array<vec4<f32>, 240>, // 240 roots in 3-D (Lattice E8 shadow): xyz + pad
};
@group(0) @binding(0) var<uniform> m: KifsU;

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;

// Rotation matrix R(a): R·v rotates v by +a (column-major construction).
fn rot2(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(c, s, -s, c);
}

fn sd_circle(p: vec2<f32>, r: f32) -> f32 {
    return abs(length(p) - r);
}

// Petal (rhodonea) curve: |r - (radius + A·cos(petals·θ))|.
fn petal(p: vec2<f32>, petals: f32, radius: f32) -> f32 {
    let a = atan2(p.y, p.x);
    let r = length(p);
    let f = cos(a * petals) * 0.08;
    return abs(r - (radius + f));
}

fn hex_grid(p_in: vec2<f32>) -> f32 {
    let p = abs(p_in);
    return max(dot(p, normalize(vec2<f32>(1.0, 1.73))), p.x);
}

// ---------- colour: a bank of IQ cosine palettes, a+b·cos(2π(c·t+d)) ----------
fn palette(id: i32, t: f32) -> vec3<f32> {
    var a = vec3<f32>(0.5);
    var b = vec3<f32>(0.5);
    var c = vec3<f32>(1.0);
    var d = vec3<f32>(0.15, 0.35, 0.65); // 0: Spectral (the original look)
    if (id == 1) {
        // Ember — deep red → orange → gold.
        a = vec3<f32>(0.5, 0.3, 0.15);
        b = vec3<f32>(0.5, 0.35, 0.2);
        c = vec3<f32>(1.0, 1.0, 1.0);
        d = vec3<f32>(0.0, 0.1, 0.2);
    } else if (id == 2) {
        // Ice — teal → cyan → white.
        a = vec3<f32>(0.4, 0.5, 0.6);
        b = vec3<f32>(0.3, 0.4, 0.4);
        c = vec3<f32>(1.0, 1.0, 1.0);
        d = vec3<f32>(0.5, 0.55, 0.65);
    } else if (id == 3) {
        // Toxic — green → lime → yellow (viridis-ish).
        a = vec3<f32>(0.3, 0.45, 0.25);
        b = vec3<f32>(0.4, 0.4, 0.2);
        c = vec3<f32>(1.0, 1.0, 0.8);
        d = vec3<f32>(0.5, 0.25, 0.1);
    } else if (id == 4) {
        // Neon — magenta ↔ cyan.
        a = vec3<f32>(0.5, 0.4, 0.5);
        b = vec3<f32>(0.5, 0.4, 0.5);
        c = vec3<f32>(1.0, 0.7, 1.0);
        d = vec3<f32>(0.3, 0.9, 0.6);
    } else if (id == 5) {
        // Gold — amber/sepia, low chroma swing.
        a = vec3<f32>(0.5, 0.42, 0.28);
        b = vec3<f32>(0.45, 0.38, 0.22);
        c = vec3<f32>(1.0, 1.0, 1.0);
        d = vec3<f32>(0.1, 0.15, 0.2);
    } else if (id == 6) {
        // Rainbow — full-saturation wheel.
        a = vec3<f32>(0.5);
        b = vec3<f32>(0.5);
        c = vec3<f32>(1.0);
        d = vec3<f32>(0.0, 0.33, 0.67);
    }
    return a + b * cos(TAU * (c * t + d));
}

// ---------- the fractal engines (each a different inner-loop fold) ----------
// All accumulate emission on shared motif zero-sets (circle / petal / hex), so the
// pattern changes the GEOMETRY while the palette + rings + rays stay consistent.
// `line` is the motif filament half-width (from the `sharp` slider).
fn motif_glow(z: vec2<f32>, petals: f32, line: f32) -> f32 {
    let c1 = sd_circle(z, 0.55);
    let c2 = petal(z, petals, 0.38);
    let h = abs(hex_grid(z * 1.8) - 0.72);
    return 0.015 / (c1 + line) + 0.010 / (c2 + line) + 0.006 / (h + line);
}

fn fractal_glow(
    q_in: vec2<f32>, iters: i32, pattern: i32,
    fold_c: f32, iter_rot: f32, time: f32, petals: f32, line: f32,
) -> f32 {
    var z = q_in;
    var glow = 0.0;
    let rstep = rot2(0.25 + iter_rot + time * 0.2);
    for (var i = 0; i < iters; i = i + 1) {
        if (pattern == 1) {
            // Mandelbox-2D: box fold + ball fold + scale.
            z = clamp(z, vec2<f32>(-1.0), vec2<f32>(1.0)) * 2.0 - z;
            let r2 = max(dot(z, z), 1e-6);
            if (r2 < 0.25) {
                z = z * 4.0;
            } else if (r2 < 1.0) {
                z = z / r2;
            }
            z = rstep * (z * (1.6 + fold_c) - vec2<f32>(fold_c * 0.3));
        } else if (pattern == 2) {
            // Sierpinski kaleidoscope: absolute-value diagonal folds + scale/offset.
            z = abs(z);
            if (z.x < z.y) { z = z.yx; }
            z = rstep * (z * (1.0 + fold_c) - vec2<f32>(fold_c));
        } else if (pattern == 3) {
            // Log-spiral: conformal twist in log-polar space → spiral arms.
            let r = max(length(z), 1e-4);
            let a = atan2(z.y, z.x) + (0.6 + iter_rot) * log(r) + time * 0.15;
            let lr = log(r) - fold_c;
            z = vec2<f32>(cos(a), sin(a)) * exp(lr);
            z = rstep * z;
        } else if (pattern == 4) {
            // Kleinian-ish: pure circle inversion + abs fold (no self-square).
            z = z / max(dot(z, z), 1e-6);
            z = abs(z) - vec2<f32>(fold_c);
            z = rstep * z;
        } else {
            // 0 — Circle-inversion absolute-value IFS (the original engine).
            z = abs(z) / max(dot(z, z), 1e-6) - vec2<f32>(fold_c);
            z = rstep * z;
        }
        glow = glow + motif_glow(z, petals, line);
    }
    return glow;
}

// ---------- GROUP 1, ENGINE: aperiodic quasicrystal ----------
// Sum of N plane waves at incommensurate orientations → quasiperiodic order with
// N-fold symmetry that NEVER repeats (no translational symmetry — a genuinely
// different topology from every periodic mode). N (= sectors) odd/non-crystallo-
// graphic (5,7,8,10,12) is the interesting regime; `fold_c` sets the wave density.
// This is the cut-and-project / de Bruijn structure that descends from high-D
// lattices (Penrose from Z⁵, the Elser–Sloane quasicrystal from E8). Returns the
// interference web AND its gradient (used to warp the fractal pattern aperiodically).
struct Qc { web: f32, warp: vec2<f32> };
fn quasicrystal_eval(p: vec2<f32>, sectors: f32) -> Qc {
    let time = m.p0.y;
    let freq = 6.0 + m.p1.x * 10.0;            // fold_c → wave density
    let n = i32(clamp(sectors, 3.0, 16.0) + 0.5);
    let phase = time * m.p6.y * 0.6;           // wave phase rides the churn rate
    let drift = time * m.p2.z * 0.04;          // slow rotation off the spin
    var s = 0.0;
    var grad = vec2<f32>(0.0);
    for (var k = 0; k < n; k = k + 1) {
        let ang = PI * f32(k) / f32(n) + drift;
        let dir = vec2<f32>(cos(ang), sin(ang));
        let ph = dot(p, dir) * freq + phase;
        s = s + cos(ph);
        grad = grad - sin(ph) * dir;           // ∇s — the local flow of the field
    }
    let nf = f32(n);
    let v = s / nf;                            // ~[-1,1]
    let line = mix(0.06, 0.012, clamp(m.p5.y, 0.0, 1.0));
    var o: Qc;
    o.web = pow(max(v, 0.0), 3.0) * 3.0 + line / (abs(v) + line);
    o.warp = grad / nf;
    return o;
}

// ---------- GROUP 1, ENGINE: hyperbolic (Poincaré disk) ----------
// Fold a disk point back into a fundamental domain by p-fold mirrors + repeated
// inversion across a geodesic circle (orthogonal to the unit circle) → Escher
// "Circle Limit": infinite self-similar detail crowding into the boundary.
// `sectors` = p (centre symmetry); `fold_c` = curvature (geodesic depth = tile
// size); `iterations` = recursion depth. Returns the folded coordinate (so the
// fractal pattern can tessellate each tile) AND the tile-edge glow.
struct Hf { coord: vec2<f32>, edge: f32 };
fn hyperbolic_fold(z_in: vec2<f32>, sectors: f32) -> Hf {
    let iters = i32(m.p1.z) + 4;
    let curv = clamp(m.p1.x, 0.05, 1.5);
    let line = mix(0.05, 0.008, clamp(m.p5.y, 0.0, 1.0));
    let p2 = max(sectors, 2.0);
    let kfold = TAU / p2;
    let d = 1.0 + curv;
    let cr2 = d * d - 1.0;
    let cc = vec2<f32>(d, 0.0);

    var z = z_in;
    var glow = 0.0;
    for (var i = 0; i < iters; i = i + 1) {
        let ang0 = atan2(z.y, z.x);
        let rad0 = length(z);
        var a = ang0 - floor(ang0 / kfold) * kfold;
        a = abs(a - 0.5 * kfold);
        z = vec2<f32>(cos(a), sin(a)) * rad0;
        glow = glow + 0.5 * line / (abs(z.y) + line);        // wedge mirror seam
        let dz = z - cc;
        let dd = dot(dz, dz);
        if (dd < cr2) {
            glow = glow + line / (abs(sqrt(dd) - sqrt(cr2)) + line); // geodesic edge
            z = cc + dz * (cr2 / dd);
        }
    }
    var o: Hf;
    o.coord = z;
    o.edge = glow;
    return o;
}

// Complex division a/b (a, b as vec2 = x + iy).
fn cdiv(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let dd = dot(b, b) + 1e-9;
    return vec2<f32>(a.x * b.x + a.y * b.y, a.y * b.x - a.x * b.y) / dd;
}

// ---------- the emission field for ONE symmetry count ----------
// Sector-dependent glow, reading the rest of the look from `m`. The space selector
// (p6.x) picks the geometry the field lives on — Euclidean (flat/tunnel), the
// hyperbolic disk, or the aperiodic quasicrystal — and in EVERY space the chosen
// fractal-engine `pattern` is drawn on the resulting coordinate, and `tunnel`
// re-projects it onto a receding bore. Factored out so the fragment can crossfade
// two sector counts (symmetry morph) and sample at offset scales per channel
// (chromatic dispersion) — every space inherits both for free.
fn compute_glow(p: vec2<f32>, sectors: f32) -> f32 {
    let time = m.p0.y;
    let rt = time * m.p2.z;
    let fold_c = m.p1.x;
    let iter_rot = m.p1.y;
    let iters = i32(m.p1.z);
    let tunnel = m.p1.w >= 0.5;
    let ring_gain = m.p2.x;
    let ray_count = m.p2.y;
    let pattern = i32(m.p3.z + 0.5);
    let flow = m.p4.z;
    let petals = max(m.p4.w, 1.0);
    let line = mix(0.02, 0.0035, clamp(m.p5.y, 0.0, 1.0));
    let secs = max(sectors, 1.0);
    let space = i32(m.p6.x + 0.5);
    // `et` = the intrinsic self-animation clock: the rate the fractal churns/pulses
    // on its own (the per-iteration IFS rotation, ring/ray breathing), separate from
    // spin (whole-field rotation), colour speed, and tunnel flow. `churn = 0`
    // freezes the field; the pattern only moves under spin/flow then.
    let churn = m.p6.y;
    let et = time * churn;

    // ---- QUASICRYSTAL: the interference web + the fractal riding its warp. ----
    if (space == 2) {
        var pc = p;
        var shade = 1.0;
        if (tunnel) {
            // Stream the quasicrystal down a bore (angle across, 1/r depth scroll).
            let r = length(p) + 1e-3;
            let a = atan2(p.y, p.x);
            pc = vec2<f32>(a * secs * 0.3, (1.0 / r + time * flow) * 0.6);
            shade = mix(0.12, 1.5, smoothstep(0.0, 0.55, r));
        }
        let qc = quasicrystal_eval(pc, secs);
        // The pattern shows through, distorted aperiodically by the quasicrystal flow.
        let q = rot2(rt) * (pc + qc.warp * 0.4);
        let g = qc.web + 0.7 * fractal_glow(q, iters, pattern, fold_c, iter_rot, et, petals, line);
        return g * shade;
    }

    // ---- HYPERBOLIC: tessellate the fractal pattern across the tiling. ----
    if (space == 1) {
        var z = rot2(time * m.p2.z * 0.15) * p * 0.5;
        var shade = 1.0;
        if (tunnel) {
            // Oscillating hyperbolic boost (a real Möbius translation) → fly through
            // the tiling; fog adds depth toward the bore walls.
            let bx = clamp(0.7 * sin(time * 0.2 * (1.0 + flow * 60.0)), -0.92, 0.92);
            z = cdiv(z + vec2<f32>(bx, 0.0), vec2<f32>(1.0 + bx * z.x, bx * z.y));
            shade = mix(0.3, 1.4, smoothstep(0.0, 0.7, length(p)));
        }
        let rr = dot(z, z);
        if (rr >= 0.999) { z = z * (0.999 / sqrt(rr)); }
        let h = hyperbolic_fold(z, secs);
        let g = h.edge
            + 0.6 * fractal_glow(h.coord * 1.6, iters, pattern, fold_c, iter_rot, et, petals, line);
        return g * smoothstep(1.0, 0.5, length(z)) * shade;
    }

    // ---- WEYL: crystallographic mirror arrangement (rank-2 root hyperplanes). ----
    // N line-families at πk/N reproduce the affine Weyl / wallpaper tiling: N=2 →
    // square (A₁×A₁), N=3 → triangular (A₂), N=6 → hexagonal (G₂). The fractal rides
    // the rotated lattice; the lines are the tiling.
    if (space == 3) {
        var pc = p;
        var shade = 1.0;
        if (tunnel) {
            let r = length(p) + 1e-3;
            let a = atan2(p.y, p.x);
            pc = vec2<f32>(a * secs * 0.3, (1.0 / r + time * flow) * 0.6);
            shade = mix(0.12, 1.5, smoothstep(0.0, 0.55, r));
        }
        let n = clamp(secs, 2.0, 8.0);
        let freq = 2.0 + fold_c * 5.0;
        var edge = 0.0;
        for (var k = 0; k < i32(n + 0.5); k = k + 1) {
            let ang = PI * f32(k) / n;
            let d = dot(pc, vec2<f32>(cos(ang), sin(ang))) * freq + et * 0.1;
            edge = edge + line / (abs(fract(d) - 0.5) + line);
        }
        edge = edge * (0.4 + ring_gain * 0.4) / n;
        let q = rot2(rt) * pc;
        let g = edge + 0.7 * fractal_glow(q, iters, pattern, fold_c, iter_rot, et, petals, line);
        return g * shade;
    }

    // ---- E8: the Petrie projection — 30-fold Coxeter symmetry, 8 rings of 30 roots. ----
    // Fold into one 30-fold sector, then emit each ring's vertex (1 per sector) as a
    // glowing root + a faint concentric circle. Ring (radius, phase) come from the
    // CPU Coxeter-plane projection (math::e8_petrie_rings — unit-tested 8×30).
    // A rigid 30-fold-symmetric figure looks static under whole-field spin (rotating
    // it by 2π/30 maps it onto itself), so the flow here is DIFFERENTIAL — the 8
    // shells counter-rotate and breathe, and a radial wave ripples through them —
    // all driven by `churn`, which a rigid symmetry can't hide.
    if (space == 4) {
        var pc = p;
        var shade = 1.0;
        if (tunnel) {
            let r0 = length(p) + 1e-3;
            let a0 = atan2(p.y, p.x);
            pc = vec2<f32>(cos(a0), sin(a0)) * (1.0 / r0) * 0.6;
            shade = mix(0.2, 1.4, smoothstep(0.0, 0.55, r0));
        }
        let z = rot2(rt) * pc;
        let bigR = 0.9;
        let dotw = mix(0.03, 0.008, clamp(m.p5.y, 0.0, 1.0));

        // 8-D plane rotation: the CPU tumbles the viewing 2-plane through 8-space and
        // re-projects all 240 roots each frame (p6.z = flow rate). Splat them — the
        // rings split, merge and flow as the shadow of the E8 polytope turns. Heavier
        // (240 points/pixel), so only when engaged; otherwise the cheap 8-ring fold.
        if (m.p6.z > 0.001) {
            var glow = 0.0;
            for (var i = 0; i < 120; i = i + 1) {
                let q = m.e8pts[i];
                let d1 = z - q.xy * bigR;
                let d2 = z - q.zw * bigR;
                glow = glow + dotw * 1.5 / (dot(d1, d1) + dotw * dotw)
                            + dotw * 1.5 / (dot(d2, d2) + dotw * dotw);
            }
            let g = glow + 0.2 * fractal_glow(z * 2.0, iters, pattern, fold_c, iter_rot, et, petals, line);
            return g * shade;
        }

        let r = length(z);
        let a = atan2(z.y, z.x);
        let sect = TAU / 30.0;
        let af = a - floor(a / sect) * sect; // angle within one 30-fold sector
        var glow = 0.0;
        for (var k = 0; k < 8; k = k + 1) {
            let pair = m.e8[k / 2];
            var rr0 = pair.x;
            var ph = pair.y;
            if ((k & 1) == 1) { rr0 = pair.z; ph = pair.w; }
            // Counter-rotating shells: inner rings wind one way, outer the other, so
            // the roots shear past each other (and travel around their ring).
            ph = ph + et * 0.12 * (f32(k) - 3.5);
            // Per-ring radial breathing.
            let rr = rr0 * bigR * (1.0 + 0.05 * sin(et * 0.6 + f32(k) * 1.3));
            // Distance to the nearest 30-fold image of the (now moving) vertex.
            var da = af - ph;
            da = da - sect * round(da / sect);
            let dd = r * r + rr * rr - 2.0 * r * rr * cos(da);
            // A radial energy wave pulses brightness through the shells.
            let pulse = 0.7 + 0.5 * sin(et * 1.5 - rr * 7.0);
            glow = glow + dotw * 1.5 * pulse / (dd + dotw * dotw);   // glowing root vertex
            glow = glow + ring_gain * 0.010 / (abs(r - rr) + 0.012); // concentric ring
        }
        // A touch of the fractal engine for surface texture inside the field.
        let g = glow + 0.25 * fractal_glow(z * 2.0, iters, pattern, fold_c, iter_rot, et, petals, line);
        return g * shade;
    }

    // ---- APOLLONIAN: Kleinian circle-inversion gasket (Indra's Pearls). ----
    // Repeated fold-into-cell + circle inversion (a Schottky/Apollonian IFS) packs
    // space with nested tangent circles. `fold_c` is the inversion constant (the
    // packing's character); the fractal engine textures the cells.
    if (space == 6) {
        var pc = p;
        var shade = 1.0;
        if (tunnel) {
            let r0 = length(p) + 1e-3;
            let a0 = atan2(p.y, p.x);
            pc = vec2<f32>(a0 * secs * 0.3, (1.0 / r0 + time * flow) * 0.6);
            shade = mix(0.12, 1.5, smoothstep(0.0, 0.55, r0));
        }
        var z = rot2(rt) * pc * 1.2;
        var s = 1.0;
        let cst = 1.0 + fold_c * 0.5;
        let n = i32(m.p1.z) + 4;
        var glow = 0.0;
        for (var i = 0; i < n; i = i + 1) {
            z = 2.0 * fract(0.5 * z + 0.5) - vec2<f32>(1.0); // mirror-fold into the cell
            let r2 = max(dot(z, z), 0.04);
            let k = cst / r2;                                // circle inversion + scale
            z = z * k;
            s = s * k;
            // Emit on the inversion circle, finer (and dimmer) at deeper levels.
            glow = glow + line / ((abs(length(z) - 1.0) + line) * (1.0 + s * 0.15));
        }
        let g = glow * (0.6 + ring_gain * 0.6)
            + 0.4 * fractal_glow(z, iters, pattern, fold_c, iter_rot, et, petals, line);
        return g * shade;
    }

    // ---- MODULAR: the SL(2,ℤ) tessellation of the hyperbolic plane. ----
    // Cayley-map the disk to the upper half-plane, reduce by T:z→z+1 and S:z→−1/z to
    // the modular fundamental domain, emitting on the tessellation edges → the
    // Dedekind tessellation crowding into the boundary (a number-theoretic Circle-Limit).
    if (space == 7) {
        var pc = p;
        var shade = 1.0;
        if (tunnel) {
            let bx = clamp(0.7 * sin(time * 0.2 * (1.0 + flow * 60.0)), -0.92, 0.92);
            pc = cdiv(pc + vec2<f32>(bx, 0.0), vec2<f32>(1.0 + bx * pc.x, bx * pc.y));
        }
        var z = rot2(rt) * pc;
        let rr = dot(z, z);
        if (rr >= 0.98) { z = z * (0.98 / sqrt(rr)); }
        // Cayley: w = i·(1+z)/(1−z) maps the disk to the upper half-plane.
        let q = cdiv(vec2<f32>(1.0 + z.x, z.y), vec2<f32>(1.0 - z.x, -z.y));
        var w = vec2<f32>(-q.y, q.x);
        let n = i32(m.p1.z) + 6;
        var edge = 0.0;
        for (var i = 0; i < n; i = i + 1) {
            w.x = w.x - round(w.x);                       // T: real part → [−½, ½]
            edge = edge + line / (abs(abs(w.x) - 0.5) + line); // the vertical walls
            let r2 = dot(w, w);
            if (r2 < 1.0) {
                w = -w / r2;                              // S: w → −1/w
                edge = edge + line / (abs(sqrt(r2) - 1.0) + line); // the unit-circle wall
            }
        }
        let g = edge * (0.5 + ring_gain * 0.5)
            + 0.4 * fractal_glow(w, iters, pattern, fold_c, iter_rot, et, petals, line);
        return g * smoothstep(1.0, 0.5, length(z)) * shade;
    }

    // ---- EUCLIDEAN (flat / tunnel). ----
    var glow: f32;
    if (tunnel) {
        // Tunnel: a bore receding to a vanishing point at the centre.
        let r = length(p) + 1e-3;
        let a = atan2(p.y, p.x);
        let depth = 1.0 / r + time * flow;
        let k = TAU / secs;
        let aw = abs(a - floor(a / k) * k - 0.5 * k);
        let q = rot2(rt * 0.3) * vec2<f32>(aw * secs * 0.5, depth);
        glow = fractal_glow(q, iters, pattern, fold_c, iter_rot, et, petals, line);
        let rings = 0.5 + 0.5 * sin(depth * TAU - time * flow * TAU);
        glow = glow * (0.55 + 0.9 * rings) * (0.6 + ring_gain * 0.4);
        glow = glow + pow(abs(cos(aw * ray_count)), 14.0) * 0.5;
        let fog = smoothstep(0.0, 0.55, r);
        glow = glow * mix(0.12, 1.5, fog);
    } else {
        // Flat: rose-window kaleidoscope.
        var q = rot2(rt) * p;
        let ang = atan2(q.y, q.x);
        let rad = length(q);
        let k = TAU / secs;
        var a2 = ang - floor(ang / k) * k; // GLSL-style mod, always in [0,k)
        a2 = abs(a2 - PI / secs);
        q = vec2<f32>(cos(a2), sin(a2)) * rad;
        glow = fractal_glow(q, iters, pattern, fold_c, iter_rot, et, petals, line);
        let r1 = sd_circle(p, 0.35 + 0.03 * sin(et));
        let r2 = sd_circle(p, 0.70 + 0.04 * cos(et * 0.8));
        let r3 = sd_circle(p, 1.05);
        glow = glow + ring_gain *
            (0.025 / (r1 + 0.01) + 0.020 / (r2 + 0.01) + 0.015 / (r3 + 0.01));
        let a = atan2(p.y, p.x);
        let rays = pow(abs(cos(a * ray_count + et)), 18.0);
        glow = glow + rays * 0.25 / (1.0 + length(p) * 8.0);
    }
    return glow;
}

// ---------- GROUP 4, CONFORMAL: Weierstrass ℘ (doubly-periodic / torus) ----------
// The elliptic function ℘(z) ≈ Σ 1/(z−w)² over the lattice has a DOUBLE pole at every
// lattice point — its argument winds (charge +2) around each, doubly-periodically, so
// the field lives on a torus (ℂ/lattice). Colour by arg ℘ through the palette (the
// periodic phase), bright poles. `fold_c` = lattice density; `churn` shears the
// lattice modulus τ, deforming the torus. (Coloured like the gauge phase fields.)
fn elliptic_color(p: vec2<f32>) -> vec3<f32> {
    let time = m.p0.y;
    let et = time * m.p6.y;
    let rt = time * m.p2.z;
    let scale = 1.5 + m.p1.x * 3.0;
    let tau = vec2<f32>(0.3 * sin(et * 0.2), 1.0); // lattice basis 2 (basis 1 = (1,0))
    let z = rot2(rt) * p * scale;
    var pp = vec2<f32>(0.0);
    for (var a = -2; a <= 2; a = a + 1) {
        for (var b = -2; b <= 2; b = b + 1) {
            let w = vec2<f32>(f32(a), 0.0) + f32(b) * tau;
            let d = z - w;
            let r2 = dot(d, d) + 1e-4;
            let inv = vec2<f32>(d.x, -d.y) / r2;                      // 1/d  (complex)
            pp = pp + vec2<f32>(inv.x * inv.x - inv.y * inv.y, 2.0 * inv.x * inv.y); // +(1/d)²
        }
    }
    let ph = atan2(pp.y, pp.x);
    let mag = length(pp);
    let ct = ph / TAU + m.p3.x - time * m.p4.x;
    let col = palette(i32(m.p3.w + 0.5), ct);
    let sharp = clamp(m.p5.y, 0.0, 1.0);
    let lines = pow(abs(sin(log(mag + 1e-3) * 3.0)), mix(1.0, 6.0, sharp)); // |℘| contours
    let bright = 0.35 + 1.4 * lines + 0.04 * mag; // contours + glowing poles
    return col * pow(max(bright, 0.0), max(m.p5.x, 0.05));
}

// ---------- GROUP 3, GAUGE FIELD: U(1) vortices ----------
// A complex order parameter ψ = |ψ|·e^{iθ} carrying topological defects. Each vortex
// of charge ±1 contributes θ += ±atan2(p−c); the phase WINDS around it (π₁(S¹)=ℤ),
// |ψ|→0 at the core (a dark hole), and the defects orbit on `churn`. The phase is
// the colour (mapped through the palette — Aharonov–Bohm winding made visible);
// equiphase contour ridges glow. `sectors` = vortex count, `fold_c` = core size.
fn vortex_color(p: vec2<f32>) -> vec3<f32> {
    let time = m.p0.y;
    let et = time * m.p6.y;            // churn → orbital churn of the defects
    let spin = m.p2.z;                 // whole-constellation rotation
    let flowrate = m.p4.z * 200.0;     // tunnel-flow slot → density-wave speed
    let ring_gain = m.p2.x;            // structure (ridge + wave) intensity
    let nv = i32(clamp(m.p0.w, 1.0, 8.0) + 0.5);
    let core = 0.04 + m.p1.x * 0.08;

    // The whole vortex constellation rotates with spin.
    let pr = rot2(time * spin * 0.2) * p;

    var theta = 0.0;
    var amp = 1.0;
    var dmin = 1e9;
    for (var k = 0; k < nv; k = k + 1) {
        let kk = f32(k);
        let charge = select(1.0, -1.0, (k & 1) == 1);
        let r0 = 0.22 + 0.5 * kk / f32(nv);
        // DIFFERENTIAL orbit: each vortex circles at its own rate, so the defects
        // drift relative to each other (pair encounters) rather than co-rotating
        // rigidly — the phase field genuinely churns.
        let a0 = TAU * kk / f32(nv) + et * (0.2 + 0.18 * kk);
        let c = vec2<f32>(r0 * cos(a0), r0 * sin(a0));
        let d = pr - c;
        let dist = length(d);
        theta = theta + charge * atan2(d.y, d.x);
        amp = amp * smoothstep(0.0, core * 4.0, dist); // dark vortex core
        dmin = min(dmin, dist);
    }
    let sharp = clamp(m.p5.y, 0.0, 1.0);
    // Equiphase ridges that SPIRAL around the cores (the supercurrent winding).
    let ridges = pow(abs(sin(theta * 3.0 - et * 1.5)), mix(1.0, 8.0, sharp));
    // Density waves rippling OUTWARD from the nearest core — the flow you can dial
    // with `tunnel flow` (0 = still). Concentric, advecting in time.
    let waves = 0.65 + 0.35 * sin(dmin * 22.0 - time * flowrate);

    let ct = theta / TAU + m.p3.x - time * m.p4.x; // palette of the PHASE (winding)
    let col = palette(i32(m.p3.w + 0.5), ct);
    let bright = amp * waves * (0.4 + (1.0 + ring_gain) * 1.2 * ridges);
    return col * pow(bright, max(m.p5.x, 0.05));
}

// ---------- the full field colour at one screen coordinate ----------
// Continuous symmetry MORPH: crossfade the two nearest integer foldings so the
// kaleidoscope breathes between fold-counts without a seam. Then shape contrast,
// invert (figure↔ground), and colour through the selected palette. The U(1)-vortex
// gauge space (5) colours by the winding phase instead.
fn field_color(p: vec2<f32>) -> vec3<f32> {
    let space = i32(m.p6.x + 0.5);
    if (space == 5) { return vortex_color(p); }
    if (space == 8) { return elliptic_color(p); }
    let time = m.p0.y;
    let sectors = max(m.p0.w, 2.0);
    let lo = floor(sectors);
    let f = sectors - lo;
    var glow = compute_glow(p, lo);
    if (f > 0.002) {
        glow = mix(glow, compute_glow(p, lo + 1.0), f);
    }

    // Contrast shaping on the (HDR) emission — >1 punches highlights, <1 lifts mids.
    glow = pow(max(glow, 0.0), max(m.p5.x, 0.05));

    // Invert: blend toward a bright field with the structure carved out as dark bands.
    let invert = clamp(m.p5.z, 0.0, 1.0);
    let mask = 1.0 - exp(-glow * 1.5);
    let intensity = mix(glow, (1.0 - mask) * 2.2, invert);

    // Colour: selectable palette cycled at its own rate, `hue` a static offset.
    let ct = length(p) * 0.4 + m.p3.x - time * m.p4.x;
    return palette(i32(m.p3.w + 0.5), ct) * intensity;
}

// ===================== 3-D RELIEF (lit heightfield raymarch) =====================
// IDEA 1 of the KIFS 3-D program: every space already paints a per-pixel scalar/
// phase field; read that field's BRIGHTNESS as a height and raymarch it as a lit,
// glowing relief — "the field becomes carved jewel". One mechanism, all nine spaces,
// zero per-space code: the medallion you see flat is now embossed by its own light,
// orbited by an auto-camera (azimuth rides `spin`) and still churning on `churn`.
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }

// Scalar brightness of a space at a field coordinate `q` (already zoom-scaled).
// Scalar spaces use the raw glow (no symmetry morph — cheaper, height needs no
// crossfade); the phase spaces (U(1) vortices, ℘) use the luminance of their colour.
fn relief_scalar(q: vec2<f32>, sectors: f32) -> f32 {
    let space = i32(m.p6.x + 0.5);
    if (space == 5) { return luma(vortex_color(q)); }
    if (space == 8) { return luma(elliptic_color(q)); }
    return compute_glow(q, floor(max(sectors, 2.0)));
}

// World→field map shared by the height sampler and the hit colour (so they agree).
fn relief_field_coord(world_xy: vec2<f32>) -> vec2<f32> {
    return world_xy * m.p0.z * 0.6; // `zoom` sets how much pattern fits in the disk
}

// Compressed surface height (world units). Bounded by `1 − e^(−1.5·g)` so the march
// is stable where the field spikes near motif cores. The field covers the WHOLE
// plane (no disk cut) so the relief fills the frame — dark regions sit at the flat
// base (height 0), bright regions rise, exactly mirroring the flat field's coverage.
fn relief_height(world_xy: vec2<f32>, sectors: f32, hscale: f32) -> f32 {
    let g = relief_scalar(relief_field_coord(world_xy), sectors);
    return hscale * (1.0 - exp(-max(g, 0.0) * 1.5)); // [0, hscale)
}

// Returns rgb + alpha (alpha 0 on a miss → transparent over the shared background).
fn render_relief(uv_ndc: vec2<f32>) -> vec4<f32> {
    let aspect = m.p0.x;
    let time = m.p0.y;
    let sectors = max(m.p0.w, 2.0);
    let hscale = m.p7.x;
    let elev = clamp(m.p7.y, 0.05, 1.45);
    let steps = i32(clamp(m.p7.z, 16.0, 360.0));
    let shine = max(m.p7.w, 0.0);

    // Auto-orbit camera: azimuth rides `spin` (ties to the beat/spin stack), elevation
    // is the dialled tilt. Z is up; the field lives on the z = h(x,y) surface. The
    // default elevation is near top-down so the relief FILLS the frame; lowering it
    // tilts to an oblique view (the far plane then fades into the background, below).
    let az = time * m.p2.z * 0.2;
    let cz = cos(elev);
    let sz = sin(elev);
    let ctr = vec3<f32>(0.0, 0.0, hscale * 0.3);
    let ro = ctr + 2.1 * vec3<f32>(cos(az) * cz, sin(az) * cz, sz);
    let fwd = normalize(ctr - ro);
    let right = normalize(cross(fwd, vec3<f32>(0.0, 0.0, 1.0)));
    let upv = cross(right, fwd);
    var sc = uv_ndc;
    sc.x = sc.x * aspect;
    let rd = normalize(fwd * 1.7 + right * sc.x + upv * sc.y);

    // March the full-plane heightfield (not a true SDF → conservative fractional
    // steps; the step grows with distance so grazing rays still terminate cheaply).
    let tmax = 12.0;
    var t = 0.02;
    var hit = false;
    var hitp = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        let pos = ro + rd * t;
        if (t > tmax) { break; }
        let d = pos.z - relief_height(pos.xy, sectors, hscale);
        if (d < 0.0015) { hit = true; hitp = pos; break; }
        t = t + max(d * 0.4, 0.008 + t * 0.01);
    }
    if (!hit) { return vec4<f32>(0.0); } // sky (above the horizon) → background

    // Surface normal from the height gradient: N ∝ (−∂h/∂x, −∂h/∂y, 1).
    let e = 0.02;
    let hx = relief_height(hitp.xy + vec2<f32>(e, 0.0), sectors, hscale)
           - relief_height(hitp.xy - vec2<f32>(e, 0.0), sectors, hscale);
    let hy = relief_height(hitp.xy + vec2<f32>(0.0, e), sectors, hscale)
           - relief_height(hitp.xy - vec2<f32>(0.0, e), sectors, hscale);
    let n = normalize(vec3<f32>(-hx / (2.0 * e), -hy / (2.0 * e), 1.0));

    // Colour from the FULL field (morph + palette + contrast/invert) at the hit, lit
    // on top of its own emission so peaks still bloom through the shared composite.
    let base = field_color(relief_field_coord(hitp.xy));
    let lk = normalize(vec3<f32>(cos(az + 1.2), sin(az + 1.2), 0.85));
    let diff = max(dot(n, lk), 0.0);
    let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    let peak = clamp(hitp.z / max(hscale, 1e-3), 0.0, 1.0);
    var col = base * (0.45 + 0.8 * diff) + base * rim * shine + base * peak * 0.6;
    col = col * mix(1.0, 0.35, smoothstep(2.0, tmax, t)); // depth fog
    // Distance alpha fade: when tilted oblique, the far plane dissolves into the
    // shared background instead of a hard flat plain — keeps the near, fully-relieved
    // content the focus. (Near top-down nothing reaches this range → fully opaque.)
    let afade = 1.0 - smoothstep(tmax * 0.5, tmax * 0.92, t);
    return vec4<f32>(col * m.p2.w, afade); // × glow_gain
}

// ===================== 3-D CONFORMAL (sphere-inversion solid) =====================
// IDEA 3, the conformal cluster: the four conformal spaces (Euclidean / Hyperbolic /
// Modular / Apollonian) live naturally in ℝ³, where the 2-D circle inversion that
// defines them becomes a SPHERE inversion (ℂ→ℝ³ for free). This raymarches a
// distance-estimated Apollonian/Kleinian solid — the honest 3-D form of those spaces —
// instead of faking depth from a flat field. `fold (c)`, `iterations`, `sectors`,
// `churn`, `palette`/`hue`/`colour speed` and the shared 3-D camera all carry over.
struct DeResult { d: f32, trap: f32, orb: vec3<f32> };

// Distance estimate to the conformal solid at world point `p0`, with an orbit trap
// (`trap` = closest the orbit came to the origin; `orb` = a colour accumulator).
fn conformal_de(p0: vec3<f32>) -> DeResult {
    let space = i32(m.p6.x + 0.5);
    let iters = i32(m.p1.z) + 4;          // reuse `iterations` (+a few for solid detail)
    let foldc = m.p1.x;                    // `fold (c)` → inversion scale / packing character
    let secs = max(m.p0.w, 2.0);
    let et = m.p0.y * m.p6.y;              // churn breathes the packing

    var p = p0;

    // EUCLIDEAN: the dihedral kaleidoscope lifted to 3-D — fold the azimuth into one
    // 2π/N wedge about the vertical axis → an N-fold rosette of inverted spheres.
    if (space == 0) {
        let a = atan2(p.z, p.x);
        let k = TAU / secs;
        var aw = a - floor(a / k) * k;
        aw = abs(aw - 0.5 * k);
        let r = length(p.xz);
        p = vec3<f32>(cos(aw) * r, p.y, sin(aw) * r);
    }

    // Sphere-inversion IFS (circle inversion → sphere inversion). The per-iteration
    // cell offset biases the limit set: Apollonian = the classic tangent-sphere
    // packing; Hyperbolic / Modular crowd toward a boundary (a small shear/offset).
    let s = 1.05 + foldc * 0.45 + 0.03 * sin(et);
    var off = vec3<f32>(0.0);
    if (space == 1) { off = vec3<f32>(0.20, 0.0, 0.0); }   // Hyperbolic
    if (space == 7) { off = vec3<f32>(0.0, 0.18, 0.12); }  // Modular (T+S-like shear)

    var scale = 1.0;
    var trap = 1e9;
    var orb = vec3<f32>(0.0);
    for (var i = 0; i < iters; i = i + 1) {
        p = -1.0 + 2.0 * fract(0.5 * p + 0.5); // fold into the [-1,1]³ cell
        p = p + off;
        let r2 = dot(p, p);
        trap = min(trap, r2);
        orb = max(orb, abs(p) * (1.0 - smoothstep(0.0, 1.5, r2)));
        let k = s / max(r2, 0.06);             // sphere inversion + rescale
        p = p * k;
        scale = scale * k;
    }
    var o: DeResult;
    o.d = 0.25 * abs(p.y) / scale;             // Quilez-style apollonian bound (thin trap)
    o.trap = trap;
    o.orb = orb;
    return o;
}

fn render_conformal(uv_ndc: vec2<f32>) -> vec4<f32> {
    let aspect = m.p0.x;
    let time = m.p0.y;
    let elev = clamp(m.p7.y, 0.05, 1.45);
    let steps = i32(clamp(m.p7.z, 24.0, 400.0));
    let shine = max(m.p7.w, 0.0);

    // Orbit camera around the solid (azimuth rides `spin`; `zoom` dollies in/out).
    let az = time * m.p2.z * 0.2;
    let cz = cos(elev);
    let sz = sin(elev);
    let dist = 3.4 / max(m.p0.z, 0.3);
    let ro = vec3<f32>(cos(az) * cz, sz, sin(az) * cz) * dist;
    let fwd = normalize(-ro);
    let right = normalize(cross(fwd, vec3<f32>(0.0, 1.0, 0.0)));
    let upv = cross(right, fwd);
    var sc = uv_ndc;
    sc.x = sc.x * aspect;
    let rd = normalize(fwd * 1.6 + right * sc.x + upv * sc.y);

    var t = 0.1;
    var hit = false;
    var res: DeResult;
    var hp = vec3<f32>(0.0);
    for (var i = 0; i < steps; i = i + 1) {
        hp = ro + rd * t;
        res = conformal_de(hp);
        if (res.d < 0.0008 * t) { hit = true; break; }
        t = t + res.d * 0.7;
        if (t > 12.0) { break; }
    }
    if (!hit) { return vec4<f32>(0.0); } // background shows through

    // Normal from the DE gradient.
    let e = vec2<f32>(0.0009 * t, 0.0);
    let n = normalize(vec3<f32>(
        conformal_de(hp + e.xyy).d - conformal_de(hp - e.xyy).d,
        conformal_de(hp + e.yxy).d - conformal_de(hp - e.yxy).d,
        conformal_de(hp + e.yyx).d - conformal_de(hp - e.yyx).d,
    ));

    // Colour from the orbit trap through the shared palette (cycled like the field).
    let tc = sqrt(clamp(res.trap, 0.0, 1.0));
    let ct = tc + m.p3.x - time * m.p4.x;
    let base = palette(i32(m.p3.w + 0.5), ct) * (0.35 + 1.6 * length(res.orb));
    let lk = normalize(vec3<f32>(cos(az + 1.2), 0.8, sin(az + 1.2)));
    let diff = max(dot(n, lk), 0.0);
    let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    var col = base * (0.35 + 0.85 * diff) + base * rim * shine;
    col = col * mix(1.0, 0.3, smoothstep(3.0, 11.0, t)); // depth fog
    let afade = 1.0 - smoothstep(8.0, 11.5, t);
    return vec4<f32>(col * m.p2.w, afade);                // × glow_gain
}

// ===================== 3-D FILAMENTS (phase-defect volumetric) =====================
// IDEA 3, the field cluster: U(1) vortices and Elliptic ℘ are PHASE fields whose
// defects are points in 2-D but LINES in 3-D. A U(1) vortex is a filament (the core
// where |ψ|→0 traces a curve — a superfluid vortex line); ℘'s doubly-periodic poles
// extrude into a lattice of phase lines. This volumetrically raymarches glowing,
// phase-coloured filaments through a bounded volume — the honest 3-D of both spaces.
// `sectors`/`p0.w` = vortex count, `fold (c)` = core thickness, `churn` weaves the
// tangle, `spin` orbits the camera, `zoom` dollies, `palette`/`hue`/`colour speed`
// colour the winding, `3D shine` boosts the soft sheath glow.

// Emission (rgb) + density (a) of the phase field at a world point.
fn field3d_sample(p3: vec3<f32>) -> vec4<f32> {
    let space = i32(m.p6.x + 0.5);
    let time = m.p0.y;
    let et = time * m.p6.y;
    let pal = i32(m.p3.w + 0.5);
    let ct0 = m.p3.x - time * m.p4.x;          // palette phase offset + cycle
    let core = 0.05 + m.p1.x * 0.10;           // `fold (c)` → filament thickness
    let halo = max(m.p7.w, 0.0);               // `3D shine` → soft sheath glow

    var theta = 0.0;
    var dmin = 1e9;

    if (space == 8) {
        // ℘: a lattice of vertical phase lines (the doubly-periodic poles extruded),
        // gently twisted with height. Phase = arg ℘ via the lattice sum (charge +2).
        let scale = 1.5 + m.p1.x * 2.0;
        let tau = vec2<f32>(0.3 * sin(et * 0.2), 1.0);
        let zc = rot2(p3.y * 0.25 + time * m.p2.z * 0.2) * (p3.xz * scale);
        var pp = vec2<f32>(0.0);
        for (var a = -2; a <= 2; a = a + 1) {
            for (var b = -2; b <= 2; b = b + 1) {
                let w = vec2<f32>(f32(a), 0.0) + f32(b) * tau;
                let d = zc - w;
                let r2 = dot(d, d) + 1e-4;
                let inv = vec2<f32>(d.x, -d.y) / r2;
                pp = pp + vec2<f32>(inv.x * inv.x - inv.y * inv.y, 2.0 * inv.x * inv.y);
                dmin = min(dmin, length(d) / scale); // back to world units
            }
        }
        theta = atan2(pp.y, pp.x);
    } else {
        // U(1) vortices: each defect is a weaving vertical filament (a 3-D tangle).
        let nv = i32(clamp(m.p0.w, 1.0, 8.0) + 0.5);
        let pr = rot2(time * m.p2.z * 0.2) * p3.xz; // constellation rotates with spin
        for (var k = 0; k < nv; k = k + 1) {
            let kk = f32(k);
            let charge = select(1.0, -1.0, (k & 1) == 1);
            let r0 = 0.22 + 0.5 * kk / f32(nv);
            let a0 = TAU * kk / f32(nv) + et * (0.2 + 0.18 * kk);
            // The filament weaves with height → the cores cross into a churning tangle.
            let cx = r0 * cos(a0) + 0.18 * sin(p3.y * 1.6 + a0 + et);
            let cz = r0 * sin(a0) + 0.18 * cos(p3.y * 1.5 - a0 - et);
            let d = pr - vec2<f32>(cx, cz);
            let dist = length(d);
            theta = theta + charge * atan2(d.y, d.x);
            dmin = min(dmin, dist);
        }
    }

    // Bright thin core + soft halo sheath; colour by the winding phase.
    let tube = core * core / (dmin * dmin + core * core);
    let sheath = (core * 3.0) * (core * 3.0) / (dmin * dmin + (core * 3.0) * (core * 3.0));
    let col = palette(pal, theta / TAU + ct0);
    let emit = col * (tube + 0.35 * halo * sheath);
    let dens = tube + 0.12 * sheath;
    return vec4<f32>(emit, dens);
}

fn render_filaments(uv_ndc: vec2<f32>) -> vec4<f32> {
    let aspect = m.p0.x;
    let time = m.p0.y;
    let elev = clamp(m.p7.y, 0.05, 1.45);
    let nsteps = i32(clamp(m.p7.z, 24.0, 400.0));

    // Orbit camera around the volume (azimuth rides `spin`; `zoom` dollies).
    let az = time * m.p2.z * 0.2;
    let cz = cos(elev);
    let sz = sin(elev);
    let dist = 3.4 / max(m.p0.z, 0.3);
    let ro = vec3<f32>(cos(az) * cz, sz, sin(az) * cz) * dist;
    let fwd = normalize(-ro);
    let right = normalize(cross(fwd, vec3<f32>(0.0, 1.0, 0.0)));
    let upv = cross(right, fwd);
    var sc = uv_ndc;
    sc.x = sc.x * aspect;
    let rd = normalize(fwd * 1.6 + right * sc.x + upv * sc.y);

    // March the bounded sphere (radius R), accumulating emission (additive HDR) and
    // density (for alpha → the volume reveals the shared background where it's empty).
    let rad = 2.0;
    let t0 = max(dist - rad, 0.05);
    let t1 = dist + rad;
    let dt = (t1 - t0) / f32(nsteps);
    var t = t0;
    var acc = vec3<f32>(0.0);
    var dens = 0.0;
    for (var i = 0; i < nsteps; i = i + 1) {
        let p = ro + rd * t;
        if (dot(p, p) < rad * rad) {
            let s = field3d_sample(p);
            acc = acc + s.rgb * dt * 3.0;     // emission
            dens = dens + s.a * dt * 3.0;     // optical depth
        }
        t = t + dt;
    }
    let alpha = 1.0 - exp(-dens);
    return vec4<f32>(acc * m.p2.w, alpha); // × glow_gain
}

// ===================== 3-D LATTICE (higher-D shadow volumetric) =====================
// IDEA 3, the lattice cluster: Quasicrystal, Weyl and E8 are each a projection or
// slice of a higher-D lattice, so their honest 3-D is to render in THREE dimensions
// instead of two — a 3-D icosahedral quasicrystal (sum of waves on the 6 five-fold
// axes), a rank-3 reflection-group honeycomb (glowing mirror planes), and the 3-D
// shadow of the 240 E8 roots (precomputed on the CPU, tumbling in 8-space). One
// volumetric raymarch, branched per space. `fold (c)` = density/thickness, `churn`
// animates, `spin` orbits, `zoom` dollies, `E8 8-D rotation` tumbles the E8 shadow.

fn lattice3d_sample(p3: vec3<f32>) -> vec4<f32> {
    let space = i32(m.p6.x + 0.5);
    let time = m.p0.y;
    let et = time * m.p6.y;
    let pal = i32(m.p3.w + 0.5);
    let ct0 = m.p3.x - time * m.p4.x;
    let foldc = m.p1.x;

    if (space == 3) {
        // WEYL: rank-3 reflection honeycomb — glowing mirror planes (cubic axes +
        // face diagonals); their intersections carve the space-group cells.
        let freq = 2.0 + foldc * 4.0;
        let sharp = clamp(m.p5.y, 0.0, 1.0);
        let line = mix(0.05, 0.014, sharp);
        let s = 0.70710678;
        var nrm = array<vec3<f32>, 9>(
            vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0),
            vec3<f32>(s, s, 0.0), vec3<f32>(s, -s, 0.0), vec3<f32>(0.0, s, s),
            vec3<f32>(0.0, s, -s), vec3<f32>(s, 0.0, s), vec3<f32>(s, 0.0, -s),
        );
        var edge = 0.0;
        for (var i = 0; i < 9; i = i + 1) {
            let d = dot(p3, nrm[i]) * freq + et * 0.1;
            edge = edge + line / (abs(fract(d) - 0.5) + line);
        }
        edge = edge * 0.12;
        let col = palette(pal, length(p3) * 0.3 + ct0);
        return vec4<f32>(col * edge, edge * 0.5);
    }

    if (space == 4) {
        // E8: splat the 240 precomputed 3-D roots as glowing points (the 3-D shadow
        // of the polytope, tumbling in 8-space via `E8 8-D rotation`).
        let bigR = 1.4;
        let dw = 0.05;
        var glow = 0.0;
        for (var i = 0; i < 240; i = i + 1) {
            let q = m.e8pts3[i].xyz * bigR;
            let d = p3 - q;
            glow = glow + dw * dw / (dot(d, d) + dw * dw);
        }
        let col = palette(pal, length(p3) * 0.5 + ct0); // colour by shell radius
        return vec4<f32>(col * glow * 0.5, glow * 0.3);
    }

    // QUASICRYSTAL (default): a 3-D icosahedral quasicrystal — sum of plane waves on
    // the six five-fold axes → aperiodic order with icosahedral symmetry.
    let freq = 4.0 + foldc * 8.0;
    let phase = et * 0.6;
    let a = 0.52573111;
    let b = 0.85065081;
    var dirs = array<vec3<f32>, 6>(
        vec3<f32>(0.0, a, b), vec3<f32>(0.0, a, -b),
        vec3<f32>(a, b, 0.0), vec3<f32>(-a, b, 0.0),
        vec3<f32>(b, 0.0, a), vec3<f32>(b, 0.0, -a),
    );
    var sum = 0.0;
    for (var i = 0; i < 6; i = i + 1) {
        sum = sum + cos(freq * dot(p3, dirs[i]) + phase);
    }
    sum = sum / 6.0;                       // [-1, 1]
    let bright = pow(max(sum, 0.0), 3.0) * 3.0;
    let col = palette(pal, sum * 0.5 + 0.5 + ct0);
    return vec4<f32>(col * bright, bright * 0.6);
}

fn render_lattice(uv_ndc: vec2<f32>) -> vec4<f32> {
    let aspect = m.p0.x;
    let time = m.p0.y;
    let elev = clamp(m.p7.y, 0.05, 1.45);
    let nsteps = i32(clamp(m.p7.z, 24.0, 400.0));

    let az = time * m.p2.z * 0.2;
    let cz = cos(elev);
    let sz = sin(elev);
    let dist = 3.4 / max(m.p0.z, 0.3);
    let ro = vec3<f32>(cos(az) * cz, sz, sin(az) * cz) * dist;
    let fwd = normalize(-ro);
    let right = normalize(cross(fwd, vec3<f32>(0.0, 1.0, 0.0)));
    let upv = cross(right, fwd);
    var sc = uv_ndc;
    sc.x = sc.x * aspect;
    let rd = normalize(fwd * 1.6 + right * sc.x + upv * sc.y);

    let rad = 2.2;
    let t0 = max(dist - rad, 0.05);
    let t1 = dist + rad;
    let dt = (t1 - t0) / f32(nsteps);
    var t = t0;
    var acc = vec3<f32>(0.0);
    var dens = 0.0;
    for (var i = 0; i < nsteps; i = i + 1) {
        let p = ro + rd * t;
        if (dot(p, p) < rad * rad) {
            let s = lattice3d_sample(p);
            acc = acc + s.rgb * dt * 3.0;
            dens = dens + s.a * dt * 3.0;
        }
        t = t + dt;
    }
    let alpha = 1.0 - exp(-dens);
    return vec4<f32>(acc * m.p2.w, alpha); // × glow_gain
}

// ===================== vertex (fullscreen triangle) =====================
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>, // [-1,1] across the screen (overscanned)
};

@vertex
fn vs_kifs(@builtin(vertex_index) vi: u32) -> VsOut {
    let g = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u)); // (0,0),(2,0),(0,2)
    let ndc = g * 2.0 - vec2<f32>(1.0);                    // (-1,-1),(3,-1),(-1,3)
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = ndc;
    return out;
}

// ===================== fragment (the field) =====================
@fragment
fn fs_kifs(in: VsOut) -> @location(0) vec4<f32> {
    // 3-D modes (KifsView): 1 = lit-relief heightfield, 2 = conformal sphere-inversion
    // solid, 3 = volumetric phase filaments. 0 = the flat/tunnel 2-D field below.
    let view = i32(m.p6.w + 0.5);
    if (view == 1) { return render_relief(in.uv); }
    if (view == 2) { return render_conformal(in.uv); }
    if (view == 3) { return render_filaments(in.uv); }
    if (view == 4) { return render_lattice(in.uv); }

    let aspect = m.p0.x;
    let time = m.p0.y;
    let zoom_base = m.p0.z;
    let breathe = m.p3.y;
    let warp = m.p4.y;
    let glow_gain = m.p2.w;
    let invert = clamp(m.p5.z, 0.0, 1.0);
    let dispersion = max(m.p5.w, 0.0);

    // Aspect-correct, centred coords matching the classic (frag-0.5·res)/res.y map.
    var uv = in.uv * 0.5;
    uv.x = uv.x * aspect;
    let zoom = zoom_base + breathe * sin(time * 0.7);
    var p = uv * zoom;

    // Domain warp — a low-frequency swirl that adds organic, non-self-similar
    // variation on top of the fractal (inert at 0).
    if (warp > 0.0) {
        let w = warp * 0.18;
        p = p + w * vec2<f32>(
            sin(p.y * 3.0 + time * 0.6),
            cos(p.x * 3.0 + time * 0.5),
        );
    }

    // Chromatic dispersion: sample the field at slightly different scales per
    // channel so bright edges split into a prism rim — light through cut glass
    // (inert at 0). Each tap also runs the symmetry morph, so the two compose.
    var col: vec3<f32>;
    if (dispersion > 0.0) {
        // Split by scale (radial, lens-like) AND a small rotation, so the fringe
        // shows even near the centre. ~12% at full strength = a bold rainbow rim.
        let s = dispersion * 0.12;
        let cr = field_color((rot2(s * 0.25) * p) * (1.0 - s)); // red: outer + CCW
        let cg = field_color(p);
        let cb = field_color((rot2(-s * 0.25) * p) * (1.0 + s)); // blue: inner + CW
        col = vec3<f32>(cr.r, cg.g, cb.b);
    } else {
        col = field_color(p);
    }

    // Faint cool ambient fill so the dark field is not pure black — faded out as we
    // invert (there the bands should stay the darkest part of the image).
    col = col + (1.0 - invert) * vec3<f32>(0.03, 0.015, 0.06) / (0.2 + length(uv) * 2.0);
    // Gentle radial vignette (NO clamp/tonemap — the composite owns that).
    let vig = smoothstep(1.55, 0.1, length(uv));
    col = col * vig * glow_gain;

    return vec4<f32>(col, 1.0);
}

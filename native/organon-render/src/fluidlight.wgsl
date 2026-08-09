// Fluid light coupling (#182 Tier 4) — the light-space passes.
//
// Two products, one RGBA16F light-space map (same projection as the shadow map):
//   .r = TRANSMITTANCE — cs_resolve marches the ink dye density along the key
//        light through each texel's light ray (Beer–Lambert): the smoke casts
//        shadow on scene geometry. cube.wgsl multiplies the key light by it.
//   .g = CAUSTIC — cs_caustic fires one ortho key-light ray per texel, finds
//        the liquid isosurface (the MLS-MPM MetaField), refracts at the
//        field-gradient normal, carries the ray to the tank-floor receiver
//        plane and splats its landing texel (fixed-point atomics). Undeviated
//        rays land in their own texel (raw ≈ 1), so `max(raw − 1, 0)` is the
//        pure convergence pattern — the shimmer. cube.wgsl adds it to the key.
//
// Both are approximations by design: the transmittance LUT is half a deep
// shadow map (one slice, not per-depth), and the caustic map is applied as a
// light-space gobo (fragments above the receiver see it too, faintly wrong,
// classically acceptable). The CPU mirrors (`math::optical_depth`,
// `math::refract_dir`) carry the unit tests — keep the arithmetic in lockstep.

struct FlU {
    inv_light_vp: mat4x4<f32>, // light NDC → world (ortho, w = 1)
    light_vp: mat4x4<f32>,     // world → light clip
    dye0: vec4<f32>,           // xyz = dye AABB min, w = extinction σ
    dye1: vec4<f32>,           // xyz = dye AABB max, w = trans on (>0 = march)
    liq0: vec4<f32>,           // xyz = liquid AABB min, w = IOR
    liq1: vec4<f32>,           // xyz = liquid AABB max, w = iso threshold
    misc: vec4<f32>,           // x = caustic on (>0), y = sharpness, z = map res, w = _
};

@group(0) @binding(0) var<uniform> u: FlU;
@group(0) @binding(1) var dye_tex: texture_3d<f32>;
@group(0) @binding(2) var liq_tex: texture_3d<f32>;
@group(0) @binding(3) var samp: sampler;
// Caustic splat accumulator (fixed-point u32 per texel; resolve re-zeroes).
@group(0) @binding(4) var<storage, read_write> accum: array<atomic<u32>>;
@group(0) @binding(5) var out_tex: texture_storage_2d<rgba16float, write>;

const FP: f32 = 256.0;
const TRANS_STEPS: i32 = 48;
const CAUSTIC_STEPS: i32 = 40;

// Texel centre → the light's ortho ray through it (world-space entry + dir).
fn light_ray(px: vec2<u32>) -> array<vec3<f32>, 2> {
    let res = max(u.misc.z, 1.0);
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) / res;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let p0 = u.inv_light_vp * vec4<f32>(ndc, 0.0, 1.0);
    let p1 = u.inv_light_vp * vec4<f32>(ndc, 1.0, 1.0);
    let a = p0.xyz / p0.w;
    let b = p1.xyz / p1.w;
    return array<vec3<f32>, 2>(a, normalize(b - a));
}

// Slab-clip a ray against an AABB; returns (tmin, tmax), miss if tmax <= tmin.
fn clip_aabb(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec2<f32> {
    let inv = 1.0 / rd;
    let ta = (bmin - ro) * inv;
    let tb = (bmax - ro) * inv;
    let tsmall = min(ta, tb);
    let tbig = max(ta, tb);
    return vec2<f32>(
        max(max(tsmall.x, tsmall.y), max(tsmall.z, 0.0)),
        min(min(tbig.x, tbig.y), tbig.z),
    );
}

fn dye_density(p: vec3<f32>) -> f32 {
    let ext = max(u.dye1.xyz - u.dye0.xyz, vec3<f32>(1e-5));
    let uvw = (p - u.dye0.xyz) / ext;
    if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) {
        return 0.0;
    }
    let c = textureSampleLevel(dye_tex, samp, uvw, 0.0);
    return max(c.r, max(c.g, c.b));
}

fn liq_field(p: vec3<f32>) -> f32 {
    let ext = max(u.liq1.xyz - u.liq0.xyz, vec3<f32>(1e-5));
    let uvw = (p - u.liq0.xyz) / ext;
    if (any(uvw < vec3<f32>(0.0)) || any(uvw > vec3<f32>(1.0))) {
        return 0.0;
    }
    return textureSampleLevel(liq_tex, samp, uvw, 0.0).a;
}

// WGSL's refract() (kept explicit so the CPU mirror math::refract_dir matches
// term-for-term): eta = n1/n2, i and n unit, returns 0 on total internal
// reflection.
fn refract_dir(i: vec3<f32>, n: vec3<f32>, eta: f32) -> vec3<f32> {
    let ci = -dot(i, n);
    let k = 1.0 - eta * eta * (1.0 - ci * ci);
    if (k < 0.0) {
        return vec3<f32>(0.0);
    }
    return eta * i + (eta * ci - sqrt(k)) * n;
}

// ==================== caustic splat: one light ray per texel ====================
@compute @workgroup_size(8, 8)
fn cs_caustic(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = u32(u.misc.z);
    if (gid.x >= res || gid.y >= res || u.misc.x <= 0.0) {
        return;
    }
    let ray = light_ray(gid.xy);
    let ro = ray[0];
    let rd = ray[1];
    let span = clip_aabb(ro, rd, u.liq0.xyz, u.liq1.xyz);
    if (span.y <= span.x) {
        return;
    }
    // March to the liquid surface (first sample over the iso threshold).
    let thresh = max(u.liq1.w, 1e-3);
    let dt = (span.y - span.x) / f32(CAUSTIC_STEPS);
    var t = span.x + dt * 0.5;
    var hit = -1.0;
    for (var i = 0; i < CAUSTIC_STEPS; i = i + 1) {
        if (liq_field(ro + rd * t) >= thresh) {
            hit = t;
            break;
        }
        t = t + dt;
    }

    // Refract at the surface; a miss (never entered the liquid) carries the
    // ray straight through — undeviated rays build the ≈1 baseline the resolve
    // subtracts, so only CONVERGENCE shows.
    var dir = rd;
    var ps = ro + rd * max(hit, span.x);
    if (hit >= 0.0) {
        // Normal from the field gradient (6 taps, half a field cell apart);
        // density rises into the liquid, so the outward normal is −∇.
        let ext = max(u.liq1.xyz - u.liq0.xyz, vec3<f32>(1e-5));
        let h = ext * 0.5 / 64.0;
        let g = vec3<f32>(
            liq_field(ps + vec3<f32>(h.x, 0.0, 0.0)) - liq_field(ps - vec3<f32>(h.x, 0.0, 0.0)),
            liq_field(ps + vec3<f32>(0.0, h.y, 0.0)) - liq_field(ps - vec3<f32>(0.0, h.y, 0.0)),
            liq_field(ps + vec3<f32>(0.0, 0.0, h.z)) - liq_field(ps - vec3<f32>(0.0, 0.0, h.z)),
        );
        if (dot(g, g) > 1e-12) {
            var n = -normalize(g);
            if (dot(n, rd) > 0.0) {
                n = -n;
            }
            let ior = max(u.liq0.w, 1.01);
            let r = refract_dir(rd, n, 1.0 / ior);
            if (dot(r, r) > 1e-6) {
                dir = normalize(r);
            }
        }
    }

    // Carry to the receiver plane (the tank floor); non-descending rays exit.
    if (dir.y >= -1e-4) {
        return;
    }
    let tf = (u.liq0.y - ps.y) / dir.y;
    let ph = ps + dir * tf;

    // Splat the landing point in light space (bilinear over 2×2 texels).
    let lp = u.light_vp * vec4<f32>(ph, 1.0);
    let ndc = lp.xyz / lp.w;
    if (abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0) {
        return;
    }
    let fres = u.misc.z;
    let fx = (ndc.x * 0.5 + 0.5) * fres - 0.5;
    let fy = (0.5 - ndc.y * 0.5) * fres - 0.5;
    let bx = i32(floor(fx));
    let by = i32(floor(fy));
    let wx = fx - f32(bx);
    let wy = fy - f32(by);
    for (var dy = 0; dy < 2; dy = dy + 1) {
        for (var dx = 0; dx < 2; dx = dx + 1) {
            let cx = bx + dx;
            let cy = by + dy;
            if (cx < 0 || cy < 0 || cx >= i32(fres) || cy >= i32(fres)) {
                continue;
            }
            let w = (select(1.0 - wx, wx, dx == 1)) * (select(1.0 - wy, wy, dy == 1));
            atomicAdd(&accum[u32(cy) * res + u32(cx)], u32(w * FP));
        }
    }
}

// ============== resolve: transmittance march + caustic normalise ==============
@compute @workgroup_size(8, 8)
fn cs_resolve(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = u32(u.misc.z);
    if (gid.x >= res || gid.y >= res) {
        return;
    }

    // Caustic: consume + clear the accumulator. Undeviated coverage ≈ 1, so
    // the pattern is the excess; `sharpness` shapes the contrast.
    let raw = f32(atomicExchange(&accum[gid.y * res + gid.x], 0u)) / FP;
    var caustic = 0.0;
    if (u.misc.x > 0.0) {
        caustic = min(pow(max(raw - 1.0, 0.0), max(u.misc.y, 0.1)), 8.0);
    }

    // Transmittance: Beer–Lambert through the dye along this texel's light ray.
    var trans = 1.0;
    if (u.dye1.w > 0.0) {
        let ray = light_ray(gid.xy);
        let span = clip_aabb(ray[0], ray[1], u.dye0.xyz, u.dye1.xyz);
        if (span.y > span.x) {
            let dl = (span.y - span.x) / f32(TRANS_STEPS);
            var acc = 0.0;
            var t = span.x + dl * 0.5;
            for (var i = 0; i < TRANS_STEPS; i = i + 1) {
                acc = acc + dye_density(ray[0] + ray[1] * t);
                t = t + dl;
            }
            trans = exp(-acc * max(u.dye0.w, 0.0) * dl);
        }
    }

    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(trans, caustic, 0.0, 1.0));
}

// Particle Aura (#81, Tiers 1 & 2) — GPU advection + additive HDR billboards.
//
// A cloud of luminous motes rides the active generator's velocity field, read
// through ONE abstraction: a coarse 3-D velocity grid (analytic fields baked
// per-cell, node motion splatted — both on the CPU in `math.rs`). The compute
// pass advects/ages/respawns each mote; the draw pass expands each to a camera-
// facing billboard (or a velocity-stretched ribbon) and adds it into the linear
// HDR scene buffer, so it blooms + tonemaps with everything else.
//
// The trilinear `sample_vel` here mirrors `math::VelGrid::sample`, and the
// advection step mirrors `math::VelGrid::advect`, so the offline Rust tests are
// a faithful oracle for this shader (which only runs on the Mac's GPU).

struct Particle {
    pos: vec4<f32>,   // xyz = world position, w = life remaining (seconds)
    vel: vec4<f32>,   // xyz = last velocity (for ribbons), w = max life (seconds)
    col: vec4<f32>,   // rgb = tint, w = per-particle random seed (0..1)
};

struct SimU {
    grid_min: vec4<f32>,  // xyz = grid AABB min, w = dt (seconds)
    grid_max: vec4<f32>,  // xyz = grid AABB max, w = time (seconds)
    grid_res: vec4<u32>,  // xyz = grid resolution, w = particle count
    p0: vec4<f32>,        // speed, lifetime, spawn_radius, drag
    p1: vec4<f32>,        // turbulence, max_step, node_count, _
    p2: vec4<f32>,        // frame_seed, energize(0/1), _, _
};

@group(0) @binding(0) var<uniform> U: SimU;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(2) var<storage, read> velgrid: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> nodes: array<vec4<f32>>;

fn hash_u32(x: u32) -> u32 {
    var v = x;
    v = v ^ (v >> 16u);
    v = v * 0x7feb352du;
    v = v ^ (v >> 15u);
    v = v * 0x846ca68bu;
    v = v ^ (v >> 16u);
    return v;
}

fn rand01(x: u32) -> f32 {
    return f32(hash_u32(x) & 0x00ffffffu) / f32(0x01000000u);
}

// Trilinear velocity at world `p` — mirrors `VelGrid::sample` exactly.
fn sample_vel(p: vec3<f32>) -> vec3<f32> {
    let res = vec3<f32>(f32(U.grid_res.x), f32(U.grid_res.y), f32(U.grid_res.z));
    let span = U.grid_max.xyz - U.grid_min.xyz;
    let u = (p - U.grid_min.xyz) / span;
    let f = u * res - vec3<f32>(0.5);
    let base = floor(f);
    let frac = f - base;
    let rx = i32(U.grid_res.x);
    let ry = i32(U.grid_res.y);
    let rz = i32(U.grid_res.z);
    let i0 = clamp(i32(base.x), 0, rx - 1);
    let j0 = clamp(i32(base.y), 0, ry - 1);
    let k0 = clamp(i32(base.z), 0, rz - 1);
    let i1 = clamp(i32(base.x) + 1, 0, rx - 1);
    let j1 = clamp(i32(base.y) + 1, 0, ry - 1);
    let k1 = clamp(i32(base.z) + 1, 0, rz - 1);
    let g000 = velgrid[(k0 * ry + j0) * rx + i0].xyz;
    let g100 = velgrid[(k0 * ry + j0) * rx + i1].xyz;
    let g010 = velgrid[(k0 * ry + j1) * rx + i0].xyz;
    let g110 = velgrid[(k0 * ry + j1) * rx + i1].xyz;
    let g001 = velgrid[(k1 * ry + j0) * rx + i0].xyz;
    let g101 = velgrid[(k1 * ry + j0) * rx + i1].xyz;
    let g011 = velgrid[(k1 * ry + j1) * rx + i0].xyz;
    let g111 = velgrid[(k1 * ry + j1) * rx + i1].xyz;
    let c00 = mix(g000, g100, frac.x);
    let c10 = mix(g010, g110, frac.x);
    let c01 = mix(g001, g101, frac.x);
    let c11 = mix(g011, g111, frac.x);
    let c0 = mix(c00, c10, frac.y);
    let c1 = mix(c01, c11, frac.y);
    return mix(c0, c1, frac.z);
}

// Trilinear scalar sample of the velocity grid's `w` channel — the field ENERGY
// density (#247), baked per cell in `math::VelGrid::to_vec4`. Mirrors `sample_vel`
// but on the scalar w. 0 everywhere the field has no energy (non-Maxwell / splatted).
fn sample_energy(p: vec3<f32>) -> f32 {
    let res = vec3<f32>(f32(U.grid_res.x), f32(U.grid_res.y), f32(U.grid_res.z));
    let span = U.grid_max.xyz - U.grid_min.xyz;
    let u = (p - U.grid_min.xyz) / span;
    let f = u * res - vec3<f32>(0.5);
    let base = floor(f);
    let frac = f - base;
    let rx = i32(U.grid_res.x);
    let ry = i32(U.grid_res.y);
    let rz = i32(U.grid_res.z);
    let i0 = clamp(i32(base.x), 0, rx - 1);
    let j0 = clamp(i32(base.y), 0, ry - 1);
    let k0 = clamp(i32(base.z), 0, rz - 1);
    let i1 = clamp(i32(base.x) + 1, 0, rx - 1);
    let j1 = clamp(i32(base.y) + 1, 0, ry - 1);
    let k1 = clamp(i32(base.z) + 1, 0, rz - 1);
    let g000 = velgrid[(k0 * ry + j0) * rx + i0].w;
    let g100 = velgrid[(k0 * ry + j0) * rx + i1].w;
    let g010 = velgrid[(k0 * ry + j1) * rx + i0].w;
    let g110 = velgrid[(k0 * ry + j1) * rx + i1].w;
    let g001 = velgrid[(k1 * ry + j0) * rx + i0].w;
    let g101 = velgrid[(k1 * ry + j0) * rx + i1].w;
    let g011 = velgrid[(k1 * ry + j1) * rx + i0].w;
    let g111 = velgrid[(k1 * ry + j1) * rx + i1].w;
    let c00 = mix(g000, g100, frac.x);
    let c10 = mix(g010, g110, frac.x);
    let c01 = mix(g001, g101, frac.x);
    let c11 = mix(g011, g111, frac.x);
    let c0 = mix(c00, c10, frac.y);
    let c1 = mix(c01, c11, frac.y);
    return mix(c0, c1, frac.z);
}

// A fresh spawn position near the geometry: jitter around a random node (or the
// grid AABB if there are no nodes). `s` seeds the RNG.
fn spawn_pos(s: u32) -> vec3<f32> {
    let node_count = u32(max(U.p1.z, 0.0));
    let jitter = vec3<f32>(
        rand01(s + 11u) * 2.0 - 1.0,
        rand01(s + 23u) * 2.0 - 1.0,
        rand01(s + 41u) * 2.0 - 1.0,
    ) * U.p0.z;
    if (node_count > 0u) {
        let ni = hash_u32(s + 97u) % node_count;
        return nodes[ni].xyz + jitter;
    }
    // No nodes: scatter through the velocity grid AABB.
    let r = vec3<f32>(rand01(s + 3u), rand01(s + 7u), rand01(s + 13u));
    return mix(U.grid_min.xyz, U.grid_max.xyz, r);
}

@compute @workgroup_size(64)
fn cs_init(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= U.grid_res.w) {
        return;
    }
    let s = i * 2654435761u + u32(U.p2.x);
    var p: Particle;
    p.pos = vec4<f32>(spawn_pos(s), rand01(s + 5u) * U.p0.y);
    p.vel = vec4<f32>(0.0, 0.0, 0.0, U.p0.y);
    p.col = vec4<f32>(0.0, 0.0, 0.0, rand01(s + 71u));
    particles[i] = p;
}

@compute @workgroup_size(64)
fn cs_advect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= U.grid_res.w) {
        return;
    }
    var p = particles[i];
    let dt = U.grid_min.w;
    var life = p.pos.w - dt;

    if (life <= 0.0) {
        // Age-out → respawn near the geometry. The beat burst shortens lifetimes
        // a touch on the pulse so the cloud renews in waves.
        let s = i * 2246822519u + u32(U.p2.x);
        let life_scale = 0.5 + 0.5 * rand01(s + 17u);
        particles[i].pos = vec4<f32>(spawn_pos(s), U.p0.y * life_scale);
        particles[i].vel = vec4<f32>(0.0, 0.0, 0.0, U.p0.y * life_scale);
        return;
    }

    // Advect: sample the field, add a little turbulence, ease toward it (drag),
    // clamp the per-step displacement so the coarse grid can't tunnel a mote.
    var v = sample_vel(p.pos.xyz) * U.p0.x;
    let turb = U.p1.x;
    if (turb > 0.0) {
        let s = i * 374761393u + u32(U.grid_max.w * 60.0);
        let jit = vec3<f32>(
            rand01(s + 1u) * 2.0 - 1.0,
            rand01(s + 2u) * 2.0 - 1.0,
            rand01(s + 3u) * 2.0 - 1.0,
        );
        v = v + jit * turb * U.p0.x;
    }
    // Drag blends the carried velocity toward the field (0 = pure advection).
    let drag = clamp(U.p0.w, 0.0, 1.0);
    let blended = mix(v, p.vel.xyz, drag);

    var step = blended * dt;
    let max_step = U.p1.y;
    let len = length(step);
    if (len > max_step && len > 1e-9) {
        step = step * (max_step / len);
    }

    p.pos = vec4<f32>(p.pos.xyz + step, life);
    p.vel = vec4<f32>(blended, p.vel.w);
    // Field energization (#247): stash the local field energy density in col.x for
    // the draw pass. Only when on — a fluid grid's `w` isn't energy (Tier 3). col.w
    // (the per-mote seed) is preserved.
    if (U.p2.y > 0.5) {
        p.col.x = sample_energy(p.pos.xyz);
    }
    particles[i] = p;
}

// ---------------------------------------------------------------------------
// Draw: each particle → a camera-facing billboard (or velocity ribbon).
// ---------------------------------------------------------------------------

struct DrawU {
    view_proj: mat4x4<f32>, // UNSCALED world → clip (motes live in world space)
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    params: vec4<f32>,      // size, emissive, alpha, ribbon(0/1)
    params2: vec4<f32>,     // ribbon_stretch, hue_shift, energize(0/1), energy_gain
    params3: vec4<f32>,     // energy_knee, energy_hue, energy_contrast, energy_hue_cycle (#247/#248)
    // #298 Tier 1 shaded beads — the scene's PBR/IBL context (filled from the cube
    // Uniforms in render.rs), consumed only by vs_bead/fs_bead:
    cam_pos: vec4<f32>,     // xyz = camera world pos, w = prefilter mip count
    key_light: vec4<f32>,   // xyz = world dir TO key light (unit), w = intensity
    fill_light: vec4<f32>,  // xyz = world dir TO fill light (unit), w = intensity
    env: vec4<f32>,         // x=exposure(unused here), y=env_intensity, z=env_rotation, w=ambient_mul
    env_tint: vec4<f32>,    // xyz = environment tint (white = none), w unused
    bead: vec4<f32>,        // x=beads on(0/1), y=metallic, z=roughness, w=emissive scale
    bead2: vec4<f32>,       // #298 Tier 2: x=material, y=shape, z=ior, w=shape amount
    bead_hsv: vec4<f32>,    // #305 Tier 1: x=effective hue, y=saturation, z=value, w reserved
    skyrefl: vec4<f32>,     // #305 Tier 2: x=enable, y=cover, z=drift phase, w=strength
    // PBR text T6 (#217) — the coaxial glass capsule. x = CORE FRACTION: the inner
    // emissive capsule's radius as a fraction of the outer radius; 0 = OFF, and the
    // capsule Glass/Refractive path is then exactly today's `shade_bead`. y =
    // absorption density (Beer–Lambert σ scale per outer radius; 0 = a clear shell).
    // z/w reserved. Read only by `fs_capsule`; the bead and spark paths ignore it.
    // Appended at the TAIL so every earlier offset is untouched.
    capsule: vec4<f32>,
};

@group(0) @binding(0) var<uniform> D: DrawU;

// ----- group(1): the shared split-sum IBL (mirrors cube.wgsl group(1)) -----
// Bound only for the bead (opaque, PBR-shaded) draw; the additive spark pipeline
// leaves it unbound. Same layout as env.rs::ibl_layout: 0=irradiance, 1=prefilter,
// 2=BRDF LUT, 3=Filtering sampler — all equirect Rgba16Float.
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.1591, 0.3183);

// Unit direction -> equirect UV (mirrors cube.wgsl::dir_to_equirect_uv).
fn dir_to_equirect_uv(dir: vec3<f32>) -> vec2<f32> {
    let d = normalize(dir);
    var uv = vec2<f32>(atan2(d.z, d.x), asin(clamp(d.y, -1.0, 1.0)));
    uv = uv * INV_ATAN + vec2<f32>(0.5, 0.5);
    return uv;
}
fn rotate_y(d: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
}
fn fresnel_schlick_roughness(cos_theta: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let one_minus_r = vec3<f32>(1.0 - roughness);
    return f0 + (max(one_minus_r, f0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}
fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = max(roughness * roughness, 4e-4);
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}
fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}
fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    return geometry_schlick_ggx(max(dot(n, v), 0.0), roughness)
         * geometry_schlick_ggx(max(dot(n, l), 0.0), roughness);
}
// One directional light's Cook-Torrance contribution (mirrors cube.wgsl::direct_light).
fn direct_light(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>,
                albedo: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>,
                radiance: vec3<f32>) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) { return vec3<f32>(0.0); }
    let h = normalize(v + l);
    let f = fresnel_schlick(max(dot(h, v), 0.0), f0);
    let d = distribution_ggx(n, h, roughness);
    let g = geometry_smith(n, v, l, roughness);
    let specular = (d * f * g) / (4.0 * max(dot(n, v), 1e-4) * n_dot_l + 1e-4);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    return (kd * albedo / PI + specular) * radiance * n_dot_l;
}
fn sample_irradiance(n_env: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(irradiance_tex, ibl_samp, dir_to_equirect_uv(n_env), 0.0).rgb;
}
// #305 Tier 2: drifting cloud modulation of the sharp env reflection (mirrors
// cube.wgsl). Returns a brightness multiplier; identity when D.skyrefl.x < 0.5.
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
    if (sky.x < 0.5) { return 1.0; }
    let up = smoothstep(-0.02, 0.35, dir.y);
    let sharp = clamp(1.0 - roughness, 0.0, 1.0);
    let amt = up * sharp * clamp(sky.w, 0.0, 1.0);
    if (amt <= 0.0) { return 1.0; }
    let uv = dir.xz / max(dir.y + 0.25, 0.15) * 1.2 + vec2<f32>(sky.z, sky.z * 0.5);
    let n = cloud_fbm(uv);
    let cover = clamp(sky.y, 0.0, 1.0);
    let mask = smoothstep(1.0 - cover - 0.15, 1.0 - cover + 0.15, n);
    return mix(1.0, mix(0.65, 1.7, mask), amt);
}
fn sample_prefiltered(r_env: vec3<f32>, roughness: f32) -> vec3<f32> {
    let mip_count = D.cam_pos.w;
    let lod = roughness * max(mip_count - 1.0, 0.0);
    let base = textureSampleLevel(prefilter_tex, ibl_samp, dir_to_equirect_uv(r_env), lod).rgb;
    return base * cloud_bright(r_env, roughness, D.skyrefl);
}

struct VsIn {
    @builtin(vertex_index) vi: u32,
    @location(0) pos: vec4<f32>,
    @location(1) vel: vec4<f32>,
    @location(2) col: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) fade: f32,
    @location(3) heat: f32, // 0 = cool ember, 1 = white-hot spark (drives the core)
};

// HSV → RGB (hue in turns, s/v in 0..1).
fn hsv_sv(hue: f32, s: f32, v: f32) -> vec3<f32> {
    let h = fract(hue) * 6.0;
    let x = 1.0 - abs((h % 2.0) - 1.0);
    var rgb = vec3<f32>(0.0);
    if (h < 1.0) { rgb = vec3<f32>(1.0, x, 0.0); }
    else if (h < 2.0) { rgb = vec3<f32>(x, 1.0, 0.0); }
    else if (h < 3.0) { rgb = vec3<f32>(0.0, 1.0, x); }
    else if (h < 4.0) { rgb = vec3<f32>(0.0, x, 1.0); }
    else if (h < 5.0) { rgb = vec3<f32>(x, 0.0, 1.0); }
    else { rgb = vec3<f32>(1.0, 0.0, x); }
    return ((rgb - vec3<f32>(1.0)) * s + vec3<f32>(1.0)) * v;
}

@vertex
fn vs_particle(in: VsIn) -> VsOut {
    // Two triangles (6 verts) → a unit quad in [-1,1]².
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[in.vi];

    let life = in.pos.w;
    let max_life = max(in.vel.w, 1e-3);
    let t = clamp(life / max_life, 0.0, 1.0);
    // Fade in at birth, out at death (soft, so motes don't pop).
    let fade_life = smoothstep(0.0, 0.15, t) * smoothstep(0.0, 0.2, 1.0 - t);

    let size = D.params.x;
    var right = D.cam_right.xyz;
    var up = D.cam_up.xyz;

    // Ribbon: stretch along the screen-projected velocity for a motion streak.
    if (D.params.w > 0.5) {
        let vlen = length(in.vel.xyz);
        if (vlen > 1e-5) {
            let vdir = in.vel.xyz / vlen;
            let rr = dot(vdir, D.cam_right.xyz);
            let uu = dot(vdir, D.cam_up.xyz);
            let screen_dir = normalize(vec2<f32>(rr, uu) + vec2<f32>(1e-6, 0.0));
            let perp = vec2<f32>(-screen_dir.y, screen_dir.x);
            let stretch = 1.0 + D.params2.x * min(vlen, 8.0);
            let axis = screen_dir * stretch;
            right = (D.cam_right.xyz * axis.x + D.cam_up.xyz * axis.y);
            up = (D.cam_right.xyz * perp.x + D.cam_up.xyz * perp.y);
        }
    }

    let world = in.pos.xyz + (right * c.x + up * c.y) * size;

    var out: VsOut;
    out.clip = D.view_proj * vec4<f32>(world, 1.0);
    out.uv = c;

    // Colour: glowing embers, not dull dots — driven either by field ENERGY (#247
    // energization) or by the mote's speed.
    let seed = in.col.w;
    if (D.params2.z > 0.5) {
        // #247 field energization: the mote lights by the field ENERGY density it
        // sampled (col.x), not its speed — the fluorescent-tube-near-an-antenna demo.
        // Bright in the strong-field zones, dark in the nulls (the honest energy
        // gradient the advector normalized away). The near field spans ~1/r⁶, so
        // log-compress then softly cap at the knee (a Reinhard whitepoint) — the
        // near-source spike blooms instead of flattening into one solid ball.
        // #248 near-core contrast: raise the sampled energy to a power BEFORE the log
        // so the high-energy core stands out instead of the log flattening the whole
        // cloud to one brightness. 1 = the original look. Works in both the Lite tier
        // (grid |E|²) and the Fluid tier (the solver's kinetic energy) — both ride col.x.
        let contrast = max(D.params3.z, 0.05);
        let u = pow(max(in.col.x, 0.0), contrast);
        let gain = D.params2.w;
        let knee = max(D.params3.x, 1e-3);
        let base_hue = D.params3.y;
        let xl = gain * log(1.0 + u);
        let mapped = xl / (1.0 + xl / knee);
        let heat_e = clamp(mapped / knee, 0.0, 1.0);
        // #248 hue cycle (params3.w): the beat pulse walks the hue around the wheel.
        let hue = base_hue + D.params3.w + (seed - 0.5) * 0.03;
        let sat = mix(0.90, 0.15, heat_e); // hot cores desaturate toward white
        // Premultiply by the tone-mapped energy so HDR brightness tracks |E|² and
        // blooms in the strong zones (fs multiplies color by shape × emissive × alpha).
        out.color = hsv_sv(hue, sat, 1.0) * mapped;
        out.heat = heat_e;
    } else {
        // The mote's SPEED is its "heat" — slow motes are deep saturated embers, fast
        // ones flare white-hot — and they cool as they age. hue_shift rotates the whole
        // palette coherently, with a touch of per-mote jitter so it stays alive.
        let spd = length(in.vel.xyz);
        let heat = clamp((1.0 - exp(-spd * 0.35)) * mix(0.65, 1.0, t), 0.0, 1.0);
        let hue = 0.055 + D.params2.y + (seed - 0.5) * 0.05;
        let sat = mix(0.95, 0.20, heat); // hot → desaturate toward white
        let val = mix(0.55, 1.0, heat);  // slow embers dimmer, hot ones bright
        out.color = hsv_sv(hue, sat, val);
        out.heat = heat;
    }
    out.fade = fade_life;
    return out;
}

@fragment
fn fs_particle(in: VsOut) -> @location(0) vec4<f32> {
    // Soft round mote with a tight, HDR-bright core so it blooms like a spark.
    let r2 = dot(in.uv, in.uv);
    let falloff = clamp(1.0 - r2, 0.0, 1.0);
    let glow = falloff * falloff;       // broad coloured halo
    let core = pow(falloff, 5.0);       // tight centre
    // The core punches past 1 (HDR) so the mote glows + rolls off in the post
    // chain — emissive × opacity scale it.
    let gain = in.fade * D.params.y * D.params.z; // emissive × alpha × birth/death fade
    let shape = glow * 0.7 + core * 3.0;
    var rgb = in.color * shape * gain;
    // A white-hot pinpoint at the centre of the faster motes (the "spark").
    let white = core * core * mix(0.15, 1.6, in.heat);
    rgb = rgb + vec3<f32>(white) * gain;
    let intensity = shape * gain;
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), clamp(intensity, 0.0, 1.0));
}

// ===========================================================================
// #298 Tiers 1 & 2 — shaded impostor BEADS: the same advected motes drawn as
// opaque droplets bearing the shared split-sum IBL + key/fill lighting, in a
// choice of PBR MATERIAL (Standard / Chrome / Glass / Refractive) and SHAPE
// (Sphere / Ellipsoid / Teardrop / Rounded-Box / Dice). The particle SYSTEM is
// unchanged (same compute advection, count, energization, hue cycle); only the
// DRAW differs. Sphere uses a cheap analytic impostor (Tier 1); the other shapes
// sphere-trace a per-mote SDF inside the billboard, oriented by the mote's
// velocity (teardrops streak along the flow). Depth-write on so droplets occlude
// each other + the scene. For BEADS, Glass/Refractive reflect+refract the
// ENVIRONMENT only (the scene-behind refraction is the Tier-4 RT job). CAPSULE
// impostors can instead show a coaxial emissive CORE through the shell when
// `D.capsule.x > 0` — `shade_capsule_glass` below (PBR text T6, #217).
// ===========================================================================

// ---- signed distance fields (local unit space, long axis = +z) ----
fn sd_round_box(p: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}
fn sd_ellipsoid(p: vec3<f32>, r: vec3<f32>) -> f32 {
    let k0 = length(p / r);
    let k1 = length(p / (r * r));
    return k0 * (k0 - 1.0) / max(k1, 1e-6);
}
// iq's vertical round cone (base radius r1 at z=0, tip radius r2 at z=h).
fn sd_round_cone_z(p: vec3<f32>, r1: f32, r2: f32, h: f32) -> f32 {
    let q = vec2<f32>(length(p.xy), p.z);
    let b = (r1 - r2) / h;
    let a = sqrt(max(1.0 - b * b, 0.0));
    let k = dot(q, vec2<f32>(-b, a));
    if (k < 0.0) { return length(q) - r1; }
    if (k > a * h) { return length(q - vec2<f32>(0.0, h)) - r2; }
    return dot(q, vec2<f32>(a, b)) - r1;
}
// The bead SDF at local point `lp` for `shape` with stretch/roundness `amt`.
fn bead_sdf(lp: vec3<f32>, shape: u32, amt: f32) -> f32 {
    if (shape == 1u) {
        // Ellipsoid: stretched along +z (velocity) as `amt` rises.
        let r = vec3<f32>(mix(0.85, 0.6, amt), mix(0.85, 0.6, amt), mix(1.0, 1.5, amt));
        return sd_ellipsoid(lp, r);
    } else if (shape == 2u) {
        // Teardrop: a round cone, fat base trailing, pointed tip leading (+z).
        let h = mix(1.3, 1.8, amt);
        let r1 = mix(0.6, 0.75, amt);
        let q = vec3<f32>(lp.x, lp.y, lp.z + h * 0.5); // base near z=0, tip at +z
        return sd_round_cone_z(q, r1, 0.04, h);
    } else if (shape == 3u) {
        // Rounded box.
        return sd_round_box(lp, vec3<f32>(mix(0.85, 0.7, amt)), mix(0.15, 0.35, amt));
    } else if (shape == 4u) {
        // Dice: a harder-edged rounded cube.
        return sd_round_box(lp, vec3<f32>(0.8), 0.08);
    }
    return length(lp) - 1.0; // sphere (unused — sphere takes the analytic path)
}
fn bead_sdf_normal(lp: vec3<f32>, shape: u32, amt: f32) -> vec3<f32> {
    let e = 0.002;
    let dx = vec3<f32>(e, 0.0, 0.0);
    let dy = vec3<f32>(0.0, e, 0.0);
    let dz = vec3<f32>(0.0, 0.0, e);
    return normalize(vec3<f32>(
        bead_sdf(lp + dx, shape, amt) - bead_sdf(lp - dx, shape, amt),
        bead_sdf(lp + dy, shape, amt) - bead_sdf(lp - dy, shape, amt),
        bead_sdf(lp + dz, shape, amt) - bead_sdf(lp - dz, shape, amt),
    ));
}
// Orthonormal frame with +z = the mote's velocity (the long axis of oriented
// shapes). Falls back to world axes for a near-still mote. Columns = (ax, ay, az).
fn bead_frame(vel: vec3<f32>) -> mat3x3<f32> {
    let sp = length(vel);
    let az = select(vec3<f32>(0.0, 0.0, 1.0), vel / max(sp, 1e-6), sp > 1e-4);
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(az.y) > 0.9);
    let ax = normalize(cross(up, az));
    let ay = cross(az, ax);
    return mat3x3<f32>(ax, ay, az);
}

struct BeadOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,      // billboard corner in [-1,1]²
    @location(1) center: vec3<f32>,  // mote world-space centre
    @location(2) albedo: vec3<f32>,  // bead base colour (hue-tinted pearl)
    @location(3) emissive: vec3<f32>,// the energization / ember glow (kept alive)
    @location(4) size: f32,          // world radius
    @location(5) world: vec3<f32>,   // billboard-plane world pos (the SDF ray target)
    @location(6) vel: vec3<f32>,     // mote world velocity (shape orientation)
};

@vertex
fn vs_bead(in: VsIn) -> BeadOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[in.vi];
    let center = in.pos.xyz;
    let size = D.params.x;

    // Enlarge the billboard for non-sphere shapes so their (possibly elongated)
    // silhouette fits the quad the fragment stage marches within.
    let extent = select(1.0, 1.9, D.bead2.y > 0.5) * size;
    let world = center + (D.cam_right.xyz * c.x + D.cam_up.xyz * c.y) * extent;

    let life = in.pos.w;
    let max_life = max(in.vel.w, 1e-3);
    let t = clamp(life / max_life, 0.0, 1.0);
    let fade_life = smoothstep(0.0, 0.15, t) * smoothstep(0.0, 0.2, 1.0 - t);

    // Reuse the spark colour logic, split into an ALBEDO (hue-tinted pearl the PBR
    // reflects) and an EMISSIVE glow (so beads still light up in the vortex).
    let seed = in.col.w;
    var hue_rgb: vec3<f32>;
    var glow: f32;
    if (D.params2.z > 0.5) {
        let contrast = max(D.params3.z, 0.05);
        let uu = pow(max(in.col.x, 0.0), contrast);
        let gain = D.params2.w;
        let knee = max(D.params3.x, 1e-3);
        let base_hue = D.params3.y;
        let xl = gain * log(1.0 + uu);
        let mapped = xl / (1.0 + xl / knee);
        let hue = base_hue + D.params3.w + (seed - 0.5) * 0.03;
        hue_rgb = hsv_sv(hue, 0.9, 1.0);
        glow = mapped;
    } else {
        let spd = length(in.vel.xyz);
        let heat = clamp((1.0 - exp(-spd * 0.35)) * mix(0.65, 1.0, t), 0.0, 1.0);
        let hue = 0.055 + D.params2.y + (seed - 0.5) * 0.05;
        hue_rgb = hsv_sv(hue, mix(0.95, 0.25, heat), 1.0);
        glow = heat;
    }

    var out: BeadOut;
    out.clip = D.view_proj * vec4<f32>(world, 1.0);
    out.uv = c;
    out.center = center;
    out.size = size;
    out.world = world;
    out.vel = in.vel.xyz;
    out.albedo = mix(vec3<f32>(1.0), hue_rgb, 0.5);
    // Energization glow + the Material **Emissive** (bead_hsv.w, HDR): the solid
    // bead impostor emits its OWN hue (hue_rgb) × the emissive value, so it blooms
    // in colour. bead_hsv.w = 0 → the additive-spark look, byte-identical.
    out.emissive = hue_rgb * (glow * D.params.y * D.bead.w + D.bead_hsv.w) * fade_life;
    return out;
}

struct BeadFrag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

// Shade a bead surface point with the chosen material. `n` world normal, `surf`
// Material colour transform (#305 Tier 1) — recolour by Hue/Saturation/Value.
// Mirrors cube.wgsl::apply_hsv. `hsv.x`=hue offset (turns), `hsv.y`=saturation,
// `hsv.z`=value. [0,1,1] identity.
fn apply_hsv(rgb: vec3<f32>, hsv: vec4<f32>) -> vec3<f32> {
    var c = rgb;
    let hue = hsv.x;
    if (hue != 0.0) {
        let a = hue * 6.2831853;
        let cs = cos(a);
        let sn = sin(a);
        let k = 0.57735026;
        let one_c = 1.0 - cs;
        let m0 = vec3<f32>(cs + k * k * one_c,     k * k * one_c - k * sn, k * k * one_c + k * sn);
        let m1 = vec3<f32>(k * k * one_c + k * sn, cs + k * k * one_c,     k * k * one_c - k * sn);
        let m2 = vec3<f32>(k * k * one_c - k * sn, k * k * one_c + k * sn, cs + k * k * one_c);
        c = vec3<f32>(dot(m0, c), dot(m1, c), dot(m2, c));
    }
    let luma = dot(max(c, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, hsv.y);
    c = c * hsv.z;
    return max(c, vec3<f32>(0.0));
}

// world position, `albedo`/`emissive` from the vertex stage.
fn shade_bead(n: vec3<f32>, surf: vec3<f32>, albedo: vec3<f32>, emissive: vec3<f32>) -> vec3<f32> {
    let v = normalize(D.cam_pos.xyz - surf);
    let n_dot_v = max(dot(n, v), 1e-4);
    let mat = D.bead2.x;
    let env_rot = D.env.z;
    let ei = D.env.y;
    let am = D.env.w;
    let et = D.env_tint.rgb;
    let n_env = rotate_y(n, env_rot);

    // ---- Glass / Refractive: Fresnel-blended env reflect + refract ----
    if (mat >= 1.5) {
        let roughness = clamp(D.bead.z, 0.02, 1.0);
        let ior = max(D.bead2.z, 1.0);
        let r_env = rotate_y(reflect(-v, n), env_rot);
        let refl = sample_prefiltered(r_env, roughness);
        let rdir = refract(-v, n, 1.0 / ior);
        var thru: vec3<f32>;
        if (dot(rdir, rdir) < 1e-6) {
            thru = refl; // total internal reflection
        } else {
            thru = sample_prefiltered(rotate_y(rdir, env_rot), roughness);
        }
        let f0s = (ior - 1.0) / (ior + 1.0);
        let f0g = f0s * f0s;
        let fr = f0g + (1.0 - f0g) * pow(1.0 - n_dot_v, 5.0);
        // Refractive (mat ≥ 2.5) murks the transmission in the mote's colour; Glass
        // (2) keeps a lighter body tint.
        let murk = select(mix(vec3<f32>(1.0), albedo, 0.35),
                          mix(vec3<f32>(1.0), albedo, 0.7), mat >= 2.5);
        return mix(thru * murk, refl, fr) * ei * am * et + emissive;
    }

    // ---- Standard (pearl) / Chrome (mirror): metallic-roughness PBR ----
    let is_chrome = mat >= 0.5;
    let metallic = select(clamp(D.bead.y, 0.0, 1.0), 1.0, is_chrome);
    let roughness = select(clamp(D.bead.z, 0.02, 1.0), min(clamp(D.bead.z, 0.02, 1.0), 0.08), is_chrome);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let r_env = rotate_y(reflect(-v, n), env_rot);
    let f = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let diffuse = sample_irradiance(n_env) * albedo;
    let prefiltered = sample_prefiltered(r_env, roughness);
    let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
                                  vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0),
                                            max(roughness, 1e-3)), 0.0).rg;
    let specular = prefiltered * (f0 * brdf.x + vec3<f32>(brdf.y));
    let ambient = (kd * diffuse + specular) * ei * am * et;
    let l_key = normalize(D.key_light.xyz);
    let l_fill = normalize(D.fill_light.xyz);
    let direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, vec3<f32>(D.key_light.w))
               + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(D.fill_light.w));
    return ambient + direct + emissive;
}

// Resolve the impostor surface for a fragment: the world normal + hit point of the
// bead's shape (analytic sphere, or an SDF sphere-trace for the others). `hit` is
// false when the fragment falls outside the silhouette (the caller discards). Shared
// by the colour draw (`fs_bead`) and the depth-prepass draw (`fs_bead_depth`) so
// both agree on the exact surface (#298 Tier 3).
struct BeadHit {
    hit: bool,
    n: vec3<f32>,
    surf: vec3<f32>,
};

fn bead_trace(in: BeadOut) -> BeadHit {
    var out: BeadHit;
    out.hit = false;
    out.n = vec3<f32>(0.0, 0.0, 1.0);
    out.surf = in.center;
    let shape = u32(D.bead2.y + 0.5);

    if (shape == 0u) {
        // Analytic sphere impostor (Tier 1): normal + surface from the billboard UV.
        let uv = in.uv;
        let r2 = dot(uv, uv);
        if (r2 > 1.0) { return out; }
        let z = sqrt(max(1.0 - r2, 0.0));
        let to_cam = normalize(D.cam_pos.xyz - in.center);
        let offset = D.cam_right.xyz * uv.x + D.cam_up.xyz * uv.y + to_cam * z;
        out.n = normalize(offset);
        out.surf = in.center + offset * in.size;
        out.hit = true;
        return out;
    }

    // SDF sphere-trace inside the billboard (Tier 2). Ray from the camera through
    // this fragment's billboard-plane point; march the per-mote SDF in its local
    // (velocity-oriented) frame between the bounding-sphere entry/exit.
    let ro = D.cam_pos.xyz;
    let rd = normalize(in.world - ro);
    let amt = clamp(D.bead2.w, 0.0, 1.0);
    let rb = in.size * 1.9;
    let oc = ro - in.center;
    let b = dot(oc, rd);
    let cc = dot(oc, oc) - rb * rb;
    let disc = b * b - cc;
    if (disc < 0.0) { return out; }
    let sq = sqrt(disc);
    var t = max(-b - sq, 0.0);
    let t_far = -b + sq;
    let frame = bead_frame(in.vel);
    let frame_t = transpose(frame);
    var lp = vec3<f32>(0.0);
    for (var i = 0; i < 48; i = i + 1) {
        let wp = ro + rd * t;
        lp = frame_t * (wp - in.center) / in.size;
        let d = bead_sdf(lp, shape, amt) * in.size;
        if (d < in.size * 0.0025) {
            out.hit = true;
            out.surf = wp;
            out.n = normalize(frame * bead_sdf_normal(lp, shape, amt));
            return out;
        }
        t = t + max(d, in.size * 0.004);
        if (t > t_far) { return out; }
    }
    return out;
}

@fragment
fn fs_bead(in: BeadOut) -> BeadFrag {
    let shape = u32(D.bead2.y + 0.5);
    var n: vec3<f32>;
    var surf: vec3<f32>;
    // #305 Tier 3: silhouette coverage → alpha, fed to alpha-to-coverage so MSAA
    // smooths the impostor edge (a hard `discard` gets no sub-pixel coverage). For the
    // analytic sphere the edge is exact (derivative of the disc radius); the SDF shapes
    // keep the hard hit/miss boundary (coverage 1) — a coarser follow-up.
    var cov = 1.0;
    if (shape == 0u) {
        // Analytic sphere with a screen-space-consistent 1px feathered rim.
        let uv = in.uv;
        let r = length(uv);
        let w = max(fwidth(r), 1e-5);
        cov = clamp((1.0 - r) / w + 0.5, 0.0, 1.0);
        if (cov <= 0.001) { discard; }
        let r2 = min(r * r, 1.0);
        let z = sqrt(max(1.0 - r2, 0.0));
        let to_cam = normalize(D.cam_pos.xyz - in.center);
        let offset = D.cam_right.xyz * uv.x + D.cam_up.xyz * uv.y + to_cam * z;
        n = normalize(offset);
        surf = in.center + offset * in.size;
    } else {
        let h = bead_trace(in);
        if (!h.hit) { discard; }
        n = h.n;
        surf = h.surf;
    }
    let clip = D.view_proj * vec4<f32>(surf, 1.0);
    var out: BeadFrag;
    // #305 Tier 1: recolour the bead albedo + emissive by the per-bead HSV (identity
    // [0,1,1] → unchanged).
    let alb = apply_hsv(in.albedo, D.bead_hsv);
    let emi = apply_hsv(in.emissive, D.bead_hsv);
    out.color = vec4<f32>(max(shade_bead(n, surf, alb, emi), vec3<f32>(0.0)), cov);
    out.depth = clip.z / max(clip.w, 1e-6);
    return out;
}

// Depth-only bead draw for the FX depth prepass (#298 Tier 3): write the impostor's
// reconstructed `frag_depth` (and nothing else) so the screen-space effects — SSAO,
// SSR, SSGI, DoF, TAA — that reconstruct from the prepass depth pick the droplets up
// as first-class scene geometry (cubes reflect them, they contact-darken + bleed
// colour). No colour target.
@fragment
fn fs_bead_depth(in: BeadOut) -> @builtin(frag_depth) f32 {
    let h = bead_trace(in);
    if (!h.hit) { discard; }
    let clip = D.view_proj * vec4<f32>(h.surf, 1.0);
    return clip.z / max(clip.w, 1e-6);
}

// ===========================================================================
// Membrane Skin-Arms capsule impostors (#membrane-arms Stage 2).
// ---------------------------------------------------------------------------
// Each arm segment (node A → node B of a generator strand) is drawn as ONE
// per-instance capsule sphere-impostor: a camera-facing billboard sized to the
// capsule's bounding sphere, with the fragment sphere-tracing the analytic
// capsule SDF in world space and shading it through the SAME split-sum IBL +
// key/fill + Material branch as the beads (`shade_bead`). Unlike beads the
// geometry is per-instance (A, B, radius) and the colour is the strand's own
// tint — so a chain of these along an arm reads as a fleshy capped finger, and
// the gaps between arms come for free (nothing is drawn between them). No
// per-frame mesh: the whole finger is a handful of billboards.
//
// Instance layout (reuses the 3×vec4 particle vbuf): loc0 = A.xyz + radius,
// loc1 = B.xyz (+ unused), loc2 = colour.rgb (+ unused).

// Signed distance to a capsule: segment a→b, radius r.
fn sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

// ---------------------------------------------------------------------------
// PBR text T6 (#217) — the COAXIAL GLASS CAPSULE (doc/pbr_text_engine.md §11,
// route 1). A Glass/Refractive capsule used to refract the ENVIRONMENT: a glass
// tube showing you the sky. With `D.capsule.x > 0` it shows the glowing wire
// inside it instead: trace to the outer capsule as before (UNCHANGED — the depth
// it writes is the depth the FX prepass sees), refract the view ray at that
// surface, solve the refracted ray against an INNER capsule (same endpoints,
// radius = outer × core fraction) carrying the instance's emission, attenuate
// by Beer–Lambert over the glass path in the instance's colour, and Fresnel-
// compose with the environment reflection.
//
// Cost: NO extra march. The inner hit and the outer exit are analytic — a
// capsule is a convex union of a finite cylinder and two spheres, so its ray
// interval is [min of the pieces' entries, max of their exits]: three quadratics
// each, no loop. Beside the 96-step sphere trace that found the outer surface
// this is noise, and it runs only for Glass/Refractive fragments with the core on.
//
// 🚨 Inert by default: `D.capsule.x == 0` never reaches this code — `fs_capsule`
// calls today's `shade_bead` exactly as before, so the frame is pixel-identical.
// The Rust twin (`particles.rs::capsule_core`) pins the arithmetic.
// ---------------------------------------------------------------------------

// Ray interval through a capsule (segment a→b, radius r): (t_in, t_out) along
// `rd` from `ro`, or t_in > t_out when the ray misses. Correct for a ray that
// starts INSIDE (t_in ≤ 0 < t_out) — that is how the outer exit is found.
fn capsule_interval(ro: vec3<f32>, rd: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> vec2<f32> {
    let big = 1e30;
    var t_in = big;
    var t_out = -big;
    // Piece 1: the finite cylinder between the endpoint planes.
    let ba = b - a;
    let oa = ro - a;
    let baba = dot(ba, ba);
    let bard = dot(ba, rd);
    let baoa = dot(ba, oa);
    let rdoa = dot(rd, oa);
    let oaoa = dot(oa, oa);
    let qa = baba - bard * bard;
    let qb = baba * rdoa - baoa * bard;
    let qc = baba * oaoa - baoa * baoa - r * r * baba;
    let qh = qb * qb - qa * qc;
    // qa → 0 when the ray runs along the axis, or the capsule is a degenerate
    // A≈B node sphere: the wall test then yields nothing and the caps decide.
    if (qa > 1e-8 && qh >= 0.0) {
        let sq = sqrt(qh);
        let t1 = (-qb - sq) / qa;
        let t2 = (-qb + sq) / qa;
        let y1 = baoa + t1 * bard;
        let y2 = baoa + t2 * bard;
        if (y1 >= 0.0 && y1 <= baba) { t_in = min(t_in, t1); }
        if (y2 >= 0.0 && y2 <= baba) { t_out = max(t_out, t2); }
    }
    // Pieces 2 and 3: the end spheres. A ray crossing an endpoint disc is inside
    // that sphere, so the sphere's roots cover the disc crossing.
    for (var k = 0; k < 2; k = k + 1) {
        let oc = select(oa, ro - b, k == 1);
        let sb = dot(rd, oc);
        let sc = dot(oc, oc) - r * r;
        let sh = sb * sb - sc;
        if (sh >= 0.0) {
            let sq = sqrt(sh);
            t_in = min(t_in, -sb - sq);
            t_out = max(t_out, -sb + sq);
        }
    }
    return vec2<f32>(t_in, t_out);
}

// Beer–Lambert through the shell: absorb the complement of the instance colour
// (a red tube passes red), at `density` per OUTER radius so the look survives a
// scale change. ⚠️ Optical depth is clamped at 6 per channel (exp(-6) ≈ 0.25 %):
// a near-black tint over a long chord otherwise underflows to exactly zero and
// reads as a black tube rather than a dark one.
fn capsule_transmittance(albedo: vec3<f32>, path: f32, r_outer: f32, density: f32) -> vec3<f32> {
    let sigma = (vec3<f32>(1.0) - clamp(albedo, vec3<f32>(0.0), vec3<f32>(1.0)))
              * (max(density, 0.0) / max(r_outer, 1e-6));
    let od = min(sigma * max(path, 0.0), vec3<f32>(6.0));
    return exp(-od);
}

// The Glass/Refractive capsule with a visible core. Mirrors `shade_bead`'s Glass
// branch term for term — same Fresnel, same reflection, same murk, same roughness
// — and differs in exactly one place: what the transmitted ray SEES. Only called
// when `D.capsule.x > 0` and the material is Glass/Refractive. With the core on,
// the instance's self-emission (`emissive`) moves INSIDE the glass: a lit tube is
// a tube around a lit thing, not a glowing tube around a glowing thing.
fn shade_capsule_glass(in: CapOut, n: vec3<f32>, surf: vec3<f32>,
                       albedo: vec3<f32>, emissive: vec3<f32>) -> vec3<f32> {
    let v = normalize(D.cam_pos.xyz - surf);
    let n_dot_v = max(dot(n, v), 1e-4);
    let env_rot = D.env.z;
    let env_scale = D.env.y * D.env.w * D.env_tint.rgb;
    let roughness = clamp(D.bead.z, 0.02, 1.0);
    let ior = max(D.bead2.z, 1.0);
    let refl = sample_prefiltered(rotate_y(reflect(-v, n), env_rot), roughness);
    let f0s = (ior - 1.0) / (ior + 1.0);
    let f0g = f0s * f0s;
    let fr = f0g + (1.0 - f0g) * pow(1.0 - n_dot_v, 5.0);
    let murk = select(mix(vec3<f32>(1.0), albedo, 0.35),
                      mix(vec3<f32>(1.0), albedo, 0.7), D.bead2.x >= 2.5);
    let rdir = refract(-v, n, 1.0 / ior);
    // WGSL refract() returns vec3(0) on total internal reflection. Air→glass
    // (η = 1/ior ≤ 1) cannot TIR, so this guards against a bad normal rather than
    // a physical branch — but a zero direction solved against a capsule would be
    // silent garbage, so it falls back to today's reflection-only expression.
    if (dot(rdir, rdir) < 1e-6) {
        return mix(refl * murk, refl, fr) * env_scale + emissive;
    }
    // The glass path: from the outer surface along the refracted ray to the core
    // (hit) or out the far wall of the shell (miss). The origin is nudged inward
    // so the outer exit is the far wall, never the wall we are standing on.
    let ro = surf + rdir * (in.r * 1e-3);
    let outer = capsule_interval(ro, rdir, in.a, in.b, in.r);
    let core = capsule_interval(ro, rdir, in.a, in.b, in.r * clamp(D.capsule.x, 0.0, 1.0));
    let chord = max(outer.y, 0.0);
    let hit = core.x <= core.y && core.y > 0.0;
    let path = select(chord, min(max(core.x, 0.0), chord), hit);
    let trans = capsule_transmittance(albedo, path, in.r, D.capsule.y);
    var thru: vec3<f32>;
    if (hit) {
        thru = emissive * trans;
    } else {
        thru = sample_prefiltered(rotate_y(rdir, env_rot), roughness) * murk * trans * env_scale;
    }
    return refl * env_scale * fr + thru * (1.0 - fr);
}

struct CapIn {
    @builtin(vertex_index) vi: u32,
    @location(0) a_r: vec4<f32>,   // A.xyz, radius
    @location(1) b: vec4<f32>,     // B.xyz
    @location(2) color: vec4<f32>, // strand tint (rgb)
};

struct CapOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) a: vec3<f32>,     // capsule endpoint A (world)
    @location(2) b: vec3<f32>,     // capsule endpoint B (world)
    @location(3) center: vec3<f32>,// bounding-sphere centre (world)
    @location(4) r: f32,           // capsule radius (world)
    @location(5) rb: f32,          // bounding-sphere radius (world)
    @location(6) world: vec3<f32>, // billboard-plane world point (the ray target)
    @location(7) color: vec3<f32>, // strand tint
};

@vertex
fn vs_capsule(in: CapIn) -> CapOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[in.vi];
    let a = in.a_r.xyz;
    let b = in.b.xyz;
    let r = max(in.a_r.w, 1e-4);
    let center = (a + b) * 0.5;
    // Bounding sphere: half the segment length + the radius (+ a hair for the
    // sphere-trace step epsilon), so the whole capsule silhouette fits the quad.
    let rb = length(b - a) * 0.5 + r * 1.05;
    let world = center + (D.cam_right.xyz * c.x + D.cam_up.xyz * c.y) * rb;

    var out: CapOut;
    out.clip = D.view_proj * vec4<f32>(world, 1.0);
    out.uv = c;
    out.a = a;
    out.b = b;
    out.center = center;
    out.r = r;
    out.rb = rb;
    out.world = world;
    out.color = in.color.rgb;
    return out;
}

// Sphere-trace the capsule from the camera through this fragment; returns hit +
// world normal + surface point. Bounds the march by the billboard's bounding sphere.
struct CapHit { hit: bool, n: vec3<f32>, surf: vec3<f32> };
fn capsule_trace(in: CapOut) -> CapHit {
    var out: CapHit;
    out.hit = false;
    out.n = vec3<f32>(0.0, 0.0, 1.0);
    out.surf = in.center;
    let ro = D.cam_pos.xyz;
    let rd = normalize(in.world - ro);
    let oc = ro - in.center;
    let bq = dot(oc, rd);
    let cq = dot(oc, oc) - in.rb * in.rb;
    let disc = bq * bq - cq;
    if (disc < 0.0) { return out; }
    let sq = sqrt(disc);
    var t = max(-bq - sq, 0.0);
    let t_far = -bq + sq;
    let eps = in.r * 0.01;
    // 96 steps (up from 64): sphere-tracing under-steps at grazing/side angles,
    // dropping fragments → thin cracks between segments. More steps close most.
    for (var i = 0; i < 96; i = i + 1) {
        let wp = ro + rd * t;
        let d = sd_capsule(wp, in.a, in.b, in.r);
        if (d < eps) {
            out.hit = true;
            out.surf = wp;
            // SDF gradient (central differences) → world normal.
            let e = max(in.r * 0.02, 1e-3);
            let nx = sd_capsule(wp + vec3<f32>(e, 0.0, 0.0), in.a, in.b, in.r)
                   - sd_capsule(wp - vec3<f32>(e, 0.0, 0.0), in.a, in.b, in.r);
            let ny = sd_capsule(wp + vec3<f32>(0.0, e, 0.0), in.a, in.b, in.r)
                   - sd_capsule(wp - vec3<f32>(0.0, e, 0.0), in.a, in.b, in.r);
            let nz = sd_capsule(wp + vec3<f32>(0.0, 0.0, e), in.a, in.b, in.r)
                   - sd_capsule(wp - vec3<f32>(0.0, 0.0, e), in.a, in.b, in.r);
            out.n = normalize(vec3<f32>(nx, ny, nz));
            return out;
        }
        t = t + max(d, eps);
        if (t > t_far) { return out; }
    }
    return out;
}

@fragment
fn fs_capsule(in: CapOut) -> BeadFrag {
    let h = capsule_trace(in);
    if (!h.hit) { discard; }
    // Strand tint recoloured by the per-draw HSV (identity [0,1,1] = unchanged),
    // shaded by the shared IBL + key/fill + Material branch — matching the cube
    // path. Material parity: the same `emissive = albedo · glow` self-emission the
    // cube shader adds (`params.y` carries the Material Glow), so the Glow slider
    // drives the arms too (glow 0 = pure PBR pearls, like the beads).
    let alb = apply_hsv(in.color, D.bead_hsv);
    let emissive = alb * D.params.y;
    let clip = D.view_proj * vec4<f32>(h.surf, 1.0);
    // PBR text T6 (#217): a Glass/Refractive capsule with the core on shows the
    // wire through the shell; otherwise — and always when `D.capsule.x == 0` —
    // exactly today's shading. `capsule_trace`, and so the depth written, is the
    // same either way.
    var rgb: vec3<f32>;
    if (D.capsule.x > 0.0 && D.bead2.x >= 1.5) {
        rgb = shade_capsule_glass(in, h.n, h.surf, alb, emissive);
    } else {
        rgb = shade_bead(h.n, h.surf, alb, emissive);
    }
    var out: BeadFrag;
    out.color = vec4<f32>(max(rgb, vec3<f32>(0.0)), 1.0);
    out.depth = clip.z / max(clip.w, 1e-6);
    return out;
}

// Depth-only capsule draw for the FX depth prepass (mirrors fs_bead_depth).
@fragment
fn fs_capsule_depth(in: CapOut) -> @builtin(frag_depth) f32 {
    let h = capsule_trace(in);
    if (!h.hit) { discard; }
    let clip = D.view_proj * vec4<f32>(h.surf, 1.0);
    return clip.z / max(clip.w, 1e-6);
}

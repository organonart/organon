// Gaussian Splatting surface (SurfaceMode::Splat) — the 3DGS *primitive* used for
// forward synthesis. Organon already knows every node's position/orientation/scale/
// colour analytically (draw_tissue → per-node Mat4 + tint), so there is NO
// reconstruction/optimization step (that's the photogrammetry half of 3DGS). Each
// node becomes an anisotropic Gaussian directly: translation → centre μ, the 3×3
// rot·scale columns → the covariance basis (a,b,c = the σ-scaled semi-axes), tint →
// colour. `splat.rs` packs those on the CPU each frame.
//
// Two tiers, both drawn into the linear HDR scene buffer so they bloom + tonemap
// with everything else:
//   • Tier 1 `fs_splat_add` — additive, UNLIT anisotropic Gaussians (no sort). The
//     robust "made of light" look; blends src One / dst One.
//   • Tier 2 `fs_splat_lit` — the splat as a 2DGS oriented DISK: the disk normal
//     (the thin axis ĉ) rides the split-sum IBL + key/fill (the same math as the
//     cube/bead PBR), alpha-composited back-to-front (splat.rs sorts the instances
//     by view depth each frame). Binds group(1) = the shared IBL, exactly like beads.
//
// The footprint is evaluated as the ellipsoid's screen-plane CUT through μ (the
// fragment lives on the camera-facing billboard, so d = world − μ is purely in the
// screen plane; the Mahalanobis distance uses the world-space axes). It's a cut,
// not the true EWA integral-projection, but it is always finite, needs no 2×2
// inverse in-shader, and reads as an oriented anisotropic blob — a deliberate,
// GPU-robust simplification (a true-EWA pass is a follow-up).

struct SplatInst {
    mu:  vec4<f32>,  // xyz = centre, w = per-splat opacity (already × global opacity)
    ax:  vec4<f32>,  // xyz = σ-scaled semi-axis a (world), w unused
    ay:  vec4<f32>,  // xyz = σ-scaled semi-axis b (world), w unused
    az:  vec4<f32>,  // xyz = σ-scaled semi-axis c (world) — the disk-normal axis, w unused
    col: vec4<f32>,  // rgb = colour, w unused
};

struct SplatU {
    view_proj: mat4x4<f32>, // world → clip (unscaled; splats live in world space)
    cam_right: vec4<f32>,   // world-space camera right (billboard basis)
    cam_up:    vec4<f32>,   // world-space camera up
    cam_pos:   vec4<f32>,   // xyz = camera world pos, w = prefilter mip count
    params:    vec4<f32>,   // x=falloff, y=opacity scale, z=cutoff, w=mode(0 add/1 lit)
    // Tier 2 lighting context (filled from the cube Uniforms in render.rs):
    key_light:  vec4<f32>,  // xyz = world dir TO key light (unit), w = intensity
    fill_light: vec4<f32>,  // xyz = world dir TO fill light (unit), w = intensity
    env:        vec4<f32>,  // x unused, y = env_intensity, z = env_rotation, w = ambient_mul
    env_tint:   vec4<f32>,  // xyz = environment tint (white = none), w unused
    mat:        vec4<f32>,  // x = metallic, y = roughness, z = material_type, w = ior
    skyrefl:    vec4<f32>,  // #305 live-sky cloud reflections: x=enable, y=cover, z=drift, w=strength
    params2:    vec4<f32>,  // Tier 3: x = solidity (Gaussian→opaque disc), y/z/w unused
};

@group(0) @binding(0) var<uniform> U: SplatU;

// ----- group(1): the shared split-sum IBL (mirrors cube.wgsl / particles.wgsl) -----
// Bound only for the lit (Tier 2) pipeline; the additive pipeline leaves it unbound.
@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

const PI: f32 = 3.14159265359;
const INV_ATAN: vec2<f32> = vec2<f32>(0.1591, 0.3183);

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
// cube.wgsl / particles.wgsl). Returns a brightness multiplier; identity when disabled.
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
    let mip_count = U.cam_pos.w;
    let lod = roughness * max(mip_count - 1.0, 0.0);
    let base = textureSampleLevel(prefilter_tex, ibl_samp, dir_to_equirect_uv(r_env), lod).rgb;
    return base * cloud_bright(r_env, roughness, U.skyrefl);
}

struct VsIn {
    @builtin(vertex_index) vi: u32,
    @location(0) mu:  vec4<f32>,
    @location(1) ax:  vec4<f32>,
    @location(2) ay:  vec4<f32>,
    @location(3) az:  vec4<f32>,
    @location(4) col: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:    vec2<f32>, // billboard corner in [-1,1]²
    @location(1) mu:    vec3<f32>, // splat centre (world)
    @location(2) ax:    vec3<f32>, // σ-scaled semi-axes (world)
    @location(3) ay:    vec3<f32>,
    @location(4) az:    vec3<f32>,
    @location(5) col:   vec3<f32>,
    @location(6) world: vec3<f32>, // this fragment's billboard-plane world position
    @location(7) opacity: f32,     // per-splat opacity (mu.w)
};

// Gaussian tail reach: the billboard must contain the ellipse out to ~3σ.
const REACH: f32 = 3.0;

@vertex
fn vs_splat(in: VsIn) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[in.vi];
    let mu = in.mu.xyz;
    // World-space bound = the largest σ-axis, extended to the Gaussian tail. A square
    // camera-facing billboard of this half-extent always contains the projected ellipse.
    let r = max(length(in.ax.xyz), max(length(in.ay.xyz), length(in.az.xyz)));
    let extent = max(r, 1e-5) * REACH;
    let world = mu + (U.cam_right.xyz * c.x + U.cam_up.xyz * c.y) * extent;

    var out: VsOut;
    out.clip = U.view_proj * vec4<f32>(world, 1.0);
    out.uv = c;
    out.mu = mu;
    out.ax = in.ax.xyz;
    out.ay = in.ay.xyz;
    out.az = in.az.xyz;
    out.col = in.col.rgb;
    out.world = world;
    out.opacity = in.mu.w;
    return out;
}

// Anisotropic Gaussian weight at this fragment: the Mahalanobis distance of the
// screen-plane point relative to the ellipsoid basis (a,b,c are σ-scaled, so the
// component along axis a in σ-units is dot(d,a)/|a|²). Returns exp(-½·m²·falloff).
fn splat_weight(in: VsOut) -> f32 {
    let d = in.world - in.mu;
    let la2 = max(dot(in.ax, in.ax), 1e-12);
    let lb2 = max(dot(in.ay, in.ay), 1e-12);
    let lc2 = max(dot(in.az, in.az), 1e-12);
    let da = dot(d, in.ax) / la2;
    let db = dot(d, in.ay) / lb2;
    let dc = dot(d, in.az) / lc2;
    let m2 = da * da + db * db + dc * dc;
    return exp(-0.5 * m2 * max(U.params.x, 1e-3));
}

// Tier 3 solidity: remap the soft Gaussian weight `w` toward a flat-topped, sharp-edged
// opaque disc. `solid = 0` → identity (the original soft Gaussian). As `solid → 1` the
// transition band around the w = 0.5 iso-contour narrows to a hard step, so the splat
// reads as an opaque disc (radius = where the Gaussian crosses 0.5) with a crisp,
// slightly-feathered rim — overlapping opaque discs occlude into a compact surface
// instead of blurring into bokeh.
fn splat_coverage(w: f32) -> f32 {
    let solid = clamp(U.params2.x, 0.0, 1.0);
    if (solid <= 0.0) { return w; }
    let k = mix(0.5, 0.02, solid);      // band half-width: 0.5 (soft) → 0.02 (hard disc)
    let sharp = smoothstep(0.5 - k, 0.5 + k, w);
    return mix(w, sharp, solid);
}

// ----- Tier 1: additive, unlit. Premultiplied glow (src One / dst One). -----
@fragment
fn fs_splat_add(in: VsOut) -> @location(0) vec4<f32> {
    let w = splat_coverage(splat_weight(in));
    let alpha = w * in.opacity * U.params.y;
    if (alpha < U.params.z) { discard; }
    return vec4<f32>(in.col * alpha, alpha);
}

// Tier 3 relightable material shading (mirrors particles.wgsl::shade_bead + cube.wgsl):
// branch on the Material card's `material_type` so lit splats reflect (Chrome) and
// refract (Glass/Refractive) the environment like the cubes, instead of being
// Standard-only. `n` = disk normal (camera-facing), `surf` = splat centre.
fn shade_splat(n: vec3<f32>, surf: vec3<f32>, albedo: vec3<f32>) -> vec3<f32> {
    let v = normalize(U.cam_pos.xyz - surf);
    let n_dot_v = max(dot(n, v), 1e-4);
    let mat = U.mat.z; // material_type: 0 Standard, 1 Chrome, 2 Glass, 3 Refractive
    let env_rot = U.env.z;
    let ei = U.env.y;
    let am = U.env.w;
    let et = U.env_tint.rgb;
    let n_env = rotate_y(n, env_rot);

    // ---- Glass / Refractive: Fresnel-blended env reflect + refract ----
    if (mat >= 1.5 && mat < 3.5) {
        let roughness = clamp(U.mat.y, 0.02, 1.0);
        let ior = max(U.mat.w, 1.0);
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
        // Refractive (≥ 2.5) murks the transmission harder in the splat's colour.
        let murk = select(mix(vec3<f32>(1.0), albedo, 0.35),
                          mix(vec3<f32>(1.0), albedo, 0.7), mat >= 2.5);
        return mix(thru * murk, refl, fr) * ei * am * et;
    }

    // ---- Standard (albedo PBR) / Chrome (mirror): metallic-roughness ----
    let is_chrome = (mat >= 0.5 && mat < 1.5);
    let metallic = select(clamp(U.mat.x, 0.0, 1.0), 1.0, is_chrome);
    let roughness = select(clamp(U.mat.y, 0.04, 1.0),
                           min(clamp(U.mat.y, 0.04, 1.0), 0.08), is_chrome);
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
    let l_key = normalize(U.key_light.xyz);
    let l_fill = normalize(U.fill_light.xyz);
    let direct = direct_light(n, v, l_key, albedo, metallic, roughness, f0, vec3<f32>(U.key_light.w))
               + direct_light(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(U.fill_light.w));
    return max(ambient + direct, vec3<f32>(0.0));
}

// ----- Tier 2/3: lit 2DGS oriented disk, alpha-composited (src alpha / 1−src alpha). -----
// Instances are pre-sorted back-to-front on the CPU each frame.
@fragment
fn fs_splat_lit(in: VsOut) -> @location(0) vec4<f32> {
    let w = splat_coverage(splat_weight(in));
    let alpha = clamp(w * in.opacity * U.params.y, 0.0, 1.0);
    if (alpha < U.params.z) { discard; }

    // Disk normal = the thin (c) axis, flipped to face the camera.
    let v = normalize(U.cam_pos.xyz - in.mu);
    var n = normalize(in.az);
    if (dot(n, v) < 0.0) { n = -n; }

    let lit = shade_splat(n, in.mu, in.col);
    return vec4<f32>(lit * alpha, alpha); // premultiplied — pairs with One / 1−src_alpha
}

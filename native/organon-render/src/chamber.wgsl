// Field Chamber (#346) Tier 1 — flat analyzer panels drawn in the scene pass:
// the oscilloscope ribbon + spectrum bars (lit surface) and the panel frames (lines).
// Geometry is already in world space; group(0) is the shared scene camera.

struct Cam {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Cam;

// ---- Surface (ribbon + bars): lit flat ----
struct SurfIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};
struct SurfOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_surf(in: SurfIn) -> SurfOut {
    var o: SurfOut;
    o.pos = cam.view_proj * vec4<f32>(in.pos, 1.0);
    o.normal = in.normal;
    o.color = in.color;
    return o;
}

@fragment
fn fs_surf(in: SurfOut) -> @location(0) vec4<f32> {
    // A soft key so the panels read as surfaces without stealing focus from the field.
    let n = normalize(in.normal);
    let l = normalize(vec3<f32>(0.35, 0.75, 0.55));
    let diff = max(dot(n, l), 0.0);
    let shade = 0.65 + 0.45 * diff;
    return vec4<f32>(in.color.rgb * shade, in.color.a);
}

// ---- Lines (frames + grid): flat ----
struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};
struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(in: LineIn) -> LineOut {
    var o: LineOut;
    o.pos = cam.view_proj * vec4<f32>(in.pos, 1.0);
    o.color = in.color;
    return o;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}

// ===========================================================================
// Tier 2 (#346) — impostor rounded lines: each scope segment / spectrum bar is a
// camera-facing billboard inside which we sphere-trace a CAPSULE SDF, shaded with the
// shared split-sum IBL + key/fill and a MaterialType (Standard / Chrome / Glass /
// metallic, env-only reflection) — the #298 bead technique. Writes frag_depth so the
// panels depth-occlude correctly and (via the depth-only entry) join the FX prepass.
// The IBL helpers mirror particles.wgsl (proven cube/bead code).
// ===========================================================================

struct ChD {
    view_proj: mat4x4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_pos: vec4<f32>,   // xyz = camera world pos, w = prefilter mip count
    key_light: vec4<f32>, // xyz = dir TO key light, w = intensity
    fill_light: vec4<f32>,// xyz = dir TO fill light, w = intensity
    env: vec4<f32>,       // exposure(unused), env_intensity, env_rotation, ambient_mul
    env_tint: vec4<f32>,  // xyz = env tint (white = none)
    material: vec4<f32>,  // mat_type, metallic, roughness, ior
    opts: vec4<f32>,      // x = opacity, y..w reserved
};
@group(0) @binding(0) var<uniform> D: ChD;

@group(1) @binding(0) var irradiance_tex : texture_2d<f32>;
@group(1) @binding(1) var prefilter_tex  : texture_2d<f32>;
@group(1) @binding(2) var brdf_lut_tex   : texture_2d<f32>;
@group(1) @binding(3) var ibl_samp       : sampler;

const CH_PI: f32 = 3.14159265359;
const CH_INV_ATAN: vec2<f32> = vec2<f32>(0.1591, 0.3183);

fn ch_dir_to_uv(dir: vec3<f32>) -> vec2<f32> {
    let d = normalize(dir);
    var uv = vec2<f32>(atan2(d.z, d.x), asin(clamp(d.y, -1.0, 1.0)));
    uv = uv * CH_INV_ATAN + vec2<f32>(0.5, 0.5);
    return uv;
}
fn ch_rotate_y(d: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
}
fn ch_fresnel_rough(cos_theta: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let one_minus_r = vec3<f32>(1.0 - roughness);
    return f0 + (max(one_minus_r, f0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}
fn ch_fresnel(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}
fn ch_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = max(roughness * roughness, 4e-4);
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (CH_PI * denom * denom);
}
fn ch_g_schlick(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}
fn ch_g_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    return ch_g_schlick(max(dot(n, v), 0.0), roughness) * ch_g_schlick(max(dot(n, l), 0.0), roughness);
}
fn ch_direct(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if (n_dot_l <= 0.0) { return vec3<f32>(0.0); }
    let h = normalize(v + l);
    let f = ch_fresnel(max(dot(h, v), 0.0), f0);
    let d = ch_ggx(n, h, roughness);
    let g = ch_g_smith(n, v, l, roughness);
    let spec = (d * f * g) / (4.0 * max(dot(n, v), 1e-4) * n_dot_l + 1e-4);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    return (kd * albedo / CH_PI + spec) * radiance * n_dot_l;
}
fn ch_irradiance(n_env: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(irradiance_tex, ibl_samp, ch_dir_to_uv(n_env), 0.0).rgb;
}
fn ch_prefiltered(r_env: vec3<f32>, roughness: f32) -> vec3<f32> {
    let lod = roughness * max(D.cam_pos.w - 1.0, 0.0);
    return textureSampleLevel(prefilter_tex, ibl_samp, ch_dir_to_uv(r_env), lod).rgb;
}

// Orthonormal frame with +z = `axis` (the capsule's long axis).
fn ch_frame(axis: vec3<f32>) -> mat3x3<f32> {
    let sp = length(axis);
    let az = select(vec3<f32>(0.0, 0.0, 1.0), axis / max(sp, 1e-6), sp > 1e-4);
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(az.y) > 0.9);
    let ax = normalize(cross(up, az));
    let ay = cross(az, ax);
    return mat3x3<f32>(ax, ay, az);
}
// Capsule SDF along local z from −hl..+hl, radius r (world units).
fn ch_capsule(p: vec3<f32>, hl: f32, r: f32) -> f32 {
    var q = p;
    q.z = q.z - clamp(q.z, -hl, hl);
    return length(q) - r;
}
fn ch_capsule_n(p: vec3<f32>, hl: f32, r: f32) -> vec3<f32> {
    let e = 0.0008;
    let dx = vec3<f32>(e, 0.0, 0.0);
    let dy = vec3<f32>(0.0, e, 0.0);
    let dz = vec3<f32>(0.0, 0.0, e);
    return normalize(vec3<f32>(
        ch_capsule(p + dx, hl, r) - ch_capsule(p - dx, hl, r),
        ch_capsule(p + dy, hl, r) - ch_capsule(p - dy, hl, r),
        ch_capsule(p + dz, hl, r) - ch_capsule(p - dz, hl, r),
    ));
}

struct ImpIn {
    @builtin(vertex_index) vi: u32,
    @location(0) center: vec4<f32>, // xyz + radius
    @location(1) axis: vec4<f32>,   // xyz + half_len
    @location(2) color: vec4<f32>,  // albedo
    @location(3) emissive: vec4<f32>,
};
struct ImpOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) center: vec3<f32>,
    @location(1) radius: f32,
    @location(2) axis: vec3<f32>,
    @location(3) half_len: f32,
    @location(4) albedo: vec3<f32>,
    @location(5) emissive: f32,
    @location(6) world: vec3<f32>, // billboard-plane target (the SDF ray target)
};

@vertex
fn vs_imp(in: ImpIn) -> ImpOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[in.vi];
    let center = in.center.xyz;
    let radius = in.center.w;
    let half_len = in.axis.w;
    // Bounding radius covers the capsule's extent from any view.
    let rb = half_len + radius + 1e-4;
    let world = center + (D.cam_right.xyz * c.x + D.cam_up.xyz * c.y) * rb;
    var out: ImpOut;
    out.clip = D.view_proj * vec4<f32>(world, 1.0);
    out.center = center;
    out.radius = radius;
    out.axis = in.axis.xyz;
    out.half_len = half_len;
    out.albedo = in.color.rgb;
    out.emissive = in.emissive.x;
    out.world = world;
    return out;
}

struct ImpHit {
    hit: bool,
    n: vec3<f32>,
    surf: vec3<f32>,
};
fn ch_trace(in: ImpOut) -> ImpHit {
    var out: ImpHit;
    out.hit = false;
    out.n = vec3<f32>(0.0, 0.0, 1.0);
    out.surf = in.center;
    let ro = D.cam_pos.xyz;
    let rd = normalize(in.world - ro);
    let rb = in.half_len + in.radius + 1e-4;
    let oc = ro - in.center;
    let b = dot(oc, rd);
    let cc = dot(oc, oc) - rb * rb;
    let disc = b * b - cc;
    if (disc < 0.0) { return out; }
    let sq = sqrt(disc);
    var t = max(-b - sq, 0.0);
    let t_far = -b + sq;
    let frame = ch_frame(in.axis);
    let frame_t = transpose(frame);
    let eps = max(in.radius * 0.02, 1e-4);
    for (var i = 0; i < 64; i = i + 1) {
        let wp = ro + rd * t;
        let lp = frame_t * (wp - in.center);
        let d = ch_capsule(lp, in.half_len, in.radius);
        if (d < eps) {
            out.hit = true;
            out.surf = wp;
            out.n = normalize(frame * ch_capsule_n(lp, in.half_len, in.radius));
            return out;
        }
        t = t + max(d, eps);
        if (t > t_far) { return out; }
    }
    return out;
}

fn ch_shade(n: vec3<f32>, surf: vec3<f32>, albedo: vec3<f32>, emissive: vec3<f32>) -> vec3<f32> {
    let v = normalize(D.cam_pos.xyz - surf);
    let n_dot_v = max(dot(n, v), 1e-4);
    let mat = D.material.x;
    let env_rot = D.env.z;
    let ei = D.env.y;
    let am = D.env.w;
    let et = D.env_tint.rgb;
    let n_env = ch_rotate_y(n, env_rot);

    // Glass / Refractive: Fresnel-blended env reflect + refract (env-only).
    if (mat >= 1.5) {
        let roughness = clamp(D.material.z, 0.02, 1.0);
        let ior = max(D.material.w, 1.0);
        let r_env = ch_rotate_y(reflect(-v, n), env_rot);
        let refl = ch_prefiltered(r_env, roughness);
        let rdir = refract(-v, n, 1.0 / ior);
        var thru: vec3<f32>;
        if (dot(rdir, rdir) < 1e-6) {
            thru = refl;
        } else {
            thru = ch_prefiltered(ch_rotate_y(rdir, env_rot), roughness);
        }
        let f0s = (ior - 1.0) / (ior + 1.0);
        let f0g = f0s * f0s;
        let fr = f0g + (1.0 - f0g) * pow(1.0 - n_dot_v, 5.0);
        let murk = select(mix(vec3<f32>(1.0), albedo, 0.35), mix(vec3<f32>(1.0), albedo, 0.7), mat >= 2.5);
        return mix(thru * murk, refl, fr) * ei * am * et + emissive;
    }

    // Standard / Chrome: metallic-roughness PBR.
    let is_chrome = mat >= 0.5;
    let metallic = select(clamp(D.material.y, 0.0, 1.0), 1.0, is_chrome);
    let roughness = select(clamp(D.material.z, 0.02, 1.0), min(clamp(D.material.z, 0.02, 1.0), 0.08), is_chrome);
    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let r_env = ch_rotate_y(reflect(-v, n), env_rot);
    let f = ch_fresnel_rough(n_dot_v, f0, roughness);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let diffuse = ch_irradiance(n_env) * albedo;
    let prefiltered = ch_prefiltered(r_env, roughness);
    let brdf = textureSampleLevel(brdf_lut_tex, ibl_samp,
        vec2<f32>(min(n_dot_v, 1.0 - 0.5 / 256.0), max(roughness, 1e-3)), 0.0).rg;
    let specular = prefiltered * (f0 * brdf.x + vec3<f32>(brdf.y));
    let ambient = (kd * diffuse + specular) * ei * am * et;
    let l_key = normalize(D.key_light.xyz);
    let l_fill = normalize(D.fill_light.xyz);
    let direct = ch_direct(n, v, l_key, albedo, metallic, roughness, f0, vec3<f32>(D.key_light.w))
               + ch_direct(n, v, l_fill, albedo, metallic, roughness, f0, vec3<f32>(D.fill_light.w));
    return ambient + direct + emissive;
}

struct ImpFrag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_imp(in: ImpOut) -> ImpFrag {
    let h = ch_trace(in);
    if (!h.hit) { discard; }
    let clip = D.view_proj * vec4<f32>(h.surf, 1.0);
    let emi = in.albedo * in.emissive;
    var out: ImpFrag;
    out.color = vec4<f32>(max(ch_shade(h.n, h.surf, in.albedo, emi), vec3<f32>(0.0)), D.opts.x);
    out.depth = clip.z / max(clip.w, 1e-6);
    return out;
}

// Depth-only entry for the FX depth prepass (SSAO / SSR / SSGI join).
@fragment
fn fs_imp_depth(in: ImpOut) -> @builtin(frag_depth) f32 {
    let h = ch_trace(in);
    if (!h.hit) { discard; }
    let clip = D.view_proj * vec4<f32>(h.surf, 1.0);
    return clip.z / max(clip.w, 1e-6);
}

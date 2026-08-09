// Organic Math — HDR starfield + sun.
//
// Two passes drawn as a BACKGROUND into the linear-HDR scene buffer (far-plane
// depth, additive blend) behind the generator:
//   • vs_star/fs_star — instanced point-sprite billboards, one per Bright Star
//     Catalog entry. Each star's fixed equatorial unit vector is rotated into world
//     space by R(latitude, sidereal-time), projected as a point-at-infinity through
//     the scene view-projection (so the orbit camera frames it), and drawn as a soft
//     additive disc whose linear-HDR brightness comes from the V-magnitude (bright
//     stars exceed 1 → they bloom + use the EDR headroom). Per-star twinkle + a
//     magnitude-limit density cull + a colour-saturation control.
//   • vs_sun/fs_sun — a single broad HDR sun disc at the day-cycle sun direction.
//
// All output is LINEAR HDR; the composite owns exposure/tonemap. The night factor
// (driven CPU-side from the sun's elevation) fades the stars in as the sun sets.

struct StarU {
    view_proj: mat4x4<f32>, // unscaled scene view-projection
    rot: mat4x4<f32>,       // equatorial → world (3x3 upper-left)
    params: vec4<f32>,      // brightness, star_size(px), night_factor, mag_limit
    twinkle: vec4<f32>,     // amount, speed, time(s), saturation
    viewport: vec4<f32>,    // width, height, _, _
    sun: vec4<f32>,         // xyz = sun world dir, w = sun NDC radius (vertical)
    sun_color: vec4<f32>,   // rgb = tint, w = HDR brightness (<=0 → off)
};
@group(0) @binding(0) var<uniform> u: StarU;

// Hash → [0,1) for a per-star twinkle phase (Hugo Elias / pcg-ish float hash).
fn hash11(p: f32) -> f32 {
    var x = fract(p * 0.1031);
    x = x * (x + 33.33);
    x = x * (x + x);
    return fract(x);
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,    // quad-local [-1,1] for the radial falloff
    @location(1) col: vec3<f32>,   // premultiplied HDR colour
};

const OFFSCREEN: vec4<f32> = vec4<f32>(2.0, 2.0, 2.0, 1.0); // degenerate (culled)

@vertex
fn vs_star(
    @location(0) corner: vec2<f32>,
    @location(1) dir: vec3<f32>,
    @location(2) mag: f32,
    @location(3) color: vec3<f32>,
    @builtin(instance_index) idx: u32,
) -> VsOut {
    var out: VsOut;
    out.uv = corner;
    out.col = vec3<f32>(0.0);
    // Magnitude-limit density cull (also drops the blank slots at mag = 99) +
    // fully-faded night gate.
    if (mag > u.params.w || u.params.z <= 0.0) {
        out.pos = OFFSCREEN;
        return out;
    }
    // Equatorial → world, then project the point-at-infinity (w = 0) so only the
    // camera orientation matters (translation cancels), matching the skybox.
    let world = (u.rot * vec4<f32>(dir, 0.0)).xyz;
    // Stars below the celestial horizon are "under the ground" — cull them so they
    // don't shine through the terrain, with a soft fade right at the horizon.
    if (world.y < -0.02) {
        out.pos = OFFSCREEN;
        return out;
    }
    let horizon = smoothstep(-0.02, 0.06, world.y);
    let clip = u.view_proj * vec4<f32>(world, 0.0);
    if (clip.w <= 0.0) { // behind the camera
        out.pos = OFFSCREEN;
        return out;
    }
    let ndc = clip.xy / clip.w;

    // Twinkle: per-star phase from the index, modulating brightness + a little size.
    let ph = hash11(f32(idx) + 0.5) * 6.2831853;
    let tw = 1.0 + u.twinkle.x * sin(u.twinkle.z * u.twinkle.y + ph);

    // Magnitude → linear flux (each mag step ≈ ×2.512); MAG_REF ≈ 1 puts first-
    // magnitude stars near unit brightness so brighter ones bloom past it.
    let flux = pow(10.0, -0.4 * (mag - 1.0));
    let inten = u.params.x * flux * u.params.z * max(tw, 0.0) * horizon;

    // Sprite radius (px → NDC). Brighter stars read a touch larger; twinkle wobbles it.
    let size_px = u.params.y * (0.7 + 0.6 * clamp(flux, 0.0, 3.0)) * (0.85 + 0.15 * tw);
    let off = corner * size_px / u.viewport.xy * 2.0; // NDC half-extent (screen = 2)
    out.pos = vec4<f32>(ndc + off, 1.0, 1.0); // far-plane depth

    // Colour: desaturate toward white by the saturation control, scaled by flux.
    let tint = mix(vec3<f32>(1.0), color, clamp(u.twinkle.w, 0.0, 1.0));
    out.col = tint * inten;
    return out;
}

@fragment
fn fs_star(in: VsOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    // Soft point: a tight core inside a broad glow, premultiplied into rgb.
    let core = smoothstep(1.0, 0.0, r);
    let rgb = in.col * (0.45 * core + 0.55 * pow(core, 4.0));
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), core);
}

// --- HDR sun ---------------------------------------------------------------

@vertex
fn vs_sun(@builtin(vertex_index) vid: u32) -> VsOut {
    var out: VsOut;
    // Unit quad from the vertex index (two triangles).
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vid];
    out.uv = corner;
    out.col = u.sun_color.rgb * u.sun_color.w;
    if (u.sun_color.w <= 0.0) { // sun off / below horizon
        out.pos = OFFSCREEN;
        return out;
    }
    let clip = u.view_proj * vec4<f32>(u.sun.xyz, 0.0);
    if (clip.w <= 0.0) {
        out.pos = OFFSCREEN;
        return out;
    }
    let ndc = clip.xy / clip.w;
    // sun.w is the vertical NDC radius; correct x by the aspect ratio.
    let aspect = u.viewport.x / max(u.viewport.y, 1.0);
    let off = corner * vec2<f32>(u.sun.w / aspect, u.sun.w);
    out.pos = vec4<f32>(ndc + off, 1.0, 1.0);
    return out;
}

@fragment
fn fs_sun(in: VsOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    // A bright, slightly-clamped disc with a soft limb + a wide outer glow.
    let disc = smoothstep(0.55, 0.40, r);          // solid core
    let halo = pow(max(1.0 - r, 0.0), 3.0);        // falloff corona
    let rgb = in.col * (disc + 0.5 * halo);
    return vec4<f32>(max(rgb, vec3<f32>(0.0)), disc + halo);
}

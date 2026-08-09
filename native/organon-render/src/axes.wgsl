// Capture decoration (#135 Phase 5): the XYZ axes are shaded TUBES + conical arrowheads
// (a triangle surface, lit), and the bounding box is gridded back-walls (a line list).
// Both share the scene camera + depth buffer so they sit correctly in 3-D.

struct Cam {
    view_proj: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Cam;

// ---- Surface (tubes + cones): lit ----
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
    o.normal = in.normal; // geometry is already in world space (no model transform)
    o.color = in.color;
    return o;
}

@fragment
fn fs_surf(in: SurfOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let l = normalize(vec3<f32>(0.4, 0.85, 0.55));
    let diff = max(dot(n, l), 0.0);
    let shade = 0.4 + 0.7 * diff; // ambient + lambert
    return vec4<f32>(in.color.rgb * shade, in.color.a);
}

// ---- Lines (box walls): flat ----
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

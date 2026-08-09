//! Field Chamber (#346): the analyzer **panels** hung on the reference box's back
//! walls — the **oscilloscope** on the rear −Z wall (time) and the **spectrum** on the
//! right +X wall (frequency), so the Duo-Field sits inside a time × frequency frame.
//!
//! This mirrors `axes.rs`: pure geometry builders (`select_walls`, `build_frames`,
//! `build_scope`, `build_spectrum`) that are unit-tested, plus a small GPU object
//! (`Chamber`) that draws them **inside the scene pass** (shares the camera + depth
//! buffer, so the panels are depth-correct against the field). Panels are drawn only on
//! **back-facing** walls (reusing `AxesConfig.eye`) so they never occlude the field.
//!
//! Tier 1 (`PanelStyle::Flat`) is the cheap 2-D composite drawn here: a thin ribbon for
//! the scope, flat quads for the spectrum bars, a gridded frame. Tier 2
//! (`PanelStyle::Impostor`, see `fs_bead` in `particles.wgsl`) adds rounded material
//! lines — its extra pipeline binds the IBL group and is wired from `render.rs`.
//!
//! Compiled only by the visual binary (via `#[path]`), like `render.rs`.

use glam::Vec3;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// A lit surface vertex (ribbon + bars).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug, PartialEq)]
pub struct ChVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

/// A flat line vertex (frame borders + grid).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug, PartialEq)]
pub struct ChLine {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

/// Tier 2 impostor instance: one oriented **capsule** (a rounded rod) — a scope-tube
/// segment or a spectrum bar. Sphere-traced inside a camera-facing billboard, shaded by
/// the shared IBL + a MaterialType. 64 bytes (4× vec4) for a clean instance stride.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug, PartialEq)]
pub struct ChBead {
    pub center: [f32; 4],   // xyz world centre + w = radius
    pub axis: [f32; 4],     // xyz unit long-axis + w = half-length
    pub color: [f32; 4],    // albedo (a unused)
    pub emissive: [f32; 4], // x = emissive scale
}

/// The scene PBR/IBL + camera context handed to the impostor pass each frame (built in
/// `render.rs` from the cube `Uniforms`, mirroring `ParticleShade` for the beads).
#[derive(Clone, Copy, Debug)]
pub struct ImpostorFrame {
    pub view_proj: [[f32; 4]; 4],
    pub cam_right: [f32; 3],
    pub cam_up: [f32; 3],
    pub cam_pos: [f32; 3],
    pub prefilter_mips: f32,
    pub key_light: [f32; 4],
    pub fill_light: [f32; 4],
    pub env: [f32; 4],      // exposure(unused), env_intensity, env_rotation, ambient_mul
    pub env_tint: [f32; 3],
    pub material: [f32; 4], // mat_type, metallic, roughness, ior
    pub opacity: f32,
}

/// GPU-side mirror of `chamber.wgsl`'s `ChD` uniform.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ChDRaw {
    view_proj: [[f32; 4]; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    cam_pos: [f32; 4],
    key_light: [f32; 4],
    fill_light: [f32; 4],
    env: [f32; 4],
    env_tint: [f32; 4],
    material: [f32; 4],
    opts: [f32; 4],
}

/// Everything the panel builders need (filled from `Shared.chamber` + `Shared.axes` by
/// the visual). The waveform + spectrum data are passed to the builders separately.
#[derive(Clone, Copy, Debug)]
pub struct ChamberConfig {
    pub on: bool,
    pub style: u32, // 0 Flat, 1 Impostor
    pub rear_on: bool,
    pub right_on: bool,
    pub extent: f32,   // box half-size (the wall position; = AxesConfig.extent)
    pub eye: [f32; 3], // camera position → chooses the back-facing walls
    pub opacity: f32,
    pub fill: f32, // 0..1 wall inset
    pub scope_amp: f32,
    pub db_floor: f32,
    pub db_top: f32,
    pub thickness: f32, // ribbon/bar half-width as a fraction of the panel half-size
    pub emissive: f32,
    pub wall_relative: bool,
    pub frame_color: [f32; 4], // border/grid tint (reuses the box colour)
}

/// One back-wall panel: an oriented rectangle in world space. `center` is the wall
/// plane point, `u`/`v` the horizontal/vertical unit axes, `n` the into-room normal,
/// `h` the half-size (already inset by `fill`).
#[derive(Clone, Copy, Debug)]
pub struct Panel {
    pub center: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    pub n: Vec3,
    pub h: f32,
}

impl Panel {
    /// Local (lu, lv) ∈ [−1, 1]² → world, nudged `eps` off the wall toward the room so
    /// the panel never z-fights the wall grid.
    #[inline]
    fn at(&self, lu: f32, lv: f32, eps: f32) -> [f32; 3] {
        (self.center + self.u * (lu * self.h) + self.v * (lv * self.h) + self.n * eps).into()
    }
}

/// Pick the rear (time) + right (frequency) panels for this camera. In **fixed** mode
/// the time panel lives on the −Z wall (shown only when it is back-facing, i.e. the
/// camera is on the +Z side) and the frequency panel on the +X wall (shown when the
/// camera is on the −X side). In **camera-relative** mode each rides whichever Z / X
/// wall is currently back-facing, so both are always visible. Returns `(rear, right)`.
pub fn select_walls(cfg: &ChamberConfig) -> (Option<Panel>, Option<Panel>) {
    if !cfg.on || cfg.extent <= 0.0 {
        return (None, None);
    }
    let e = cfg.extent;
    let h = (e * cfg.fill.clamp(0.05, 1.0)).max(0.001);

    // Rear = oscilloscope on a Z wall (u = +X time, v = +Y amplitude).
    let rear = if cfg.rear_on {
        // Back-facing Z wall = the one on the opposite side of the camera.
        let zc = if cfg.eye[2] >= 0.0 { -e } else { e };
        // Fixed mode pins it to −Z; only draw when −Z is actually the back wall.
        let show = cfg.wall_relative || cfg.eye[2] >= 0.0;
        if show {
            Some(Panel {
                center: Vec3::new(0.0, 0.0, zc),
                u: Vec3::X,
                v: Vec3::Y,
                n: Vec3::new(0.0, 0.0, -zc.signum()),
                h,
            })
        } else {
            None
        }
    } else {
        None
    };

    // Right = spectrum on an X wall (u = +Z frequency, v = +Y magnitude).
    let right = if cfg.right_on {
        let xc = if cfg.eye[0] >= 0.0 { -e } else { e };
        // Fixed mode pins it to +X; only draw when +X is the back wall (eye on −X).
        let show = cfg.wall_relative || cfg.eye[0] < 0.0;
        if show {
            Some(Panel {
                center: Vec3::new(xc, 0.0, 0.0),
                u: Vec3::Z,
                v: Vec3::Y,
                n: Vec3::new(-xc.signum(), 0.0, 0.0),
                h,
            })
        } else {
            None
        }
    } else {
        None
    };

    (rear, right)
}

/// The little offset off the wall (fraction of the panel half-size) so the trace/bars
/// layer cleanly above the wall inset.
fn eps_data(h: f32) -> f32 {
    h * 0.012
}

/// The analyzer panels are FRAMELESS — no border and no interior graticule. The scene's
/// own box wireframe (the #135 axes/box decoration) already frames the panels and
/// supplies the time × frequency grid behind them, so any lines here just double up.
/// Kept as a (now-empty) hook so the callers/pipeline are undisturbed; only the scope
/// ribbon + spectrum bars draw. `_cfg`/`_rear`/`_right` are unused.
pub fn build_frames(_cfg: &ChamberConfig, _rear: &Option<Panel>, _right: &Option<Panel>) -> Vec<ChLine> {
    Vec::new()
}

/// Build the oscilloscope ribbon on the rear panel (a thin 2-quad strip so it has a
/// little width). `wave` = the display samples (−1..1, oldest→newest); `n` = valid
/// count. Amplitude → v (vertical), time → u (horizontal). Pure.
pub fn build_scope(cfg: &ChamberConfig, rear: &Option<Panel>, wave: &[f32], n: usize) -> Vec<ChVertex> {
    let mut out = Vec::new();
    let p = match rear {
        Some(p) => p,
        None => return out,
    };
    let n = n.min(wave.len());
    if n < 2 {
        return out;
    }
    let eps = eps_data(p.h);
    let w = cfg.thickness.clamp(0.002, 0.4); // ribbon half-width in local units
    let amp = cfg.scope_amp.max(0.0);
    let glow = 1.0 + cfg.emissive;
    // A bright cyan trace, emissive-boosted so it blooms.
    let col = [0.35 * glow, 0.85 * glow, 1.0 * glow, cfg.opacity];
    let nrm: [f32; 3] = p.n.into();
    let lu = |j: usize| -1.0 + 2.0 * j as f32 / (n as f32 - 1.0);
    let lv = |j: usize| (wave[j] * amp).clamp(-1.0, 1.0);
    // Emit a triangle strip as pairs of triangles between consecutive samples.
    for j in 0..n - 1 {
        let (u0, u1) = (lu(j), lu(j + 1));
        let (a0, a1) = (lv(j), lv(j + 1));
        let p0t = p.at(u0, a0 + w, eps);
        let p0b = p.at(u0, a0 - w, eps);
        let p1t = p.at(u1, a1 + w, eps);
        let p1b = p.at(u1, a1 - w, eps);
        push_tri(&mut out, p0b, p1b, p1t, nrm, col);
        push_tri(&mut out, p0b, p1t, p0t, nrm, col);
    }
    out
}

/// Build the spectrum bars on the right panel. `levels` = calibrated dBFS band levels
/// (floored at −120); `n` = valid band count. dB → v height (from the floor), band →
/// u (log-frequency is already baked into the band spacing). Pure.
pub fn build_spectrum(cfg: &ChamberConfig, right: &Option<Panel>, levels: &[f32], n: usize) -> Vec<ChVertex> {
    let mut out = Vec::new();
    let p = match right {
        Some(p) => p,
        None => return out,
    };
    let n = n.min(levels.len());
    if n == 0 {
        return out;
    }
    let eps = eps_data(p.h);
    let glow = 1.0 + cfg.emissive;
    let floor = cfg.db_floor;
    let top = cfg.db_top.max(floor + 1.0);
    let nrm: [f32; 3] = p.n.into();
    let half_w = (1.0 / n as f32) * 0.8; // bar half-width in local u, small gap between
    for b in 0..n {
        let norm = ((levels[b] - floor) / (top - floor)).clamp(0.0, 1.0);
        if norm <= 0.0 {
            continue;
        }
        let uc = -1.0 + 2.0 * (b as f32 + 0.5) / n as f32;
        let (u0, u1) = (uc - half_w, uc + half_w);
        let base = -1.0; // bottom of the panel
        let tip = -1.0 + 2.0 * norm;
        let mut col = db_color(norm);
        col[0] *= glow;
        col[1] *= glow;
        col[2] *= glow;
        col[3] = cfg.opacity;
        let q00 = p.at(u0, base, eps);
        let q10 = p.at(u1, base, eps);
        let q11 = p.at(u1, tip, eps);
        let q01 = p.at(u0, tip, eps);
        push_tri(&mut out, q00, q10, q11, nrm, col);
        push_tri(&mut out, q00, q11, q01, nrm, col);
    }
    out
}

/// Tier 2 (`PanelStyle::Impostor`): the scope trace as a chain of oriented capsule
/// impostors (a swept rounded tube) sitting proud of the rear wall. Pure.
pub fn build_scope_beads(cfg: &ChamberConfig, rear: &Option<Panel>, wave: &[f32], n: usize) -> Vec<ChBead> {
    let mut out = Vec::new();
    let p = match rear {
        Some(p) => p,
        None => return out,
    };
    let n = n.min(wave.len());
    if n < 2 {
        return out;
    }
    let eps = eps_data(p.h);
    let r = (cfg.thickness.clamp(0.002, 0.4) * p.h).max(1e-3);
    let amp = cfg.scope_amp.max(0.0);
    let col = [0.35, 0.85, 1.0, 1.0];
    let lu = |j: usize| -1.0 + 2.0 * j as f32 / (n as f32 - 1.0);
    let lv = |j: usize| (wave[j] * amp).clamp(-1.0, 1.0);
    for j in 0..n - 1 {
        // Lift the rod off the wall by its radius so it sits proud (into the room).
        let w0 = Vec3::from(p.at(lu(j), lv(j), eps)) + p.n * r;
        let w1 = Vec3::from(p.at(lu(j + 1), lv(j + 1), eps)) + p.n * r;
        push_capsule(&mut out, w0, w1, r, col, cfg.emissive);
    }
    out
}

/// Tier 2: the spectrum bars as vertical capsule impostors (rounded bars) standing off
/// the right wall. Pure.
pub fn build_spectrum_beads(cfg: &ChamberConfig, right: &Option<Panel>, levels: &[f32], n: usize) -> Vec<ChBead> {
    let mut out = Vec::new();
    let p = match right {
        Some(p) => p,
        None => return out,
    };
    let n = n.min(levels.len());
    if n == 0 {
        return out;
    }
    let eps = eps_data(p.h);
    let floor = cfg.db_floor;
    let top = cfg.db_top.max(floor + 1.0);
    // Bar radius: fit the per-band slot, gently capped by the line-thickness dial.
    let br = ((0.8 / n as f32) * p.h * 0.8)
        .min(cfg.thickness.clamp(0.002, 0.4) * p.h * 4.0)
        .max(1e-3);
    for b in 0..n {
        let norm = ((levels[b] - floor) / (top - floor)).clamp(0.0, 1.0);
        if norm <= 0.0 {
            continue;
        }
        let uc = -1.0 + 2.0 * (b as f32 + 0.5) / n as f32;
        let base = -1.0 + br / p.h; // start a radius up so the rounded end sits on the floor
        let tip = -1.0 + 2.0 * norm;
        let w0 = Vec3::from(p.at(uc, base, eps)) + p.n * br;
        let w1 = Vec3::from(p.at(uc, tip.max(base + 1e-3), eps)) + p.n * br;
        let mut col = db_color(norm);
        col[3] = 1.0;
        push_capsule(&mut out, w0, w1, br, col, cfg.emissive);
    }
    out
}

#[inline]
fn push_capsule(out: &mut Vec<ChBead>, a: Vec3, b: Vec3, r: f32, color: [f32; 4], emissive: f32) {
    let mid = (a + b) * 0.5;
    let d = b - a;
    let len = d.length();
    let axis = if len > 1e-6 { d / len } else { Vec3::Y };
    out.push(ChBead {
        center: [mid.x, mid.y, mid.z, r],
        axis: [axis.x, axis.y, axis.z, len * 0.5],
        color,
        emissive: [emissive, 0.0, 0.0, 0.0],
    });
}

#[inline]
fn push_tri(out: &mut Vec<ChVertex>, a: [f32; 3], b: [f32; 3], c: [f32; 3], n: [f32; 3], col: [f32; 4]) {
    out.push(ChVertex { pos: a, normal: n, color: col });
    out.push(ChVertex { pos: b, normal: n, color: col });
    out.push(ChVertex { pos: c, normal: n, color: col });
}

/// A simple perceptual dB colour ramp (blue → cyan → green → yellow → red), reused for
/// the calibrated RTA bars. `t` ∈ [0, 1].
fn db_color(t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    // piecewise gradient
    let stops = [
        (0.0, [0.10, 0.25, 0.9]),
        (0.35, [0.10, 0.85, 0.85]),
        (0.6, [0.25, 0.9, 0.25]),
        (0.8, [0.95, 0.9, 0.2]),
        (1.0, [1.0, 0.25, 0.2]),
    ];
    let mut c = stops[stops.len() - 1].1;
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            c = [
                c0[0] + (c1[0] - c0[0]) * f,
                c0[1] + (c1[1] - c0[1]) * f,
                c0[2] + (c1[2] - c0[2]) * f,
            ];
            break;
        }
    }
    [c[0], c[1], c[2], 1.0]
}

// ===========================================================================
// GPU: flat surface (ribbon + bars) + line (frames) pipelines, drawn in the scene pass.
// ===========================================================================

pub struct Chamber {
    bgl: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    ubuf: wgpu::Buffer,
    bind: wgpu::BindGroup,
    surf_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    sample_count: u32,
    surf_vbuf: Option<wgpu::Buffer>,
    surf_cap: u64,
    surf_count: u32,
    line_vbuf: Option<wgpu::Buffer>,
    line_cap: u64,
    line_count: u32,
    // Tier 2 impostor pass.
    imp_bgl: wgpu::BindGroupLayout,   // group(0) ChD uniform
    ibl_layout: wgpu::BindGroupLayout, // group(1) IBL (cloned, for MSAA rebuilds)
    imp_ubuf: wgpu::Buffer,
    imp_bind: wgpu::BindGroup,
    imp_color_pipeline: wgpu::RenderPipeline,
    imp_depth_pipeline: wgpu::RenderPipeline,
    imp_vbuf: Option<wgpu::Buffer>,
    imp_cap: u64,
    imp_count: u32,
}

impl Chamber {
    pub fn new(device: &wgpu::Device, ibl_layout: &wgpu::BindGroupLayout, sample_count: u32) -> Chamber {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chamber-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("chamber.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chamber-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chamber-ubuf"),
            size: 64, // mat4x4<f32>
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chamber-bind"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let surf_pipeline = make_surf_pipeline(device, &shader, &bgl, sample_count);
        let line_pipeline = make_line_pipeline(device, &shader, &bgl, sample_count);

        // Tier 2 impostor: group(0) = ChD uniform (reuse the same BGL shape as `bgl` but
        // a bigger buffer), group(1) = the shared IBL. Colour pipeline is MSAA-matched;
        // the depth-only pipeline is single-sample (the FX prepass).
        let imp_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chamber-imp-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let imp_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chamber-imp-ubuf"),
            size: std::mem::size_of::<ChDRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let imp_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chamber-imp-bind"),
            layout: &imp_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: imp_ubuf.as_entire_binding() }],
        });
        let imp_color_pipeline = make_imp_pipeline(device, &shader, &imp_bgl, ibl_layout, sample_count);
        let imp_depth_pipeline = make_imp_depth_pipeline(device, &shader, &imp_bgl);

        Chamber {
            bgl,
            shader,
            ubuf,
            bind,
            surf_pipeline,
            line_pipeline,
            sample_count,
            surf_vbuf: None,
            surf_cap: 0,
            surf_count: 0,
            line_vbuf: None,
            line_cap: 0,
            line_count: 0,
            imp_bgl,
            ibl_layout: ibl_layout.clone(),
            imp_ubuf,
            imp_bind,
            imp_color_pipeline,
            imp_depth_pipeline,
            imp_vbuf: None,
            imp_cap: 0,
            imp_count: 0,
        }
    }

    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        if n != self.sample_count {
            self.sample_count = n;
            self.surf_pipeline = make_surf_pipeline(device, &self.shader, &self.bgl, n);
            self.line_pipeline = make_line_pipeline(device, &self.shader, &self.bgl, n);
            self.imp_color_pipeline = make_imp_pipeline(device, &self.shader, &self.imp_bgl, &self.ibl_layout, n);
        }
    }

    /// Upload camera + geometry (call before the scene pass).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: &[[f32; 4]; 4],
        surfs: &[ChVertex],
        lines: &[ChLine],
    ) {
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(view_proj));
        self.surf_count = surfs.len() as u32;
        if !surfs.is_empty() {
            let bytes = bytemuck::cast_slice(surfs);
            grow_write(device, queue, &mut self.surf_vbuf, &mut self.surf_cap, "chamber-surf-vbuf", bytes);
        }
        self.line_count = lines.len() as u32;
        if !lines.is_empty() {
            let bytes = bytemuck::cast_slice(lines);
            grow_write(device, queue, &mut self.line_vbuf, &mut self.line_cap, "chamber-line-vbuf", bytes);
        }
    }

    /// Draw the prepared panels (surfaces + frames) into the scene pass.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if self.surf_count > 0 {
            if let Some(vb) = self.surf_vbuf.as_ref() {
                rp.set_pipeline(&self.surf_pipeline);
                rp.set_bind_group(0, &self.bind, &[]);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.draw(0..self.surf_count, 0..1);
            }
        }
        if self.line_count > 0 {
            if let Some(vb) = self.line_vbuf.as_ref() {
                rp.set_pipeline(&self.line_pipeline);
                rp.set_bind_group(0, &self.bind, &[]);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.draw(0..self.line_count, 0..1);
            }
        }
    }

    /// Upload the impostor context (ChD uniform) + capsule instances. Call before the
    /// final submit (uploads are queue-ordered, so it may be textually after the passes).
    pub fn prepare_impostor(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: &ImpostorFrame, beads: &[ChBead]) {
        let d = ChDRaw {
            view_proj: f.view_proj,
            cam_right: [f.cam_right[0], f.cam_right[1], f.cam_right[2], 0.0],
            cam_up: [f.cam_up[0], f.cam_up[1], f.cam_up[2], 0.0],
            cam_pos: [f.cam_pos[0], f.cam_pos[1], f.cam_pos[2], f.prefilter_mips],
            key_light: f.key_light,
            fill_light: f.fill_light,
            env: f.env,
            env_tint: [f.env_tint[0], f.env_tint[1], f.env_tint[2], 0.0],
            material: f.material,
            opts: [f.opacity, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.imp_ubuf, 0, bytemuck::bytes_of(&d));
        self.imp_count = beads.len() as u32;
        if !beads.is_empty() {
            let bytes = bytemuck::cast_slice(beads);
            grow_write(device, queue, &mut self.imp_vbuf, &mut self.imp_cap, "chamber-imp-vbuf", bytes);
        }
    }

    /// Draw the impostor capsules (colour + frag_depth) in the scene pass. `ibl_bind` is
    /// the shared split-sum IBL bind group (group 1).
    pub fn draw_impostor<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, ibl_bind: &'a wgpu::BindGroup) {
        if self.imp_count == 0 {
            return;
        }
        if let Some(vb) = self.imp_vbuf.as_ref() {
            rp.set_pipeline(&self.imp_color_pipeline);
            rp.set_bind_group(0, &self.imp_bind, &[]);
            rp.set_bind_group(1, ibl_bind, &[]);
            rp.set_vertex_buffer(0, vb.slice(..));
            rp.draw(0..6, 0..self.imp_count);
        }
    }

    /// Draw the impostor capsules depth-only into the FX depth prepass (single-sample),
    /// so SSAO / SSR / SSGI reconstruct off the panels too.
    pub fn draw_impostor_depth<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if self.imp_count == 0 {
            return;
        }
        if let Some(vb) = self.imp_vbuf.as_ref() {
            rp.set_pipeline(&self.imp_depth_pipeline);
            rp.set_bind_group(0, &self.imp_bind, &[]);
            rp.set_vertex_buffer(0, vb.slice(..));
            rp.draw(0..6, 0..self.imp_count);
        }
    }
}

fn grow_write(device: &wgpu::Device, queue: &wgpu::Queue, buf: &mut Option<wgpu::Buffer>, cap: &mut u64, label: &str, bytes: &[u8]) {
    let need = bytes.len() as u64;
    if buf.is_none() || *cap < need {
        *cap = need.next_power_of_two().max(2048);
        *buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: *cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    queue.write_buffer(buf.as_ref().unwrap(), 0, bytes);
}

fn pipeline_layout(device: &wgpu::Device, bgl: &wgpu::BindGroupLayout) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("chamber-pl"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    })
}

fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

fn color_target() -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format: HDR_FORMAT,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })
}

fn make_surf_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, bgl: &wgpu::BindGroupLayout, sample_count: u32) -> wgpu::RenderPipeline {
    let layout = pipeline_layout(device, bgl);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("chamber-surf-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_surf"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ChVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_surf"),
            targets: &[color_target()],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(depth_state()),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn make_line_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, bgl: &wgpu::BindGroupLayout, sample_count: u32) -> wgpu::RenderPipeline {
    let layout = pipeline_layout(device, bgl);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("chamber-line-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_line"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ChLine>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_line"),
            targets: &[color_target()],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(depth_state()),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

const IMP_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

fn imp_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ChBead>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &IMP_ATTRS,
    }
}

fn make_imp_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, imp_bgl: &wgpu::BindGroupLayout, ibl_layout: &wgpu::BindGroupLayout, sample_count: u32) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("chamber-imp-pl"),
        bind_group_layouts: &[Some(imp_bgl), Some(ibl_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("chamber-imp-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_imp"),
            buffers: &[Some(imp_instance_layout())],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_imp"),
            targets: &[color_target()],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn make_imp_depth_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule, imp_bgl: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("chamber-imp-depth-pl"),
        bind_group_layouts: &[Some(imp_bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("chamber-imp-depth-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_imp"),
            buffers: &[Some(imp_instance_layout())],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_imp_depth"),
            targets: &[], // depth only
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: 1, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ChamberConfig {
        ChamberConfig {
            on: true,
            style: 0,
            rear_on: true,
            right_on: true,
            extent: 4.0,
            eye: [6.0, 4.0, 8.0], // +x, +z
            opacity: 0.85,
            fill: 0.9,
            scope_amp: 1.0,
            db_floor: -60.0,
            db_top: 0.0,
            thickness: 0.03,
            emissive: 0.0,
            wall_relative: false,
            frame_color: [0.5, 0.55, 0.7, 0.5],
        }
    }

    #[test]
    fn off_selects_nothing() {
        let mut c = cfg();
        c.on = false;
        let (a, b) = select_walls(&c);
        assert!(a.is_none() && b.is_none());
    }

    #[test]
    fn fixed_shows_rear_on_neg_z_and_right_on_pos_x_when_back_facing() {
        // eye = +x, +z: −Z is back-facing (rear shows); +X is NOT back (right hidden).
        let (rear, right) = select_walls(&cfg());
        let rear = rear.expect("rear shows when eye is on +Z");
        assert!((rear.center.z + 4.0).abs() < 1e-5, "rear not on −Z: {:?}", rear.center);
        assert!(rear.n.z > 0.0, "rear normal must point into the room (+Z)");
        assert!(right.is_none(), "right hidden when the camera is on +X");
    }

    #[test]
    fn camera_relative_always_shows_both_on_back_walls() {
        let mut c = cfg();
        c.wall_relative = true;
        c.eye = [5.0, 3.0, 5.0]; // +x,+z
        let (rear, right) = select_walls(&c);
        let rear = rear.unwrap();
        let right = right.unwrap();
        // Both back walls at −extent (opposite the camera).
        assert!((rear.center.z + 4.0).abs() < 1e-5);
        assert!((right.center.x + 4.0).abs() < 1e-5);
        // Into-room normals point back toward the origin.
        assert!(rear.n.z > 0.0 && right.n.x > 0.0);
    }

    #[test]
    fn scope_ribbon_has_two_tris_per_segment() {
        let (rear, _) = select_walls(&cfg());
        let wave: Vec<f32> = (0..64).map(|i| (i as f32 * 0.2).sin()).collect();
        let v = build_scope(&cfg(), &rear, &wave, wave.len());
        // (n-1) segments × 2 tris × 3 verts.
        assert_eq!(v.len(), (64 - 1) * 6);
        // Every vertex sits ~on the rear wall (z ≈ −extent, within eps + amp).
        for sv in &v {
            assert!(sv.pos[2] < -3.0, "scope vert not near the rear wall: {:?}", sv.pos);
        }
    }

    #[test]
    fn spectrum_bars_skip_silent_bands() {
        let mut c = cfg();
        c.wall_relative = true; // force the right panel visible from this eye
        let (_, right) = select_walls(&c);
        // Two bands above floor, the rest at the floor (skipped).
        let mut levels = vec![-120.0f32; 8];
        levels[2] = -20.0;
        levels[5] = -6.0;
        let v = build_spectrum(&c, &right, &levels, levels.len());
        // 2 bars × 2 tris × 3 verts.
        assert_eq!(v.len(), 2 * 6);
    }

    #[test]
    fn scope_beads_one_capsule_per_segment() {
        let (rear, _) = select_walls(&cfg());
        let wave: Vec<f32> = (0..32).map(|i| (i as f32 * 0.3).sin()).collect();
        let b = build_scope_beads(&cfg(), &rear, &wave, wave.len());
        assert_eq!(b.len(), 32 - 1);
        for cap in &b {
            assert!(cap.center[3] > 0.0, "capsule radius must be positive");
            let ax = Vec3::new(cap.axis[0], cap.axis[1], cap.axis[2]).length();
            assert!((ax - 1.0).abs() < 1e-3 || cap.axis[3] < 1e-6, "axis not unit: {ax}");
        }
    }

    #[test]
    fn spectrum_beads_skip_silent_bands() {
        let mut c = cfg();
        c.wall_relative = true;
        let (_, right) = select_walls(&c);
        let mut levels = vec![-120.0f32; 6];
        levels[1] = -30.0;
        levels[4] = -3.0;
        let b = build_spectrum_beads(&c, &right, &levels, levels.len());
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn panels_are_frameless() {
        // The panels draw NO frame/graticule lines at all — the box wireframe frames
        // them and supplies the grid, so `build_frames` is empty even with panels active
        // (#346 update).
        let c = cfg();
        assert!(build_frames(&c, &None, &None).is_empty());
        let (rear, _) = select_walls(&c);
        assert!(build_frames(&c, &rear, &None).is_empty());
    }
}

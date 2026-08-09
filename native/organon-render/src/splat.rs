//! Gaussian Splatting surface (`SurfaceMode::Splat`) — the render-side of the 3DGS
//! *primitive* used for forward synthesis. There is NO reconstruction step: the
//! generator already produced every node's model matrix (`draw_tissue` → `Vec<Mat4>`)
//! + tint, so each node maps DIRECTLY to an anisotropic Gaussian — translation → the
//! centre μ, the 3×3 rot·scale columns → the σ-scaled covariance axes, tint → colour.
//! This module owns only the GPU plumbing (instance buffer, uniform, the two draw
//! pipelines); the math lives in the shared `math.rs` node build + `splat.wgsl`.
//!
//! Included by `render.rs` via `#[path]` (like `particles`/`metaball`), so it compiles
//! only into the visual binary. Off (any `surface_mode` ≠ 8) → nothing runs.
//!
//! Two tiers (chosen by `SplatFrame::lit`):
//!   • Tier 1 — additive, unlit anisotropic Gaussians, no depth sort (the robust glow).
//!   • Tier 2 — IBL-lit 2DGS oriented disks, alpha-composited back-to-front. The CPU
//!     sorts the instances by view depth each frame (`prepare`), the shader shades the
//!     disk normal with the split-sum IBL + key/fill, and the pipeline binds group(1)
//!     = the shared IBL (exactly like the #298 beads).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

/// One splat instance (per node), streamed to the vertex stage as an instance vbuf.
/// Mirrors `splat.wgsl::SplatInst` (5× vec4 = 80 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SplatInst {
    mu: [f32; 4],  // xyz = centre, w = per-splat opacity
    ax: [f32; 4],  // xyz = σ-scaled semi-axis a (world)
    ay: [f32; 4],  // xyz = σ-scaled semi-axis b (world)
    az: [f32; 4],  // xyz = σ-scaled semi-axis c (world) — the disk-normal axis
    col: [f32; 4], // rgb = colour
}

/// The draw uniform. Mirrors `splat.wgsl::SplatU` (mat4 + 9× vec4 = 208 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SplatU {
    view_proj: [[f32; 4]; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    cam_pos: [f32; 4],
    params: [f32; 4],
    key_light: [f32; 4],
    fill_light: [f32; 4],
    env: [f32; 4],
    env_tint: [f32; 4],
    mat: [f32; 4], // [metallic, roughness, material_type, ior] — Tier 3 relightable material
    skyrefl: [f32; 4], // #305 live-sky cloud reflections: [enable, cover, drift phase, strength]
    params2: [f32; 4], // Tier 3: [solid (Gaussian→opaque disc), _, _, _]
}

/// The per-frame splat look, passed through `render::Surface` from the visual. The
/// geometry (`instances`/`tints`) + camera + lighting are filled in `render.rs`.
#[derive(Clone, Copy)]
pub struct SplatParams {
    pub radius: f32,
    pub opacity: f32,
    pub falloff: f32,
    pub cutoff: f32,
    pub aniso: f32,
    /// Tier 3: sub-splats per node (1 = one/node) + their spread (fraction of node size).
    pub scatter: u32,
    pub jitter: f32,
    /// Tier 3 solidity (0..1): 0 = soft Gaussian, 1 = opaque disc (compact surface).
    pub solid: f32,
    /// Tier 2 (lit 2DGS) vs Tier 1 (additive glow).
    pub lit: bool,
}

/// Everything the splat system needs each frame (assembled in `render.rs` from the
/// node `instances`/`tints`, the cube `Uniforms`, and the shared IBL context).
pub struct SplatFrame<'a> {
    pub enabled: bool,
    pub instances: &'a [Mat4],
    pub tints: &'a [Vec4],
    /// World → clip (the SAME cube view_proj the instanced draw uses, so the splats
    /// sit exactly where the cubes would — breath scale folded in).
    pub view_proj: Mat4,
    pub cam_right: Vec3,
    pub cam_up: Vec3,
    pub cam_pos: Vec3,
    pub prefilter_mips: f32,
    // Splat look (from `Shared.splat`):
    pub radius: f32,
    pub opacity: f32,
    pub falloff: f32,
    pub cutoff: f32,
    pub aniso: f32,
    /// Tier 3 scatter: sub-splats sprayed per node (1 = one/node). Each sub-splat's
    /// opacity is divided by the count so the total brightness is preserved.
    pub scatter: u32,
    /// Tier 3 scatter jitter: sub-splat spread as a fraction of the node's size.
    pub jitter: f32,
    /// Tier 2: lit 2DGS disks (sorted alpha) instead of Tier 1 additive glow.
    pub lit: bool,
    /// Tier 3 relightable material (from the Material card): 0 Standard, 1 Chrome,
    /// 2 Glass, 3 Refractive (4+ fall back to Standard). Lit splats reflect/refract
    /// the environment like the cubes.
    pub material_type: f32,
    /// Glass/Refractive index of refraction (from the Material card).
    pub ior: f32,
    // Tier 2 lighting context (mirrors particles::ParticleShade):
    /// xyz = world dir TO the key/fill light (unit), w = intensity.
    pub key_light: Vec4,
    pub fill_light: Vec4,
    /// x unused, y = env_intensity, z = env_rotation (rad), w = ambient_mul.
    pub env: Vec4,
    pub env_tint: Vec3,
    pub metallic: f32,
    pub roughness: f32,
    /// #305 live-sky cloud reflections: [enable, cover, drift phase, strength]. Applied
    /// to the sharp env reflection so lit splats match the cubes/beads.
    pub skyrefl: Vec4,
    /// Tier 3 solidity (0..1): remap the Gaussian toward an opaque disc (compact surface).
    pub solid: f32,
}

pub struct SplatSystem {
    shader: wgpu::ShaderModule,
    add_layout: wgpu::PipelineLayout,
    lit_layout: wgpu::PipelineLayout,
    draw_ub: wgpu::Buffer,
    draw_bind: wgpu::BindGroup,
    add_pipeline: wgpu::RenderPipeline,
    lit_pipeline: wgpu::RenderPipeline,
    inst_buf: wgpu::Buffer,
    inst_cap: usize,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    // Live frame state
    count: u32,
    lit: bool,
    scratch: Vec<SplatInst>,
}

impl SplatSystem {
    pub fn new(
        device: &wgpu::Device,
        ibl_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("splat"),
            source: wgpu::ShaderSource::Wgsl(include_str!("splat.wgsl").into()),
        });
        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("splat-draw-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        // Tier 1 (additive) binds only the uniform; Tier 2 (lit) also binds the IBL.
        let add_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("splat-add-pl"),
            bind_group_layouts: &[Some(&draw_bgl)],
            immediate_size: 0,
        });
        let lit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("splat-lit-pl"),
            bind_group_layouts: &[Some(&draw_bgl), Some(ibl_layout)],
            immediate_size: 0,
        });
        let draw_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("splat-draw-ub"),
            size: std::mem::size_of::<SplatU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let draw_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("splat-draw-bind"),
            layout: &draw_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: draw_ub.as_entire_binding() }],
        });
        let add_pipeline = make_pipeline(
            device, &shader, &add_layout, color_format, depth_format, sample_count, false,
        );
        let lit_pipeline = make_pipeline(
            device, &shader, &lit_layout, color_format, depth_format, sample_count, true,
        );
        let inst_cap = 1usize;
        let inst_buf = make_inst_buf(device, inst_cap);
        SplatSystem {
            shader,
            add_layout,
            lit_layout,
            draw_ub,
            draw_bind,
            add_pipeline,
            lit_pipeline,
            inst_buf,
            inst_cap,
            color_format,
            depth_format,
            sample_count,
            count: 0,
            lit: false,
            scratch: Vec::new(),
        }
    }

    /// Rebuild the pipelines at a new MSAA sample count (mirrors the scene pass).
    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        if n == self.sample_count {
            return;
        }
        self.sample_count = n;
        self.add_pipeline = make_pipeline(
            device, &self.shader, &self.add_layout, self.color_format, self.depth_format, n, false,
        );
        self.lit_pipeline = make_pipeline(
            device, &self.shader, &self.lit_layout, self.color_format, self.depth_format, n, true,
        );
    }

    /// Build the per-node splat instances (+ sort back-to-front for the lit tier),
    /// grow + upload the instance buffer, and write the draw uniform. Returns whether
    /// there is anything to draw this frame.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: &SplatFrame) -> bool {
        self.count = 0;
        self.lit = f.lit;
        if !f.enabled || f.instances.is_empty() || f.radius <= 0.0 {
            return false;
        }

        self.scratch.clear();
        let aniso = f.aniso.max(0.0);
        // Tier 3 scatter: N sub-splats per node, deterministically jittered within the
        // node's own size. Each sub-splat's opacity is 1/N so the total energy (additive)
        // and coverage (alpha) stay ~constant as density rises. scatter = 1 → one splat
        // per node at the exact centre (byte-identical to Tier 2).
        //
        // Scatter only makes sense WITH spread: N sub-splats stacked at one point would,
        // in the alpha-composited LIT tier, skew peak coverage to 1−(1−α/N)^N instead of
        // the intended α (additive is fine — N·(1/N) = 1 — but lit is not). So with
        // jitter = 0 collapse to a single splat/node (which is also what "no spread"
        // means, and keeps it byte-identical to Tier 2).
        let jitter = f.jitter.max(0.0);
        let scatter = if jitter <= 0.0 { 1 } else { f.scatter.clamp(1, 16) };
        let sub_op = 1.0 / scatter as f32;
        self.scratch.reserve(f.instances.len() * scatter as usize);
        for (i, m) in f.instances.iter().enumerate() {
            // Colour: the per-node tint (RGB-cube × sweep). White (w = 0 in the mesh
            // path) means "use the RGB-cube colour" — here we just take the rgb, which
            // is white for the default look; explicit palettes/HSV sweeps ride through.
            let col = f.tints.get(i).copied().unwrap_or(Vec4::ONE);
            let (mu, ax, ay, az) = splat_from_matrix(m, f.radius, aniso);
            // Node characteristic size = its largest σ-axis; jitter spreads within it.
            let csize = ax.length().max(ay.length()).max(az.length());
            for k in 0..scatter {
                // scatter == 1 (incl. the jitter = 0 collapse above) → the node centre.
                let c = if scatter == 1 {
                    mu
                } else {
                    mu + jitter_vec(i as u32, k) * (jitter * csize)
                };
                self.scratch.push(SplatInst {
                    mu: [c.x, c.y, c.z, sub_op],
                    ax: [ax.x, ax.y, ax.z, 0.0],
                    ay: [ay.x, ay.y, ay.z, 0.0],
                    az: [az.x, az.y, az.z, 0.0],
                    col: [col.x, col.y, col.z, 1.0],
                });
            }
        }

        // Tier 2 needs back-to-front order (farthest drawn first) so the alpha-over
        // composite is correct. Sort by squared distance from the camera, descending
        // (a robust painter's order for splats; radial distance ≈ view depth here).
        // Tier 1 is additive → order-independent, so it skips the sort.
        if f.lit {
            let cp = f.cam_pos;
            self.scratch.sort_by(|a, b| {
                let da = dist2(&a.mu, cp);
                let db = dist2(&b.mu, cp);
                db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Grow the instance buffer (power-of-two, grow-only — a look feature; the field
        // is bounded by the node count already capped upstream).
        if self.scratch.len() > self.inst_cap {
            self.inst_cap = self.scratch.len().next_power_of_two();
            self.inst_buf = make_inst_buf(device, self.inst_cap);
        }
        queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&self.scratch));

        let mode = if f.lit { 1.0 } else { 0.0 };
        let u = SplatU {
            view_proj: f.view_proj.to_cols_array_2d(),
            cam_right: [f.cam_right.x, f.cam_right.y, f.cam_right.z, 0.0],
            cam_up: [f.cam_up.x, f.cam_up.y, f.cam_up.z, 0.0],
            cam_pos: [f.cam_pos.x, f.cam_pos.y, f.cam_pos.z, f.prefilter_mips],
            params: [f.falloff, f.opacity, f.cutoff, mode],
            key_light: f.key_light.into(),
            fill_light: f.fill_light.into(),
            env: f.env.into(),
            env_tint: [f.env_tint.x, f.env_tint.y, f.env_tint.z, 0.0],
            mat: [f.metallic, f.roughness, f.material_type, f.ior],
            skyrefl: f.skyrefl.into(),
            params2: [f.solid.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.draw_ub, 0, bytemuck::bytes_of(&u));

        self.count = self.scratch.len() as u32;
        self.count > 0
    }

    /// Draw the prepared splats into the HDR scene target. `ibl_bind` is bound only for
    /// the lit tier. Call inside the scene render pass (after the skybox).
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, ibl_bind: &'a wgpu::BindGroup) {
        if self.count == 0 {
            return;
        }
        if self.lit {
            rp.set_pipeline(&self.lit_pipeline);
            rp.set_bind_group(0, &self.draw_bind, &[]);
            rp.set_bind_group(1, ibl_bind, &[]);
        } else {
            rp.set_pipeline(&self.add_pipeline);
            rp.set_bind_group(0, &self.draw_bind, &[]);
        }
        rp.set_vertex_buffer(0, self.inst_buf.slice(..));
        rp.draw(0..6, 0..self.count);
    }
}

/// Squared distance from a splat centre to the camera (the back-to-front sort key).
fn dist2(mu: &[f32; 4], cam: Vec3) -> f32 {
    let d = Vec3::new(mu[0], mu[1], mu[2]) - cam;
    d.dot(d)
}

/// Integer hash (Wang-style; mirrors `particles.wgsl::hash_u32`) → a deterministic,
/// well-mixed 32-bit value. Used for the scatter jitter so the cloud is stable frame
/// to frame (no RNG state, no `Math::random`).
fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// A deterministic jitter offset in the cube [-1,1]³ for sub-splat `k` of node `i`.
fn jitter_vec(i: u32, k: u32) -> Vec3 {
    let base = i.wrapping_mul(0x9e37_79b1).wrapping_add(k.wrapping_mul(0x85eb_ca77));
    let f = |h: u32| (hash_u32(h) & 0x00ff_ffff) as f32 / 0x0100_0000 as f32 * 2.0 - 1.0;
    Vec3::new(f(base), f(base ^ 0x68bc_21eb), f(base ^ 0x02e5_be93))
}

/// Convert a node model matrix into a splat: centre + three σ-scaled covariance axes.
/// The matrix columns are the node's world-space edge vectors; `radius` scales them
/// into the Gaussian σ, and `aniso` reshapes the per-axis lengths around their mean
/// (1 = the node's own scale as-is, 0 = forced round, >1 = exaggerated elongation).
fn splat_from_matrix(m: &Mat4, radius: f32, aniso: f32) -> (Vec3, Vec3, Vec3, Vec3) {
    let mu = m.w_axis.truncate();
    let cols = [m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate()];
    let fallback = [Vec3::X, Vec3::Y, Vec3::Z];
    let lens = [cols[0].length(), cols[1].length(), cols[2].length()];
    let mean = ((lens[0] + lens[1] + lens[2]) / 3.0).max(1e-4);
    let mut out = [Vec3::ZERO; 3];
    for i in 0..3 {
        let dir = if lens[i] > 1e-6 { cols[i] / lens[i] } else { fallback[i] };
        // Reshape the length around the mean by the aniso exponent, then scale to σ.
        let nl = mean * (lens[i] / mean).powf(aniso);
        out[i] = dir * (nl * radius);
    }
    // 2DGS: the lit shader treats the returned `az` as the disk normal, so it must be
    // the SHORTEST (thin) axis — NOT whichever matrix column happened to be Z. Order the
    // three so `az` = shortest and `ax`/`ay` span the disk plane. Reordering leaves the
    // symmetric Gaussian weight (da²+db²+dc²) unchanged, so Tier 1 stays identical; only
    // Tier 2's normal is corrected.
    let l = [out[0].length(), out[1].length(), out[2].length()];
    let short = if l[0] <= l[1] && l[0] <= l[2] {
        0
    } else if l[1] <= l[2] {
        1
    } else {
        2
    };
    let az = out[short];
    let mut tangents = [Vec3::ZERO; 2];
    let mut t = 0;
    for i in 0..3 {
        if i != short {
            tangents[t] = out[i];
            t += 1;
        }
    }
    (mu, tangents[0], tangents[1], az)
}

fn make_inst_buf(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("splat-instances"),
        size: (cap * std::mem::size_of::<SplatInst>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Build a splat draw pipeline. `lit` picks the alpha-composite (Tier 2) blend + the
/// `fs_splat_lit` entry; else the additive (Tier 1) blend + `fs_splat_add`. Both
/// depth-TEST against the scene (LessEqual) but never WRITE depth — splats are
/// translucent and must not occlude each other in the depth buffer.
fn make_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    lit: bool,
) -> wgpu::RenderPipeline {
    // Both fragments output PREMULTIPLIED colour (rgb already × alpha):
    //   • additive  → src One / dst One (glow accumulates, blooms in HDR).
    //   • lit alpha → src One / dst (1 − src_alpha) (back-to-front over-composite).
    let blend = if lit {
        wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }
    } else {
        wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        }
    };
    let fs_entry = if lit { "fs_splat_lit" } else { "fs_splat_add" };
    let inst_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<SplatInst>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 32, shader_location: 2 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 48, shader_location: 3 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 64, shader_location: 4 },
        ],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("splat-draw-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_splat"),
            buffers: &[Some(inst_layout)],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        // Depth-tested against the scene (occluded by opaque geometry) but no write.
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

//! Creature anatomy overlay (GitHub #476 Tier 2c): the artist's "diagram over the
//! creature" look. From the same body plan the raymarch uses, build a thin
//! world-space wireframe — the spine, a cross-section ring around each body
//! segment, and a vector per limb — and draw it as additive glowing lines in the
//! scene pass, TWICE against the creature's shared depth buffer: once depth-Always
//! (occluded lines read faintly) and once depth-Less (front lines read bright), so
//! landmarks dim where they pass behind the translucent body. The lines follow the
//! same peristaltic swim warp as the body, so the diagram stays glued to it.
//!
//! Compiled only by the visual binary (via `#[path]`), like `axes.rs`/`render.rs`.
//! The geometry builder (`build_creature_overlay`) is pure + unit-tested.

use glam::Vec3;

use organon_core::math::{CreaturePrim, CPRIM_ELLIPSOID, CPRIM_ROUND_CONE};

// Mirror render.rs's scene targets (kept local so the module is self-contained).
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Segments per cross-section ring.
const RING_SEG: usize = 24;

// Diagram colours (RGBA; alpha scales the per-line brightness).
const SPINE_COL: [f32; 4] = [0.55, 0.9, 1.0, 1.0];
const RING_COL: [f32; 4] = [0.4, 0.78, 0.96, 0.7];
const LIMB_COL: [f32; 4] = [1.0, 0.78, 0.42, 0.9];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug, PartialEq)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

/// Apply the same peristaltic swim warp the shader uses so the diagram tracks the
/// rendered (deformed) body. The shader domain-warps `pu.x -= sin(freq·z − phase)`,
/// which shifts the rendered surface the OTHER way, so a landmark glued to it moves
/// `+sin`. Amp 0 → the rest pose.
fn warp_pt(p: Vec3, phase: f32, freq: f32, amp: f32) -> Vec3 {
    if amp.abs() <= 1e-6 {
        return p;
    }
    Vec3::new(p.x + (freq * p.z - phase).sin() * amp, p.y, p.z)
}

/// Build the anatomy overlay line list (world space) from a body plan. `scale`
/// blows the unit-space plan up to world size (matching the raymarch); the swim
/// warp params keep the diagram glued to the deforming body. Pure.
pub fn build_creature_overlay(
    prims: &[CreaturePrim],
    scale: f32,
    warp_phase: f32,
    warp_freq: f32,
    warp_amp: f32,
) -> Vec<LineVertex> {
    let mut v: Vec<LineVertex> = Vec::new();
    let w = |p: Vec3| warp_pt(p, warp_phase, warp_freq, warp_amp) * scale;
    let mut seg = |a: Vec3, b: Vec3, col: [f32; 4]| {
        v.push(LineVertex { pos: w(a).into(), color: col });
        v.push(LineVertex { pos: w(b).into(), color: col });
    };

    // Spine: the ellipsoid segment centres, ordered along +Z, joined tip to tail.
    let mut spine: Vec<Vec3> = prims
        .iter()
        .filter(|p| p.kind == CPRIM_ELLIPSOID)
        .map(|p| p.a)
        .collect();
    spine.sort_by(|a, b| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal));
    for pair in spine.windows(2) {
        seg(pair[0], pair[1], SPINE_COL);
    }

    // A cross-section ring around each ellipsoid, in the plane perpendicular to the
    // body axis (+Z), radius = the segment's cross-section (max of its X/Y radii).
    for p in prims.iter().filter(|p| p.kind == CPRIM_ELLIPSOID) {
        let r = p.b.x.max(p.b.y);
        if r <= 1e-4 {
            continue;
        }
        let c = p.a;
        let mut prev = c + Vec3::new(r, 0.0, 0.0);
        for i in 1..=RING_SEG {
            let a = (i as f32 / RING_SEG as f32) * std::f32::consts::TAU;
            let cur = c + Vec3::new(a.cos() * r, a.sin() * r, 0.0);
            seg(prev, cur, RING_COL);
            prev = cur;
        }
    }

    // A limb vector per appendage: a round-cone draws along its own axis (a → b); a
    // paddle draws from its spine-projected foot out to its centre (the fin offset).
    for p in prims {
        match p.kind {
            CPRIM_ROUND_CONE => seg(p.a, p.b, LIMB_COL),
            CPRIM_ELLIPSOID => {}
            _ => {
                // Paddle (or any centred non-ellipsoid): body → fin offset vector.
                let foot = Vec3::new(0.0, 0.0, p.a.z);
                if (p.a - foot).length() > 1e-3 {
                    seg(foot, p.a, LIMB_COL);
                }
            }
        }
    }

    v
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayU {
    view_proj: [[f32; 4]; 4],
    params: [f32; 4], // brightness, alpha, _, _
}

/// GPU renderer for the overlay: one line buffer drawn through two pipelines
/// (depth-Always faint + depth-Less bright), each with its own params UBO.
pub struct CreatureOverlay {
    bgl: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    ub_faint: wgpu::Buffer,
    ub_bright: wgpu::Buffer,
    bind_faint: wgpu::BindGroup,
    bind_bright: wgpu::BindGroup,
    pipeline_always: wgpu::RenderPipeline,
    pipeline_less: wgpu::RenderPipeline,
    sample_count: u32,
    vbuf: Option<wgpu::Buffer>,
    vcap: u64,
    vcount: u32,
}

impl CreatureOverlay {
    pub fn new(device: &wgpu::Device, sample_count: u32) -> CreatureOverlay {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("creature-overlay-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("creature_overlay.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("creature-overlay-bgl"),
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
        let mk_ub = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<OverlayU>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let ub_faint = mk_ub("creature-overlay-ub-faint");
        let ub_bright = mk_ub("creature-overlay-ub-bright");
        let mk_bind = |ub: &wgpu::Buffer, label| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &bgl,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            })
        };
        let bind_faint = mk_bind(&ub_faint, "creature-overlay-bind-faint");
        let bind_bright = mk_bind(&ub_bright, "creature-overlay-bind-bright");
        let pipeline_always = make_line_pipeline(device, &shader, &bgl, sample_count, wgpu::CompareFunction::Always);
        let pipeline_less = make_line_pipeline(device, &shader, &bgl, sample_count, wgpu::CompareFunction::LessEqual);
        CreatureOverlay {
            bgl,
            shader,
            ub_faint,
            ub_bright,
            bind_faint,
            bind_bright,
            pipeline_always,
            pipeline_less,
            sample_count,
            vbuf: None,
            vcap: 0,
            vcount: 0,
        }
    }

    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        if n != self.sample_count {
            self.sample_count = n;
            self.pipeline_always = make_line_pipeline(device, &self.shader, &self.bgl, n, wgpu::CompareFunction::Always);
            self.pipeline_less = make_line_pipeline(device, &self.shader, &self.bgl, n, wgpu::CompareFunction::LessEqual);
        }
    }

    /// Upload the camera + line geometry + brightness. `brightness` is the front
    /// (depth-Less) pass; the occluded (depth-Always) pass gets a fraction of it so
    /// lines behind the body read faintly. Call before the scene pass.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: &[[f32; 4]; 4],
        lines: &[LineVertex],
        brightness: f32,
        opacity: f32,
    ) {
        let bright = OverlayU { view_proj: *view_proj, params: [brightness.max(0.0), opacity.clamp(0.0, 1.0), 0.0, 0.0] };
        let faint = OverlayU { view_proj: *view_proj, params: [brightness.max(0.0) * 0.28, opacity.clamp(0.0, 1.0), 0.0, 0.0] };
        queue.write_buffer(&self.ub_bright, 0, bytemuck::bytes_of(&bright));
        queue.write_buffer(&self.ub_faint, 0, bytemuck::bytes_of(&faint));
        self.vcount = lines.len() as u32;
        if !lines.is_empty() {
            let bytes = bytemuck::cast_slice(lines);
            let need = bytes.len() as u64;
            if self.vbuf.is_none() || self.vcap < need {
                self.vcap = need.next_power_of_two().max(2048);
                self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("creature-overlay-vbuf"),
                    size: self.vcap,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            queue.write_buffer(self.vbuf.as_ref().unwrap(), 0, bytes);
        }
    }

    /// Draw the overlay into the active scene pass: occluded-faint then front-bright.
    pub fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>) {
        if self.vcount == 0 {
            return;
        }
        let Some(vb) = self.vbuf.as_ref() else { return };
        rp.set_vertex_buffer(0, vb.slice(..));
        rp.set_pipeline(&self.pipeline_always);
        rp.set_bind_group(0, &self.bind_faint, &[]);
        rp.draw(0..self.vcount, 0..1);
        rp.set_pipeline(&self.pipeline_less);
        rp.set_bind_group(0, &self.bind_bright, &[]);
        rp.draw(0..self.vcount, 0..1);
    }
}

fn make_line_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    bgl: &wgpu::BindGroupLayout,
    sample_count: u32,
    depth_compare: wgpu::CompareFunction,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("creature-overlay-pl"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("creature-overlay-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_line"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<LineVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_line"),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                // Additive glow: src·1 + dst·1, so strokes accumulate light.
                blend: Some(wgpu::BlendState {
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
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            cull_mode: None,
            ..Default::default()
        },
        // Share the scene depth buffer but never write to it (the diagram floats over
        // the surface; the compare function is what varies between the two passes).
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(depth_compare),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use organon_core::math::creature_body_plan;

    #[test]
    fn overlay_builds_bounded_finite_lines() {
        for form in 0..organon_core::math::CREATURE_FORMS {
            let plan = creature_body_plan(form);
            let lines = build_creature_overlay(&plan, 100.0, 0.0, 4.0, 0.0);
            assert!(!lines.is_empty(), "form {form} produced no overlay");
            // LineList → an even number of vertices.
            assert_eq!(lines.len() % 2, 0);
            let bound = organon_core::math::creature_bounds(&plan) * 100.0 * 1.6;
            for lv in &lines {
                let p = Vec3::from(lv.pos);
                assert!(p.is_finite() && p.length() <= bound, "vertex out of bound in form {form}");
            }
        }
    }

    #[test]
    fn overlay_warp_glues_to_the_body_and_is_inert_at_zero() {
        let plan = creature_body_plan(1); // ribbon-swimmer
        let rest = build_creature_overlay(&plan, 100.0, 0.0, 4.0, 0.0);
        let warped = build_creature_overlay(&plan, 100.0, 0.7, 4.0, 0.1);
        assert_eq!(rest.len(), warped.len());
        // Amp 0 == rest; a non-zero warp must move at least one vertex laterally (X).
        let moved = rest.iter().zip(&warped).any(|(a, b)| (a.pos[0] - b.pos[0]).abs() > 1e-3);
        assert!(moved, "swim warp did not displace the overlay");
        // The warp only touches X (the undulation axis).
        for (a, b) in rest.iter().zip(&warped) {
            assert!((a.pos[1] - b.pos[1]).abs() < 1e-4 && (a.pos[2] - b.pos[2]).abs() < 1e-4);
        }
    }
}

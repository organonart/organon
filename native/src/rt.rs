//! Hardware ray tracing, Tier 0 plumbing (GitHub #195).
//!
//! The foundation every later RT effect (shadows, reflections, AO, GI) traces
//! against: a BLAS per static mesh (unit cube, unit cylinder — the same source
//! meshes the raster pipelines draw) and a per-frame TLAS built from the same
//! per-instance model matrices the instance buffer gets. Tier 0 ships dark —
//! nothing in the render changes; the two deliverables are the **cost
//! measurement** (`Feedback.tlas_ms`: the field animates every instance every
//! frame, so this is a full rebuild per frame — the number that green-lights
//! Tier 2) and the **debug view** (`rt_debug.wgsl`, a fullscreen ray query
//! drawn over the final frame to verify the TLAS matches the raster scene).
//!
//! Uses wgpu's `EXPERIMENTAL_RAY_QUERY` (Metal exposes RT as ray queries only;
//! hardware-accelerated on M3+). The API is explicitly experimental — every RT
//! call site stays inside this module so upstream churn is contained. On a
//! device without the feature `RtContext::new` returns `None` and everything
//! downstream silently no-ops (the editor greys the card out via
//! `Feedback.rt_available`).
//!
//! **Known Tier-0 scope limits** (#197 review; both fine for a cost
//! measurement, both must be revisited when a real RT effect consumes the
//! TLAS in Tier 1+):
//! - **Fluid sway (#182 T4)** displaces instance translations *on the GPU*
//!   (`sway.rs`, no readback by design), so a swayed frame's TLAS holds the
//!   pre-sway matrices. Tier 1+ either disables sway while tracing or mirrors
//!   the sway offsets into the TLAS path.
//! - **Boids creature forms** draw per-agent creature meshes with no BLAS
//!   here; the visual gates the TLAS off entirely on that path (`rt_on`)
//!   rather than trace wrong shapes. A creature BLAS is Tier 1+ work if RT
//!   should cover flocks.

use glam::Mat4;

/// TLAS capacity, fixed at creation (wgpu requires a max up front). Fields
/// beyond this are truncated — acceptable for the Tier-0 measurement; the
/// TLAS covers the first `RT_MAX_INSTANCES` nodes and the debug view shows
/// exactly what it holds.
pub const RT_MAX_INSTANCES: usize = 65_536;

/// Convert a glam column-major `Mat4` into the row-major 3x4 affine transform
/// (`[f32; 12]`) a `TlasInstance` wants: the top three rows of the matrix.
pub fn tlas_transform(m: &Mat4) -> [f32; 12] {
    let r0 = m.row(0);
    let r1 = m.row(1);
    let r2 = m.row(2);
    [
        r0.x, r0.y, r0.z, r0.w, //
        r1.x, r1.y, r1.z, r1.w, //
        r2.x, r2.y, r2.z, r2.w,
    ]
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DebugU {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4], // x = view mode (1/2/3), y = t_max
}

struct Mesh {
    blas: wgpu::Blas,
    // Kept alive for the lazy first build (BLAS build reads them on the GPU).
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    size: wgpu::BlasTriangleGeometrySizeDescriptor,
}

/// One bead handed to the TLAS build (#298 Tier 4): its world centre + radius. The
/// sphere BLAS is unit-radius, so the instance transform is `translate · scale(size)`.
#[derive(Clone, Copy, Debug)]
pub struct BeadRt {
    pub center: glam::Vec3,
    pub size: f32,
}

/// Custom-index bit that tags a TLAS instance as a **bead** (vs a field cube/tube),
/// so the RT hit shaders can shade the sphere geometry as a droplet. The low 23 bits
/// carry the bead's index; bit 23 is the tag (24-bit custom index total).
pub const RT_BEAD_TAG: u32 = 0x0080_0000;

fn build_mesh(device: &wgpu::Device, label: &str, positions: &[[f32; 3]], indices: &[u32]) -> Mesh {
    use wgpu::util::DeviceExt;
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("rt-{label}-verts")),
        contents: bytemuck::cast_slice(positions),
        usage: wgpu::BufferUsages::BLAS_INPUT,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("rt-{label}-indices")),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::BLAS_INPUT,
    });
    let size = wgpu::BlasTriangleGeometrySizeDescriptor {
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: positions.len() as u32,
        index_format: Some(wgpu::IndexFormat::Uint32),
        index_count: Some(indices.len() as u32),
        flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
    };
    let blas = device.create_blas(
        &wgpu::CreateBlasDescriptor {
            label: Some(&format!("rt-{label}-blas")),
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        },
        wgpu::BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![size.clone()],
        },
    );
    Mesh { blas, vbuf, ibuf, size }
}

/// Owns the acceleration structures + the debug pipeline. Created once at
/// startup when the device has ray-query support (else absent), used only
/// while the RT master toggle is on.
pub struct RtContext {
    cube: Mesh,
    cyl: Mesh,
    /// #298 Tier 4: the unit-radius sphere BLAS the particle beads instance.
    sphere: Mesh,
    blas_built: bool,
    tlas: wgpu::Tlas,
    /// Instance slots occupied by the last build (stale tail is None-ed out).
    used: usize,
    /// At least one successful TLAS build happened (gates the debug bind).
    built: bool,
    /// Smoothed CPU encode+submit cost of the per-frame TLAS rebuild (ms).
    pub build_ms: f32,
    // Debug pass, built lazily against the current swapchain format.
    debug_pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    debug_layout: wgpu::BindGroupLayout,
    debug_ubuf: wgpu::Buffer,
}

impl RtContext {
    /// `None` when the device lacks `EXPERIMENTAL_RAY_QUERY` — the caller
    /// stores the `Option` and every use site no-ops through it.
    pub fn new(device: &wgpu::Device) -> Option<RtContext> {
        if !device.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
            return None;
        }
        let tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
            label: Some("rt-tlas"),
            max_instances: RT_MAX_INSTANCES as u32,
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        });
        let debug_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-debug-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                    count: None,
                },
            ],
        });
        let debug_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-debug-uniforms"),
            size: std::mem::size_of::<DebugU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (cube_p, cube_i) = super::render::rt_mesh(false);
        let (cyl_p, cyl_i) = super::render::rt_mesh(true);
        let (sph_p, sph_i) = super::render::rt_sphere_mesh();
        Some(RtContext {
            cube: build_mesh(device, "cube", &cube_p, &cube_i),
            cyl: build_mesh(device, "cyl", &cyl_p, &cyl_i),
            sphere: build_mesh(device, "sphere", &sph_p, &sph_i),
            blas_built: false,
            tlas,
            used: 0,
            built: false,
            build_ms: 0.0,
            debug_pipeline: None,
            debug_layout,
            debug_ubuf,
        })
    }

    /// Rebuild the TLAS from this frame's instance matrices (the whole field
    /// animates every frame, so it's a full rebuild, not a refit). `tube`
    /// picks the cylinder BLAS (Swept Tubes) exactly like the raster path
    /// picks its mesh. Returns the CPU encode+submit time; `build_ms` keeps a
    /// smoothed copy for the feedback channel.
    pub fn build(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[Mat4],
        tube: bool,
    ) -> f32 {
        self.build_with_beads(device, queue, instances, tube, &[])
    }

    /// Plexus Tier-1 shape morph: the field is drawn as two morphed sub-batches —
    /// markers `[0, markers)` and struts `[markers, …)`. A single cube/cyl BLAS would
    /// mismatch the raster, so trace each range with the nearest static BLAS: markers
    /// → the **sphere** BLAS (pre-scaled ×½ since it's the radius-1 bead sphere, giving
    /// the radius-0.5 node), struts → the **cylinder** BLAS (radius 0.5, matches the
    /// `push_rod` strut directly). Exact at the default (node sphere / edge circle); a
    /// static BLAS can't follow the slider toward cube/square, so those approximate —
    /// acceptable for the opt-in RT path (a per-frame morph BLAS rebuild isn't worth it).
    pub fn build_plexus(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[Mat4],
        markers: usize,
    ) -> f32 {
        let t0 = std::time::Instant::now();
        let n = instances.len().min(RT_MAX_INSTANCES);
        let half = Mat4::from_scale(glam::Vec3::splat(0.5));
        for (i, m) in instances[..n].iter().enumerate() {
            let (blas, xf) = if i < markers {
                (&self.sphere.blas, *m * half) // radius-1 sphere → radius-0.5 node
            } else {
                (&self.cyl.blas, *m) // radius-0.5 cylinder = the push_rod strut
            };
            self.tlas[i] = Some(wgpu::TlasInstance::new(
                blas,
                tlas_transform(&xf),
                i as u32 & 0x00ff_ffff,
                0xff,
            ));
        }
        for i in n..self.used {
            self.tlas[i] = None;
        }
        self.used = n;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rt-build-plexus"),
        });
        let blas_entries: Vec<wgpu::BlasBuildEntry> = if self.blas_built {
            Vec::new()
        } else {
            self.blas_built = true;
            [&self.cube, &self.cyl, &self.sphere]
                .into_iter()
                .map(|m| wgpu::BlasBuildEntry {
                    blas: &m.blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                        wgpu::BlasTriangleGeometry {
                            size: &m.size,
                            vertex_buffer: &m.vbuf,
                            first_vertex: 0,
                            vertex_stride: 12,
                            index_buffer: Some(&m.ibuf),
                            first_index: Some(0),
                            transform_buffer: None,
                            transform_buffer_offset: None,
                        },
                    ]),
                })
                .collect()
        };
        encoder.build_acceleration_structures(blas_entries.iter(), std::iter::once(&self.tlas));
        queue.submit(std::iter::once(encoder.finish()));
        self.built = true;
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.build_ms = if self.build_ms == 0.0 { ms } else { self.build_ms * 0.9 + ms * 0.1 };
        ms
    }

    /// #298 Tier 4: build the TLAS from the field instances **plus** a curated set of
    /// particle **beads** (unit-radius sphere BLAS, `translate·scale(size)` per bead,
    /// tagged `RT_BEAD_TAG` in the custom index so the hit shaders shade them as
    /// droplets). Beads are appended after the field instances and share the 65 536
    /// cap; `math::curate_beads` picks the subset that fits. An empty `beads` slice is
    /// byte-identical to `build`.
    pub fn build_with_beads(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[Mat4],
        tube: bool,
        beads: &[BeadRt],
    ) -> f32 {
        let t0 = std::time::Instant::now();
        let n = instances.len().min(RT_MAX_INSTANCES);
        let blas = if tube { &self.cyl.blas } else { &self.cube.blas };
        for (i, m) in instances[..n].iter().enumerate() {
            self.tlas[i] = Some(wgpu::TlasInstance::new(
                blas,
                tlas_transform(m),
                i as u32 & 0x00ff_ffff, // custom index: 24 bits max
                0xff,
            ));
        }
        // Append the beads (sphere BLAS) after the field, up to the shared cap.
        let bead_room = RT_MAX_INSTANCES - n;
        let bn = beads.len().min(bead_room);
        for (bi, b) in beads[..bn].iter().enumerate() {
            let m = Mat4::from_translation(b.center) * Mat4::from_scale(glam::Vec3::splat(b.size));
            self.tlas[n + bi] = Some(wgpu::TlasInstance::new(
                &self.sphere.blas,
                tlas_transform(&m),
                RT_BEAD_TAG | (bi as u32 & 0x007f_ffff), // tag + bead index
                0xff,
            ));
        }
        let total = n + bn;
        for i in total..self.used {
            self.tlas[i] = None;
        }
        self.used = total;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rt-build"),
        });
        // The BLASes are static — built once, in the same submission as the
        // first TLAS build (wgpu allows referencing a BLAS built in the same
        // call).
        let blas_entries: Vec<wgpu::BlasBuildEntry> = if self.blas_built {
            Vec::new()
        } else {
            self.blas_built = true;
            [&self.cube, &self.cyl, &self.sphere]
                .into_iter()
                .map(|m| wgpu::BlasBuildEntry {
                    blas: &m.blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                        wgpu::BlasTriangleGeometry {
                            size: &m.size,
                            vertex_buffer: &m.vbuf,
                            first_vertex: 0,
                            vertex_stride: 12, // tightly packed [f32; 3]
                            index_buffer: Some(&m.ibuf),
                            first_index: Some(0),
                            transform_buffer: None,
                            transform_buffer_offset: None,
                        },
                    ]),
                })
                .collect()
        };
        encoder.build_acceleration_structures(blas_entries.iter(), std::iter::once(&self.tlas));
        queue.submit(std::iter::once(encoder.finish()));
        self.built = true;

        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        // EMA so the editor readout doesn't flicker frame to frame.
        self.build_ms = if self.build_ms == 0.0 { ms } else { self.build_ms * 0.9 + ms * 0.1 };
        ms
    }

    /// The TLAS, once at least one build has completed (it may only be bound
    /// after a build). The RT shadow pass (#195 Tier 1) traces against this.
    pub fn tlas(&self) -> Option<&wgpu::Tlas> {
        self.built.then_some(&self.tlas)
    }

    /// The debug view: a fullscreen ray query drawn OVER the final frame
    /// (opaque — it replaces the image while active; it's a verification
    /// tool, not a look). No-op until a TLAS build has happened.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_debug(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        inv_view_proj: Mat4,
        cam_pos: glam::Vec3,
        mode: u32,
        t_max: f32,
        // `(x, y, w, h)` sub-rect of the target to confine the pass to — the
        // production letterbox fit rect, so the rays' aspect matches the
        // letterboxed raster scene. `None` = the full target.
        viewport: Option<(u32, u32, u32, u32)>,
    ) {
        if !self.built || mode == 0 {
            return;
        }
        // (Re)build the pipeline when the swapchain format changes (SDR ↔ EDR).
        if self.debug_pipeline.as_ref().map(|(f, _)| *f) != Some(format) {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rt-debug-shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("rt_debug.wgsl").into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rt-debug-pl"),
                bind_group_layouts: &[Some(&self.debug_layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rt-debug-pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            self.debug_pipeline = Some((format, pipeline));
        }
        let u = DebugU {
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            cam_pos: [cam_pos.x, cam_pos.y, cam_pos.z, 0.0],
            params: [mode as f32, t_max, 0.0, 0.0],
        };
        queue.write_buffer(&self.debug_ubuf, 0, bytemuck::bytes_of(&u));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-debug-bind"),
            layout: &self.debug_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.debug_ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::AccelerationStructure(&self.tlas),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rt-debug"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rt-debug-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let (_, pipeline) = self.debug_pipeline.as_ref().unwrap();
            pass.set_pipeline(pipeline);
            if let Some((x, y, w, h)) = viewport {
                // Viewport maps the fullscreen triangle's NDC onto the fit
                // rect (uv still spans 0..1 across it → correct aspect);
                // scissor keeps the letterbox bars untouched.
                pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, w, h);
            }
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `super::*` puts us one level too deep to reach the module tree's `render`: inside this
    // test module `super` is `rt` itself, so the module-level `super::render::…` spelling
    // (which resolves to `world::render` in both hosts — see `world.rs`) needs one more hop.
    use super::super::render;
    use glam::{Vec3, Vec4};

    #[test]
    fn tlas_transform_is_row_major_3x4() {
        // A translate(2,3,4) · scale(2) matrix: rows carry the scale on the
        // diagonal and the translation in the 4th column.
        let m = Mat4::from_translation(Vec3::new(2.0, 3.0, 4.0))
            * Mat4::from_scale(Vec3::splat(2.0));
        let t = tlas_transform(&m);
        #[rustfmt::skip]
        let expected = [
            2.0, 0.0, 0.0, 2.0,
            0.0, 2.0, 0.0, 3.0,
            0.0, 0.0, 2.0, 4.0,
        ];
        assert_eq!(t, expected);
    }

    #[test]
    fn tlas_transform_transforms_points_like_the_mat4() {
        // Applying the 3x4 rows to a homogeneous point must equal the Mat4.
        let m = Mat4::from_rotation_y(0.7)
            * Mat4::from_translation(Vec3::new(-1.0, 5.0, 0.5))
            * Mat4::from_scale(Vec3::new(1.0, 2.0, 3.0));
        let t = tlas_transform(&m);
        let p = Vec4::new(0.3, -0.7, 1.1, 1.0);
        let expect = m * p;
        let row = |r: usize| Vec4::new(t[r * 4], t[r * 4 + 1], t[r * 4 + 2], t[r * 4 + 3]);
        for r in 0..3 {
            assert!((row(r).dot(p) - expect[r]).abs() < 1e-5, "row {r} drifted");
        }
    }

    #[test]
    fn rt_meshes_are_blas_compatible() {
        // Every BLAS source mesh (cube, cylinder, and the #298 bead sphere): indices
        // in range, counts divisible by 3 (triangle lists), and every vertex finite.
        let meshes = [
            render::rt_mesh(false),
            render::rt_mesh(true),
            render::rt_sphere_mesh(),
        ];
        for (pos, idx) in meshes {
            assert!(!pos.is_empty() && !idx.is_empty());
            assert_eq!(idx.len() % 3, 0, "triangle list expected");
            assert!(idx.iter().all(|&i| (i as usize) < pos.len()), "index out of range");
            assert!(pos.iter().all(|p| p.iter().all(|c| c.is_finite())));
        }
    }

    #[test]
    fn rt_bead_sphere_is_unit_radius() {
        // #298 Tier 4: the bead BLAS sphere must be radius ~1 so a bead instance is a
        // plain translate·scale(size). Every vertex sits on the unit sphere.
        let (pos, _) = render::rt_sphere_mesh();
        for p in &pos {
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!((r - 1.0).abs() < 1e-4, "bead sphere vertex off unit radius: {r}");
        }
    }

    #[test]
    fn rt_bead_tag_round_trips() {
        // The tag bit marks a bead; the low 23 bits carry its index, both recoverable.
        for i in [0u32, 1, 42, 0x007f_fffe] {
            let ci = RT_BEAD_TAG | (i & 0x007f_ffff);
            assert!(ci & RT_BEAD_TAG != 0, "bead tag lost");
            assert_eq!(ci & 0x007f_ffff, i, "bead index corrupted");
        }
        // A plain field index (< 2^23) is never mistaken for a bead.
        assert_eq!(1234u32 & RT_BEAD_TAG, 0);
    }
}

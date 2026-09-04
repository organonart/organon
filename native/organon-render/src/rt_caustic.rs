//! Photon-mapped caustics (#258 Tier 5 — "caustics that converge").
//!
//! The GPU side of the light-tracing pass (`rt_caustic.wgsl`): each frame it
//! clears a per-pixel fixed-point splat buffer, traces N photons from the key
//! light through the scene's specular chain (the tracer's exact dielectric +
//! analytic-lens transport, per-λ Cauchy when spectral is on) depositing where
//! they land, then resolves the splats through a normalized disc blur (a
//! screen-space kernel-density estimate) into an HDR map the path tracer adds
//! into its progressive accumulation. Owned and driven by
//! `rt_pathtrace::PathTracer::trace` — the pass only exists while the path
//! tracer is active with caustics enabled; off → nothing is dispatched and the
//! tracer's output is byte-identical. Like every `rt_*` module, all
//! experimental ray-query call sites stay in here (and its shader).
//!
//! organon#217 T8b — **emitters as photon sources.** Photons no longer leave the key
//! light alone: while a glyph ring is live, `cs_cdf` builds a per-frame CDF over the
//! emissive instances in emitted power and each photon comes from a lit tile with
//! probability equal to the tiles' share of the scene's total power. The tracer has
//! no light list and no next-event estimation toward an emitter, so tile → glass →
//! floor is exactly the path a camera-first walk essentially never finds; this is the
//! other end of it. **No parameter turns it on** — the renderer's emissive
//! high-water mark does, so a frame with no ring is byte-identical down to the
//! photons' random stream.

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CaU {
    view_proj: [[f32; 4]; 4], // UNJITTERED forward VP (deposit projection)
    sphere: [f32; 4],         // scene bounding sphere (xyz, radius) — emission disc
    lens0: [f32; 4],          // analytic lens centre + active flag (#258 T3)
    lens1: [f32; 4],          // lens shape (r, dz, aperture, plano)
    spectral: [f32; 4],       // spectral_on, Abbe number (#258 T4), tube flag
    params_a: [f32; 4],       // absorption, photon count, frame seed, pixel scale
    params_b: [f32; 4],       // intensity, gather radius (px), width, height
    emit: [f32; 4],           // organon#217 T8b: live emissive instance count (0 = none)
}

/// organon#217 T8b — where this pass's group 0 carries the per-instance emission it
/// samples photon SOURCES from (`rt_caustic.wgsl`'s `emits`), and the per-frame power
/// CDF built over it. The layout comment at binding 5 named 6 before either existed;
/// these two constants are what the layout, the bind group and the tests all read.
pub(crate) const EMIT_BINDING: u32 = 6;
pub(crate) const CDF_BINDING: u32 = 7;

pub struct CausticMap {
    cdf_pipeline: wgpu::ComputePipeline,
    photon_pipeline: wgpu::ComputePipeline,
    resolve_pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    tlas_layout: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    splat_buf: Option<wgpu::Buffer>,
    // The per-frame emitter CDF (one f32 per instance), grown to the emissive count.
    cdf_buf: Option<wgpu::Buffer>,
    cdf_cap: u32,
    out_view: Option<wgpu::TextureView>,
    size: (u32, u32),
}

impl CausticMap {
    /// The group-0 layout as data — one entry per `@binding` the shader declares.
    /// Pure (no device) for the same reason `rt_pathtrace::layout_entries` is: wgpu
    /// validates a bind group against its layout at **dispatch** time, so an entry
    /// here with no matching `create_bind_group` entry — or a shader `@binding` that
    /// disagrees — is a runtime panic on a GPU no leg of the bar has.
    pub(crate) fn layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage_entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        vec![
            uniform_entry(0),
            uniform_entry(1),
            storage_entry(2, false), // photon splat accumulator (atomics)
            storage_entry(3, true),  // instances
            storage_entry(4, true),  // tints
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
            // organon#217 T8b — the emit buffer, read-only, exactly where the layout
            // comment that stood here said it would go. ⚠️ Read to SAMPLE with, never
            // to SHADE with: `shade_hit` below is the landing surface's BSDF and that
            // surface's own emission still plays no part in a photon's transport. What
            // it buys is the other end — a lit tile is a photon SOURCE, sampled in
            // proportion to its emitted power, so tile → glass → floor converges. The
            // tracer binds the same buffer at 7 (`rt_pathtrace::EMIT_BINDING`).
            storage_entry(EMIT_BINDING, true),
            // The CDF `cs_cdf` writes and `cs_photon` binary-searches. Read-write: one
            // pass produces it and the next consumes it, inside the one compute pass.
            storage_entry(CDF_BINDING, false),
        ]
    }

    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-caustic-bgl"),
            entries: &Self::layout_entries(),
        });
        let tlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-caustic-tlas-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-caustic-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_caustic.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-caustic-pl"),
            bind_group_layouts: &[Some(&layout), Some(&tlas_layout)],
            immediate_size: 0,
        });
        let mk = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("rt-caustic-pipeline"),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let cdf_pipeline = mk("cs_cdf");
        let photon_pipeline = mk("cs_photon");
        let resolve_pipeline = mk("cs_resolve");
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-caustic-uniforms"),
            size: std::mem::size_of::<CaU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        CausticMap {
            cdf_pipeline,
            photon_pipeline,
            resolve_pipeline,
            layout,
            tlas_layout,
            ubuf,
            splat_buf: None,
            cdf_buf: None,
            cdf_cap: 0,
            out_view: None,
            size: (0, 0),
        }
    }

    /// Keep the emitter CDF at least `n` entries long. Always at least one entry: the
    /// layout declares the binding whether or not anything is emitting, and wgpu
    /// rejects a zero-sized storage binding — the uniform's count, not the buffer's
    /// length, is what says "no emitters" (`live_emitters` in the shader).
    fn ensure_cdf(&mut self, device: &wgpu::Device, n: u32) {
        let want = n.max(1);
        if self.cdf_buf.is_some() && self.cdf_cap >= want {
            return;
        }
        self.cdf_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-caustic-emitter-cdf"),
            size: (want as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        }));
        self.cdf_cap = want;
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.size == size && self.splat_buf.is_some() {
            return;
        }
        let (w, h) = (size.0.max(1), size.1.max(1));
        self.splat_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-caustic-splats"),
            size: (w as u64) * (h as u64) * 3 * 4, // 3 × u32 fixed point per pixel
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.out_view = Some(
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("rt-caustic-map"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        self.size = size;
    }

    /// The resolved caustic map (valid after `run` this frame).
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.out_view.as_ref()
    }

    /// Trace + splat + resolve one frame of photons. `scene_ubuf` is the path
    /// tracer's verbatim scene-uniform copy (already written this frame);
    /// `p` is the tracer's frame (lens / spectral / caustic dials).
    ///
    /// organon#217 T8b: `emit_buf` is the cube pipeline's per-instance emission and
    /// `emit_lit` is how many of its entries this frame may have non-zero emission —
    /// the renderer's high-water mark, which is the glyph frame's instance count and
    /// **0 on every other frame**. That zero is the whole inertness story: no CDF is
    /// dispatched, the uniform says there are no emitters, and the photon pass takes
    /// the pre-T8b branch consuming the pre-T8b random stream. It is deliberately a
    /// count rather than a parameter — there is no dial to turn on, only a ring to be
    /// live (invariant #4).
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        scene_ubuf: &wgpu::Buffer,
        inst_buf: &wgpu::Buffer,
        tint_buf: &wgpu::Buffer,
        emit_buf: &wgpu::Buffer,
        emit_lit: u32,
        tube: bool,
        p: &super::rt_pathtrace::PathtraceFrame,
    ) {
        self.ensure_targets(device, size);
        // The buffer is the belt to the count's braces: a mark that outran the buffer
        // would have the shader index past `emits`, and the shader clamps too.
        let emitters = emit_lit.min((emit_buf.size() / 16) as u32);
        self.ensure_cdf(device, emitters);
        let photons = (p.caustic[1].max(1.0) as u32).clamp(1024, 2_097_152);
        let u = CaU {
            view_proj: p.unjittered_view_proj,
            sphere: p.scene_sphere,
            lens0: [p.lens[0], p.lens[1], p.lens[2], p.lens[3]],
            lens1: [p.lens[4], p.lens[5], p.lens[6], p.lens[7]],
            spectral: [p.spectral[0], p.spectral[1], if tube { 1.0 } else { 0.0 }, 0.0],
            params_a: [p.pt_absorb.max(0.0), photons as f32, p.frame as f32, p.pixel_scale],
            params_b: [
                p.caustic[2].max(0.0),
                p.caustic[3].clamp(0.0, 8.0),
                size.0.max(1) as f32,
                size.1.max(1) as f32,
            ],
            emit: [emitters as f32, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));

        let splats = self.splat_buf.as_ref().unwrap();
        encoder.clear_buffer(splats, 0, None);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-caustic-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: scene_ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: splats.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: inst_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: tint_buf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(self.out_view.as_ref().unwrap()),
                },
                // organon#217 T8b: every entry the layout declares must be supplied
                // here whether or not anything is emitting, or wgpu rejects the bind
                // group at dispatch.
                wgpu::BindGroupEntry { binding: EMIT_BINDING, resource: emit_buf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: CDF_BINDING,
                    resource: self.cdf_buf.as_ref().unwrap().as_entire_binding(),
                },
            ],
        });
        let tlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-caustic-tlas-bind"),
            layout: &self.tlas_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::AccelerationStructure(p.tlas),
            }],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("rt-caustic-pass"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &tlas_bind, &[]);
        // organon#217 T8b: build the emitter CDF before the photons that read it —
        // one workgroup, two passes over the live emissive instances, and skipped
        // entirely when there are none. Dispatches within one compute pass are
        // ordered and their writes visible to the next, which is what the splat
        // buffer's photon → resolve hand-off below already relies on.
        if emitters > 0 {
            pass.set_pipeline(&self.cdf_pipeline);
            pass.dispatch_workgroups(1, 1, 1);
        }
        pass.set_pipeline(&self.photon_pipeline);
        pass.dispatch_workgroups(photons.div_ceil(64), 1, 1);
        pass.set_pipeline(&self.resolve_pipeline);
        pass.dispatch_workgroups(size.0.max(1).div_ceil(8), size.1.max(1).div_ceil(8), 1);
    }
}

#[cfg(test)]
mod tests {
    //! organon#217 T8b. Nothing here has a GPU: what the CPU can hold is the layout
    //! against the shader that reads it, and the two properties the whole tier rests
    //! on — that a frame with no emitters is byte-identical to the pass before it
    //! existed, and that the emission is read to SAMPLE with and never to shade with.
    use super::*;

    const SHADER: &str = include_str!("rt_caustic.wgsl");
    const SRC: &str = include_str!("rt_caustic.rs");
    /// The hand-off site lives one module up, and so does the only way to get the
    /// count wrong — see `the_photon_source_count_is_this_frames_upload`. The test
    /// sits here rather than in `render.rs` because the invariant belongs to this
    /// pass: `render.rs` has no reason to know that indexing without a hit test is
    /// what makes a stale count dangerous.
    const RENDER: &str = include_str!("render.rs");

    /// A shader `struct`'s body, `\r` stripped — this checkout is CRLF on Windows and
    /// LF elsewhere, and every property below is about the text, not the platform.
    fn struct_body(name: &str) -> String {
        let s = SHADER.replace('\r', "");
        let start = s.find(&format!("struct {name} {{")).expect("no such struct");
        let end = s[start..].find("\n};").expect("unterminated struct") + start;
        s[start..end].to_string()
    }

    #[test]
    fn the_photon_pass_binds_the_emit_buffer_where_its_shader_reads_it() {
        let entries = CausticMap::layout_entries();
        // Compute-visible, not fragment: the photon walk is a compute entry point.
        super::super::rt_pathtrace::emit_binding::check_visible(
            "rt_caustic",
            &entries,
            EMIT_BINDING,
            SHADER,
            wgpu::ShaderStages::COMPUTE,
        );
        // Six bindings before T8b (scene, params, splats, insts, tints, out) + emits + cdf.
        assert_eq!(entries.len(), 8, "rt_caustic: group 0 should carry exactly eight entries");
    }

    #[test]
    fn the_emitter_cdf_is_read_write_storage_where_both_dispatches_meet() {
        // `cs_cdf` writes it and `cs_photon` binary-searches it inside the same compute
        // pass, so it cannot be declared read-only on either side.
        let entries = CausticMap::layout_entries();
        let e = entries.iter().find(|e| e.binding == CDF_BINDING).expect("no CDF layout entry");
        assert_eq!(
            e.ty,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        );
        assert_eq!(e.visibility, wgpu::ShaderStages::COMPUTE);
        assert!(SHADER.contains(&format!(
            "@group(0) @binding({CDF_BINDING}) var<storage, read_write> cdf: array<f32>;"
        )));
    }

    #[test]
    fn a_frame_with_no_emitters_draws_no_extra_randomness() {
        // 🚨 The property invariant #4 rests on, and it is one line deep: the source
        // choice consumes a `rand` only when something is actually emitting, so an
        // ordinary Organon frame walks the identical random stream and every caustic
        // lands where it landed before T8b. Short-circuiting `&&` would read the same
        // and hide the intent, so the draw sits inside an explicit guard.
        let s = SHADER.replace('\r', "");
        assert!(s.contains("var from_emitter = false;\n"));
        assert!(
            s.contains(
                "if (e_total > 0.0) { from_emitter = rand(&seed) * (key_power + e_total) < e_total; }"
            ),
            "rt_caustic.wgsl: the emitter draw is no longer guarded by a live total"
        );
        // That guarded line is the ONLY draw outside the two pre-T8b disc draws, and the
        // only other emitter-side randomness is inside `sample_emitter`, which the
        // guarded branch is the sole caller of.
        assert_eq!(s.matches("rand(&seed) * (key_power + e_total)").count(), 1);
        assert_eq!(s.matches("sample_emitter(&seed").count(), 1);
        assert!(s.contains("    if (from_emitter) {\n        let src = sample_emitter(&seed"));
    }

    #[test]
    fn the_photon_budget_is_the_whole_scene_power_over_the_population() {
        // Every photon carries the same flux and the sources split the population, so
        // each deposits exactly its own power — and with `e_total` zero this reduces to
        // the pre-T8b `key.w · π r² / N`, the same expression in the same order.
        let s = SHADER.replace('\r', "");
        assert!(s.contains("let key_power = u.key_light.w * PI * radius * radius;\n"));
        assert!(s.contains("let flux = (key_power + e_total) / f32(n_photons);\n"));
    }

    #[test]
    fn an_emitter_photon_still_needs_a_specular_redirect_before_it_deposits() {
        // A photon straight from a tile onto the floor is DIRECT light from that tile,
        // and the tracer already finds it by hitting the tile. Only the redirected ones
        // are this pass's business, so the deposit gate must stay the single
        // `spec_events` test both sources pass through — no second, emitter-only path.
        let s = SHADER.replace('\r', "");
        assert_eq!(s.matches("if (spec_events > 0u) {").count(), 1);
        assert_eq!(s.matches("spec_events += 1u;").count(), 2);
    }

    #[test]
    fn the_uniform_block_and_the_shaders_struct_carry_the_same_slots() {
        // `CaU` is written with `bytemuck::bytes_of`, so a member added on one side and
        // not the other silently reinterprets every field after it.
        let body = struct_body("CaU");
        let vec4s = body.matches(": vec4<f32>,").count();
        let mat4s = body.matches(": mat4x4<f32>,").count();
        assert_eq!((mat4s, vec4s), (1, 7), "rt_caustic.wgsl's CaU changed shape");
        assert_eq!(std::mem::size_of::<CaU>(), 64 + 7 * 16);
        assert!(body.contains("emit: vec4<f32>,"), "the emitter count left the uniform");
    }

    #[test]
    fn the_photon_source_count_is_this_frames_upload_not_the_high_water_mark() {
        // 🚨 #250 review. `emit_hi` is a HIGH-WATER mark across frames — it exists so a
        // later, shorter upload knows how far to zero — and it is refreshed only on a
        // frame that actually uploads the instance buffers. Every raymarch/bake mode
        // and the hidden-generator case upload nothing, so on those frames the mark,
        // `inst_buf` and `emit_buf` all stay frozen at whatever a previous glyph-ring
        // frame left. The three passes that SHADE with emission are safe from that by
        // construction — they index only where a live TLAS hit pointed. This pass is
        // not: `cs_cdf`/`cs_photon` walk `insts[i]`/`emits[i]` directly, so a stale
        // count spawns photons from a ring that has left the scene. There is no GPU on
        // any leg of the bar, so the call site is the only place this is observable.
        // `\r` stripped first: this checkout is CRLF on Windows and LF elsewhere, and
        // every property below is about the argument list, not the platform.
        let render = RENDER.replace('\r', "");
        let call = render.find("pt.trace(").expect("render.rs no longer calls the tracer");
        let end = render[call..].find(");\n").expect("unterminated trace call") + call;
        // ⚠️ Comments stripped, and the reason is not hypothetical: the comment at that
        // call site NAMES `emit_hi` to say what must not be passed, which a naive scan
        // reads as the very thing it forbids. The invariant is about the code. (No
        // string literal appears in that argument list, so cutting at `//` is exact.)
        let code: String = render[call..end]
            .lines()
            .map(|l| l.split("//").next().unwrap_or("").trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        let args = code.as_str();
        assert!(
            !args.contains("emit_hi"),
            "render.rs hands the photon pass `emit_hi` — a mark that SURVIVES a frame \
             which uploads nothing, so a raymarch/bake frame after a glyph ring would \
             spawn photons from geometry no longer in the scene (a ghost caustic). \
             Pass this frame's own uploaded count instead:\n{args}"
        );
        assert!(
            args.contains("emit_sources"),
            "render.rs no longer hands the photon pass a per-frame source count:\n{args}"
        );
        // And that count defaults to ZERO before the upload branch, so a path added
        // later that skips the upload is inert without having to remember to say so.
        let decl = render.find("let mut emit_sources: u32 = 0;").expect(
            "the photon source count lost its fail-safe zero default — a new early \
             return or a new non-uploading mode would inherit the last ring's count",
        );
        let gate = render.find("if inst_gpu_used {").expect("the upload gate moved");
        assert!(decl < gate, "the source count is declared after the upload branch it guards");
    }

    #[test]
    fn the_cdf_dispatch_is_skipped_when_nothing_is_emitting() {
        // The other half of inertness, and the half a shader test cannot see: with no
        // ring live the CDF pass is not recorded at all, so a plain Organon frame does
        // no work for a feature it is not using.
        // ⚠️ Both needles are BUILT rather than written: this file includes itself, so a
        // literal here would match its own text and the count would be of the test.
        let dispatch = format!("set_pipeline(&self.{}_pipeline)", "cdf");
        let guard = format!("if emitters > {} {{", 0);
        assert_eq!(SRC.matches(dispatch.as_str()).count(), 1);
        let g = SRC.find(guard.as_str()).expect("the CDF dispatch is unguarded");
        assert!(g < SRC.find(dispatch.as_str()).unwrap(), "the CDF dispatch escaped its guard");
    }
}

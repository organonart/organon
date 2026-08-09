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
}

pub struct CausticMap {
    photon_pipeline: wgpu::ComputePipeline,
    resolve_pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    tlas_layout: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    splat_buf: Option<wgpu::Buffer>,
    out_view: Option<wgpu::TextureView>,
    size: (u32, u32),
}

impl CausticMap {
    pub fn new(device: &wgpu::Device) -> Self {
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-caustic-bgl"),
            entries: &[
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
            ],
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
        let photon_pipeline = mk("cs_photon");
        let resolve_pipeline = mk("cs_resolve");
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-caustic-uniforms"),
            size: std::mem::size_of::<CaU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        CausticMap {
            photon_pipeline,
            resolve_pipeline,
            layout,
            tlas_layout,
            ubuf,
            splat_buf: None,
            out_view: None,
            size: (0, 0),
        }
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
        tube: bool,
        p: &super::rt_pathtrace::PathtraceFrame,
    ) {
        self.ensure_targets(device, size);
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
        pass.set_pipeline(&self.photon_pipeline);
        pass.dispatch_workgroups(photons.div_ceil(64), 1, 1);
        pass.set_pipeline(&self.resolve_pipeline);
        pass.dispatch_workgroups(size.0.max(1).div_ceil(8), size.1.max(1).div_ceil(8), 1);
    }
}

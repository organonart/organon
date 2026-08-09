//! Reaction–diffusion (Gray–Scott) surface patterning. Runs a Turing-pattern sim
//! on a tiling 2-D texture (compute, ping-pong), which the lit shaders sample
//! triplanar-projected onto the surface as crawling HDR emissive dapple — spots,
//! stripes, or labyrinths depending on the feed/kill rates.
//!
//! Included by `render.rs` via `#[path]`, like `metaball`/`post`/`env`. Exposes a
//! `scene_layout()` (added to the cube + metaball pipeline layouts) and a
//! `scene_bind()` (the current field texture + sampler + look uniforms) that the
//! shaders read at group 2 (cube) / group 3 (metaball).

use bytemuck::{Pod, Zeroable};

/// Simulation grid resolution (square, tiling).
pub const RD_SIZE: u32 = 256;
/// Sim steps per frame. MUST be even so the ping-pong ends back in texture A (the
/// one the scene samples), keeping the scene bind group stable.
const STEPS_PER_FRAME: u32 = 12;

/// Live reaction–diffusion params from the editor.
#[derive(Clone, Copy)]
pub struct RdParams {
    pub feed: f32,
    pub kill: f32,
    pub diffuse_u: f32,
    pub diffuse_v: f32,
    pub dt: f32,
    pub intensity: f32,  // HDR emissive gain of the pattern (0 = off)
    pub scale: f32,      // world→pattern frequency (triplanar)
    pub albedo_mix: f32, // 0 = pure emissive add; up = also tint albedo
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RdSim {
    rates: [f32; 4], // feed, kill, diffuse_u, diffuse_v
    misc: [f32; 4],  // dt, size, _, _
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RdLook {
    params: [f32; 4], // intensity, scale, albedo_mix, _
}

pub struct RdField {
    size: u32,
    sim_ub: wgpu::Buffer,
    look_ub: wgpu::Buffer,
    step_pipeline: wgpu::ComputePipeline,
    seed_pipeline: wgpu::ComputePipeline,
    bind_ab: wgpu::BindGroup, // front = A, back = B
    bind_ba: wgpu::BindGroup, // front = B, back = A (also used to seed A)
    scene_bgl: wgpu::BindGroupLayout,
    scene_bind: wgpu::BindGroup, // samples A (the stable, end-of-frame texture)
    seeded: bool,
}

impl RdField {
    pub fn new(device: &wgpu::Device) -> Self {
        let size = RD_SIZE;
        let mk_tex = |label: &str| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let a = mk_tex("rd-a");
        let b = mk_tex("rd-b");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rd-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let sim_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rd-sim-ub"),
            size: std::mem::size_of::<RdSim>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let look_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rd-look-ub"),
            size: std::mem::size_of::<RdLook>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Compute (sim) ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rd"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rd.wgsl").into()),
        });
        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rd-compute-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let compute_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rd-compute-pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let step_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("rd-step"),
            layout: Some(&compute_pl),
            module: &shader,
            entry_point: Some("cs_step"),
            compilation_options: Default::default(),
            cache: None,
        });
        let seed_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("rd-seed"),
            layout: Some(&compute_pl),
            module: &shader,
            entry_point: Some("cs_seed"),
            compilation_options: Default::default(),
            cache: None,
        });

        let mk_compute_bind = |front: &wgpu::TextureView, back: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rd-compute-bind"),
                layout: &compute_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(front) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(back) },
                    wgpu::BindGroupEntry { binding: 2, resource: sim_ub.as_entire_binding() },
                ],
            })
        };
        let bind_ab = mk_compute_bind(&a, &b);
        let bind_ba = mk_compute_bind(&b, &a);

        // --- Scene-facing bind group (samples A) ---
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rd-scene-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rd-scene-bind"),
            layout: &scene_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&a) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: look_ub.as_entire_binding() },
            ],
        });

        RdField {
            size,
            sim_ub,
            look_ub,
            step_pipeline,
            seed_pipeline,
            bind_ab,
            bind_ba,
            scene_bgl,
            scene_bind,
            seeded: false,
        }
    }

    /// The scene bind-group layout, appended to the cube + metaball pipeline
    /// layouts so their fragment shaders can sample the field.
    pub fn scene_layout(&self) -> &wgpu::BindGroupLayout {
        &self.scene_bgl
    }
    pub fn scene_bind(&self) -> &wgpu::BindGroup {
        &self.scene_bind
    }

    /// Upload the live params + advance the sim. On the first call (and whenever a
    /// reseed is forced) the field is seeded. Runs an even number of steps so the
    /// final state lands in texture A (what `scene_bind` samples).
    pub fn step(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        p: &RdParams,
    ) {
        queue.write_buffer(
            &self.sim_ub,
            0,
            bytemuck::bytes_of(&RdSim {
                rates: [p.feed, p.kill, p.diffuse_u, p.diffuse_v],
                misc: [p.dt.max(0.0), self.size as f32, 0.0, 0.0],
            }),
        );
        queue.write_buffer(
            &self.look_ub,
            0,
            bytemuck::bytes_of(&RdLook {
                params: [p.intensity, p.scale.max(1e-4), p.albedo_mix, 0.0],
            }),
        );

        let wg = self.size.div_ceil(8);
        // Seed once: cs_seed writes texture A via the BA bind (back = A).
        if !self.seeded {
            self.seeded = true;
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("rd-seed-pass"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&self.seed_pipeline);
            cp.set_bind_group(0, &self.bind_ba, &[]);
            cp.dispatch_workgroups(wg, wg, 1);
        }

        // Ping-pong steps. Start A→B; even total ends writing A.
        for i in 0..STEPS_PER_FRAME {
            let bind = if i % 2 == 0 { &self.bind_ab } else { &self.bind_ba };
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("rd-step-pass"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&self.step_pipeline);
            cp.set_bind_group(0, bind, &[]);
            cp.dispatch_workgroups(wg, wg, 1);
        }
    }
}

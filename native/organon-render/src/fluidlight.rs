//! Fluid light coupling (#182 Tier 4) — host for `fluidlight.wgsl`.
//!
//! Owns the light-space RGBA16F map (r = dye transmittance, g = caustic), the
//! caustic splat accumulator, and the two compute pipelines. The map rides the
//! cube pipeline's **group 4** (the shadow group — `shadow.rs` binds it), so
//! scene geometry pays one extra sample only when either amount is non-zero.
//! Included by `render.rs` via `#[path]` (visual binary only).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Light-space map resolution (square) — shared by transmittance + caustic.
pub const FLUIDLIGHT_RES: u32 = 256;

/// Matches `FlU` in fluidlight.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FlU {
    inv_light_vp: [[f32; 4]; 4],
    light_vp: [[f32; 4]; 4],
    dye0: [f32; 4],
    dye1: [f32; 4],
    liq0: [f32; 4],
    liq1: [f32; 4],
    misc: [f32; 4],
}

/// Per-frame inputs for the light-space passes.
pub struct FluidLightFrame<'a> {
    pub light_vp: Mat4,
    /// Ink dye source: AABB + Beer–Lambert σ; `trans_on` gates the march.
    pub dye_min: Vec3,
    pub dye_max: Vec3,
    pub extinction: f32,
    pub trans_on: bool,
    /// Liquid source: AABB + IOR + iso threshold; `caustic_on` gates the splat.
    pub liq_min: Vec3,
    pub liq_max: Vec3,
    pub ior: f32,
    pub threshold: f32,
    pub caustic_on: bool,
    pub caustic_sharpness: f32,
    pub dye_view: &'a wgpu::TextureView,
    /// Bumped when the dye texture is recreated (fluid-res change).
    pub dye_gen: u64,
    pub liq_view: &'a wgpu::TextureView,
}

pub struct FluidLight {
    ubo: wgpu::Buffer,
    accum: wgpu::Buffer,
    out_view: wgpu::TextureView,
    samp: wgpu::Sampler,
    caustic_pipeline: wgpu::ComputePipeline,
    resolve_pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    bind: Option<(u64, wgpu::BindGroup)>, // keyed by dye texture generation
}

impl FluidLight {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fluidlight"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fluidlight.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluidlight-ubo"),
            size: std::mem::size_of::<FlU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let accum = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluidlight-accum"),
            size: (FLUIDLIGHT_RES as u64).pow(2) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fluidlight-map"),
            size: wgpu::Extent3d {
                width: FLUIDLIGHT_RES,
                height: FLUIDLIGHT_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let out_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fluidlight-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let tex3 = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D3,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluidlight-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex3(1),
                tex3(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluidlight-pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let mk = |label: &str, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        FluidLight {
            ubo,
            accum,
            out_view,
            samp,
            caustic_pipeline: mk("fluidlight-caustic", "cs_caustic"),
            resolve_pipeline: mk("fluidlight-resolve", "cs_resolve"),
            bgl,
            bind: None,
        }
    }

    /// The light-space map (r = transmittance, g = caustic) for group 4.
    pub fn map_view(&self) -> &wgpu::TextureView {
        &self.out_view
    }
    /// A filtering sampler for the map (shared with group 4).
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.samp
    }

    /// Run the light-space passes (call between the solver steps and the scene
    /// pass, only when `trans_on || caustic_on` — the cube shader's amounts are
    /// zeroed whenever this did not run, so a stale map is never read).
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        f: &FluidLightFrame,
    ) {
        let u = FlU {
            inv_light_vp: f.light_vp.inverse().to_cols_array_2d(),
            light_vp: f.light_vp.to_cols_array_2d(),
            dye0: [f.dye_min.x, f.dye_min.y, f.dye_min.z, f.extinction.max(0.0)],
            dye1: [f.dye_max.x, f.dye_max.y, f.dye_max.z, f.trans_on as u32 as f32],
            liq0: [f.liq_min.x, f.liq_min.y, f.liq_min.z, f.ior.max(1.01)],
            liq1: [f.liq_max.x, f.liq_max.y, f.liq_max.z, f.threshold.max(1e-3)],
            misc: [
                if f.caustic_on { 1.0 } else { 0.0 },
                f.caustic_sharpness.max(0.1),
                FLUIDLIGHT_RES as f32,
                0.0,
            ],
        };
        queue.write_buffer(&self.ubo, 0, bytemuck::bytes_of(&u));

        if self.bind.as_ref().map(|(k, _)| *k) != Some(f.dye_gen) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fluidlight-bind"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(f.dye_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(f.liq_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.samp),
                    },
                    wgpu::BindGroupEntry { binding: 4, resource: self.accum.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&self.out_view),
                    },
                ],
            });
            self.bind = Some((f.dye_gen, bind));
        }

        let wg = FLUIDLIGHT_RES.div_ceil(8);
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fluidlight-pass"),
            timestamp_writes: None,
        });
        cpass.set_bind_group(0, &self.bind.as_ref().unwrap().1, &[]);
        if f.caustic_on {
            cpass.set_pipeline(&self.caustic_pipeline);
            cpass.dispatch_workgroups(wg, wg, 1);
        }
        cpass.set_pipeline(&self.resolve_pipeline);
        cpass.dispatch_workgroups(wg, wg, 1);
    }
}

//! Voxel global illumination (#152 Tier 3, #10).
//!
//! Self-contained world-space GI: a compute pass voxelizes the node set into a small
//! 3D colour+occupancy volume (`vxgi.wgsl::cs_voxelize`), then a fullscreen pass
//! (`fs_vxgi`) reconstructs each pixel's world position from the depth prepass,
//! marches a few hemisphere rays through the volume, and **adds** the gathered bounce
//! straight into the HDR scene buffer (additive blend, RGB-only write) so it blooms +
//! tonemaps with the scene. Because the volume holds the whole field, the bounce
//! captures light from off-screen / occluded emitters that SSGI misses.
//!
//! Touches neither `cube.wgsl` nor the composite. Off → the passes aren't run.
//! Included by `render.rs` via `#[path]`. Reuses `MetaNode` (pos + colour) as the
//! voxelize input, which the visual already builds for the metaball/voxel modes.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::MetaNode;

/// The scene HDR buffer format (matches `post::HDR_FORMAT`) — the additive gather
/// pass targets it.
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Voxel grid resolution (cubic). 32³ = 32k cells — GI is low-frequency, so this is
/// plenty and keeps the per-frame gather (O(cells·nodes)) cheap.
pub const VXGI_RES: u32 = 32;
/// Cap on nodes fed to the voxelizer (stride-subsampled above this).
pub const MAX_VXGI_NODES: usize = 2048;

/// Live GI look params (from the editor).
#[derive(Clone, Copy)]
pub struct VxgiParams {
    pub intensity: f32,
    pub rays: f32,
    pub steps: f32,
    pub max_dist: f32, // world-space march distance
    // #163 Tier 3 — specular cone tracing (reflections). Off when strength = 0.
    pub spec_strength: f32,
    pub spec_aperture: f32,
    pub spec_max_dist: f32,
    pub spec_steps: f32,
}

/// Matches `VoxU` (compute) in vxgi.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VoxU {
    bmin: [f32; 4],
    bmax: [f32; 4],
    grid: [f32; 4], // xyz = res, w = node count
    // Fluid → GI (#182 T4): source AABBs; dye0.w / dye1.w = dye / liquid gains.
    dye0: [f32; 4],
    dye1: [f32; 4],
    liq0: [f32; 4],
    liq1: [f32; 4],
}

/// Matches `GiU` (gather) in vxgi.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GiU {
    inv_vp: [[f32; 4]; 4],
    bmin: [f32; 4],
    bmax: [f32; 4],
    params: [f32; 4], // intensity, rays, steps, max_dist
    texel: [f32; 4],
    // #163 Tier 3 — specular cone tracing.
    cam: [f32; 4],  // xyz = camera world position (for the reflection ray)
    spec: [f32; 4], // strength, aperture, max_dist, steps (all 0 strength → off)
}

/// Matches `VxSampleU` in metaball.wgsl (#182 T4: external volume sampling).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VxSampleU {
    bmin: [f32; 4], // xyz = volume min, w = sample gain (0 = off/stale)
    bmax: [f32; 4], // xyz = volume max
}

pub struct Vxgi {
    res: u32,
    vox_view: wgpu::TextureView,
    // #182 T4: external volume sampling (metaball/liquid isosurface).
    sample_ub: wgpu::Buffer,
    sample_layout: wgpu::BindGroupLayout,
    sample_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    nearest: wgpu::Sampler,
    // voxelize (compute): scatter per node → fixed-point atomics, then resolve
    // per cell → the volume texture (#174 T2 — replaced the O(cells×nodes) gather).
    scatter_pipeline: wgpu::ComputePipeline,
    resolve_pipeline: wgpu::ComputePipeline,
    compute_bgl: wgpu::BindGroupLayout,
    vox_ub: wgpu::Buffer,
    node_buf: wgpu::Buffer,
    accum_buf: wgpu::Buffer,
    node_cap: usize,
    compute_bind: Option<wgpu::BindGroup>,
    // Cache key for the lazy compute bind: (node cap, dye texture generation) —
    // the dye 3D texture is recreated on a res change (#182 T4 injection).
    compute_bind_key: (usize, u64),
    // gather (fullscreen, additive into HDR)
    gi_pipeline: wgpu::RenderPipeline,
    gi_bgl: wgpu::BindGroupLayout,
    gi_ub: wgpu::Buffer,
    node_count: u32,
    // Cached gather bind group (#174 T2 — was recreated every frame), keyed by the
    // renderer's depth key (the depth view is the only externally-owned resource
    // it references; it rebuilds on every size / MSAA / prepass-route change).
    gather_bind: Option<(u64, wgpu::BindGroup)>,
}

impl Vxgi {
    pub fn new(device: &wgpu::Device) -> Self {
        let res = VXGI_RES;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vxgi"),
            source: wgpu::ShaderSource::Wgsl(include_str!("vxgi.wgsl").into()),
        });

        let vox_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vxgi-volume"),
            size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: res },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let vox_view = vox_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vxgi-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vxgi-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let vox_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vxgi-vox-ub"),
            size: std::mem::size_of::<VoxU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gi_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vxgi-gi-ub"),
            size: std::mem::size_of::<GiU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let node_cap = 1024usize;
        let node_buf = make_node_buf(device, node_cap);
        // Scatter accumulator: 4 fixed-point u32 per cell (zero-initialized by
        // wgpu; cs_resolve re-zeroes it every frame via atomicExchange).
        let accum_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vxgi-accum"),
            size: (res as u64).pow(3) * 4 * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // --- compute (voxelize: scatter + resolve) ---
        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vxgi-compute-layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D3,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Fluid → GI (#182 T4): dye + liquid 3D fields + a linear sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let compute_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vxgi-compute-pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let mk_compute = |label: &str, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&compute_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let scatter_pipeline = mk_compute("vxgi-scatter", "cs_scatter");
        let resolve_pipeline = mk_compute("vxgi-resolve", "cs_resolve");
        // Built lazily in `voxelize` — the fluid source textures (#182 T4)
        // don't exist yet at construction time.
        let compute_bind = None;

        // --- gather (fullscreen, additive into HDR) ---
        let gi_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vxgi-gi-layout"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let gi_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vxgi-gi-pl"),
            bind_group_layouts: &[Some(&gi_bgl)],
            immediate_size: 0,
        });
        // Additive blend, RGB-only write so the scene's coverage alpha is untouched.
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let gi_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vxgi-gi-pipeline"),
            layout: Some(&gi_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_vxgi"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(add),
                    write_mask: wgpu::ColorWrites::COLOR, // R|G|B only, not alpha
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // #182 T4: the external-sampling group (uniform + volume + linear
        // sampler) for surfaces outside the cube pipeline (metaball/liquid).
        let sample_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vxgi-sample-ub"),
            size: std::mem::size_of::<VxSampleU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sample_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vxgi-sample-layout"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sample_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vxgi-sample-bind"),
            layout: &sample_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sample_ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&vox_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        Vxgi {
            res,
            vox_view,
            sample_ub,
            sample_layout,
            sample_bind,
            sampler,
            nearest,
            scatter_pipeline,
            resolve_pipeline,
            compute_bgl,
            vox_ub,
            node_buf,
            accum_buf,
            node_cap,
            compute_bind,
            compute_bind_key: (0, u64::MAX),
            gi_pipeline,
            gi_bgl,
            gi_ub,
            node_count: 0,
            gather_bind: None,
        }
    }

    /// Group layout for external consumers sampling the radiance volume
    /// (#182 T4: the metaball/liquid isosurface) — uniform + volume + sampler.
    pub fn sample_layout(&self) -> &wgpu::BindGroupLayout {
        &self.sample_layout
    }
    pub fn sample_bind(&self) -> &wgpu::BindGroup {
        &self.sample_bind
    }
    /// Update the external-sampling uniform (call once per frame; `gain` 0
    /// whenever the volume wasn't voxelized this frame, so stale radiance is
    /// never read).
    pub fn set_sample(&self, queue: &wgpu::Queue, bmin: Vec3, bmax: Vec3, gain: f32) {
        queue.write_buffer(
            &self.sample_ub,
            0,
            bytemuck::bytes_of(&VxSampleU {
                bmin: [bmin.x, bmin.y, bmin.z, gain.max(0.0)],
                bmax: [bmax.x, bmax.y, bmax.z, 0.0],
            }),
        );
    }

    /// The radiance volume (rgb = bounce radiance, a = occupancy) — #182 T4:
    /// the ink march samples it directly (the world-space twin of the gather).
    pub fn volume_view(&self) -> &wgpu::TextureView {
        &self.vox_view
    }
    /// A linear sampler for the volume (shared with the gather).
    pub fn linear_sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// Voxelize the node set into the volume (compute). Call once per frame before
    /// `render`. `nodes` may exceed `MAX_VXGI_NODES` — it's stride-subsampled.
    /// `radiance_scale` converts the node TINT (an albedo) into injected radiance —
    /// glow + a key-light estimate, so an unlit non-emissive field doesn't "bounce".
    pub fn voxelize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        nodes: &[MetaNode],
        bmin: Vec3,
        bmax: Vec3,
        radiance_scale: f32,
        fluid: &FluidGiSources,
    ) {
        let mut packed: Vec<MetaNode>;
        let used: &[MetaNode] = if nodes.len() > MAX_VXGI_NODES {
            let stride = nodes.len().div_ceil(MAX_VXGI_NODES);
            packed = nodes.iter().step_by(stride).copied().collect();
            packed.truncate(MAX_VXGI_NODES);
            &packed
        } else {
            nodes
        };
        self.node_count = used.len() as u32;
        // #182 T4: fluid injection alone still wants the resolve pass (a pure
        // ink/liquid scene with the generator hidden has no nodes).
        let fluid_inject = fluid.dye_gain > 0.0 || fluid.liq_gain > 0.0;
        if used.is_empty() && !fluid_inject {
            return;
        }
        if used.len() > self.node_cap {
            self.node_cap = used.len().next_power_of_two();
            self.node_buf = make_node_buf(device, self.node_cap);
            self.compute_bind = None;
        }
        // (Re)build the compute bind when the node buffer or the dye texture
        // changed (the dye 3D texture is recreated on a fluid-res change).
        let key = (self.node_cap, fluid.dye_gen);
        if self.compute_bind.is_none() || self.compute_bind_key != key {
            self.compute_bind_key = key;
            self.compute_bind = Some(make_compute_bind(
                device, &self.compute_bgl, &self.vox_ub, &self.node_buf, &self.vox_view,
                &self.accum_buf, fluid.dye_view, fluid.liq_view, &self.sampler,
            ));
        }
        if !used.is_empty() {
            queue.write_buffer(&self.node_buf, 0, bytemuck::cast_slice(used));
        }
        queue.write_buffer(
            &self.vox_ub,
            0,
            bytemuck::bytes_of(&VoxU {
                bmin: [bmin.x, bmin.y, bmin.z, radiance_scale.max(0.0)],
                bmax: [bmax.x, bmax.y, bmax.z, 0.0],
                grid: [self.res as f32, self.res as f32, self.res as f32, self.node_count as f32],
                dye0: [fluid.dye_min.x, fluid.dye_min.y, fluid.dye_min.z, fluid.dye_gain.max(0.0)],
                dye1: [fluid.dye_max.x, fluid.dye_max.y, fluid.dye_max.z, fluid.liq_gain.max(0.0)],
                liq0: [fluid.liq_min.x, fluid.liq_min.y, fluid.liq_min.z, 0.0],
                liq1: [fluid.liq_max.x, fluid.liq_max.y, fluid.liq_max.z, 0.0],
            }),
        );
        let wg = self.res.div_ceil(4);
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("vxgi-voxelize-pass"),
            timestamp_writes: None,
        });
        cpass.set_bind_group(0, self.compute_bind.as_ref().unwrap(), &[]);
        // Scatter (one thread per node) → resolve (one thread per cell). Dispatch
        // ordering within a pass gives the resolve visibility of the scatter's
        // atomic writes.
        if self.node_count > 0 {
            cpass.set_pipeline(&self.scatter_pipeline);
            cpass.dispatch_workgroups(self.node_count.div_ceil(64), 1, 1);
        }
        cpass.set_pipeline(&self.resolve_pipeline);
        cpass.dispatch_workgroups(wg, wg, wg);
    }

    /// Gather + add the bounce into `target` (the resolved HDR buffer). Reads the
    /// single-sample `depth` prepass. No-op if the last `voxelize` had no nodes.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        depth_key: u64,
        inv_view_proj: Mat4,
        cam_pos: Vec3,
        bmin: Vec3,
        bmax: Vec3,
        size: (u32, u32),
        p: &VxgiParams,
    ) {
        // Run when either the diffuse bounce OR the specular reflection is active.
        if self.node_count == 0 || (p.intensity <= 0.0 && p.spec_strength <= 0.0) {
            return;
        }
        queue.write_buffer(
            &self.gi_ub,
            0,
            bytemuck::bytes_of(&GiU {
                inv_vp: inv_view_proj.to_cols_array_2d(),
                bmin: [bmin.x, bmin.y, bmin.z, 0.0],
                bmax: [bmax.x, bmax.y, bmax.z, 0.0],
                params: [p.intensity, p.rays, p.steps, p.max_dist],
                texel: [1.0 / size.0 as f32, 1.0 / size.1 as f32, size.0 as f32, size.1 as f32],
                cam: [cam_pos.x, cam_pos.y, cam_pos.z, 0.0],
                spec: [p.spec_strength, p.spec_aperture, p.spec_max_dist, p.spec_steps],
            }),
        );
        if self.gather_bind.as_ref().map(|(k, _)| *k != depth_key).unwrap_or(true) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vxgi-gi-bind"),
                layout: &self.gi_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.gi_ub.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.nearest) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.vox_view) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.gather_bind = Some((depth_key, bind));
        }
        let bind = &self.gather_bind.as_ref().unwrap().1;
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vxgi-gather-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.gi_pipeline);
        rp.set_bind_group(0, bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

fn make_node_buf(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vxgi-nodes"),
        size: (cap * std::mem::size_of::<MetaNode>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn make_compute_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    vox_ub: &wgpu::Buffer,
    node_buf: &wgpu::Buffer,
    vox_view: &wgpu::TextureView,
    accum_buf: &wgpu::Buffer,
    dye_view: &wgpu::TextureView,
    liq_view: &wgpu::TextureView,
    samp: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vxgi-compute-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: vox_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: node_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(vox_view) },
            wgpu::BindGroupEntry { binding: 3, resource: accum_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(dye_view) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(liq_view) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(samp) },
        ],
    })
}

/// Fluid → GI (#182 T4): the two extra injection sources `voxelize` samples —
/// the ink dye 3D texture and the liquid density `MetaField` texture, each with
/// its world AABB and an inject gain (0 = that source off).
pub struct FluidGiSources<'a> {
    pub dye_view: &'a wgpu::TextureView,
    /// Bumped when the dye texture is recreated (fluid-res change).
    pub dye_gen: u64,
    pub dye_min: Vec3,
    pub dye_max: Vec3,
    pub dye_gain: f32,
    pub liq_view: &'a wgpu::TextureView,
    pub liq_min: Vec3,
    pub liq_max: Vec3,
    pub liq_gain: f32,
}

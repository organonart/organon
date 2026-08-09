//! Crisp voxel surface mode (the Eulerian render path). Splats the node point-set
//! into a fixed 3D occupancy+colour grid (a compute pass), then DDA-raymarches it
//! as crisp grid-snapped cubes — flat face shading, voxel AO, soft shadows,
//! palette posterization (voxel.wgsl). Owns the 3D field texture, the node storage
//! buffer, and both pipelines.
//!
//! Voxel GI (#89): the field is given a mip pyramid (built each frame by
//! voxgi.wgsl) and, when GI is enabled, the raymarch cone-traces it for bounced
//! colour — emissive voxels bleed onto their neighbours and the world. The mip
//! pyramid averages the SAME albedo+density grid (averaged albedo = region colour,
//! averaged density = coverage), so GI needs no extra data. Default off → the #88
//! look is byte-identical.
//!
//! Included by `render.rs` via `#[path]`, like `metaball`/`post`/`env`, so it
//! compiles only into the visual binary. The raymarch reuses the cube shader's
//! uniform bind group (group 0) for the camera + key/fill lights; group 1 is the
//! splatted field + sampler + raymarch/GI params. Reuses `render::MetaNode` for the
//! node set (the visual builds it once and feeds both the metaball and voxel paths).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::MetaNode;

/// Default field grid resolution (cubic); the editor's "voxel grid" dial drives it
/// at runtime (the projector perf lever). The splat is O(voxels·nodes), so the
/// node set is capped + stride-subsampled like metaball.
pub const DEFAULT_VOX_RES: u32 = 96;
/// Cap on nodes fed to the splat (stride-subsampled past this).
pub const MAX_VOX_NODES: usize = 4096;

/// Live voxel look params (from the editor).
#[derive(Clone, Copy)]
pub struct VoxelParams {
    pub res: u32,        // grid resolution (perf dial)
    pub threshold: f32,  // fill iso level (cells above it are solid)
    pub radius: f32,     // per-node splat radius (strand thickness, world units)
    pub emission: f32,   // emissive gain (hot voxels bloom)
    pub ao: f32,         // ambient-occlusion strength (0..1)
    pub shadow: f32,     // soft-shadow strength (0..1)
    pub quantize: f32,   // palette posterize levels (0 = off)
    pub sharpness: f32,  // splat falloff edge (crisper walls)
    // Voxel GI (#89): cone-traced bounced colour.
    pub gi_on: bool,     // build the mip pyramid + cone-trace it
    pub gi_strength: f32,// bounce intensity
    pub gi_max_dist: f32,// cone march distance (world units)
    pub gi_sky: f32,     // sky/ambient fill in the unoccluded cone remainder
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FieldU {
    bmin: [f32; 4], // xyz = grid min, w unused
    bmax: [f32; 4], // xyz = grid max, w = edge sharpness
    grid: [u32; 4], // xyz = resolution, w = node count
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VoxU {
    inv_vp: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    bmin: [f32; 4],  // xyz = AABB min, w = threshold
    bmax: [f32; 4],  // xyz = AABB max, w = max steps
    grid: [f32; 4],  // xyz = resolution, w unused
    shade: [f32; 4], // x = AO, y = shadow, z = emission, w = quantize levels
    gi: [f32; 4],    // x = GI on, y = strength, z = max distance, w = sky/ambient
}

pub struct VoxField {
    res: u32,
    mips: u32,
    // Compute (splat)
    compute_pipeline: wgpu::ComputePipeline,
    compute_bgl: wgpu::BindGroupLayout,
    field_ub: wgpu::Buffer,
    node_buf: wgpu::Buffer,
    node_cap: usize,
    field_tex: wgpu::Texture,
    field_view: wgpu::TextureView,      // all-mip sampled view (raymarch + cone trace)
    mip_views: Vec<wgpu::TextureView>,  // single-mip views (mip 0 = splat target; downsample)
    compute_bind: wgpu::BindGroup,
    // GI mip downsample (voxgi.wgsl)
    down_pipeline: wgpu::ComputePipeline,
    down_bgl: wgpu::BindGroupLayout,
    down_binds: Vec<wgpu::BindGroup>,   // one per mip level 1..mips (reads L-1, writes L)
    // Raymarch
    ray_shader: wgpu::ShaderModule,
    ray_layout: wgpu::PipelineLayout,
    ray_pipeline: wgpu::RenderPipeline,
    // Depth-only variant (screen-space GI): the SAME DDA march writing only
    // frag_depth into the single-sample screen-space-FX prepass, so SSR/SSGI can
    // reconstruct normals and gather off the voxel surface. Always sample_count 1
    // (the prepass depth is single-sample), so it never needs the MSAA rebuild.
    depth_pipeline: wgpu::RenderPipeline,
    vox_bgl: wgpu::BindGroupLayout,
    vox_ub: wgpu::Buffer,
    sampler: wgpu::Sampler,
    vox_bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    node_count: u32,
}

impl VoxField {
    pub fn new(
        device: &wgpu::Device,
        uniform_layout: &wgpu::BindGroupLayout, // group 0 (cube uniforms)
        ibl_layout: &wgpu::BindGroupLayout,     // group 1 (IBL maps)
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let res = DEFAULT_VOX_RES;
        let mips = mip_count(res);

        let (field_tex, field_view, mip_views) = make_field(device, res);

        let field_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voxel-field-ub"),
            size: std::mem::size_of::<FieldU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vox_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voxel-ray-ub"),
            size: std::mem::size_of::<VoxU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let node_cap = 1024usize;
        let node_buf = make_node_buf(device, node_cap);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("voxel-field-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // --- Compute pipeline (voxelize.wgsl) ---
        let field_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel-splat"),
            source: wgpu::ShaderSource::Wgsl(include_str!("voxelize.wgsl").into()),
        });
        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("voxel-compute-layout"),
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
            ],
        });
        let compute_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxel-compute-pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("voxel-splat"),
            layout: Some(&compute_layout),
            module: &field_shader,
            entry_point: Some("cs_splat"),
            compilation_options: Default::default(),
            cache: None,
        });
        let compute_bind =
            make_compute_bind(device, &compute_bgl, &field_ub, &node_buf, &mip_views[0]);

        // --- GI downsample pipeline (voxgi.wgsl): src mip (sampled) → dst mip (storage) ---
        let down_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel-gi-downsample"),
            source: wgpu::ShaderSource::Wgsl(include_str!("voxgi.wgsl").into()),
        });
        let down_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("voxel-gi-down-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
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
                        view_dimension: wgpu::TextureViewDimension::D3,
                    },
                    count: None,
                },
            ],
        });
        let down_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxel-gi-down-pl"),
            bind_group_layouts: &[Some(&down_bgl)],
            immediate_size: 0,
        });
        let down_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("voxel-gi-downsample"),
            layout: Some(&down_layout),
            module: &down_shader,
            entry_point: Some("cs_downsample"),
            compilation_options: Default::default(),
            cache: None,
        });
        let down_binds = make_down_binds(device, &down_bgl, &mip_views);

        // --- Raymarch pipeline (voxel.wgsl): group2 = field + sampler + params ---
        // (group 0 = cube uniforms, group 1 = IBL maps — for the PBR/material shade.)
        let vox_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("voxel-ray-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
        let vox_bind = make_vox_bind(device, &vox_bgl, &vox_ub, &field_view, &sampler);

        let ray_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxel-ray"),
            source: wgpu::ShaderSource::Wgsl(include_str!("voxel.wgsl").into()),
        });
        let ray_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxel-ray-pl"),
            bind_group_layouts: &[Some(uniform_layout), Some(ibl_layout), Some(&vox_bgl)],
            immediate_size: 0,
        });
        let ray_pipeline =
            make_ray_pipeline(device, &ray_shader, &ray_layout, color_format, depth_format, sample_count);
        // Reuses ray_layout (all 3 groups) so draw_depth can bind the same groups;
        // fs_ray_depth only reads groups 0 & 2, but binding the IBL group is harmless.
        let depth_pipeline = make_depth_pipeline(device, &ray_shader, &ray_layout, depth_format);

        VoxField {
            res,
            mips,
            compute_pipeline,
            compute_bgl,
            field_ub,
            node_buf,
            node_cap,
            field_tex,
            field_view,
            mip_views,
            compute_bind,
            down_pipeline,
            down_bgl,
            down_binds,
            ray_shader,
            ray_layout,
            ray_pipeline,
            depth_pipeline,
            vox_bgl,
            vox_ub,
            sampler,
            vox_bind,
            color_format,
            depth_format,
            sample_count,
            node_count: 0,
        }
    }

    /// Rebuild the raymarch pipeline for a new MSAA sample count (matches the scene
    /// pass, like the cube/metaball pipelines). No-op if unchanged.
    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        let n = n.max(1);
        if n == self.sample_count {
            return;
        }
        self.sample_count = n;
        self.ray_pipeline = make_ray_pipeline(
            device,
            &self.ray_shader,
            &self.ray_layout,
            self.color_format,
            self.depth_format,
            n,
        );
    }

    /// Rebuild the 3D field texture (+ mip pyramid) + its bind groups for a new
    /// grid resolution.
    fn set_res(&mut self, device: &wgpu::Device, res: u32) {
        let res = res.clamp(16, 256);
        if res == self.res {
            return;
        }
        self.res = res;
        self.mips = mip_count(res);
        let (tex, view, mips) = make_field(device, res);
        self.field_tex = tex;
        self.field_view = view;
        self.mip_views = mips;
        self.compute_bind = make_compute_bind(
            device,
            &self.compute_bgl,
            &self.field_ub,
            &self.node_buf,
            &self.mip_views[0],
        );
        self.down_binds = make_down_binds(device, &self.down_bgl, &self.mip_views);
        self.vox_bind =
            make_vox_bind(device, &self.vox_bgl, &self.vox_ub, &self.field_view, &self.sampler);
    }

    /// Upload nodes + params and run the splat compute pass into the field texture
    /// (then the GI mip downsample when GI is on). Call once per frame BEFORE the
    /// scene render pass that raymarches it. `nodes` may exceed `MAX_VOX_NODES` — it
    /// is stride-subsampled to fit.
    #[allow(clippy::too_many_arguments)]
    pub fn splat(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        nodes: &[MetaNode],
        bounds_min: Vec3,
        bounds_max: Vec3,
        inv_view_proj: Mat4,
        p: &VoxelParams,
    ) {
        self.set_res(device, p.res);

        // Subsample to the cap (stride keeps coverage across the whole field).
        let mut packed: Vec<MetaNode>;
        let used: &[MetaNode] = if nodes.len() > MAX_VOX_NODES {
            let stride = nodes.len().div_ceil(MAX_VOX_NODES);
            packed = nodes.iter().step_by(stride).copied().collect();
            packed.truncate(MAX_VOX_NODES);
            &packed
        } else {
            nodes
        };
        self.node_count = used.len() as u32;
        if used.is_empty() {
            return;
        }

        if used.len() > self.node_cap {
            self.node_cap = used.len().next_power_of_two();
            self.node_buf = make_node_buf(device, self.node_cap);
            self.compute_bind = make_compute_bind(
                device,
                &self.compute_bgl,
                &self.field_ub,
                &self.node_buf,
                &self.mip_views[0],
            );
        }
        queue.write_buffer(&self.node_buf, 0, bytemuck::cast_slice(used));

        // Pad the AABB by the splat radius so edge cells aren't clipped, and keep a
        // non-zero extent.
        let pad = Vec3::splat(p.radius * 1.5 + 1e-3);
        let bmin = bounds_min - pad;
        let bmax = bounds_max + pad;

        queue.write_buffer(
            &self.field_ub,
            0,
            bytemuck::bytes_of(&FieldU {
                bmin: [bmin.x, bmin.y, bmin.z, 0.0],
                bmax: [bmax.x, bmax.y, bmax.z, p.sharpness.max(0.0)],
                grid: [self.res, self.res, self.res, self.node_count],
            }),
        );
        queue.write_buffer(
            &self.vox_ub,
            0,
            bytemuck::bytes_of(&VoxU {
                inv_vp: inv_view_proj.to_cols_array_2d(),
                view_proj: inv_view_proj.inverse().to_cols_array_2d(),
                bmin: [bmin.x, bmin.y, bmin.z, p.threshold.max(1e-4)],
                // DDA worst case: a ray crosses at most resₓ+res_y+res_z cell
                // boundaries before exiting the grid. Size the step budget to the
                // actual resolution (was a fixed 512) so smaller grids — the perf
                // dial — march proportionally fewer steps on through-the-gaps rays.
                bmax: [bmax.x, bmax.y, bmax.z, (3 * self.res + 4) as f32],
                grid: [self.res as f32, self.res as f32, self.res as f32, 0.0],
                shade: [p.ao, p.shadow, p.emission, p.quantize.max(0.0)],
                gi: [
                    if p.gi_on { 1.0 } else { 0.0 },
                    p.gi_strength,
                    p.gi_max_dist,
                    p.gi_sky,
                ],
            }),
        );

        // Splat the nodes into mip 0.
        let wg = self.res.div_ceil(4);
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("voxel-splat-pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.compute_pipeline);
            cpass.set_bind_group(0, &self.compute_bind, &[]);
            cpass.dispatch_workgroups(wg, wg, wg);
        }

        // GI: build the mip pyramid (one pass per level, each reads the level above).
        if p.gi_on {
            for (i, bind) in self.down_binds.iter().enumerate() {
                let level = (i + 1) as u32; // down_binds[0] writes mip 1
                let lr = (self.res >> level).max(1);
                let wg = lr.div_ceil(4);
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("voxel-gi-downsample-pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(&self.down_pipeline);
                cpass.set_bind_group(0, bind, &[]);
                cpass.dispatch_workgroups(wg, wg, wg);
            }
        }
    }

    /// Draw the raymarched voxels into the active scene render pass (after the
    /// background). `uniform_bind` = group 0 (camera + lights); `ibl_bind` = group 1
    /// (IBL maps, for the PBR/material shade). No-op if the last `splat` had no nodes.
    pub fn draw<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        uniform_bind: &'a wgpu::BindGroup,
        ibl_bind: &'a wgpu::BindGroup,
    ) {
        if self.node_count == 0 {
            return;
        }
        rp.set_pipeline(&self.ray_pipeline);
        rp.set_bind_group(0, uniform_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_bind_group(2, &self.vox_bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Depth-only draw into the screen-space-FX depth prepass (single-sample, no
    /// colour target): the same DDA march writing just `frag_depth`, so SSR/SSGI
    /// reconstruct normals and gather off the voxel surface. Binds the same 3
    /// groups as `draw` (the depth pipeline shares `ray_layout`; only 0 & 2 are
    /// read). Call inside the `depth-prepass` render pass. No-op if no nodes.
    pub fn draw_depth<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        uniform_bind: &'a wgpu::BindGroup,
        ibl_bind: &'a wgpu::BindGroup,
    ) {
        if self.node_count == 0 {
            return;
        }
        rp.set_pipeline(&self.depth_pipeline);
        rp.set_bind_group(0, uniform_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_bind_group(2, &self.vox_bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

/// Depth-only pipeline for the screen-space-FX prepass: `fs_ray_depth` marches the
/// grid and writes `frag_depth` with no colour attachment. Always single-sample
/// (the prepass depth is single-sample) with `Less` depth write.
fn make_depth_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    depth_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("voxel-depth-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_ray"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_ray_depth"),
            targets: &[],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
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

/// Mip levels for a cubic field of side `res`: floor(log2(res)) + 1.
fn mip_count(res: u32) -> u32 {
    32 - res.max(1).leading_zeros()
}

fn make_field(
    device: &wgpu::Device,
    res: u32,
) -> (wgpu::Texture, wgpu::TextureView, Vec<wgpu::TextureView>) {
    let mips = mip_count(res);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("voxel-field"),
        size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: res },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    // All-mip sampled view (raymarch textureLoad on mip 0 + cone-trace LOD sampling).
    let all = tex.create_view(&wgpu::TextureViewDescriptor::default());
    // Single-mip views (mip 0 = splat storage target; the rest = downsample dst/src).
    let mip_views = (0..mips)
        .map(|l| {
            tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some("voxel-field-mip"),
                base_mip_level: l,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    (tex, all, mip_views)
}

fn make_node_buf(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("voxel-nodes"),
        size: (cap * std::mem::size_of::<MetaNode>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn make_compute_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    field_ub: &wgpu::Buffer,
    node_buf: &wgpu::Buffer,
    mip0_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("voxel-compute-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: field_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: node_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(mip0_view) },
        ],
    })
}

/// One downsample bind group per mip level 1..mips: reads level L-1, writes level L.
fn make_down_binds(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    mip_views: &[wgpu::TextureView],
) -> Vec<wgpu::BindGroup> {
    (1..mip_views.len())
        .map(|l| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("voxel-gi-down-bind"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&mip_views[l - 1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&mip_views[l]),
                    },
                ],
            })
        })
        .collect()
}

fn make_vox_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    vox_ub: &wgpu::Buffer,
    field_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("voxel-ray-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: vox_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(field_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn make_ray_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("voxel-ray-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_ray"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_ray"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

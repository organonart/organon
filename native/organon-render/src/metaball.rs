//! Metaball isosurface mode: bake the node point-set into a 3D scalar+colour
//! field (a compute pass), then raymarch it as one smooth contiguous skin (a
//! fullscreen pass that writes into the scene HDR buffer + depth). Owns the field
//! texture, the node storage buffer, and both pipelines.
//!
//! Included by `render.rs` via `#[path]`, like `post`/`env`, so it compiles only
//! into the visual binary. The raymarch reuses the cube shader's uniform bind
//! group (group 0) + the IBL bind group (group 1) so its Standard PBR shading
//! matches the other surface modes; group 2 is the baked field + raymarch params.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Field grid resolution (cubic). 64³ = 262k voxels — a good organic/blobby
/// look at a tractable per-frame voxelization cost; expose later if needed.
pub const FIELD_RES: u32 = 64;
/// Cap on nodes fed to the field. The voxelizer is O(voxels · nodes), so very
/// large fields (the uncapped cube-of-cubes) are stride-subsampled to this many
/// to keep the compute pass cheap. (A spatial grid would lift this later.)
pub const MAX_META_NODES: usize = 4096;
/// Fixed raymarch step budget across the field AABB.
const MAX_STEPS: f32 = 128.0;

/// One metaball: a world position + influence radius (`pos.w`) and a colour.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MetaNode {
    pub pos: [f32; 4],   // xyz = world position, w = influence radius
    pub color: [f32; 4], // rgb = colour, w unused
}

/// Live metaball look params (from the editor). The `vol_*` + `steps` fields are
/// used only by the emissive Volume mode (#152); the isosurface path leaves them
/// at 0 and ignores them.
#[derive(Clone, Copy)]
pub struct MetaballParams {
    pub radius: f32,     // blob influence radius (world units)
    pub threshold: f32,  // iso level the surface sits at
    pub smoothness: f32, // falloff-edge sharpness (0 = soft/round, up = tighter)
    pub vol_density: f32,    // Volume mode: density multiplier
    pub vol_emission: f32,   // Volume mode: emissive glow strength
    pub vol_absorption: f32, // Volume mode: Beer–Lambert extinction
    pub steps: f32,          // Volume mode: raymarch steps (0 = the metaball default)
    /// Field Volume (#348/#349 Tier 3): 1 = the uploaded field grid already carries
    /// baked per-CELL colours (the Duo-Field's pressure/velocity channels — rgb tells
    /// which channel dominates), so the volume raymarch shows that colour directly
    /// instead of re-applying the single calibrated tint. 0 = uncoloured field (white /
    /// node albedo). Rides `MetaU.vol.w`.
    pub band_coloured: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FieldU {
    bmin: [f32; 4], // xyz = grid min, w unused
    bmax: [f32; 4], // xyz = grid max, w = sharpness
    grid: [u32; 4], // xyz = resolution, w = node count
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MetaU {
    inv_vp: [[f32; 4]; 4],
    /// Forward view-projection matching `inv_vp` (i.e. UNSCALED — no breath). The
    /// raymarch reconstructs hit points in unscaled world space via `inv_vp`, so it
    /// must write `frag_depth` through this same matrix; using the scene's
    /// breath-scaled `view_proj` would desync the surface's depth.
    view_proj: [[f32; 4]; 4],
    bmin: [f32; 4], // xyz = AABB min, w = threshold
    bmax: [f32; 4], // xyz = AABB max, w = max steps
    grid: [f32; 4], // xyz = resolution, w unused
    vol: [f32; 4],  // emissive Volume (#152): x=density, y=emission, z=absorption, w unused
}

pub struct MetaField {
    res: u32,
    // Compute (voxelize)
    compute_pipeline: wgpu::ComputePipeline,
    compute_bgl: wgpu::BindGroupLayout,
    field_ub: wgpu::Buffer,
    node_buf: wgpu::Buffer,
    node_cap: usize,
    field_tex: wgpu::Texture, // 3D field: storage (compute) + sampled (raymarch) + CPU upload (#348)
    field_view: wgpu::TextureView, // 3D: storage (compute) + sampled (raymarch)
    compute_bind: wgpu::BindGroup,
    // Raymarch
    ray_shader: wgpu::ShaderModule,
    ray_layout: wgpu::PipelineLayout,
    ray_pipeline: wgpu::RenderPipeline,
    // Emissive Volume (#152): same field + bind groups, a sibling pipeline whose
    // fragment integrates the field as a glowing medium (alpha-over, no depth write).
    vol_pipeline: wgpu::RenderPipeline,
    meta_ub: wgpu::Buffer,
    meta_bind: wgpu::BindGroup,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    node_count: u32,
}

impl MetaField {
    pub fn new(
        device: &wgpu::Device,
        uniform_layout: &wgpu::BindGroupLayout, // group 0 (cube uniforms)
        ibl_layout: &wgpu::BindGroupLayout,     // group 1 (IBL maps)
        rd_layout: &wgpu::BindGroupLayout,      // group 3 (reaction–diffusion field)
        gi_layout: &wgpu::BindGroupLayout,      // group 4 (#182 T4: probe GI)
        vx_layout: &wgpu::BindGroupLayout,      // group 5 (#182 T4: VXGI volume)
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let res = FIELD_RES;

        // --- 3D field texture (storage write + sampled read) ---
        let field_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("metaball-field"),
            size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: res },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            // COPY_DST so the Field Volume path (#348) can upload a CPU-baked
            // analytic energy grid straight into the field (no compute voxelize).
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let field_view = field_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("metaball-field-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let field_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metaball-field-ub"),
            size: std::mem::size_of::<FieldU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let meta_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("metaball-ray-ub"),
            size: std::mem::size_of::<MetaU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let node_cap = 1024usize;
        let node_buf = make_node_buf(device, node_cap);

        // --- Compute pipeline (field.wgsl) ---
        let field_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("metaball-field"),
            source: wgpu::ShaderSource::Wgsl(include_str!("field.wgsl").into()),
        });
        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("metaball-compute-layout"),
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
            label: Some("metaball-compute-pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("metaball-voxelize"),
            layout: Some(&compute_layout),
            module: &field_shader,
            entry_point: Some("cs_voxelize"),
            compilation_options: Default::default(),
            cache: None,
        });
        let compute_bind =
            make_compute_bind(device, &compute_bgl, &field_ub, &node_buf, &field_view);

        // --- Raymarch pipeline (metaball.wgsl): group2 = field + params ---
        let meta_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("metaball-ray-layout"),
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
        let meta_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("metaball-ray-bind"),
            layout: &meta_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: meta_ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&field_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let ray_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("metaball-ray"),
            source: wgpu::ShaderSource::Wgsl(include_str!("metaball.wgsl").into()),
        });
        let ray_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("metaball-ray-pl"),
            bind_group_layouts: &[
                Some(uniform_layout),
                Some(ibl_layout),
                Some(&meta_bgl),
                Some(rd_layout),
                Some(gi_layout),
                Some(vx_layout),
            ],
            immediate_size: 0,
        });
        let ray_pipeline =
            make_ray_pipeline(device, &ray_shader, &ray_layout, color_format, depth_format, sample_count);
        let vol_pipeline =
            make_vol_pipeline(device, &ray_shader, &ray_layout, color_format, depth_format, sample_count);

        MetaField {
            res,
            compute_pipeline,
            compute_bgl,
            field_ub,
            node_buf,
            node_cap,
            field_tex,
            field_view,
            compute_bind,
            ray_shader,
            ray_layout,
            ray_pipeline,
            vol_pipeline,
            meta_ub,
            meta_bind,
            color_format,
            depth_format,
            sample_count,
            node_count: 0,
        }
    }

    /// The 3D field texture view — the liquid (#182 Tier 3) fills it from a
    /// compute splat instead of the node voxelizer.
    pub fn field_view(&self) -> &wgpu::TextureView {
        &self.field_view
    }

    /// Prepare the raymarch uniforms for an EXTERNALLY-filled field (#182
    /// Tier 3: the MLS-MPM density splat writes the texture directly, so no
    /// node upload / voxelize dispatch happens). `bmin/bmax` is the field's
    /// world AABB (the liquid container — unpadded, unlike `voxelize`), and
    /// `draw`/`draw_volume` are armed by faking a non-zero node count.
    pub fn prepare_direct(
        &mut self,
        queue: &wgpu::Queue,
        bmin: Vec3,
        bmax: Vec3,
        inv_view_proj: Mat4,
        p: &MetaballParams,
    ) {
        self.node_count = 1;
        queue.write_buffer(
            &self.field_ub,
            0,
            bytemuck::bytes_of(&FieldU {
                bmin: [bmin.x, bmin.y, bmin.z, 0.0],
                bmax: [bmax.x, bmax.y, bmax.z, p.smoothness.max(0.0)],
                grid: [self.res, self.res, self.res, 1],
            }),
        );
        queue.write_buffer(
            &self.meta_ub,
            0,
            bytemuck::bytes_of(&MetaU {
                inv_vp: inv_view_proj.to_cols_array_2d(),
                view_proj: inv_view_proj.inverse().to_cols_array_2d(),
                bmin: [bmin.x, bmin.y, bmin.z, p.threshold.max(1e-4)],
                bmax: [bmax.x, bmax.y, bmax.z, if p.steps > 0.5 { p.steps } else { MAX_STEPS }],
                grid: [self.res as f32, self.res as f32, self.res as f32, 0.0],
                vol: [p.vol_density, p.vol_emission, p.vol_absorption, p.band_coloured],
            }),
        );
    }

    /// Field Volume (#348) — upload a CPU-baked `res³` RGBA field straight into the
    /// 3D texture (no node upload / voxelize dispatch), then `prepare_direct` arms
    /// the raymarch. Used by the analytic field-energy bake for Maxwell/Acoustic so
    /// the density cloud follows the *field* energy instead of the node metaball
    /// skin. `grid` is laid out `(z·res + y)·res + x` (matching `field.wgsl` /
    /// `math::bake_field_energy`); rgb = colour, a = density. Silently ignores a
    /// short/empty grid (the caller falls back to the node path).
    pub fn upload_field(&self, queue: &wgpu::Queue, grid: &[glam::Vec4]) {
        let res = self.res;
        let expect = (res * res * res) as usize;
        if grid.len() < expect {
            return;
        }
        // Rgba16Float: convert the f32 grid to packed f16 texels.
        let mut half: Vec<half::f16> = Vec::with_capacity(expect * 4);
        for v in &grid[..expect] {
            half.push(half::f16::from_f32(v.x));
            half.push(half::f16::from_f32(v.y));
            half.push(half::f16::from_f32(v.z));
            half.push(half::f16::from_f32(v.w));
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.field_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&half),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(res * 8), // 4ch * 2 bytes
                rows_per_image: Some(res),
            },
            wgpu::Extent3d { width: res, height: res, depth_or_array_layers: res },
        );
    }

    /// Rebuild the raymarch pipeline for a new MSAA sample count (matches the
    /// scene pass, like the cube/sky pipelines). No-op if unchanged.
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
        self.vol_pipeline = make_vol_pipeline(
            device,
            &self.ray_shader,
            &self.ray_layout,
            self.color_format,
            self.depth_format,
            n,
        );
    }

    /// Upload nodes + params and run the voxelize compute pass into the field
    /// texture. Call once per frame BEFORE the scene render pass that samples it.
    /// `nodes` may exceed `MAX_META_NODES` — it is stride-subsampled to fit.
    pub fn voxelize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        nodes: &[MetaNode],
        bounds_min: Vec3,
        bounds_max: Vec3,
        inv_view_proj: Mat4,
        p: &MetaballParams,
    ) {
        // Subsample to the cap (stride keeps coverage across the whole field
        // rather than truncating to one corner).
        let mut packed: Vec<MetaNode>;
        let used: &[MetaNode] = if nodes.len() > MAX_META_NODES {
            let stride = nodes.len().div_ceil(MAX_META_NODES);
            packed = nodes.iter().step_by(stride).copied().collect();
            packed.truncate(MAX_META_NODES);
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
                &self.field_view,
            );
        }
        queue.write_buffer(&self.node_buf, 0, bytemuck::cast_slice(used));

        // Pad the field AABB by the influence radius so edge blobs aren't clipped,
        // and guarantee a non-zero extent (a single node would give min == max).
        let pad = Vec3::splat(p.radius * 1.5 + 1e-3);
        let bmin = bounds_min - pad;
        let bmax = bounds_max + pad;

        queue.write_buffer(
            &self.field_ub,
            0,
            bytemuck::bytes_of(&FieldU {
                bmin: [bmin.x, bmin.y, bmin.z, 0.0],
                bmax: [bmax.x, bmax.y, bmax.z, p.smoothness.max(0.0)],
                grid: [self.res, self.res, self.res, self.node_count],
            }),
        );
        queue.write_buffer(
            &self.meta_ub,
            0,
            bytemuck::bytes_of(&MetaU {
                inv_vp: inv_view_proj.to_cols_array_2d(),
                // Recover the unscaled forward matrix from the (unscaled) inverse we
                // were handed, so depth is projected in the same space the rays are.
                view_proj: inv_view_proj.inverse().to_cols_array_2d(),
                bmin: [bmin.x, bmin.y, bmin.z, p.threshold.max(1e-4)],
                // Volume mode overrides the step budget; metaball keeps MAX_STEPS.
                bmax: [bmax.x, bmax.y, bmax.z, if p.steps > 0.5 { p.steps } else { MAX_STEPS }],
                grid: [self.res as f32, self.res as f32, self.res as f32, 0.0],
                vol: [p.vol_density, p.vol_emission, p.vol_absorption, p.band_coloured],
            }),
        );

        let wg = self.res.div_ceil(4);
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("metaball-voxelize-pass"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&self.compute_pipeline);
        cpass.set_bind_group(0, &self.compute_bind, &[]);
        cpass.dispatch_workgroups(wg, wg, wg);
    }

    /// Draw the raymarched isosurface into the active scene render pass (after the
    /// skybox). `uniform_bind` = group 0; `ibl_bind` = group 1; `rd_bind` = group 3
    /// (reaction–diffusion field). No-op if the last `voxelize` had no nodes.
    pub fn draw<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        uniform_bind: &'a wgpu::BindGroup,
        ibl_bind: &'a wgpu::BindGroup,
        rd_bind: &'a wgpu::BindGroup,
        gi_bind: &'a wgpu::BindGroup,
        vx_bind: &'a wgpu::BindGroup,
    ) {
        if self.node_count == 0 {
            return;
        }
        rp.set_pipeline(&self.ray_pipeline);
        rp.set_bind_group(0, uniform_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_bind_group(2, &self.meta_bind, &[]);
        rp.set_bind_group(3, rd_bind, &[]);
        rp.set_bind_group(4, gi_bind, &[]);
        rp.set_bind_group(5, vx_bind, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Draw the field as an emissive volume (#152) instead of an isosurface —
    /// same bind groups, the `fs_volume` pipeline (alpha-over, no depth write).
    /// No-op if the last `voxelize` had no nodes.
    pub fn draw_volume<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        uniform_bind: &'a wgpu::BindGroup,
        ibl_bind: &'a wgpu::BindGroup,
        rd_bind: &'a wgpu::BindGroup,
        gi_bind: &'a wgpu::BindGroup,
        vx_bind: &'a wgpu::BindGroup,
    ) {
        if self.node_count == 0 {
            return;
        }
        rp.set_pipeline(&self.vol_pipeline);
        rp.set_bind_group(0, uniform_bind, &[]);
        rp.set_bind_group(1, ibl_bind, &[]);
        rp.set_bind_group(2, &self.meta_bind, &[]);
        rp.set_bind_group(3, rd_bind, &[]);
        rp.set_bind_group(4, gi_bind, &[]);
        rp.set_bind_group(5, vx_bind, &[]);
        rp.draw(0..3, 0..1);
    }
}

fn make_node_buf(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("metaball-nodes"),
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
    field_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("metaball-compute-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: field_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: node_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(field_view) },
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
        label: Some("metaball-ray-pipeline"),
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

/// Emissive-volume pipeline (#152): the `fs_volume` fragment alpha-blends a glowing
/// medium over the already-drawn background. Depth test Always (the fog is the whole
/// geometry for this mode, so it never clips against the far-cleared depth) but it
/// **writes** the cloud's front depth (`fs_volume.out.depth` = the first occupied
/// sample, or far on a miss), so later passes in the same scene pass — Particle Aura,
/// the capture axes — are occluded by the cloud instead of always drawing in front.
fn make_vol_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    // Premultiplied-alpha over: out = src.rgb + dst*(1-src.a); alpha accumulates.
    // `fs_volume` already returns transmittance-attenuated radiance in `rgb` (the
    // emission is integrated along the ray, not a flat colour to be scaled by α), so
    // the source factor must be One — using SrcAlpha would scale the HDR glow down by
    // α and dim the cloud.
    let blend = wgpu::BlendState {
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
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("metaball-vol-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_ray"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_volume"),
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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

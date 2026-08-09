//! Fluid Ink (#182 Tier 1) — the render half of "the medium IS the image".
//!
//! The fluid solver (`fluid.rs`) evolves an RGB dye field the generator's nodes
//! inject; this module makes it visible: a compute blit copies the dye storage
//! buffer into a trilinear 3D texture, a fullscreen pass raymarches it
//! (Beer–Lambert + Henyey–Greenstein key scatter with a self-shadow light-march
//! + IBL-irradiance ambient + emissive dye) into an offscreen ink target at
//! full or half resolution, and a depth-aware upsample composites it over the
//! resolved HDR scene buffer pre-bloom — so the ink blooms, tone-maps and EDRs
//! with everything else. The march is clamped by the scene depth prepass, so
//! the ink swirls *around* visible geometry; with `hide_generator` it's the
//! pure medium.
//!
//! Included by `render.rs` via `#[path]` (visual binary only), like `vxgi.rs`.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// The scene HDR buffer format (matches `post::HDR_FORMAT`).
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Live ink look params (from `Shared.fluidvis` + `Shared.fluid2`).
#[derive(Clone, Copy)]
pub struct InkParams {
    pub extinction: f32,
    pub scatter: f32,
    pub emissive: f32,
    pub anisotropy: f32,
    pub steps: f32,
    /// March at half resolution + bilateral upsample (the perf dial).
    pub half_res: bool,
    /// #182 T2: curl-noise micro-detail amount (0 = off), scaled by local |ω|.
    pub detail: f32,
    /// Soft density threshold the march culls below (chips away the dilute
    /// haze crust — the vector-field "reveal"). 0 = show everything.
    pub reveal: f32,
}

/// Matches `InkU` in fluidvis.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InkU {
    inv_vp: [[f32; 4]; 4],
    cam: [f32; 4],
    bmin: [f32; 4],
    bmax: [f32; 4],
    key: [f32; 4],
    scat: [f32; 4],
    envt: [f32; 4],
    texel: [f32; 4],
    grid: [f32; 4],
    sizes: [f32; 4],
    // #182 T4 "geometry shades the smoke": the scene shadow map's light matrix
    // (sizes.w carries the receive strength, 0 = off).
    light_vp: [[f32; 4]; 4],
    // #182 T4 "the GI lights the smoke": the VXGI volume AABB + gain.
    vgi0: [f32; 4],
    vgi1: [f32; 4],
}

pub struct FluidVis {
    ubo: wgpu::Buffer,
    lin_samp: wgpu::Sampler,
    // 1×1 depth stand-in bound when no prepass ran this frame (the shader's
    // `use_depth` flag is 0, so it is never read).
    dummy_depth: wgpu::TextureView,
    // Dye 3D texture (recreated on a fluid-grid resolution change).
    dye_view: wgpu::TextureView,
    dye_res: u32,
    tex_gen: u64,
    // blit: dye storage buffer → 3D texture.
    blit_pipeline: wgpu::ComputePipeline,
    blit_bgl: wgpu::BindGroupLayout,
    blit_bind: Option<(u64, wgpu::BindGroup)>, // keyed by fluid epoch ^ tex_gen
    // march: fullscreen raymarch into the ink target.
    march_pipeline: wgpu::RenderPipeline,
    march_bgl: wgpu::BindGroupLayout,
    march_bind: Option<(u64, wgpu::BindGroup)>, // keyed by depth_key ^ tex_gen
    // Ink target (full- or half-res of the scaled render size).
    ink_view: wgpu::TextureView,
    ink_size: (u32, u32),
    ink_gen: u64,
    // upsample: depth-aware composite onto the HDR buffer.
    up_pipeline: wgpu::RenderPipeline,
    up_bgl: wgpu::BindGroupLayout,
    up_bind: Option<(u64, wgpu::BindGroup)>, // keyed by depth_key ^ ink_gen
}

impl FluidVis {
    pub fn new(device: &wgpu::Device, ibl_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fluidvis"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fluidvis.wgsl").into()),
        });
        let ubo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluidvis-ubo"),
            size: std::mem::size_of::<InkU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lin_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fluidvis-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let dummy_depth = make_dummy_depth(device);
        let dye_view = make_dye_tex(device, 8);

        let ubo_entry = |b: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let samp_entry = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let depth_entry = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };

        // --- blit (compute): {0 ubo, 5 dye buffer ro, 6 storage 3D tex,
        //     7 curl buffer ro (#182 T2 — |ω| folded into the texture alpha)} ---
        let ro_buf = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluidvis-blit-layout"),
            entries: &[
                ubo_entry(0, wgpu::ShaderStages::COMPUTE),
                ro_buf(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D3,
                    },
                    count: None,
                },
                ro_buf(7),
            ],
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluidvis-blit-pl"),
            bind_group_layouts: &[Some(&blit_bgl)],
            immediate_size: 0,
        });
        let blit_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fluidvis-blit"),
            layout: Some(&blit_pl),
            module: &shader,
            entry_point: Some("cs_dye_blit"),
            compilation_options: Default::default(),
            cache: None,
        });

        // --- march: {0 ubo, 1 dye 3D tex, 2 sampler, 3 depth} + group(1) IBL ---
        let march_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluidvis-march-layout"),
            entries: &[
                ubo_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
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
                samp_entry(2),
                depth_entry(3),
                // #182 T4: the scene shadow map + comparison sampler (geometry
                // shades the smoke; sizes.w = 0 → never sampled).
                depth_entry(8),
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // #182 T4: probe-GI uniform + the VXGI radiance volume (the GI
                // toggles light the smoke; both self-gate to 0 when off).
                ubo_entry(10, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 11,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                samp_entry(12),
            ],
        });
        let march_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluidvis-march-pl"),
            bind_group_layouts: &[Some(&march_bgl), Some(ibl_layout)],
            immediate_size: 0,
        });
        let march_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fluidvis-march-pipeline"),
            layout: Some(&march_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ink"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // --- upsample: {0 ubo, 2 sampler, 3 depth, 4 ink 2D tex} → HDR buffer ---
        let up_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluidvis-up-layout"),
            entries: &[
                ubo_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                samp_entry(2),
                depth_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let up_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluidvis-up-pl"),
            bind_group_layouts: &[Some(&up_bgl)],
            immediate_size: 0,
        });
        // Premultiplied over: dst.rgb = src.rgb + dst.rgb·(1−src.a); RGB-only
        // write so the scene's coverage alpha is untouched (like VXGI).
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let up_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fluidvis-up-pipeline"),
            layout: Some(&up_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_upsample"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(over),
                    write_mask: wgpu::ColorWrites::COLOR,
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

        let ink_view = make_ink_target(device, (8, 8));
        FluidVis {
            ubo,
            lin_samp,
            dummy_depth,
            dye_view,
            dye_res: 8,
            tex_gen: 0,
            blit_pipeline,
            blit_bgl,
            blit_bind: None,
            march_pipeline,
            march_bgl,
            march_bind: None,
            ink_view,
            ink_size: (8, 8),
            ink_gen: 0,
            up_pipeline,
            up_bgl,
            up_bind: None,
        }
    }

    /// Blit the dye buffer into the 3D texture, march it into the ink target,
    /// and composite over `target` (the resolved HDR buffer, pre-bloom).
    /// `depth` = the single-sample prepass depth + its cache key when it ran
    /// this frame (the march then stops at scene geometry); `None` marches the
    /// whole volume (raymarch modes / hidden generator).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: Option<(&wgpu::TextureView, u64)>,
        size: (u32, u32),
        inv_vp: Mat4,
        cam: Vec3,
        gmin: Vec3,
        gmax: Vec3,
        dye_buf: &wgpu::Buffer,
        curl_buf: &wgpu::Buffer,
        dye_res: u32,
        fluid_epoch: u64,
        key_dir: Vec3,
        key_intensity: f32,
        ambient: f32,
        env_rot: f32,
        env_tint: Vec3,
        time: f32,
        ibl_bind: &wgpu::BindGroup,
        p: &InkParams,
        // #182 T4 "geometry shades the smoke": light matrix + shadow map +
        // comparison sampler + strength (0 = off).
        shadow: (Mat4, &wgpu::TextureView, &wgpu::Sampler, f32),
        // #182 T4 "the GI lights the smoke": the probe uniform buffer, the
        // VXGI volume + sampler, its world AABB, and the in-scatter gain
        // (0 = volume not voxelized this frame → never sampled).
        gi: (&wgpu::Buffer, &wgpu::TextureView, &wgpu::Sampler, Vec3, Vec3, f32),
    ) {
        let dye_res = dye_res.max(2);
        if dye_res != self.dye_res {
            self.dye_view = make_dye_tex(device, dye_res);
            self.dye_res = dye_res;
            self.tex_gen = self.tex_gen.wrapping_add(1);
        }
        let scale = if p.half_res { 2u32 } else { 1u32 };
        let ink_size = (size.0.div_ceil(scale).max(1), size.1.div_ceil(scale).max(1));
        if ink_size != self.ink_size {
            self.ink_view = make_ink_target(device, ink_size);
            self.ink_size = ink_size;
            self.ink_gen = self.ink_gen.wrapping_add(1);
        }

        let (depth_view, depth_key) = match depth {
            Some((v, k)) => (v, k),
            None => (&self.dummy_depth, u64::MAX),
        };
        let use_depth = if depth.is_some() { 1.0 } else { 0.0 };

        queue.write_buffer(
            &self.ubo,
            0,
            bytemuck::bytes_of(&InkU {
                inv_vp: inv_vp.to_cols_array_2d(),
                cam: [cam.x, cam.y, cam.z, use_depth],
                bmin: [gmin.x, gmin.y, gmin.z, p.extinction.max(0.0)],
                bmax: [gmax.x, gmax.y, gmax.z, p.steps.clamp(8.0, 256.0)],
                key: [key_dir.x, key_dir.y, key_dir.z, key_intensity.max(0.0)],
                scat: [
                    p.scatter.max(0.0),
                    p.emissive.max(0.0),
                    p.anisotropy.clamp(-0.95, 0.95),
                    ambient.max(0.0),
                ],
                envt: [env_rot, env_tint.x, env_tint.y, env_tint.z],
                texel: [1.0 / size.0.max(1) as f32, 1.0 / size.1.max(1) as f32, scale as f32, time],
                grid: [dye_res as f32, dye_res as f32, dye_res as f32, p.detail.clamp(0.0, 1.0)],
                sizes: [ink_size.0 as f32, ink_size.1 as f32, p.reveal.max(0.0), shadow.3.max(0.0)],
                light_vp: shadow.0.to_cols_array_2d(),
                vgi0: [gi.3.x, gi.3.y, gi.3.z, gi.5.max(0.0)],
                vgi1: [gi.4.x, gi.4.y, gi.4.z, 0.0],
            }),
        );

        // 1) Dye buffer → 3D texture (the fluid buffers are recreated on a
        //    resolution change, so key the bind on the fluid epoch + our texture).
        let blit_key = fluid_epoch.wrapping_mul(31).wrapping_add(self.tex_gen);
        if self.blit_bind.as_ref().map(|(k, _)| *k != blit_key).unwrap_or(true) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fluidvis-blit-bind"),
                layout: &self.blit_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: dye_buf.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&self.dye_view),
                    },
                    wgpu::BindGroupEntry { binding: 7, resource: curl_buf.as_entire_binding() },
                ],
            });
            self.blit_bind = Some((blit_key, bind));
        }
        {
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fluidvis-blit-pass"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&self.blit_pipeline);
            cp.set_bind_group(0, &self.blit_bind.as_ref().unwrap().1, &[]);
            let wg = dye_res.div_ceil(4);
            cp.dispatch_workgroups(wg, wg, wg);
        }

        // 2) The ink march (into the offscreen target).
        let march_key = depth_key.wrapping_mul(31).wrapping_add(self.tex_gen);
        if self.march_bind.as_ref().map(|(k, _)| *k != march_key).unwrap_or(true) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fluidvis-march-bind"),
                layout: &self.march_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.dye_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.lin_samp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(shadow.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::Sampler(shadow.2),
                    },
                    wgpu::BindGroupEntry { binding: 10, resource: gi.0.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 11,
                        resource: wgpu::BindingResource::TextureView(gi.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 12,
                        resource: wgpu::BindingResource::Sampler(gi.2),
                    },
                ],
            });
            self.march_bind = Some((march_key, bind));
        }
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluidvis-march-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ink_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.march_pipeline);
            rp.set_bind_group(0, &self.march_bind.as_ref().unwrap().1, &[]);
            rp.set_bind_group(1, ibl_bind, &[]);
            rp.draw(0..3, 0..1);
        }

        // 3) Depth-aware upsample/composite over the HDR buffer.
        let up_key = depth_key.wrapping_mul(31).wrapping_add(self.ink_gen);
        if self.up_bind.as_ref().map(|(k, _)| *k != up_key).unwrap_or(true) {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fluidvis-up-bind"),
                layout: &self.up_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.ubo.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.lin_samp),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&self.ink_view),
                    },
                ],
            });
            self.up_bind = Some((up_key, bind));
        }
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluidvis-up-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            rp.set_pipeline(&self.up_pipeline);
            rp.set_bind_group(0, &self.up_bind.as_ref().unwrap().1, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

impl FluidVis {
    /// The dye 3D texture (rgb = colour × amount, a = |ω|) — #182 T4 sources
    /// (VXGI injection + the light-space transmittance march) sample it.
    pub fn dye_view(&self) -> &wgpu::TextureView {
        &self.dye_view
    }
    /// Bumped when the dye texture is recreated (fluid-res change) — bind
    /// groups holding `dye_view` key on this.
    pub fn dye_gen(&self) -> u64 {
        self.tex_gen
    }
}

fn make_dye_tex(device: &wgpu::Device, res: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fluidvis-dye"),
        size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: res },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn make_ink_target(device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fluidvis-ink"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A 1×1 depth texture bound when no prepass depth exists this frame; the
/// shader's `use_depth` flag is 0 so it's never actually read.
fn make_dummy_depth(device: &wgpu::Device) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fluidvis-dummy-depth"),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

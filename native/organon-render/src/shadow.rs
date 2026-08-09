//! Cast-shadow map (#152 Tier 3).
//!
//! A single world-space depth map rendered from the **key light** each frame, then
//! PCF-sampled in `cube.wgsl` (group 4) to darken the key light's direct term where
//! the instanced geometry occludes the light. This is the headline "#1 realism gap"
//! item — cubes finally cast shadows on each other.
//!
//! Instanced path only (the raymarch siblings keep their own shading). Off → a 1×1
//! dummy map is bound and the shader's `enabled` flag returns visibility 1 (no
//! change). A single tight ortho map (not cascaded — the cube-field is bounded about
//! the origin, so one map covers it; cascades are a follow-up).
//!
//! Reuses the depth-prepass pipeline (`render.rs`) for the light pass: it binds this
//! module's `cam_bind` (a group-0 `Uniforms` whose `view_proj` is the light matrix)
//! and renders the instances depth-only into `map_view`. Included by `render.rs` via
//! `#[path]`.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

/// Shadow-map resolution (square). 2048² is a good balance for a bounded field.
pub const SHADOW_RES: u32 = 2048;

/// Matches `ShadowU` in cube.wgsl (group 4 binding 0).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShadowU {
    light_vp: [[f32; 4]; 4],
    params: [f32; 4],  // enabled, depth bias, strength, texel size
    // #182 T4 fluid light coupling: x = fluid-shadow amount (dye transmittance
    // on the key), y = caustic amount. #195 Tier 1 hardware RT shadows:
    // z = RT key-shadow strength, w = RT fill-shadow strength — when z > 0 the
    // cube shader darkens by the screen-space RT mask (binding 5) instead of
    // the PCF map. All zeroed whenever their pass didn't run this frame, so a
    // stale map/mask is never read.
    params2: [f32; 4],
}

pub struct Shadow {
    map_view: wgpu::TextureView,
    comp_samp: wgpu::Sampler,
    cam_ub: wgpu::Buffer,      // super::Uniforms with view_proj = light matrix
    cam_bind: wgpu::BindGroup, // group-0 layout (for the depth pass)
    light_ub: wgpu::Buffer,    // ShadowU
    layout: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    // #195 Tier 1: the RT shadow mask rides binding 5 — a 1×1 white dummy
    // whenever the RT pass didn't run (visibility 1 = no-op). `rt_bound_gen`
    // tracks which mask generation the group currently points at (0 = dummy).
    rt_dummy_view: wgpu::TextureView,
    rt_bound_gen: u64,
}

impl Shadow {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniform_layout: &wgpu::BindGroupLayout,
        fluidlight_view: &wgpu::TextureView,
        fluidlight_samp: &wgpu::Sampler,
    ) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow-map"),
            size: wgpu::Extent3d {
                width: SHADOW_RES,
                height: SHADOW_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let map_view = tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Group-0 camera uniform for the light depth pass (Uniforms-sized; only
        // `view_proj` is read by the depth-only vertex shader).
        let cam_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow-cam-ub"),
            size: std::mem::size_of::<super::Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cam_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow-cam-bind"),
            layout: uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: cam_ub.as_entire_binding() }],
        });

        let light_ub = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow-light-ub"),
            size: std::mem::size_of::<ShadowU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Comparison sampler: textureSampleCompare returns 1 where the reference
        // depth ≤ the stored depth (lit), 0 occluded.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow-layout"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // #182 T4: the light-space fluid map (r = dye transmittance,
                // g = caustic) + its filtering sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
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
                // #195 Tier 1: the screen-space RT shadow mask (r = key
                // visibility, g = fill). Sampled with the binding-4 filtering
                // sampler; a 1×1 white dummy when the RT pass didn't run.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
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
        // 1×1 WHITE RT-mask dummy: visibility 1 everywhere → the RT branch is
        // a no-op even if it were taken (it also isn't — strength rides at 0).
        let rt_dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow-rt-dummy"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &rt_dummy_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8; 4],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let rt_dummy_view = rt_dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = make_shadow_bind(
            device,
            &layout,
            &light_ub,
            &map_view,
            &sampler,
            fluidlight_view,
            fluidlight_samp,
            &rt_dummy_view,
        );

        Shadow {
            map_view,
            comp_samp: sampler,
            cam_ub,
            cam_bind,
            light_ub,
            layout,
            bind,
            rt_dummy_view,
            rt_bound_gen: 0,
        }
    }

    /// Point binding 5 at this frame's RT shadow mask — or back at the white
    /// dummy when the RT pass didn't run. Rebuilds the group-4 bind only when
    /// the target actually changes (`gen` 0 = dummy; the mask's gen bumps on
    /// every reallocation).
    pub fn rebind_rt_mask(
        &mut self,
        device: &wgpu::Device,
        fluidlight_view: &wgpu::TextureView,
        fluidlight_samp: &wgpu::Sampler,
        mask: Option<(&wgpu::TextureView, u64)>,
    ) {
        let (view, gen) = match mask {
            Some((v, g)) => (v, g),
            None => (&self.rt_dummy_view, 0),
        };
        if gen == self.rt_bound_gen {
            return;
        }
        self.rt_bound_gen = gen;
        self.bind = make_shadow_bind(
            device,
            &self.layout,
            &self.light_ub,
            &self.map_view,
            &self.comp_samp,
            fluidlight_view,
            fluidlight_samp,
            view,
        );
    }

    /// Group-4 layout for the cube pipeline.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }
    /// Group-4 bind group (light matrix + shadow map + comparison sampler).
    pub fn bind(&self) -> &wgpu::BindGroup {
        &self.bind
    }
    /// Group-0 bind for the light depth pass (Uniforms with view_proj = light_vp).
    pub fn cam_bind(&self) -> &wgpu::BindGroup {
        &self.cam_bind
    }
    /// Shadow-map render target (depth) for the light pass.
    pub fn map_view(&self) -> &wgpu::TextureView {
        &self.map_view
    }
    /// The PCF comparison sampler (#182 T4: the ink march shares it).
    pub fn comparison_sampler(&self) -> &wgpu::Sampler {
        &self.comp_samp
    }

    /// Upload the light matrix + shadow params (once per frame, before the passes).
    /// When `enabled` is false the shader returns visibility 1, so the light pass can
    /// be skipped entirely.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &self,
        queue: &wgpu::Queue,
        light_vp: Mat4,
        enabled: bool,
        bias: f32,
        strength: f32,
        fluid_shadow: f32,
        caustic: f32,
        rt_key: f32,
        rt_fill: f32,
    ) {
        let m = light_vp.to_cols_array_2d();
        // Group-0 camera uniform: a zeroed Uniforms with just view_proj set (the
        // depth-only vertex shader reads nothing else).
        let mut cam = super::Uniforms::zeroed();
        cam.view_proj = m;
        queue.write_buffer(&self.cam_ub, 0, bytemuck::bytes_of(&cam));
        queue.write_buffer(
            &self.light_ub,
            0,
            bytemuck::bytes_of(&ShadowU {
                light_vp: m,
                params: [
                    if enabled { 1.0 } else { 0.0 },
                    bias,
                    strength,
                    1.0 / SHADOW_RES as f32,
                ],
                params2: [
                    fluid_shadow.max(0.0),
                    caustic.max(0.0),
                    rt_key.clamp(0.0, 1.0),
                    rt_fill.clamp(0.0, 1.0),
                ],
            }),
        );
    }
}

/// The group-4 bind for the cube pipeline (also rebuilt when the RT mask
/// (re)appears — see `rebind_rt_mask`).
#[allow(clippy::too_many_arguments)]
fn make_shadow_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    light_ub: &wgpu::Buffer,
    map_view: &wgpu::TextureView,
    comp_samp: &wgpu::Sampler,
    fluidlight_view: &wgpu::TextureView,
    fluidlight_samp: &wgpu::Sampler,
    rt_mask_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow-bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: light_ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(map_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(comp_samp) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(fluidlight_view) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(fluidlight_samp) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(rt_mask_view) },
        ],
    })
}

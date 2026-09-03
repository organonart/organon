//! Hardware-RT reflections pass (#195 Tier 2).
//!
//! Cubes reflect the ACTUAL scene — neighbours, off-screen emitters, the field
//! behind the camera — with none of SSR's screen-edge dropout. Runs at SSR's
//! slot (after the depth prepass, before the composite) and writes into the
//! SAME confidence-weighted buffer the composite already blends, so
//! `composite.wgsl` is untouched and a miss falls back to the cube shader's
//! env reflection with no seam. While RT reflections are on they supersede the
//! SSR march entirely (one reflection source at a time).
//!
//! Hits are shaded PBR-lite in the shader from the hit instance's transform +
//! tint (storage-bound copies of the same buffers the raster path draws) — see
//! rt_reflect.wgsl for the local-space colour/normal reconstruction. Like the
//! other `rt_*` modules, every experimental-API call site stays in here.

use glam::Mat4;

/// Per-frame inputs, threaded through `LightTransport.rt_reflect`. `None`
/// there = off (byte-identical path).
#[derive(Clone, Copy)]
pub struct RtReflectFrame<'a> {
    /// The Tier-0 TLAS, built by the visual this frame (`rt::RtContext`).
    pub tlas: &'a wgpu::Tlas,
    /// Reflection intensity (the weight scale; 0 = the pass is skipped).
    pub intensity: f32,
    /// Roughness cutoff — above this the env/IBL reflection stands (SSR's dial).
    pub max_roughness: f32,
    /// Ray reach (world units), finite-clamped by the visual.
    pub t_max: f32,
    /// Trace a key-light shadow ray at each hit (reflections contain shadows).
    pub hit_shadows: bool,
    /// The hit-shadow rays' own reach (world units, finite-clamped by the
    /// visual — the Tier-1 scene reach). Deliberately NOT the reflection
    /// `t_max`: a short reflection-reach dial must not hide occluders from
    /// the shadow rays (#204 review).
    pub shadow_t_max: f32,
    /// Stochastic reflection rays per pixel (1–16; 1 = the original look).
    pub rays: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RtReflectU {
    inv_view_proj: [[f32; 4]; 4],
    params: [f32; 4],  // intensity, max_roughness, t_max, frame index
    params2: [f32; 4], // tube flag, hit-shadow flag, shadow ray reach, rays
}

pub struct RtReflect {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,      // group 0: scene uniforms + params + depth + instances + tints
    tlas_layout: wgpu::BindGroupLayout, // group 1: the acceleration structure
    scene_ubuf: wgpu::Buffer,           // a verbatim copy of render::Uniforms
    ubuf: wgpu::Buffer,
    frame: u32,
}

/// organon#217 T8 — where this pass's group 0 carries the per-instance emission
/// buffer (`rt_reflect.wgsl`'s `emits`): after the instances (3) and tints (4).
pub(crate) const EMIT_BINDING: u32 = 5;

impl RtReflect {
    /// The group-0 layout as data — pure, so a CPU test can hold it (wgpu only
    /// checks a bind group against its layout at draw time; see
    /// `rt_pathtrace::PathTracer::layout_entries`).
    pub(crate) fn layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
        let storage_entry = super::rt_pathtrace::readonly_storage_entry;
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        vec![
            uniform_entry(0),
            uniform_entry(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            storage_entry(3),
            storage_entry(4),
            // organon#217 T8: the per-instance emission (`emit_buf`), so a reflected
            // glyph is lit. All-zero on every non-glyph frame → byte-identical.
            storage_entry(EMIT_BINDING),
        ]
    }

    /// Requires a device with `EXPERIMENTAL_RAY_QUERY` (the caller only
    /// constructs this once a TLAS exists, which already implies it).
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-reflect-bgl"),
            entries: &Self::layout_entries(),
        });
        let tlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt-reflect-tlas-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rt-reflect-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rt_reflect.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt-reflect-pl"),
            bind_group_layouts: &[Some(&layout), Some(&tlas_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rt-reflect-pipeline"),
            layout: Some(&pl),
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
                    // The SSR buffer's format (post.rs HDR_FORMAT): linear
                    // radiance × weight, weight in alpha.
                    format: super::post::HDR_FORMAT,
                    // Premultiplied src-over. RT alone (Clear→transparent, one
                    // full-screen draw) composites onto (0,0,0,0) → src, so this
                    // is byte-identical to the old overwrite. In the RT+SSR
                    // HYBRID (SSR ran first, RT loads) it composites RT OVER SSR:
                    // RT hits (weight>0) replace the SSR reflection, RT misses
                    // (0,0,0,0) leave SSR showing through — the confidence
                    // weights layer RT > SSR > env automatically.
                    blend: Some(wgpu::BlendState {
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
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let scene_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-reflect-scene-uniforms"),
            size: std::mem::size_of::<super::Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt-reflect-uniforms"),
            size: std::mem::size_of::<RtReflectU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        RtReflect { pipeline, layout, tlas_layout, scene_ubuf, ubuf, frame: 0 }
    }

    /// Trace this frame's reflections into `target` (the composite's SSR
    /// buffer). `depth_view` is the shared single-sample prepass depth;
    /// `uniforms` the (jittered) scene uniforms; `inst_buf`/`tint_buf`/`emit_buf`
    /// the live instance/tint/emission buffers (storage-bound; the TLAS custom
    /// indices point into all three — `emit_buf` is organon#217 T8's addition).
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        uniforms: &super::Uniforms,
        inst_buf: &wgpu::Buffer,
        tint_buf: &wgpu::Buffer,
        emit_buf: &wgpu::Buffer,
        tube: bool,
        // `true` = load + blend RT over the buffer's existing content (the
        // hybrid, where SSR wrote it first); `false` = clear first (RT alone).
        load: bool,
        p: &RtReflectFrame,
    ) {
        self.frame = self.frame.wrapping_add(1);
        queue.write_buffer(&self.scene_ubuf, 0, bytemuck::bytes_of(uniforms));
        let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
        let u = RtReflectU {
            inv_view_proj: inv_vp.to_cols_array_2d(),
            params: [
                p.intensity.max(0.0),
                p.max_roughness.clamp(0.0, 1.0),
                p.t_max.max(1.0),
                self.frame as f32,
            ],
            params2: [
                if tube { 1.0 } else { 0.0 },
                if p.hit_shadows { 1.0 } else { 0.0 },
                p.shadow_t_max.max(1.0),
                p.rays.clamp(1, 16) as f32,
            ],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-reflect-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.scene_ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth_view),
                },
                wgpu::BindGroupEntry { binding: 3, resource: inst_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: tint_buf.as_entire_binding() },
                // organon#217 T8: the per-instance emission (every layout entry must be
                // supplied here, or wgpu rejects the bind group at draw time).
                wgpu::BindGroupEntry { binding: EMIT_BINDING, resource: emit_buf.as_entire_binding() },
            ],
        });
        let tlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt-reflect-tlas-bind"),
            layout: &self.tlas_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::AccelerationStructure(p.tlas),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rt-reflect-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if load {
                        wgpu::LoadOp::Load // hybrid: blend RT over the SSR fill
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.set_bind_group(1, &tlas_bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reflection_pass_binds_the_emit_buffer_where_its_shader_reads_it() {
        // organon#217 T8 — see `rt_pathtrace::emit_binding` for what this pins and why
        // it has to be pinned on the CPU.
        let entries = RtReflect::layout_entries();
        super::super::rt_pathtrace::emit_binding::check(
            "rt_reflect",
            &entries,
            EMIT_BINDING,
            include_str!("rt_reflect.wgsl"),
        );
        // Five bindings before T8 (scene, params, depth, insts, tints) + one.
        assert_eq!(entries.len(), 6, "rt_reflect: group 0 should carry exactly six entries");
        assert!(
            include_str!("rt_reflect.wgsl").contains("radiance = radiance + emissive + phosphor + diffuse + ambient;"),
            "rt_reflect.wgsl: the hit's radiance no longer adds its own emission"
        );
    }
}

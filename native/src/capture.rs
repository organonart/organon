//! Capture / production frame (#135 Phase 1).
//!
//! Renders the scene+composite into a **fixed-resolution offscreen texture**, then
//! blits it **centred (letterboxed)** into the window swapchain. This makes an OBS
//! capture pixel-exact: the production texture is the dimensions we choose (e.g.
//! 1080×1920 for 9:16), independent of the window/monitor. A per-display capture
//! setting (a sibling of HDR/MSAA), driven by `Shared.capture[12]`.
//!
//! Compiled only by the visual binary (via `#[path]`), like `render.rs`. The pure
//! sizing maths (`production_size`, `letterbox_rect`) is unit-tested below; the blit
//! shader (`capture.wgsl`) is naga-validated offline by `tests/wgsl.rs`.

/// The fixed output size for a capture aspect, or `None` for **Native** (= render
/// straight to the window). Mirrors `params::AspectPreset` indices:
/// 0 Native · 1 9:16 · 2 16:9 · 3 1:1 · 4 4:5 · 5 21:9 · 6 Custom.
///
/// For the fixed presets `long_edge` sets the *longer* dimension (the short edge is
/// derived from the ratio); `Custom` uses `custom_w`×`custom_h` verbatim. Results are
/// rounded to even pixels (≥2) so encoders/blits stay tidy.
pub fn production_size(
    aspect: u32,
    long_edge: u32,
    custom_w: u32,
    custom_h: u32,
) -> Option<(u32, u32)> {
    let even = |v: u32| (v & !1u32).max(2);
    match aspect {
        0 => None,                                          // Native → use the window
        6 => Some((even(custom_w), even(custom_h))),        // Custom
        _ => {
            let (rw, rh): (u32, u32) = match aspect {
                1 => (9, 16),
                2 => (16, 9),
                3 => (1, 1),
                4 => (4, 5),
                5 => (21, 9),
                _ => return None,
            };
            let long = long_edge.max(2) as f64;
            let (w, h) = if rw >= rh {
                (long, long * rh as f64 / rw as f64)
            } else {
                (long * rw as f64 / rh as f64, long)
            };
            Some((even(w.round() as u32), even(h.round() as u32)))
        }
    }
}

/// Centred aspect-fit of an `out` rectangle inside the `win` rectangle, as an
/// integer viewport `(x, y, w, h)`. Scales `out` by `min(win/out)` so it fits
/// entirely, then centres it — the remaining margin is the letterbox/pillarbox.
pub fn letterbox_rect(win: (u32, u32), out: (u32, u32)) -> (u32, u32, u32, u32) {
    let (ww, wh) = (win.0.max(1), win.1.max(1));
    let (ow, oh) = (out.0.max(1), out.1.max(1));
    let s = (ww as f64 / ow as f64).min(wh as f64 / oh as f64);
    let w = ((ow as f64 * s).round() as u32).clamp(1, ww);
    let h = ((oh as f64 * s).round() as u32).clamp(1, wh);
    let x = (ww - w) / 2;
    let y = (wh - h) / 2;
    (x, y, w, h)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitUniform {
    /// Frame-guide border half-thickness in UV, per axis (0,0 = guide off).
    thick: [f32; 2],
    _pad: [f32; 2],
    /// Guide line colour (display-referred; written straight to the surface).
    guide_color: [f32; 4],
}

struct ProdTarget {
    size: (u32, u32),
    format: wgpu::TextureFormat,
    tex: wgpu::Texture,
}

/// Owns the production texture + the letterbox blit pipeline. Lives on the visual's
/// `Gfx` next to the `Renderer`, so it shares the device/queue and tracks the same
/// surface format (rebuilds when the HDR toggle swaps sRGB8 ↔ Rgba16Float).
pub struct Capture {
    sampler: wgpu::Sampler,
    bgl: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    ubuf: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    pipeline_format: wgpu::TextureFormat,
    prod: Option<ProdTarget>,
}

impl Capture {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Capture {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("capture-blit-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("capture.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("capture-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("capture-bgl"),
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
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture-ubuf"),
            size: std::mem::size_of::<BlitUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline = make_pipeline(device, &shader, &bgl, format);
        Capture {
            sampler,
            bgl,
            shader,
            ubuf,
            pipeline,
            pipeline_format: format,
            prod: None,
        }
    }

    /// Ensure the production texture exists at `size`/`format`, returning a fresh
    /// view for the renderer to composite into. The renderer's composite pipeline is
    /// built for the current surface format, so `format` must equal the swapchain's.
    pub fn production_view(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureView {
        // Floor of 2, matching `recorder::Recorder`'s own clamp: the recorder sizes its
        // readback from the same `size` and copies the full extent out of this texture, so a
        // looser floor here would let the copy exceed the texture for a degenerate 1-pixel
        // window. (Video encoders want >= 2 px anyway.)
        let size = (size.0.max(2), size.1.max(2));
        let stale = self
            .prod
            .as_ref()
            .map(|p| p.size != size || p.format != format)
            .unwrap_or(true);
        if stale {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("capture-production"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                // COPY_SRC (#430): the in-app recorder reads this texture back to the
                // CPU each frame. Free when nothing records (a usage flag, no cost).
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            self.prod = Some(ProdTarget { size, format, tex });
        }
        self.prod
            .as_ref()
            .unwrap()
            .tex
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// The production texture itself (for the #430 recorder's GPU→CPU readback), valid
    /// after `production_view` has ensured it this frame. `None` before the first use.
    pub fn production_tex(&self) -> Option<&wgpu::Texture> {
        self.prod.as_ref().map(|p| &p.tex)
    }

    /// Clear `dst` to the letterbox `backdrop`, then blit the production texture into
    /// the centred fit rect. `guide` draws a thin safe-area border at the frame edge.
    /// Pure pass-through (no tonemap, no clamp) so EDR/HDR content survives.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_letterbox(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        window: (u32, u32),
        out: (u32, u32),
        backdrop: [f32; 3],
        guide: bool,
    ) {
        if dst_format != self.pipeline_format {
            self.pipeline = make_pipeline(device, &self.shader, &self.bgl, dst_format);
            self.pipeline_format = dst_format;
        }
        let Some(prod) = self.prod.as_ref() else { return };

        // A ~2 px guide, expressed in UV so it's even on both axes regardless of the
        // output's pixel aspect.
        let thick = if guide {
            [2.0 / out.0.max(1) as f32, 2.0 / out.1.max(1) as f32]
        } else {
            [0.0, 0.0]
        };
        let u = BlitUniform {
            thick,
            _pad: [0.0, 0.0],
            guide_color: [0.35, 0.8, 1.0, 1.0], // light blue, like the reference frame
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));

        let src_view = prod.tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("capture-bind"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.ubuf.as_entire_binding(),
                },
            ],
        });

        let (x, y, w, h) = letterbox_rect(window, out);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture-blit") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("capture-letterbox-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: backdrop[0] as f64,
                            g: backdrop[1] as f64,
                            b: backdrop[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Scissor confines the draw to the fit rect; viewport maps NDC→rect.
            rp.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            rp.set_scissor_rect(x, y, w, h);
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    bgl: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("capture-pl"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("capture-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_custom_sizes() {
        assert_eq!(production_size(0, 1920, 1920, 1080), None); // Native
        assert_eq!(production_size(6, 1920, 1920, 1080), Some((1920, 1080))); // Custom
        // Custom rounds to even, ≥2.
        assert_eq!(production_size(6, 1920, 1921, 1081), Some((1920, 1080)));
        assert_eq!(production_size(6, 1920, 1, 1), Some((2, 2)));
    }

    #[test]
    fn fixed_aspect_long_edge() {
        // Portrait 9:16, long edge = height.
        assert_eq!(production_size(1, 1920, 0, 0), Some((1080, 1920)));
        // Landscape 16:9, long edge = width.
        assert_eq!(production_size(2, 1920, 0, 0), Some((1920, 1080)));
        // Square.
        assert_eq!(production_size(3, 1440, 0, 0), Some((1440, 1440)));
        // Portrait 4:5.
        assert_eq!(production_size(4, 2000, 0, 0), Some((1600, 2000)));
        // Cinematic 21:9 (width is the long edge; height even-rounded).
        assert_eq!(production_size(5, 2520, 0, 0), Some((2520, 1080)));
    }

    #[test]
    fn letterbox_centers_and_fits() {
        // Wider window than a portrait output → pillarbox (bars left/right).
        let (x, y, w, h) = letterbox_rect((1000, 1000), (1080, 1920));
        assert_eq!((w, h), (563, 1000)); // height-limited: 1080*1000/1920 ≈ 562.5 → 563
        assert_eq!(y, 0);
        assert_eq!(x, (1000 - 563) / 2);

        // Taller window than a landscape output → letterbox (bars top/bottom).
        let (x, y, w, h) = letterbox_rect((1000, 1000), (1920, 1080));
        assert_eq!((w, h), (1000, 563));
        assert_eq!(x, 0);
        assert_eq!(y, (1000 - 563) / 2);

        // Exact aspect match → fills, no bars.
        assert_eq!(letterbox_rect((1920, 1080), (1920, 1080)), (0, 0, 1920, 1080));
        assert_eq!(letterbox_rect((960, 540), (1920, 1080)), (0, 0, 960, 540));
    }

    #[test]
    fn letterbox_rect_stays_in_bounds() {
        for (win, out) in [
            ((1100u32, 760u32), (1080u32, 1920u32)),
            ((1920, 1080), (1080, 1920)),
            ((33, 1000), (1920, 1080)),
            ((1, 1), (21, 9)),
        ] {
            let (x, y, w, h) = letterbox_rect(win, out);
            assert!(x + w <= win.0, "x+w out of bounds for {win:?}/{out:?}");
            assert!(y + h <= win.1, "y+h out of bounds for {win:?}/{out:?}");
        }
    }
}

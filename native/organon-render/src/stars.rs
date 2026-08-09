//! HDR starfield — the Yale Bright Star Catalog (BSC5) parsed + embedded, plus the
//! GPU pass that draws it as instanced point-sprite billboards behind the scene.
//!
//! The catalog ships embedded (`include_bytes!`, ~292 KB) so the visual binary has
//! no external-file dependency. `parse_catalog` decodes it into a star table of
//! {equatorial unit vector, V-magnitude, spectral colour}; it's pure + unit-tested.
//!
//! The render pass mirrors the terrain backdrop: a peer of the skybox drawn into
//! the linear-HDR scene buffer as a BACKGROUND (far-plane depth, Equal/no-write on
//! the opaque path, Always/write otherwise) so the generator composites in front.
//! Stars are additive — bright stars output linear > 1 so they bloom + use the
//! EDR/true-HDR headroom — and tinted by spectral type. A per-frame rotation matrix
//! R(latitude, sidereal-time) maps the fixed equatorial vectors into world space so
//! the sky sits correctly over the horizon and wheels with time; the orbit camera
//! frames it via the scene view-projection. A companion HDR sun disc rides the same
//! day-cycle sun direction.

use bytemuck::{Pod, Zeroable};

/// Number of catalog entries (STARN = −9110 in the BSC5 header ⇒ standard format,
/// 9110 stars). A handful are blank/deleted placeholders (flagged `present=false`).
pub const STAR_COUNT: usize = 9110;

const CATALOG: &[u8] = include_bytes!("BSC5");
const HEADER_BYTES: usize = 28; // 7 × i32
const ENTRY_BYTES: usize = 32; // NBENT

/// One catalog star, decoded to render-ready form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Star {
    /// Equatorial unit vector (B1950): x = vernal equinox, z = north celestial pole.
    pub dir: [f32; 3],
    /// V-magnitude (lower = brighter; Sirius ≈ −1.46, faint BSC ≈ 8.0).
    pub mag: f32,
    /// Spectral-type tint (normalized so the brightest channel ≈ 1; the magnitude
    /// carries the brightness). O/B blue → A white → G yellow → K/M red.
    pub color: [f32; 3],
    /// False for the ~14 blank/deleted catalog slots (RA = Dec = mag = 0); these are
    /// kept (so the table is exactly `STAR_COUNT`) but never rendered.
    pub present: bool,
}

#[inline]
fn le_f64(b: &[u8]) -> f64 {
    f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}
#[inline]
fn le_i16(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}

/// Map a spectral class (first letter of the BSC type field) to an approximate
/// blackbody tint. Normalized so the max channel ≈ 1 — brightness comes from the
/// magnitude, this is only the hue. Unknown/peculiar classes fall back to white.
fn spectral_color(class: u8) -> [f32; 3] {
    match class {
        b'O' | b'W' => [0.61, 0.71, 1.00], // hot blue (incl. Wolf–Rayet)
        b'B' => [0.67, 0.75, 1.00],         // blue-white
        b'A' => [0.82, 0.86, 1.00],         // white
        b'F' => [1.00, 0.97, 0.92],         // yellow-white
        b'G' => [1.00, 0.92, 0.74],         // yellow (Sun-like)
        b'K' => [1.00, 0.80, 0.56],         // orange
        b'M' => [1.00, 0.66, 0.46],         // red-orange
        b'C' | b'N' | b'S' => [1.00, 0.55, 0.36], // deep-red carbon / S stars
        _ => [1.00, 1.00, 1.00],
    }
}

/// Decode the embedded BSC5 into the star table. Deterministic; always returns
/// exactly `STAR_COUNT` entries (blank slots flagged `present=false`).
pub fn parse_catalog() -> Vec<Star> {
    let mut out = Vec::with_capacity(STAR_COUNT);
    for i in 0..STAR_COUNT {
        let off = HEADER_BYTES + i * ENTRY_BYTES;
        let e = &CATALOG[off..off + ENTRY_BYTES];
        // Layout (LE): XNO f32, SRA0 f64 (RA rad), SDEC0 f64 (Dec rad),
        // IS 2×char (spectral), MAG i16 (V×100), XRPM f32, XDPM f32.
        let ra = le_f64(&e[4..12]);
        let dec = le_f64(&e[12..20]);
        let class = e[20];
        let mag = le_i16(&e[22..24]) as f32 / 100.0;
        // Blank/deleted slots have RA = Dec = mag = 0 simultaneously.
        let present = !(ra == 0.0 && dec == 0.0 && mag == 0.0);
        let (cd, sd) = (dec.cos(), dec.sin());
        let (ca, sa) = (ra.cos(), ra.sin());
        out.push(Star {
            dir: [(cd * ca) as f32, (cd * sa) as f32, sd as f32],
            mag,
            color: spectral_color(class),
            present,
        });
    }
    out
}

// --- GPU pass ---------------------------------------------------------------

/// Per-instance star data uploaded once (the catalog never changes). Blank slots
/// are given `mag = CULL_MAG` so the magnitude-limit test always drops them.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct StarInstance {
    dir: [f32; 3],
    mag: f32,
    color: [f32; 3],
    _pad: f32,
}

const CULL_MAG: f32 = 99.0;

/// Star pass uniform (matches `StarU` in stars.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct StarUniforms {
    /// Unscaled scene view-projection — projects star directions (point-at-infinity,
    /// w = 0) to the same screen positions the orbit camera frames the scene with.
    pub view_proj: [[f32; 4]; 4],
    /// Equatorial → world rotation (latitude tilt + sidereal spin); 3×3 in the
    /// upper-left, applied as `rot · vec4(dir, 0)`.
    pub rot: [[f32; 4]; 4],
    /// brightness, star_size (px), night_factor (0..1), mag_limit.
    pub params: [f32; 4],
    /// twinkle_amount, twinkle_speed, time (s), colour_saturation.
    pub twinkle: [f32; 4],
    /// viewport width, height, _, _ (px → NDC sprite sizing + aspect).
    pub viewport: [f32; 4],
    /// xyz = sun world direction (unit), w = sun NDC radius (vertical).
    pub sun: [f32; 4],
    /// rgb = sun tint, w = sun HDR brightness (≤ 0 ⇒ sun off / below horizon).
    pub sun_color: [f32; 4],
}

pub struct Stars {
    uniform: wgpu::Buffer,
    u_bind: wgpu::BindGroup,
    corner_buf: wgpu::Buffer,
    inst_buf: wgpu::Buffer,
    inst_count: u32,
    shader: wgpu::ShaderModule,
    pl: wgpu::PipelineLayout,
    // Two depth variants mirroring the skybox/terrain: Always/write (glass/fallback)
    // and Equal/no-write (opaque early-Z path).
    star_always: wgpu::RenderPipeline,
    star_eq: wgpu::RenderPipeline,
    sun_always: wgpu::RenderPipeline,
    sun_eq: wgpu::RenderPipeline,
}

impl Stars {
    pub fn new(
        device: &wgpu::Device,
        hdr_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stars"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stars.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star-uniforms"),
            size: std::mem::size_of::<StarUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let u_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("star-uniform-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let u_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("star-uniform-bind"),
            layout: &u_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });

        // A unit quad (two triangles) reused per instance for the billboard sprite.
        let corners: [[f32; 2]; 6] = [
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 1.0],
            [-1.0, -1.0],
            [1.0, 1.0],
            [-1.0, 1.0],
        ];
        let corner_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star-corners"),
            size: std::mem::size_of_val(&corners) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        corner_buf
            .slice(..)
            .get_mapped_range_mut()
            .unwrap()
            .copy_from_slice(bytemuck::cast_slice(&corners));
        corner_buf.unmap();

        // Per-star instance buffer, built once from the embedded catalog.
        let stars = parse_catalog();
        let insts: Vec<StarInstance> = stars
            .iter()
            .map(|s| StarInstance {
                dir: s.dir,
                mag: if s.present { s.mag } else { CULL_MAG },
                color: s.color,
                _pad: 0.0,
            })
            .collect();
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("star-instances"),
            size: (insts.len() * std::mem::size_of::<StarInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        inst_buf
            .slice(..)
            .get_mapped_range_mut()
            .unwrap()
            .copy_from_slice(bytemuck::cast_slice(&insts));
        inst_buf.unmap();

        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("star-pipeline-layout"),
            bind_group_layouts: &[Some(&u_layout)],
            immediate_size: 0,
        });
        let star_always = make_pipeline(
            device, &shader, &pl, hdr_format, depth_format, sample_count, "vs_star", "fs_star",
            true, wgpu::CompareFunction::Always, true,
        );
        let star_eq = make_pipeline(
            device, &shader, &pl, hdr_format, depth_format, sample_count, "vs_star", "fs_star",
            true, wgpu::CompareFunction::Equal, false,
        );
        let sun_always = make_pipeline(
            device, &shader, &pl, hdr_format, depth_format, sample_count, "vs_sun", "fs_sun",
            false, wgpu::CompareFunction::Always, true,
        );
        let sun_eq = make_pipeline(
            device, &shader, &pl, hdr_format, depth_format, sample_count, "vs_sun", "fs_sun",
            false, wgpu::CompareFunction::Equal, false,
        );

        Stars {
            uniform,
            u_bind,
            corner_buf,
            inst_buf,
            inst_count: insts.len() as u32,
            shader,
            pl,
            star_always,
            star_eq,
            sun_always,
            sun_eq,
        }
    }

    /// Rebuild the pipelines for a new MSAA sample count (alongside the scene pipelines).
    pub fn set_sample_count(
        &mut self,
        device: &wgpu::Device,
        hdr_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        n: u32,
    ) {
        self.star_always = make_pipeline(
            device, &self.shader, &self.pl, hdr_format, depth_format, n, "vs_star", "fs_star",
            true, wgpu::CompareFunction::Always, true,
        );
        self.star_eq = make_pipeline(
            device, &self.shader, &self.pl, hdr_format, depth_format, n, "vs_star", "fs_star",
            true, wgpu::CompareFunction::Equal, false,
        );
        self.sun_always = make_pipeline(
            device, &self.shader, &self.pl, hdr_format, depth_format, n, "vs_sun", "fs_sun",
            false, wgpu::CompareFunction::Always, true,
        );
        self.sun_eq = make_pipeline(
            device, &self.shader, &self.pl, hdr_format, depth_format, n, "vs_sun", "fs_sun",
            false, wgpu::CompareFunction::Equal, false,
        );
    }

    pub fn write_uniforms(&self, queue: &wgpu::Queue, u: &StarUniforms) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(u));
    }

    /// Draw the starfield (and HDR sun) into the current scene pass, after the
    /// background. `opaque_path` selects the Equal/no-write variant. `draw_sun`
    /// adds the sun disc (skipped when the sun is off/below the horizon).
    pub fn draw(&self, rp: &mut wgpu::RenderPass<'_>, opaque_path: bool, draw_sun: bool) {
        // Sun first (a broad halo the sharp stars sparkle over).
        if draw_sun {
            rp.set_pipeline(if opaque_path { &self.sun_eq } else { &self.sun_always });
            rp.set_bind_group(0, &self.u_bind, &[]);
            rp.draw(0..6, 0..1);
        }
        rp.set_pipeline(if opaque_path { &self.star_eq } else { &self.star_always });
        rp.set_bind_group(0, &self.u_bind, &[]);
        rp.set_vertex_buffer(0, self.corner_buf.slice(..));
        rp.set_vertex_buffer(1, self.inst_buf.slice(..));
        rp.draw(0..6, 0..self.inst_count);
    }
}

#[allow(clippy::too_many_arguments)]
fn make_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    hdr_format: wgpu::TextureFormat,
    depth_format: wgpu::TextureFormat,
    sample_count: u32,
    vs: &str,
    fs: &str,
    instanced: bool, // star pass binds the corner + instance buffers; sun pass binds none
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
) -> wgpu::RenderPipeline {
    // Additive HDR blend: src + dst (no alpha modulation — the falloff is premultiplied).
    let blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let corner_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<[f32; 2]>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
    };
    let inst_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<StarInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![1 => Float32x3, 2 => Float32, 3 => Float32x3],
    };
    let buffers: &[Option<wgpu::VertexBufferLayout>] =
        if instanced { &[Some(corner_layout), Some(inst_layout)] } else { &[] };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("star-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs),
            buffers,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            targets: &[Some(wgpu::ColorTargetState {
                format: hdr_format,
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
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(depth_compare),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: sample_count, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_to_a_sane_star_table() {
        let stars = parse_catalog();
        // The header says 9110 standard-format entries.
        assert_eq!(stars.len(), STAR_COUNT);
        // Deterministic.
        assert_eq!(stars, parse_catalog());

        let mut present = 0;
        for (i, s) in stars.iter().enumerate() {
            // Unit vectors.
            let n = (s.dir[0] * s.dir[0] + s.dir[1] * s.dir[1] + s.dir[2] * s.dir[2]).sqrt();
            assert!((n - 1.0).abs() < 1e-4, "star {i} not a unit vector: {n}");
            // Magnitudes in a sane V-band range (blanks read 0).
            assert!((-3.0..=10.0).contains(&s.mag), "star {i} mag {} out of range", s.mag);
            // Colour tint normalized (max channel ≈ 1, all in [0,1]).
            let m = s.color[0].max(s.color[1]).max(s.color[2]);
            assert!((m - 1.0).abs() < 1e-4, "star {i} colour not normalized: {:?}", s.color);
            assert!(s.color.iter().all(|&c| (0.0..=1.0).contains(&c)));
            if s.present {
                present += 1;
            }
        }
        // The vast majority are real stars (only ~14 blank slots).
        assert!(present > 9000, "too few present stars: {present}");

        // RA ∈ [0, 2π), Dec ∈ [−π/2, π/2] — reconstruct from the unit vector.
        use std::f32::consts::{PI, TAU};
        for s in &stars {
            let dec = s.dir[2].clamp(-1.0, 1.0).asin();
            let ra = s.dir[1].atan2(s.dir[0]).rem_euclid(TAU);
            assert!((0.0..TAU).contains(&ra));
            assert!((-PI / 2.0 - 1e-4..=PI / 2.0 + 1e-4).contains(&dec));
        }
    }

    #[test]
    fn entry_one_matches_the_known_header() {
        // BSC #1: XNO 1.0, RA ≈ 0.02254 rad, Dec ≈ 0.78940 rad, type "A1", mag 6.70.
        let s = &parse_catalog()[0];
        assert!(s.present);
        assert!((s.mag - 6.70).abs() < 1e-3, "mag {}", s.mag);
        // RA 0.02254, Dec 0.78940 → reconstruct and compare.
        let dec = s.dir[2].clamp(-1.0, 1.0).asin();
        let ra = s.dir[1].atan2(s.dir[0]).rem_euclid(std::f32::consts::TAU);
        assert!((ra - 0.022536).abs() < 1e-3, "ra {ra}");
        assert!((dec - 0.789398).abs() < 1e-3, "dec {dec}");
        // Spectral class A → white-ish tint (blue ≥ red).
        assert!(s.color[2] >= s.color[0]);
    }
}

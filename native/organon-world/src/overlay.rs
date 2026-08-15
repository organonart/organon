//! Capture overlay renderer (#135 Phase 2): the maths-account-style 2-D overlay —
//! title, description, formula image, a live readout panel, and a handle — composited
//! on top of the production frame after the letterbox blit, before present.
//!
//! Self-contained (no `glyphon`, so no `wgpu 29` coupling risk): a CPU glyph atlas
//! (`ab_glyph`, rasterized once) + bundled formula PNGs, drawn as alpha-blended quads
//! through one tiny pipeline (`overlay.wgsl`), mirroring `capture.rs`. The pure atlas /
//! layout maths is unit-tested; the metadata + live values come from `overlay_meta.rs`.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use organon_scene::overlay_meta::{OverlayMeta, Values, LIVE_SLOT};

// --- bundled assets ---------------------------------------------------------
const FONT_REGULAR: &[u8] = include_bytes!("overlay/font_regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("overlay/font_bold.ttf");
const F_ORIGINAL: &[u8] = include_bytes!("overlay/formula_original.png");
const F_FRENET: &[u8] = include_bytes!("overlay/formula_frenet.png");
const F_DNA: &[u8] = include_bytes!("overlay/formula_dna.png");
const F_HARMONIC: &[u8] = include_bytes!("overlay/formula_harmonic.png");
const F_MINIMAL: &[u8] = include_bytes!("overlay/formula_minimal.png");
const F_ATTRACTOR: &[u8] = include_bytes!("overlay/formula_attractor.png");
const F_LSYSTEM: &[u8] = include_bytes!("overlay/formula_lsystem.png");
const F_CURLNOISE: &[u8] = include_bytes!("overlay/formula_curlnoise.png");
const F_POLARIZATION: &[u8] = include_bytes!("overlay/formula_polarization.png");
const F_MAXWELL: &[u8] = include_bytes!("overlay/formula_maxwell.png");
const F_PHYLLOTAXIS: &[u8] = include_bytes!("overlay/formula_phyllotaxis.png");
const F_MANDELBULB: &[u8] = include_bytes!("overlay/formula_mandelbulb.png");
const F_KIFS: &[u8] = include_bytes!("overlay/formula_kifs.png");
const F_BOIDS: &[u8] = include_bytes!("overlay/formula_boids.png");
const F_TESSELLATION: &[u8] = include_bytes!("overlay/formula_tessellation.png");
const F_SYNCHROTRON: &[u8] = include_bytes!("overlay/formula_synchrotron.png");
const F_VECFIELD: &[u8] = include_bytes!("overlay/formula_vecfield.png");
const F_RAILS: &[u8] = include_bytes!("overlay/formula_rails.png");
const F_AXON: &[u8] = include_bytes!("overlay/formula_axon.png");

/// Bundled formula images, ordered to match `formula_index`.
const FORMULA_PNGS: [&[u8]; 19] = [
    F_ORIGINAL, F_FRENET, F_DNA, F_HARMONIC, F_MINIMAL, F_ATTRACTOR, F_LSYSTEM, F_CURLNOISE,
    F_POLARIZATION, F_MAXWELL, F_PHYLLOTAXIS, F_MANDELBULB, F_KIFS, F_BOIDS, F_TESSELLATION,
    F_SYNCHROTRON, F_VECFIELD, F_RAILS, F_AXON,
];

fn formula_index(f: organon_scene::overlay_meta::FormulaId) -> usize {
    use organon_scene::overlay_meta::FormulaId::*;
    match f {
        Original => 0,
        Frenet => 1,
        Dna => 2,
        Harmonic => 3,
        Minimal => 4,
        Attractor => 5,
        LSystem => 6,
        CurlNoise => 7,
        Polarization => 8,
        Maxwell => 9,
        Phyllotaxis => 10,
        Mandelbulb => 11,
        Kifs => 12,
        Boids => 13,
        Tessellation => 14,
        Synchrotron => 15,
        VectorField => 16,
        Rails => 17,
        Axon => 18,
    }
}
fn formula_bytes(f: organon_scene::overlay_meta::FormulaId) -> &'static [u8] {
    FORMULA_PNGS[formula_index(f)]
}

/// Fold common Unicode punctuation that sneaks into prose to its ASCII lookalike, so the
/// ASCII-only atlas degrades gracefully instead of drawing '?' (the atlas covers ASCII;
/// metadata is kept ASCII too, this is a belt-and-braces net).
fn fold_ascii(ch: char) -> char {
    match ch {
        '\u{2010}'..='\u{2015}' => '-',  // hyphens / en / em dashes
        '\u{2018}' | '\u{2019}' => '\'', // curly single quotes
        '\u{201C}' | '\u{201D}' => '"',  // curly double quotes
        '\u{2026}' => '.',               // ellipsis → '.'
        '\u{00B7}' | '\u{2022}' => '.',  // middle dot / bullet
        '\u{00D7}' => 'x',               // multiplication sign
        '\u{2212}' => '-',               // minus sign
        '\u{00A0}' => ' ',               // nbsp
        other => other,
    }
}

// ============================================================================
// Pure CPU glyph atlas + layout (no GPU — unit-tested)
// ============================================================================

#[derive(Clone, Copy, Default)]
struct Glyph {
    uv: [f32; 4],   // u0,v0,u1,v1 in the atlas
    size: [f32; 2], // px at base size
    min: [f32; 2],  // bitmap top-left relative to pen baseline (min.y < 0 = above)
    advance: f32,
}

/// Per-font glyph metrics (printable ASCII 0x20..0x7E). Pure; the GPU atlas texture is
/// built from the same `rasterize_atlas` rgba buffer.
pub struct AtlasMetrics {
    glyphs: [Glyph; 96],
    base_px: f32,
    #[allow(dead_code)] // line spacing — used by tests + reserved for multi-line zones
    line_height: f32,
}

impl AtlasMetrics {
    fn glyph(&self, ch: char) -> &Glyph {
        let ch = fold_ascii(ch); // map common Unicode punctuation → ASCII (no '?')
        let c = ch as u32;
        let idx = if (32..127).contains(&c) {
            (c - 32) as usize
        } else {
            ('?' as u32 - 32) as usize
        };
        &self.glyphs[idx]
    }
    /// Advance width of `text` rendered at `px`.
    pub fn measure(&self, text: &str, px: f32) -> f32 {
        let s = px / self.base_px;
        text.chars().map(|c| self.glyph(c).advance * s).sum()
    }
    /// Greedy word-wrap `text` to lines no wider than `max_w` px (an over-long single
    /// word still gets its own line). Always returns ≥ 1 line.
    pub fn wrap(&self, text: &str, px: f32, max_w: f32) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        for word in text.split_whitespace() {
            let trial = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if cur.is_empty() || self.measure(&trial, px) <= max_w {
                cur = trial;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }
}

const ATLAS_W: u32 = 1024;
const ATLAS_PAD: u32 = 2;

/// Rasterize printable ASCII into an RGBA coverage atlas (rgb = 1, a = coverage) and
/// return the metrics + the buffer. Pure CPU (no wgpu), so it's unit-testable.
fn rasterize_atlas(font_bytes: &[u8], base_px: f32) -> (AtlasMetrics, Vec<u8>, u32, u32) {
    let font = FontRef::try_from_slice(font_bytes).expect("overlay font parse");
    let scale = PxScale::from(base_px);
    let sf = font.as_scaled(scale);
    let line_height = sf.height() + sf.line_gap();

    struct Placed {
        idx: usize,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        cov: Vec<u8>,
    }
    let mut glyphs = [Glyph::default(); 96];
    let mut placed: Vec<Placed> = Vec::new();
    let (mut cx, mut cy, mut row_h) = (ATLAS_PAD, ATLAS_PAD, 0u32);

    for c in 32u8..127 {
        let ch = c as char;
        let idx = (c - 32) as usize;
        let gid = font.glyph_id(ch);
        let advance = sf.h_advance(gid);
        let g = gid.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            let w = b.width().ceil() as u32;
            let h = b.height().ceil() as u32;
            if w > 0 && h > 0 {
                let mut cov = vec![0u8; (w * h) as usize];
                og.draw(|x, y, a| {
                    if x < w && y < h {
                        cov[(y * w + x) as usize] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                });
                if cx + w + ATLAS_PAD > ATLAS_W {
                    cx = ATLAS_PAD;
                    cy += row_h + ATLAS_PAD;
                    row_h = 0;
                }
                glyphs[idx] = Glyph {
                    uv: [0.0; 4],
                    size: [w as f32, h as f32],
                    min: [b.min.x, b.min.y],
                    advance,
                };
                placed.push(Placed { idx, x: cx, y: cy, w, h, cov });
                cx += w + ATLAS_PAD;
                row_h = row_h.max(h);
                continue;
            }
        }
        glyphs[idx] = Glyph { uv: [0.0; 4], size: [0.0, 0.0], min: [0.0, 0.0], advance };
    }

    let height = (cy + row_h + ATLAS_PAD).max(4);
    let mut rgba = vec![0u8; (ATLAS_W * height * 4) as usize];
    for p in &placed {
        for yy in 0..p.h {
            for xx in 0..p.w {
                let a = p.cov[(yy * p.w + xx) as usize];
                let di = (((p.y + yy) * ATLAS_W + (p.x + xx)) * 4) as usize;
                rgba[di] = 255;
                rgba[di + 1] = 255;
                rgba[di + 2] = 255;
                rgba[di + 3] = a;
            }
        }
        let g = &mut glyphs[p.idx];
        g.uv = [
            p.x as f32 / ATLAS_W as f32,
            p.y as f32 / height as f32,
            (p.x + p.w) as f32 / ATLAS_W as f32,
            (p.y + p.h) as f32 / height as f32,
        ];
    }
    (AtlasMetrics { glyphs, base_px, line_height }, rgba, ATLAS_W, height)
}

// ============================================================================
// Vertex batch (px → NDC)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

struct Batch {
    v: Vec<Vertex>,
    win: (f32, f32),
}

impl Batch {
    fn new(win: (u32, u32)) -> Batch {
        Batch { v: Vec::new(), win: (win.0.max(1) as f32, win.1.max(1) as f32) }
    }
    fn ndc(&self, x: f32, y: f32) -> [f32; 2] {
        [x / self.win.0 * 2.0 - 1.0, 1.0 - y / self.win.1 * 2.0]
    }
    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], col: [f32; 4]) {
        let p0 = self.ndc(x, y);
        let p1 = self.ndc(x + w, y);
        let p2 = self.ndc(x + w, y + h);
        let p3 = self.ndc(x, y + h);
        let (u0, v0, u1, v1) = (uv[0], uv[1], uv[2], uv[3]);
        self.v.extend_from_slice(&[
            Vertex { pos: p0, uv: [u0, v0], color: col },
            Vertex { pos: p1, uv: [u1, v0], color: col },
            Vertex { pos: p2, uv: [u1, v1], color: col },
            Vertex { pos: p0, uv: [u0, v0], color: col },
            Vertex { pos: p2, uv: [u1, v1], color: col },
            Vertex { pos: p3, uv: [u0, v1], color: col },
        ]);
    }
    fn solid(&mut self, x: f32, y: f32, w: f32, h: f32, col: [f32; 4]) {
        self.quad(x, y, w, h, [0.0, 0.0, 1.0, 1.0], col);
    }
    /// Push glyph quads for `text` left-aligned at `(x, baseline)`. Returns the width.
    fn text(&mut self, m: &AtlasMetrics, text: &str, x: f32, baseline: f32, px: f32, col: [f32; 4]) -> f32 {
        let s = px / m.base_px;
        let mut pen = x;
        for ch in text.chars() {
            let g = m.glyph(ch);
            if g.size[0] > 0.0 {
                self.quad(pen + g.min[0] * s, baseline + g.min[1] * s, g.size[0] * s, g.size[1] * s, g.uv, col);
            }
            pen += g.advance * s;
        }
        pen - x
    }
    /// Text with a 1px-ish dark drop shadow for legibility over bright content.
    fn text_sh(&mut self, m: &AtlasMetrics, text: &str, x: f32, baseline: f32, px: f32, col: [f32; 4]) -> f32 {
        let sh = (px * 0.06).max(1.0);
        self.text(m, text, x + sh, baseline + sh, px, [0.0, 0.0, 0.0, col[3] * 0.75]);
        self.text(m, text, x, baseline, px, col)
    }
    /// One flat triangle (screen-space; the 1×1-white bind samples uv 0,0).
    fn tri(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], col: [f32; 4]) {
        self.v.push(Vertex { pos: self.ndc(a[0], a[1]), uv: [0.0, 0.0], color: col });
        self.v.push(Vertex { pos: self.ndc(b[0], b[1]), uv: [0.0, 0.0], color: col });
        self.v.push(Vertex { pos: self.ndc(c[0], c[1]), uv: [0.0, 0.0], color: col });
    }
    /// A filled quarter-disc fan centred at `(cx, cy)` sweeping `a0→a1` (radians;
    /// screen y grows downward). `seg` triangles approximate the arc.
    fn corner_fan(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, col: [f32; 4], seg: usize) {
        let mut prev = [cx + r * a0.cos(), cy + r * a0.sin()];
        for i in 1..=seg {
            let t = a0 + (a1 - a0) * (i as f32 / seg as f32);
            let cur = [cx + r * t.cos(), cy + r * t.sin()];
            self.tri([cx, cy], prev, cur, col);
            prev = cur;
        }
    }
    /// A filled rounded rectangle: a middle band + top/bottom bands + four
    /// quarter-disc corners (`cull_mode = None`, so winding is irrelevant). `r` is
    /// the corner radius in px, clamped to half the smaller side; `r < 0.75` → a
    /// plain rect.
    fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, col: [f32; 4]) {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let r = r.max(0.0).min(w * 0.5).min(h * 0.5);
        if r < 0.75 {
            self.solid(x, y, w, h, col);
            return;
        }
        self.solid(x, y + r, w, h - 2.0 * r, col); // full-width middle
        self.solid(x + r, y, w - 2.0 * r, r, col); // top band
        self.solid(x + r, y + h - r, w - 2.0 * r, r, col); // bottom band
        let seg = ((r * 0.6) as usize).clamp(3, 20);
        self.corner_fan(x + r, y + r, r, PI, PI * 1.5, col, seg); // top-left
        self.corner_fan(x + w - r, y + r, r, PI * 1.5, TAU, col, seg); // top-right
        self.corner_fan(x + w - r, y + h - r, r, 0.0, FRAC_PI_2, col, seg); // bottom-right
        self.corner_fan(x + r, y + h - r, r, FRAC_PI_2, PI, col, seg); // bottom-left
    }
}

// ============================================================================
// GPU overlay
// ============================================================================

struct AtlasTex {
    metrics: AtlasMetrics,
    bind: wgpu::BindGroup,
}

struct FormulaTex {
    bind: wgpu::BindGroup,
    dims: (u32, u32),
}

/// Per-frame style pulled from `Shared.overlay` + the string sidecar.
pub struct OverlayStyle {
    pub opacity: f32,
    pub scale: f32,
    pub show_title: bool,
    pub show_desc: bool,
    pub show_formula: bool,
    pub show_readouts: bool,
    pub show_handle: bool,
    pub panel: [f32; 4], // rgb + alpha
    pub text: [f32; 3],
    pub handle: String,
    pub title_override: Option<String>,
}

pub struct Overlay {
    sampler: wgpu::Sampler,
    bgl: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipeline: wgpu::RenderPipeline,
    pipeline_format: wgpu::TextureFormat,
    white: wgpu::BindGroup,
    atlas: [AtlasTex; 2], // 0 = regular, 1 = bold
    // Sized from the bundled-PNG table so a new generator's formula can't
    // outgrow the cache (indexing is by `formula_index`, which mirrors it).
    formulas: [Option<FormulaTex>; FORMULA_PNGS.len()],
    vbuf: Option<wgpu::Buffer>,
    vcap: u64,
}

const ATLAS_PX: f32 = 96.0; // base rasterization size; quads scale from this

impl Overlay {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Overlay {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay-bgl"),
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
            ],
        });
        let pipeline = make_pipeline(device, &shader, &bgl, format);

        // 1×1 white texture for solid quads.
        let white = make_tex_bind(device, queue, &bgl, &sampler, &[255, 255, 255, 255], 1, 1);

        let mk_atlas = |bytes: &[u8]| {
            let (metrics, rgba, w, h) = rasterize_atlas(bytes, ATLAS_PX);
            let bind = make_tex_bind(device, queue, &bgl, &sampler, &rgba, w, h);
            AtlasTex { metrics, bind }
        };
        let atlas = [mk_atlas(FONT_REGULAR), mk_atlas(FONT_BOLD)];

        Overlay {
            sampler,
            bgl,
            shader,
            pipeline,
            pipeline_format: format,
            white,
            atlas,
            formulas: std::array::from_fn(|_| None),
            vbuf: None,
            vcap: 0,
        }
    }

    fn ensure_formula(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: organon_scene::overlay_meta::FormulaId) {
        let i = formula_index(f);
        if self.formulas[i].is_some() {
            return;
        }
        let img = image::load_from_memory(formula_bytes(f)).expect("formula png").to_rgba8();
        let (w, h) = (img.width(), img.height());
        let bind = make_tex_bind(device, queue, &self.bgl, &self.sampler, &img, w, h);
        self.formulas[i] = Some(FormulaTex { bind, dims: (w, h) });
    }

    /// Draw the overlay into `dst`, laid out inside `rect` (the production fit rect, or
    /// the full window for Native). `surface` is the swapchain size (px → NDC).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        surface: (u32, u32),
        rect: (u32, u32, u32, u32),
        meta: &OverlayMeta,
        vals: &Values,
        st: &OverlayStyle,
    ) {
        if dst_format != self.pipeline_format {
            self.pipeline = make_pipeline(device, &self.shader, &self.bgl, dst_format);
            self.pipeline_format = dst_format;
        }
        if st.show_formula {
            if let Some(f) = meta.formula {
                self.ensure_formula(device, queue, f);
            }
        }

        let op = st.opacity.clamp(0.0, 1.0);
        let (rx, ry, rw, rh) = (rect.0 as f32, rect.1 as f32, rect.2 as f32, rect.3 as f32);
        let base = rh * 0.024 * st.scale.clamp(0.2, 4.0); // base text unit
        let reg = &self.atlas[0].metrics;
        let bold = &self.atlas[1].metrics;
        let textc = [st.text[0], st.text[1], st.text[2], op];

        let mut solids = Batch::new(surface);
        let mut text_reg = Batch::new(surface);
        let mut text_bold = Batch::new(surface);
        let mut formula = Batch::new(surface);

        // --- Title + rules + divider dot ---
        let mut y = ry + rh * 0.05;
        if st.show_title {
            let title = st.title_override.as_deref().filter(|s| !s.is_empty()).unwrap_or(meta.title);
            let px = base * 1.9;
            let w = bold.measure(title, px);
            let cx = rx + rw * 0.5;
            let bl = y + px;
            text_bold.text_sh(bold, title, cx - w * 0.5, bl, px, textc);
            // thin rules above + below, a centred dot
            let rule_w = rw * 0.34;
            let rl = [textc[0], textc[1], textc[2], op * 0.5];
            solids.solid(cx - rule_w * 0.5, y - base * 0.4, rule_w, (base * 0.04).max(1.0), rl);
            solids.solid(cx - rule_w * 0.5, bl + base * 0.35, rule_w, (base * 0.04).max(1.0), rl);
            let dot = (base * 0.16).max(2.0);
            solids.solid(cx - dot * 0.5, bl + base * 0.35 - dot * 0.4, dot, dot, rl);
            y = bl + base * 0.9;
        }

        // --- Description (word-wrapped so it fits narrow/portrait frames) ---
        if st.show_desc && !meta.description.is_empty() {
            let px = base * 0.85;
            let cx = rx + rw * 0.5;
            let lh = px * 1.2;
            let dc = [textc[0], textc[1], textc[2], op * 0.92];
            let lines = reg.wrap(meta.description, px, rw * 0.86);
            y += px; // baseline of the first line
            for (i, line) in lines.iter().enumerate() {
                let bl = y + i as f32 * lh;
                let w = reg.measure(line, px);
                text_reg.text_sh(reg, line, cx - w * 0.5, bl, px, dc);
            }
            y += (lines.len().saturating_sub(1)) as f32 * lh + base * 0.6;
        }

        // --- Formula image ---
        if st.show_formula {
            if let Some(f) = meta.formula {
                if let Some(ft) = &self.formulas[formula_index(f)] {
                    let (tw, th) = (ft.dims.0 as f32, ft.dims.1 as f32);
                    let target_h = (rh * 0.13).min(rw * 0.7 * th / tw);
                    let target_w = target_h * tw / th;
                    let fx = rx + (rw - target_w) * 0.5;
                    let fy = y + base * 0.3;
                    formula.quad(fx, fy, target_w, target_h, [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, op]);
                }
            }
        }

        // --- Readout panel (bottom) ---
        if st.show_readouts && !meta.groups.is_empty() {
            let pw = rw * 0.92;
            let ph = rh * 0.24;
            let px0 = rx + (rw - pw) * 0.5;
            let py0 = ry + rh - ph - rh * 0.04;
            // panel fill + border
            solids.solid(px0, py0, pw, ph, [st.panel[0], st.panel[1], st.panel[2], st.panel[3] * op]);
            let bd = (base * 0.06).max(1.0);
            let bc = [textc[0], textc[1], textc[2], op * 0.4];
            solids.solid(px0, py0, pw, bd, bc);
            solids.solid(px0, py0 + ph - bd, pw, bd, bc);
            solids.solid(px0, py0, bd, ph, bc);
            solids.solid(px0 + pw - bd, py0, bd, ph, bc);

            let row_px = base * 0.82;
            // Live animation ticker (always changing) — a prominent cyan line across the
            // panel top, with a thin divider under it, then the param columns below.
            let live_px = base * 1.05;
            let live = format!("{} = {:.2}", meta.live_label, vals.get(LIVE_SLOT));
            let lw = reg.measure(&live, live_px);
            let live_y = py0 + base * 1.0;
            text_reg.text_sh(reg, &live, px0 + (pw - lw) * 0.5, live_y, live_px, [0.298, 0.788, 0.941, op]);
            solids.solid(px0 + pw * 0.08, live_y + base * 0.28, pw * 0.84, (base * 0.03).max(1.0), bc);
            let cols_top = live_y + base * 0.55;

            let n = meta.groups.len().max(1);
            let col_w = pw / n as f32;
            let pad = pw * 0.03;
            for (gi, grp) in meta.groups.iter().enumerate() {
                let cx = px0 + col_w * gi as f32 + pad;
                let mut ry2 = cols_top + row_px;
                // group header
                text_reg.text_sh(reg, grp.title, cx, ry2, row_px * 0.8, [textc[0], textc[1], textc[2], op * 0.55]);
                ry2 += row_px * 1.3;
                for r in grp.rows {
                    // Scalar → "label = v"; vector (span > 1) → "label [a, b, c]".
                    let (label, val) = if r.span > 1 {
                        let mut s = String::from("[");
                        for k in 0..r.span {
                            if k > 0 {
                                s.push_str(", ");
                            }
                            s.push_str(&r.fmt.apply(vals.get(r.slot + k)));
                        }
                        s.push(']');
                        (format!("{} ", r.label), s)
                    } else {
                        (format!("{} = ", r.label), r.fmt.apply(vals.get(r.slot)))
                    };
                    let lc = match r.color {
                        Some(i) if i < meta.symbols.len() => {
                            let c = meta.symbols[i].color;
                            [c[0], c[1], c[2], op]
                        }
                        _ => textc,
                    };
                    let lw = text_reg.text_sh(reg, &label, cx, ry2, row_px, lc);
                    text_reg.text_sh(reg, &val, cx + lw, ry2, row_px, textc);
                    ry2 += row_px * 1.35;
                }
            }
        }

        // --- Handle / watermark (bottom-right) ---
        if st.show_handle && !st.handle.is_empty() {
            let px = base * 0.72;
            let w = reg.measure(&st.handle, px);
            let hx = rx + rw - w - rw * 0.03;
            let hy = ry + rh - rh * 0.012;
            text_reg.text(reg, &st.handle, hx, hy, px, [textc[0], textc[1], textc[2], op * 0.45]);
        }

        // --- Upload + draw (one pass; painter's order: solids, formula, text) ---
        let mut all: Vec<Vertex> = Vec::with_capacity(solids.v.len() + formula.v.len() + text_reg.v.len() + text_bold.v.len());
        let mut ranges: Vec<(&wgpu::BindGroup, u32, u32)> = Vec::new();

        // Build ranges explicitly (one draw per texture, in painter's order).
        let start_solid = all.len() as u32;
        all.extend_from_slice(&solids.v);
        if !solids.v.is_empty() {
            ranges.push((&self.white, start_solid, solids.v.len() as u32));
        }
        let start_formula = all.len() as u32;
        all.extend_from_slice(&formula.v);
        if !formula.v.is_empty() {
            if let Some(f) = meta.formula {
                if let Some(ft) = &self.formulas[formula_index(f)] {
                    ranges.push((&ft.bind, start_formula, formula.v.len() as u32));
                }
            }
        }
        let start_reg = all.len() as u32;
        all.extend_from_slice(&text_reg.v);
        if !text_reg.v.is_empty() {
            ranges.push((&self.atlas[0].bind, start_reg, text_reg.v.len() as u32));
        }
        let start_bold = all.len() as u32;
        all.extend_from_slice(&text_bold.v);
        if !text_bold.v.is_empty() {
            ranges.push((&self.atlas[1].bind, start_bold, text_bold.v.len() as u32));
        }
        if all.is_empty() {
            return;
        }

        let bytes = bytemuck::cast_slice(&all);
        let need = bytes.len() as u64;
        if self.vbuf.is_none() || self.vcap < need {
            self.vcap = need.next_power_of_two().max(4096);
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay-vbuf"),
                size: self.vcap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vbuf = self.vbuf.as_ref().unwrap();
        queue.write_buffer(vbuf, 0, bytes);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay-pass") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, vbuf.slice(..));
            for (bind, start, count) in ranges {
                rp.set_bind_group(0, bind, &[]);
                rp.draw(start..start + count, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Draw free-floating text markers at given screen-pixel positions (centred), used for
    /// the projected 3-D axis labels (#135 P5). Reuses the glyph atlas + text pipeline.
    pub fn draw_markers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        surface: (u32, u32),
        markers: &[(f32, f32, &str, [f32; 4])],
        px: f32,
    ) {
        if markers.is_empty() {
            return;
        }
        if dst_format != self.pipeline_format {
            self.pipeline = make_pipeline(device, &self.shader, &self.bgl, dst_format);
            self.pipeline_format = dst_format;
        }
        let reg = &self.atlas[0].metrics;
        let mut batch = Batch::new(surface);
        for (mx, my, text, col) in markers {
            let w = reg.measure(text, px);
            batch.text_sh(reg, text, mx - w * 0.5, *my + px * 0.5, px, *col);
        }
        if batch.v.is_empty() {
            return;
        }
        let bytes = bytemuck::cast_slice(&batch.v);
        let need = bytes.len() as u64;
        if self.vbuf.is_none() || self.vcap < need {
            self.vcap = need.next_power_of_two().max(4096);
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay-vbuf"),
                size: self.vcap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vbuf = self.vbuf.as_ref().unwrap();
        queue.write_buffer(vbuf, 0, bytes);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay-markers") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay-markers-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_bind_group(0, &self.atlas[0].bind, &[]);
            rp.draw(0..batch.v.len() as u32, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Left-aligned multi-line text HUD at `(x, y0)` in pixels (#333 calibrated
    /// meter readout). Each line carries its own colour; drawn with a drop shadow
    /// for legibility over the scene. Self-contained pass (mirrors `draw_markers`).
    pub fn draw_hud(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        surface: (u32, u32),
        lines: &[(String, [f32; 4])],
        x: f32,
        y0: f32,
        px: f32,
    ) {
        if lines.is_empty() {
            return;
        }
        if dst_format != self.pipeline_format {
            self.pipeline = make_pipeline(device, &self.shader, &self.bgl, dst_format);
            self.pipeline_format = dst_format;
        }
        let reg = &self.atlas[0].metrics;
        let mut batch = Batch::new(surface);
        let lh = px * 1.35;
        let mut y = y0;
        for (text, col) in lines {
            batch.text_sh(reg, text, x, y + px, px, *col);
            y += lh;
        }
        if batch.v.is_empty() {
            return;
        }
        let bytes = bytemuck::cast_slice(&batch.v);
        let need = bytes.len() as u64;
        if self.vbuf.is_none() || self.vcap < need {
            self.vcap = need.next_power_of_two().max(4096);
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay-vbuf"),
                size: self.vcap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vbuf = self.vbuf.as_ref().unwrap();
        queue.write_buffer(vbuf, 0, bytes);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay-hud") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay-hud-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_bind_group(0, &self.atlas[0].bind, &[]);
            rp.draw(0..batch.v.len() as u32, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Upload + draw one `Batch` in a self-contained alpha-blended pass. `atlas` picks
    /// the bind: `true` = the glyph atlas (text), `false` = the 1×1 white texture
    /// (solid geometry). Queue writes + submits are ordered, so callers may flush a
    /// panel batch (white) then a text batch (atlas) reusing the same vertex buffer.
    fn flush_batch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        batch: &Batch,
        atlas: bool,
    ) {
        if batch.v.is_empty() {
            return;
        }
        if dst_format != self.pipeline_format {
            self.pipeline = make_pipeline(device, &self.shader, &self.bgl, dst_format);
            self.pipeline_format = dst_format;
        }
        let bytes = bytemuck::cast_slice(&batch.v);
        let need = bytes.len() as u64;
        if self.vbuf.is_none() || self.vcap < need {
            self.vcap = need.next_power_of_two().max(4096);
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay-vbuf"),
                size: self.vcap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vbuf = self.vbuf.as_ref().unwrap();
        queue.write_buffer(vbuf, 0, bytes);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay-panel") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay-panel-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_bind_group(0, if atlas { &self.atlas[0].bind } else { &self.white }, &[]);
            rp.draw(0..batch.v.len() as u32, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Draw a HUD block on a **rounded-rectangle backing panel** (#391 Tier 1
    /// presentation): the panel gives the text real contrast over the render, and the
    /// whole thing docks to a corner of `area` (the letterbox rect). `dock`: 0 = top-
    /// left, 1 = bottom-left, 2 = top-right, 3 = bottom-right. `px` is the (already
    /// scaled) font height; `bg` the panel rgba (opacity baked in); `bevel` ∈ [0,1] the
    /// corner-radius as a fraction of the panel's half-min-side (0 = square, 1 = pill);
    /// `margin` the gap from the area edge; `stack_lines` an extra offset (in text
    /// lines) from the docked edge so the panel can sit clear of other HUDs. The panel
    /// (solid) and the text draw in two ordered sub-passes (different binds).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_hud_panel(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        surface: (u32, u32),
        area: (f32, f32, f32, f32),
        lines: &[(String, [f32; 4])],
        dock: u32,
        px: f32,
        bg: [f32; 4],
        bevel: f32,
        margin: f32,
        stack_lines: f32,
    ) {
        if lines.is_empty() {
            return;
        }
        let lh = px * 1.35;
        let pad = (px * 0.55).max(4.0);
        // Measure each line up front (drops the immutable atlas borrow before we draw).
        let widths: Vec<f32> = {
            let reg = &self.atlas[0].metrics;
            lines.iter().map(|(t, _)| reg.measure(t, px)).collect()
        };
        let text_w = widths.iter().copied().fold(0.0_f32, f32::max);
        let n = lines.len() as f32;
        let panel_w = text_w + 2.0 * pad;
        let panel_h = n * lh + 2.0 * pad;
        let (ax, ay, aw, ah) = area;
        let stack = stack_lines * lh;
        let right = matches!(dock, 2 | 3);
        let bottom = matches!(dock, 1 | 3);
        let panel_x = if right { ax + aw - margin - panel_w } else { ax + margin };
        let panel_y = if bottom { ay + ah - margin - panel_h - stack } else { ay + margin + stack };
        let corner = bevel.clamp(0.0, 1.0) * 0.5 * panel_w.min(panel_h);

        // Pass 1 — the rounded backing panel (solid geometry → white bind).
        let mut pb = Batch::new(surface);
        pb.rounded_rect(panel_x, panel_y, panel_w, panel_h, corner, bg);
        self.flush_batch(device, queue, dst, dst_format, &pb, false);

        // Pass 2 — the text (glyph atlas bind), left- or right-aligned to the panel.
        let mut tb = Batch::new(surface);
        {
            let reg = &self.atlas[0].metrics;
            let mut y = panel_y + pad;
            for ((text, col), w) in lines.iter().zip(&widths) {
                let x = if right { panel_x + panel_w - pad - w } else { panel_x + pad };
                tb.text_sh(reg, text, x, y + px, px, *col);
                y += lh;
            }
        }
        self.flush_batch(device, queue, dst, dst_format, &tb, true);
    }

    /// Draw the #380 Density-Map Attractor **parameter-space inset** — the source
    /// image's `(a, b)` orbit plot ("you are here in chaos-space"). A small panel
    /// at `rect` (x, y, w, h pixels) with the closed trajectory (`traj`, normalized
    /// to `[0,1]²`) as faint dots and the live current point (`cur`) as a bright
    /// marker. Solid-quad only (uses the 1×1 white bind), a self-contained pass
    /// mirroring `draw_hud`. `opacity` scales the whole inset (0 → nothing drawn).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_param_plot(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        surface: (u32, u32),
        rect: (f32, f32, f32, f32),
        traj: &[(f32, f32)],
        cur: (f32, f32),
        opacity: f32,
    ) {
        if opacity <= 0.001 {
            return;
        }
        if dst_format != self.pipeline_format {
            self.pipeline = make_pipeline(device, &self.shader, &self.bgl, dst_format);
            self.pipeline_format = dst_format;
        }
        let (rx, ry, rw, rh) = rect;
        let op = opacity.clamp(0.0, 1.0);
        // Map a normalized (a,b) in [0,1]² to a pixel position inside the plot area
        // (a small inset margin; y flipped so +b points up like a real plot).
        let m = (rw.min(rh) * 0.08).clamp(2.0, 12.0); // inner margin
        let px0 = rx + m;
        let py0 = ry + m;
        let pw = (rw - 2.0 * m).max(1.0);
        let ph = (rh - 2.0 * m).max(1.0);
        let to_px = |n: (f32, f32)| -> (f32, f32) {
            let nx = n.0.clamp(0.0, 1.0);
            let ny = n.1.clamp(0.0, 1.0);
            (px0 + nx * pw, py0 + (1.0 - ny) * ph)
        };

        let mut batch = Batch::new(surface);
        // Panel background + a thin frame.
        batch.solid(rx, ry, rw, rh, [0.02, 0.02, 0.04, 0.62 * op]);
        let bw = 1.0f32.max(rw * 0.006);
        let frame = [0.55, 0.6, 0.75, 0.5 * op];
        batch.solid(rx, ry, rw, bw, frame);
        batch.solid(rx, ry + rh - bw, rw, bw, frame);
        batch.solid(rx, ry, bw, rh, frame);
        batch.solid(rx + rw - bw, ry, bw, rh, frame);
        // Centre crosshair (the (a,b) = centre of the swept box).
        let cx = px0 + pw * 0.5;
        let cy = py0 + ph * 0.5;
        let ch = [0.4, 0.44, 0.55, 0.35 * op];
        batch.solid(px0, cy - bw * 0.5, pw, bw, ch);
        batch.solid(cx - bw * 0.5, py0, bw, ph, ch);
        // The closed trajectory: faint cyan dots.
        let ds = (rw.min(rh) * 0.012).clamp(1.0, 3.0);
        let tcol = [0.35, 0.75, 0.95, 0.5 * op];
        for &n in traj {
            let (x, y) = to_px(n);
            batch.solid(x - ds * 0.5, y - ds * 0.5, ds, ds, tcol);
        }
        // The live current point: a bright warm marker.
        let (x, y) = to_px(cur);
        let cs = (rw.min(rh) * 0.05).clamp(3.0, 9.0);
        batch.solid(x - cs * 0.5, y - cs * 0.5, cs, cs, [1.0, 0.85, 0.35, 0.95 * op]);

        if batch.v.is_empty() {
            return;
        }
        let bytes = bytemuck::cast_slice(&batch.v);
        let need = bytes.len() as u64;
        if self.vbuf.is_none() || self.vcap < need {
            self.vcap = need.next_power_of_two().max(4096);
            self.vbuf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("overlay-vbuf"),
                size: self.vcap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vbuf = self.vbuf.as_ref().unwrap();
        queue.write_buffer(vbuf, 0, bytes);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay-plot") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay-plot-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_bind_group(0, &self.white, &[]);
            rp.draw(0..batch.v.len() as u32, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// #423 Tier 1 — the **roofline inset**: a log-log plot of operational intensity
    /// (X) vs achievable FLOP/s (Y) against a hardware profile's own ceiling, each scanned
    /// model a dot sitting ON the ceiling at its OI, coloured by quant family. The
    /// memory-bound region (left of the ridge) and compute-bound region (right) are
    /// shaded so you can see which regime each model lives in — the brief's "roofline
    /// stops being a diagram and becomes a place." Geometry draws on the white bind,
    /// then axis/region labels on the atlas bind (the `draw_hud_panel` two-pass).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_roofline(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        dst: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
        surface: (u32, u32),
        rect: (f32, f32, f32, f32),
        plot: &organon_core::math::RooflinePlot,
        profile_name: &str,
        opacity: f32,
    ) {
        if opacity <= 0.001 {
            return;
        }
        let (rx, ry, rw, rh) = rect;
        let op = opacity.clamp(0.0, 1.0);
        let m = (rw.min(rh) * 0.10).clamp(4.0, 18.0); // inner margin (room for labels)
        let px0 = rx + m;
        let py0 = ry + m;
        let pw = (rw - 2.0 * m).max(1.0);
        let ph = (rh - 2.0 * m).max(1.0);
        let to_px = |nx: f32, ny: f32| -> (f32, f32) {
            (px0 + nx.clamp(0.0, 1.0) * pw, py0 + (1.0 - ny.clamp(0.0, 1.0)) * ph)
        };

        // ── Pass 1: geometry (white bind) ──
        let mut b = Batch::new(surface);
        // Panel background + frame.
        b.solid(rx, ry, rw, rh, [0.02, 0.02, 0.04, 0.62 * op]);
        let bw = 1.0f32.max(rw * 0.006);
        let frame = [0.55, 0.6, 0.75, 0.5 * op];
        b.solid(rx, ry, rw, bw, frame);
        b.solid(rx, ry + rh - bw, rw, bw, frame);
        b.solid(rx, ry, bw, rh, frame);
        b.solid(rx + rw - bw, ry, bw, rh, frame);

        // Region shading: memory-bound (left of ridge, cool) vs compute-bound (right, warm).
        let ridge = plot.ridge_x.clamp(0.0, 1.0);
        let ridge_px = px0 + ridge * pw;
        b.solid(px0, py0, (ridge_px - px0).max(0.0), ph, [0.10, 0.22, 0.42, 0.30 * op]);
        b.solid(ridge_px, py0, (px0 + pw - ridge_px).max(0.0), ph, [0.40, 0.24, 0.12, 0.30 * op]);
        // Ridge divider.
        b.solid(ridge_px - bw * 0.5, py0, bw, ph, [0.8, 0.8, 0.85, 0.5 * op]);

        // The roofline ceiling as a bright polyline.
        let ceil_col = [0.85, 0.9, 1.0, 0.85 * op];
        let th = (rw.min(rh) * 0.010).clamp(1.5, 4.0);
        for w in plot.ceiling.windows(2) {
            let p0 = to_px(w[0].0, w[0].1);
            let p1 = to_px(w[1].0, w[1].1);
            thick_line(&mut b, p0, p1, th, ceil_col);
        }

        // Model dots, coloured by quant family.
        let ds = (rw.min(rh) * 0.045).clamp(4.0, 12.0);
        for p in &plot.points {
            let (x, y) = to_px(p.x, p.y);
            let c3 = quant_color(p.quant_ordinal);
            b.solid(x - ds * 0.5, y - ds * 0.5, ds, ds, [c3[0], c3[1], c3[2], 0.95 * op]);
        }
        self.flush_batch(device, queue, dst, dst_format, &b, false);

        // ── Pass 2: labels (atlas bind) ──
        let lpx = (rw.min(rh) * 0.075).clamp(9.0, 16.0);
        let mut tb = Batch::new(surface);
        {
            let reg = &self.atlas[0].metrics;
            let title = format!("roofline ~ {profile_name}");
            tb.text_sh(reg, &title, rx + m * 0.5, ry + lpx + 2.0, lpx, [0.85, 0.9, 1.0, 0.95 * op]);
            // Axis hints.
            tb.text_sh(reg, "FLOP/s", rx + m * 0.5, py0 + lpx, lpx * 0.9, [0.6, 0.66, 0.8, 0.85 * op]);
            let oiw = reg.measure("OI ->", lpx * 0.9);
            tb.text_sh(reg, "OI ->", rx + rw - oiw - m * 0.5, ry + rh - m * 0.4, lpx * 0.9, [0.6, 0.66, 0.8, 0.85 * op]);
            // Region tags near the ridge.
            tb.text_sh(reg, "mem", px0 + 2.0, ry + rh - m * 0.4, lpx * 0.85, [0.55, 0.7, 0.95, 0.8 * op]);
            let cw = reg.measure("cmp", lpx * 0.85);
            tb.text_sh(reg, "cmp", px0 + pw - cw - 2.0, ry + lpx * 2.2, lpx * 0.85, [0.95, 0.7, 0.5, 0.8 * op]);
        }
        self.flush_batch(device, queue, dst, dst_format, &tb, true);
    }
}

/// A thick line segment between two pixel points, as two triangles (the overlay has
/// no line primitive — draw_param_plot only needs axis-aligned solids, but the
/// roofline ceiling is sloped). Perpendicular half-width `th/2`.
fn thick_line(b: &mut Batch, p0: (f32, f32), p1: (f32, f32), th: f32, col: [f32; 4]) {
    let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
    let len = (dx * dx + dy * dy).sqrt().max(1.0e-4);
    let (nx, ny) = (-dy / len * th * 0.5, dx / len * th * 0.5);
    let a = [p0.0 + nx, p0.1 + ny];
    let bb = [p0.0 - nx, p0.1 - ny];
    let c = [p1.0 - nx, p1.1 - ny];
    let d = [p1.0 + nx, p1.1 + ny];
    b.tri(a, bb, c, col);
    b.tri(a, c, d, col);
}

/// Quant-family colour for the roofline dots + constellation legend: a ladder from
/// cool (Full/high-precision) to hot (Q1/most-compressed), grey for Other. Ordinals
/// match `gguf::QuantFamily::ordinal` (0 Full … 7 Q1, 8 Other).
fn quant_color(ord: u32) -> [f32; 3] {
    match ord {
        0 => [0.30, 0.85, 0.90], // Full — cyan
        1 => [0.35, 0.85, 0.55], // Q8   — green
        2 => [0.60, 0.85, 0.40], // Q6   — chartreuse
        3 => [0.85, 0.88, 0.40], // Q5   — yellow-green
        4 => [0.95, 0.82, 0.35], // Q4   — yellow
        5 => [0.96, 0.62, 0.30], // Q3   — orange
        6 => [0.95, 0.45, 0.32], // Q2   — red-orange
        7 => [0.92, 0.30, 0.34], // Q1   — red
        _ => [0.55, 0.55, 0.60], // Other — grey
    }
}

fn make_tex_bind(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    rgba: &[u8],
    w: u32,
    h: u32,
) -> wgpu::BindGroup {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overlay-tex"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("overlay-bind"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn make_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    bgl: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("overlay-pl"),
        bind_group_layouts: &[Some(bgl)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("overlay-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16, shader_location: 2 },
                ],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
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
    fn every_formula_id_indexes_a_bundled_png() {
        // Regression (#175 Bugbot): the formula cache is sized from
        // FORMULA_PNGS, so every FormulaId must index inside it — and its PNG
        // must decode (each is drawn via `ensure_formula` at runtime).
        use organon_scene::overlay_meta::FormulaId::*;
        let all = [
            Original, Frenet, Dna, Harmonic, Minimal, Attractor, LSystem, CurlNoise,
            Polarization, Maxwell, Phyllotaxis, Mandelbulb, Kifs, Boids, Tessellation,
            Synchrotron, VectorField, Rails, Axon,
        ];
        assert_eq!(all.len(), FORMULA_PNGS.len(), "FormulaId count vs bundled PNGs");
        for f in all {
            let i = formula_index(f);
            assert!(i < FORMULA_PNGS.len(), "{f:?} indexes past the cache");
            let img = image::load_from_memory(formula_bytes(f))
                .unwrap_or_else(|e| panic!("{f:?} png does not decode: {e}"));
            assert!(img.width() > 0 && img.height() > 0);
        }
    }

    #[test]
    fn atlas_rasterizes_ascii() {
        let (m, rgba, w, h) = rasterize_atlas(FONT_REGULAR, 64.0);
        assert!(w == ATLAS_W && h > 0);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        // 'x' has an outline + positive advance; space has advance but no glyph box.
        assert!(m.glyph('x').advance > 0.0);
        assert!(m.glyph('x').size[0] > 0.0);
        assert!(m.glyph(' ').advance > 0.0);
        assert_eq!(m.glyph(' ').size, [0.0, 0.0]);
    }

    #[test]
    fn measure_scales_and_is_monotonic() {
        let (m, _, _, _) = rasterize_atlas(FONT_REGULAR, 64.0);
        let a = m.measure("xx", 32.0);
        let b = m.measure("xxxx", 32.0);
        assert!(b > a && a > 0.0);
        // measuring at 2× px ≈ 2× width
        let one = m.measure("organic", 30.0);
        let two = m.measure("organic", 60.0);
        assert!((two / one - 2.0).abs() < 0.01);
    }

    #[test]
    fn wrap_breaks_into_fitting_lines() {
        let (m, _, _, _) = rasterize_atlas(FONT_REGULAR, 64.0);
        let text = "The original cube field rotate then translate compounded into strands";
        let full = m.measure(text, 32.0);
        let max = full * 0.45; // force ≥ 2 lines
        let lines = m.wrap(text, 32.0, max);
        assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());
        for l in &lines {
            assert!(m.measure(l, 32.0) <= max + 0.01 || !l.contains(' '), "line too wide: {l:?}");
        }
        // recombining the words round-trips the content
        assert_eq!(lines.join(" ").split_whitespace().count(), text.split_whitespace().count());
        // a wide budget keeps it on one line
        assert_eq!(m.wrap(text, 32.0, full * 2.0).len(), 1);
    }

    #[test]
    fn ndc_maps_corners() {
        let b = Batch::new((100, 200));
        assert_eq!(b.ndc(0.0, 0.0), [-1.0, 1.0]);
        assert_eq!(b.ndc(100.0, 200.0), [1.0, -1.0]);
        assert_eq!(b.ndc(50.0, 100.0), [0.0, 0.0]);
    }

    #[test]
    fn bold_atlas_also_builds() {
        let (m, _, _, _) = rasterize_atlas(FONT_BOLD, 64.0);
        assert!(m.glyph('M').advance > 0.0 && m.line_height > 0.0);
    }
}

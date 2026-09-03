//! Raw-wgpu renderer for the cube field, targeting a window surface with its own
//! depth buffer. Used by the separate visual binary. Geometry/shading are the
//! RGB color-cube + hemisphere/key/fill lighting from the web app.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

// IBL precompute + HDR post-processing live next to this file and are referenced
// ONLY here, so they compile solely into the visual binary (render.rs is not a
// cdylib module).
#[path = "env.rs"]
mod env;
use env::Environment;
pub use env::AtmosphereParams; // #100: built visual-side, passed to load_atmosphere
#[path = "post.rs"]
mod post;
use post::Post;
pub use post::{PostParams, SsaoParams, SsgiParams, SsrParams};
#[path = "metaball.rs"]
mod metaball;
use metaball::MetaField;
pub use metaball::{MetaNode, MetaballParams, FIELD_RES};
#[path = "fx.rs"]
mod fx;
use fx::Fx;
pub use fx::FxParams;
#[path = "temporal.rs"]
mod temporal;
use temporal::Temporal;
pub use temporal::TemporalParams;
#[path = "voxel.rs"]
mod voxel;
use voxel::VoxField;
pub use voxel::VoxelParams;
#[path = "mandelbulb.rs"]
mod mandelbulb;
use mandelbulb::MandelField;
pub use mandelbulb::MandelParams;
#[path = "creature.rs"]
mod creature;
use creature::CreatureField;
pub use creature::CreatureParams;
#[path = "creature_overlay.rs"]
mod creature_overlay;
use creature_overlay::CreatureOverlay;
#[path = "neural.rs"]
mod neural;
use neural::NeuralField;
pub use neural::NeuralFieldParams;
#[path = "minimal.rs"]
mod minimal;
use minimal::MinimalField;
pub use minimal::MinimalParams;
#[path = "lens.rs"]
mod lens;
use lens::LensField;
pub use lens::LensParams;
#[path = "kifs.rs"]
mod kifs;
use kifs::KifsField;
pub use kifs::KifsParams;
#[path = "kaleido.rs"]
mod kaleido;
use kaleido::Kaleido;
pub use kaleido::KaleidoParams;
#[path = "rd.rs"]
mod rd;
use rd::RdField;
pub use rd::RdParams;
#[path = "terrain.rs"]
mod terrain;
use terrain::Terrain;
#[path = "ocean.rs"]
mod ocean;
use ocean::Ocean;
pub use ocean::OceanParams; // #102B: built visual-side, fed to update_ocean
#[path = "particles.rs"]
mod particles;
use particles::{ArmInstance, ParticleShade, ParticleSystem};
pub use particles::{ArmInstance as MembraneArmInstance, ParticlesFrame, PlexMat};
#[path = "splat.rs"]
mod splat;
use splat::{SplatFrame, SplatSystem};
pub use splat::SplatParams;
#[path = "fluid.rs"]
mod fluid;
use fluid::FluidSim;
pub use fluid::{DyeParams, FluidParams};
#[path = "fluidvis.rs"]
mod fluidvis;
use fluidvis::FluidVis;
pub use fluidvis::InkParams;
#[path = "liquid.rs"]
mod liquid;
#[path = "fluidlight.rs"]
mod fluidlight;
#[path = "rt_shadow.rs"]
mod rt_shadow;
pub use rt_shadow::RtShadowFrame;
#[path = "rt_reflect.rs"]
mod rt_reflect;
pub use rt_reflect::RtReflectFrame;
#[path = "rt_ao.rs"]
mod rt_ao;
pub use rt_ao::RtAoFrame;
#[path = "rt_gi.rs"]
mod rt_gi;
pub use rt_gi::RtGiFrame;
#[path = "rt_pathtrace.rs"]
mod rt_pathtrace;
pub use rt_pathtrace::PathtraceFrame;
#[path = "rt_caustic.rs"]
mod rt_caustic;
pub use post::RtBuffer;

/// Beat-aware temporal accumulator inputs (#200 Tier 4½ parts 3 + 4), threaded
/// through `LightTransport.rt_temporal`. All owned (no borrow).
#[derive(Clone, Copy)]
pub struct RtTemporalFrame {
    /// This frame's **unjittered** scene view-proj (NOT `uniforms.view_proj`,
    /// which carries the TAA Halton jitter when TAA is on). Its inverse
    /// reconstructs world pos for reprojection, and it must match the also-
    /// unjittered `prev_view_proj` or the history wobbles sub-pixel (#213 review).
    pub cur_view_proj: [[f32; 4]; 4],
    /// Last frame's (unjittered) scene view-proj, for camera reprojection.
    pub prev_view_proj: [[f32; 4]; 4],
    /// History weight (exponential-moving-average feedback, 0..0.98). In the
    /// variance path (part 4) this is the adaptive ceiling.
    pub feedback: f32,
    /// Precomputed `beat_pulse · beat_relax_amount` (0..1): how much this
    /// frame's PLL beat kick drops the history weight (the visual folds the
    /// live beat envelope in, so the shader stays stateless).
    pub beat_relax_factor: f32,
    /// Part 4: variance-guided SVGF (history-length-adaptive blend + luminance
    /// σ-clamp). `false` reproduces part 3 (fixed feedback + box clamp).
    pub variance: bool,
    /// Part 4: max accumulated-sample count the adaptive blend ramps to.
    pub max_accum: f32,
    /// Part 4: σ-clamp width γ (history luma clamped to μ ± γσ).
    pub clamp_gamma: f32,
}

/// Neural denoiser inputs (#200 Tier 5a), threaded through
/// `LightTransport.rt_ndenoise`. When present, the RT denoise step uses the
/// kernel-predicting neural filter; `net = 0` reproduces the classical à-trous.
#[derive(Clone, Copy)]
pub struct NDenoiseFrame {
    /// Network influence (0..1): the strength of the seeded-MLP modulation on the
    /// classical bilateral tap weights. 0 = the classical filter, byte-for-byte.
    pub net: f32,
    /// Filter-network seed (regenerates the whole weight set inline).
    pub seed: f32,
    /// SIREN feature scale ω (the network's first-layer frequency).
    pub omega: f32,
}

#[path = "sway.rs"]
mod sway;
#[path = "liquidsurf.rs"]
mod liquidsurf;
#[path = "refractsurf.rs"]
mod refractsurf;
use liquid::LiquidSim;
pub use liquid::{LiquidParams, MAX_LIQUID_PARTICLES};
#[path = "gi.rs"]
mod gi;
use gi::GiVolume;
#[path = "shadow.rs"]
mod shadow;
use shadow::Shadow;
#[path = "vxgi.rs"]
mod vxgi;
use vxgi::Vxgi;
pub use vxgi::VxgiParams;
// Re-exported so the visual binary can build terrain uniforms + synthesize noise
// (the CPU heightfield rides the synthetic fly-camera above the landscape).
pub use terrain::{gen_noise as terrain_gen_noise, terrain_height, TerrainUniforms};
#[path = "stars.rs"]
mod stars;
use stars::Stars;
// Re-exported so the visual binary can build the per-frame star uniforms (the
// equatorial→world rotation, night factor, sun direction).
pub use stars::StarUniforms;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Uniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4], // xyz = camera world pos
    pub mat: [f32; 4],        // metallic, roughness, glow, prefilter_mip_count
    pub env: [f32; 4],        // exposure, env_intensity, env_rotation_rad, opacity
    pub key_light: [f32; 4],  // xyz = world dir TO key light (unit), w = intensity
    pub fill_light: [f32; 4], // xyz = world dir TO fill light (unit), w = intensity
    pub amb: [f32; 4],        // x = ambient/IBL mult, y = material_type, z = glass IOR, w reserved
    pub sss: [f32; 4],        // translucency: x = amount, y = distortion, z = power, w reserved
    pub irid: [f32; 4],       // iridescence: x = amount, y = scale, z = hue shift, w reserved
    pub env_tint: [f32; 4],   // xyz = environment/IBL tint colour (white = none), w unused
    // Bioluminescent emissive ripple (free-running travelling HDR band).
    pub ripple: [f32; 4],      // x = intensity, y = phase, z = freq, w = sharpness
    pub ripple_ctr: [f32; 4],  // xyz = field centre (world), w = field radius
    pub ripple_mode: [f32; 4], // x = geom (0 radial / 1 axial), yzw = axial axis dir
    // Spectral glass (#80 Part C): x = dispersion, y = caustic, z = thin_film,
    // w = spectral_samples. All 0 → today's single-IOR glass.
    pub glassx: [f32; 4],
    // Reflection controls (#163 Tier 1): x = reflect_tint (palette influence on the
    // reflection), y = chrome_purity, z = glass_clarity, w = f0_override. All 0 →
    // today's chrome/glass/standard look byte-identical.
    pub reflect_ctl: [f32; 4],
    // Reflection probe / parallax (#163 Tier 2). The visual fills these from the live
    // field AABB (scaled by the box params) at the uniform-patch site, since only
    // `render()`/the patch have the bounds. xyz = box min / max (world);
    // refl_box_min.w = source_id (0 EnvOnly = off), refl_box_max.w = parallax blend.
    // source_id 0 → the cube shader ignores them → today's reflection.
    pub refl_box_min: [f32; 4],
    pub refl_box_max: [f32; 4],
    // Refractive material: x = Beer–Lambert absorption strength (σ scale; only
    // read when material_type = 3), yzw reserved. Appended — the sibling shaders
    // (metaball/minimal/voxel/mandelbulb) declare the shorter prefix struct,
    // which wgpu allows against the larger bound buffer.
    pub refr: [f32; 4],
    // Anisotropy (#214 Tier 1): x = amount (−1..1), y = brush rotation (rad),
    // z = overlay enable (0/1), w = overlay blend (0..1). All 0 → isotropic.
    // Appended after `refr`; the sibling raymarch shaders keep their shorter
    // prefix struct, and `rt_reflect.wgsl` mirrors up to here to reflect brushed
    // metal as brushed metal.
    pub aniso: [f32; 4],
    // Surface lobes (#214 Tier 2): clearcoat + sheen. `coat` = [clearcoat strength,
    // clearcoat roughness, clearcoat overlay enable, sheen overlay enable];
    // `sheen` = [sheen amount, sheen roughness, sheen tint (white→albedo), _].
    // All 0 → today's look. Appended after `aniso` (siblings/rt keep their prefix).
    pub coat: [f32; 4],
    pub sheen: [f32; 4],
    // Body optics (#214 Tier 3): x = SSS thickness drive, y = SSS radius, z = interior
    // in-scatter, w reserved. All 0 → today's look. Appended after `sheen` (siblings
    // and rt shaders keep their shorter prefix struct).
    pub body: [f32; 4],
    // Microstructure (#214 Tier 4): `micro` = [glitter amount, glitter density,
    // glitter sharpness, diffraction amount]; `micro2` = [diffraction freq, retro
    // amount, _, _]. All 0 → today's look. Appended after `body`.
    pub micro: [f32; 4],
    pub micro2: [f32; 4],
    // Spectral emission (#214 Tier 5 pt 1): [fluorescence, fluor hue, incandescence,
    // temperature (K)]. Additive emissive; all amounts 0 → today's look. Appended
    // after `micro2` (siblings/rt keep their shorter prefix struct).
    pub emit: [f32; 4],
    // Physical thin-film interference (#258 Tier 1): x = base film thickness (nm;
    // 0 → the model is disabled and the shader keeps the cosine-hack path),
    // y = thickness noise-marbling amount, z = film refractive index, w = gravity-
    // drainage gradient (top thin → bottom thick). Appended after `emit` (siblings/rt
    // keep their shorter prefix struct). thickness 0 → byte-identical.
    pub thinfilm: [f32; 4],
    // Demo scene bench (#288 Tier 3): a single placeable point light driven by the
    // brightest demo emitter. `demo_light_pos` = xyz world position, w = intensity
    // (0 → the light is OFF and the shader adds nothing, so every non-demo frame is
    // byte-identical); `demo_light_col` = rgb colour, w = radius (falloff scale).
    // Appended after `thinfilm` (siblings/rt keep their shorter prefix struct).
    pub demo_light_pos: [f32; 4],
    pub demo_light_col: [f32; 4],
    // Material colour transform (#305 Tier 1): x = effective hue offset (turns; the
    // base hue + the auto hue-cycle phase, both accumulated CPU-side), y = saturation
    // (1 = unchanged, → 0 greyscale), z = value (1 = unchanged, → 0 black), w reserved.
    // Applied to the resolved albedo so every cube-shader material (generator +
    // scenery/environment) recolours from one control. [0, 1, 1, _] → byte-identical.
    // Appended at the very tail (siblings/rt keep their shorter prefix struct).
    pub matcol: [f32; 4],
    // Live-sky cloud reflections (#305 Tier 2): [enable, cloud_cover, drift_phase,
    // strength]. A drifting procedural cloud layer modulates the sharp env reflection
    // (`sample_prefiltered`). enable 0 → byte-identical. Appended after `matcol`.
    pub skyrefl: [f32; 4],
    // Calibrated colour (#349 Tier 1): [mode, lut, amount, cal_t]. `mode` = the
    // `ColourMode` wire value (0 Aesthetic → identity, 1 Calibrated); `lut` = the
    // `CalLut` wire value (0 Turbo/1 Viridis/2 Inferno/3 Magma); `amount` (0..1) blends
    // the calibrated LUT colour over the aesthetic albedo; `cal_t` = the CPU-computed
    // `math::db_to_colour_t` coord for the frame's representative measured level. The
    // cube shader applies the law ONCE on the resolved albedo (`apply_calibrated`).
    // mode 0 → byte-identical. Appended at the tail (siblings/rt keep their shorter
    // prefix struct).
    pub cal: [f32; 4],
    // Node bevel (#bevel): x = bevel amount (0 = sharp cube → 1 = sphere); y = the
    // organon#217 T3 **face crown** — a per-fragment dome normal across each cube face
    // (`fs_main`, gated on y > 0; normal-only, so no geometry / depth / RT change); zw
    // reserved. x drives the cube shader's rounded-box vertex morph (`vs_main`/
    // `vs_depth`). Both set nonzero ONLY on the Original / Flow-Aligned instanced-cube
    // draw (render() gates it; scenery/liquid/water copies zero it), so the shared
    // cube mesh rounds only for the generator. x = y = 0 → today's sharp, flat cube
    // (byte-identical). The world writes y only for a live glyph ring. Appended at the
    // very tail (siblings/rt keep their prefix).
    pub shape: [f32; 4],
    // Procedural / texture-mapped materials (#472 Tier 1). `mtl` = [material_on
    // (0 → today's scalar-uniform PBR, byte-identical), projection_mode (0 triplanar
    // / 1 world-planar XZ / 2 object-planar), scale (world→UV frequency),
    // normal_strength]; `mtl2` = [ao_strength, rough_scale, metal_scale,
    // present_mask (runtime bitfield of which PNG channel maps loaded: 1 albedo |
    // 2 normal | 4 roughness | 8 metallic | 16 AO — an absent map falls back to the
    // scalar-uniform value)]. `mtl.x = 0` → byte-identical. Set nonzero ONLY on the
    // generator cube draw (render() gates it; scenery/liquid/water/plexus copies
    // zero it, like `shape`). Appended at the very tail (siblings/rt keep their
    // shorter prefix struct).
    pub mtl: [f32; 4],
    pub mtl2: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SkyUniforms {
    pub inv_view_proj: [[f32; 4]; 4],
    pub cam_pos: [f32; 4],
    pub params: [f32; 4],   // exposure, env_intensity, env_rotation_rad, bg_brightness
    pub env_tint: [f32; 4], // xyz = background tint colour (white = none), w unused
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Procedural / texture-mapped materials (#472 Tier 1) — the GPU **material texture
/// set** bound as `group(5)` on the cube pipeline. Six PBR channel maps (albedo /
/// normal / roughness / metallic / AO / height) + one shared sampler; the cube
/// shader samples them (triplanar or planar) when `Uniforms.mtl.x` is on. A neutral
/// 1×1 default set stands in for any absent channel, and `present_mask` records
/// which maps actually loaded so an absent channel falls back to the scalar-uniform
/// value in the shader. The set is (re)loaded from a folder of PNGs by the visual
/// when `Shared.material_gen` bumps (the `hdr_gen` pattern). This lives only in the
/// visual binary's compile of `render.rs` (it needs a GPU), like the rest of the
/// renderer.
struct MaterialTextures {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    // Kept alive so the views (held by `bind`) stay valid.
    _texs: Vec<wgpu::Texture>,
    bind: wgpu::BindGroup,
    /// Bitfield of loaded channels: 1 albedo | 2 normal | 4 roughness | 8 metallic |
    /// 16 AO, 32 height (#472 Tier 5: height present gates vertex displacement). 0 = the
    /// neutral built-in set → the shader keeps the scalar path.
    present_mask: u32,
}

impl MaterialTextures {
    /// The six channel filenames looked up inside a material folder, in bind order.
    /// `(filename, is_srgb, present_bit)` — albedo is colour (sRGB→linear on sample);
    /// the data maps are linear. Height's present bit (32) gates #472 Tier 5 displacement.
    const CHANNELS: [(&'static str, bool, u32); 6] = [
        ("albedo.png", true, 1),
        ("normal.png", false, 2),
        ("roughness.png", false, 4),
        ("metallic.png", false, 8),
        ("ao.png", false, 16),
        ("height.png", false, 32), // #472 Tier 5: present bit gates vertex displacement
    ];

    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material-layout"),
            entries: &Self::layout_entries(),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // Build the neutral built-in set up front (before the struct exists).
        let (texs, views) = neutral_material_set(device, queue);
        let bind = build_material_bind(device, &layout, &sampler, view_refs(&views));
        MaterialTextures { layout, sampler, _texs: texs, bind, present_mask: 0 }
    }

    /// The seven `group(5)` bindings: six `texture_2d<f32>` + one filtering sampler.
    fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 7] {
        // VERTEX_FRAGMENT: the fragment shades from these maps, and the vertex stage
        // samples the height map for #472 Tier 5 displacement.
        let tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        [
            tex(0), tex(1), tex(2), tex(3), tex(4), tex(5),
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]
    }

    /// Load the six channel maps from `dir` (missing ones → neutral 1×1 default),
    /// rebuild the bind group, and update `present_mask`. `dir = None` unloads back
    /// to the neutral set.
    fn load(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, dir: Option<&str>) {
        let Some(dir) = dir else {
            let (texs, views) = neutral_material_set(device, queue);
            self.bind = build_material_bind(device, &self.layout, &self.sampler, view_refs(&views));
            self._texs = texs;
            self.present_mask = 0;
            return;
        };
        let base = std::path::Path::new(dir);
        let mut texs: Vec<wgpu::Texture> = Vec::with_capacity(6);
        let mut views: Vec<wgpu::TextureView> = Vec::with_capacity(6);
        let mut mask = 0u32;
        for (name, srgb, bit) in Self::CHANNELS {
            let loaded = image::open(base.join(name)).ok().map(|img| img.to_rgba8());
            let (tex, present) = match loaded {
                Some(px) => {
                    let (w, h) = px.dimensions();
                    (make_rgba8_texture(device, queue, w, h, srgb, &px), true)
                }
                None => (make_rgba8_texture(device, queue, 1, 1, srgb, &neutral_pixel(name)), false),
            };
            if present {
                mask |= bit;
            }
            views.push(tex.create_view(&wgpu::TextureViewDescriptor::default()));
            texs.push(tex);
        }
        self.bind = build_material_bind(device, &self.layout, &self.sampler, view_refs(&views));
        self._texs = texs;
        self.present_mask = mask;
    }

    /// #472 Tier 3: point all six channel slots at the **baked** procedural set
    /// (owned by `MaterialBaker`), and set the combined `present_mask`. The cube
    /// shader samples only the channels whose present bit is set, so slots the graph
    /// didn't fill (absent bit) fall back to the scalar sliders even though a texture
    /// is bound. `baked` is in CHANNELS bind-slot order (albedo/normal/rough/metal/
    /// ao/height). Rebuilds the group(5) bind group.
    fn set_procedural(
        &mut self,
        device: &wgpu::Device,
        present_mask: u32,
        baked: [&wgpu::TextureView; 6],
    ) {
        self.bind = build_material_bind(device, &self.layout, &self.sampler, baked);
        // Drop any neutral 1×1 textures — the baked set (owned by MaterialBaker) is
        // what the bind now references.
        self._texs = Vec::new();
        self.present_mask = present_mask;
    }
}

/// Build the neutral built-in material set (six 1×1 default channel textures) +
/// their views.
fn neutral_material_set(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (Vec<wgpu::Texture>, Vec<wgpu::TextureView>) {
    let mut texs = Vec::with_capacity(6);
    let mut views = Vec::with_capacity(6);
    for (name, srgb, _bit) in MaterialTextures::CHANNELS {
        let tex = make_rgba8_texture(device, queue, 1, 1, srgb, &neutral_pixel(name));
        views.push(tex.create_view(&wgpu::TextureViewDescriptor::default()));
        texs.push(tex);
    }
    (texs, views)
}

/// Assemble the `group(5)` bind group from six channel views + the shared sampler.
/// Takes view *references* so the procedural path can splice a baked texture (owned
/// by `MaterialBaker`) into one slot alongside neutral views owned elsewhere.
fn build_material_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    views: [&wgpu::TextureView; 6],
) -> wgpu::BindGroup {
    let entries = [
        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(views[0]) },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(views[1]) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(views[2]) },
        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(views[3]) },
        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(views[4]) },
        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(views[5]) },
        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(sampler) },
    ];
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material-bind"),
        layout,
        entries: &entries,
    })
}

/// `[&views[0], …, &views[5]]` — the common all-neutral / all-loaded case.
fn view_refs(views: &[wgpu::TextureView]) -> [&wgpu::TextureView; 6] {
    [&views[0], &views[1], &views[2], &views[3], &views[4], &views[5]]
}

/// Neutral 1×1 RGBA8 fallback for an absent channel (content only matters if the
/// shader's `present_mask` bit is set, which it never is for a fallback — but keep
/// them physically sane).
fn neutral_pixel(channel: &str) -> [u8; 4] {
    match channel {
        "normal.png" => [128, 128, 255, 255], // flat tangent-space normal (+Z)
        "metallic.png" => [0, 0, 0, 255],      // dielectric
        "height.png" => [128, 128, 128, 255],  // mid height
        _ => [255, 255, 255, 255],             // albedo / roughness / AO = 1
    }
}

/// Upload one RGBA8 image as a 2-D texture (sRGB for colour, linear for data maps).
fn make_rgba8_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    w: u32,
    h: u32,
    srgb: bool,
    rgba: &[u8],
) -> wgpu::Texture {
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material-map"),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w.max(1) * 4),
            rows_per_image: Some(h.max(1)),
        },
        wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
    );
    tex
}

/// #472 Tier 2 — the compute baker. Evaluates one noise/pattern layer per texel
/// (`material_bake.wgsl`) into a single `Rgba16Float` storage texture that the
/// Tier-1 material set then samples for the routed channel. One layer for now.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LayerU {
    p0: [f32; 4],      // kind, channel, scale, rotation
    p1: [f32; 4],      // offset_x, offset_y, octaves, lacunarity
    p2: [f32; 4],      // gain, warp, contrast, gamma
    p3: [f32; 4],      // remap_lo, remap_hi, invert, seed
    meta: [f32; 4],    // enabled, blend_mode, _, _
    grad_lo: [f32; 4], // linear RGB, noise-0 end
    grad_hi: [f32; 4], // linear RGB, noise-1 end
}

/// #472 Tier 3 — the composite bake uniform: the layer stack + which channel this
/// dispatch bakes. `ctrl = [target_channel (MatChannel), res, num_layers, _]`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BakeU {
    layers: [LayerU; MAT_LAYERS],
    ctrl: [f32; 4],
}

/// #472 Tier 3 — the derive uniform: `ctrl = [res, strength, radius, source (0
/// height / 1 albedo luminance)]`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DeriveU {
    ctrl: [f32; 4],
}

/// The material-bake layer stack depth (base + overlays). Tier 2 = 1, Tier 3 = 2.
/// Must match `MAX_LAYERS` in `material_bake.wgsl`.
const MAT_LAYERS: usize = 2;
/// Bind-slot order of the six channel textures: albedo / normal / rough / metal /
/// ao / height (matches `MaterialTextures::CHANNELS`).
const MAT_SLOTS: usize = 6;

struct MaterialBaker {
    composite_pl: wgpu::ComputePipeline,
    normal_pl: wgpu::ComputePipeline,
    ao_pl: wgpu::ComputePipeline,
    bake_bgl: wgpu::BindGroupLayout,
    derive_bgl: wgpu::BindGroupLayout,
    bake_ubuf: wgpu::Buffer,
    derive_ubuf: wgpu::Buffer,
    samp: wgpu::Sampler,
    /// The six baked channel textures (bind-slot order), all at `res`.
    channels: Vec<(wgpu::Texture, wgpu::TextureView)>,
    res: u32,
    last_key: u64,
}

impl MaterialBaker {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("material-bake"),
            source: wgpu::ShaderSource::Wgsl(include_str!("material_bake.wgsl").into()),
        });
        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba16Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        // Composite: uniform B @0 + storage bake_out @1.
        let bake_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material-bake-layout"),
            entries: &[uniform(0), storage(1)],
        });
        // Derive: uniform D @2 + sampled input @3 + sampler @4 + storage out @5.
        let derive_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material-derive-layout"),
            entries: &[
                uniform(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                storage(5),
            ],
        });
        let bake_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("material-bake-pl"),
            bind_group_layouts: &[Some(&bake_bgl)],
            immediate_size: 0,
        });
        let derive_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("material-derive-pl"),
            bind_group_layouts: &[Some(&derive_bgl)],
            immediate_size: 0,
        });
        let mk = |label, layout: &wgpu::PipelineLayout, entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let composite_pl = mk("material-bake", &bake_pl, "bake");
        let normal_pl = mk("material-derive-normal", &derive_pl, "derive_normal");
        let ao_pl = mk("material-derive-ao", &derive_pl, "derive_ao");
        let bake_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material-bake-uniforms"),
            size: std::mem::size_of::<BakeU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let derive_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material-derive-uniforms"),
            size: std::mem::size_of::<DeriveU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material-derive-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let res = 512;
        let channels = (0..MAT_SLOTS).map(|_| Self::make_target(device, res)).collect();
        MaterialBaker {
            composite_pl, normal_pl, ao_pl, bake_bgl, derive_bgl,
            bake_ubuf, derive_ubuf, samp, channels, res, last_key: 0,
        }
    }

    fn make_target(device: &wgpu::Device, res: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("material-baked"),
            size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    /// Pack a raw `[f32; 18]` layer block + its gradient into the GPU LayerU. `enabled`
    /// overrides slot [16] (layer 1's [16] is the global procedural flag, not per-layer
    /// enable, so the caller passes it explicitly).
    fn layer_u(layer: &[f32; 18], grad: &[f32; 8], enabled: f32) -> LayerU {
        LayerU {
            p0: [layer[0], layer[1], layer[2], layer[3]],
            p1: [layer[4], layer[5], layer[6], layer[7]],
            p2: [layer[8], layer[9], layer[10], layer[11]],
            p3: [layer[12], layer[13], layer[14], layer[15]],
            meta: [enabled, layer[17], 0.0, 0.0], // enabled, blend_mode
            grad_lo: [grad[0], grad[1], grad[2], 1.0],
            grad_hi: [grad[4], grad[5], grad[6], 1.0],
        }
    }

    /// A cheap change key over all the layers + derive floats (bit-folded), so the
    /// baker only re-dispatches when the graph actually changes.
    fn key(layers: &[LayerU; MAT_LAYERS], derive: &[f32; 8], res: u32) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut fold = |bits: u32| {
            h ^= bits as u64;
            h = h.wrapping_mul(0x100000001b3);
        };
        for l in layers {
            for arr in [&l.p0, &l.p1, &l.p2, &l.p3, &l.meta, &l.grad_lo, &l.grad_hi] {
                for v in arr {
                    fold(v.to_bits());
                }
            }
        }
        for v in derive {
            fold(v.to_bits());
        }
        fold(res);
        h
    }

    /// (Re)bake the whole material: composite each channel that a layer targets, then
    /// derive normal / AO from height (or albedo). Returns `(changed, present_mask)`;
    /// `present_mask` is the OR of the shaded channel bits (albedo 1 / normal 2 /
    /// rough 4 / metal 8 / ao 16). The baked channel views live in `self.channels`.
    fn bake(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: &[LayerU; MAT_LAYERS],
        derive: &[f32; 8],
        res: u32,
        force: bool,
    ) -> (bool, u32) {
        let want_res = res.clamp(64, 2048);
        let k = Self::key(layers, derive, want_res);
        // present_mask is deterministic from the layers/derive, so recompute it even
        // on an unchanged bake (the caller may need it after a set rebuild).
        let mask = Self::present_mask(layers, derive);
        if !force && k == self.last_key && want_res == self.res {
            return (false, mask);
        }
        self.last_key = k;
        if want_res != self.res {
            self.channels = (0..MAT_SLOTS).map(|_| Self::make_target(device, want_res)).collect();
            self.res = want_res;
        }
        let bu = BakeU { layers: *layers, ctrl: [0.0, self.res as f32, MAT_LAYERS as f32, 0.0] };
        let wg = (self.res + 7) / 8;

        // --- Composite pass: one dispatch per MatChannel that a layer targets. ---
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("material-bake-enc"),
        });
        for ch in 0u32..6 {
            let Some((slot, _bit)) = Self::channel_slot(ch) else { continue };
            if !layers.iter().any(|l| l.meta[0] > 0.5 && l.p0[1] as u32 == ch) {
                continue;
            }
            let mut u = bu;
            u.ctrl[0] = ch as f32;
            // The uniform buffer is reused per channel; write, bind, dispatch in order.
            queue.write_buffer(&self.bake_ubuf, 0, bytemuck::bytes_of(&u));
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material-bake-bind"),
                layout: &self.bake_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.bake_ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.channels[slot].1),
                    },
                ],
            });
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("material-composite-pass"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&self.composite_pl);
            cp.set_bind_group(0, &bind, &[]);
            cp.dispatch_workgroups(wg, wg, 1);
        }
        queue.submit(Some(enc.finish()));

        // --- Derive passes (separate submit so the composite writes are visible). ---
        if derive[0] > 0.5 || derive[1] > 0.5 {
            let src_albedo = derive[2] > 0.5;
            // Normal / AO read height (slot 5) unless normal is sourced from albedo (slot 0).
            let mut denc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("material-derive-enc"),
            });
            let mut derive_pass = |pipeline: &wgpu::ComputePipeline,
                                   input_slot: usize,
                                   output_slot: usize,
                                   du: DeriveU| {
                queue.write_buffer(&self.derive_ubuf, 0, bytemuck::bytes_of(&du));
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material-derive-bind"),
                    layout: &self.derive_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 2, resource: self.derive_ubuf.as_entire_binding() },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&self.channels[input_slot].1),
                        },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.samp) },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&self.channels[output_slot].1),
                        },
                    ],
                });
                let mut cp = denc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("material-derive-pass"),
                    timestamp_writes: None,
                });
                cp.set_pipeline(pipeline);
                cp.set_bind_group(0, &bind, &[]);
                cp.dispatch_workgroups(wg, wg, 1);
            };
            if derive[0] > 0.5 {
                let input = if src_albedo { 0 } else { 5 }; // albedo or height
                let du = DeriveU { ctrl: [self.res as f32, derive[3], 0.0, if src_albedo { 1.0 } else { 0.0 }] };
                derive_pass(&self.normal_pl, input, 1, du); // → normal slot 1
            }
            if derive[1] > 0.5 {
                let du = DeriveU { ctrl: [self.res as f32, derive[4], derive[5], 0.0] };
                derive_pass(&self.ao_pl, 5, 4, du); // height → AO slot 4
            }
            drop(derive_pass);
            queue.submit(Some(denc.finish()));
        }
        (true, mask)
    }

    /// Which shaded present-mask bits the current graph produces (albedo 1 / normal 2
    /// / rough 4 / metal 8 / ao 16). Height (slot 5) is baked but not shaded in T1.
    fn present_mask(layers: &[LayerU; MAT_LAYERS], derive: &[f32; 8]) -> u32 {
        let mut mask = 0u32;
        for l in layers {
            if l.meta[0] > 0.5 {
                if let Some((_slot, bit)) = Self::channel_slot(l.p0[1] as u32) {
                    mask |= bit;
                }
            }
        }
        if derive[0] > 0.5 {
            mask |= 2; // derived normal
        }
        if derive[1] > 0.5 {
            mask |= 16; // derived AO
        }
        mask
    }

    /// Map a MatChannel ordinal → (CHANNELS slot index, present-mask bit). Height's
    /// bit (32) gates #472 Tier 5 vertex displacement (it also feeds derived normal/AO);
    /// Emissive has no bound slot yet → None.
    fn channel_slot(channel: u32) -> Option<(usize, u32)> {
        match channel {
            0 => Some((0, 1)),  // Albedo
            1 => Some((2, 4)),  // Roughness
            2 => Some((3, 8)),  // Metallic
            3 => Some((5, 32)), // Height (bit 32 gates #472 Tier 5 vertex displacement)
            4 => Some((4, 16)), // AO
            _ => None,          // Emissive (Tier 5)
        }
    }
}

// The static cube mesh, SUBDIVIDED into an N×N grid per face. The extra interior
// vertices exist so the cube shader's node-bevel morph (`round_local`, driven by
// `Uniforms.shape.x`) can bulge the faces from a sharp cube through a rounded cube to
// a full sphere. At bevel 0 the mesh is a flat cube — geometrically/shading identical
// to the old 4-verts-per-face version (same face normals, same RGB-cube colour), just
// more triangles; the bevel morph is what puts the interior vertices to work. Every
// non-generator cube draw (scenery, demo, …) shares this mesh but renders it flat
// because render() zeros `shape.x` for them.
fn cube_mesh() -> (Vec<Vertex>, Vec<u16>) {
    // Segments per face edge. 8 → smooth spheres at bevel 1 with the analytic normals
    // (`round_local` returns `normalize(d)`); tune for the silhouette-vs-triangle-count
    // trade-off. 6·(N+1)² verts, 6·N²·2 triangles.
    const N: usize = 8;
    let faces = [
        ([0.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([-0.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
        ([0.0, 0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, -0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
        ([0.0, 0.0, 0.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    ];
    let stride = (N + 1) as u16;
    let mut verts = Vec::with_capacity(6 * (N + 1) * (N + 1));
    let mut idx = Vec::with_capacity(6 * N * N * 6);
    for (c, u, v, n) in faces {
        let base = verts.len() as u16;
        // Grid of verts across the face, each spanning (su, sv) ∈ [-0.5, 0.5]².
        for j in 0..=N {
            for i in 0..=N {
                let su = i as f32 / N as f32 - 0.5;
                let sv = j as f32 / N as f32 - 0.5;
                let pos = [
                    c[0] + u[0] * su + v[0] * sv,
                    c[1] + u[1] * su + v[1] * sv,
                    c[2] + u[2] * su + v[2] * sv,
                ];
                let color = [pos[0] + 0.5, pos[1] + 0.5, pos[2] + 0.5];
                verts.push(Vertex { pos, normal: n, color });
            }
        }
        // Emit the grid quads so the front (CCW) face points OUTWARD (matching `n`),
        // which lets the opaque pass back-face cull safely. The (u, v) basis isn't
        // consistently right-handed w.r.t. `n` across the six faces, so pick the
        // winding per face from the sign of (u × v) · n.
        let uxv = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let outward = uxv[0] * n[0] + uxv[1] * n[1] + uxv[2] * n[2] > 0.0;
        for j in 0..N as u16 {
            for i in 0..N as u16 {
                let a = base + j * stride + i; // (i,   j)
                let b = a + 1; //               (i+1, j)
                let cc = a + stride; //         (i,   j+1)
                let d = cc + 1; //              (i+1, j+1)
                if outward {
                    idx.extend_from_slice(&[a, b, d, a, d, cc]);
                } else {
                    idx.extend_from_slice(&[a, d, b, a, cc, d]);
                }
            }
        }
    }
    (verts, idx)
}

/// A unit cylinder wall along local +Z: radius 0.5, spanning z ∈ [-0.5, 0.5],
/// `SIDES` facets, radial (outward) normals. Open-ended (no caps) so consecutive
/// segments meeting at a node read as a continuous tube rather than discs at the
/// joints. Per-segment instance matrices stretch local +Z to bridge node→node.
fn cyl_mesh() -> (Vec<Vertex>, Vec<u16>) {
    const SIDES: usize = 16;
    let mut verts = Vec::with_capacity(SIDES * 2);
    let mut idx = Vec::with_capacity(SIDES * 6);
    for s in 0..SIDES {
        let a = (s as f32 / SIDES as f32) * std::f32::consts::TAU;
        let (c, sn) = (a.cos(), a.sin());
        let n = [c, sn, 0.0]; // radial → smooth round shading
        // White base: the per-instance tint (an HSV sweep along the tube) provides
        // the colour, so it flows along the whole strand instead of repeating per
        // segment.
        for z in [-0.5f32, 0.5] {
            verts.push(Vertex {
                pos: [0.5 * c, 0.5 * sn, z],
                normal: n,
                color: [1.0, 1.0, 1.0],
            });
        }
    }
    // Verts laid out per facet as [bottom(2s), top(2s+1)]; stitch facet s→s+1.
    for s in 0..SIDES {
        let s2 = (s + 1) % SIDES;
        let (b0, t0) = ((2 * s) as u16, (2 * s + 1) as u16);
        let (b1, t1) = ((2 * s2) as u16, (2 * s2 + 1) as u16);
        idx.extend_from_slice(&[b0, b1, t1, b0, t1, t0]);
    }
    (verts, idx)
}

/// A closed **capsule** along local +Z (#260 Tier 1 Neural Tissue): radius 0.5,
/// z ∈ [-0.5, 0.5], `SIDES` facets, radial normals — a cylinder body with
/// hemispherical caps folded INTO the ±z ends so the tube is CLOSED (no open pipe)
/// while still spanning exactly [-0.5, 0.5] (so the per-segment `push_rod` instance
/// matrix — x,y = thickness, z = node→node length — places it centre-to-centre, the
/// soma bodies burying the joints). Wound outward (front faces point out) so it
/// survives back-face culling on the opaque path, like `cyl_mesh`. White base — the
/// per-instance tint paints it.
fn capsule_mesh() -> (Vec<Vertex>, Vec<u16>) {
    const SIDES: usize = 16;
    const RINGS: usize = 3; // latitude rings per cap
    const BODY: f32 = 0.35; // body half-length; caps occupy the remaining 0.15 each
    const R: f32 = 0.5;
    let mut verts: Vec<Vertex> = Vec::new();
    let mut idx: Vec<u16> = Vec::new();
    // A ring of `SIDES` verts at height z, radius r, with a normal tilted by the cap
    // slope (drdz). Returns the base index of the ring.
    let push_ring = |verts: &mut Vec<Vertex>, z: f32, r: f32, drdz: f32| -> u16 {
        let base = verts.len() as u16;
        let inv = 1.0 / (1.0 + drdz * drdz).sqrt();
        for s in 0..SIDES {
            let a = (s as f32 / SIDES as f32) * std::f32::consts::TAU;
            let (c, sn) = (a.cos(), a.sin());
            verts.push(Vertex {
                pos: [r * c, r * sn, z],
                normal: [c * inv, sn * inv, -drdz * inv],
                color: [1.0, 1.0, 1.0],
            });
        }
        base
    };
    let mut rings: Vec<u16> = Vec::new();
    // Bottom cap: pole → equator (z from -0.5 to -BODY), a squashed hemisphere.
    for ri in 0..=RINGS {
        let t = ri as f32 / RINGS as f32; // 0 pole → 1 equator
        let phi = t * std::f32::consts::FRAC_PI_2;
        let r = R * phi.sin();
        let z = -0.5 + (0.5 - BODY) * (1.0 - phi.cos());
        // dr/dz ≈ slope of the cap; positive-going radius as z rises.
        let drdz = if t < 1.0 { 2.0 } else { 0.0 };
        rings.push(push_ring(&mut verts, z, r.max(1e-4), drdz));
    }
    // Body: straight wall (drdz = 0) at z = ±BODY.
    rings.push(push_ring(&mut verts, -BODY, R, 0.0));
    rings.push(push_ring(&mut verts, BODY, R, 0.0));
    // Top cap: equator → pole (z from BODY to 0.5).
    for ri in 0..=RINGS {
        let t = ri as f32 / RINGS as f32; // 0 equator → 1 pole
        let phi = (1.0 - t) * std::f32::consts::FRAC_PI_2;
        let r = R * phi.sin();
        let z = 0.5 - (0.5 - BODY) * (1.0 - phi.cos());
        let drdz = if t > 0.0 { -2.0 } else { 0.0 };
        rings.push(push_ring(&mut verts, z, r.max(1e-4), drdz));
    }
    // Stitch consecutive rings into quads, wound outward.
    for w in rings.windows(2) {
        let (r0, r1) = (w[0], w[1]);
        for s in 0..SIDES {
            let s2 = ((s + 1) % SIDES) as u16;
            let (a, bb) = (r0 + s as u16, r0 + s2);
            let (c, d) = (r1 + s as u16, r1 + s2);
            // outward winding (matches cyl_mesh's b0,b1,t1 / b0,t1,t0 order)
            idx.extend_from_slice(&[a, bb, d, a, d, c]);
        }
    }
    (verts, idx)
}

/// A unit **icosphere** (subdivided octahedron), radius 0.5, radial normals — the
/// soma cell body + synaptic-bouton mesh (#260 Tier 1). Same ±0.5 half-extent
/// convention as `cube_mesh` (so a soma reads as the inscribed sphere of the cube
/// a node would draw). Wound outward. White base — the per-instance tint paints it.
fn soma_mesh() -> (Vec<Vertex>, Vec<u16>) {
    const SUBDIV: usize = 2; // octahedron → 2 subdivisions = 128 tris
    // Octahedron faces (CCW when viewed from outside), each a triangle of unit dirs.
    let o = [
        [0.0f32, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    // 8 faces, each (top/bottom, +x/-x, +z/-z) with outward winding.
    let faces: [[usize; 3]; 8] = [
        [0, 4, 2], [0, 2, 5], [0, 5, 3], [0, 3, 4],
        [1, 2, 4], [1, 5, 2], [1, 3, 5], [1, 4, 3],
    ];
    let mut verts: Vec<Vertex> = Vec::new();
    let mut idx: Vec<u16> = Vec::new();
    let push_v = |verts: &mut Vec<Vertex>, dir: Vec3| -> u16 {
        let n = dir.normalize();
        let base = verts.len() as u16;
        verts.push(Vertex {
            pos: [n.x * 0.5, n.y * 0.5, n.z * 0.5],
            normal: [n.x, n.y, n.z],
            color: [1.0, 1.0, 1.0],
        });
        base
    };
    for f in faces {
        let a = Vec3::from_array(o[f[0]]);
        let b = Vec3::from_array(o[f[1]]);
        let c = Vec3::from_array(o[f[2]]);
        // Uniformly subdivide the triangle in barycentric steps, projecting to the
        // sphere. Emit the small triangles (winding preserved → outward).
        let n = 1usize << SUBDIV;
        for i in 0..n {
            for j in 0..(n - i) {
                let p = |ii: usize, jj: usize| -> Vec3 {
                    let u = ii as f32 / n as f32;
                    let v = jj as f32 / n as f32;
                    a * (1.0 - u - v) + b * u + c * v
                };
                let v0 = push_v(&mut verts, p(i, j));
                let v1 = push_v(&mut verts, p(i + 1, j));
                let v2 = push_v(&mut verts, p(i, j + 1));
                idx.extend_from_slice(&[v0, v1, v2]);
                if j < n - i - 1 {
                    let v3 = push_v(&mut verts, p(i + 1, j + 1));
                    idx.extend_from_slice(&[v1, v3, v2]);
                }
            }
        }
    }
    (verts, idx)
}

/// A coarse (12-triangle) sharp cube — the historical cube geometry, before
/// `cube_mesh` was subdivided for the node bevel. Used only for the RT BLAS: the
/// subdivided raster cube is the SAME flat shape at bevel 0, so this keeps the
/// acceleration structure light (12 tris vs 6·N²·2) without changing what it traces.
/// (The bevel morph is a vertex-shader effect the ray tracer can't see, so RT traces
/// the un-beveled cube regardless — matching the flat cube exactly at bevel 0.)
fn cube_mesh_coarse() -> (Vec<Vertex>, Vec<u16>) {
    let faces = [
        ([0.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([-0.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
        ([0.0, 0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, -0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
        ([0.0, 0.0, 0.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    ];
    let mut verts = Vec::with_capacity(24);
    let mut idx = Vec::with_capacity(36);
    for (c, u, v, n) in faces {
        let base = verts.len() as u16;
        for (su, sv) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
            let pos = [
                c[0] + u[0] * su + v[0] * sv,
                c[1] + u[1] * su + v[1] * sv,
                c[2] + u[2] * su + v[2] * sv,
            ];
            let color = [pos[0] + 0.5, pos[1] + 0.5, pos[2] + 0.5];
            verts.push(Vertex { pos, normal: n, color });
        }
        let uxv = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let outward = uxv[0] * n[0] + uxv[1] * n[1] + uxv[2] * n[2] > 0.0;
        if outward {
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    (verts, idx)
}

/// The RT BLAS source geometry (#195 Tier 0): positions + u32 indices of the
/// static mesh the raster pipelines draw (cube, or the Swept-Tubes cylinder when
/// `tube`), so the acceleration structure and the raster scene agree about shape.
/// The cube uses the coarse 12-tri variant (identical flat shape to the subdivided
/// raster cube at bevel 0; RT can't see the vertex bevel morph either way).
pub fn rt_mesh(tube: bool) -> (Vec<[f32; 3]>, Vec<u32>) {
    let (verts, indices) = if tube { cyl_mesh() } else { cube_mesh_coarse() };
    (
        verts.iter().map(|v| v.pos).collect(),
        indices.iter().map(|&i| i as u32).collect(),
    )
}

/// The RT BLAS source geometry for the **particle beads** (#298 Tier 4): a
/// **unit-RADIUS** sphere (positions + u32 indices), so a bead's TLAS instance is a
/// plain `translate(centre) · scale(size)` — the sphere impostor made real geometry.
/// Reuses the `soma_mesh` octahedron-subdivided sphere (radius 0.5 there → doubled to
/// radius 1.0 here) so the shape matches the raster impostor.
pub fn rt_sphere_mesh() -> (Vec<[f32; 3]>, Vec<u32>) {
    let (verts, indices) = soma_mesh();
    (
        verts.iter().map(|v| [v.pos[0] * 2.0, v.pos[1] * 2.0, v.pos[2] * 2.0]).collect(),
        indices.iter().map(|&i| i as u32).collect(),
    )
}

/// Outward normal for a surface of revolution at angle `a` whose local radius
/// changes with z at rate `drdz` (dr/dz): tilts the radial normal toward −z
/// where the body widens. Normalised.
fn revol_normal(a: f32, drdz: f32) -> [f32; 3] {
    let (c, s) = (a.cos(), a.sin());
    let inv = 1.0 / (1.0 + drdz * drdz).sqrt();
    [c * inv, s * inv, -drdz * inv]
}

/// Push a closed body of revolution along local +Z (forward), white-coloured (the
/// per-instance tint paints it). `profile(t)` is the radius at t∈[0,1] from tail
/// (z=ztail) to nose (z=znose). Outward winding matches `cyl_mesh` (the body is
/// convex + correctly wound, so it survives back-face culling on the opaque path).
fn push_spindle(
    verts: &mut Vec<Vertex>,
    idx: &mut Vec<u16>,
    rings: usize,
    sides: usize,
    ztail: f32,
    znose: f32,
    profile: &dyn Fn(f32) -> f32,
) {
    let base = verts.len() as u16;
    for ri in 0..=rings {
        let t = ri as f32 / rings as f32;
        let z = ztail + (znose - ztail) * t;
        let r = profile(t);
        // dr/dz via a central finite difference (clamped at the ends).
        let dt = 1.0 / rings as f32;
        let (tp, tm) = ((t + dt).min(1.0), (t - dt).max(0.0));
        let dz = (tp - tm) * (znose - ztail);
        let drdz = if dz.abs() > 1e-6 {
            (profile(tp) - profile(tm)) / dz
        } else {
            0.0
        };
        for si in 0..sides {
            let a = si as f32 / sides as f32 * std::f32::consts::TAU;
            let (c, s) = (a.cos(), a.sin());
            verts.push(Vertex {
                pos: [r * c, r * s, z],
                normal: revol_normal(a, drdz),
                color: [1.0, 1.0, 1.0],
            });
        }
    }
    for ri in 0..rings {
        for si in 0..sides {
            let s2 = (si + 1) % sides;
            let a = base + (ri * sides + si) as u16;
            let b = base + (ri * sides + s2) as u16;
            let c = base + ((ri + 1) * sides + si) as u16;
            let d = base + ((ri + 1) * sides + s2) as u16;
            idx.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
}

/// Push a flat, **double-sided** quad (corners p0→p1→p2→p3) — both winding orders
/// so the thin fin/wing shows from either side under back-face culling.
/// White-coloured (tint paints it).
fn push_quad2(verts: &mut Vec<Vertex>, idx: &mut Vec<u16>, p: [[f32; 3]; 4], n: [f32; 3]) {
    for side in 0..2 {
        let nn = if side == 0 { n } else { [-n[0], -n[1], -n[2]] };
        let base = verts.len() as u16;
        for q in p {
            verts.push(Vertex { pos: q, normal: nn, color: [1.0, 1.0, 1.0] });
        }
        if side == 0 {
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            idx.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
}

/// The Boids creature meshes (#52): local +Z = forward, +Y = up, +X = right,
/// roughly unit-length (the per-agent instance matrix orients + scales them).
/// White vertex colour so the agent's HSV/palette tint paints the whole body.
/// Kinds: 0 Fish, 1 Bird, 2 Manta, 3 Dart (`BoidsForm` − 1).
pub const CREATURE_KINDS: u32 = 4;
fn creature_mesh(kind: u32) -> (Vec<Vertex>, Vec<u16>) {
    use std::f32::consts::PI;
    let mut v = Vec::new();
    let mut i = Vec::new();
    match kind {
        // --- Bird: slim body + swept delta wings + forked tail ---
        1 => {
            push_spindle(&mut v, &mut i, 10, 10, -0.5, 0.55, &|t| {
                0.09 * (PI * t).sin().max(0.0).powf(0.6)
            });
            let d = 0.08; // wing-tip dihedral lift
            push_quad2(&mut v, &mut i,
                [[0.04, 0.0, 0.18], [0.04, 0.0, -0.10], [0.62, d, -0.22], [0.50, d, 0.04]],
                [0.0, 1.0, 0.0]);
            push_quad2(&mut v, &mut i,
                [[-0.04, 0.0, 0.18], [-0.50, d, 0.04], [-0.62, d, -0.22], [-0.04, 0.0, -0.10]],
                [0.0, 1.0, 0.0]);
            push_quad2(&mut v, &mut i,
                [[0.10, 0.01, -0.40], [0.0, 0.0, -0.62], [-0.10, 0.01, -0.40], [0.0, 0.0, -0.46]],
                [0.0, 1.0, 0.0]);
        }
        // --- Manta / ray: wide flat delta + a thin trailing tail ---
        2 => {
            push_quad2(&mut v, &mut i,
                [[0.0, 0.0, 0.45], [0.62, 0.0, -0.10], [0.0, 0.05, -0.28], [-0.62, 0.0, -0.10]],
                [0.0, 1.0, 0.0]);
            // a low spine ridge for a touch of thickness
            push_spindle(&mut v, &mut i, 8, 8, -0.28, 0.42, &|t| 0.05 * (PI * t).sin().max(0.0));
            push_quad2(&mut v, &mut i,
                [[0.02, 0.0, -0.28], [0.0, 0.0, -0.9], [-0.02, 0.0, -0.28], [0.0, 0.01, -0.6]],
                [0.0, 1.0, 0.0]);
        }
        // --- Dart / arrow: sleek 4-sided spindle + crossed tail fins ---
        3 => {
            push_spindle(&mut v, &mut i, 6, 4, -0.5, 0.6, &|t| {
                0.12 * t.powf(0.8) * (1.0 - 0.2 * t)
            });
            push_quad2(&mut v, &mut i,
                [[0.0, 0.0, -0.18], [0.0, 0.28, -0.55], [0.0, 0.0, -0.5], [0.0, -0.28, -0.55]],
                [1.0, 0.0, 0.0]);
            push_quad2(&mut v, &mut i,
                [[0.0, 0.0, -0.18], [0.28, 0.0, -0.55], [0.0, 0.0, -0.5], [-0.28, 0.0, -0.55]],
                [0.0, 1.0, 0.0]);
        }
        // --- Fish (default): tapered body + caudal/dorsal/pectoral fins ---
        _ => {
            push_spindle(&mut v, &mut i, 12, 12, -0.45, 0.5, &|t| {
                0.20 * (PI * t).sin().max(0.0).powf(0.7)
            });
            // caudal (tail) fin — vertical & forked, in the x≈0 plane
            push_quad2(&mut v, &mut i,
                [[0.0, 0.0, -0.40], [0.0, 0.26, -0.62], [0.0, 0.0, -0.5], [0.0, -0.26, -0.62]],
                [1.0, 0.0, 0.0]);
            // dorsal fin on top
            push_quad2(&mut v, &mut i,
                [[0.0, 0.11, 0.06], [0.0, 0.30, -0.16], [0.0, 0.10, -0.2], [0.0, 0.11, -0.06]],
                [1.0, 0.0, 0.0]);
            // pectoral fins (left/right)
            push_quad2(&mut v, &mut i,
                [[0.05, 0.0, 0.10], [0.26, -0.05, -0.02], [0.06, -0.02, -0.10], [0.05, 0.0, 0.04]],
                [0.0, 1.0, 0.0]);
            push_quad2(&mut v, &mut i,
                [[-0.05, 0.0, 0.10], [-0.05, 0.0, 0.04], [-0.06, -0.02, -0.10], [-0.26, -0.05, -0.02]],
                [0.0, 1.0, 0.0]);
        }
    }
    (v, i)
}

// ===========================================================================
// RenderFrame (GitHub #104) — the per-frame inputs to `Renderer::render`,
// bundled into one (Copy) parameter object instead of ~55 positional args.
// `RenderPath` makes the formerly-transposable mode bools (metaball / voxel /
// mandelbulb / kifs / membrane) a type invariant: exactly one path per frame.
// Pure plumbing — no wire-format or behaviour change.
// ===========================================================================

/// Which path produces the scene this frame — mutually exclusive by construction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderPath {
    /// The default instanced cube / cylinder / creature draw.
    Instanced,
    /// A prebuilt world-space membrane mesh.
    Membrane,
    /// Raymarched smooth metaball field.
    Metaball,
    /// Emissive volume — the metaball field raymarched as a glowing medium (#152).
    Volume,
    /// DDA grid raymarch (voxels).
    Voxel,
    /// Analytic distance-estimated fractal (Mandelbulb).
    Mandelbulb,
    /// Raymarched union-of-SDF-primitives sea creature (#476 Tier 1).
    Creature,
    /// Raymarched TPMS / minimal-surface isosurface (gyroid, #127).
    MinimalSurface,
    /// Fullscreen kaleidoscopic field (KIFS).
    Kifs,
    /// Raymarched neural-field isosurface (#200 Tier 1).
    NeuralField,
    /// Raymarched analytic lens SDF (double-convex / plano-convex, #258 Tier 3).
    Lens,
    /// Gaussian Splatting surface: the node set drawn as anisotropic 3-D Gaussians
    /// (additive glow or IBL-lit 2DGS disks), reusing the instance/tint buffers.
    Splat,
}

/// Background layers drawn behind the scene.
#[derive(Clone, Copy)]
pub struct Background<'a> {
    pub terrain_on: bool,
    pub terrain_u: &'a TerrainUniforms,
    /// Terrain resolution divisor (1 = full, 2 = half, 4 = quarter).
    pub terrain_scale: u32,
    pub stars_on: bool,
    pub star_sun: bool,
    pub star_u: &'a StarUniforms,
    // #102B: the FFT ocean is on — the terrain pass runs even with the landscape off
    // (ocean-only mode), drawing the sky + sea.
    pub ocean_on: bool,
}

/// Neural Tissue sub-batches (#260 Tier 1). When set, the instanced draw path
/// splits the ONE instance/tint buffer into three contiguous ranges — somata
/// (icospheres), capsules (capped tubes), boutons (small icospheres) — and issues
/// one instanced draw per range with the matching mesh bound. The ranges are
/// `[0, soma)`, `[soma, soma+capsule)`, `[soma+capsule, total)`; the sum equals
/// `Surface.instances.len()`.
#[derive(Clone, Copy, Debug)]
pub struct NeuralBatches {
    pub soma_count: u32,
    pub capsule_count: u32,
    pub bouton_count: u32,
}

/// Plexus Tier-1 shape-morph draw split (#plexus): the one instance buffer holds
/// `markers` node instances then `struts` edge instances (as `math::draw_plexus`
/// emits them). Each range is drawn with its own procedurally-morphed mesh — the
/// node mesh (cube→sphere) over `[0, markers)`, the strut mesh (square→circle) over
/// `[markers, markers+struts)` — so the two shapes morph independently.
#[derive(Clone, Copy, Debug)]
pub struct PlexusBatches {
    pub markers: u32,
    pub struts: u32,
}

/// The geometry payload — which path, plus every path's inputs.
#[derive(Clone, Copy)]
pub struct Surface<'a> {
    pub path: RenderPath,
    // Instanced payload
    pub instances: &'a [Mat4],
    pub tints: &'a [Vec4],
    /// organon#217 T1 — per-instance **emission**, parallel to `instances` (linear RGB
    /// radiance in `xyz`, gain in `w`; `cube.wgsl` adds `rgb * w` to `emissive`,
    /// bypassing albedo). **Empty = inert**: the renderer binds an all-zero buffer and
    /// the shader's added term is exactly zero, so every frame that passes `&[]` here
    /// — which is every frame not driven by the glyph ring — is byte-identical to
    /// before the attribute existed. A non-empty slice must be `instances.len()` long;
    /// any other length is treated as empty (zero), never as a partial upload.
    pub emits: &'a [Vec4],
    /// RT / path-tracer geometry override. When non-empty, the ray tracer's hit-
    /// shading instance buffers use THESE instead of `instances` (which the raster
    /// draws). In Contiguous Swept Tubes the raster draws the welded mesh and
    /// `instances` is empty, so this carries the per-segment cylinder approximation
    /// so the path tracer / RT passes have geometry to trace + shade. Empty = use
    /// `instances` (every non-welded path, byte-identical).
    pub rt_instances: &'a [Mat4],
    pub rt_tints: &'a [Vec4],
    pub tube: bool,
    /// Neural Tissue (#260 Tier 1): when Some, the instanced draw is split into
    /// soma / capsule / bouton sub-batches, each drawn with its own mesh. Overrides
    /// the single-mesh (`tube`/`creature`) selection for the main draws. None =
    /// ordinary single-mesh instancing.
    pub neural_batches: Option<NeuralBatches>,
    /// Neural Tissue single-mesh fallback (#260 Tier 1): for NON-graph generators
    /// with the Neural Tissue surface, render the swept bridges as CLOSED capsules
    /// (capped tube) instead of the open `cyl_mesh`. Ignored when `neural_batches`
    /// is set. `tube` is also true in this case.
    pub neural_capsule: bool,
    // Contiguous (welded) Swept Tubes: when `swept` is set, `instances` is empty and
    // this one dynamic welded mesh (u32 indices, per-vertex colour) is drawn instead
    // of the per-segment instanced cylinders (mirrors the Membrane sheet path).
    pub swept: bool,
    pub swept_verts: &'a [organon_core::math::TubeVertex],
    pub swept_idx: &'a [u32],
    /// Boids creature mesh (#52): -1 = none, else the creature kind.
    pub creature: i32,
    // Membrane mesh (parallel arrays)
    pub mem_pos: &'a [Vec3],
    pub mem_norm: &'a [Vec3],
    pub mem_col: &'a [Vec4],
    pub mem_idx: &'a [u32],
    pub show_strands: bool,
    /// Membrane Skin-Arms mode: the shell sheet is NOT built (`mem_idx` empty);
    /// instead each strand (arm) is drawn as its own closed finger — the welded
    /// `swept` mesh (Mesh build) or the per-segment `instances` rods (Impostor
    /// build placeholder) — so the membrane branch must draw that geometry.
    pub membrane_arms: bool,
    /// Membrane Skin-Arms Impostor build (Stage 2): one capsule impostor per arm
    /// segment (empty unless arms + Impostor build are active). Drawn by the shared
    /// bead-style pipeline in the Membrane branch; no per-frame mesh.
    pub arm_caps: &'a [ArmInstance],
    /// Plexus Tier 2 impostors: when `plexus_impostor`, the web is drawn as GPU
    /// impostors instead of instanced cubes — `plexus_node_caps` as analytic sphere
    /// impostors (A≈B degenerate capsules) and `plexus_edge_caps` as capsule tubes,
    /// each with its OWN material (`plexus_node_mat` / `plexus_edge_mat`). `instances`
    /// is left empty in this mode so the raster cube draw is skipped.
    pub plexus_impostor: bool,
    pub plexus_node_caps: &'a [ArmInstance],
    pub plexus_edge_caps: &'a [ArmInstance],
    pub plexus_node_mat: PlexMat,
    pub plexus_edge_mat: PlexMat,
    /// organon#217 T6/T3 — the coaxial-glass core for every capsule impostor draw this
    /// frame (arms + both plexus batches): `[core_frac, absorb]`, `Shared.capsule[0..2]`.
    /// Handed to `ParticleSystem::set_capsule_core` before the uploads it affects; `[0, 0]`
    /// (the default) is T6's inert gate. ⚠️ `ORGANON_CAPSULE_CORE`, when set, overrides
    /// it inside the particle system — see `particles::capsule_core::resolve`.
    pub capsule_core: [f32; 2],
    /// Plexus Tier-1 shape morph: when `Some`, the plexus instance buffer is drawn as
    /// two sub-batches (markers with the morphed node mesh, struts with the morphed
    /// strut mesh) instead of the single cube mesh. The two meshes ride below (uploaded
    /// per frame like the swept mesh; `TubeVertex` shares the `Vertex` layout).
    pub plexus_batches: Option<PlexusBatches>,
    pub plexus_node_verts: &'a [organon_core::math::TubeVertex],
    pub plexus_node_idx: &'a [u32],
    pub plexus_edge_verts: &'a [organon_core::math::TubeVertex],
    pub plexus_edge_idx: &'a [u32],
    /// Plexus OVERLAY Tier-1: same two morphed sub-batches (markers + struts), but
    /// drawn over their OWN instance/tint buffers so they layer on top of the base
    /// surface instead of replacing it. Shares the morph meshes above. `None` = no
    /// overlay this frame. (Tier-2/3 impostor overlay rides the existing
    /// `plexus_node_caps`/`plexus_edge_caps` path.)
    pub plexus_overlay_batches: Option<PlexusBatches>,
    pub plexus_ov_insts: &'a [Mat4],
    pub plexus_ov_tints: &'a [Vec4],
    // Metaball / Voxel field (shared node set + AABB)
    pub meta_nodes: &'a [MetaNode],
    pub meta_min: Vec3,
    pub meta_max: Vec3,
    pub meta_params: &'a MetaballParams,
    /// Field Volume (#348): when non-empty (a `FIELD_RES³` RGBA grid), the Volume
    /// path uploads this CPU-baked analytic field-energy density into the field
    /// texture instead of voxelizing the node point-set (Maxwell/Acoustic → an
    /// analytic density cloud, no far-node scraggle). Empty = the node metaball bake
    /// (Legacy / smoothed-node / every non-Volume path), byte-identical.
    pub field_vol_grid: &'a [Vec4],
    pub voxel_params: &'a VoxelParams,
    // Raymarch params
    pub mandel_params: &'a MandelParams,
    pub creature_params: &'a CreatureParams<'a>,
    pub minimal_params: &'a MinimalParams,
    pub kifs_params: &'a KifsParams,
    pub neural_field_params: &'a NeuralFieldParams,
    pub lens_params: &'a LensParams,
    /// Gaussian Splatting surface look (only consumed when `path == Splat`); reuses
    /// `instances`/`tints` as the splat cloud.
    pub splat_params: SplatParams,
    // Particle aura (drawn over the scene)
    pub particles: &'a ParticlesFrame<'a>,
    /// Hide the generator geometry (it still built the particle stir field).
    pub hide_generator: bool,
    /// Capture decoration (#135 P5): prebuilt axes surface (tubes + cones) + box wall
    /// lines, drawn at the end of the scene pass. Empty = nothing drawn.
    pub axes_solids: &'a [super::axes::SurfVertex],
    pub box_lines: &'a [super::axes::LineVertex],
    /// Field Chamber (#346): prebuilt analyzer-panel surfaces (scope ribbon + spectrum
    /// bars) + frame lines, drawn in the scene pass right after the axes/box. Empty =
    /// panels off / no signal → nothing drawn.
    pub chamber_surfs: &'a [super::chamber::ChVertex],
    pub chamber_lines: &'a [super::chamber::ChLine],
    /// Tier 2 impostor capsules (scope tube + spectrum bars). The PBR/IBL shading
    /// context is filled from `uniforms` in `render`; the visual supplies only the
    /// camera billboard basis, the MaterialType params, and the panel opacity. Empty
    /// beads = flat style / panels off → no impostor draw.
    pub chamber_beads: &'a [super::chamber::ChBead],
    pub chamber_cam_right: [f32; 3],
    pub chamber_cam_up: [f32; 3],
    pub chamber_material: [f32; 4], // mat_type, metallic, roughness, ior
    pub chamber_opacity: f32,
    /// Scenery layer (#187 pivot): a second CONCURRENT instanced draw with its
    /// own material (a second group-0 uniform set, like the liquid's). None =
    /// scenery off.
    pub scenery: Option<SceneryLayer<'a>>,
    /// Scenery water floor (#206 Tier 3): a rippled sheet lofted as a membrane
    /// with its OWN (third) group-0 material — independent of the scenery's.
    /// None = no water.
    pub water: Option<WaterLayer<'a>>,
    /// Demo scene bench (#288): per-(mesh,material) sub-batches partitioning the
    /// instance/tint buffers (opaque first, transmissive last). Empty = not a Demo
    /// frame. When non-empty, the instanced draw is replaced by one draw per batch —
    /// its mesh bound (box/sphere/cylinder) + its own patched group-0 material.
    pub demo_batches: &'a [organon_core::math::DemoBatch],
}

/// The scenery layer's geometry + material (#187 pivot). The material slots
/// patch a copy of the scene `Uniforms` exactly like the liquid material does,
/// so scenery is a different substance without touching the generator's look.
#[derive(Clone, Copy)]
pub struct SceneryLayer<'a> {
    pub instances: &'a [Mat4],
    pub tints: &'a [Vec4],
    /// Swept-tube scenery uses the cylinder mesh (else the cube).
    pub tube: bool,
    /// Scenery membrane skin (#206 Tier 1): the lofted surface (parallel
    /// arrays). Non-empty `mem_idx` ⇒ draw the skin instead of the instances.
    pub mem_pos: &'a [Vec3],
    pub mem_norm: &'a [Vec3],
    pub mem_col: &'a [Vec4],
    pub mem_idx: &'a [u32],
    pub mat_type: f32,
    pub metallic: f32,
    pub roughness: f32,
    pub glow: f32,
    /// HDR self-emission in the scenery's own colour (Material Emissive). 0 = off.
    pub emissive: f32,
    pub opacity: f32,
    pub ior: f32,
    /// 1 = the scenery palette replaces the RGB-cube albedo (tint IS colour).
    pub palette_active: f32,
    pub sss: [f32; 3],
    pub irid: [f32; 3],
    /// Per-material HSV (#305 Tier 1): [effective hue, saturation, value, _] for the
    /// scenery/environment material. Identity [0, 1, 1, _] → byte-identical.
    pub matcol: [f32; 4],
    /// The view-proj for the scenery draws (#187 composite fix). Pure ride:
    /// equals the scene view-proj. Composite (a generator on the orbit rig):
    /// identity view × projection — the corridor is glued to the eye and does
    /// not orbit; its clip depths stay eye-relative, so it composites honestly
    /// against the orbit-viewed generator in the shared depth buffer.
    pub view_proj: [[f32; 4]; 4],
    /// True in composite mode: scenery coordinates are view-locked (not world
    /// space), so the world-space shadow map must skip it.
    pub view_locked: bool,
}

/// The scenery water floor (#206 Tier 3): a rippled membrane sheet at the
/// per-cell water level, spanning the valley, with its OWN material (a third
/// group-0 uniform set — the liquid/scenery pattern). Membrane-only (no
/// instances). Drawn in the scene pass (own material, alpha-blended) and the FX
/// depth prepass (so SSR reflects the fjord walls in the channel — the money
/// shot). Terrain banks occlude it at the shoreline by depth.
#[derive(Clone, Copy)]
pub struct WaterLayer<'a> {
    pub mem_pos: &'a [Vec3],
    pub mem_norm: &'a [Vec3],
    pub mem_col: &'a [Vec4],
    pub mem_idx: &'a [u32],
    pub mat_type: f32,
    pub roughness: f32,
    pub glow: f32,
    pub opacity: f32,
    pub ior: f32,
    /// Per-material HSV (#305 T1) for the water floor — the **scenery** material's
    /// `[hue, hue_cycle·beat, sat, value]`, so the Terra channel water follows the
    /// Scenery Material card (not the generator's `matcol`, which `let mut wu = u`
    /// would otherwise leave in place).
    pub matcol: [f32; 4],
    /// Physical-water shading (#206): depth-absorption strength, sun-glitter
    /// intensity, extra reflectivity. Packed into the water uniform's `sss.xyz`
    /// with a `sss.w = 3` sentinel the cube shader detects → the dedicated water
    /// path (Fresnel reflect + Beer–Lambert depth + glitter), independent of the
    /// scenery material.
    pub absorb: f32,
    pub glitter: f32,
    pub reflect: f32,
    /// The view-proj for the water draws — matches the scenery's (view-locked in
    /// composite, the scene camera in the pure ride). Water isn't drawn in the
    /// shadow pass (a flat sheet below the banks casts nothing useful), so it
    /// needs no `view_locked` flag.
    pub view_proj: [[f32; 4]; 4],
}

/// Light-transport / surface-shading inputs (SSAO, SSR, GI, reaction-diffusion).
#[derive(Clone, Copy)]
pub struct LightTransport<'a> {
    pub ssao_on: bool,
    pub ssao: &'a SsaoParams,
    pub ssr_on: bool,
    pub ssr: &'a SsrParams,
    /// Screen-space GI (#152 Tier 2): one diffuse bounce, composite adds it.
    pub ssgi_on: bool,
    pub ssgi: &'a SsgiParams,
    pub gi_on: bool,
    pub gi_intensity: f32,
    pub gi_falloff: f32,
    pub gi_min: Vec3,
    pub gi_max: Vec3,
    pub gi_probes: &'a [Vec4],
    pub rd_params: &'a RdParams,
    /// Cast shadows (#152 Tier 3): the key-light view-projection + params. `on`
    /// gates the light depth pass; the cube shader darkens the key term where
    /// occluded.
    pub shadow_on: bool,
    pub shadow_light_vp: [[f32; 4]; 4],
    pub shadow_bias: f32,
    pub shadow_strength: f32,
    /// Voxel GI (#152 Tier 3, #10): a world-space bounce marched from the voxelized
    /// node field, added into the HDR buffer. `on` gates the voxelize + gather; the
    /// node set is `Surface.meta_nodes`, the volume the scene bounds (`gi_min/max`).
    pub vxgi_on: bool,
    pub vxgi: VxgiParams,
    /// Emissive cubes as real lights (#167 Tier 3): pick the brightest `ml_count` nodes
    /// and upload them as point lights the cube shader loops (group 3 binding 1). `on`
    /// gates it; `ml_radius` is a fraction of the scene diagonal. Instanced path only.
    pub ml_on: bool,
    pub ml_intensity: f32,
    pub ml_radius: f32,
    pub ml_count: i32,
    /// ReSTIR many-lights (#200 Tier 5d): when true, the emissive-cube light set is
    /// chosen by weighted reservoir sampling (every cube gets a luminance-
    /// proportional chance over time) instead of a hard brightest-`count` cap.
    /// `false` = brightest-N (byte-identical default).
    pub ml_restir: bool,
    /// Hardware-RT shadows (#195 Tier 1): `Some` = trace the screen-space
    /// visibility mask against the visual's TLAS this frame (rides the depth
    /// prepass; cube.wgsl samples it instead of the PCF map). `None` = off —
    /// the byte-identical default path.
    pub rt_shadow: Option<RtShadowFrame<'a>>,
    /// Hardware-RT reflections (#195 Tier 2): `Some` = trace the scene's own
    /// geometry into the SSR/reflection buffer this frame (supersedes the SSR
    /// march while on). `None` = off — the byte-identical default path.
    pub rt_reflect: Option<RtReflectFrame<'a>>,
    /// Hardware-RT ambient occlusion (#195 Tier 3): `Some` = short hemisphere
    /// rays fill the raw-AO target instead of the GTAO march (the AO card's
    /// source switch; requires SSAO enabled). `None` = GTAO — the
    /// byte-identical default path.
    pub rt_ao: Option<RtAoFrame<'a>>,
    /// Hardware-RT diffuse GI (#195 Tier 4): `Some` = gather one indirect
    /// bounce against the TLAS into the SSGI buffer instead of the SSGI march
    /// (supersedes it while on). `None` = off / SSGI — the byte-identical
    /// default path.
    pub rt_gi: Option<RtGiFrame<'a>>,
    /// Beat-aware temporal accumulator (#200 Tier 4½ part 3): `Some` = reproject
    /// + accumulate the RT reflection/GI buffers across frames (the RT pass
    /// writes a raw buffer, the accumulator writes the SSR/SSGI view). `None` =
    /// off — the byte-identical default path.
    pub rt_temporal: Option<RtTemporalFrame>,
    /// RT denoise amount (#200 Tier 4½ part 2): edge-aware à-trous over the
    /// RT-written reflection / GI buffers, in place, before the composite reads
    /// them. `0` = off (byte-identical). Reflections apply it roughness-adaptively
    /// (sharp mirrors untouched); GI applies it in full.
    pub rt_denoise: f32,
    /// Neural denoiser (#200 Tier 5a): when `Some`, the RT reflection / GI
    /// denoise step routes through the kernel-predicting neural filter
    /// (`Post::neural_denoise`) instead of the classical à-trous. `None` = off
    /// (the classical `rt_denoise` path is byte-identical). At `net = 0` the
    /// neural filter reproduces the classical result exactly.
    pub rt_ndenoise: Option<NDenoiseFrame>,
    /// Membrane screen-space FX opt-in (`Shared.membrane_fx[0]`). When true, the
    /// Membrane surface is drawn into the depth prepass so the screen-space effects
    /// (VXGI diffuse + specular, SSAO, SSR, SSGI, DoF, TAA) apply to it too. Off →
    /// membrane skips the prepass (today's look).
    pub membrane_fx: bool,
    /// Progressive path tracer (#200 Tier 4): `Some` = trace the whole image
    /// against the TLAS into the HDR scene buffer (ground truth), progressively
    /// averaged while the camera is still. Replaces the raster scene; the visual
    /// nulls the screen-space light effects when active. `None` = raster (default).
    pub pathtrace: Option<PathtraceFrame<'a>>,
    /// Screen-space refraction (#214 Tier 5 pt 2, `Shared.ssrefr[0..1]`). When the
    /// material is Refractive and `refract_ss > 0`, a post pass reconstructs the
    /// covered pixels from the depth prepass and replaces their env-only refraction
    /// with the displaced RESOLVED SCENE behind them (cubes show their neighbours).
    /// `refract_dist` is the world-space step along the refracted ray. `0` = off
    /// (the pass isn't dispatched → byte-identical).
    pub refract_ss: f32,
    pub refract_dist: f32,
}

/// Fluid Ink (#182 Tier 1): the dye the generator stirs into the fluid medium,
/// rendered as a lit volumetric. `enabled = false` (or an empty `dye_src`) →
/// every dye/ink pass is skipped (byte-identical default). When enabled the
/// fluid solver runs even without the Particle Aura's Fluid tier — the ink is
/// what makes the (otherwise invisible) medium the image.
#[derive(Clone, Copy)]
pub struct InkFrame<'a> {
    pub enabled: bool,
    /// CPU-splatted `res³` dye injection grid (rgb = node colour × ball kernel),
    /// parallel to the particle frame's velocity grid.
    pub dye_src: &'a [Vec4],
    pub dye: DyeParams,
    pub params: InkParams,
}

/// MLS-MPM liquid (#182 Tier 3a): a free-surface liquid the generator churns,
/// rendered through the metaball isosurface path (set Material = Glass for
/// water). `enabled = false` → no sim dispatch, no draw (byte-identical).
#[derive(Clone, Copy)]
pub struct LiquidFrame<'a> {
    pub enabled: bool,
    /// Requested particle count (clamped to `MAX_LIQUID_PARTICLES`).
    pub count: usize,
    /// Sim grid resolution (cubic, 16..96).
    pub grid_res: u32,
    /// The container (an invisible tank) in world space.
    pub container_min: Vec3,
    pub container_max: Vec3,
    /// `grid_res³` node-collider occupancy (xyz = world velocity, w = solid),
    /// or empty when the generator isn't colliding.
    pub colliders: &'a [Vec4],
    /// #247 Tier 3 (energy → liquid): `FIELD_RES³` HDR ember glow (rgb) the Maxwell
    /// energization splats at energized nodes; empty = off (byte-identical).
    pub glow: &'a [Vec4],
    pub dt: f32,
    /// Isosurface raymarch params for the liquid's MetaField (threshold etc.).
    pub surface: MetaballParams,
    pub params: LiquidParams,
    /// #182 T4 follow-up: the liquid's OWN material — the FULL scene material
    /// set, patched into the liquid's uniform copy on its draw only. `None`
    /// follows the scene (byte-identical default).
    pub material: Option<LiquidMaterial>,
    /// #182 T3b: 0 = isosurface (in-scene metaball draw, the default);
    /// 1 = **refractive** — the post-scene see-through pass (Snell + measured
    /// thickness + Beer–Lambert + Fresnel; fetches the resolved scene).
    pub render_mode: u32,
    /// Beer–Lambert absorption strength for the refractive mode.
    pub absorb: f32,
}

/// Fluid light coupling (#182 Tier 4): the dials that make the medium a
/// citizen of the light transport. All 0/false = every pass skipped.
#[derive(Clone, Copy)]
pub struct CouplingFrame {
    /// Fluid → VXGI injection gain (ink radiance + liquid occupancy).
    pub gi: f32,
    /// Dye density attenuates the key light on scene geometry (0..1).
    pub shadow: f32,
    /// The ink march samples the scene shadow map (geometry shades the smoke).
    pub receive: bool,
    /// Fluid velocity → per-node sway springs on the drawn instances (0..1).
    pub sway: f32,
    /// Caustics: key light refracted through the liquid surface (0..2).
    pub caustic: f32,
    pub caustic_sharp: f32,
    /// Ghost light: a hidden generator keeps feeding probe GI, the VXGI
    /// volume and the emissive-cube point lights (a pure GI/light emitter).
    pub ghost: bool,
}

/// The liquid's own material (#182 T4 follow-up) — mirrors the scene's full
/// material dial set; every field lands on the corresponding uniform slot.
#[derive(Clone, Copy)]
pub struct LiquidMaterial {
    /// 0 = Standard, 1 = Chrome, 2 = Glass (cube.wgsl ids).
    pub mat_type: u32,
    pub metallic: f32,
    pub roughness: f32,
    pub ior: f32,
    pub glow: f32,
    pub chrome_purity: f32,
    pub glass_clarity: f32,
    pub f0_override: f32,
    pub dispersion: f32,
    pub glass_caustic: f32,
    pub thin_film: f32,
}

/// All per-frame inputs to `Renderer::render` (besides the wgpu device/queue/view).
#[derive(Clone, Copy)]
pub struct RenderFrame<'a> {
    /// Output size in px (the render targets are `size · render_scale`).
    pub size: (u32, u32),
    /// Global render-resolution scale (0..1); the composite upscales to full-res.
    pub render_scale: f32,
    pub uniforms: &'a Uniforms,
    pub sky_uniforms: &'a SkyUniforms,
    pub post_params: &'a PostParams,
    /// Post-composite creative FX (#152). `enabled = false` → the FX pass is
    /// skipped and the composite writes straight to the view (default, unchanged).
    pub fx: fx::FxParams,
    /// Temporal pass (#152 Tier 2: TAA + motion blur). `enabled = false` → the pass
    /// is skipped and the composite writes straight to the view (default, unchanged).
    pub temporal: TemporalParams,
    /// Scene Kaleidoscope (#361 Tier 1): a post-stage kaleidoscopic fold of the
    /// resolved HDR scene, run before the bloom/composite. `enabled = false` → the
    /// HDR buffer is untouched (byte-identical default).
    pub kaleido: KaleidoParams,
    /// Fluid Ink (#182 Tier 1): dye injection + volumetric render of the medium.
    pub ink: InkFrame<'a>,
    /// MLS-MPM liquid (#182 Tier 3a): the tank the generator churns.
    pub liquid: LiquidFrame<'a>,
    /// Fluid light coupling (#182 Tier 4).
    pub coupling: CouplingFrame,
    pub background: Background<'a>,
    pub surface: Surface<'a>,
    pub light: LightTransport<'a>,
}

pub struct Renderer {
    // Glass/fallback scene pipeline: single-pass, cull None, depth Less + write,
    // alpha blend (both faces for refraction). Used for Glass material + as the
    // safe default whenever the opaque early-Z path is skipped.
    pipeline: wgpu::RenderPipeline,
    // Opaque scene pipeline: cull Back, depth Equal + no write. Shades only the
    // front-most fragments the depth prepass left, so the heavy PBR shader runs
    // ~once per visible pixel instead of once per overlapping fragment.
    pipeline_opaque: wgpu::RenderPipeline,
    // Scenery Skin membrane (#217 review): cull None + LessEqual + write. On the
    // shared-prepass route the FX prepass already wrote the skin's depth, so a
    // plain `Less` scene-pass test would reject it (it disappears); LessEqual
    // passes against its own pre-written depth and is identical to Less on every
    // other route (the skin is closer than the backdrop). cull None keeps the
    // single-sided sheet double-faced, as the blend pipeline does.
    pipeline_skin: wgpu::RenderPipeline,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    // A unit cylinder (along local +Z) for Swept-Tubes mode: the same per-segment
    // instance matrices as Flow-Aligned, but a round cross-section instead of a box.
    cyl_vbuf: wgpu::Buffer,
    cyl_ibuf: wgpu::Buffer,
    cyl_index_count: u32,
    // Neural Tissue meshes (#260 Tier 1): a soma/bouton icosphere + a capped
    // capsule, bound per sub-batch by the multi-mesh instanced draw. The capsule
    // also serves the single-mesh closed-tube fallback for non-graph generators.
    soma_vbuf: wgpu::Buffer,
    soma_ibuf: wgpu::Buffer,
    soma_index_count: u32,
    capsule_vbuf: wgpu::Buffer,
    capsule_ibuf: wgpu::Buffer,
    capsule_index_count: u32,
    // Boids creature meshes (#52): one (vbuf, ibuf, index_count) per `BoidsForm`
    // creature (Fish / Bird / Manta / Dart). When the Boids generator picks a
    // creature form, the instanced draw uses one of these instead of the cube /
    // cylinder — one whole creature per agent, oriented by velocity.
    creature_meshes: Vec<(wgpu::Buffer, wgpu::Buffer, u32)>,
    inst_buf: wgpu::Buffer,
    tint_buf: wgpu::Buffer,
    // organon#217 T1 — the FOURTH instance buffer: per-instance emission (loc 8),
    // parallel to `tint_buf` and grown with it. `emit_hi` = the HIGH-WATER mark: one
    // past the highest instance whose emission may be non-zero since the buffer was
    // last fully clear. Every upload zeroes `[its own length, emit_hi)` and lowers the
    // mark to its length (`emit_upload_plan`), so a stale glyph frame's emission can
    // never survive a shrink to light whatever draws next — not the previous frame's
    // length, which a 100-then-50-then-80 sequence defeats (review on #224).
    // `zero_emit` is the all-zero buffer bound wherever a draw
    // binds a tint buffer that is not `tint_buf` — `white_tint`, the scenery's, the
    // plexus overlay's — and it is kept at least as long as the largest instance
    // count any of them draws, because a fourth layout in the pipeline means a
    // fourth buffer at EVERY draw or wgpu fails validation at draw time (no leg of
    // the bar has a GPU, so this comment is the guard).
    emit_buf: wgpu::Buffer,
    emit_hi: usize,
    zero_emit: wgpu::Buffer,
    inst_cap: usize,
    // Consecutive frames the field has been ≤ ¼ of `inst_cap` (#174 T2 shrink).
    inst_lowwater: u32,
    // Membrane mode: a dynamic world-space triangle mesh (built CPU-side each
    // frame) drawn through the cube pipeline with one identity instance + a tint
    // whose w=0 forces the shader to use the per-vertex colour.
    mem_vbuf: wgpu::Buffer,
    mem_ibuf: wgpu::Buffer,
    mem_vcap: usize,
    mem_icap: usize,
    // Contiguous Swept-Tubes: one dynamic welded mesh per frame (u32 indices, layout
    // matches Vertex), drawn like the membrane sheet (identity instance + white_tint).
    swept_vbuf: wgpu::Buffer,
    swept_ibuf: wgpu::Buffer,
    swept_vcap: usize,
    swept_icap: usize,
    // Plexus Tier-1 shape morph: two dynamic morphed meshes (node cube→sphere, strut
    // square→circle) uploaded per frame like the swept mesh (u32 indices, Vertex layout),
    // drawn as two instanced sub-batches over the plexus instance buffer.
    plexus_node_vbuf: wgpu::Buffer,
    plexus_node_ibuf: wgpu::Buffer,
    plexus_node_vcap: usize,
    plexus_node_icap: usize,
    plexus_edge_vbuf: wgpu::Buffer,
    plexus_edge_ibuf: wgpu::Buffer,
    plexus_edge_vcap: usize,
    plexus_edge_icap: usize,
    // Live index counts of the two morph meshes uploaded this frame (draw ranges).
    plexus_node_icount: u32,
    plexus_edge_icount: u32,
    // Plexus OVERLAY Tier-1: its own instance/tint buffers so the markers+struts draw
    // on top of the base surface instead of replacing `inst_buf`/`tint_buf`.
    plexus_ov_inst_buf: wgpu::Buffer,
    plexus_ov_tint_buf: wgpu::Buffer,
    plexus_ov_inst_cap: usize,
    // Reused Vertex packing scratch (#174 T2 — was a fresh Vec every frame).
    mem_scratch: Vec<Vertex>,
    identity_inst: wgpu::Buffer,
    white_tint: wgpu::Buffer,
    ubuf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    // #182 T4 follow-up: the liquid's own group-0 uniforms — a copy of the
    // scene uniforms with the liquid-material overrides patched in, so the
    // liquid is a different substance without touching the generator.
    liquid_ubuf: wgpu::Buffer,
    liquid_bind: wgpu::BindGroup,
    // Scenery layer (#187 pivot): a second instance/tint pair + its own group-0
    // material uniforms (the liquid pattern), drawn concurrently in the scene
    // pass + the depth/shadow passes.
    scenery_ubuf: wgpu::Buffer,
    scenery_bind: wgpu::BindGroup,
    // Plexus overlay group-0 uniforms: the scene `u` with node bevel (`shape`) zeroed,
    // so the overlay's own morph meshes aren't double-morphed while the base cubes bevel.
    plexus_ov_ubuf: wgpu::Buffer,
    plexus_ov_bind: wgpu::BindGroup,
    // Demo scene bench (#288): a small pool of per-material group-0 uniform buffers
    // + binds. The Demo generator draws each (mesh, material) sub-batch with its own
    // patched Uniforms bound from this pool (the scenery/water patch pattern, one per
    // material). Sized once; scenes use ≤ this many distinct materials.
    demo_ubufs: Vec<wgpu::Buffer>,
    demo_binds: Vec<wgpu::BindGroup>,
    // Scenery membrane skin (#206 Tier 1): its own lofted mesh buffers, drawn
    // with the scenery uniforms (like the main membrane, second copy).
    scenery_mem_vbuf: wgpu::Buffer,
    scenery_mem_ibuf: wgpu::Buffer,
    scenery_mem_vcap: usize,
    scenery_mem_icap: usize,
    scenery_inst_buf: wgpu::Buffer,
    scenery_tint_buf: wgpu::Buffer,
    scenery_cap: usize,
    // Scenery water floor (#206 Tier 3): its OWN group-0 material uniforms + a
    // lofted-sheet mesh (membrane-only), drawn in the scene + FX prepass.
    water_ubuf: wgpu::Buffer,
    water_bind: wgpu::BindGroup,
    water_mem_vbuf: wgpu::Buffer,
    water_mem_ibuf: wgpu::Buffer,
    water_mem_vcap: usize,
    water_mem_icap: usize,
    ibl_layout: wgpu::BindGroupLayout,
    sky_env_layout: wgpu::BindGroupLayout,
    sky_pipeline: wgpu::RenderPipeline,
    // Opaque-path skybox: depth Equal + no write, so it fills only the background
    // (where the prepass left the cleared far depth) without clobbering geometry.
    sky_pipeline_eq: wgpu::RenderPipeline,
    sky_ubuf: wgpu::Buffer,
    sky_bind: wgpu::BindGroup,
    env: Environment,
    post: Post,
    // Post-composite creative FX (#152): a final pass (NPR / DoF / lens FX / grade /
    // feedback) on the composited image. Engaged only when its master is on.
    fx: Fx,
    // Temporal pass (#152 Tier 2: TAA + motion blur), a final pass on the composited
    // image. Engaged only when its master is on.
    temporal: Temporal,
    // Metaball isosurface mode: a 3D field voxelizer + raymarch, used instead of
    // the instanced cube/cylinder draw when the mode is Metaball.
    meta: MetaField,
    // Voxel mode: splat the node set into a 3D occupancy grid + DDA-raymarch crisp
    // cubes, used instead of the instanced draw when the mode is Voxel.
    vox: VoxField,
    // Mandelbulb mode: an analytic distance-estimated fractal raymarch (no nodes),
    // used instead of the instanced draw when the Mandelbulb generator is active.
    mandel: MandelField,
    // Creature Engine (#476 Tier 1): a union-of-SDF-primitives sea creature
    // raymarch (no nodes), used instead of the instanced draw when active.
    creature: CreatureField,
    // Creature anatomy overlay (#476 Tier 2c): the projected spine/ring/limb diagram,
    // a line pass drawn over the creature in the scene buffer. Inert when off.
    creature_overlay: CreatureOverlay,
    // Minimal-surface mode (#127): a raymarched TPMS isosurface (gyroid etc.; no
    // nodes), used instead of the instanced draw when that generator is active.
    minimal: MinimalField,
    // Kaleidoscopic Fractal mode: a fullscreen per-pixel field (no nodes), used
    // instead of the instanced draw when the Kaleidoscope generator is active.
    kifs: KifsField,
    // Scene Kaleidoscope (#361 Tier 1): a post-stage kaleidoscopic fold of the
    // resolved HDR scene (folds the live PBR render of ANY generator), run before
    // the bloom/composite. Inert unless enabled.
    kaleido: Kaleido,
    // Neural-field mode (#200 Tier 1): a raymarched MLP isosurface (no nodes),
    // used instead of the instanced draw when the NeuralField generator is active.
    neural: NeuralField,
    // Lens mode (#258 Tier 3): a raymarched analytic lens SDF (no nodes), used
    // instead of the instanced draw when the Lens generator is active.
    lens: LensField,
    // Reaction–diffusion surface patterning (sampled by the cube + metaball
    // shaders as triplanar emissive dapple).
    rd: RdField,
    depth_view: wgpu::TextureView,
    depth_size: (u32, u32),
    // Bumped whenever the depth textures are recreated; combined with the
    // shared-prepass route bit it keys the cached screen-space-FX bind groups.
    depth_epoch: u64,
    // Kept so the scene pipelines + depth can be rebuilt when the MSAA sample
    // count changes at runtime.
    cube_shader: wgpu::ShaderModule,
    cube_pl: wgpu::PipelineLayout,
    // group(0) uniform layout, kept so the MSAA-matched opaque prepass pipeline
    // can be rebuilt on a sample-count change.
    uniform_layout: wgpu::BindGroupLayout,
    sky_shader: wgpu::ShaderModule,
    sky_pl: wgpu::PipelineLayout,
    sample_count: u32,
    /// #618 T3: which `material_maps` variant the cube pipelines are currently
    /// compiled for. Kept beside `sample_count` because it is the same kind of fact:
    /// a pipeline-baked choice that a rebuild has to honour.
    material_maps: bool,
    // SSAO depth prepass: a single-sample, sampleable depth target + a depth-only
    // pipeline (reuses the cube vertex shader). Only used when SSAO is enabled.
    prepass_pipeline: wgpu::RenderPipeline,
    prepass_depth_view: wgpu::TextureView,
    // Opaque early-Z prepass: a depth-only pipeline at the scene's MSAA sample
    // count writing the shared `depth_view`. Rebuilt on sample-count change.
    opaque_prepass_pipeline: wgpu::RenderPipeline,
    // Infinite terrain backdrop (raymarched). Drawn instead of the skybox when
    // enabled; its own pass mirrors the skybox's Always/Equal depth variants.
    terrain: Terrain,
    // #102B FFT (Tessendorf) ocean — the CPU spectrum + the per-frame tile texture
    // the terrain water shader samples. Updated each frame when the ocean is on.
    ocean: Ocean,
    // HDR starfield (+ sun). Drawn after the background in the scene pass; its
    // pipelines mirror the skybox's Always/Equal depth variants.
    stars: Stars,
    // Particle Aura (#81): GPU motes advected through the generator's velocity
    // field, drawn additively into the scene HDR buffer. Inert when disabled.
    particles: ParticleSystem,
    splats: SplatSystem,
    // Aura-Fluid (#81 showpiece): a persistent GPU Navier–Stokes field the motes
    // ride in the Fluid tier. Inert in Off/Lite.
    fluid: FluidSim,
    // Fluid Ink (#182 Tier 1): the dye's 3D texture + volumetric march + depth-
    // aware upsample onto the HDR buffer. Inert when the ink is off.
    fluidvis: FluidVis,
    // MLS-MPM liquid (#182 Tier 3a): the particle solver + a SECOND MetaField
    // whose 3D texture the density splat fills — the existing isosurface
    // raymarch then draws the liquid surface with the full material stack.
    liquid_sim: LiquidSim,
    // Fluid light coupling (#182 T4): light-space transmittance/caustic map +
    // the two-way sway pass; `inst_gen` bumps when `inst_buf` is recreated
    // (the sway bind group holds it).
    fluidlight: fluidlight::FluidLight,
    sway: sway::Sway,
    // Refractive liquid surface (#182 T3b route B, first slice).
    liquidsurf: liquidsurf::LiquidSurf,
    refractsurf: refractsurf::RefractSurf,
    inst_gen: u64,
    liquid_meta: MetaField,
    liquid_field_gen: u64,
    // Bounced-GI irradiance probe volume (#80 Part B): group(3) on the cube
    // pipeline. Filled CPU-side from the node field each frame; inert at 0.
    gi: GiVolume,
    // Voxel GI (#152 Tier 3, #10): voxelize the node field + march it, adding a
    // world-space bounce into the HDR buffer. Self-contained; inert when off.
    vxgi: Vxgi,
    // Cast-shadow map (#152 Tier 3): a key-light depth map PCF-sampled by cube.wgsl
    // (group 4). Instanced path only.
    shadow: Shadow,
    // #472 Tier 1: the material PBR texture set bound as group(5) on the cube
    // pipeline. Neutral built-in set until a folder is loaded; sampled only when
    // Uniforms.mtl.x is on (the generator cube draw).
    material: MaterialTextures,
    // #472 Tier 2: the compute baker that fills one channel of `material` from a
    // procedural noise layer (superseding the PNG load for that channel).
    material_baker: MaterialBaker,
    // #195 Tier 1: the RT shadow-mask pass. Lazily created on the first frame
    // that asks for RT shadows (needs the ray-query feature, implied by the
    // TLAS that arrives with the request); None forever on non-RT machines.
    rt_shadow: Option<rt_shadow::RtShadow>,
    // #195 Tier 2: the RT reflections pass (same lazy-creation contract).
    rt_reflect_pass: Option<rt_reflect::RtReflect>,
    // #195 Tier 3: the RT AO pass (same lazy-creation contract).
    rt_ao_pass: Option<rt_ao::RtAo>,
    // #195 Tier 4: the RT GI gather pass (same lazy-creation contract).
    rt_gi_pass: Option<rt_gi::RtGi>,
    // #200 Tier 4: the progressive path tracer (same lazy-creation contract).
    pathtracer: Option<rt_pathtrace::PathTracer>,
    // Capture decoration (#135 P5): XYZ axes + wireframe box, a line pass in the
    // scene buffer. Inert (no verts) when disabled.
    axes: super::axes::Axes,
    chamber: super::chamber::Chamber,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, color_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cube"),
            source: wgpu::ShaderSource::Wgsl(include_str!("cube.wgsl").into()),
        });

        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform-layout"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform-bind"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubuf.as_entire_binding(),
            }],
        });
        let liquid_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquid-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let liquid_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid-uniform-bind"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: liquid_ubuf.as_entire_binding(),
            }],
        });
        // Scenery layer (#187 pivot): its own material uniforms on the shared
        // group-0 layout (the liquid pattern) + its own instance/tint pair.
        let scenery_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scenery-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scenery_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scenery-uniform-bind"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: scenery_ubuf.as_entire_binding(),
            }],
        });
        // Plexus overlay (Tier-1): its own group-0 uniforms — a copy of the scene `u`
        // with the node bevel (`shape`) forced to 0, so the overlay's OWN cube→sphere
        // morph meshes aren't double-morphed by the generator's bevel while the base
        // cubes underneath still bevel (both share the main uniform otherwise).
        let plexus_ov_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plexus-overlay-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let plexus_ov_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("plexus-overlay-uniform-bind"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: plexus_ov_ubuf.as_entire_binding(),
            }],
        });
        // Demo scene bench (#288): a pool of per-material group-0 uniform buffers +
        // binds (the scenery pattern, one slot per distinct material in a scene).
        const DEMO_MAT_SLOTS: usize = 16;
        let mut demo_ubufs = Vec::with_capacity(DEMO_MAT_SLOTS);
        let mut demo_binds = Vec::with_capacity(DEMO_MAT_SLOTS);
        for i in 0..DEMO_MAT_SLOTS {
            let ub = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("demo-uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("demo-uniform-bind"),
                layout: &bind_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
            });
            let _ = i;
            demo_ubufs.push(ub);
            demo_binds.push(bind);
        }
        let scenery_cap = 4096usize;
        let scenery_inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scenery-instances"),
            size: (scenery_cap * std::mem::size_of::<Mat4>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scenery_tint_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scenery-tints"),
            size: (scenery_cap * std::mem::size_of::<Vec4>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Scenery membrane skin (#206 Tier 1): grow-on-demand mesh buffers.
        let scenery_mem_vcap = 4096usize;
        let scenery_mem_icap = 8192usize;
        let scenery_mem_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scenery-membrane-verts"),
            size: (scenery_mem_vcap * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scenery_mem_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scenery-membrane-idx"),
            size: (scenery_mem_icap * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Scenery water floor (#206 Tier 3): its own material uniforms on the
        // shared group-0 layout (the liquid/scenery pattern) + a lofted sheet.
        let water_ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let water_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water-uniform-bind"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: water_ubuf.as_entire_binding(),
            }],
        });
        let water_mem_vcap = 4096usize;
        let water_mem_icap = 8192usize;
        let water_mem_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water-membrane-verts"),
            size: (water_mem_vcap * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let water_mem_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water-membrane-idx"),
            size: (water_mem_icap * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibl_layout = Environment::ibl_layout(device);
        let sky_env_layout = Environment::sky_env_layout(device);

        // Reaction–diffusion surface field (group 2 on the cube pipeline, group 3
        // on the metaball ray pipeline). Built before the pipeline layouts since
        // they reference its bind-group layout.
        let rd = RdField::new(device);

        // Bounced-GI probe volume (#80 Part B): group(3) on the cube pipeline.
        let gi = GiVolume::new(device, queue);
        let vxgi = Vxgi::new(device);

        // Fluid light coupling (#182 T4): the light-space transmittance/caustic
        // map lives on the shadow group, so build it first.
        let fluidlight = fluidlight::FluidLight::new(device);
        // Cast-shadow map (#152 Tier 3): group(4) on the cube pipeline. Built before
        // the layout since it references the shadow bind-group layout.
        let shadow =
            Shadow::new(device, queue, &bind_layout, fluidlight.map_view(), fluidlight.sampler());
        // #472 Tier 1: the material texture set — group(5) on the cube pipeline.
        let material = MaterialTextures::new(device, queue);
        // #618 T3: a fresh Renderer has no material folder loaded (`present_mask` 0),
        // so every cube pipeline is built with the material block compiled out. It is
        // rebuilt on the first load that actually brings channels in — see
        // `sync_material_specialisation`.
        let material_maps = material.present_mask != 0;
        // #472 Tier 2: the procedural compute baker.
        let material_baker = MaterialBaker::new(device);
        let sway = sway::Sway::new(device);
        let liquidsurf = liquidsurf::LiquidSurf::new(device, &ibl_layout, post::HDR_FORMAT);
        let refractsurf = refractsurf::RefractSurf::new(device, post::HDR_FORMAT);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cube-layout"),
            bind_group_layouts: &[
                Some(&bind_layout),
                Some(&ibl_layout),
                Some(rd.scene_layout()),
                Some(gi.layout()),
                Some(shadow.layout()),
                Some(&material.layout), // #472 Tier 1: group(5) material texture set
            ],
            immediate_size: 0,
        });

        // Scene pipelines start single-sample; the visual calls `set_sample_count`
        // with the live MSAA param, which rebuilds them.
        let sample_count = 1u32;

        // Glass/fallback (cull None, Less + write) and opaque (cull Back, Equal +
        // no write) variants of the scene pipeline.
        let pipeline = make_cube_pipeline(
            device, &shader, &pipeline_layout, sample_count,
            None, wgpu::CompareFunction::Less, true, material_maps,
        );
        let pipeline_opaque = make_cube_pipeline(
            device, &shader, &pipeline_layout, sample_count,
            Some(wgpu::Face::Back), wgpu::CompareFunction::Equal, false, material_maps,
        );
        // Scenery Skin membrane variant: cull None, LessEqual + write (#217 review).
        let pipeline_skin = make_cube_pipeline(
            device, &shader, &pipeline_layout, sample_count,
            None, wgpu::CompareFunction::LessEqual, true, material_maps,
        );

        // Skybox pipeline (group(0)=sky UBO, group(1)=env tex+sampler) + the
        // procedural environment (always-on fallback until an .hdr is loaded).
        // The skybox also draws into the HDR buffer, so it uses HDR_FORMAT.
        let (sky_shader, sky_pl, sky_pipeline, sky_ubuf, sky_bind) =
            build_skybox(device, &sky_env_layout, sample_count);
        // Opaque-path skybox variant (Equal + no write): fills only the background.
        let sky_pipeline_eq = make_sky_pipeline(
            device, &sky_shader, &sky_pl, sample_count, wgpu::CompareFunction::Equal, false,
        );
        let env = Environment::procedural(device, queue, &ibl_layout, &sky_env_layout);

        // HDR post-processing (bloom + tonemap composite) outputs to the surface.
        let post = Post::new(device, color_format);
        // Post-composite creative FX (#152): targets the same surface format as the
        // composite; rebuilt with it on the HDR toggle (set_surface_format).
        let fx = Fx::new(device, color_format);
        // Temporal pass (#152 Tier 2): targets the surface format like the composite;
        // rebuilt with it on the HDR toggle.
        let temporal = Temporal::new(device, color_format);

        // Metaball mode: reuses group(0)=uniforms + group(1)=IBL for shading, and
        // renders into the same linear HDR buffer + depth as the scene pass.
        let meta = MetaField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            gi.layout(),
            vxgi.sample_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );

        // MLS-MPM liquid (#182 Tier 3a): its own MetaField instance — the
        // density splat writes the field texture, the same raymarch draws it.
        let liquid_meta = MetaField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            gi.layout(),
            vxgi.sample_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );
        let liquid_sim = LiquidSim::new(device);

        // Voxel mode: reuses group(0)=uniforms for the camera + key/fill lights and
        // group(1)=IBL maps for the full PBR/material shade; group(2) is its own
        // splatted field + raymarch params. Renders into the same linear HDR buffer
        // + depth as the scene pass.
        let vox = VoxField::new(device, &bind_layout, &ibl_layout, post::HDR_FORMAT, DEPTH_FORMAT, sample_count);

        // Mandelbulb mode: reuses group(0)=uniforms + group(1)=IBL + group(3)=RD
        // for shading, raymarching into the same linear HDR buffer + depth.
        let mandel = MandelField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );

        // Creature Engine (#476 Tier 1): same shared bind groups as Mandelbulb
        // (group 0 = uniforms, 1 = IBL, 3 = RD), raymarching a union-of-SDF
        // sea creature into the same linear HDR buffer + depth.
        let creature = CreatureField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );
        // Creature anatomy overlay (#476 Tier 2c): a line pass in the scene buffer.
        let creature_overlay = CreatureOverlay::new(device, sample_count);

        // Minimal-surface mode (#127): same shared bind groups as Mandelbulb
        // (group 0 = uniforms, 1 = IBL, 3 = RD), raymarching a TPMS isosurface
        // into the same linear HDR buffer + depth.
        let minimal = MinimalField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );

        // Kaleidoscopic Fractal mode: a fullscreen field (group 0 = its own params)
        // plus a 3-D raymarch path (group 0 = cube uniforms, 1 = IBL, 2 = params),
        // both painting linear HDR into the scene.
        let kifs = KifsField::new(device, post::HDR_FORMAT, DEPTH_FORMAT, sample_count);

        // Scene Kaleidoscope (#361 Tier 1): a post-stage fold of the resolved HDR
        // scene (single-sample, post-resolve → no depth / no MSAA needed).
        let kaleido = Kaleido::new(device, post::HDR_FORMAT);

        // Neural-field mode (#200 Tier 1): same shared bind groups as Mandelbulb
        // (group 0 = uniforms, 1 = IBL, 3 = RD), raymarching an MLP isosurface
        // into the same linear HDR buffer + depth.
        let neural = NeuralField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );

        // Lens mode (#258 Tier 3): same shared bind groups as Mandelbulb
        // (group 0 = uniforms, 1 = IBL, 3 = RD), sphere-tracing the lens SDF
        // into the same linear HDR buffer + depth.
        let lens = LensField::new(
            device,
            &bind_layout,
            &ibl_layout,
            rd.scene_layout(),
            post::HDR_FORMAT,
            DEPTH_FORMAT,
            sample_count,
        );

        let (verts, indices) = cube_mesh();
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube-verts"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube-idx"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let (cyl_verts, cyl_indices) = cyl_mesh();
        let cyl_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cyl-verts"),
            contents: bytemuck::cast_slice(&cyl_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cyl_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cyl-idx"),
            contents: bytemuck::cast_slice(&cyl_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Neural Tissue meshes (#260 Tier 1): soma/bouton icosphere + capped capsule.
        let (soma_verts, soma_indices) = soma_mesh();
        let soma_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("soma-verts"),
            contents: bytemuck::cast_slice(&soma_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let soma_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("soma-idx"),
            contents: bytemuck::cast_slice(&soma_indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let (capsule_verts, capsule_indices) = capsule_mesh();
        let capsule_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("capsule-verts"),
            contents: bytemuck::cast_slice(&capsule_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let capsule_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("capsule-idx"),
            contents: bytemuck::cast_slice(&capsule_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Boids creature meshes — built once, instanced per agent (#52).
        let creature_meshes: Vec<(wgpu::Buffer, wgpu::Buffer, u32)> = (0..CREATURE_KINDS)
            .map(|k| {
                let (cv, ci) = creature_mesh(k);
                let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("creature-verts"),
                    contents: bytemuck::cast_slice(&cv),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("creature-idx"),
                    contents: bytemuck::cast_slice(&ci),
                    usage: wgpu::BufferUsages::INDEX,
                });
                (vbuf, ibuf, ci.len() as u32)
            })
            .collect();

        let inst_cap = 4096;
        // STORAGE so the sway pass (#182 T4) can displace translations in place.
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (inst_cap * std::mem::size_of::<Mat4>()) as u64,
            usage: RT_HIT_BUFFER_USAGE,
            mapped_at_creation: false,
        });

        // Membrane mesh buffers (grow on demand) + a 1-element identity instance
        // and a (1,1,1,0) tint so the cube shader uses the mesh's per-vertex colour.
        let mem_vcap = 4096;
        let mem_icap = 8192;
        let mem_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("membrane-verts"),
            size: (mem_vcap * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mem_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("membrane-idx"),
            size: (mem_icap * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Contiguous Swept-Tubes welded mesh buffers (grow on demand), reusing the
        // membrane identity-instance + white tint below for the single-instance draw.
        let swept_vcap = 4096;
        let swept_icap = 8192;
        let swept_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("swept-verts"),
            size: (swept_vcap * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let swept_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("swept-idx"),
            size: (swept_icap * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Plexus shape-morph node + strut meshes (grow on demand). Small (a subdivided
        // cube ≈ 300 verts, a prism ≈ 32), so tiny initial caps.
        let plexus_node_vcap = 512;
        let plexus_node_icap = 1024;
        let plexus_edge_vcap = 64;
        let plexus_edge_icap = 128;
        let mk_vbuf = |label: &str, cap: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (cap * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let mk_ibuf = |label: &str, cap: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (cap * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let plexus_node_vbuf = mk_vbuf("plexus-node-verts", plexus_node_vcap);
        let plexus_node_ibuf = mk_ibuf("plexus-node-idx", plexus_node_icap);
        let plexus_edge_vbuf = mk_vbuf("plexus-edge-verts", plexus_edge_vcap);
        let plexus_edge_ibuf = mk_ibuf("plexus-edge-idx", plexus_edge_icap);
        let identity_inst = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("membrane-identity-instance"),
            contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let white_tint = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("membrane-white-tint"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 1.0, 0.0]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let tint_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tints"),
            size: (inst_cap * std::mem::size_of::<Vec4>()) as u64,
            usage: RT_HIT_BUFFER_USAGE,
            mapped_at_creation: false,
        });
        // organon#217 T1 — per-instance emission, parallel to `tint_buf`. wgpu zero-
        // initialises a fresh buffer, so until a frame uploads emission every instance
        // reads `vec4(0)` and the shader's added term is exactly zero (invariant #4).
        let emit_buf = make_emit_buf(device, "emits", inst_cap);
        // The all-zero emission bound beside every tint buffer that is not `tint_buf`.
        // Sized to the instance capacity here; `ensure_zero_emit` regrows it whenever a
        // scenery / plexus-overlay upload would draw more instances than it covers.
        let zero_emit = make_emit_buf(device, "zero-emits", inst_cap);

        // Plexus overlay instance/tint buffers (grow on demand; overlay-only path).
        let plexus_ov_inst_cap = 256usize;
        let plexus_ov_inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plexus-overlay-instances"),
            size: (plexus_ov_inst_cap * std::mem::size_of::<Mat4>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let plexus_ov_tint_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("plexus-overlay-tints"),
            size: (plexus_ov_inst_cap * std::mem::size_of::<Vec4>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_size = (1, 1);
        let depth_view = make_depth(device, depth_size, sample_count);
        // SSAO prepass (single-sample, no cull) + opaque early-Z prepass (MSAA,
        // cull Back to match the opaque scene pipeline).
        let prepass_pipeline =
            make_depth_prepass_pipeline(device, &shader, &bind_layout, &material.layout, 1, None, material_maps);
        let opaque_prepass_pipeline = make_depth_prepass_pipeline(
            device, &shader, &bind_layout, &material.layout, sample_count, Some(wgpu::Face::Back),
            material_maps,
        );
        let prepass_depth_view = make_prepass_depth(device, depth_size);

        let mut terrain = Terrain::new(device, queue, post::HDR_FORMAT, DEPTH_FORMAT, sample_count);
        // #102B: create the FFT ocean and bind its tile into the terrain water shader.
        let ocean = Ocean::new(device, OceanParams::default(), 0x0cea_1234);
        terrain.set_ocean(device, ocean.view(), ocean.sampler());
        let stars = Stars::new(device, post::HDR_FORMAT, DEPTH_FORMAT, sample_count);

        // Particle Aura: renders into the same linear HDR buffer + depth as the
        // scene pass (additive sparks, or opaque IBL-shaded beads — #298 Tier 1), so
        // it shares the format + MSAA sample count. Beads bind the shared IBL group.
        let particles =
            ParticleSystem::new(device, &ibl_layout, post::HDR_FORMAT, DEPTH_FORMAT, sample_count);
        // Gaussian Splatting surface: draws anisotropic Gaussians into the same linear
        // HDR buffer + depth as the scene pass; the lit tier binds the shared IBL group.
        let splats =
            SplatSystem::new(device, &ibl_layout, post::HDR_FORMAT, DEPTH_FORMAT, sample_count);
        // Aura-Fluid solver (compute-only; no swapchain/MSAA dependency).
        let fluid = FluidSim::new(device);
        // Fluid Ink (#182 Tier 1): needs the IBL layout for the irradiance ambient.
        let fluidvis = FluidVis::new(device, &ibl_layout);
        // #346 Field Chamber: built here (before `ibl_layout` is moved into the struct)
        // so the impostor pass can bind the shared IBL group.
        let chamber = super::chamber::Chamber::new(device, &ibl_layout, sample_count);

        Renderer {
            pipeline,
            pipeline_opaque,
            pipeline_skin,
            vbuf,
            ibuf,
            index_count: indices.len() as u32,
            cyl_vbuf,
            cyl_ibuf,
            cyl_index_count: cyl_indices.len() as u32,
            soma_vbuf,
            soma_ibuf,
            soma_index_count: soma_indices.len() as u32,
            capsule_vbuf,
            capsule_ibuf,
            capsule_index_count: capsule_indices.len() as u32,
            creature_meshes,
            inst_buf,
            tint_buf,
            emit_buf,
            emit_hi: 0,
            zero_emit,
            inst_cap,
            inst_lowwater: 0,
            mem_vbuf,
            mem_ibuf,
            mem_vcap,
            mem_icap,
            swept_vbuf,
            swept_ibuf,
            swept_vcap,
            swept_icap,
            plexus_node_vbuf,
            plexus_node_ibuf,
            plexus_node_vcap,
            plexus_node_icap,
            plexus_edge_vbuf,
            plexus_edge_ibuf,
            plexus_edge_vcap,
            plexus_edge_icap,
            plexus_node_icount: 0,
            plexus_edge_icount: 0,
            plexus_ov_inst_buf,
            plexus_ov_tint_buf,
            plexus_ov_inst_cap,
            mem_scratch: Vec::new(),
            identity_inst,
            white_tint,
            ubuf,
            bind_group,
            liquid_ubuf,
            liquid_bind,
            scenery_ubuf,
            scenery_bind,
            plexus_ov_ubuf,
            plexus_ov_bind,
            demo_ubufs,
            demo_binds,
            scenery_mem_vbuf,
            scenery_mem_ibuf,
            water_ubuf,
            water_bind,
            water_mem_vbuf,
            water_mem_ibuf,
            water_mem_vcap,
            water_mem_icap,
            scenery_mem_vcap,
            scenery_mem_icap,
            scenery_inst_buf,
            scenery_tint_buf,
            scenery_cap,
            ibl_layout,
            sky_env_layout,
            sky_pipeline,
            sky_pipeline_eq,
            sky_ubuf,
            sky_bind,
            env,
            post,
            fx,
            temporal,
            meta,
            vox,
            mandel,
            creature,
            creature_overlay,
            minimal,
            kifs,
            kaleido,
            neural,
            lens,
            rd,
            depth_view,
            depth_size,
            depth_epoch: 0,
            cube_shader: shader,
            cube_pl: pipeline_layout,
            uniform_layout: bind_layout,
            sky_shader,
            sky_pl,
            sample_count,
            material_maps,
            prepass_pipeline,
            prepass_depth_view,
            opaque_prepass_pipeline,
            terrain,
            ocean,
            stars,
            particles,
            splats,
            fluid,
            fluidvis,
            liquid_sim,
            liquid_meta,
            liquid_field_gen: 0,
            gi,
            vxgi,
            shadow,
            material,
            material_baker,
            rt_shadow: None,
            rt_reflect_pass: None,
            rt_ao_pass: None,
            rt_gi_pass: None,
            pathtracer: None,
            fluidlight,
            sway,
            liquidsurf,
            refractsurf,
            inst_gen: 0,
            axes: super::axes::Axes::new(device, sample_count),
            chamber,
        }
    }

    /// Replace the terrain noise tile (editor noise-type / seed change). `data` is
    /// row-major `[0,1]`, length `terrain::NOISE_DIM²`.
    pub fn set_terrain_noise(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[f32]) {
        self.terrain.set_noise(device, queue, data);
    }

    /// #102B: synthesise + upload the FFT ocean tile for this frame. The visual calls
    /// this each frame before `render`; it's a no-op cost when `enabled` is false.
    pub fn update_ocean(&mut self, queue: &wgpu::Queue, enabled: bool, params: OceanParams, time: f32) {
        if enabled {
            self.ocean.set_params(params);
            self.ocean.update(queue, time);
        }
    }

    /// Set the MSAA sample count (1/2/4/8). Rebuilds the scene + sky pipelines for
    /// the new sample count, recreates the depth target, and tells `post` to
    /// rebuild its multisampled scene color. A no-op if unchanged.
    pub fn set_sample_count(&mut self, device: &wgpu::Device, n: u32) {
        let n = n.max(1);
        if n == self.sample_count {
            return;
        }
        self.sample_count = n;
        let material_maps = self.material_maps;
        self.pipeline = make_cube_pipeline(
            device, &self.cube_shader, &self.cube_pl, n,
            None, wgpu::CompareFunction::Less, true, material_maps,
        );
        self.pipeline_opaque = make_cube_pipeline(
            device, &self.cube_shader, &self.cube_pl, n,
            Some(wgpu::Face::Back), wgpu::CompareFunction::Equal, false, material_maps,
        );
        self.pipeline_skin = make_cube_pipeline(
            device, &self.cube_shader, &self.cube_pl, n,
            None, wgpu::CompareFunction::LessEqual, true, material_maps,
        );
        self.sky_pipeline = make_sky_pipeline(
            device, &self.sky_shader, &self.sky_pl, n, wgpu::CompareFunction::Always, true,
        );
        self.sky_pipeline_eq = make_sky_pipeline(
            device, &self.sky_shader, &self.sky_pl, n, wgpu::CompareFunction::Equal, false,
        );
        self.opaque_prepass_pipeline = make_depth_prepass_pipeline(
            device, &self.cube_shader, &self.uniform_layout, &self.material.layout, n,
            Some(wgpu::Face::Back), material_maps,
        );
        self.depth_view = make_depth(device, self.depth_size, n);
        self.depth_epoch = self.depth_epoch.wrapping_add(1);
        self.post.set_sample_count(n);
        self.meta.set_sample_count(device, n);
        self.liquid_meta.set_sample_count(device, n);
        self.vox.set_sample_count(device, n);
        self.mandel.set_sample_count(device, n);
        self.creature.set_sample_count(device, n);
        self.creature_overlay.set_sample_count(device, n);
        self.minimal.set_sample_count(device, n);
        self.kifs.set_sample_count(device, n);
        self.neural.set_sample_count(device, n);
        self.lens.set_sample_count(device, n);
        self.terrain.set_sample_count(device, post::HDR_FORMAT, DEPTH_FORMAT, n);
        self.stars.set_sample_count(device, post::HDR_FORMAT, DEPTH_FORMAT, n);
        self.particles.set_sample_count(device, n);
        self.splats.set_sample_count(device, n);
        self.axes.set_sample_count(device, n);
        self.chamber.set_sample_count(device, n);
    }

    /// Load an .hdr (Some) or generate the procedural sky (None / load failure),
    /// re-run IBL precompute, and rebuild both bind groups.
    pub fn load_environment(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: Option<&std::path::Path>,
    ) {
        let source = match path {
            Some(p) => env::EnvSource::Hdr(p),
            None => env::EnvSource::Procedural(env::DEFAULT_SKY),
        };
        self.env = Environment::build(device, queue, source, &self.ibl_layout, &self.sky_env_layout);
    }

    /// (Re)load the #472 Tier 1 material texture set from a folder of PNGs (albedo /
    /// normal / roughness / metallic / AO / height). `dir = None` unloads back to the
    /// neutral built-in set. The visual calls this when `Shared.material_gen` bumps.
    pub fn load_material(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, dir: Option<&str>) {
        self.material.load(device, queue, dir);
        self.sync_material_specialisation(device);
    }

    /// #618 Tier 3: keep the compiled cube pipelines matching whether any material
    /// channel is actually loaded, rebuilding them only when that crosses.
    ///
    /// This is the whole cost of the specialisation, and it is bounded: loading or
    /// clearing a material folder is a user action, not a per-frame one, so the
    /// rebuild happens at most once per such action and never during steady state.
    /// Rebuilt here for the same reason `set_sample_count` rebuilds them — a pipeline
    /// bakes this choice at creation, so a changed choice means new pipelines.
    ///
    /// ⚠️ Both variants must draw identically while no map is present. The `false`
    /// variant skips a block whose every output is multiplied by zero in that state,
    /// and `m_albedo`/`has_alb` default to the identity for the base-colour resolve —
    /// so this is a compile-time removal of dead work, not a second look. Proving that
    /// on pixels needs the frame harness; `verify.sh` is where that claim gets tested.
    fn sync_material_specialisation(&mut self, device: &wgpu::Device) {
        let want = self.material.present_mask != 0;
        if want == self.material_maps {
            return;
        }
        self.material_maps = want;
        let n = self.sample_count;
        self.pipeline = make_cube_pipeline(
            device, &self.cube_shader, &self.cube_pl, n,
            None, wgpu::CompareFunction::Less, true, want,
        );
        self.pipeline_opaque = make_cube_pipeline(
            device, &self.cube_shader, &self.cube_pl, n,
            Some(wgpu::Face::Back), wgpu::CompareFunction::Equal, false, want,
        );
        self.pipeline_skin = make_cube_pipeline(
            device, &self.cube_shader, &self.cube_pl, n,
            None, wgpu::CompareFunction::LessEqual, true, want,
        );
        self.prepass_pipeline = make_depth_prepass_pipeline(
            device, &self.cube_shader, &self.uniform_layout, &self.material.layout, 1, None, want,
        );
        self.opaque_prepass_pipeline = make_depth_prepass_pipeline(
            device, &self.cube_shader, &self.uniform_layout, &self.material.layout, n,
            Some(wgpu::Face::Back), want,
        );
    }

    /// #472 Tier 2/3: (re)bake the procedural material — composite the layer stack per
    /// channel + derive normal/AO — and splice the baked set into the group(5)
    /// textures (superseding any loaded PNGs). `layer` is the base layer
    /// (`Shared.material_layer`, whose [16] is the global procedural flag, [17] the
    /// bake resolution); `layer2` is the Tier-3 overlay (`Shared.material_layer2`,
    /// [16] enabled / [17] blend). `derive` = `Shared.material_derive`. Only
    /// re-dispatches when the graph changes; the visual calls this each frame while
    /// procedural is on and restores the PNG/neutral set on the falling edge.
    pub fn bake_material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &[f32; 18],
        grad: &[f32; 8],
        layer2: &[f32; 18],
        grad2: &[f32; 8],
        derive: &[f32; 8],
    ) {
        // Layer 1 (base) is always enabled while procedural is on (its [16] is the
        // global procedural flag); layer 2's [16] is its own enable.
        let layers = [
            MaterialBaker::layer_u(layer, grad, 1.0),
            MaterialBaker::layer_u(layer2, grad2, layer2[16]),
        ];
        let res = (layer[17] as u32).clamp(64, 2048);
        let (changed, mask) = self.material_baker.bake(device, queue, &layers, derive, res, false);
        // Re-point the set at the baked views when the bake changed, or when the set
        // isn't currently showing this procedural mask (first enable / a PNG load
        // clobbered it). Steady state is a no-op.
        if changed || self.material.present_mask != mask {
            let c = &self.material_baker.channels;
            let refs = [&c[0].1, &c[1].1, &c[2].1, &c[3].1, &c[4].1, &c[5].1];
            self.material.set_procedural(device, mask, refs);
        }
        // #618 T3: the procedural path moves `present_mask` too (0 ↔ non-zero on
        // first enable / full disable), so it needs the same rebuild as a PNG load.
        // A no-op in steady state, which is where this spends all its time.
        self.sync_material_specialisation(device);
    }

    /// The runtime bitfield of which material channel maps are currently loaded
    /// (0 = the neutral built-in set → the cube shader keeps the scalar path). Fed
    /// into `Uniforms.mtl2.w` each frame.
    pub fn material_present_mask(&self) -> f32 {
        self.material.present_mask as f32
    }

    /// The channel textures the procedural bake last wrote, in the bind order of
    /// `MaterialTextures::CHANNELS` — albedo, normal, roughness, metallic, AO, height.
    ///
    /// **Why this exists.** `bake_material` is public and `Renderer::new` takes only a device,
    /// a queue and a format — no surface, no window — so another wgpu application can already
    /// construct a `Renderer` on **its own** device and drive the bake. What it could not do was
    /// reach the result: the textures live in a private field of the private `MaterialBaker`.
    /// A downstream renderer therefore had to reimplement the bake to use it, which is exactly
    /// the fork this crate's licence split exists to avoid.
    ///
    /// 📌 **Views, not pixels, and that is the point.** A caller on the same `wgpu::Device`
    /// binds these directly; nothing is copied and no readback is involved. That is why the
    /// targets need no `COPY_SRC` and why this is an accessor rather than a transfer path. A
    /// caller that genuinely wants the bytes on the CPU needs `COPY_SRC` adding to
    /// `make_target` — deliberately not done here, because nothing in this repo needs it and an
    /// unused usage flag costs every target its fast paths.
    ///
    /// ⚠️ **The contents are only meaningful after a `bake_material` call**, and they are
    /// replaced by the next one. Before the first bake these are allocated but never written.
    ///
    /// ⚠️ **Bind order is `CHANNELS`', not `channel_slot`'s.** `channel_slot` maps a *shader*
    /// channel ordinal to a slot and a present bit and is not in this order; reading one as the
    /// other silently swaps roughness and metallic.
    pub fn baked_material_views(&self) -> impl Iterator<Item = &wgpu::TextureView> {
        self.material_baker.channels.iter().map(|(_, view)| view)
    }

    /// The edge of every texture [`Renderer::baked_material_views`] returns. Square, and the
    /// same for all of them.
    ///
    /// 📌 Offered because a caller binding these has to size its own pipeline against them, and
    /// the alternative is guessing at the bake resolution — which is set from
    /// `Shared.material_layer[17]` and is therefore not a constant a caller can assume.
    pub fn baked_material_resolution(&self) -> u32 {
        self.material_baker.res
    }

    /// Bake the physically based atmosphere (#100) into the env equirect + re-run
    /// the IBL precompute, so the cubes are lit by the derived sky at the current
    /// sun angle. The visual calls this when the atmosphere params or (quantized)
    /// sun direction change.
    pub fn load_atmosphere(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params: env::AtmosphereParams,
    ) {
        let source = env::EnvSource::Atmosphere(params);
        self.env = Environment::build(device, queue, source, &self.ibl_layout, &self.sky_env_layout);
    }

    /// Rebuild the composite pipeline for a new swapchain format (HDR toggle:
    /// sRGB 8-bit ↔ `Rgba16Float`). Only the final composite pass touches the
    /// surface, so nothing else needs rebuilding.
    pub fn set_surface_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        self.post.set_surface_format(device, format);
        // The FX pass also targets the surface (its src + history textures match the
        // surface format), so rebuild it for the new format too.
        self.fx.set_surface_format(device, format);
        // The temporal pass also targets the surface (its src + history match the
        // surface format), so rebuild it for the new format too.
        self.temporal.set_surface_format(device, format);
    }

    /// Neural Tissue (#260 Tier 1): draw the three sub-batches — somata / capsules /
    /// boutons — as separate instanced draws, binding the matching mesh for each and
    /// slicing the shared instance/tint buffers at the sub-batch offsets. The pipeline
    /// + bind groups are set by the caller (identical across sub-batches); only the
    /// vertex/index buffers + instance range differ. Used by the FX prepass, the
    /// early-Z prepass, the shadow pass, and the scene pass so FX/shadows are correct.
    fn draw_neural_batches<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, nb: &NeuralBatches) {
        let m4 = std::mem::size_of::<Mat4>() as u64;
        let v4 = std::mem::size_of::<Vec4>() as u64;
        let soma = nb.soma_count as u64;
        let caps = nb.capsule_count as u64;
        // Somata → icosphere, instances [0, soma).
        if nb.soma_count > 0 {
            rp.set_vertex_buffer(0, self.soma_vbuf.slice(..));
            rp.set_vertex_buffer(1, self.inst_buf.slice(..));
            rp.set_vertex_buffer(2, self.tint_buf.slice(..));
            rp.set_vertex_buffer(3, self.emit_buf.slice(..));
            rp.set_index_buffer(self.soma_ibuf.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..self.soma_index_count, 0, 0..nb.soma_count);
        }
        // Capsules → capped tube, instances [soma, soma+caps).
        if nb.capsule_count > 0 {
            rp.set_vertex_buffer(0, self.capsule_vbuf.slice(..));
            rp.set_vertex_buffer(1, self.inst_buf.slice(soma * m4..));
            rp.set_vertex_buffer(2, self.tint_buf.slice(soma * v4..));
            rp.set_vertex_buffer(3, self.emit_buf.slice(soma * v4..));
            rp.set_index_buffer(self.capsule_ibuf.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..self.capsule_index_count, 0, 0..nb.capsule_count);
        }
        // Boutons → small icosphere, instances [soma+caps, total).
        if nb.bouton_count > 0 {
            let off = soma + caps;
            rp.set_vertex_buffer(0, self.soma_vbuf.slice(..));
            rp.set_vertex_buffer(1, self.inst_buf.slice(off * m4..));
            rp.set_vertex_buffer(2, self.tint_buf.slice(off * v4..));
            rp.set_vertex_buffer(3, self.emit_buf.slice(off * v4..));
            rp.set_index_buffer(self.soma_ibuf.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..self.soma_index_count, 0, 0..nb.bouton_count);
        }
    }

    /// Plexus Tier-1 shape morph (#plexus): draw the markers `[0, markers)` with the
    /// morphed node mesh and the struts `[markers, markers+struts)` with the morphed
    /// strut mesh, re-basing the per-instance vertex buffers by a byte offset (the
    /// same trick as `draw_neural_batches`). Meshes were uploaded this frame. u32
    /// indices (the morph meshes are `TubeMesh`).
    fn draw_plexus_batches<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, pb: &PlexusBatches) {
        self.draw_plexus_batches_from(rp, pb, &self.inst_buf, &self.tint_buf, &self.emit_buf);
    }

    /// organon#217 T1 — keep `zero_emit` at least `n` instances long. The buffer is
    /// bound at slot 3 beside every tint buffer that is not `tint_buf`, and wgpu
    /// validates at draw time that an instance-stepped buffer covers the instance
    /// range, so the all-zero buffer has to be as long as the longest such draw. A
    /// fresh buffer is zero (wgpu zero-initialises), which is the whole content.
    fn ensure_zero_emit(&mut self, device: &wgpu::Device, n: usize) {
        let want = (n.max(1) * std::mem::size_of::<Vec4>()) as u64;
        if self.zero_emit.size() < want {
            self.zero_emit = make_emit_buf(device, "zero-emits", n.max(1));
        }
    }

    /// Draw the plexus markers+struts sub-batches over an explicit instance/tint pair.
    /// The standalone surface passes the main buffers; the overlay passes its own so
    /// the web layers on top of the base surface instead of replacing it.
    /// `emit_buf` is the slot-3 emission beside them — the main buffer for the
    /// surface, `zero_emit` for the overlay.
    fn draw_plexus_batches_from<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        pb: &PlexusBatches,
        inst_buf: &'a wgpu::Buffer,
        tint_buf: &'a wgpu::Buffer,
        emit_buf: &'a wgpu::Buffer,
    ) {
        let m4 = std::mem::size_of::<Mat4>() as u64;
        let v4 = std::mem::size_of::<Vec4>() as u64;
        if pb.markers > 0 {
            rp.set_vertex_buffer(0, self.plexus_node_vbuf.slice(..));
            rp.set_vertex_buffer(1, inst_buf.slice(..));
            rp.set_vertex_buffer(2, tint_buf.slice(..));
            rp.set_vertex_buffer(3, emit_buf.slice(..));
            rp.set_index_buffer(self.plexus_node_ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..self.plexus_node_icount, 0, 0..pb.markers);
        }
        if pb.struts > 0 {
            let off = pb.markers as u64;
            rp.set_vertex_buffer(0, self.plexus_edge_vbuf.slice(..));
            rp.set_vertex_buffer(1, inst_buf.slice(off * m4..));
            rp.set_vertex_buffer(2, tint_buf.slice(off * v4..));
            rp.set_vertex_buffer(3, emit_buf.slice(off * v4..));
            rp.set_index_buffer(self.plexus_edge_ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.draw_indexed(0..self.plexus_edge_icount, 0, 0..pb.struts);
        }
    }

    /// Demo scene bench (#288): the vertex/index buffers + index count for a
    /// primitive mesh kind (box / icosphere / unit cylinder).
    fn demo_mesh(&self, mesh: organon_core::math::DemoMesh) -> (&wgpu::Buffer, &wgpu::Buffer, u32) {
        match mesh {
            organon_core::math::DemoMesh::Box => (&self.vbuf, &self.ibuf, self.index_count),
            organon_core::math::DemoMesh::Sphere => (&self.soma_vbuf, &self.soma_ibuf, self.soma_index_count),
            organon_core::math::DemoMesh::Cylinder => (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count),
        }
    }

    /// Demo geometry pass (#288): draw each sub-batch's mesh over its slice of the
    /// shared instance/tint buffers. `opaque_only` skips transmissive batches (used
    /// by the depth prepass + shadow pass, which want opaque occluders only). The
    /// pipeline + bind groups are set by the caller (this is material-agnostic).
    fn draw_demo_geometry<'a>(
        &'a self,
        rp: &mut wgpu::RenderPass<'a>,
        batches: &[organon_core::math::DemoBatch],
        opaque_only: bool,
    ) {
        let m4 = std::mem::size_of::<Mat4>() as u64;
        let v4 = std::mem::size_of::<Vec4>() as u64;
        let mut off = 0u64; // running instance offset
        for bt in batches.iter() {
            if bt.count == 0 {
                continue;
            }
            if !(opaque_only && bt.material.is_transmissive()) {
                let (vbuf, ibuf, icount) = self.demo_mesh(bt.mesh);
                rp.set_vertex_buffer(0, vbuf.slice(..));
                rp.set_vertex_buffer(1, self.inst_buf.slice(off * m4..));
                rp.set_vertex_buffer(2, self.tint_buf.slice(off * v4..));
                rp.set_vertex_buffer(3, self.emit_buf.slice(off * v4..));
                rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                rp.draw_indexed(0..icount, 0, 0..bt.count);
            }
            off += bt.count as u64;
        }
    }

    /// Demo scene pass (#288): draw each sub-batch with its OWN patched group-0
    /// material (per-primitive materials, Tier 2) via the single-pass blend pipeline
    /// (Less + write — correct for opaque and, since opaque batches come first and
    /// glass last, adequate for the reference scene). Binds 1–4 are set by the caller.
    fn draw_demo_scene<'a>(&'a self, rp: &mut wgpu::RenderPass<'a>, batches: &[organon_core::math::DemoBatch]) {
        let m4 = std::mem::size_of::<Mat4>() as u64;
        let v4 = std::mem::size_of::<Vec4>() as u64;
        let mut off = 0u64;
        // LessEqual + write (the `pipeline_skin` config): correct whether or not a
        // depth prepass pre-wrote this geometry's depth (equal test passes), so the
        // demo composites cleanly in both the plain and shared-prepass routes.
        rp.set_pipeline(&self.pipeline_skin);
        for (i, bt) in batches.iter().enumerate() {
            if bt.count == 0 {
                continue;
            }
            let slot = i.min(self.demo_binds.len().saturating_sub(1));
            rp.set_bind_group(0, &self.demo_binds[slot], &[]);
            let (vbuf, ibuf, icount) = self.demo_mesh(bt.mesh);
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_vertex_buffer(1, self.inst_buf.slice(off * m4..));
            rp.set_vertex_buffer(2, self.tint_buf.slice(off * v4..));
            rp.set_vertex_buffer(3, self.emit_buf.slice(off * v4..));
            rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..icount, 0, 0..bt.count);
            off += bt.count as u64;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        frame: &RenderFrame,
    ) {
        // #104: unpack the frame into the names the body below already uses
        // (every field is Copy: scalars + shared refs), so the body is unchanged.
        let RenderFrame {
            size,
            render_scale,
            uniforms,
            sky_uniforms,
            post_params,
            fx: fxp,
            temporal: tp,
            kaleido,
            ink,
            liquid,
            coupling,
            background,
            surface,
            light,
        } = *frame;
        // The terrain pass also runs in ocean-only mode (landscape off, ocean on) to
        // draw the sky + sea; the shader's `land_on` flag gates the actual terrain.
        let Background { terrain_on, terrain_u, terrain_scale, stars_on, star_sun, star_u, ocean_on } =
            background;
        let terrain_pass = terrain_on || ocean_on;
        let Surface {
            path,
            instances,
            tints,
            emits,
            rt_instances,
            rt_tints,
            tube,
            neural_batches,
            neural_capsule,
            swept,
            swept_verts,
            swept_idx,
            creature,
            mem_pos,
            mem_norm,
            mem_col,
            mem_idx,
            show_strands,
            membrane_arms,
            arm_caps,
            plexus_impostor,
            plexus_node_caps,
            plexus_edge_caps,
            plexus_node_mat,
            plexus_edge_mat,
            capsule_core,
            plexus_batches,
            plexus_node_verts,
            plexus_node_idx,
            plexus_edge_verts,
            plexus_edge_idx,
            plexus_overlay_batches,
            plexus_ov_insts,
            plexus_ov_tints,
            meta_nodes,
            meta_min,
            meta_max,
            meta_params,
            field_vol_grid,
            voxel_params,
            mandel_params,
            creature_params,
            minimal_params,
            kifs_params,
            neural_field_params,
            lens_params,
            splat_params,
            particles,
            hide_generator,
            axes_solids,
            box_lines,
            chamber_surfs,
            chamber_lines,
            chamber_beads,
            chamber_cam_right,
            chamber_cam_up,
            chamber_material,
            chamber_opacity,
            scenery,
            water,
            demo_batches,
        } = surface;
        // Demo scene bench (#288): live when the Demo generator emitted sub-batches.
        let demo_live = !demo_batches.is_empty();
        // Scenery layer (#187 pivot): live when it has geometry this frame.
        let scenery_count = scenery.map(|sc| sc.instances.len()).unwrap_or(0);
        // Live when it has instances OR a membrane skin (#206 Tier 1).
        let scenery_live =
            scenery_count > 0 || scenery.map(|sc| !sc.mem_idx.is_empty()).unwrap_or(false);
        // Scenery water floor (#206 Tier 3): live when its sheet has triangles.
        let water_mem_icount = water.map(|w| w.mem_idx.len()).unwrap_or(0);
        let water_live = water_mem_icount > 0;
        let LightTransport {
            ssao_on,
            ssao,
            ssr_on,
            ssr,
            ssgi_on,
            ssgi,
            gi_on,
            gi_intensity,
            gi_falloff,
            gi_min,
            gi_max,
            gi_probes,
            rd_params,
            shadow_on,
            shadow_light_vp,
            shadow_bias,
            shadow_strength,
            vxgi_on,
            vxgi,
            ml_on,
            ml_intensity,
            ml_radius,
            ml_count,
            ml_restir,
            rt_shadow,
            rt_reflect,
            rt_ao,
            rt_gi,
            rt_temporal,
            rt_denoise,
            rt_ndenoise,
            membrane_fx,
            pathtrace,
            refract_ss,
            refract_dist,
        } = light;
        let pathtrace_active = pathtrace.is_some();
        // The old mutually-exclusive mode bools, derived from the single path so the
        // dispatch logic in the body stays byte-for-byte the same.
        let metaball = path == RenderPath::Metaball;
        let volume = path == RenderPath::Volume;
        let voxel = path == RenderPath::Voxel;
        let mandelbulb = path == RenderPath::Mandelbulb;
        let creature_ray = path == RenderPath::Creature;
        let minimal = path == RenderPath::MinimalSurface;
        let kifs_on = path == RenderPath::Kifs;
        let neural_on = path == RenderPath::NeuralField;
        let lens_on = path == RenderPath::Lens;
        let membrane = path == RenderPath::Membrane;
        // Gaussian Splatting surface: the node set is drawn as anisotropic Gaussians
        // instead of the instanced cubes. It reuses `instances`/`tints` but replaces
        // the cube draw + its depth prepass, so it's gated out of both below.
        let splat_on = path == RenderPath::Splat;
        // The composite upscales the scaled render buffer into the full-res `view`,
        // so the post-composite FX (#152) and temporal (#152 Tier 2) passes operate at
        // the FULL output size, not the scaled render size. Capture it before `size` is
        // shadowed below.
        let full_size = size;
        // Shadow `size` with the scaled render resolution: every internal target
        // (depth, SSAO prepass, scene HDR, bloom chain, terrain/metaball/mandelbulb
        // passes) is built at this size, and the composite samples them by UV into
        // the full-res `view` — i.e. renders low, presents native, upscaled.
        let size = scaled_render_size(size, render_scale);
        if size != self.depth_size && size.0 > 0 && size.1 > 0 {
            self.depth_size = size;
            self.depth_view = make_depth(device, size, self.sample_count);
            self.prepass_depth_view = make_prepass_depth(device, size);
            // Invalidates the cached screen-space-FX bind groups (#174 T2).
            self.depth_epoch = self.depth_epoch.wrapping_add(1);
        }
        // The instance buffers hold the RT/PT hit-shading geometry: `rt_instances`
        // when present (welded Swept Tubes → the per-segment cylinder approximation
        // the ray tracer traces while the raster draws the welded mesh), else the
        // raster's own `instances`. The raster instanced draws count off
        // `instances.len()` (0 in welded), so populating the buffer here doesn't
        // draw anything extra.
        let up_inst: &[Mat4] = if !rt_instances.is_empty() { rt_instances } else { instances };
        let up_tint: &[Vec4] = if !rt_tints.is_empty() { rt_tints } else { tints };
        // Grow-or-shrink the instance buffers. Shrink (#174 T2): the caps only ever
        // grew, so one large-field experiment permanently pinned its VRAM (~84 MB
        // after a 1M-node run) — release when the field has been ≤ ¼ of the cap
        // for a few hundred consecutive frames.
        if up_inst.len() > self.inst_cap {
            self.inst_cap = up_inst.len().next_power_of_two();
            self.inst_lowwater = 0;
        } else if self.inst_cap > 4096 && up_inst.len() * 4 < self.inst_cap {
            self.inst_lowwater += 1;
            if self.inst_lowwater >= 300 {
                self.inst_cap = up_inst.len().next_power_of_two().max(4096);
                self.inst_lowwater = 0;
            } else {
                // keep the current cap this frame
            }
        } else {
            self.inst_lowwater = 0;
        }
        let want_bytes = (self.inst_cap * std::mem::size_of::<Mat4>()) as u64;
        if self.inst_buf.size() != want_bytes {
            self.inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: want_bytes,
                usage: RT_HIT_BUFFER_USAGE,
                mapped_at_creation: false,
            });
            self.inst_gen = self.inst_gen.wrapping_add(1);
            self.tint_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tints"),
                size: (self.inst_cap * std::mem::size_of::<Vec4>()) as u64,
                usage: RT_HIT_BUFFER_USAGE,
                mapped_at_creation: false,
            });
            // Grown with `tint_buf`, and fresh = zero (see `make_emit_buf`).
            self.emit_buf = make_emit_buf(device, "emits", self.inst_cap);
            self.emit_hi = 0;
            self.ensure_zero_emit(device, self.inst_cap);
        }
        // Upload only when a GPU path actually consumes the instance buffers this
        // frame (#174 T2): the raymarch/bake modes (metaball / volume / voxel /
        // mandelbulb / implicit-minimal / KIFS) and the hidden-generator case read
        // the node set CPU-side only — the unconditional upload was ~80 MB/frame
        // of wasted traffic at 1M nodes in those modes.
        let inst_gpu_used = !up_inst.is_empty()
            && !hide_generator
            && !(metaball || volume || voxel || mandelbulb || creature_ray || minimal || kifs_on || neural_on || lens_on);
        if inst_gpu_used {
            queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(up_inst));
            // `up_tint` is built parallel to `up_inst` (same length).
            queue.write_buffer(&self.tint_buf, 0, bytemuck::cast_slice(up_tint));
            // organon#217 T1 — emission rides beside the tints. Uploaded only when the
            // caller handed a slice exactly `up_inst.len()` long (the glyph frame); any
            // other length — including the empty slice every other frame passes — means
            // "no emission". Whatever this upload does NOT cover, up to the HIGH-WATER
            // mark of everything lit since the last full clear, is zeroed back —
            // `emit_upload_plan` decides the range, and its tests pin the property that
            // no index ever lit survives a shrink. (The first version zeroed only the
            // previous frame's length: a glyph frame of 100 followed by one of 50 left
            // 50..100 lit, and a later 80-instance generator draw read it — review on
            // #224.) A fresh buffer is already zero (wgpu zero-initialises), so the
            // common case writes nothing at all.
            let lit = if emits.len() == up_inst.len() && !emits.is_empty() { emits.len() } else { 0 };
            let (zero, high) = emit_upload_plan(self.emit_hi, lit);
            if lit > 0 {
                queue.write_buffer(&self.emit_buf, 0, bytemuck::cast_slice(emits));
            }
            let zero = zero.start.min(self.inst_cap)..zero.end.min(self.inst_cap);
            if !zero.is_empty() {
                let zeros = vec![Vec4::ZERO; zero.len()];
                let off = (zero.start * std::mem::size_of::<Vec4>()) as u64;
                queue.write_buffer(&self.emit_buf, off, bytemuck::cast_slice(&zeros));
            }
            self.emit_hi = high;
        }

        // Scenery layer (#187 pivot): grow-and-upload its instance/tint pair.
        if let Some(sc) = scenery {
            if scenery_live {
                if sc.instances.len() > self.scenery_cap {
                    self.scenery_cap = sc.instances.len().next_power_of_two();
                    self.scenery_inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("scenery-instances"),
                        size: (self.scenery_cap * std::mem::size_of::<Mat4>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.scenery_tint_buf = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("scenery-tints"),
                        size: (self.scenery_cap * std::mem::size_of::<Vec4>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    // The scenery draws bind `zero_emit` at slot 3; it must cover them.
                    self.ensure_zero_emit(device, self.scenery_cap);
                }
                queue.write_buffer(&self.scenery_inst_buf, 0, bytemuck::cast_slice(sc.instances));
                queue.write_buffer(&self.scenery_tint_buf, 0, bytemuck::cast_slice(sc.tints));
            }
        }

        // Membrane: pack the parallel arrays into the cube `Vertex` layout and
        // upload (growing the buffers as needed). World-space positions → drawn
        // with the identity instance.
        let mem_icount = if membrane { mem_idx.len() } else { 0 };
        if membrane && !mem_idx.is_empty() {
            if mem_pos.len() > self.mem_vcap {
                self.mem_vcap = mem_pos.len().next_power_of_two();
                self.mem_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("membrane-verts"),
                    size: (self.mem_vcap * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            if mem_idx.len() > self.mem_icap {
                self.mem_icap = mem_idx.len().next_power_of_two();
                self.mem_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("membrane-idx"),
                    size: (self.mem_icap * std::mem::size_of::<u32>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            // Pack into a reused scratch Vec (#174 T2 — this allocated a fresh
            // Vec<Vertex> every Membrane frame).
            self.mem_scratch.clear();
            self.mem_scratch.extend((0..mem_pos.len()).map(|i| Vertex {
                pos: mem_pos[i].to_array(),
                normal: mem_norm[i].to_array(),
                color: [mem_col[i].x, mem_col[i].y, mem_col[i].z],
            }));
            queue.write_buffer(&self.mem_vbuf, 0, bytemuck::cast_slice(&self.mem_scratch));
            queue.write_buffer(&self.mem_ibuf, 0, bytemuck::cast_slice(mem_idx));
        }

        // Contiguous Swept-Tubes: grow (if needed) + upload the dynamic welded mesh.
        // `TubeVertex` shares `Vertex`'s (pos, normal, color) layout, so it uploads
        // straight into a Vertex-layout buffer with no repacking.
        // `hide_generator` (Hide Generator) must suppress the welded mesh too — it's the
        // generator geometry in Contiguous mode, just as `draw_instances` is the
        // per-segment geometry otherwise (both gated the same, see below).
        let draw_swept = swept && !swept_idx.is_empty() && !hide_generator;
        if draw_swept {
            if swept_verts.len() > self.swept_vcap {
                self.swept_vcap = swept_verts.len().next_power_of_two();
                self.swept_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("swept-verts"),
                    size: (self.swept_vcap * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            if swept_idx.len() > self.swept_icap {
                self.swept_icap = swept_idx.len().next_power_of_two();
                self.swept_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("swept-idx"),
                    size: (self.swept_icap * std::mem::size_of::<u32>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&self.swept_vbuf, 0, bytemuck::cast_slice(swept_verts));
            queue.write_buffer(&self.swept_ibuf, 0, bytemuck::cast_slice(swept_idx));
        }

        // Plexus Tier-1 shape morph: upload the two morphed meshes (node + strut) when
        // active — for the standalone surface OR the overlay (both draw the same meshes).
        // `TubeVertex` shares the `Vertex` layout, so it casts straight in.
        if plexus_batches.is_some() || plexus_overlay_batches.is_some() {
            if plexus_node_verts.len() > self.plexus_node_vcap {
                self.plexus_node_vcap = plexus_node_verts.len().next_power_of_two();
                self.plexus_node_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plexus-node-verts"),
                    size: (self.plexus_node_vcap * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            if plexus_node_idx.len() > self.plexus_node_icap {
                self.plexus_node_icap = plexus_node_idx.len().next_power_of_two();
                self.plexus_node_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plexus-node-idx"),
                    size: (self.plexus_node_icap * std::mem::size_of::<u32>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            if plexus_edge_verts.len() > self.plexus_edge_vcap {
                self.plexus_edge_vcap = plexus_edge_verts.len().next_power_of_two();
                self.plexus_edge_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plexus-edge-verts"),
                    size: (self.plexus_edge_vcap * std::mem::size_of::<Vertex>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            if plexus_edge_idx.len() > self.plexus_edge_icap {
                self.plexus_edge_icap = plexus_edge_idx.len().next_power_of_two();
                self.plexus_edge_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plexus-edge-idx"),
                    size: (self.plexus_edge_icap * std::mem::size_of::<u32>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&self.plexus_node_vbuf, 0, bytemuck::cast_slice(plexus_node_verts));
            queue.write_buffer(&self.plexus_node_ibuf, 0, bytemuck::cast_slice(plexus_node_idx));
            queue.write_buffer(&self.plexus_edge_vbuf, 0, bytemuck::cast_slice(plexus_edge_verts));
            queue.write_buffer(&self.plexus_edge_ibuf, 0, bytemuck::cast_slice(plexus_edge_idx));
            self.plexus_node_icount = plexus_node_idx.len() as u32;
            self.plexus_edge_icount = plexus_edge_idx.len() as u32;
        }

        // Plexus OVERLAY Tier-1: upload the markers+struts instance/tint data into the
        // overlay's own buffers (grown on demand) so they draw over the base surface.
        if plexus_overlay_batches.is_some() && !plexus_ov_insts.is_empty() {
            if plexus_ov_insts.len() > self.plexus_ov_inst_cap {
                self.plexus_ov_inst_cap = plexus_ov_insts.len().next_power_of_two();
                self.plexus_ov_inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plexus-overlay-instances"),
                    size: (self.plexus_ov_inst_cap * std::mem::size_of::<Mat4>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.plexus_ov_tint_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("plexus-overlay-tints"),
                    size: (self.plexus_ov_inst_cap * std::mem::size_of::<Vec4>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                // The overlay draws bind `zero_emit` at slot 3; it must cover them.
                self.ensure_zero_emit(device, self.plexus_ov_inst_cap);
            }
            queue.write_buffer(&self.plexus_ov_inst_buf, 0, bytemuck::cast_slice(plexus_ov_insts));
            queue.write_buffer(&self.plexus_ov_tint_buf, 0, bytemuck::cast_slice(plexus_ov_tints));
        }

        // Scenery membrane skin (#206 Tier 1): pack + upload the scenery loft
        // into its own mesh buffers (mirrors the main membrane; reuses the
        // scratch — the main membrane already uploaded above). Drawn with the
        // scenery uniforms at each pass below.
        let scenery_mem_icount = scenery.map(|sc| sc.mem_idx.len()).unwrap_or(0);
        if let Some(sc) = scenery {
            if scenery_live && scenery_mem_icount > 0 {
                if sc.mem_pos.len() > self.scenery_mem_vcap {
                    self.scenery_mem_vcap = sc.mem_pos.len().next_power_of_two();
                    self.scenery_mem_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("scenery-membrane-verts"),
                        size: (self.scenery_mem_vcap * std::mem::size_of::<Vertex>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                if sc.mem_idx.len() > self.scenery_mem_icap {
                    self.scenery_mem_icap = sc.mem_idx.len().next_power_of_two();
                    self.scenery_mem_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("scenery-membrane-idx"),
                        size: (self.scenery_mem_icap * std::mem::size_of::<u32>()) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                self.mem_scratch.clear();
                self.mem_scratch.extend((0..sc.mem_pos.len()).map(|i| Vertex {
                    pos: sc.mem_pos[i].to_array(),
                    normal: sc.mem_norm[i].to_array(),
                    color: [sc.mem_col[i].x, sc.mem_col[i].y, sc.mem_col[i].z],
                }));
                queue.write_buffer(&self.scenery_mem_vbuf, 0, bytemuck::cast_slice(&self.mem_scratch));
                queue.write_buffer(&self.scenery_mem_ibuf, 0, bytemuck::cast_slice(sc.mem_idx));
            }
        }

        // Scenery water floor (#206 Tier 3): pack + upload the rippled sheet into
        // its own mesh buffers (mirrors the scenery membrane; reuses the scratch).
        if let Some(w) = water {
            if water_live {
                if w.mem_pos.len() > self.water_mem_vcap {
                    self.water_mem_vcap = w.mem_pos.len().next_power_of_two();
                    self.water_mem_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("water-membrane-verts"),
                        size: (self.water_mem_vcap * std::mem::size_of::<Vertex>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                if w.mem_idx.len() > self.water_mem_icap {
                    self.water_mem_icap = w.mem_idx.len().next_power_of_two();
                    self.water_mem_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("water-membrane-idx"),
                        size: (self.water_mem_icap * std::mem::size_of::<u32>()) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
                self.mem_scratch.clear();
                self.mem_scratch.extend((0..w.mem_pos.len()).map(|i| Vertex {
                    pos: w.mem_pos[i].to_array(),
                    normal: w.mem_norm[i].to_array(),
                    color: [w.mem_col[i].x, w.mem_col[i].y, w.mem_col[i].z],
                }));
                queue.write_buffer(&self.water_mem_vbuf, 0, bytemuck::cast_slice(&self.mem_scratch));
                queue.write_buffer(&self.water_mem_ibuf, 0, bytemuck::cast_slice(w.mem_idx));
            }
        }

        let mut u = *uniforms;
        u.mat[3] = self.env.prefilter_mips() as f32; // shader does roughness*(count-1)
        // Node bevel scoping: the cube shader's rounded-box morph (`u.shape.x`) rounds
        // the SHARED cube mesh, so enable it ONLY when this frame's main instanced draw
        // is the plain generator cube (Original / Flow-Aligned). Every other user of the
        // main uniform — Swept-Tubes cylinders, Boids creatures, welded/membrane meshes,
        // demo / neural / plexus sub-batches — must see bevel 0 so its geometry is
        // untouched. (Scenery / liquid / water copy `u` below and zero it too; the plexus
        // OVERLAY, which layers over the base cubes with the same uniform, gets its own
        // shape-zeroed `plexus_ov_ubuf` so the base can still bevel underneath it.) On
        // these frames the morph is inert, so they stay byte-identical.
        let cube_draw = matches!(path, RenderPath::Instanced)
            && !tube
            && creature < 0
            && !swept
            && !membrane_arms
            && neural_batches.is_none()
            && plexus_batches.is_none()
            && demo_batches.is_empty();
        // Material-set scoping — a SEPARATE predicate from the bevel above, because the two
        // scope different things. `cube_draw` exists to protect the shared cube MESH from a
        // morph meant only for the generator's cubes; the material set is not geometry, it is
        // a surface response, and any draw that shades through `cube.wgsl` with the main
        // uniform can carry it. The Membrane sheet is exactly that draw: an arbitrary
        // world-space triangle mesh through the cube pipeline with one identity instance, and
        // group(5) is ALREADY bound at both its sites (the scene branch below and the depth
        // prepass) — so this is a uniform-value gate, never a pipeline one.
        //
        // Console Spike Tier 2 (`doc/console_spike_as_built_brief.md` R5,
        // `native/src/substrate_materials.rs`): the substrate backdrop is a flat membrane
        // plane, and graphite / paper / slate are map-driven — they cannot exist on this path
        // while `u.mtl[0]` is forced to 0. Widening the predicate is the same move the five
        // patched uniform copies below make in the other direction (plexus overlay, liquid,
        // scenery, scenery water, demo sub-batches each copy `u` and zero `mtl[0]` because
        // their geometry has a material of its OWN); the membrane has no material of its own,
        // it shares the generator's, so it belongs on the near side of the gate.
        //
        // Byte-identical by default (the repo's 4th invariant): `u.mtl[0]` is set from
        // `material[0] || material_layer[16]` (`world.rs:10850-10852`), both 0 at the stock
        // defaults, so a membrane with no material configured writes exactly the same
        // uniform it did before. `material_maps` specialisation (`sync_material_specialisation`)
        // additionally compiles the whole sampling block out while `present_mask` is 0.
        //
        // Scoped to the lofted SHEET, not Skin-Arms: `membrane_arms` draws rods/welded arm
        // meshes and is already excluded from `cube_draw` for the bevel. The sheet's optional
        // boundary strands (Show Strands) do come with it — they share the main uniform and
        // every other material dial already, and a boundary that disagreed with its own
        // surface would be the odd result. The depth prepass reads this SAME uniform, so a
        // future height-displacing material stays consistent between the two by construction.
        let material_draw = cube_draw || (membrane && !membrane_arms);
        if !cube_draw {
            // The three lanes the cube shader reads: `x` the vertex bevel, `y` the
            // organon#217 T3 face crown (a fragment-stage normal dome, gated on `y > 0`),
            // `z` the T9 emission profile (`tile_profile`, exactly 1.0 at 0 — it scales
            // the PER-INSTANCE emit term). Same scoping, same reason — all three are meant
            // for the generator's cubes only, and while a glyph ring is live the world
            // writes `z` for the tiles, which ARE that draw; a plexus node or a membrane
            // sheet shading through the same uniform must not inherit the falloff.
            u.shape[0] = 0.0;
            u.shape[1] = 0.0;
            u.shape[2] = 0.0;
        }
        if !material_draw {
            // #472 Tier 1: every other draw keeps the scalar PBR path (byte-identical
            // when materials are off).
            u.mtl[0] = 0.0;
        }
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        // Plexus overlay: a shape-zeroed copy of `u` so the overlay's own morph meshes
        // (their own cube→sphere control) aren't double-morphed by the generator bevel,
        // while the base Original / Flow-Aligned cubes underneath keep beveling.
        if plexus_overlay_batches.is_some() {
            let mut ou = u;
            ou.shape = [0.0; 4];
            ou.mtl[0] = 0.0; // overlay geometry never uses the generator material
            queue.write_buffer(&self.plexus_ov_ubuf, 0, bytemuck::bytes_of(&ou));
        }
        // #182 T4 follow-up: the liquid's group-0 uniforms — the scene copy
        // with its own material patched in (None = follow the scene).
        if liquid.enabled {
            let mut lu = u;
            lu.shape = [0.0; 4]; // liquid geometry never bevels
            lu.mtl[0] = 0.0; // liquid never uses the generator material set
            if let Some(m) = liquid.material {
                lu.amb[1] = m.mat_type as f32;
                lu.mat[0] = m.metallic;
                lu.mat[1] = m.roughness;
                lu.mat[2] = m.glow;
                lu.amb[2] = m.ior;
                lu.reflect_ctl[1] = m.chrome_purity;
                lu.reflect_ctl[2] = m.glass_clarity;
                lu.reflect_ctl[3] = m.f0_override;
                lu.glassx[0] = m.dispersion;
                lu.glassx[1] = m.glass_caustic;
                lu.glassx[2] = m.thin_film;
            }
            queue.write_buffer(&self.liquid_ubuf, 0, bytemuck::bytes_of(&lu));
        }
        // Scenery layer (#187 pivot): its group-0 uniforms — the scene copy
        // with the scenery material/FX patched in (independent of the main
        // Material / Surface FX cards).
        if let Some(sc) = scenery {
            if scenery_live {
                let mut su = u;
                su.shape = [0.0; 4]; // scenery geometry never bevels
                su.mtl[0] = 0.0; // scenery never uses the generator material set
                su.mat[0] = sc.metallic;
                su.mat[1] = sc.roughness;
                su.mat[2] = sc.glow;
                // Scenery's own Material Emissive (else it inherits the generator's
                // via the copied base `u.env_tint.w`).
                su.env_tint[3] = sc.emissive;
                su.amb[1] = sc.mat_type;
                su.amb[2] = sc.ior;
                su.amb[3] = sc.palette_active;
                su.matcol = sc.matcol; // #305 T1: scenery's own HSV (else it'd inherit the generator's)
                su.env[3] = sc.opacity;
                su.sss = [sc.sss[0], sc.sss[1], sc.sss[2], 0.0];
                su.irid = [sc.irid[0], sc.irid[1], sc.irid[2], 0.0];
                // The refraction overlay (#201, refr[1..2]) and the anisotropy
                // overlay (#214, aniso[2] enable) are MAIN-material dials — zero
                // them here so the corridor doesn't inherit the generator's
                // glassy/brushed overlay when it's ticked (a scenery-side overlay
                // is a follow-up on the reserved scenery slot). #220 review.
                su.refr[1] = 0.0;
                su.refr[2] = 0.0;
                su.aniso[2] = 0.0;
                // Surface-lobe overlays (#214 T2, coat[2]/coat[3]) are likewise
                // MAIN-material dials — zero them so the corridor doesn't inherit
                // the generator's clearcoat/sheen overlay (a pure Clearcoat/Velvet
                // scenery material via sc.mat_type still works).
                su.coat[2] = 0.0;
                su.coat[3] = 0.0;
                // Body-optics effect dials (#214 T3) are MAIN-material Look dials —
                // zero the thickness drive + interior scatter so the corridor
                // doesn't inherit the generator's SSS/opal (a scenery-side body
                // block is a follow-up on the reserved scenery slots).
                su.body[0] = 0.0;
                su.body[2] = 0.0;
                // Microstructure (#214 T4) amounts are MAIN-material Look dials —
                // zero glitter/diffraction/retro so the corridor keeps its own look
                // (a scenery-side micro block is a follow-up on the reserved slots).
                su.micro[0] = 0.0;
                su.micro[3] = 0.0;
                su.micro2[1] = 0.0;
                // Spectral-emission amounts (#214 T5 pt 1) are MAIN-material Look
                // dials — zero fluorescence/incandescence so the corridor keeps its
                // own glow (a scenery-side emission block is a follow-up).
                su.emit[0] = 0.0;
                su.emit[2] = 0.0;
                // #187 composite fix: the scenery renders with its OWN
                // view-proj (view-locked in composite, the scene camera in the
                // pure ride) — see SceneryLayer::view_proj.
                su.view_proj = sc.view_proj;
                queue.write_buffer(&self.scenery_ubuf, 0, bytemuck::bytes_of(&su));
            }
        }
        // Scenery water floor (#206 Tier 3): its OWN group-0 uniforms — the scene
        // copy with the water material patched in (dielectric: metallic 0). The
        // teal per-vertex tint IS the albedo (palette_active = 1). Its view-proj
        // matches the scenery's (view-locked in composite, scene camera in ride).
        if let Some(w) = water {
            if water_live {
                let mut wu = u;
                wu.shape = [0.0; 4]; // water geometry never bevels
                wu.mtl[0] = 0.0; // water never uses the generator material set
                wu.mat[0] = 0.0; // water is dielectric
                wu.mat[1] = w.roughness;
                wu.mat[2] = w.glow;
                wu.amb[1] = w.mat_type;
                wu.amb[2] = w.ior;
                wu.amb[3] = 1.0; // tint = colour
                wu.matcol = w.matcol; // #305 T1: follow the SCENERY material HSV, not the generator's
                wu.env[3] = w.opacity;
                // Physical-water params + the `sss.w = 3` sentinel the cube
                // shader keys the dedicated water path off (#206).
                wu.sss = [w.absorb, w.glitter, w.reflect, 3.0];
                wu.irid = [0.0, 0.0, 0.0, 0.0];
                // Clear every MAIN-material overlay/effect dial so the water
                // doesn't inherit the generator's refraction / anisotropy /
                // clearcoat-sheen / body-optics / microstructure (#227 review) —
                // the dedicated water branch owns the look. Matches the scenery
                // uniform patch above.
                wu.refr[1] = 0.0;
                wu.refr[2] = 0.0;
                wu.aniso[2] = 0.0;
                wu.coat[2] = 0.0;
                wu.coat[3] = 0.0;
                wu.body[0] = 0.0;
                wu.body[2] = 0.0;
                wu.micro[0] = 0.0;
                wu.micro[3] = 0.0;
                wu.micro2[1] = 0.0;
                // Spectral emission (#214 T5 pt 1) is a MAIN-material Look dial — zero
                // fluorescence/incandescence so the water floor doesn't glow with the
                // generator's emission (its shader still adds the shared `emissive`).
                wu.emit[0] = 0.0;
                wu.emit[2] = 0.0;
                wu.view_proj = w.view_proj;
                queue.write_buffer(&self.water_ubuf, 0, bytemuck::bytes_of(&wu));
            }
        }
        // Demo scene bench (#288): one patched group-0 uniform per (mesh,material)
        // sub-batch — the per-primitive materials that let a mirror sphere sit next
        // to a glass sphere next to diffuse walls in one frame (Tier 2). The
        // per-instance tint carries the wall colour (amb.w = 1 → tint IS albedo),
        // and every MAIN-material overlay is zeroed so the reference scene stays
        // clean. Written before the pass (a pass can't write buffers).
        if demo_live {
            for (i, bt) in demo_batches.iter().take(self.demo_ubufs.len()).enumerate() {
                let m = bt.material;
                let mut du = u;
                du.amb[1] = m.mat_type as f32;
                du.amb[2] = m.ior;
                du.amb[3] = 1.0; // tint = albedo (per-instance wall colours)
                du.mat[0] = m.metallic;
                du.mat[1] = m.roughness;
                du.mat[2] = m.glow; // emissive glow (Tier-3 emitters bloom through HDR)
                // Clean material: no generator overlays leak onto the reference scene.
                du.sss = [0.0, 0.0, 0.0, 0.0];
                du.irid = [0.0, 0.0, 0.0, 0.0];
                du.refr[1] = 0.0;
                du.refr[2] = 0.0;
                du.aniso[2] = 0.0;
                du.coat[2] = 0.0;
                du.coat[3] = 0.0;
                du.body[0] = 0.0;
                du.body[2] = 0.0;
                du.micro[0] = 0.0;
                du.micro[3] = 0.0;
                du.micro2[1] = 0.0;
                du.emit[0] = 0.0;
                du.emit[2] = 0.0;
                queue.write_buffer(&self.demo_ubufs[i], 0, bytemuck::bytes_of(&du));
            }
        }
        queue.write_buffer(&self.sky_ubuf, 0, bytemuck::bytes_of(sky_uniforms));
        // Bounced-GI (#80 Part B): upload the probe grid (or "off") for group(3).
        self.gi.update(queue, gi_min, gi_max, gi_on, gi_intensity, gi_falloff, gi_probes);
        // Shared radiance estimate for the neighbour-light systems (VXGI injection +
        // emissive-cubes-as-lights): a node's tint is its ALBEDO — the light it
        // actually sheds is tint × glow (emission) plus a slice of the key light it
        // reflects. Injecting the raw tint made a completely unlit, non-emissive
        // field "bounce"/glint at full strength.
        let radiance_scale = uniforms.mat[2].max(0.0) + 0.3 * uniforms.key_light[3].max(0.0);
        // Emissive cubes as real lights (#167 Tier 3): pick the brightest nodes and upload
        // them as point lights (group 3 binding 1). Radius scales the scene diagonal. Off
        // (or a raymarch mode with no nodes) → count 0 → the cube loop adds nothing.
        let ml_radius_world = (gi_max - gi_min).length().max(1e-3) * ml_radius.max(1e-3);
        self.gi.update_lights(
            queue,
            meta_nodes,
            // #182 T4 ghost light: a hidden generator keeps its point lights.
            ml_on && !instances.is_empty() && (!hide_generator || coupling.ghost),
            ml_intensity * radiance_scale,
            ml_radius_world,
            ml_count.max(0) as usize,
            ml_restir,
        );
        if terrain_pass {
            self.terrain.write_uniforms(queue, terrain_u);
        }
        if stars_on {
            self.stars.write_uniforms(queue, star_u);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame-encoder"),
        });

        // "Hide generator" (particles-only): the generator still built the particle
        // stir field above, but we suppress every geometry path so only the
        // background + particles draw. Shadow the mode flags so the metaball/
        // mandelbulb prebakes, the SSAO/early-Z prepasses, and all the scene-pass
        // geometry branches fall through.
        let metaball = metaball && !hide_generator;
        let volume = volume && !hide_generator;
        let voxel = voxel && !hide_generator;
        let mandelbulb = mandelbulb && !hide_generator;
        let creature_ray = creature_ray && !hide_generator;
        let minimal = minimal && !hide_generator;
        let neural_on = neural_on && !hide_generator;
        let lens_on = lens_on && !hide_generator;
        let membrane = membrane && !hide_generator;
        // Splat replaces the cube draw with its own Gaussian pass, so suppress the
        // instanced cube draw (and, via `opaque_path`/`depth_fx` below, its prepass)
        // even though `instances` is non-empty (the splats read it directly).
        let draw_instances = !instances.is_empty() && !hide_generator && !splat_on;

        // Geometry available to the single-sample depth prepass that screen-space effects
        // (VXGI diffuse + specular, SSAO, SSR, SSGI, DoF, TAA) reconstruct world position
        // from: the instanced cube/tube geometry, OR — when the Membrane opt-in is on —
        // the membrane mesh. `mem_prepass` is what lets those effects apply to Membrane;
        // off, membrane skips the prepass (today's look). Raymarch modes have no prepass
        // geometry, so they're excluded below as before.
        let mem_prepass = membrane && membrane_fx && mem_icount > 0;
        let inst_prepass = draw_instances && !membrane;
        // Contiguous (welded) Swept Tubes: `instances` is empty and the raster draws
        // the dynamic welded mesh, so — exactly like Membrane's `mem_prepass` — that
        // mesh must be rasterized into the single-sample depth prepass, else every
        // screen-space effect (SSAO / SSR / SSGI / VXGI / DoF / TAA / screen-refraction
        // AND the hardware-RT shadow/reflect/GI masks, which all reconstruct from the
        // prepass depth) reads missing depth on the welded tubes and no-ops (or, for
        // the RT shadow mask, leaves the key+fill "sun" fully shadowed → the tubes go
        // dark). Drawn below alongside the membrane sheet.
        let swept_prepass = draw_swept;
        // The path tracer is ground truth — no screen-space geometry passes
        // (prepass / SSAO / SSR / SSGI / rt_gi / VXGI / DoF / TAA) run under it.
        let screen_geo = (inst_prepass || mem_prepass || swept_prepass) && !pathtrace_active;

        // Metaball / Volume: bake the node field into the 3D texture (compute)
        // before the scene pass. Both reuse the SAME bake; only the scene-pass draw
        // differs (isosurface vs emissive medium).
        //
        // Field Volume (#348): when the Volume source selected the analytic field-
        // energy bake, `field_vol_grid` carries a CPU-baked FIELD_RES³ density grid —
        // upload it straight into the field texture + arm the raymarch (no node point-
        // set voxelize → no far-node scraggle). Empty → the classic node bake below
        // (byte-identical Legacy / smoothed-node / metaball).
        // Only take the direct-upload path when the grid is EXACTLY `FIELD_RES³` — a
        // short/mismatched grid would leave `upload_field` a no-op yet still arm the
        // raymarch, showing a stale prior-frame texture. On a mismatch fall through to
        // the node bake below (Bugbot).
        let field_upload_ok =
            field_vol_grid.len() == (FIELD_RES as usize) * (FIELD_RES as usize) * (FIELD_RES as usize);
        if volume && field_upload_ok {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.meta.upload_field(queue, field_vol_grid);
            self.meta.prepare_direct(queue, meta_min, meta_max, inv_vp, meta_params);
        } else if metaball || volume {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.meta.voxelize(
                device, queue, &mut encoder, meta_nodes, meta_min, meta_max, inv_vp, meta_params,
            );
        }

        // MLS-MPM liquid (#182 Tier 3a): step the particle solver (substeps ×
        // P2G/grid/G2P), splat the density into the liquid MetaField's texture,
        // and arm its isosurface raymarch. KIFS is a flat screen-space field
        // with no world depth to composite against, so the liquid skips it.
        let liquid_on = liquid.enabled && !kifs_on && !neural_on;
        if liquid_on {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.liquid_sim.step(
                device,
                queue,
                &mut encoder,
                liquid.count,
                liquid.grid_res,
                metaball::FIELD_RES,
                self.liquid_meta.field_view(),
                &mut self.liquid_field_gen,
                liquid.container_min,
                liquid.container_max,
                liquid.colliders,
                liquid.glow,
                liquid.dt,
                &liquid.params,
            );
            self.liquid_meta.prepare_direct(
                queue,
                liquid.container_min,
                liquid.container_max,
                inv_vp,
                &liquid.surface,
            );
        }

        // Voxel GI (#152 Tier 3): voxelize the node field (compute) whenever VXGI is
        // on the instanced path. The gather pass runs after the scene resolves. Off
        // path / raymarch modes → skipped (no prepass depth to march from).
        let vxgi_cast = vxgi_on
            && screen_geo
            && !metaball
            && !volume
            && !voxel
            && !mandelbulb
            && !creature_ray
            && !kifs_on
            && !neural_on
            && !lens_on;
        // #182 T4: the ink samples the radiance VOLUME directly (and the fluid
        // injects into it), so voxelize also when the generator is hidden —
        // the invisible structure's glow still lights the smoke. The gather
        // (onto geometry pixels) keeps the stricter `vxgi_cast` gate.
        // The raymarch-mode exclusions only matter while the generator is
        // VISIBLE (its own surface owns those pipelines); hidden (ghost/pure-
        // medium views) draws none of them, so the volume must stay alive.
        let vxgi_volume = vxgi_on
            && (hide_generator || (!metaball && !volume && !voxel && !mandelbulb && !creature_ray && !neural_on && !lens_on))
            && !kifs_on
            && (screen_geo || ink.enabled || liquid_on || coupling.ghost);
        // External volume sampling (the ink march + the liquid/metaball
        // isosurface): gain 0 whenever the volume wasn't voxelized this frame.
        self.vxgi.set_sample(
            queue,
            gi_min,
            gi_max,
            if vxgi_volume { vxgi.intensity } else { 0.0 },
        );
        if vxgi_volume {
            // Fluid → GI (#182 T4): the ink dye + the liquid density are extra
            // injection sources. `ink_active` is hoisted from the solver gate
            // below; the dye texture is one frame behind the solver here (the
            // fluid steps after the voxelize) — irrelevant for bounce light.
            let fgi_ink = ink.enabled && !ink.dye_src.is_empty();
            let fluid_src = vxgi::FluidGiSources {
                dye_view: self.fluidvis.dye_view(),
                dye_gen: self.fluidvis.dye_gen(),
                dye_min: particles.grid_min,
                dye_max: particles.grid_max,
                dye_gain: if fgi_ink { coupling.gi.max(0.0) } else { 0.0 },
                liq_view: self.liquid_meta.field_view(),
                liq_min: liquid.container_min,
                liq_max: liquid.container_max,
                liq_gain: if liquid_on { coupling.gi.max(0.0) } else { 0.0 },
            };
            self.vxgi.voxelize(
                device, queue, &mut encoder, meta_nodes, gi_min, gi_max, radiance_scale,
                &fluid_src,
            );
        }

        // Voxel: splat the same node set into the 3D occupancy grid (compute)
        // before the scene pass that DDA-raymarches it.
        if voxel {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.vox.splat(
                device, queue, &mut encoder, meta_nodes, meta_min, meta_max, inv_vp, voxel_params,
            );
        }

        // Terrain half-res (perf): raymarch the landscape into a half-size HDR
        // target now; the scene pass upscales it instead of marching at full res.
        if terrain_pass && terrain_scale > 1 {
            self.terrain.render_low(device, &mut encoder, size, terrain_scale);
        }

        // Mandelbulb: upload the per-frame params before the scene pass that
        // raymarches the fractal (analytic — no compute prebake needed).
        if mandelbulb {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.mandel.prepare(queue, inv_vp, size, mandel_params);
        }

        // Creature Engine (#476): upload the per-frame params + body-plan
        // primitives before the scene pass raymarches the creature.
        if creature_ray {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.creature.prepare(queue, inv_vp, size, creature_params);
        }

        // Minimal surface (#127): upload the per-frame params before the scene
        // pass raymarches the TPMS isosurface (analytic — no compute prebake).
        if minimal {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.minimal.prepare(queue, inv_vp, size, minimal_params);
        }

        // Kaleidoscopic Fractal: the flat field needs only the aspect ratio; the
        // The KIFS field is camera-independent (fullscreen, screen-space).
        if kifs_on {
            let aspect = size.0 as f32 / size.1.max(1) as f32;
            self.kifs.prepare(queue, aspect, kifs_params);
        }

        // Neural field (#200 Tier 1): upload the per-frame params before the
        // scene pass raymarches the MLP isosurface (analytic — no prebake).
        if neural_on {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.neural.prepare(queue, inv_vp, size, neural_field_params);
        }

        // Lens (#258 Tier 3): upload the per-frame params before the scene pass
        // sphere-traces the lens SDF (analytic — no prebake).
        if lens_on {
            let inv_vp = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj);
            self.lens.prepare(queue, inv_vp, size, lens_params);
        }

        // Advance the reaction–diffusion sim (compute, ping-pong) before the scene
        // pass that samples it. The lit shaders read group 2/3 = the RD field.
        // Reaction–diffusion: step only when some shader actually reads the field
        // (#174 T2 — the sim ran 12 compute dispatches over its 256² grid every
        // frame even at zero emissive + zero pigment). State persists across the
        // gate, so re-enabling resumes the pattern rather than reseeding.
        if rd_params.intensity > 0.0 || rd_params.albedo_mix > 0.0 {
            self.rd.step(queue, &mut encoder, rd_params);
        }

        // Particle Aura: advect/age/respawn the motes (compute) before the scene
        // pass that draws them additively. Inert (no dispatch) when disabled. In the
        // Fluid tier, first step the persistent Navier–Stokes field (the source grid
        // STIRS it) and have the motes ride the evolved fluid instead of the raw grid.
        // Fluid Ink (#182 Tier 1) runs the same solver even without the Fluid tier —
        // the visual fills the frame's grid/splat fields whenever the ink is on —
        // and rides a dye field along (injected here, rendered after the scene).
        let ink_active = ink.enabled && !ink.dye_src.is_empty();
        if (particles.enabled && particles.fluid) || ink_active {
            self.fluid.step(
                device,
                queue,
                &mut encoder,
                particles.vel_grid,
                particles.grid_res,
                particles.grid_min,
                particles.grid_max,
                particles.dt,
                particles.time,
                particles.fluid_params,
                if ink_active { Some((ink.dye_src, ink.dye)) } else { None },
            );
        } else {
            // Not stepping this frame → the dye session (if any) ended; the
            // next ink activation must start from clean water.
            self.fluid.note_idle();
        }
        // #298 Tier 1: the bead draw shades with the scene's PBR/IBL context — pull it
        // from the cube uniforms (`u` carries the live prefilter mip count in mat[3]).
        let particle_shade = ParticleShade {
            cam_pos: Vec3::from_slice(&u.camera_pos[..3]),
            prefilter_mips: u.mat[3],
            key_light: Vec4::from_array(u.key_light),
            fill_light: Vec4::from_array(u.fill_light),
            env: Vec4::new(u.env[0], u.env[1], u.env[2], u.amb[0]),
            env_tint: Vec3::from_slice(&u.env_tint[..3]),
            skyrefl: Vec4::from_array(u.skyrefl),
        };
        // Gaussian Splatting surface: build the splat cloud from the node instances +
        // the scene's camera/IBL context (the SAME `u` the cube draw uses, so the
        // splats land exactly where the cubes would). `prepare` is a no-op unless the
        // Splat path is active (and the generator isn't hidden); the draw happens in
        // the scene pass's geometry branch.
        //
        // Derive the billboard basis from the view-projection itself, NOT from
        // `particles.cam_right/up`: the particle frame only carries the true camera
        // basis while the aura/ink/liquid path is active; when it's inert (the default,
        // Aura = Off) those fields fall back to a constant world XY basis, which would
        // pin the splat Gaussians to the world plane instead of facing the camera. The
        // first two columns of inv(view_proj) are the world-space right/up screen axes
        // (perspective scale folds out under normalization), so the splats reorient as
        // the camera orbits, independent of the aura state.
        let splat_view_proj = Mat4::from_cols_array_2d(&u.view_proj);
        let splat_inv_vp = splat_view_proj.inverse();
        let splat_cam_right = splat_inv_vp.x_axis.truncate().normalize_or_zero();
        let splat_cam_up = splat_inv_vp.y_axis.truncate().normalize_or_zero();
        let splat_frame = SplatFrame {
            enabled: splat_on && !hide_generator,
            instances,
            tints,
            view_proj: splat_view_proj,
            cam_right: splat_cam_right,
            cam_up: splat_cam_up,
            cam_pos: Vec3::from_slice(&u.camera_pos[..3]),
            prefilter_mips: u.mat[3],
            radius: splat_params.radius,
            opacity: splat_params.opacity,
            falloff: splat_params.falloff,
            cutoff: splat_params.cutoff,
            aniso: splat_params.aniso,
            scatter: splat_params.scatter,
            jitter: splat_params.jitter,
            solid: splat_params.solid,
            lit: splat_params.lit,
            key_light: Vec4::from_array(u.key_light),
            fill_light: Vec4::from_array(u.fill_light),
            env: Vec4::new(u.env[0], u.env[1], u.env[2], u.amb[0]),
            env_tint: Vec3::from_slice(&u.env_tint[..3]),
            metallic: u.mat[0],
            roughness: u.mat[1],
            // Tier 3 relightable material: honour the Material card (material_type +
            // glass IOR) so lit splats reflect/refract the env like the cubes.
            material_type: u.amb[1],
            ior: u.amb[2],
            // #305 live-sky cloud reflections — same modulation the cubes/beads get.
            skyrefl: Vec4::from_array(u.skyrefl),
        };
        self.splats.prepare(device, queue, &splat_frame);
        if particles.enabled && particles.fluid {
            let ext = self.fluid.vel_buffer();
            self.particles.simulate(device, queue, &mut encoder, particles, Some(ext), &particle_shade);
        } else {
            self.particles.simulate(device, queue, &mut encoder, particles, None, &particle_shade);
        }
        // organon#217 T6/T3: the coaxial-glass core for this frame's capsule draws, from
        // the param chain (`Shared.capsule` → `Surface.capsule_core`). Set BEFORE the two
        // uploads below, which is when `set_capsule_core` says it takes effect. [0, 0]
        // is the inert gate; the env seed, if set, overrides inside.
        self.particles.set_capsule_core(capsule_core[0], capsule_core[1]);
        // Membrane Skin-Arms capsule impostors (Stage 2): upload this frame's arm
        // segments + the scene material/PBR context (Material card via the cube
        // uniforms). Empty `arm_caps` clears the draw. Uses the particles' unscaled
        // world→clip + camera basis so the billboards match the beads.
        self.particles.set_arms(
            device,
            queue,
            arm_caps,
            // The capsules are built from generator node positions (the membrane
            // sheet's space), so they must use the CUBE view_proj (breath-scale folded
            // in), not the particles' unscaled one — else the arms drift under breath.
            // `particle_shade.cam_pos` is already `u.camera_pos` (the matching origin);
            // the camera right/up are pure directions (uniform breath doesn't rotate).
            Mat4::from_cols_array_2d(&u.view_proj),
            particles.cam_right,
            particles.cam_up,
            &particle_shade,
            u.amb[1], // material_type
            u.mat[0], // metallic
            u.mat[1], // roughness
            u.amb[2], // glass IOR
            u.mat[2], // Material Glow → arm emissive (parity with the cube path)
            Vec4::new(0.0, 1.0, 1.0, 0.0), // identity HSV (no per-arm recolour)
        );

        // Plexus Tier 2 impostors: node spheres + edge tubes, each with its own
        // material. Uses the same view_proj / camera basis / shade context as the
        // arms. Empty caps → no-op (clears the batches).
        self.particles.set_plexus(
            device,
            queue,
            plexus_node_caps,
            plexus_edge_caps,
            Mat4::from_cols_array_2d(&u.view_proj),
            particles.cam_right,
            particles.cam_up,
            &particle_shade,
            plexus_node_mat,
            plexus_edge_mat,
        );
        let _ = plexus_impostor; // consumed via non-empty caps; kept for clarity

        // Fluid light coupling (#182 T4), light-space passes: the dye's key-light
        // transmittance (the smoke shadows the scene) + the liquid caustic splat.
        // Runs between the solver steps and the scene pass; the cube shader's
        // amounts (ShadowU.params2, set in `shadow.update` below) are zeroed
        // whenever this didn't run, so a stale map is never read.
        // Known one-frame lag, accepted: the dye 3D texture is blitted inside
        // fluidvis.render (later this frame), so this pass marches LAST frame's
        // dye — same trade as the VXGI dye injection; a soft transmittance map
        // of a diffusing medium is visually indistinguishable one frame behind.
        let ns_stepped = (particles.enabled && particles.fluid) || ink_active;
        let trans_on = coupling.shadow > 0.0 && ink_active;
        let caustic_on = coupling.caustic > 0.0 && liquid_on;
        if trans_on || caustic_on {
            self.fluidlight.run(
                device,
                queue,
                &mut encoder,
                &fluidlight::FluidLightFrame {
                    light_vp: Mat4::from_cols_array_2d(&shadow_light_vp),
                    dye_min: particles.grid_min,
                    dye_max: particles.grid_max,
                    extinction: ink.params.extinction,
                    trans_on,
                    liq_min: liquid.container_min,
                    liq_max: liquid.container_max,
                    // The liquid's own IOR when its material overrides the
                    // scene — the caustic refraction must match the surface.
                    ior: liquid.material.map(|m| m.ior).unwrap_or(uniforms.amb[2]),
                    threshold: liquid.surface.threshold,
                    caustic_on,
                    caustic_sharpness: coupling.caustic_sharp,
                    dye_view: self.fluidvis.dye_view(),
                    dye_gen: self.fluidvis.dye_gen(),
                    liq_view: self.liquid_meta.field_view(),
                },
            );
        }

        // Two-way coupling (#182 T4): fluid velocity → per-node sway springs,
        // displacing the uploaded instance translations in place so the depth /
        // shadow / scene passes all see the swayed structure. Velocity source:
        // the Navier–Stokes grid when it stepped this frame (world units/s),
        // else the MLS-MPM liquid grid (grid units/s → × cell size).
        // Sway displaces the DRAWN instance buffer, so it's inherently gated on
        // the instances being uploaded (`inst_gpu_used`): with the generator
        // hidden nothing samples that buffer — no draws, no depth — so running
        // it would move nothing. The lighting-source positions (meta_nodes /
        // probes / point lights) are CPU-side and intentionally UN-swayed: the
        // offset is bounded (≤ ~2 world units at full dial) and under a cell of
        // the coarse 32³/6³ lighting volumes — a documented approximation of
        // the no-readback design.
        if coupling.sway > 0.0 && inst_gpu_used && (ns_stepped || liquid_on) {
            let liq_res = self.liquid_sim.res();
            let liq_h = (liquid.container_max.x - liquid.container_min.x)
                / liq_res.max(1) as f32;
            let sf = if ns_stepped {
                sway::SwayFrame {
                    amount: coupling.sway,
                    vel_buf: self.fluid.vel_buffer(),
                    vel_gen: self.fluid.epoch(),
                    vel_src: 0,
                    vel_min: particles.grid_min,
                    vel_max: particles.grid_max,
                    vel_res: particles.grid_res,
                    vel_scale: 1.0,
                    dt: particles.dt,
                }
            } else {
                sway::SwayFrame {
                    amount: coupling.sway,
                    vel_buf: self.liquid_sim.grid_v(),
                    vel_gen: self.liquid_field_gen,
                    vel_src: 1,
                    vel_min: liquid.container_min,
                    vel_max: liquid.container_max,
                    vel_res: [liq_res; 3],
                    vel_scale: liq_h,
                    dt: liquid.dt,
                }
            };
            self.sway.run(
                device,
                queue,
                &mut encoder,
                &self.inst_buf,
                self.inst_gen,
                instances.len(),
                &sf,
            );
        }

        // 0) Single-sample depth prepass, shared by SSAO (#30) and SSR (#80 A).
        //    Skipped entirely when both are off, so the default path is unchanged.
        //    (Metaball/Membrane/Mandelbulb/KIFS produce no standard instanced geometry
        //    for the prepass, so neither effect applies in those modes; `hide_generator`
        //    suppresses the geometry, so `draw_instances` gates it too.)
        // Depth-of-field (#152) needs the single-sample depth too; SSGI (#152 Tier 2)
        // marches it; TAA (#152 Tier 2) reconstructs camera velocity from it. All
        // extend the prepass gate (instanced path only — raymarch modes have no prepass
        // geometry, so DoF/SSGI are inert there and TAA degrades to no-reprojection).
        let dof_on = fxp.enabled && fxp.dof > 0.0;
        let taa_wants_depth = tp.enabled && tp.taa;
        // VXGI (#152 Tier 3) reconstructs world position from the prepass depth too.
        // Fluid Ink (#182 Tier 1) clamps its volume march at the prepass depth so
        // the medium composites against visible geometry (no depth → full march,
        // which is exactly right for the raymarch / hidden-generator cases).
        // Hardware-RT shadows (#195 Tier 1) + reflections (Tier 2) trace off
        // the prepass depth too.
        let rt_shadow_want = rt_shadow.is_some();
        let rt_reflect_want = rt_reflect.is_some();
        let rt_gi_want = rt_gi.is_some();
        // Screen-space refraction (#214 T5 pt 2) needs the prepass depth too — only
        // on the instanced Refractive material (amb[1] == 3) with the strength up.
        // The post pass treats every prepass-covered pixel as glass, but scenery and
        // the water floor share that prepass with their OWN materials + view-locked
        // depth — so gate it to when the instanced field is the sole prepass geometry
        // (no scenery/water) to avoid refracting them with a wrong world reconstruction.
        let refract_ss_want = refract_ss > 0.0
            && (uniforms.amb[1] - 3.0).abs() < 0.5
            && !scenery_live
            && !water_live;
        // Scenery counts as prepass geometry (#203 review): in the pure ride
        // the corridor is the ONLY geometry, and in composite it still wants
        // SSAO/SSR/DoF/TAA — its prepass draw binds the scenery uniforms, so
        // its depth is valid on both routes. (VXGI keeps the stricter
        // `screen_geo` gate — scenery isn't voxelized.)
        // #298 Tier 3: the shaded beads write into the FX depth prepass too, so
        // SSAO/SSR/SSGI/DoF/TAA see the droplets. They count as prepass geometry — so
        // the prepass runs even in the particles-only (hidden-generator) case — but
        // only on the instanced routes (the `!metaball…` exclusions below still apply;
        // beads over a raymarch generator don't get the screen-space FX, a follow-up).
        let beads_live = particles.enabled && particles.beads;
        // Skin-Arms Impostor capsules are the only prepass geometry in that mode
        // (no shell, instances + welded mesh cleared), so they must open the prepass
        // themselves — else `draw_arms_depth` never runs and SSAO/SSR/SSGI/DoF/TAA
        // miss the arms. (Mesh arms ride `swept_prepass` via `screen_geo`.)
        let arm_live = membrane_arms && !arm_caps.is_empty();
        let depth_fx = (ssao_on
            || ssr_on
            || ssgi_on
            || dof_on
            || taa_wants_depth
            || vxgi_cast
            || ink_active
            || rt_shadow_want
            || rt_reflect_want
            || rt_gi_want
            || refract_ss_want)
                && (screen_geo || scenery_live || water_live || beads_live || arm_live)
                && !metaball
                && !volume
                && !voxel
                && !mandelbulb
                && !creature_ray
                && !kifs_on
                && !neural_on
                && !lens_on;
        // Neural-field GI: the raymarch has no instanced geometry for the prepass,
        // but SSR (#80 A) and SSGI (#152 T2) reconstruct normals from depth and
        // composite in POST, so a depth-only neural march into the same prepass is
        // enough for them to gather off the surface. The in-shader-consumed effects
        // (SSAO / RT shadow-reflect-GI masks) stay excluded above — the cube pass
        // that samples them isn't drawn when the neural field is active. So the
        // neural field only opts into the depth prepass when SSR/SSGI are on.
        let neural_screen_fx = neural_on && (ssr_on || ssgi_on);
        // Voxel-field GI (#voxel-pbs Level 2): the DDA raymarch has no instanced
        // geometry for the prepass either, so — exactly like the neural field — a
        // depth-only voxel march into the same single-sample prepass lets SSR/SSGI
        // reconstruct the voxel surface's normal from depth and gather off it,
        // composited in post. Only opts in when SSR/SSGI are on (the in-shader AO/RT
        // masks stay excluded — the cube pass that samples them isn't drawn here).
        let voxel_screen_fx = voxel && (ssr_on || ssgi_on);
        // Creature Engine (#476 full-PBR follow-up): the union-of-SDF raymarch has no
        // instanced geometry for the prepass either, so — exactly like the neural /
        // voxel fields — a depth-only creature march into the same single-sample
        // prepass lets SSR/SSGI reconstruct the creature's normal from depth and
        // gather off it (its neighbours' reflections + bounced GI), composited in
        // post. Only opts in when SSR/SSGI are on (the in-shader AO/RT masks stay
        // excluded — the cube pass that samples them isn't drawn for the creature).
        let creature_screen_fx = creature_ray && (ssr_on || ssgi_on);
        let run_prepass = depth_fx || neural_screen_fx || voxel_screen_fx || creature_screen_fx;
        // (Hoisted above the prepass — the sharing decision below needs it.)
        // Opaque early-Z classification: for opaque materials (everything except
        // Glass/Refractive) on the instanced cube/tube modes, a depth-only prepass
        // fills the scene depth first, so the heavy PBR shader then runs only on the
        // front-most fragment per pixel (cull Back drops back faces). Glass and
        // Refractive stay single-pass — their alpha-blended refraction needs both
        // faces and can't use an Equal test — and Membrane/Metaball keep their own paths.
        let glass = uniforms.amb[1] >= 1.5; // material_type == Glass (2) or Refractive (3)
        let opaque_path = !metaball
            && !volume
            && !voxel
            && !membrane
            && !mandelbulb
            && !creature_ray
            && !minimal
            && !kifs_on
            && !lens_on
            && !glass
            && draw_instances;
        // #174 T2: at MSAA 1× on the opaque instanced path, the screen-space-FX
        // prepass and the opaque early-Z prepass rasterized the IDENTICAL geometry
        // into two separate single-sample depth textures. Share one: render the FX
        // prepass with the early-Z pipeline (cull Back — matches the scene pass's
        // Equal test) straight into the scene depth, skip the early-Z pass, and
        // point the screen-space effects at the scene depth — the whole field is
        // rasterized one less time per frame.
        let shared_prepass = depth_fx && opaque_path && self.sample_count == 1;
        // The depth the screen-space effects read this frame; the epoch+route key
        // invalidates their cached bind groups when the texture or route changes.
        let fx_depth: &wgpu::TextureView = if shared_prepass {
            &self.depth_view
        } else {
            &self.prepass_depth_view
        };
        let depth_key = self.depth_epoch.wrapping_mul(2).wrapping_add(shared_prepass as u64);
        if depth_fx {
            let (vbuf, ibuf, index_count) =
                match self.creature_meshes.get(if creature >= 0 { creature as usize } else { usize::MAX }) {
                    Some(m) => (&m.0, &m.1, m.2),
                    None if neural_capsule => (&self.capsule_vbuf, &self.capsule_ibuf, self.capsule_index_count),
                    None if tube => (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count),
                    None => (&self.vbuf, &self.ibuf, self.index_count),
                };
            {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("depth-prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: fx_depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // Shared route: the early-Z pipeline (cull Back) so the depth also
                // serves the scene pass's Equal test.
                rp.set_pipeline(if shared_prepass {
                    &self.opaque_prepass_pipeline
                } else {
                    &self.prepass_pipeline
                });
                rp.set_bind_group(0, &self.bind_group, &[]);
                // group(5): `vs_depth` samples the height map (#472 Tier 5). The
                // later group(0) rebinds below (plexus overlay / scenery / water)
                // don't disturb it — same pipeline, higher slot.
                rp.set_bind_group(5, &self.material.bind, &[]);
                if mem_prepass {
                    // Membrane opt-in: rasterize the same geometry the scene pass draws in
                    // membrane mode — the optional boundary strands (instanced tubes) AND
                    // the lofted sheet — so every membrane pixel has prepass depth (else the
                    // screen-space FX read missing/stale depth on the strand tubes).
                    if show_strands && !instances.is_empty() {
                        rp.set_vertex_buffer(0, vbuf.slice(..));
                        rp.set_vertex_buffer(1, self.inst_buf.slice(..));
                        rp.set_vertex_buffer(2, self.tint_buf.slice(..));
                        rp.set_vertex_buffer(3, self.emit_buf.slice(..));
                        rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                        rp.draw_indexed(0..index_count, 0, 0..instances.len() as u32);
                    }
                    // The lofted sheet (one identity instance, per-vertex layout).
                    rp.set_vertex_buffer(0, self.mem_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                    rp.set_vertex_buffer(2, self.white_tint.slice(..));
                    rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                    rp.set_index_buffer(self.mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mem_icount as u32, 0, 0..1);
                } else if draw_swept {
                    // Contiguous (welded) Swept Tubes: rasterize the same welded mesh
                    // the scene pass draws (one identity instance, per-vertex layout, u32
                    // indices) so the screen-space FX + RT masks have valid prepass depth
                    // on the tubes. Mirrors the membrane sheet above.
                    rp.set_vertex_buffer(0, self.swept_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                    rp.set_vertex_buffer(2, self.white_tint.slice(..));
                    rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                    rp.set_index_buffer(self.swept_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..swept_idx.len() as u32, 0, 0..1);
                } else if demo_live {
                    // Demo scene bench (#288): rasterize the opaque sub-batches into
                    // the FX prepass depth so SSAO/SSR/SSGI/DoF/TAA see the scene.
                    self.draw_demo_geometry(&mut rp, demo_batches, true);
                } else if let Some(pb) = &plexus_batches {
                    // Plexus Tier-1 shape morph: markers + struts as two morphed sub-batches.
                    self.draw_plexus_batches(&mut rp, pb);
                } else if let Some(nb) = &neural_batches {
                    // Neural Tissue (#260 Tier 1): the same soma/capsule/bouton
                    // sub-batches the scene pass draws, into the FX prepass depth.
                    self.draw_neural_batches(&mut rp, nb);
                } else if !splat_on {
                    rp.set_vertex_buffer(0, vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.inst_buf.slice(..));
                    rp.set_vertex_buffer(2, self.tint_buf.slice(..));
                    rp.set_vertex_buffer(3, self.emit_buf.slice(..));
                    rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                    rp.draw_indexed(0..index_count, 0, 0..instances.len() as u32);
                }
                // Plexus OVERLAY Tier-1: layer the outer-shell markers+struts over the
                // base surface's prepass depth so SSAO/SSR/SSGI see the web too. Group 0
                // swaps to the shape-zeroed overlay uniform (the base cube used `u`'s
                // bevel), so the overlay's prepass depth matches its opaque Equal draw.
                if let Some(pb) = &plexus_overlay_batches {
                    rp.set_bind_group(0, &self.plexus_ov_bind, &[]);
                    self.draw_plexus_batches_from(
                        &mut rp, pb, &self.plexus_ov_inst_buf, &self.plexus_ov_tint_buf, &self.zero_emit,
                    );
                }
                // Splat mode: the cube instances aren't drawn in the scene pass, so they
                // must NOT be rasterized into the FX prepass depth either (else SSAO/SSR/
                // SSGI/DoF/TAA would gather off invisible cube silhouettes). The splat
                // cloud has no depth-prepass geometry of its own — a first-cut limitation:
                // screen-space FX simply don't include the splats.
                // Scenery layer (#187 pivot): rasterize it too, so the
                // screen-space FX (SSAO/SSR/SSGI/DoF/TAA) see the corridor —
                // and on the shared route so the scene pass's Equal test
                // admits the main geometry drawn alongside it. Group 0 swaps
                // to the scenery uniforms (#187 composite fix): the scenery
                // view-proj differs from the scene's when view-locked, and the
                // scene pass shades it via the same uniforms, so prepass and
                // scene depths stay Equal-consistent. (Last draw in the pass.)
                if let Some(sc) = scenery {
                    if scenery_live {
                        rp.set_bind_group(0, &self.scenery_bind, &[]);
                        // Skin (#206 Tier 1): rasterize the loft so the
                        // screen-space FX see the corridor/valley surface.
                        // A mixed-topology Skin transition fills BOTH the
                        // membrane (Grid side) and the instances (Streamlines
                        // side), so draw whichever are non-empty — not either/or
                        // (#217 review: the instanced half was being dropped).
                        if scenery_mem_icount > 0 {
                            rp.set_vertex_buffer(0, self.scenery_mem_vbuf.slice(..));
                            rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                            rp.set_vertex_buffer(2, self.white_tint.slice(..));
                            rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                            rp.set_index_buffer(self.scenery_mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                            rp.draw_indexed(0..scenery_mem_icount as u32, 0, 0..1);
                        }
                        if scenery_count > 0 {
                            let (svb, sib, sic) = if sc.tube {
                                (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count)
                            } else {
                                (&self.vbuf, &self.ibuf, self.index_count)
                            };
                            rp.set_vertex_buffer(0, svb.slice(..));
                            rp.set_vertex_buffer(1, self.scenery_inst_buf.slice(..));
                            rp.set_vertex_buffer(2, self.scenery_tint_buf.slice(..));
                            rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                            rp.set_index_buffer(sib.slice(..), wgpu::IndexFormat::Uint16);
                            rp.draw_indexed(0..sic, 0, 0..scenery_count as u32);
                        }
                    }
                }
                // Scenery water floor (#206 Tier 3): rasterize the sheet into the
                // prepass depth too, so the screen-space FX — chiefly SSR — see
                // the channel water and reflect the fjord walls in it (the money
                // shot). Group 0 swaps to the water uniforms for its view-proj.
                if let Some(_w) = water {
                    if water_live {
                        rp.set_bind_group(0, &self.water_bind, &[]);
                        rp.set_vertex_buffer(0, self.water_mem_vbuf.slice(..));
                        rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                        rp.set_vertex_buffer(2, self.white_tint.slice(..));
                        rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                        rp.set_index_buffer(self.water_mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..water_mem_icount as u32, 0, 0..1);
                    }
                }
                // #298 Tier 3: the shaded beads (depth only) into the prepass, so the
                // screen-space FX reconstruct off the droplets too. Sets its own
                // pipeline + group-0 (DrawU); single-sample, matching the prepass depth.
                self.particles.draw_depth(&mut rp);
                // Membrane Skin-Arms impostor capsules (depth only) → the FX prepass, so
                // SSAO/SSR/SSGI/DoF/TAA see the fingers. No-op unless arms are uploaded.
                self.particles.draw_arms_depth(&mut rp);
                // Plexus impostors (depth only) → the FX prepass, so SSAO/SSR/etc see
                // the node spheres + edge tubes. No-op unless plexus impostors uploaded.
                self.particles.draw_plexus_depth(&mut rp);
                // #346 Field Chamber: impostor capsules depth-only into the prepass, so
                // SSAO / SSR / SSGI reconstruct off the panels (no-op in Flat / off).
                self.chamber.draw_impostor_depth(&mut rp);
            }
            if ssao_on {
                // AO source switch (#195 Tier 3): hardware-RT hemisphere rays
                // write the SAME raw target GTAO would, then the existing blur
                // runs — the composite multiply + specular occlusion downstream
                // never know which source filled it. GTAO is the fallback
                // whenever the RT frame is absent (source = GTAO, no ray-query
                // support, sway live, …).
                if let Some(p) = rt_ao {
                    if let Some(target) = self.post.ao_raw_target(device, size) {
                        let pass = self
                            .rt_ao_pass
                            .get_or_insert_with(|| rt_ao::RtAo::new(device));
                        pass.run(device, queue, &mut encoder, target, fx_depth, uniforms, &p);
                    }
                    self.post
                        .blur_ao(device, queue, &mut encoder, fx_depth, depth_key, ssao);
                } else {
                    self.post
                        .compute_ao(device, queue, &mut encoder, fx_depth, depth_key, size, ssao);
                }
            }
        } else if neural_screen_fx {
            // Neural-field GI: no instanced geometry to rasterize, so a depth-only
            // march writes the surface into the SAME single-sample prepass depth
            // (`fx_depth` == prepass_depth_view here, since `shared_prepass` needs
            // the instanced opaque path). SSR/SSGI then reconstruct the normal from
            // depth and gather off the neural surface, composited in post.
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("neural-depth-prepass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: fx_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.neural.draw_depth(
                &mut rp,
                &self.bind_group,
                self.env.ibl_bind(),
                self.rd.scene_bind(),
            );
        } else if voxel_screen_fx {
            // Voxel-field GI (#voxel-pbs Level 2): no instanced geometry to
            // rasterize, so a depth-only DDA march writes the voxel surface into the
            // SAME single-sample prepass depth. SSR/SSGI then reconstruct the normal
            // from depth and gather off the voxel faces, composited in post.
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("voxel-depth-prepass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: fx_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.vox.draw_depth(&mut rp, &self.bind_group, self.env.ibl_bind());
        } else if creature_screen_fx {
            // Creature Engine (#476): no instanced geometry to rasterize, so a
            // depth-only SDF march writes the creature surface into the SAME
            // single-sample prepass depth. SSR/SSGI then reconstruct the normal from
            // depth and gather off the body (neighbour reflections + bounced GI),
            // composited in post.
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("creature-depth-prepass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: fx_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.creature.draw_depth(
                &mut rp,
                &self.bind_group,
                self.env.ibl_bind(),
                self.rd.scene_bind(),
            );
        }
        // #174 T3: point the cube pipeline's specular-occlusion binding (group 3
        // binding 2) at this frame's blurred AO — or back at the white no-op dummy
        // whenever AO wasn't computed (raymarch modes, SSAO off, prepass skipped).
        let ao_ready = ssao_on && depth_fx;
        self.gi
            .ensure_ao(device, if ao_ready { self.post.ao_view() } else { None }, depth_key);

        // 0c) Hardware-RT shadow mask (#195 Tier 1): one any-hit ray per pixel
        //     toward the key light (+ optional fill) against the visual's TLAS,
        //     from the prepass depth into a screen-space visibility mask the cube
        //     shader samples (group 4 binding 5). Only when the prepass actually
        //     ran this frame — otherwise the strengths are zeroed in
        //     `shadow.update` below and the dummy stays bound, so a stale mask is
        //     never read. The pass is created lazily on first use: its pipeline
        //     binds an acceleration structure, which needs the ray-query feature —
        //     guaranteed live here because a TLAS exists.
        let rt_shadow_active = rt_shadow_want && depth_fx;
        let (rt_key_s, rt_fill_s) = rt_shadow
            .filter(|_| rt_shadow_active)
            .map(|p| (p.key_strength, p.fill_strength))
            .unwrap_or((0.0, 0.0));
        if let Some(p) = rt_shadow.filter(|_| rt_shadow_active) {
            let pass = self
                .rt_shadow
                .get_or_insert_with(|| rt_shadow::RtShadow::new(device));
            pass.run(device, queue, &mut encoder, fx_depth, size, uniforms, &p);
        }
        self.shadow.rebind_rt_mask(
            device,
            self.fluidlight.map_view(),
            self.fluidlight.sampler(),
            if rt_shadow_active {
                self.rt_shadow.as_ref().and_then(|p| p.mask())
            } else {
                None
            },
        );

        // Swept-Tubes (`tube`) draws the same per-segment instances as a round
        // cylinder instead of a box; a Boids creature form swaps in a fish/bird/…
        // mesh (one whole creature per instance). Otherwise it's the box.
        let (mesh_vbuf, mesh_ibuf, mesh_index_count) =
            match self.creature_meshes.get(if creature >= 0 { creature as usize } else { usize::MAX }) {
                Some(m) => (&m.0, &m.1, m.2),
                // Neural Tissue single-mesh fallback: closed capsules for non-graph
                // generators (the multi-mesh sub-batch path overrides this when set).
                None if neural_capsule => (&self.capsule_vbuf, &self.capsule_ibuf, self.capsule_index_count),
                None if tube => (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count),
                None => (&self.vbuf, &self.ibuf, self.index_count),
            };

        // The early-Z pass is skipped when the FX prepass already filled the scene
        // depth with the same pipeline (#174 T2, `shared_prepass`).
        if opaque_path && !shared_prepass {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque-depth-prepass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.opaque_prepass_pipeline);
            rp.set_bind_group(0, &self.bind_group, &[]);
            rp.set_bind_group(5, &self.material.bind, &[]); // #472 T5: vs_depth height map
            if demo_live {
                // Demo (#288): per-mesh opaque sub-batches into the early-Z depth.
                self.draw_demo_geometry(&mut rp, demo_batches, true);
            } else if let Some(pb) = &plexus_batches {
                // Plexus Tier-1 shape morph: markers + struts as two morphed sub-batches.
                self.draw_plexus_batches(&mut rp, pb);
            } else if let Some(nb) = &neural_batches {
                self.draw_neural_batches(&mut rp, nb);
            } else {
                rp.set_vertex_buffer(0, mesh_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.inst_buf.slice(..));
                rp.set_vertex_buffer(2, self.tint_buf.slice(..));
                rp.set_vertex_buffer(3, self.emit_buf.slice(..));
                rp.set_index_buffer(mesh_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                rp.draw_indexed(0..mesh_index_count, 0, 0..instances.len() as u32);
            }
        }

        // 0b) Cast-shadow map (#152 Tier 3): render the instanced geometry depth-only
        //     from the key light into the shadow map (reusing the single-sample
        //     depth-prepass pipeline with a light-matrix group-0 uniform). Instanced
        //     path only — the raymarch siblings + membrane don't cast in v1. The cube
        //     shader's `shadow_factor` (group 4) PCF-samples it. `update` runs every
        //     frame so the group-4 `enabled` flag tracks `shadow_cast`.
        // World-space scenery (the pure ride) casts shadows even with the
        // primary generator off/empty (#203 review) — view-locked scenery
        // never does (eye-space coords; see the draw below).
        let scenery_shadow = scenery_live && scenery.map(|sc| !sc.view_locked).unwrap_or(false);
        let shadow_cast = shadow_on
            && (draw_instances || scenery_shadow || draw_swept)
            && !metaball
            && !volume
            && !voxel
            && !membrane
            && !mandelbulb
            && !creature_ray
            && !minimal
            && !kifs_on
            && !neural_on
            && !lens_on;
        self.shadow.update(
            queue,
            Mat4::from_cols_array_2d(&shadow_light_vp),
            shadow_cast,
            shadow_bias,
            shadow_strength,
            if trans_on { coupling.shadow } else { 0.0 },
            if caustic_on { coupling.caustic } else { 0.0 },
            rt_key_s,
            rt_fill_s,
        );
        if shadow_cast {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-map-pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.shadow.map_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.prepass_pipeline); // single-sample depth-only, cull None
            rp.set_bind_group(0, self.shadow.cam_bind(), &[]);
            // #472 T5: the shadow caster displaces with the same height map the
            // scene pass does, so the shadow matches the displaced silhouette.
            rp.set_bind_group(5, &self.material.bind, &[]);
            // Gated on draw_instances (not just shadow_cast): a scenery-only
            // shadow pass must not cast from a hidden/empty generator.
            if draw_instances {
                if demo_live {
                    // Demo (#288): opaque sub-batches cast shadows (glass omitted).
                    self.draw_demo_geometry(&mut rp, demo_batches, true);
                } else if let Some(pb) = &plexus_batches {
                    // Plexus Tier-1 shape morph: markers + struts as two morphed sub-batches.
                    self.draw_plexus_batches(&mut rp, pb);
                } else if let Some(nb) = &neural_batches {
                    self.draw_neural_batches(&mut rp, nb);
                } else {
                    rp.set_vertex_buffer(0, mesh_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.inst_buf.slice(..));
                    rp.set_vertex_buffer(2, self.tint_buf.slice(..));
                    rp.set_vertex_buffer(3, self.emit_buf.slice(..));
                    rp.set_index_buffer(mesh_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                    rp.draw_indexed(0..mesh_index_count, 0, 0..instances.len() as u32);
                }
            } else if draw_swept {
                // Contiguous (welded) Swept Tubes: cast from the welded mesh (one
                // identity instance) so the tubes shadow themselves + the scene.
                rp.set_vertex_buffer(0, self.swept_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                rp.set_vertex_buffer(2, self.white_tint.slice(..));
                rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                rp.set_index_buffer(self.swept_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..swept_idx.len() as u32, 0, 0..1);
            }
            // Plexus OVERLAY Tier-1: the shell casts shadows over whatever the base
            // surface is (instanced, welded, or otherwise), matching standalone Tier-1.
            if let Some(pb) = &plexus_overlay_batches {
                self.draw_plexus_batches_from(
                    &mut rp, pb, &self.plexus_ov_inst_buf, &self.plexus_ov_tint_buf, &self.zero_emit,
                );
            }
            // Scenery layer (#187 pivot): the corridor casts shadows too —
            // but only in the pure ride, where rail space IS world space.
            // View-locked scenery (#187 composite fix) lives in eye space; the
            // world-space light matrix would smear a bogus corridor shadow
            // across the generator.
            if let Some(sc) = scenery {
                if scenery_live && !sc.view_locked {
                    // World-space loft AND the instanced (Streamlines) fallback
                    // both cast — draw whichever are present (#217 review).
                    if scenery_mem_icount > 0 {
                        rp.set_vertex_buffer(0, self.scenery_mem_vbuf.slice(..));
                        rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                        rp.set_vertex_buffer(2, self.white_tint.slice(..));
                        rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                        rp.set_index_buffer(self.scenery_mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..scenery_mem_icount as u32, 0, 0..1);
                    }
                    if scenery_count > 0 {
                        let (svb, sib, sic) = if sc.tube {
                            (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count)
                        } else {
                            (&self.vbuf, &self.ibuf, self.index_count)
                        };
                        rp.set_vertex_buffer(0, svb.slice(..));
                        rp.set_vertex_buffer(1, self.scenery_inst_buf.slice(..));
                        rp.set_vertex_buffer(2, self.scenery_tint_buf.slice(..));
                        rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                        rp.set_index_buffer(sib.slice(..), wgpu::IndexFormat::Uint16);
                        rp.draw_indexed(0..sic, 0, 0..scenery_count as u32);
                    }
                }
            }
        }

        // 1) Scene → linear HDR buffer. Opaque path: the prepass already filled
        // depth, so LOAD it, shade cubes with Equal + no write (back-face culled),
        // and fill the backdrop only where no geometry wrote depth. Glass/membrane/
        // metaball: CLEAR depth and paint the far plane as before. With MSAA on,
        // `scene_view` is multisampled and resolves into `resolve` (the single-
        // sample HDR buffer the bloom/composite read).
        // Capture decoration (#135 P5): upload axis surface + box-wall lines + camera
        // before the pass (buffer writes can't happen inside a render pass).
        self.axes.prepare(device, queue, &uniforms.view_proj, axes_solids, box_lines);
        self.chamber.prepare(device, queue, &uniforms.view_proj, chamber_surfs, chamber_lines);
        // Creature anatomy overlay (#476 Tier 2c): build the spine/ring/limb diagram
        // from the SAME body plan the raymarch uses (warp-glued to the swim), and
        // upload it for the two-pass line draw after the creature. Only when active.
        if creature_ray && creature_params.overlay_on {
            let lines = creature_overlay::build_creature_overlay(
                creature_params.prims,
                creature_params.scale,
                creature_params.swim_phase,
                creature_params.warp_freq,
                creature_params.warp_amp,
            );
            self.creature_overlay.prepare(
                device,
                queue,
                &uniforms.view_proj,
                &lines,
                creature_params.overlay_bright,
                creature_params.overlay_opacity,
            );
        }
        let chamber_imp = super::chamber::ImpostorFrame {
            view_proj: uniforms.view_proj,
            cam_right: chamber_cam_right,
            cam_up: chamber_cam_up,
            cam_pos: [uniforms.camera_pos[0], uniforms.camera_pos[1], uniforms.camera_pos[2]],
            prefilter_mips: uniforms.mat[3],
            key_light: uniforms.key_light,
            fill_light: uniforms.fill_light,
            env: [uniforms.env[0], uniforms.env[1], uniforms.env[2], uniforms.amb[0]],
            env_tint: [uniforms.env_tint[0], uniforms.env_tint[1], uniforms.env_tint[2]],
            material: chamber_material,
            opacity: chamber_opacity,
        };
        self.chamber.prepare_impostor(device, queue, &chamber_imp, chamber_beads);
        {
            let (scene_view, resolve) = self.post.scene_targets(device, size);
            let depth_load = if opaque_path {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(1.0)
            };
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), // sky/cubes overpaint
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Background: the terrain backdrop (raymarched, draws its own sky)
            // replaces the skybox when enabled; both fill only the background
            // (Equal/no-write on the opaque path, Always/write otherwise).
            if terrain_pass {
                if terrain_scale > 1 {
                    self.terrain.upscale_draw(&mut rp, opaque_path);
                } else {
                    self.terrain.draw(&mut rp, opaque_path);
                }
            } else {
                rp.set_pipeline(if opaque_path { &self.sky_pipeline_eq } else { &self.sky_pipeline });
                rp.set_bind_group(0, &self.sky_bind, &[]);
                rp.set_bind_group(1, self.env.sky_env_bind(), &[]);
                rp.draw(0..3, 0..1);
            }

            // Starfield + HDR sun: additive over the background (far-plane depth, so
            // the generator composites in front). Drawn whether the background is the
            // terrain or the skybox.
            if stars_on {
                self.stars.draw(&mut rp, opaque_path, star_sun);
            }

            if voxel {
                // Voxel: DDA-raymarch the splatted occupancy grid as crisp cubes,
                // physically shaded — group 0 = camera + lights, group 1 = IBL maps
                // for the metallic-roughness PBR + Material card.
                self.vox.draw(&mut rp, &self.bind_group, self.env.ibl_bind());
            } else if metaball {
                // Metaball: raymarch the baked field as one contiguous skin,
                // reusing the cube uniform (group 0) + IBL (group 1) + RD (group 3).
                self.meta.draw(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind(), self.gi.bind(), self.vxgi.sample_bind());
            } else if volume {
                // Volume (#152): raymarch the SAME baked field as a glowing medium
                // (alpha-over the sky), reusing the same shared bind groups.
                self.meta.draw_volume(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind(), self.gi.bind(), self.vxgi.sample_bind());
            } else if mandelbulb {
                // Mandelbulb: analytic DE raymarch, same shared bind groups.
                self.mandel.draw(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind());
            } else if creature_ray {
                // Creature Engine (#476): union-of-SDF-primitives raymarch, same
                // shared bind groups as Mandelbulb (uniforms + IBL + RD).
                self.creature.draw(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind());
                // Tier 2c: the anatomy diagram over it (two-pass line occlusion),
                // after the creature has written depth. Inert unless prepared (off).
                if creature_params.overlay_on {
                    self.creature_overlay.draw(&mut rp);
                }
            } else if minimal {
                // Minimal surface (#127): TPMS isosurface raymarch, same shared
                // bind groups as Mandelbulb (uniforms + IBL + RD).
                self.minimal.draw(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind());
            } else if kifs_on {
                // Kaleidoscopic Fractal: the flat/tunnel field overpaints the
                // background (screen-space, camera-independent).
                self.kifs.draw(&mut rp);
            } else if neural_on {
                // Neural field (#200 Tier 1): MLP isosurface raymarch, same shared
                // bind groups as Mandelbulb (uniforms + IBL + RD).
                self.neural.draw(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind());
            } else if lens_on {
                // Lens (#258 Tier 3): analytic lens-SDF sphere-trace, same shared
                // bind groups as Mandelbulb (uniforms + IBL + RD).
                self.lens.draw(&mut rp, &self.bind_group, self.env.ibl_bind(), self.rd.scene_bind());
            } else if membrane {
                rp.set_bind_group(0, &self.bind_group, &[]);
                rp.set_bind_group(1, self.env.ibl_bind(), &[]);
                rp.set_bind_group(2, self.rd.scene_bind(), &[]);
                rp.set_bind_group(3, self.gi.bind(), &[]); // GI probe volume (#80 B)
                rp.set_bind_group(4, self.shadow.bind(), &[]); // shadow map (#152 T3)
                rp.set_bind_group(5, &self.material.bind, &[]); // #472 material set
                // Boundary strands (swept tubes) under the membrane — drawn when
                // explicitly shown (Show Strands) OR as the Skin-Arms Impostor-build
                // placeholder (per-segment rods; the shell sheet is absent in arms mode).
                let arm_rods = membrane_arms && mem_icount == 0 && !draw_swept;
                if (show_strands || arm_rods) && !instances.is_empty() {
                    rp.set_pipeline(&self.pipeline);
                    let (vbuf, ibuf, index_count) = if tube {
                        (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count)
                    } else {
                        (&self.vbuf, &self.ibuf, self.index_count)
                    };
                    rp.set_vertex_buffer(0, vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.inst_buf.slice(..));
                    rp.set_vertex_buffer(2, self.tint_buf.slice(..));
                    rp.set_vertex_buffer(3, self.emit_buf.slice(..));
                    rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                    rp.draw_indexed(0..index_count, 0, 0..instances.len() as u32);
                }
                if mem_icount > 0 {
                    // The lofted shell sheet: one identity instance, per-vertex colour
                    // (the white tint's w=0 selects the mesh colour in the shader).
                    rp.set_pipeline(&self.pipeline);
                    rp.set_vertex_buffer(0, self.mem_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                    rp.set_vertex_buffer(2, self.white_tint.slice(..));
                    rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                    rp.set_index_buffer(self.mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mem_icount as u32, 0, 0..1);
                } else if membrane_arms && draw_swept {
                    // Skin-Arms Mesh build: each arm welded into one capped finger,
                    // drawn like the sheet (identity instance, w=0 white tint → baked
                    // sweep colour), so PBR/IBL/Chrome/Glass + MSAA all apply.
                    rp.set_pipeline(&self.pipeline);
                    rp.set_vertex_buffer(0, self.swept_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                    rp.set_vertex_buffer(2, self.white_tint.slice(..));
                    rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                    rp.set_index_buffer(self.swept_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..swept_idx.len() as u32, 0, 0..1);
                }
                // Skin-Arms Impostor build: per-segment capsule impostors, IBL-shaded
                // by the shared bead pipeline (their own DrawU + Material context,
                // uploaded above). Opaque, depth-written; gaps between arms are free.
                if membrane_arms && !arm_caps.is_empty() {
                    self.particles.draw_arms(&mut rp, self.env.ibl_bind());
                }
            } else if splat_on {
                // Gaussian Splatting surface: anisotropic Gaussians drawn from the node
                // instances into the HDR scene buffer. Tier 1 = additive (unlit); Tier 2
                // = IBL-lit 2DGS disks (sorted alpha) — the draw binds the shared IBL
                // group. Depth-tested against the scene, no depth write.
                self.splats.draw(&mut rp, self.env.ibl_bind());
            } else if draw_swept {
                // Contiguous Swept-Tubes: one welded mesh (u32 indices) per strand,
                // drawn as a single identity instance with the w=0 white tint so the
                // shader uses the baked per-vertex sweep colour. Shares the cube
                // pipeline + bind groups (0..4) so MSAA + PBR/IBL + Chrome/Glass all
                // apply, exactly like the membrane sheet. `instances` is empty in this
                // mode, so the instanced draw + SSAO prepass no-op themselves.
                rp.set_pipeline(&self.pipeline);
                rp.set_bind_group(0, &self.bind_group, &[]);
                rp.set_bind_group(1, self.env.ibl_bind(), &[]);
                rp.set_bind_group(2, self.rd.scene_bind(), &[]);
                rp.set_bind_group(3, self.gi.bind(), &[]);
                rp.set_bind_group(4, self.shadow.bind(), &[]);
                rp.set_bind_group(5, &self.material.bind, &[]); // #472 material set
                rp.set_vertex_buffer(0, self.swept_vbuf.slice(..));
                rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                rp.set_vertex_buffer(2, self.white_tint.slice(..));
                rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                rp.set_index_buffer(self.swept_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..swept_idx.len() as u32, 0, 0..1);
            } else if draw_instances {
                // Opaque → Equal/no-write + back-face cull (depth from the prepass);
                // Glass → Less/write, both faces, single-pass.
                rp.set_pipeline(if opaque_path { &self.pipeline_opaque } else { &self.pipeline });
                rp.set_bind_group(0, &self.bind_group, &[]);
                rp.set_bind_group(1, self.env.ibl_bind(), &[]);
                rp.set_bind_group(2, self.rd.scene_bind(), &[]);
                rp.set_bind_group(3, self.gi.bind(), &[]); // GI probe volume (#80 B)
                rp.set_bind_group(4, self.shadow.bind(), &[]); // shadow map (#152 T3)
                rp.set_bind_group(5, &self.material.bind, &[]); // #472 material set
                if demo_live {
                    // Demo scene bench (#288): per-(mesh,material) sub-batches, each
                    // with its own mesh + patched group-0 material (binds 1–4 stay).
                    self.draw_demo_scene(&mut rp, demo_batches);
                } else if let Some(pb) = &plexus_batches {
                    // Plexus Tier-1 shape morph: markers + struts as two morphed sub-batches.
                    self.draw_plexus_batches(&mut rp, pb);
                } else if let Some(nb) = &neural_batches {
                    // Neural Tissue (#260 Tier 1): soma / capsule / bouton sub-batches,
                    // each with its own mesh. Same pipeline + bind groups as above.
                    self.draw_neural_batches(&mut rp, nb);
                } else {
                    rp.set_vertex_buffer(0, mesh_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.inst_buf.slice(..));
                    rp.set_vertex_buffer(2, self.tint_buf.slice(..));
                    rp.set_vertex_buffer(3, self.emit_buf.slice(..));
                    rp.set_index_buffer(mesh_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                    rp.draw_indexed(0..mesh_index_count, 0, 0..instances.len() as u32);
                }
            }

            // Scenery layer (#187 pivot): the concurrent corridor, drawn with
            // its OWN material uniforms (group 0 = scenery_bind) through the
            // blend pipeline (Less + write — correct for any scenery material
            // and independent of the main path's Equal/early-Z route; the
            // shared-prepass depth already contains the scenery, so the
            // LessEqual test passes on its own pixels). Draws in every main
            // path except the fullscreen KIFS field, which owns the frame.
            // NeuralField is a sibling of Mandelbulb (which DOES draw scenery), so
            // the Zone corridor / scenery stays visible under it (#224 review).
            if scenery_live && path != RenderPath::Kifs {
                if let Some(sc) = scenery {
                    rp.set_bind_group(0, &self.scenery_bind, &[]);
                    rp.set_bind_group(1, self.env.ibl_bind(), &[]);
                    rp.set_bind_group(2, self.rd.scene_bind(), &[]);
                    rp.set_bind_group(3, self.gi.bind(), &[]);
                    rp.set_bind_group(4, self.shadow.bind(), &[]);
                    rp.set_bind_group(5, &self.material.bind, &[]); // #472 material set
                    // Skin membrane AND the instanced (Streamlines) fallback can
                    // both be present in a mixed transition — draw each when
                    // non-empty (#217 review), no longer either/or.
                    if scenery_mem_icount > 0 {
                        // The lofted sheet is single-sided, so it draws cull None
                        // + write. LessEqual (`pipeline_skin`, #217 review) so the
                        // shared-prepass route — where the FX prepass already
                        // wrote the skin's depth — doesn't reject it with a plain
                        // Less; identical to Less on every other route.
                        rp.set_pipeline(&self.pipeline_skin);
                        rp.set_vertex_buffer(0, self.scenery_mem_vbuf.slice(..));
                        rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                        rp.set_vertex_buffer(2, self.white_tint.slice(..));
                        rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                        rp.set_index_buffer(self.scenery_mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..scenery_mem_icount as u32, 0, 0..1);
                    }
                    if scenery_count > 0 {
                        // Instanced scenery. Shared-prepass route: depth was
                        // pre-filled by the FX prepass INCLUDING the scenery
                        // (cull Back), so shade Equal/no-write like the main
                        // geometry. Every other route: the Less + write blend
                        // pipeline is correct for any scenery material.
                        rp.set_pipeline(if shared_prepass {
                            &self.pipeline_opaque
                        } else {
                            &self.pipeline
                        });
                        let (svb, sib, sic) = if sc.tube {
                            (&self.cyl_vbuf, &self.cyl_ibuf, self.cyl_index_count)
                        } else {
                            (&self.vbuf, &self.ibuf, self.index_count)
                        };
                        rp.set_vertex_buffer(0, svb.slice(..));
                        rp.set_vertex_buffer(1, self.scenery_inst_buf.slice(..));
                        rp.set_vertex_buffer(2, self.scenery_tint_buf.slice(..));
                        rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                        rp.set_index_buffer(sib.slice(..), wgpu::IndexFormat::Uint16);
                        rp.draw_indexed(0..sic, 0, 0..scenery_count as u32);
                    }
                }
            }

            // Scenery water floor (#206 Tier 3): the channel water, drawn with
            // its OWN (third) material uniforms (group 0 = water_bind) through the
            // skin pipeline (LessEqual + write, cull None + alpha blend — correct
            // for the single-sided glass sheet, and LessEqual admits it on the
            // shared-prepass route where the water prepass already wrote its
            // depth). Drawn after the scenery so it composites into the valley.
            if water_live && path != RenderPath::Kifs {
                if let Some(_w) = water {
                    rp.set_bind_group(0, &self.water_bind, &[]);
                    rp.set_bind_group(1, self.env.ibl_bind(), &[]);
                    rp.set_bind_group(2, self.rd.scene_bind(), &[]);
                    rp.set_bind_group(3, self.gi.bind(), &[]);
                    rp.set_bind_group(4, self.shadow.bind(), &[]);
                    rp.set_bind_group(5, &self.material.bind, &[]); // #472 material set
                    rp.set_pipeline(&self.pipeline_skin);
                    rp.set_vertex_buffer(0, self.water_mem_vbuf.slice(..));
                    rp.set_vertex_buffer(1, self.identity_inst.slice(..));
                    rp.set_vertex_buffer(2, self.white_tint.slice(..));
                    rp.set_vertex_buffer(3, self.zero_emit.slice(..));
                    rp.set_index_buffer(self.water_mem_ibuf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..water_mem_icount as u32, 0, 0..1);
                }
            }

            // MLS-MPM liquid surface (#182 Tier 3a): raymarch the splatted
            // density field with the metaball isosurface pipeline — full
            // material stack (Glass = water), depth-composited against the
            // geometry drawn above.
            if liquid_on {
                if liquid.render_mode == 0 {
                self.liquid_meta.draw(
                    &mut rp,
                    &self.liquid_bind,
                    self.env.ibl_bind(),
                    self.rd.scene_bind(),
                    self.gi.bind(),
                    self.vxgi.sample_bind(),
                );
                }
            }

            // Particle Aura: additive HDR sparks over the geometry (depth-tested, no
            // write) — or, at #298 Tier 1, opaque IBL-shaded beads (depth-write on)
            // that occlude each other + the scene. No-op when disabled.
            self.particles.draw(&mut rp, self.env.ibl_bind());
            // Plexus OVERLAY Tier-1: draw the outer-shell markers+struts over whatever
            // base surface is active (any RenderPath), independent of the base's depth
            // route. The main uniforms (group 0) + IBL/scene/GI/shadow binds are
            // re-asserted since the base draw may have left different ones bound.
            // Pipeline choice mirrors the base surface: on the shared-prepass route the
            // FX depth-prepass already rasterized this SAME overlay geometry into the
            // scene depth (cull Back), so shade Equal/no-write (`pipeline_opaque`) — the
            // Less+write pipeline would reject every overlay fragment at equal depth
            // (invisible web) AND punch the base surface's Equal test wherever the overlay
            // wrote a nearer depth (black holes). Every other route has no matching overlay
            // prepass depth, so the Less+write glass/fallback pipeline (cull None)
            // composites it by depth. No-op when absent.
            if let Some(pb) = &plexus_overlay_batches {
                rp.set_pipeline(if shared_prepass {
                    &self.pipeline_opaque
                } else {
                    &self.pipeline
                });
                // Shape-zeroed overlay uniform (not the base `bind_group`, whose bevel
                // must not double-morph the overlay's own morph meshes); matches the
                // overlay's FX-prepass depth so the Equal test holds.
                rp.set_bind_group(0, &self.plexus_ov_bind, &[]);
                rp.set_bind_group(1, self.env.ibl_bind(), &[]);
                rp.set_bind_group(2, self.rd.scene_bind(), &[]);
                rp.set_bind_group(3, self.gi.bind(), &[]);
                rp.set_bind_group(4, self.shadow.bind(), &[]);
                rp.set_bind_group(5, &self.material.bind, &[]); // #472 material set
                self.draw_plexus_batches_from(
                    &mut rp, pb, &self.plexus_ov_inst_buf, &self.plexus_ov_tint_buf, &self.zero_emit,
                );
            }
            // Plexus Tier 2 impostors: node spheres + edge tubes, opaque + IBL-shaded,
            // each with its own material. No-op unless plexus impostors were uploaded.
            self.particles.draw_plexus(&mut rp, self.env.ibl_bind());

            // Capture decoration (#135 P5): XYZ axes + wireframe box, depth-tested
            // against the geometry so they sit correctly in 3-D. No-op when empty.
            self.axes.draw(&mut rp);
            self.chamber.draw(&mut rp);
            self.chamber.draw_impostor(&mut rp, self.env.ibl_bind());
        }

        // 1b) Voxel GI (#152 Tier 3, #10): march the voxelized field from the prepass
        //     depth and ADD the world-space bounce straight into the resolved HDR
        //     buffer (before bloom, so it blooms + tonemaps with the scene). Runs
        //     BEFORE SSR/SSGI so the screen-space passes — which sample this same
        //     HDR buffer — see the voxel bounce: with the old order a VXGI-lit face
        //     was darker in its own reflection than on screen.
        if vxgi_cast && depth_fx {
            if let Some(hdr) = self.post.hdr_view() {
                let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
                let cam_pos = Vec3::new(
                    uniforms.camera_pos[0],
                    uniforms.camera_pos[1],
                    uniforms.camera_pos[2],
                );
                self.vxgi.render(
                    device,
                    queue,
                    &mut encoder,
                    hdr,
                    fx_depth,
                    depth_key,
                    inv_vp,
                    cam_pos,
                    gi_min,
                    gi_max,
                    size,
                    &vxgi,
                );
            }
        }
        // 1c) Inter-cube reflections. Hardware-RT (#195 Tier 2) when on: trace the
        //     scene's own geometry against the TLAS into the SAME confidence-
        //     weighted buffer the composite blends — no screen-edge dropout, and
        //     it SUPERSEDES the SSR march (one reflection source at a time).
        //     Else SSR (#80 Part A): march the resolved HDR + the prepass depth.
        //     Either way, only when the prepass actually ran this frame.
        let rt_reflect_active = rt_reflect_want && depth_fx;
        // HYBRID RT + SSR (opt-in by enabling both): RT reflections only see the
        // instanced field in the TLAS — everything else (terrain, particle aura,
        // membrane, the Z0NE corridor's non-instanced content flying past, sky
        // glow) is an RT miss → flat env. SSR marches the resolved on-screen HDR,
        // so it catches exactly that. Run SSR FIRST (it clears + fills the
        // reflection buffer), then RT composites OVER it (premultiplied src-over,
        // `load = true`): RT hits replace SSR where the field is, RT misses leave
        // SSR showing the rest — the confidence weights layer RT > SSR > env.
        // Legacy paths preserved: RT-only clears + src-over onto transparent =
        // byte-identical to the old overwrite; SSR-only is untouched.
        // The RT+SSR hybrid (SSR fills the buffer, RT composites over it) and the
        // temporal accumulator can't both drive the reflection buffer: with
        // temporal on, RT writes the RAW buffer and the accumulator writes the SSR
        // VIEW — so an SSR fill of the view would just be clobbered, and RT's
        // load+blend would read the fresh raw buffer, not SSR. So when RT
        // reflections + temporal are both on, skip the hybrid SSR fill (temporal
        // wins). SSR-only and RT+SSR-without-temporal are unaffected. (Full
        // hybrid+temporal compositing would route SSR through the raw buffer too —
        // a follow-up.)
        let ssr_first =
            ssr_on && run_prepass && !(rt_reflect_active && rt_temporal.is_some());
        if ssr_first {
            self.post
                .compute_ssr(device, queue, &mut encoder, fx_depth, depth_key, size, ssr);
        }
        if let Some(p) = rt_reflect.filter(|_| depth_fx) {
            // Temporal on (#200 T4½ p3): the RT pass writes the shared RAW buffer
            // and the accumulator writes the SSR view (composite reads it);
            // off: the RT pass writes the SSR view directly. Either way ensure the
            // SSR view exists + is composite-bound.
            let rt_target = if rt_temporal.is_some() {
                self.post.ensure_ssr_target(device, size);
                self.post.ensure_rt_raw(device, size)
            } else {
                self.post.ensure_ssr_target(device, size)
            };
            if let Some(target) = rt_target {
                let pass = self
                    .rt_reflect_pass
                    .get_or_insert_with(|| rt_reflect::RtReflect::new(device));
                pass.run(
                    device,
                    queue,
                    &mut encoder,
                    target,
                    fx_depth,
                    uniforms,
                    &self.inst_buf,
                    &self.tint_buf,
                    &self.emit_buf, // organon#217 T8: the hit reads its own emission
                    tube,
                    ssr_first, // hybrid: load + blend RT over the SSR fill
                    &p,
                );
            }
            if let Some(tf) = rt_temporal {
                let inv_vp = Mat4::from_cols_array_2d(&tf.cur_view_proj)
                    .inverse()
                    .to_cols_array_2d();
                self.post.temporal(
                    device,
                    queue,
                    &mut encoder,
                    RtBuffer::Reflection,
                    fx_depth,
                    inv_vp,
                    tf.prev_view_proj,
                    tf.feedback,
                    tf.beat_relax_factor,
                    tf.variance,
                    tf.max_accum,
                    tf.clamp_gamma,
                    size,
                );
            }
        }
        // RT denoise (#200 Tier 4½ part 2): edge-aware à-trous over the
        // reflection buffer, in place, before the composite reads it. Roughness-
        // adaptive — sharp mirrors (low roughness) aren't touched, since their
        // reflection isn't jittered. Gated on `rt_reflect_active`, so SSR-ONLY
        // (no RT reflections) is never denoised — the screen-space march stays
        // byte-identical. In the RT+SSR HYBRID the buffer holds RT composited
        // over SSR, and the whole thing is filtered — intended: the SSR content
        // (terrain/particles/corridor) is itself march-noisy and the edge-aware
        // filter cleans it without crossing silhouettes (#211 review).
        if rt_reflect_active && rt_denoise > 0.0 {
            let rough = uniforms.mat[1];
            // smoothstep(0.03, 0.25, roughness): 0 below 0.03 (mirror), 1 above 0.25.
            let x = ((rough - 0.03) / (0.25 - 0.03)).clamp(0.0, 1.0);
            let refl_strength = rt_denoise * (x * x * (3.0 - 2.0 * x));
            if refl_strength > 0.0 {
                let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj)
                    .inverse()
                    .to_cols_array_2d();
                // Tier 5a: route through the kernel-predicting neural filter when
                // enabled (net = 0 reproduces the classical à-trous exactly);
                // else the classical denoiser (byte-identical).
                if let Some(nd) = rt_ndenoise {
                    self.post.neural_denoise(
                        device,
                        queue,
                        &mut encoder,
                        post::DenoiseTarget::Reflection,
                        fx_depth,
                        inv_vp,
                        uniforms.camera_pos,
                        size,
                        refl_strength,
                        0.03,
                        0.4,
                        nd.net,
                        nd.seed,
                        nd.omega,
                    );
                } else {
                    self.post.denoise(
                        device,
                        queue,
                        &mut encoder,
                        post::DenoiseTarget::Reflection,
                        fx_depth,
                        inv_vp,
                        uniforms.camera_pos,
                        size,
                        refl_strength,
                        0.03,
                        0.4,
                    );
                }
            }
        }
        // 1d) Diffuse GI into the buffer the composite adds. Hardware-RT (#195
        //     Tier 4) when on: gather one indirect bounce against the TLAS — real
        //     inter-cube colour bleed incl. off-screen emitters — and it
        //     SUPERSEDES the SSGI march (one GI source at a time). Else SSGI
        //     (#152 Tier 2): the screen-space neighbour gather. Only when the
        //     prepass ran this frame.
        let rt_gi_active = rt_gi.is_some() && depth_fx;
        let ssgi_active = (ssgi_on && run_prepass) || rt_gi_active;
        if let Some(p) = rt_gi.filter(|_| depth_fx) {
            // Temporal path mirrors reflections: RT → raw, accumulator → SSGI view.
            let rt_target = if rt_temporal.is_some() {
                self.post.ensure_ssgi_target(device, size);
                self.post.ensure_rt_raw(device, size)
            } else {
                self.post.ensure_ssgi_target(device, size)
            };
            if let Some(target) = rt_target {
                let pass = self.rt_gi_pass.get_or_insert_with(|| rt_gi::RtGi::new(device));
                pass.run(
                    device,
                    queue,
                    &mut encoder,
                    target,
                    fx_depth,
                    uniforms,
                    &self.inst_buf,
                    &self.tint_buf,
                    &self.emit_buf, // organon#217 T8: a lit tile is a neighbour that emits
                    tube,
                    &p,
                );
            }
            if let Some(tf) = rt_temporal {
                let inv_vp = Mat4::from_cols_array_2d(&tf.cur_view_proj)
                    .inverse()
                    .to_cols_array_2d();
                self.post.temporal(
                    device,
                    queue,
                    &mut encoder,
                    RtBuffer::Gi,
                    fx_depth,
                    inv_vp,
                    tf.prev_view_proj,
                    tf.feedback,
                    tf.beat_relax_factor,
                    tf.variance,
                    tf.max_accum,
                    tf.clamp_gamma,
                    size,
                );
            }
        } else if ssgi_on && run_prepass {
            self.post
                .compute_ssgi(device, queue, &mut encoder, fx_depth, depth_key, size, ssgi);
        }
        // RT denoise (#200 Tier 4½ part 2): the GI buffer is diffuse (low-freq),
        // so it's always safe to filter at full strength. RT-written only.
        if rt_gi_active && rt_denoise > 0.0 {
            let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj)
                .inverse()
                .to_cols_array_2d();
            if let Some(nd) = rt_ndenoise {
                self.post.neural_denoise(
                    device,
                    queue,
                    &mut encoder,
                    post::DenoiseTarget::Gi,
                    fx_depth,
                    inv_vp,
                    uniforms.camera_pos,
                    size,
                    rt_denoise,
                    0.03,
                    0.4,
                    nd.net,
                    nd.seed,
                    nd.omega,
                );
            } else {
                self.post.denoise(
                    device,
                    queue,
                    &mut encoder,
                    post::DenoiseTarget::Gi,
                    fx_depth,
                    inv_vp,
                    uniforms.camera_pos,
                    size,
                    rt_denoise,
                    0.03,
                    0.4,
                );
            }
        }
        // 1e0) Refractive liquid (#182 T3b route B, first slice): snapshot the
        //      resolved scene (GI/SSR/SSGI already in), march the liquid field,
        //      Fresnel-split at the live IOR, refract the view ray and fetch
        //      the scene where the bent ray lands, Beer–Lambert-absorbed over
        //      the measured thickness. Runs before the ink so smoke composites
        //      over the water.
        if liquid_on && liquid.render_mode == 1 {
            if let (Some(hdr_tex), Some(hdr_view)) =
                (self.post.hdr_texture(), self.post.hdr_view())
            {
                let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
                let cam_pos = Vec3::new(
                    uniforms.camera_pos[0],
                    uniforms.camera_pos[1],
                    uniforms.camera_pos[2],
                );
                // Effective material: the liquid override, else the scene's.
                let (rough, ior) = liquid
                    .material
                    .map(|m| (m.roughness, m.ior))
                    .unwrap_or((uniforms.mat[1], uniforms.amb[2]));
                self.liquidsurf.render(
                    device,
                    queue,
                    &mut encoder,
                    hdr_tex,
                    hdr_view,
                    size,
                    self.env.ibl_bind(),
                    &liquidsurf::LiquidSurfFrame {
                        view_proj: Mat4::from_cols_array_2d(&uniforms.view_proj),
                        inv_vp,
                        cam: cam_pos,
                        tank_min: liquid.container_min,
                        tank_max: liquid.container_max,
                        threshold: liquid.surface.threshold,
                        steps: 96.0,
                        color: liquid.params.color,
                        absorption: liquid.absorb,
                        ior: ior.max(1.001),
                        roughness: rough,
                        env_rotation: uniforms.env[2],
                        env_intensity: uniforms.env[1] * uniforms.amb[0],
                        field_view: self.liquid_meta.field_view(),
                        depth: if depth_fx { Some(fx_depth) } else { None },
                        gi: (
                            self.gi.probes_buffer(),
                            self.vxgi.volume_view(),
                            self.vxgi.linear_sampler(),
                            gi_min,
                            gi_max,
                            if vxgi_volume { vxgi.intensity } else { 0.0 },
                        ),
                        lights_buf: self.gi.lights_buffer(),
                    },
                );
            }
        }

        // 1e0b) Screen-space refraction (#214 Tier 5 pt 2): the instanced Refractive
        //      material's see-through of the real scene behind it. Reconstruct the
        //      covered pixels from the prepass depth, refract the view ray at the
        //      IOR, and replace the env-only transmission with the displaced resolved
        //      scene (cubes show their neighbours). Only on the Refractive material
        //      with the strength dial up + a valid prepass depth (byte-identical off).
        if refract_ss_want && depth_fx {
            if let (Some(hdr_tex), Some(hdr_view)) =
                (self.post.hdr_texture(), self.post.hdr_view())
            {
                let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
                let cam_pos = Vec3::new(
                    uniforms.camera_pos[0],
                    uniforms.camera_pos[1],
                    uniforms.camera_pos[2],
                );
                self.refractsurf.render(
                    device,
                    queue,
                    &mut encoder,
                    hdr_tex,
                    hdr_view,
                    size,
                    &refractsurf::RefractSurfFrame {
                        view_proj: Mat4::from_cols_array_2d(&uniforms.view_proj),
                        inv_vp,
                        cam: cam_pos,
                        ior: uniforms.amb[2].max(1.001),
                        // The forward pass already applied the per-instance colour
                        // absorption; the post pass only displaces, so no colour tint
                        // here (white → σ 0 → no double-absorb).
                        absorption: 0.0,
                        tint: Vec3::ONE,
                        strength: refract_ss,
                        dist: refract_dist,
                        depth: fx_depth,
                    },
                );
            }
        }

        // 1e) Fluid Ink (#182 Tier 1): blit the evolved dye into its 3D texture,
        //     raymarch it (key HG scatter + IBL ambient + emissive, Beer–Lambert),
        //     and composite over the resolved HDR buffer — before bloom, so the
        //     ink blooms/tonemaps/EDRs with the scene. The march clamps at the
        //     prepass depth when it ran (ink swirls around the geometry); in the
        //     raymarch / hidden-generator cases it marches the whole volume.
        if ink_active {
            if let Some(hdr) = self.post.hdr_view() {
                let inv_vp = Mat4::from_cols_array_2d(&uniforms.view_proj).inverse();
                let cam_pos = Vec3::new(
                    uniforms.camera_pos[0],
                    uniforms.camera_pos[1],
                    uniforms.camera_pos[2],
                );
                let key_dir = Vec3::new(
                    uniforms.key_light[0],
                    uniforms.key_light[1],
                    uniforms.key_light[2],
                );
                let env_tint =
                    Vec3::new(uniforms.env_tint[0], uniforms.env_tint[1], uniforms.env_tint[2]);
                let depth = if depth_fx { Some((fx_depth, depth_key)) } else { None };
                self.fluidvis.render(
                    device,
                    queue,
                    &mut encoder,
                    hdr,
                    depth,
                    size,
                    inv_vp,
                    cam_pos,
                    particles.grid_min,
                    particles.grid_max,
                    self.fluid.dye_buffer(),
                    self.fluid.curl_buffer(),
                    self.fluid.res()[0],
                    self.fluid.epoch(),
                    key_dir,
                    uniforms.key_light[3],
                    uniforms.env[1] * uniforms.amb[0],
                    uniforms.env[2],
                    env_tint,
                    particles.time,
                    self.env.ibl_bind(),
                    &ink.params,
                    (
                        Mat4::from_cols_array_2d(&shadow_light_vp),
                        self.shadow.map_view(),
                        self.shadow.comparison_sampler(),
                        // Gate on the shadow PASS having run this frame, not just
                        // the param: with no casters (hidden generator, raymarch
                        // modes) the map is stale — or zero-init depth 0, which
                        // would read as "everything occluded" and black the smoke.
                        if coupling.receive && shadow_cast { shadow_strength } else { 0.0 },
                    ),
                    (
                        self.gi.probes_buffer(),
                        self.vxgi.volume_view(),
                        self.vxgi.linear_sampler(),
                        gi_min,
                        gi_max,
                        // Only when the volume was voxelized this frame (else stale).
                        if vxgi_volume { vxgi.intensity } else { 0.0 },
                    ),
                );
            }
        }

        // 1z) Path tracer (#200 Tier 4 — ground-truth substrate). Overwrite the HDR
        //     scene buffer with the traced result just before bloom/composite, so
        //     exposure / tone-map / EDR all apply to it. The raster scene pass
        //     above ran but is overwritten; the visual nulls the screen-space
        //     light effects (SSR/SSGI/AO/rt_gi/VXGI) when active so nothing else
        //     touches the buffer and the composite adds nothing extra. Reuses the
        //     TLAS + the live instance/tint buffers for hit shading.
        if let Some(pf) = pathtrace {
            if let Some(hdr) = self.post.hdr_view() {
                let pt = self
                    .pathtracer
                    .get_or_insert_with(|| rt_pathtrace::PathTracer::new(device));
                pt.trace(
                    device, queue, &mut encoder, hdr, size, uniforms,
                    // organon#217 T8: the tracer reads the per-instance emission the
                    // cube pipeline draws, so the T5 dwell converges lit, not dark.
                    &self.inst_buf, &self.tint_buf, &self.emit_buf, tube, &pf,
                );
            }
        }

        // 2) Bloom + exposure + tonemap composite (+ SSR + SSGI), then the optional
        //    post-composite chain. Both the temporal pass (#152 Tier 2: TAA + motion
        //    blur) and the creative FX pass (#152 Tier 1) operate on the composited
        //    image at full output resolution, each reading a source texture and writing
        //    onward. Chain order: composite → temporal (TAA resolve / motion blur) → FX
        //    (NPR / DoF / lens FX / grade / feedback) → view. Any subset may be active;
        //    with none on, the composite writes straight to the view (default path,
        //    byte-identical). The composite upscales the scaled render buffer into the
        //    full-res src/view either way.
        // RT reflections (#195 Tier 2) fill the same buffer SSR does, so they
        // turn the composite's SSR blend on too.
        let ssr_active = (ssr_on && run_prepass) || rt_reflect_active;
        // SSAO/SSR/DoF/TAA all read the same single-sample depth when it ran
        // (the scene depth on the shared-prepass route, else the FX prepass).
        let depth = if depth_fx { Some(fx_depth) } else { None };
        // The path tracer does its OWN progressive accumulation over stationary
        // frames; the post-composite TAA would blend that with reprojected history
        // and fight it, so skip TAA entirely while tracing (#234 review).
        let taa_on = tp.enabled && !pathtrace_active;
        if taa_on {
            self.temporal.ensure(device, full_size);
        }
        if fxp.enabled {
            self.fx.ensure(device, full_size);
        }
        // Scene Kaleidoscope (#361 Tier 1): fold the fully-lit HDR scene (any
        // generator + surface) through N-fold kaleidoscopic symmetry, in place,
        // BEFORE the bloom/tonemap composite — so highlights + the EDR headroom
        // stay physical and the fold rides the existing bloom/beat stack. Snapshots
        // the HDR buffer to a scratch, samples the fold back into it.
        if kaleido.enabled {
            if let (Some(hdr_tex), Some(hdr_view)) =
                (self.post.hdr_texture(), self.post.hdr_view())
            {
                self.kaleido
                    .render(device, queue, &mut encoder, hdr_tex, hdr_view, size, &kaleido);
            }
        }
        // The composite writes into the first active pass's source texture, else view.
        let composite_target = if taa_on {
            self.temporal.src_view()
        } else if fxp.enabled {
            self.fx.src_view()
        } else {
            view
        };
        self.post
            .run(queue, &mut encoder, composite_target, post_params, ssr_active, ssgi_active);
        if taa_on {
            // Temporal resolves first; it writes to the FX source if FX follows, else view.
            let target = if fxp.enabled { self.fx.src_view() } else { view };
            self.temporal.apply(device, queue, &mut encoder, target, depth, full_size, &tp);
        }
        if fxp.enabled {
            self.fx.apply(device, queue, &mut encoder, view, depth, full_size, &fxp);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
}

/// Single-sample, sampleable depth target for the SSAO prepass.
fn make_prepass_depth(device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ssao-prepass-depth"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Depth-only pipeline reusing the cube vertex shader (`vs_depth`) with no fragment
/// stage. Used twice: the single-sample SSAO prepass (`sample_count = 1`,
/// `cull = None`) and the opaque early-Z prepass that fills the MSAA scene depth
/// (`sample_count = N`, `cull = Back` to match the opaque scene pipeline so the
/// `Equal` test lines up on the front faces).
///
/// Layout: group(0) uniforms **and** group(5) the material texture set. The
/// material set is not optional — `vs_depth` calls the same `mat_displace_world`
/// helper `vs_main` does (#472 Tier 5 height→vertex displacement), so it samples
/// `mat_height_tex`/`mat_samp` and the prepass position can only stay
/// bit-for-bit identical (`@invariant` + the scene pass's `depth_compare: Equal`)
/// if it reads the SAME height map. Groups 1–4 (IBL / RD scene / GI / shadow) are
/// fragment-only, so they stay holes rather than dummy layouts — every prepass
/// draw site must therefore bind 0 and 5, and only those.
fn make_depth_prepass_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    bind_layout: &wgpu::BindGroupLayout,
    mat_layout: &wgpu::BindGroupLayout,
    sample_count: u32,
    cull: Option<wgpu::Face>,
    material_maps: bool,
) -> wgpu::RenderPipeline {
    let spec = cube_specialisation(material_maps);
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("depth-prepass-layout"),
        bind_group_layouts: &[Some(bind_layout), None, None, None, None, Some(mat_layout)],
        immediate_size: 0,
    });
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3],
    };
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Mat4>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4],
    };
    let tint_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vec4>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![7 => Float32x4],
    };
    // organon#217 T1 — the emission layout (loc 8). `vs_depth` never reads it, but a
    // pipeline's buffer list is what every draw against it must bind, and the scene
    // pass and the prepasses share draw code — so the prepass takes the same four
    // buffers and simply ignores the fourth.
    let emit_layout = emit_vertex_layout();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("depth-prepass"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: shader,
            // Slim position-only entry (#174 T2): these pipelines have no fragment
            // stage, so vs_main's normal/colour work was pure waste — up to 3
            // depth-only rasterizations of the whole field per frame. vs_depth
            // mirrors the position math and is @invariant, so the scene pass's
            // Equal test still matches.
            entry_point: Some("vs_depth"),
            buffers: &[Some(vertex_layout), Some(instance_layout), Some(tint_layout), Some(emit_layout)],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &spec,
                ..Default::default()
            },
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
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

/// Scale the swapchain size by `render_scale` (clamped 0.2..1.0) for the internal
/// render targets; the final composite upscales back to the native swapchain. Each
/// axis is kept ≥ 8 px and even (nicer for the half/quarter terrain sub-targets).
pub fn scaled_render_size(size: (u32, u32), render_scale: f32) -> (u32, u32) {
    let s = render_scale.clamp(0.2, 1.0);
    let scale = |d: u32| ((d as f32 * s).round() as u32).max(8) & !1u32;
    (scale(size.0), scale(size.1))
}

fn make_depth(device: &wgpu::Device, size: (u32, u32), sample_count: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        // At 1× the scene depth doubles as the screen-space-FX depth source when
        // the prepass is shared (#174 T2), so it must be sampleable.
        usage: if sample_count == 1 {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        },
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

#[allow(clippy::type_complexity)]
fn build_skybox(
    device: &wgpu::Device,
    sky_env_layout: &wgpu::BindGroupLayout,
    sample_count: u32,
) -> (
    wgpu::ShaderModule,
    wgpu::PipelineLayout,
    wgpu::RenderPipeline,
    wgpu::Buffer,
    wgpu::BindGroup,
) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("skybox"),
        source: wgpu::ShaderSource::Wgsl(include_str!("skybox.wgsl").into()),
    });
    let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky-uniforms"),
        size: std::mem::size_of::<SkyUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sky-uniform-layout"),
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
    let sky_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky-uniform-bind"),
        layout: &sky_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sky-pipeline-layout"),
        bind_group_layouts: &[Some(&sky_layout), Some(sky_env_layout)],
        immediate_size: 0,
    });
    // Default (glass/fallback single-pass) skybox: paint the far plane.
    let pipeline =
        make_sky_pipeline(device, &shader, &pl, sample_count, wgpu::CompareFunction::Always, true);
    (shader, pl, pipeline, ubuf, sky_bind)
}

/// Build the cube/tube scene pipeline at a given MSAA sample count. Targets the
/// linear HDR buffer (`post::HDR_FORMAT`), not the surface. `cull` / `depth_compare`
/// / `depth_write` vary by path: the glass/fallback build uses `(None, Less, true)`
/// (single-pass, both faces for refraction); the opaque build uses
/// `(Back, Equal, false)` so it back-face culls and shades only the front-most
/// fragments left by the depth prepass.
/// #618 Tier 3: the pipeline-overridable constants `cube.wgsl` declares.
///
/// `material_maps = false` compiles the material-texture block out of `fs_main`
/// entirely — five samples, the triplanar UV resolve and the derivative cotangent
/// frame, none of which do anything while no material folder is loaded, and all of
/// which hold registers that cap occupancy for every fragment regardless. The WGSL
/// default is `true`, so a call site that forgets this gets the correct-but-slower
/// shader rather than one that silently ignores its materials.
fn cube_specialisation(material_maps: bool) -> [(&'static str, f64); 1] {
    [("material_maps", if material_maps { 1.0 } else { 0.0 })]
}

/// organon#217 T1 — the fourth instance layout: per-instance emission at location 8,
/// one `Vec4` per instance, parallel to the tints. Built in one place so the scene
/// pipeline and the depth prepass cannot disagree about it.
fn emit_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vec4>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![8 => Float32x4],
    }
}

/// An emission buffer of `cap` instances. **Fresh = zero**: wgpu zero-initialises
/// every buffer it creates, so a buffer nothing has written reads `vec4(0)` per
/// instance and `cube.wgsl`'s emission term contributes exactly nothing. That is the
/// inert default of invariant #4, and it holds without a single upload.
/// The usage every per-instance buffer the hit shading reads is created with — the
/// instances, the tints, and (organon#217 T8) the emission. `VERTEX` for the cube draw,
/// `COPY_DST` for the per-frame upload, and `STORAGE` because the RT passes
/// (`rt_pathtrace` / `rt_reflect` / `rt_gi` / `rt_caustic`) bind the same buffers as
/// read-only storage and index them by the TLAS custom index. ⚠️ A buffer created
/// without `STORAGE` is refused by wgpu at bind-group creation — on a real GPU, which
/// no leg of the bar has (#232 review caught `make_emit_buf` doing exactly that). One
/// constant so every creation and every regrow path agrees; `rt_hit_buffer_tests`
/// walks every `BufferDescriptor` in this file and pins that the labelled buffers the
/// RT layouts bind all use it.
pub(crate) const RT_HIT_BUFFER_USAGE: wgpu::BufferUsages = wgpu::BufferUsages::VERTEX
    .union(wgpu::BufferUsages::COPY_DST)
    .union(wgpu::BufferUsages::STORAGE);

fn make_emit_buf(device: &wgpu::Device, label: &str, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (cap.max(1) * std::mem::size_of::<Vec4>()) as u64,
        // organon#217 T8: STORAGE as well — the RT passes bind this beside the tints.
        usage: RT_HIT_BUFFER_USAGE,
        mapped_at_creation: false,
    })
}

/// organon#217 T1 — what an emission upload must zero, and the new high-water mark.
///
/// `high` is one past the highest instance whose emission may be non-zero; `lit` is
/// how many instances this frame uploads non-zero emission for (`0` = none). The
/// upload itself rewrites `[0, lit)`; this returns the range **beyond** it that is
/// still dirty — `[lit, high)` — and the mark after the frame, which is `lit`, because
/// once that range is zeroed nothing above `lit` can be non-zero.
///
/// Pure, so the property is pinned without a GPU: after any sequence of frames the
/// set of possibly-non-zero instances is exactly `[0, last lit)`. The first version
/// tracked the previous frame's length instead of the mark, and a 100 → 50 → 80
/// sequence read frame one's phosphor on frame three.
fn emit_upload_plan(high: usize, lit: usize) -> (std::ops::Range<usize>, usize) {
    (lit..high.max(lit), lit)
}

#[cfg(test)]
mod rt_hit_buffer_tests {
    //! organon#217 T8 (#232 review) — the buffers the RT passes bind as read-only
    //! storage must be CREATED with `STORAGE`, or wgpu refuses the bind group at
    //! creation on a real GPU, which no leg of the bar has. The instance/tint buffers
    //! had it and the emit buffer did not; this closes the class rather than the
    //! instance by walking every `BufferDescriptor` in this file and checking the
    //! labelled buffers the RT layouts index (`insts` / `tints` / `emits` in
    //! `rt_pathtrace.wgsl`, `rt_reflect.wgsl`, `rt_gi.wgsl`, `rt_caustic.wgsl`).
    use super::RT_HIT_BUFFER_USAGE;

    const SRC: &str = include_str!("render.rs");
    /// The labels of the buffers handed to an RT storage binding, as `create_buffer`
    /// spells them. `make_emit_buf` labels through its parameter (`emits` / the
    /// `zero-emits` twin), so its descriptor is recognised by the function it sits in
    /// rather than by a literal — other factories (`mk_vbuf`, `mk_ibuf`) also label
    /// through a parameter and are not RT storage.
    const RT_STORAGE_LABELS: [&str; 3] = ["instances", "tints", "emits"];

    #[test]
    fn the_hit_buffer_usage_carries_storage_beside_vertex_and_copy_dst() {
        for (flag, why) in [
            (wgpu::BufferUsages::STORAGE, "the RT passes bind these as read-only storage"),
            (wgpu::BufferUsages::VERTEX, "the cube draw reads them as vertex slots 1-3"),
            (wgpu::BufferUsages::COPY_DST, "they are uploaded every frame with write_buffer"),
        ] {
            assert!(
                RT_HIT_BUFFER_USAGE.contains(flag),
                "RT_HIT_BUFFER_USAGE lacks {flag:?}: {why}"
            );
        }
    }

    #[test]
    fn every_buffer_the_rt_passes_bind_as_storage_is_created_with_the_hit_usage() {
        let src = SRC.replace('\r', "");
        let mut seen: std::collections::BTreeMap<&str, usize> = Default::default();
        let mut at = 0usize;
        while let Some(i) = src[at..].find("wgpu::BufferDescriptor {") {
            let start = at + i;
            let end = start + src[start..].find("})").expect("unterminated BufferDescriptor");
            let block = &src[start..end];
            at = end;
            let Some(l) = block.find("label: Some(") else { continue };
            let raw = &block[l + "label: Some(".len()..];
            let raw = &raw[..raw.find(')').expect("label without a closing paren")];
            let in_emit_factory = src[..start]
                .rfind("fn ")
                .is_some_and(|f| src[f..start].starts_with("fn make_emit_buf("));
            let name = if raw == "label" {
                if in_emit_factory { "emits" } else { continue }
            } else {
                raw.trim_matches('"')
            };
            if !RT_STORAGE_LABELS.contains(&name) {
                continue;
            }
            *seen.entry(name).or_default() += 1;
            let line = src[..start].matches('\n').count() + 1;
            assert!(
                block.contains("usage: RT_HIT_BUFFER_USAGE,"),
                "render.rs:{line}: the `{name}` buffer is bound as read-only storage by the RT passes but this create_buffer does not use RT_HIT_BUFFER_USAGE — without STORAGE wgpu refuses the bind group at creation, on a GPU CI does not have"
            );
        }
        // Each label is created in `new` and again on the regrow path (the emit buffer
        // through `make_emit_buf`, its one factory) — a label that vanishes from this
        // walk means the descriptor moved somewhere the walk cannot see.
        assert_eq!(seen.get("instances"), Some(&2), "instances: {seen:?}");
        assert_eq!(seen.get("tints"), Some(&2), "tints: {seen:?}");
        assert_eq!(seen.get("emits"), Some(&1), "emits (make_emit_buf): {seen:?}");
    }
}

#[cfg(test)]
mod emit_plan_tests {
    use super::emit_upload_plan;

    /// A model of the buffer: which instances hold non-zero emission. Apply the plan
    /// exactly as `render` does — write `[0, lit)`, zero the returned range — and the
    /// lit set must be `[0, lit)` after EVERY frame, whatever came before.
    fn run(seq: &[usize]) -> Vec<Vec<bool>> {
        let mut buf = vec![false; 256];
        let mut high = 0;
        let mut out = Vec::new();
        for &lit in seq {
            let (zero, next) = emit_upload_plan(high, lit);
            for b in &mut buf[..lit] {
                *b = true;
            }
            for b in &mut buf[zero] {
                *b = false;
            }
            high = next;
            out.push(buf.clone());
        }
        out
    }

    fn lit_set(b: &[bool]) -> Vec<usize> {
        b.iter().enumerate().filter(|(_, &v)| v).map(|(i, _)| i).collect()
    }

    /// The review's sequence: 100 lit, then 50, then a generator frame of 80 with no
    /// emission. Frame three must read zero everywhere — `[50, 100)` included.
    #[test]
    fn a_shrink_never_leaves_stale_emission_above_the_new_length() {
        let frames = run(&[100, 50, 0]);
        assert_eq!(lit_set(&frames[0]), (0..100).collect::<Vec<_>>());
        assert_eq!(lit_set(&frames[1]), (0..50).collect::<Vec<_>>(), "50..100 must be zeroed on the shrink");
        assert!(lit_set(&frames[2]).is_empty(), "an 80-instance generator draw would read {:?}", lit_set(&frames[2]));
    }

    /// Growth, shrink, growth, silence, growth again — the mark follows the last
    /// upload exactly and the dirty range never escapes it.
    #[test]
    fn the_lit_set_is_exactly_the_last_upload_after_any_sequence() {
        let seq = [10, 200, 30, 30, 120, 0, 0, 7, 0];
        for (i, f) in run(&seq).iter().enumerate() {
            assert_eq!(lit_set(f), (0..seq[i]).collect::<Vec<_>>(), "after frame {i}");
        }
    }

    /// The common case — no ring, ever — plans no write at all.
    #[test]
    fn no_emission_and_no_history_writes_nothing() {
        let (zero, high) = emit_upload_plan(0, 0);
        assert!(zero.is_empty());
        assert_eq!(high, 0);
        let (zero, _) = emit_upload_plan(40, 60);
        assert!(zero.is_empty(), "growing rewrites everything the mark covered");
    }
}

fn make_cube_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    sample_count: u32,
    cull: Option<wgpu::Face>,
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
    material_maps: bool,
) -> wgpu::RenderPipeline {
    let spec = cube_specialisation(material_maps);
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3],
    };
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Mat4>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4],
    };
    // Per-instance colour tint (location 7), parallel to the model matrices.
    let tint_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vec4>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![7 => Float32x4],
    };
    // organon#217 T1 — per-instance emission (location 8), parallel to the tints.
    let emit_layout = emit_vertex_layout();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cube-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(vertex_layout), Some(instance_layout), Some(tint_layout), Some(emit_layout)],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &spec,
                ..Default::default()
            },
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: post::HDR_FORMAT,
                // PREMULTIPLIED (One / OneMinusSrcAlpha): fs_main multiplies the
                // attenuable terms by alpha itself, so Glass can composite its
                // Fresnel reflection / specular / emissive at FULL strength while
                // only the transmitted body fades with opacity. Standard/Chrome
                // premultiply their whole output (or write alpha 1), reproducing
                // the old SrcAlpha blend exactly. The alpha component of the two
                // blend states is identical, so the composite's coverage channel
                // is unchanged.
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &spec,
                ..Default::default()
            },
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
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

/// Build the skybox pipeline at a given MSAA sample count. `depth_compare` /
/// `depth_write` vary by path: the glass/fallback single pass paints the far
/// plane with `(Always, true)`; the opaque path (which already has prepass depth
/// in the buffer) draws the backdrop only where no geometry wrote depth with
/// `(Equal, false)` — the sky's clip z is 1.0, matching the cleared far depth.
fn make_sky_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    sample_count: u32,
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sky-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_sky"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_sky"),
            targets: &[Some(wgpu::ColorTargetState {
                format: post::HDR_FORMAT,
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
            format: DEPTH_FORMAT,
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

    // For back-face culling to be safe (front face = CCW = outward), every
    // triangle's geometric normal (edge0 × edge1) must point the same way as the
    // mesh's stored outward normals. This guards the cube/cylinder winding offline
    // — a GPU isn't available in CI, but a wrong winding would invert culling.
    fn assert_wound_outward(verts: &[Vertex], idx: &[u16], label: &str) {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        for tri in idx.chunks(3) {
            let (v0, v1, v2) = (
                verts[tri[0] as usize],
                verts[tri[1] as usize],
                verts[tri[2] as usize],
            );
            let geo = cross(sub(v1.pos, v0.pos), sub(v2.pos, v0.pos));
            // Average the three stored normals as the "outward" reference.
            let nrm = [
                (v0.normal[0] + v1.normal[0] + v2.normal[0]) / 3.0,
                (v0.normal[1] + v1.normal[1] + v2.normal[1]) / 3.0,
                (v0.normal[2] + v1.normal[2] + v2.normal[2]) / 3.0,
            ];
            assert!(
                dot(geo, nrm) > 0.0,
                "{label}: triangle {tri:?} winds inward (geo·normal = {})",
                dot(geo, nrm)
            );
        }
    }

    /// #618 Tier 3 — the specialisation constant is addressed by NAME, as a string,
    /// across a language boundary. Rust passes `"material_maps"`; `cube.wgsl` declares
    /// it. Nothing in the type system connects the two: rename either side and this
    /// compiles, runs, and fails only at `create_render_pipeline` on a real device —
    /// which is exactly the class of error no GPU-less session can see. So pin it here,
    /// deriving the expected name from the function that supplies it rather than
    /// repeating the literal.
    #[test]
    fn the_specialisation_constant_exists_in_the_shader() {
        let (name, _) = cube_specialisation(true)[0];
        let src = include_str!("cube.wgsl");
        assert!(
            src.contains(&format!("override {name}:")),
            "render.rs sets the pipeline constant `{name}`, but cube.wgsl declares no \
             such override. wgpu would reject this only at pipeline creation, on a GPU."
        );
    }

    /// The WGSL default must be `true`: the unspecialised module is then exactly the
    /// pre-Tier-3 shader, so `tests/wgsl.rs` validates the file as written and any
    /// pipeline that forgets the constant renders correctly-but-slower rather than
    /// silently losing its material maps. Failing toward correctness is the whole
    /// reason the default is not `false`.
    #[test]
    fn the_specialisation_defaults_to_the_correct_slow_path() {
        let src = include_str!("cube.wgsl");
        assert!(
            src.contains("override material_maps: bool = true;"),
            "the override's default must stay `true` — see the note in cube.wgsl"
        );
        // And the flag maps to the WGSL bool encoding wgpu expects.
        assert_eq!(cube_specialisation(true)[0].1, 1.0);
        assert_eq!(cube_specialisation(false)[0].1, 0.0);
    }

    #[test]
    fn cube_mesh_is_wound_outward() {
        let (verts, idx) = cube_mesh();
        assert_wound_outward(&verts, &idx, "cube");
    }

    #[test]
    fn cyl_mesh_is_wound_outward() {
        let (verts, idx) = cyl_mesh();
        assert_wound_outward(&verts, &idx, "cylinder");
    }

    #[test]
    fn creature_meshes_are_valid() {
        // Each Boids creature mesh: non-empty, whole triangles, in-range indices,
        // finite positions/normals. (Fins are intentionally double-sided, so the
        // strict outward-winding check doesn't apply — culling can't drop them.)
        for k in 0..CREATURE_KINDS {
            let (verts, idx) = creature_mesh(k);
            assert!(!verts.is_empty() && !idx.is_empty(), "kind {k} is empty");
            assert!(idx.len() % 3 == 0, "kind {k}: indices not a triangle multiple");
            for &i in &idx {
                assert!((i as usize) < verts.len(), "kind {k}: index {i} out of range");
            }
            for v in &verts {
                assert!(v.pos.iter().chain(&v.normal).all(|x| x.is_finite()), "kind {k}: non-finite");
            }
        }
    }

    #[test]
    fn soma_mesh_is_wound_outward_and_unit_radius() {
        // #260 Tier 1: the soma/bouton icosphere — radial normals, radius 0.5,
        // wound outward so back-face culling keeps the front faces.
        let (verts, idx) = soma_mesh();
        assert!(!verts.is_empty() && idx.len() % 3 == 0, "non-empty, triangle multiple");
        for v in &verts {
            let r = (v.pos[0].powi(2) + v.pos[1].powi(2) + v.pos[2].powi(2)).sqrt();
            assert!((r - 0.5).abs() < 1e-3, "soma vertex off the unit sphere: r={r}");
        }
        assert_wound_outward(&verts, &idx, "soma");
    }

    #[test]
    fn capsule_mesh_is_closed_and_bounded() {
        // #260 Tier 1: the capped capsule spans exactly z ∈ [-0.5, 0.5] (so the
        // per-segment instance places it centre-to-centre) and is CLOSED — both ends
        // taper to the axis (no open pipe), unlike the open `cyl_mesh`.
        let (verts, idx) = capsule_mesh();
        assert!(!verts.is_empty() && idx.len() % 3 == 0, "non-empty, triangle multiple");
        for &i in &idx {
            assert!((i as usize) < verts.len(), "capsule index out of range");
        }
        let (mut zmin, mut zmax) = (f32::MAX, f32::MIN);
        let (mut rmin_lo, mut rmin_hi) = (f32::MAX, f32::MAX); // min radius near each end
        for v in &verts {
            assert!(v.pos.iter().chain(&v.normal).all(|x| x.is_finite()), "non-finite");
            zmin = zmin.min(v.pos[2]);
            zmax = zmax.max(v.pos[2]);
            let r = (v.pos[0].powi(2) + v.pos[1].powi(2)).sqrt();
            if v.pos[2] < -0.49 {
                rmin_lo = rmin_lo.min(r);
            }
            if v.pos[2] > 0.49 {
                rmin_hi = rmin_hi.min(r);
            }
        }
        assert!((zmin + 0.5).abs() < 1e-3 && (zmax - 0.5).abs() < 1e-3, "capsule z ∉ [-0.5,0.5]");
        assert!(rmin_lo < 0.05 && rmin_hi < 0.05, "capsule ends are not capped (open pipe)");
    }

    // ── The material channel tables have to agree with each other ───────────────────────
    //
    // `Renderer::baked_material_views` promises "in the bind order of
    // `MaterialTextures::CHANNELS`", and the *number* of those views comes from `MAT_SLOTS`,
    // which is a separate constant. `MaterialBaker::channel_slot` is a third table mapping a
    // shader channel ordinal to (slot, present bit). Nothing checked that the three agree, and
    // a GPU is not available in CI — so these are what can be proven offline, and they are the
    // part a downstream caller actually depends on.

    #[test]
    fn the_baker_allocates_exactly_one_target_per_declared_channel() {
        assert_eq!(
            MaterialTextures::CHANNELS.len(),
            MAT_SLOTS,
            "CHANNELS and MAT_SLOTS disagree: the baker would allocate {MAT_SLOTS} targets for \
             {} declared channels, and `baked_material_views` would hand a caller the wrong \
             number in an order it documents as CHANNELS'",
            MaterialTextures::CHANNELS.len()
        );
    }

    #[test]
    fn every_baked_channel_lands_in_a_slot_that_exists() {
        // Channel ordinals are the shader's, and only some of them are baked.
        for channel in 0..8u32 {
            if let Some((slot, bit)) = MaterialBaker::channel_slot(channel) {
                assert!(
                    slot < MAT_SLOTS,
                    "channel {channel} claims slot {slot}, past the {MAT_SLOTS} allocated"
                );
                assert_eq!(
                    bit,
                    MaterialTextures::CHANNELS[slot].2,
                    "channel {channel} sets present bit {bit} but slot {slot} ({}) declares {}",
                    MaterialTextures::CHANNELS[slot].0,
                    MaterialTextures::CHANNELS[slot].2
                );
            }
        }
    }

    #[test]
    fn no_two_baked_channels_share_a_slot() {
        // Two channels writing one target is a silent overwrite: the later bake wins and the
        // earlier channel is advertised as present while holding someone else's pixels.
        let mut seen = std::collections::BTreeMap::new();
        for channel in 0..8u32 {
            if let Some((slot, _)) = MaterialBaker::channel_slot(channel) {
                if let Some(other) = seen.insert(slot, channel) {
                    panic!("channels {other} and {channel} both write slot {slot}");
                }
            }
        }
    }

    #[test]
    fn the_present_bits_are_distinct_powers_of_two() {
        // `present_mask` is a bitfield read by the cube shader; a repeated or non-power-of-two
        // bit makes "which maps are loaded" unanswerable.
        let mut union = 0u32;
        for (name, _, bit) in MaterialTextures::CHANNELS {
            assert!(
                bit.is_power_of_two(),
                "{name}'s present bit {bit} is not a single bit"
            );
            assert_eq!(
                union & bit,
                0,
                "{name}'s present bit {bit} is already taken"
            );
            union |= bit;
        }
    }
}

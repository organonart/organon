//! **Four materials and two lighting rigs** for the substrate plane, expressed as nothing
//! but writes into an existing `Shared` snapshot.
//!
//! Console Spike Tier 2 Leaf A; `doc/console_spike_as_built_brief.md` **R5**. This is the
//! other half of Tier 1's [`crate::substrate_scene`] — same shape, same discipline, one tier
//! further on. Tier 1 built *one* look and said in as many words what it could not reach:
//!
//! > 3. *normal variation* — blocked behind the `#472` material-map gate
//! >    (`render.rs:3640-3654` forces `u.mtl[0] = 0` on every non-`Instanced` path, the
//! >    Membrane path included). **That gate lift is Tier 2's**, not ours.
//! >    — `substrate_scene.rs:34-36`
//!
//! That gate is lifted in this tier's other deliverable (`organon-render/src/render.rs`, the
//! `material_draw` predicate). This file is what the lift is *for*: the `#472` procedural
//! material stack, aimed at a flat sheet.
//!
//! ```text
//! OrganicMathParams::default().to_shared()
//!         → substrate_scene::apply_substrate_look(&mut s)      (Tier 1: the plane + a rig)
//!         → apply_material(&mut s, "graphite")                 (this file: the surface)
//!         → apply_rig(&mut s, "daylight")                      (this file: the light)
//! ```
//!
//! # Composition — what this file assumes, and what it owns
//!
//! **[`apply_substrate_look`](crate::substrate_scene::apply_substrate_look) ran first.**
//! Everything here is a *delta* on Tier 1's snapshot: the geometry, the stillness switches,
//! the palette and the background are Tier 1's and are never touched. What is written here
//! is exactly the surface response (the map stack + the PBR scalars + the per-material HSV)
//! and, separately, the light levels.
//!
//! **The two functions are independent and commute.** [`apply_material`] and [`apply_rig`]
//! write **disjoint sets of `Shared` lanes** — a test asserts it by name — so either order
//! gives the same bytes, and a material change does not disturb the light or vice versa.
//! That disjointness is why the manifests below are at **lane** granularity where Tier 1's
//! were at block granularity: `lighting` and `pbr` are each split between the two functions,
//! and a whole-block restore could not tell them apart. (Tier 1's note that a whole-array
//! restore "can never *hide* a stray write, only decline to pinpoint one within a declared
//! block" is exactly the weakness this needs closed: here a stray write *within* `lighting`
//! is the failure mode.)
//!
//! # How a material reaches the plane at all
//!
//! Three mechanisms, none of them new, and one gate that had to move:
//!
//! 1. **The bake.** `world.rs:6671-6706` runs the `#472` compute baker whenever
//!    `material_layer[16]` (procedural on) is set — **independently of the render path**.
//!    Nothing about the substrate had to change for the bake to happen; it already would
//!    have. Up to two layers (`material_layer`, `material_layer2`) each composite into
//!    **their own routed channel** — `material_bake.wgsl:379-390` skips a layer whose
//!    `[1]` does not match the channel being dispatched, so the two layers are *independent*
//!    channels, not a base/overlay pair, unless they are pointed at the same one.
//! 2. **The sample.** `cube.wgsl:1396-1427` samples the six channel maps into
//!    `mat_uv(world_pos, …)` and blends each by `(mat_on × present-bit)`. `mat_on` is
//!    `u.mtl.x`, which `world.rs:10850-10852` sets from
//!    `material[0] > 0.5 || material_layer[16] > 0.5`.
//! 3. **The gate.** `render.rs` zeroed `u.mtl[0]` for every draw that is not the plain
//!    instanced cube — the Membrane sheet included. That is the uniform-value gate R5
//!    identified, and this tier widens it (see [`GATE`]).
//!
//! **`mat_uv`'s world-planar XZ projection is the plane's own plane** (`cube.wgsl:1313-1316`,
//! `uv = world_pos.xz · scale`), which is why R5 called the flat membrane "precisely
//! `mat_uv`'s world-planar-XZ projection". Every material here names that projection rather
//! than relying on Triplanar's dominant-axis pick happening to agree.
//!
//! # The sampling arithmetic — feature sizes, in pixels, on the real pane
//!
//! The baked channel textures are **512² with `mip_level_count: 1`** — no mipmaps
//! (`render.rs`'s `MaterialBaker::make_target`). Minification therefore aliases with nothing
//! to catch it, so the sample rate is a correctness constraint and not a taste one.
//!
//! 📌 **These figures are calibrated against a capture, not assumed.** The first pass sized
//! everything for a 1080-px pane; the Tier 2 beat check ran at **2000 × 1314**, and
//! `SubstrateRig::frame_plane` is *cover* framing, so on a landscape pane the **horizontal**
//! axis governs (`substrate_camera::governing_axis`) and the plane's 127 world units
//! (`substrate_scene::SUBSTRATE_GRID_X` − 1) fill the **width**:
//!
//! | quantity | at 2000 px wide |
//! |---|---|
//! | px per world unit | 2000 / 127 ≈ **15.75** |
//! | one UV tile at scale `s` | `1/s` world units = `15.75/s` px |
//! | texels per pixel | `512·s / 15.75` = **32.5·s** |
//!
//! So at `s = 0.02` the rate is **0.65 texels/px** — magnification, the safest case, and
//! considerably better than the 1.20 the 1080-px estimate predicted. The prediction that
//! mattered was checked the other way too: this model said graphite's `p48` stripes would
//! land at 32.8 px and the capture read them at ≈ 40 px, so the two formulas below are
//! trustworthy enough to revise blind.
//!
//! ```text
//!     fractal feature   =  15.75 / (s · p)        px      (finest octave: ÷ lacunarity^(oct−1))
//!     Stripes period    =  31.5  / (s · p)        px      (sin(p.x·π): period 2 in noise space,
//!                                                          material_bake.wgsl:236-238)
//! ```
//!
//! **`uv_scale` is per-material, and only because one material needed it.** `mp_scale` caps
//! at 64 (`params.rs:8432`), which at `s = 0.02` bottoms out at a 24.6 px stripe — fine for
//! metal, far too coarse for graphite's lamination. Raising `s` is the only remaining lever,
//! so [`SubstrateMaterial::uv_scale`] exists; three of the four still take
//! [`MATERIAL_UV_SCALE`]. Raising `s` costs sampling headroom (32.5·s), which is affordable
//! only for **bandlimited** content — a sine stripe is, a Sobel-derived normal is not, and
//! the two materials that derive normals therefore stay at the floor.
//!
//! 🚨 **The limit this leaves, recorded rather than hidden:** the ratio is inversely
//! proportional to the pane's pixel width. A backdrop pane 1000 px wide doubles it. There is
//! no `Shared` field that fixes it once `s` is chosen; the fixes are mipmaps on the baked
//! targets or a coarser `mp_scale`, and both are outside this leaf.
//!
//! # Invariants honoured
//!
//! * **No `Shared` layout change** (the repo's second invariant) — existing fields only.
//! * **Every value written lies inside its host parameter's declared range in `params.rs`**,
//!   quoted beside each constant. Same reason Tier 1 gave: R6 found three hand-maintained
//!   range tables already disagreeing with `params.rs`, and a material builder that wrote
//!   out-of-range bytes would be a fourth way to lie.
//! * **Every field is cited to the line that consumes it.** A field nothing reads is not
//!   written — `material[3..8]` are consumed into `Uniforms.mtl2.xyz`
//!   (`world.rs:10859-10863`) and `cube.wgsl` reads **only** `mtl2.w`, so those four slots
//!   express nothing and are left alone. `param_table.rs:1210-1214` agrees: "reserved".
//! * `seq` and `layout_version` are never touched — they belong to the publisher.

use crate::ipc::Shared;
use crate::substrate_scene::FieldRestore;

/// The gate this tier lifted, named so the two deliverables can cite each other.
///
/// `render.rs` used one predicate — `cube_draw` — for two unrelated jobs: scoping the cube
/// **bevel** morph, and scoping the `#472` **material set**. Tier 2 splits it, because the
/// membrane wants the second and must not have the first (a bevel morphs the shared *cube*
/// mesh; the membrane is not that mesh). The material half is now `material_draw`, which
/// additionally admits the Membrane sheet:
///
/// ```text
/// if !cube_draw     { u.shape[0] = 0.0; }   // bevel — unchanged
/// if !material_draw { u.mtl[0]   = 0.0; }   // maps  — widened for the membrane sheet
/// ```
///
/// It is a **uniform-value** gate, not a pipeline gate: group(5) was already bound at both
/// membrane draw sites (`render.rs`'s membrane branch and the depth prepass), so nothing
/// about the binding had to change. With no material configured, `material_layer[16]` is 0,
/// `u.mtl[0]` is 0 anyway, and the frame is byte-identical — the repo's 4th invariant, and
/// the reason this could land inert.
pub const GATE: () = ();

// ===========================================================================
// Shared across every material
// ===========================================================================

/// `material[1]` = `MatProjection::WorldPlanar` (`params.rs:1450-1457`; Triplanar = 0,
/// WorldPlanar = 1, ObjectPlanar = 2). Reaches the shader as `Uniforms.mtl.y`
/// (`world.rs:10853`) and selects `uv = world_pos.xz · scale` (`cube.wgsl:1313-1316`).
///
/// Named rather than inherited. Triplanar's dominant-axis pick would land on the same plane
/// here (the sheet's normal is ±Y, so `an.y` wins and it returns `world_pos.xz`), but that
/// agreement is a property of the geometry, and the geometry is Tier 1's to change.
pub const MATERIAL_PROJECTION: f32 = 1.0;

/// `material[2]` — world units → UV tiles, reaching the shader as `Uniforms.mtl.z`
/// (`world.rs:10854`). The **default** three of the four materials take, and the parameter's
/// declared floor (range 0.02..16.0, `params.rs:8425`) — i.e. the least-aliasing value the
/// range admits, 0.65 texels/px on a 2000-px pane. See the module doc for why `graphite`
/// departs from it and what that costs.
pub const MATERIAL_UV_SCALE: f32 = 0.02;

/// `material_layer[17]` — the bake resolution in **pixels**, not the `BakeRes` ordinal.
/// `param_table.rs:1239-1240` packs `BakeRes::px()` onto the wire and
/// `render.rs`'s `bake_material` clamps it to 64..2048; writing the enum ordinal (1) would
/// silently bake a 64² texture. 512 = `BakeRes::R512` (`params.rs:1612-1618`), the default.
pub const MATERIAL_BAKE_PX: f32 = 512.0;

/// The four material names, in the order the CLI should list them.
pub const MATERIAL_NAMES: [&str; 4] = ["graphite", "paper", "slate", "metal"];

/// The two lighting-rig names.
pub const RIG_NAMES: [&str; 2] = ["studio", "daylight"];

// ===========================================================================
// One procedural layer
// ===========================================================================

/// One `#472` procedural layer, by field name.
///
/// The wire form is a bare `[f32; 18]` whose slot order is declared once in
/// `param_table.rs:1220-1241` (base layer) and `:1252-1272` (overlay). Naming the fields
/// here means the order is written out exactly once — in [`Layer::pack`] — instead of once
/// per material, which is the same reason `param_table.rs` is a macro table rather than
/// eight hand-indexed arrays.
///
/// Slots `[16]` and `[17]` differ between the two blocks and are therefore **not** fields
/// here: [`Layer::pack_base`] supplies them for `material_layer` (procedural-on + bake px)
/// and [`Layer::pack_overlay`] for `material_layer2` (per-layer enable + blend mode).
#[derive(Clone, Copy, PartialEq)]
pub struct Layer {
    /// `[0]` — `MatNoise` wire value (`params.rs:1494-1511`, 16 kinds; `Fbm` = 3,
    /// `Stripes` = 12). Selected in `material_bake.wgsl`'s `noise_field`.
    pub noise: f32,
    /// `[1]` — `MatChannel` wire value (`params.rs:1554-1561`: 0 Albedo / 1 Roughness /
    /// 2 Metallic / 3 Height / 4 AO / 5 Emissive). **This is what makes two layers two
    /// independent channels**: `material_bake.wgsl:382` skips a layer whose `[1]` is not the
    /// channel being dispatched. Emissive has no bound slot and would bake nowhere
    /// (`render.rs`'s `channel_slot` returns `None`), so nothing here routes to it.
    pub channel: f32,
    /// `[2]` — noise period across the bake tile. **Rounded to an integer ≥ 1** by
    /// `material_bake.wgsl:327`, which is what keeps a layer tileable; every value below is
    /// already an integer so the rounding is visible rather than surprising.
    /// Range 0.25..64 (`params.rs:8432`).
    pub scale: f32,
    /// `[3]` — field rotation in radians about the tile centre
    /// (`material_bake.wgsl:329-334`). Range 0..τ (`params.rs:8433`).
    pub rotation: f32,
    /// `[4..6]` — field offset, panning the pattern. Range −8..8 (`params.rs:8434-8435`).
    pub offset: [f32; 2],
    /// `[6]` — fractal octaves; ignored by the single-scale noises (`Stripes`, `Checker`,
    /// `Worley`, …). Range 1..8 (`params.rs:8436`).
    pub octaves: f32,
    /// `[7]` — frequency multiplier per octave. Range 1.2..4 (`params.rs:8437`).
    pub lacunarity: f32,
    /// `[8]` — amplitude multiplier per octave. Range 0.1..0.9 (`params.rs:8438`).
    pub gain: f32,
    /// `[9]` — domain warp: the sample position is perturbed by a second Perlin field
    /// (`material_bake.wgsl:261-267`), applied to **every** kind including the single-scale
    /// ones. That is what lets a `Stripes` layer wander instead of reading as print.
    /// Range 0..2 (`params.rs:8439`).
    pub warp: f32,
    /// `[10]` — output contrast about 0.5: `v = (v − 0.5)·contrast + 0.5`
    /// (`material_bake.wgsl:318`). Below 1 it **compresses** the output range, which is the
    /// only handle a scalar channel has on its output *span* (the gradient is albedo-only).
    /// Range 0.1..4 (`params.rs:8440`).
    pub contrast: f32,
    /// `[11]` — output gamma, applied after contrast (`material_bake.wgsl:319`). On a
    /// contrast-compressed band this is the handle on its output *centre*: a band of
    /// \[0.4, 0.6\] under gamma 1.3 becomes \[0.304, 0.515\]. Contrast + gamma together are
    /// how the scalar channels below hit a named mean. Range 0.2..4 (`params.rs:8441`).
    pub gamma: f32,
    /// `[12..14]` — **input** remap: noise ≤ lo → 0, ≥ hi → 1 (`material_bake.wgsl:316`).
    /// An expansion, not an output window. Range 0..1 (`params.rs:8442-8443`).
    pub remap: [f32; 2],
    /// `[14]` — invert the shaped field (`material_bake.wgsl:320`).
    pub invert: f32,
    /// `[15]` — noise seed, shifting the hash lattice. Distinct per layer so two layers of
    /// the same kind in one material are not the same field. Range 0..64
    /// (`params.rs:8445`).
    pub seed: f32,
}

impl Layer {
    /// The engine's own layer defaults (`params.rs:8430-8446`), as the base every material
    /// below overrides. Not a claim of byte-equality with `Shared::default()` — the host
    /// defaults round-trip through nih-plug on their way to the wire — just a legible
    /// starting point so each material states only what it changes.
    pub const DEFAULT: Layer = Layer {
        noise: 3.0, // Fbm
        channel: 0.0,
        scale: 4.0,
        rotation: 0.0,
        offset: [0.0, 0.0],
        octaves: 5.0,
        lacunarity: 2.0,
        gain: 0.5,
        warp: 0.0,
        contrast: 1.0,
        gamma: 1.0,
        remap: [0.0, 1.0],
        invert: 0.0,
        seed: 0.0,
    };

    /// The 16 shared slots, in `param_table.rs`'s order. `[16]`/`[17]` are left to the two
    /// wrappers because the two blocks disagree about them.
    const fn pack(&self, tail16: f32, tail17: f32) -> [f32; 18] {
        [
            self.noise,
            self.channel,
            self.scale,
            self.rotation,
            self.offset[0],
            self.offset[1],
            self.octaves,
            self.lacunarity,
            self.gain,
            self.warp,
            self.contrast,
            self.gamma,
            self.remap[0],
            self.remap[1],
            self.invert,
            self.seed,
            tail16,
            tail17,
        ]
    }

    /// → `material_layer[18]`. `[16]` is the **global** procedural switch (not a per-layer
    /// enable): `world.rs:6671`'s `proc_on` reads it to decide whether to bake at all, and
    /// `world.rs:10852` reads it to turn `Uniforms.mtl.x` on. `[17]` is the bake size in px.
    pub const fn pack_base(&self) -> [f32; 18] {
        self.pack(1.0, MATERIAL_BAKE_PX)
    }

    /// → `material_layer2[18]`. `[16]` is this layer's own enable; `[17]` is a `BlendMode`
    /// (`params.rs:1626-1635`).
    ///
    /// 📌 **The blend mode is inert for every material in this file**, and deliberately so:
    /// each of them routes its overlay to a channel the base layer does not use, so the
    /// overlay is the *first* layer in that channel's dispatch and `material_bake.wgsl:384-388`
    /// takes the `!hit` branch (`acc = col`) without ever calling `blend`. `Normal` (0) is
    /// written because it is also what `blend`'s `default` arm does, so the value agrees with
    /// the behaviour if a future material ever stacks two layers on one channel.
    pub const fn pack_overlay(&self, enabled: bool) -> [f32; 18] {
        self.pack(if enabled { 1.0 } else { 0.0 }, 0.0)
    }
}

/// `material_grad[8]` / `material_grad2[8]` = `[lo_r, lo_g, lo_b, _, hi_r, hi_g, hi_b, _]`
/// — the two **linear RGB** stops the baked noise maps between, and **only when the layer's
/// routed channel is Albedo**: `material_bake.wgsl:338-340` applies the gradient for channel
/// 0 and returns the bare scalar for every other channel. Range 0..1 per component
/// (`params.rs:8447-8452`).
const fn grad(lo: [f32; 3], hi: [f32; 3]) -> [f32; 8] {
    [lo[0], lo[1], lo[2], 0.0, hi[0], hi[1], hi[2], 0.0]
}

/// The stops written for a layer that does **not** route Albedo — the engine's own defaults
/// (`params.rs:8447-8452`), inert by the rule above.
///
/// They are written rather than left alone for the same reason the unused overlay is: a
/// material must not inherit the previous material's block. See [`apply_material`].
const GRAD_UNUSED: [f32; 8] = grad([0.04, 0.04, 0.05], [0.80, 0.76, 0.70]);

/// `material_derive[8]` = `[derive_normal, derive_ao, normal_source (0 height / 1 albedo
/// luminance), normal_strength, ao_strength, ao_radius, _, _]` (`param_table.rs:1283-1291`).
/// All flags 0 → no derived maps.
///
/// **This is the block that answers R5's item (2).** A flat plane under a uniform material
/// shades to one constant colour; a derived normal map puts real per-fragment normal
/// variation on it, which is precisely what the map gate was blocking. `cube.wgsl:1420-1422`
/// perturbs the geometric normal through the derivative cotangent frame — legal here because
/// the caller gates it on uniform control flow, and well-conditioned on a plane where
/// `dpdx/dpdy(world_pos)` are constant and non-degenerate.
///
/// 🚨 **`derive_ao` reads the HEIGHT slot, unconditionally.** `render.rs`'s derive pass wires
/// slot 5 → slot 4 for AO regardless of what was baked, while the derived *normal* honours
/// `[2]` and can read albedo instead. So AO is only honest for a material that actually bakes
/// a Height layer; asking for it otherwise derives cavities from whatever is sitting in the
/// height texture. Only `paper` bakes height, and only `paper` asks for AO.
const fn derive(normal: bool, ao: bool, from_albedo: bool, strength: f32, ao_strength: f32) -> [f32; 8] {
    [
        if normal { 1.0 } else { 0.0 },
        if ao { 1.0 } else { 0.0 },
        if from_albedo { 1.0 } else { 0.0 },
        strength,     // range 0..4  (params.rs:8485)
        ao_strength,  // range 0..1  (params.rs:8486)
        2.0,          // ao radius in texels; range 1..8 (params.rs:8487), this is the default
        0.0,
        0.0,
    ]
}

/// No derived maps.
const DERIVE_NONE: [f32; 8] = derive(false, false, false, 1.0, 1.0);

/// `material_live[8]` = `[anim_on, anim_speed, anim_mode, flow_x, flow_y, displace,
/// audio_drive (reserved), _]` (`param_table.rs:1295-1301`), written **still** by every
/// material. Two of the eight are load-bearing here and both are stillness items, exactly
/// parallel to Tier 1's `bio` clocks:
///
/// * `[0]` **anim_on = 0.** `world.rs:6676-6693` injects a wall-clock time term into the
///   baked layers and re-dispatches the whole bake at ~30 Hz while this is set. A substrate
///   that Tier 1 spent a file freezing must not have a material that flows.
/// * `[5]` **displace = 0.** It reaches `Uniforms.mtl.w` (`world.rs:10855`) and
///   `cube.wgsl:598-625` offsets each **vertex** along its normal by the sampled height.
///   That is a real temptation on this geometry — 16 384 vertices of flat sheet — and it is
///   wrong here: the membrane's normals are built CPU-side by `math::push_sheet` and are
///   **not** recomputed after displacement, so the relief would move without shading. The
///   derived normal map above is the honest way to get relief on this plane, and it costs no
///   geometry at all.
///
/// The other six are inert at `[0] = 0` and are written to their engine defaults
/// (`params.rs:8491-8496`) so a material cannot inherit an animation from its predecessor.
/// `[5]`'s range is 0..2 (`params.rs:8496`); `[1]`'s is 0..4; `[3]`/`[4]`'s are −1..1.
const MATERIAL_LIVE_STILL: [f32; 8] = [0.0, 0.1, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];

// ===========================================================================
// A material
// ===========================================================================

/// One named material: the `#472` map stack, the PBR response, and the per-material HSV.
///
/// Pure data. [`apply_material`] is the only thing that writes it into a `Shared`, and
/// [`material_field_manifest`] is the list of lanes it may touch.
#[derive(Clone, Copy)]
pub struct SubstrateMaterial {
    /// The name the CLI takes, matched case-insensitively.
    pub name: &'static str,
    /// → `material[2]`, world units → UV tiles. [`MATERIAL_UV_SCALE`] unless the material's
    /// finest layer cannot be reached inside `mp_scale`'s 0.25..64 range at the floor — see
    /// the module doc's sampling section. Per-material only because `graphite` needed it.
    pub uv_scale: f32,
    /// → `material_layer` + `material_grad`. Always enabled: `[16]` is the global
    /// procedural switch, so a material with no base layer would have no material at all.
    pub base: Layer,
    /// The base layer's albedo stops. Inert unless `base.channel` is Albedo.
    pub base_grad: [f32; 8],
    /// → `material_layer2` + `material_grad2`. `None` writes a disabled block.
    pub overlay: Option<Layer>,
    /// The overlay's albedo stops. Inert unless the overlay routes Albedo.
    pub overlay_grad: [f32; 8],
    /// → `material_derive`.
    pub derive: [f32; 8],
    /// → `lighting[7]`, the `MaterialType` wire value (`params.rs:3701-3718`: Standard = 0,
    /// Chrome 1, Glass 2, Refractive 3, **Anisotropic 4**, Clearcoat 5, Velvet 6,
    /// Subsurface 7). Decoded at `world.rs:10640`.
    pub mat_type: f32,
    /// → `pbr[0]`, metallic. Range 0..1 (`params.rs:8722`), decoded `world.rs:10612`.
    /// **What the metallic *map* would override**, so it is also the value that stands if a
    /// metallic channel is not baked — which is the case for all four.
    pub metallic: f32,
    /// → `pbr[1]`, roughness. Range 0..1 (`params.rs:8723`), decoded `world.rs:10613`.
    /// A baked roughness channel **replaces** this outright
    /// (`cube.wgsl:1417`: `roughness = mix(roughness, m_rough, has_rgh)` with `has_rgh` = 1),
    /// so where a material bakes roughness this scalar is set to the map's *mean*: it is what
    /// shows if the channel ever fails to arrive, and a scalar that disagreed with its own
    /// map would be a silent trap.
    pub roughness: f32,
    /// → `aniso[4]` = `[amount, rotation_deg, overlay_enable, overlay_blend]`
    /// (`ipc.rs:886-890`), decoded at `world.rs:10765-10769`.
    ///
    /// Two ways in, and they are exclusive: `cube.wgsl:1491-1497` uses `amount` **raw** when
    /// `mat_type` is exactly Anisotropic (4) — ignoring `overlay_enable` and `overlay_blend`
    /// entirely — and otherwise uses `amount · blend` only if `overlay_enable` is set. The
    /// brush direction is not in this block: it is the instance's local +Z basis in world
    /// (`cube.wgsl:681`, `out.brush = in.m2.xyz`), and the membrane sheet draws with the
    /// **identity** instance, so the brush is world **+Z**. `rotation` re-aims it within the
    /// tangent plane (`cube.wgsl:825-829`). Ranges: amount −1..1, rotation 0..360, blend
    /// 0..1 (`params.rs:8689-8692`).
    pub aniso: [f32; 4],
    /// → `matcol[0..4]` = `[hue, hue_cycle, saturation, value]` (`ipc.rs:1263-1270`),
    /// resolved to `matcol[0] + matcol[1]·beat` at `world.rs:10828` and applied by
    /// `apply_hsv` (`cube.wgsl:167-186`) at `:1479`.
    ///
    /// `[1]` (hue cycle) is **0 in every material**, and that is a stillness item, not a
    /// taste one: the beat clock cannot be stopped from `Shared` at all
    /// (`substrate_scene::KNOWN_UNSTILLABLE` #2), so a non-zero cycle would walk the
    /// substrate around the colour wheel forever. `[2]` (saturation) lerps toward luma
    /// (`cube.wgsl:182`) — Tier 1 pinned it to 0 for a neutral slate; a material with a
    /// colour of its own has to hand some of it back. Ranges: hue 0..1, cycle −2..2,
    /// saturation 0..1, value 0..1 (`params.rs:8892-8895`).
    pub matcol: [f32; 4],
}

// ===========================================================================
// The four materials
// ===========================================================================
//
// A note on where these numbers come from, because it changes how to read them. The layer
// shapes, channel routing, ranges and sampling rates are **derived** — from the bake shader,
// the range table and the arithmetic in the module doc. The *taste* is chosen, and this leaf
// has no GPU, so each material names the ONE dial to turn if it reads wrong.
//
// ✅ **All four have now been through a beat check on the RTX 5090** (2000 × 1314, scrim 96,
// studio rig). `slate` and `metal` passed untouched; `graphite` and `paper` failed and were
// revised. Their doc comments carry the verdict, the diagnosis and what moved — kept rather
// than tidied away, because in both cases the *reported* symptom pointed at the wrong dial
// and the shader said which one it really was. That is the part worth being able to re-read:
//
//   * graphite read as light-grey wallpaper, and the albedo bake was innocent — a dielectric's
//     specular does not scale with albedo, so a glossy near-black plane is a mirror. But
//     mirror of *what* is a question about the camera, and the value pass later measured the
//     answer this section first guessed wrong: top-down, the mirror direction is the Nishita
//     zenith — the DARK part of the sky — and roughness blurs toward the bright hemispheric
//     mean, so on this rig gloss is dark and rough is bright (metal 29.6 vs slate 96.3).
//     Graphite's own doc carries the measured four-point curve.
//   * paper read as black-and-white camouflage, and the albedo span was 0.11 — far too narrow
//     to reach either end. The patches were derived AO multiplying the ambient term.
//
// Both diagnoses were reached by re-reading `material_bake.wgsl` and `cube.wgsl` against the
// captured symptom, not by nudging constants until something looked different.

/// **slate** — Tier 1's shipped look, formalized, plus the grain it could not have.
///
/// The one material that deliberately does **not** bake albedo. Tier 1's non-uniform albedo
/// is the per-vertex `mem_col` ramp (`substrate_scene::SUBSTRATE_PALETTE`), and an albedo map
/// would replace it outright: `cube.wgsl:1477`'s `base_col = mix(in.color, m_albedo.rgb,
/// has_alb)` takes the map when the albedo bit is present. Routing the single layer to
/// **Roughness** leaves `has_alb` at 0, so the ramp arrives exactly as it does today and the
/// map adds what Tier 1 had no way to add: a broad variation in the specular sheen, so the
/// one wide lobe across the sheet breaks into cloud rather than a single gradient.
///
/// The output band is arithmetic, not a guess. `contrast` 0.20 compresses the shaped noise to
/// \[0.40, 0.60\]; `gamma` 1.30 maps that to **\[0.304, 0.515\], mean ≈ 0.406** — Tier 1's
/// `SUBSTRATE_ROUGHNESS` of 0.42, ±0.10. Feature size 131 px, finest octave ≈ 16 px.
///
/// ✅ **Tier 2 beat check: correct — Tier 1's look exactly.** Unchanged since.
///
/// *Dial:* `contrast` — up for a busier stone, down toward Tier 1's flat sheen.
pub const SLATE: SubstrateMaterial = SubstrateMaterial {
    name: "slate",
    uv_scale: MATERIAL_UV_SCALE,
    base: Layer {
        channel: 1.0, // Roughness
        scale: 6.0,
        octaves: 4.0,
        warp: 0.25,
        contrast: 0.20,
        gamma: 1.30,
        ..Layer::DEFAULT
    },
    base_grad: GRAD_UNUSED,
    overlay: None,
    overlay_grad: GRAD_UNUSED,
    derive: DERIVE_NONE,
    mat_type: 0.0,   // Standard
    metallic: 0.0,   // a dielectric slab, as Tier 1 argued
    roughness: 0.42, // = SUBSTRATE_ROUGHNESS, and the mean of the map above
    aniso: [0.0, 0.0, 0.0, 1.0],
    matcol: [0.0, 0.0, 0.0, 1.0], // = SUBSTRATE_MATCOL: saturation 0, the neutral slate
};

/// **graphite** — a dark, laminated sheen with a directional grain.
///
/// ❌ **Tier 2 beat check, first pass: FAIL** — "light-grey corduroy wallpaper". Mid-grey
/// ≈ 0.5+ on screen where graphite should sit near-black, and the stripes read as wide soft
/// columns. Both causes were found in the shader rather than guessed at, and the revision
/// below follows from them. **The albedo bake was NOT the culprit and was never bright:**
/// `remap` \[0.15, 0.90\] expands the field to the full \[0, 1\] and `contrast` 1.10 with
/// `gamma` 1.0 leaves it there, so `mix(grad_lo, grad_hi, v)` spanned exactly the near-black
/// \[0.030, 0.115\] it was asked for. Nothing compressed toward mid.
///
/// 🚨 **What lifts it is the specular — and on this scene ROUGHER IS BRIGHTER.** A
/// dielectric's specular does not scale with albedo (`F0` is 0.04 however black the pigment),
/// so the sheen carries the whole read and the albedo barely touches the *level*. That much
/// the first pass had right. What it got backwards was the direction: it argued that a glossy
/// near-black plane is "a dark mirror of a bright sky, and therefore bright", and moved
/// `gamma` 1.55 → 0.83 to make the sheet matter. **That made it brighter**, and a later pass
/// to the gamma floor (0.20) brighter still.
///
/// The counter-example is in this file. [`METAL`] is the glossiest material here — roughness
/// band \[0.116, 0.356\] — and it measures **29.6** where slate measures 96.3 on the same
/// pane: nearly black. The sky is bright only *in aggregate*. The substrate camera is
/// top-down on a horizontal sheet, so the mirror direction is the **zenith**, and the baked
/// Nishita zenith is the dark part of the sky. Roughness raises the prefiltered LOD
/// (`cube.wgsl:1071-1074`, `lod = roughness · (mip_count − 1)`), blurring the sample off that
/// dark zenith toward the bright hemispherical mean, and `fss_ess + fms·ems`
/// (`cube.wgsl:1967-1975`) hands back the multiple-scattering energy at high roughness on top
/// of it. Measured on the pane, in mean sRGB over a glyph-free region:
///
/// ```text
///     roughness band centre  0.189   0.250   0.563   0.871    (gamma 2.40 2.00 0.83 0.20)
///     graphite level          99.7   114.9   159.2   176.2    (slate 96.3, paper 155.2)
/// ```
///
/// So the order stands — **roughness first, albedo second** — but the sign is inverted from
/// what the first pass wrote down, and "second" is a distant second: at a 0.033 mean there is
/// almost nothing left in the albedo to take away.
///
/// Three cues, each on its own mechanism:
///
/// * **Albedo** — an FBM through a near-black gradient, now \[0.012, 0.055\] linear: the dark
///   end of real graphite's ≈ 0.02–0.08 reflectance. `warp` 0.30 smears the field, which is
///   what makes it read as laminated rather than as noise.
/// * **Roughness** — a `Stripes` overlay running **along world +Z** (the field is
///   `sin(p.x·π)`, `material_bake.wgsl:236-238`, and world-planar XZ puts noise-x on world x,
///   so the lines run along z). `contrast` 0.30 → \[0.35, 0.65\], `gamma` **2.40** →
///   **\[0.081, 0.356\], centre ≈ 0.19**: a satin lamination, and — on this scene, for the
///   reason below — the band that makes the sheet read dark. Its 0.275 gloss delta is wider
///   than any earlier pass carried, so the lines read as lamination rather than as a tint.
///   ("Centre" is `0.5^gamma`, the shaped midpoint, which is what [`SLATE`]'s "mean ≈ 0.406"
///   and [`METAL`]'s "≈ 0.21" are too — one definition across the four, and the one the
///   `roughness` scalar below tracks.)
/// * **Scale** — the one material that leaves [`MATERIAL_UV_SCALE`]. At `s = 0.02` the
///   coarsest possible stripe is `mp_scale = 64` → 24.6 px, and metal proves 24.6 px reads as
///   a *brush*; graphite wants lamination, several times finer. `s = 0.055` with `p = 64`
///   gives **8.95 px**. Affordable because a sine is bandlimited: 1.79 texels/px on a
///   16-texel period is eight samples per cycle, nowhere near Nyquist.
///
/// **No derived normal any more, and the reason is a real limitation worth keeping.** The
/// first pass derived one from albedo luminance, and it was inert: `derive_normal`'s Sobel
/// (`material_bake.wgsl:412-413`) scales with the *range* of what it reads, and this albedo's
/// luminance spans 0.085 — so `dx`/`dy` were ~0.05 at most and `normalize(−dx, −dy, 1)` came
/// out flat. Darkening the stops makes it flatter still; matching a unit-range height field
/// would need `strength` ≈ 14 against a 0..4 range. **Deriving a normal from albedo only
/// works on a material bright enough to have a luminance range** — `paper` can, `graphite`
/// cannot. It is off rather than left in as a parameter that does nothing.
///
/// "Anisotropic-adjacent" rather than anisotropic: the material stays **Standard** and takes
/// the anisotropy **overlay** (`aniso[2] = 1`), which `cube.wgsl:1494` scales as
/// `amount · blend` = 0.45 · 0.65 ≈ 0.29 — a stretched highlight along the same +Z the
/// stripes run down, without the full metal lobe. Metallic stays 0 on purpose: graphite's
/// conductivity is real, but a metallic near-black albedo tints its own specular to nothing
/// and the material disappears.
///
/// *Dial:* `overlay.gamma` — the roughness band's centre, and so the whole material's level.
/// **Up darkens, down brightens**, and state it in *gamma*, never in gloss: `pow(v, gamma)`
/// over a \[0.35, 0.65\] band means a higher gamma is a *lower* roughness, and here lower
/// roughness is darker. That is the reverse of the matte/gloss intuition, and saying "up
/// darkens (matter)" is what sent two passes the wrong way. The gradient stops are nominally
/// the second dial, but see above — they have almost nothing left to give.
pub const GRAPHITE: SubstrateMaterial = SubstrateMaterial {
    name: "graphite",
    uv_scale: 0.055, // see "Scale" above; range 0.02..16 (params.rs:8425)
    base: Layer {
        channel: 0.0, // Albedo
        scale: 8.0,   // 35.8 px mottle at s = 0.055
        octaves: 3.0, // 3 not 5: at s = 0.055 a 5th octave lands near the texel
        gain: 0.55,
        warp: 0.30,
        contrast: 1.10,
        remap: [0.15, 0.90],
        seed: 3.0,
        ..Layer::DEFAULT
    },
    base_grad: grad([0.012, 0.012, 0.015], [0.055, 0.055, 0.062]),
    overlay: Some(Layer {
        noise: 12.0,  // Stripes
        channel: 1.0, // Roughness
        scale: 64.0,  // the range maximum; 8.95 px per period at s = 0.055
        octaves: 1.0, // single-scale kind: unread, stated rather than left at 5
        warp: 0.10,   // 5% of a stripe — enough to unprint it, too little to alias
        contrast: 0.30,
        gamma: 2.40,
        seed: 5.0,
        ..Layer::DEFAULT
    }),
    overlay_grad: GRAD_UNUSED,
    derive: DERIVE_NONE, // albedo-sourced normals need a luminance range; see above
    mat_type: 0.0,       // Standard + the anisotropy overlay
    metallic: 0.0,
    roughness: 0.19, // ≈ the roughness map's centre (0.5^gamma), for the map-absent case
    aniso: [0.45, 0.0, 1.0, 0.65],
    matcol: [0.0, 0.0, 0.35, 1.0], // a whisper of the gradient's cool cast survives
};

/// **paper** — a bright fibrous matte, sized for legibility.
///
/// ❌ **Tier 2 beat check, first pass: FAIL** — "high-contrast blotchy static/camouflage;
/// dark clots fight the glyphs".
///
/// 🚨 **The albedo was not the blotch, and this matters because it decides which dial to
/// turn.** The reported symptom was near-black-to-white patches, but the albedo bake could
/// not produce them: `remap` \[0.20, 0.85\] expands to \[0, 1\], `contrast` 0.70 compresses
/// to \[0.15, 0.85\], and the gradient maps that to **\[0.224, 0.336\]** — a span of 0.11,
/// which cannot reach either end. The patches came from the two **derived** maps multiplying
/// the light instead:
///
/// * **Derived AO was the clot machine.** `material_bake.wgsl:437` computes
///   `ao = 1 − occ · 4 · ao_strength` — note the **×4**, so `ao_strength` 0.50 meant a ×2 on
///   occlusion — and `cube.wgsl:1419` applies it as `ambient_mul *= m_ao` at full strength.
///   Under a rig whose env is most of the light, a noisy height field therefore stamps dark
///   holes straight into the image. **It is off now.** My own `KNOWN_LIMITS` flagged this
///   derive as the riskiest of the pair; it was right for a reason I had not anticipated —
///   not the height/AO pairing, but the gain.
/// * **The derived normal was the static.** Strength 0.85 off a `contrast` 1.40 height field
///   gives large Sobel gradients at 1-texel spacing, so `N` swung hard and `N·L` with it.
///   Fixed at both ends: strength 0.85 → **0.22**, and the height's own `contrast` 1.40 →
///   **0.55** so the gradient is smaller *at the source* rather than merely scaled down after.
/// * **The camouflage scale was the albedo mottle**, at `p = 3` ≈ **262 px** on a 2000-px
///   pane — enormous. `p = 10` puts it at 79 px, and `contrast` 0.70 → **0.45** flattens the
///   variation to a span of 0.063: a sheet's formation, not a pattern.
///
/// Relief stays the material's character — a Height layer feeding a *whisper* of normal — but
/// nothing modulates the ambient any more, which is what makes it safe behind glyphs.
///
/// 🚨 **The scrim is why this is not white, and why it is not much brighter now either.** A
/// backdrop is read *through* `SCRIM_FLOOR = 96` (`term_view.rs:34`, `scrim_alpha`), i.e.
/// 96/255 ≈ 0.376 of black over it, so a backdrop pixel shows at 0.624× behind glyphs. Real
/// paper is ≈ 0.7 linear; under Tier 1's studio rig that lands far too hot. The revised stops
/// mean ≈ **0.37** linear (up from 0.28), which the rig's diffuse sum (≈ 1.22×, module
/// arithmetic) carries to ≈ 0.45 linear ≈ 0.68 sRGB and ≈ 0.42 behind the scrim. The bigger
/// legibility win is at the **dark** end: the old low was 0.224 *and then multiplied down by
/// AO*, where the new low is 0.339 and nothing attenuates it. So this is a paper *surface* at
/// a *backdrop* level, and the departure is named rather than hidden.
///
/// *Dial:* the gradient stops, and only them — `matcol[3]` (value) is left at identity on
/// purpose so there is exactly one place the brightness lives.
pub const PAPER: SubstrateMaterial = SubstrateMaterial {
    name: "paper",
    uv_scale: MATERIAL_UV_SCALE, // stays at the floor: a Sobel normal is not bandlimited
    base: Layer {
        channel: 0.0, // Albedo
        scale: 10.0,  // 79 px: the sheet's formation. Was 3.0 = 262 px, the "camouflage".
        octaves: 4.0,
        lacunarity: 2.2,
        gain: 0.45,
        warp: 0.60,
        contrast: 0.45, // → albedo span 0.063; a formation, not a pattern
        remap: [0.20, 0.85],
        seed: 7.0,
        ..Layer::DEFAULT
    },
    base_grad: grad([0.300, 0.290, 0.265], [0.440, 0.430, 0.395]),
    overlay: Some(Layer {
        channel: 3.0, // Height — read by the normal derive only, now that AO is off
        scale: 40.0,  // 19.7 px fibre, finest octave ≈ 9 px
        octaves: 2.0, // deliberately shallow: the Sobel derive already works at texel scale
        lacunarity: 2.2,
        warp: 0.15,
        contrast: 0.55, // was 1.40 — the Sobel gradient shrinks at the source
        seed: 11.0,
        ..Layer::DEFAULT
    }),
    overlay_grad: GRAD_UNUSED,
    derive: derive(true, false, false, 0.22, 1.0), // AO OFF; a whisper of normal
    mat_type: 0.0,                                 // Standard
    metallic: 0.0,
    roughness: 0.78, // scalar and unopposed: no roughness channel is baked
    aniso: [0.0, 0.0, 0.0, 1.0],
    matcol: [0.0, 0.0, 0.55, 1.0], // enough saturation to keep the warm cast
};

/// **metal** — brushed steel, on the real `MaterialType::Anisotropic` path (R5).
///
/// The only material here that changes `lighting[7]`. At exactly type 4, `cube.wgsl:1491-1497`
/// takes `aniso[0]` **raw** — `overlay_enable` and `overlay_blend` are not read at all on this
/// path — and splits the roughness into a stretched tangent/bitangent pair
/// (`aniso_alpha`, `cube.wgsl:835-850`) around a brush axis that is the identity instance's
/// local +Z, i.e. world **+Z**. `aniso[2]`/`aniso[3]` are still written, to inert values,
/// because a material must not leave graphite's overlay behind.
///
/// The brush axis is a legibility choice as much as a look. The substrate camera is top-down
/// with screen-up = world −Z (`console_main.rs:124-131`), so a +Z brush runs **vertically** up
/// the pane — across the glyph rows rather than along them, which is the orientation least
/// likely to be read as underlining. Both baked layers are `Stripes` on the same axis, so
/// grooves, banding and the stretched highlight all agree.
///
/// Metallic **1.0**, so the albedo *is* the specular colour — hence stops at steel's linear
/// reflectance (0.52–0.72, faintly cool) rather than a diffuse grey. Roughness
/// `contrast` 0.25 → \[0.375, 0.625\], `gamma` 2.20 → **\[0.116, 0.356\]**, mean ≈ 0.21: a
/// polished brush, not a satin one. No derived normal: the anisotropic lobe *is* the brushed
/// cue, and a Sobel normal off the same stripes would double it at texel scale.
///
/// ✅ **Tier 2 beat check: good under both rigs** — dark blue-steel, the vertical brushing
/// reads, calm sheen, legible. Unchanged since. Its stripes land at **24.6 px** and that is
/// now the calibrated reference for "a brush that reads without becoming corduroy" — the
/// number `graphite` was re-sized against.
///
/// *Dial:* `aniso[0]` — the whole streak, in one number.
pub const METAL: SubstrateMaterial = SubstrateMaterial {
    name: "metal",
    uv_scale: MATERIAL_UV_SCALE,
    base: Layer {
        noise: 12.0,  // Stripes
        channel: 0.0, // Albedo
        scale: 40.0,  // ≈ 21 px per stripe period
        octaves: 1.0,
        warp: 0.12,
        contrast: 0.50,
        seed: 2.0,
        ..Layer::DEFAULT
    },
    base_grad: grad([0.520, 0.530, 0.550], [0.680, 0.690, 0.720]),
    overlay: Some(Layer {
        noise: 12.0,  // Stripes
        channel: 1.0, // Roughness
        scale: 64.0,  // the range maximum; ≈ 13 px per period
        octaves: 1.0,
        warp: 0.15,
        contrast: 0.25,
        gamma: 2.20,
        seed: 9.0,
        ..Layer::DEFAULT
    }),
    overlay_grad: GRAD_UNUSED,
    derive: DERIVE_NONE,
    mat_type: 4.0,   // MaterialType::Anisotropic — params.rs:3710-3711
    metallic: 1.0,
    roughness: 0.22, // ≈ the roughness map's mean
    aniso: [0.75, 0.0, 0.0, 1.0], // [2] and [3] unread at type 4; written inert anyway
    matcol: [0.0, 0.0, 0.50, 1.0], // half saturation: steel, not blue
};

/// Every material, in [`MATERIAL_NAMES`] order.
static MATERIALS: [&SubstrateMaterial; 4] = [&GRAPHITE, &PAPER, &SLATE, &METAL];

// ===========================================================================
// The two lighting rigs
// ===========================================================================
//
// R5's rig is "key elev/azim/intensity, fill intensity, ambient, env intensity/rotation/tint,
// exposure". These two write a strict subset of that — **levels only, never a direction** —
// and the reason is that the directions are not this file's to move:
//
// * `lighting[4]` (key azimuth) is **overridden by the integrator** after the look is applied
//   (`console_main.rs:144-159`), re-derived for the camera that file installs. A rig that wrote
//   it would be silently discarded on one path and fight on another.
// * `lighting[3]` (key elevation) is not overridden, but elevation and azimuth are one
//   statement about where the light is; moving one without the other is half a rig. Tier 1
//   set both together and the integrator corrected the pair.
// * The fill's direction is derived and unaimable regardless — `world.rs:10637` builds it as
//   `dir_from_angles(10.0, azimuth − 120.0)`. R5 said "aim the fill is not a thing here", and
//   the brief's Tier-2 row says to promise intensity only.
//
// So a rig here is: key intensity, fill intensity, ambient, env intensity, exposure. Five
// numbers, no geometry. That is less than R5's full list and it is the honest subset.

/// One named lighting rig — the five level fields, and nothing that points anywhere.
#[derive(Clone, Copy)]
pub struct SubstrateRigLook {
    /// The name the CLI takes, matched case-insensitively.
    pub name: &'static str,
    /// → `lighting[0]`, the IBL multiplier. Range 0..3 (`params.rs:8550`), decoded
    /// `world.rs:10625-10641`.
    ///
    /// **Pinned at unity in both rigs**, following Tier 1's finding: `ambient_mul` and
    /// `env_intensity` are only ever used as a *product* (`cube.wgsl:1553`, `:1587`, `:1687`,
    /// `:1890`), so two knobs for one quantity double-dim. It is written rather than skipped
    /// so that switching rigs cannot leave a stray multiplier behind.
    pub ambient: f32,
    /// → `lighting[1]`, key intensity. Range 0..6 (`params.rs:8551`).
    pub key: f32,
    /// → `lighting[2]`, fill intensity. Range 0..3 (`params.rs:8552`). Grazing by
    /// construction: fixed 10° elevation means `N·L_fill = sin(10°) = 0.174` on this plane.
    pub fill: f32,
    /// → `pbr[3]`, environment intensity — the single env-level knob. Range 0..4
    /// (`params.rs:8725`), decoded `world.rs:10618`.
    pub env: f32,
    /// → `pbr[2]`, exposure in **EV stops**; `world.rs:10617` takes `2^pbr[2]`. Range −8..4
    /// (`params.rs:8724`).
    pub exposure_ev: f32,
}

/// **studio** — key-forward, and *exactly* Tier 1's shipped rig.
///
/// Every value is `substrate_scene`'s: ambient 1.0, key 2.6, fill 0.35, env 0.65, exposure 0.
/// That is not a coincidence to be tidied away — it is the property that makes "substrate +
/// slate + studio" byte-identical to what Tier 1 shipped, which a test asserts. The new
/// vocabulary describes the old picture before it changes it.
///
/// On a plane with one constant normal, `N·L` is constant everywhere for both analytic
/// lights: a key-forward rig sets the sheet's overall level and puts the specular gradient
/// where the narrow FOV can sweep across it. It cannot draw a shape.
pub const STUDIO: SubstrateRigLook = SubstrateRigLook {
    name: "studio",
    ambient: 1.0,
    key: 2.6,
    fill: 0.35,
    env: 0.65,
    exposure_ev: 0.0,
};

/// **daylight** — env-forward: the sky does the work.
///
/// The environment is a real Nishita atmosphere baked into the IBL (`atmos_enabled` defaults
/// **true**, `params.rs:9040`; `world.rs:6694` selects `EnvReq::Atmosphere`), and an
/// upward-facing plane under a full sky collects a great deal of irradiance. Pushing `env`
/// from 0.65 to 1.9 and dropping the key from 2.6 to 1.1 moves the balance from a directional
/// lobe to broad hemispherical light — softer, flatter, with the specular reading as sky
/// rather than as a lamp. The fill comes up to 0.9 so the grazing side does not go dead once
/// the key is no longer carrying it.
///
/// **Exposure −0.8 EV is a level correction, not a look.** By the module's diffuse arithmetic
/// the rig's albedo multiplier goes from ≈ 1.22 to ≈ 2.18 — 1.79× — and `2^-0.8` = 0.574
/// pulls that back to ≈ 1.25×. Daylight *should* read brighter than studio; it should not read
/// 1.8× brighter, because a backdrop that outruns the scrim takes the glyphs with it.
pub const DAYLIGHT: SubstrateRigLook = SubstrateRigLook {
    name: "daylight",
    ambient: 1.0,
    key: 1.1,
    fill: 0.9,
    env: 1.9,
    exposure_ev: -0.8,
};

/// Every rig, in [`RIG_NAMES`] order.
static RIGS: [&SubstrateRigLook; 2] = [&STUDIO, &DAYLIGHT];

// ===========================================================================
// The two writers
// ===========================================================================

/// Write the named material into `s`. Returns `false` for an unknown name, having **touched
/// nothing**.
///
/// Assumes [`crate::substrate_scene::apply_substrate_look`] has already run: this writes only
/// the surface response, never the geometry, the stillness switches, the palette or the
/// background. Names match **case-insensitively**, following the backdrop selector's own
/// precedent (`console_main.rs:96`); normalizing beyond that is the CLI's job.
///
/// Pure: a total function of the fields it writes, with no dependence on the rest of `s`, no
/// I/O, no clock and no allocation. Idempotent by construction — every write is a constant —
/// and, more usefully, **total over the block**: every lane a material can use is written on
/// every call, including the ones this particular material does not need (a disabled overlay,
/// unused gradient stops, cleared derive flags, an inert `aniso`). That is not tidiness. A
/// material that wrote only its own lanes would inherit its predecessor's overlay, and
/// `graphite → slate` would leave graphite's stripes baked into slate's roughness.
///
/// Writes existing fields only — no `Shared` layout change — and leaves `seq` /
/// `layout_version` to the publisher. The exact lane list is [`material_field_manifest`].
pub fn apply_material(s: &mut Shared, name: &str) -> bool {
    let Some(m) = MATERIALS.iter().find(|m| m.name.eq_ignore_ascii_case(name)) else {
        return false;
    };

    // --- the map stack ---------------------------------------------------------------
    // `material[0]` (the Tier-1 PNG texture-set master) is deliberately NOT written: the
    // procedural bake supersedes any loaded folder (`render.rs`'s `bake_material` →
    // `set_procedural`), and `world.rs:10852` already turns `Uniforms.mtl.x` on from
    // `material_layer[16]` alone. Writing it would express nothing. `material[3..8]` are
    // reserved (`param_table.rs:1210-1214`) and read by no shader — likewise untouched.
    s.material[1] = MATERIAL_PROJECTION;
    s.material[2] = m.uv_scale;
    s.material_layer = m.base.pack_base();
    s.material_grad = m.base_grad;
    s.material_layer2 = match m.overlay {
        Some(l) => l.pack_overlay(true),
        // A disabled layer is skipped outright by both the bake and the present mask
        // (`material_bake.wgsl:381`, `render.rs`'s `present_mask`), so its other slots are
        // inert — but they are still written, so nothing is inherited.
        None => Layer::DEFAULT.pack_overlay(false),
    };
    s.material_grad2 = m.overlay_grad;
    s.material_derive = m.derive;
    s.material_live = MATERIAL_LIVE_STILL;

    // --- the PBR response ------------------------------------------------------------
    s.lighting[7] = m.mat_type;
    s.pbr[0] = m.metallic;
    s.pbr[1] = m.roughness;
    s.aniso = m.aniso;

    // --- the per-material HSV --------------------------------------------------------
    // `matcol[4..8]` is the SCENERY material and is not ours (scenery is off — SceneryMode
    // ::None, `params.rs:8205`).
    s.matcol[0] = m.matcol[0];
    s.matcol[1] = m.matcol[1];
    s.matcol[2] = m.matcol[2];
    s.matcol[3] = m.matcol[3];
    true
}

/// Write the named lighting rig into `s`. Returns `false` for an unknown name, having
/// **touched nothing**.
///
/// Levels only — see the rig section's note on why no direction is written here. Disjoint
/// from [`apply_material`] by lane, so the two compose in either order. Same purity,
/// idempotence and case-insensitivity as [`apply_material`]; the lane list is
/// [`rig_field_manifest`].
pub fn apply_rig(s: &mut Shared, name: &str) -> bool {
    let Some(r) = RIGS.iter().find(|r| r.name.eq_ignore_ascii_case(name)) else {
        return false;
    };
    s.lighting[0] = r.ambient;
    s.lighting[1] = r.key;
    s.lighting[2] = r.fill;
    s.pbr[2] = r.exposure_ev;
    s.pbr[3] = r.env;
    true
}

// ===========================================================================
// The manifests
// ===========================================================================

/// Every `Shared` lane [`apply_material`] writes, by name, paired with a restore that copies
/// that lane back from a reference snapshot.
///
/// Same device as `substrate_scene::SUBSTRATE_FIELD_MANIFEST` — apply, restore every declared
/// lane from the reference, and the result must be byte-identical — with one change: entries
/// are at **lane** granularity for `lighting`, `pbr` and `matcol`, because those blocks are
/// shared with [`rig_field_manifest`] and a whole-block restore could not tell a material's
/// write from a rig's. Tighter in both directions: it also catches a stray write *inside* a
/// declared block, which a whole-array restore cannot.
static MATERIAL_FIELD_MANIFEST: &[(&str, FieldRestore)] = &[
    // the map stack
    ("material[1]", |d, r| d.material[1] = r.material[1]),
    ("material[2]", |d, r| d.material[2] = r.material[2]),
    ("material_layer", |d, r| d.material_layer = r.material_layer),
    ("material_grad", |d, r| d.material_grad = r.material_grad),
    ("material_layer2", |d, r| d.material_layer2 = r.material_layer2),
    ("material_grad2", |d, r| d.material_grad2 = r.material_grad2),
    ("material_derive", |d, r| d.material_derive = r.material_derive),
    ("material_live", |d, r| d.material_live = r.material_live),
    // the PBR response
    ("lighting[7]", |d, r| d.lighting[7] = r.lighting[7]),
    ("pbr[0]", |d, r| d.pbr[0] = r.pbr[0]),
    ("pbr[1]", |d, r| d.pbr[1] = r.pbr[1]),
    ("aniso", |d, r| d.aniso = r.aniso),
    // the per-material HSV
    ("matcol[0]", |d, r| d.matcol[0] = r.matcol[0]),
    ("matcol[1]", |d, r| d.matcol[1] = r.matcol[1]),
    ("matcol[2]", |d, r| d.matcol[2] = r.matcol[2]),
    ("matcol[3]", |d, r| d.matcol[3] = r.matcol[3]),
];

/// Every `Shared` lane [`apply_rig`] writes. See [`MATERIAL_FIELD_MANIFEST`].
static RIG_FIELD_MANIFEST: &[(&str, FieldRestore)] = &[
    ("lighting[0]", |d, r| d.lighting[0] = r.lighting[0]),
    ("lighting[1]", |d, r| d.lighting[1] = r.lighting[1]),
    ("lighting[2]", |d, r| d.lighting[2] = r.lighting[2]),
    ("pbr[2]", |d, r| d.pbr[2] = r.pbr[2]),
    ("pbr[3]", |d, r| d.pbr[3] = r.pbr[3]),
];

/// See [`MATERIAL_FIELD_MANIFEST`].
pub fn material_field_manifest() -> &'static [(&'static str, FieldRestore)] {
    MATERIAL_FIELD_MANIFEST
}

/// See [`RIG_FIELD_MANIFEST`].
pub fn rig_field_manifest() -> &'static [(&'static str, FieldRestore)] {
    RIG_FIELD_MANIFEST
}

/// What Tier 2's material set still cannot do or cannot honestly claim — **integrator and
/// follow-up items**, recorded rather than fudged.
///
/// 1. **All four have been seen once, at one size, under one rig.** The beat check ran at
///    2000 × 1314 with the studio rig (plus `metal` under `daylight`); `slate` and `metal`
///    passed, `graphite` and `paper` were revised against it and have **not** been re-captured.
///    The revisions are arithmetic on mechanisms the captures confirmed — the model predicted
///    graphite's stripe pitch to within an eyeball estimate — but arithmetic is not a
///    photograph. Six of the eight material×rig pairs remain unseen.
/// 2. **The texel rate is tied to the pane's pixel width** (module doc, and it is *width*
///    because cover framing on a landscape pane is governed by the horizontal axis). At 2000 px
///    the floor gives 0.65 texels/px and `graphite`'s 0.055 gives 1.79. Halve the pane and both
///    double, which puts `graphite` past the no-mip budget. The fixes are a mip chain on
///    `MaterialBaker::make_target` or a coarser `mp_scale` — both outside this leaf.
/// 3. **`apply_substrate_look` does not turn the maps off.** Tier 1 predates them and writes
///    no `material_*` field, so re-running it over a material leaves the material on. There is
///    no "none" material name here on purpose — an off switch belongs with whoever owns the
///    console's material state, not in a table of four looks.
/// 4. **The material's bake is not free on the frame it changes.** `world.rs:6685-6706` bakes
///    whenever the layer params change; steady state is a no-op (`MaterialBaker::bake` keys on
///    the layer bytes), but a rapid sequence of `background <name>` commands dispatches a
///    full six-channel composite plus the derive passes each time. Fine for a human at a
///    prompt; worth knowing before anything drives it from a clock.
/// 5. **Derived AO is off everywhere, and should probably stay off.** The beat check found it
///    is not merely fussy about its Height pairing (see [`derive`]) — it is *loud*:
///    `material_bake.wgsl:437` multiplies the occlusion by **4** before `ao_strength` scales
///    it, and `cube.wgsl:1419` then multiplies the whole ambient term by the result at full
///    strength. On an env-forward rig that is a hole punched in the backdrop. Anything wanting
///    contact shading here should reach for a gentler `ao_strength` **and** a much smoother
///    height field, and be captured before it is believed.
/// 6. **Deriving a normal from albedo luminance needs a bright albedo.** The Sobel scales with
///    the range of what it reads (`material_bake.wgsl:412-413`), so on `graphite`'s ≈ 0.085
///    luminance span it produced a flat normal and the parameter was doing nothing. There is
///    no in-range `strength` that compensates. Bake a Height layer instead — height is unit
///    range whatever the albedo does.
/// 7. **A rig cannot aim anything** (see the rig section). Key elevation and azimuth are left
///    to Tier 1 and the integrator, so "two lighting rigs" means two *levels*, not two
///    directions — which is the same limit R5 recorded for the fill, applied honestly to the
///    key as well.
/// 8. **`graphite`'s level now lives in its roughness, which is a coupling worth knowing.**
///    Because a dielectric's specular is albedo-independent, the gradient stops set how dark it
///    *can* be and `overlay.gamma` sets how much sheen is added on top. Darkening the stops
///    alone will not darken the material; that was the first pass's mistake and it is easy to
///    repeat.
pub const KNOWN_LIMITS: () = ();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::OrganicMathParams;
    use crate::substrate_scene::{self, apply_substrate_look};

    /// The baseline every test starts from: Tier 1's substrate snapshot, which is what these
    /// functions are specified as deltas on.
    fn substrate() -> Shared {
        let mut s = OrganicMathParams::default().to_shared();
        apply_substrate_look(&mut s);
        s
    }

    fn with_material(name: &str) -> Shared {
        let mut s = substrate();
        assert!(apply_material(&mut s, name), "`{name}` must be a known material");
        s
    }

    fn with_rig(name: &str) -> Shared {
        let mut s = substrate();
        assert!(apply_rig(&mut s, name), "`{name}` must be a known rig");
        s
    }

    // -----------------------------------------------------------------------------
    // (a) every value, exactly
    // -----------------------------------------------------------------------------

    /// The shared map-stack preamble, identical for all four.
    #[test]
    fn every_material_writes_the_same_projection_and_bake_size() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            assert_eq!(s.material[1], 1.0, "{name}: projection must be world-planar XZ");
            assert_eq!(s.material_layer[16], 1.0, "{name}: procedural must be ON");
            assert_eq!(s.material_layer[17], 512.0, "{name}: bake size in PIXELS, not the enum");
            // The stillness pair — a material must not animate or displace.
            assert_eq!(s.material_live[0], 0.0, "{name}: the material bake must not animate");
            assert_eq!(s.material_live[5], 0.0, "{name}: height→vertex displace must stay off");
        }
    }

    /// `uv_scale` is per-material, but only one material may depart from the floor — and only
    /// because `mp_scale` caps at 64. A departure costs sampling headroom (32.5·s texels/px),
    /// which is affordable for bandlimited content and not for a Sobel-derived normal, so the
    /// rule is pinned rather than left as prose: **any material that derives a normal stays at
    /// [`MATERIAL_UV_SCALE`].** `paper` is the case this protects.
    #[test]
    fn only_bandlimited_materials_leave_the_uv_scale_floor() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let derives_normal = s.material_derive[0] > 0.5;
            if derives_normal {
                assert_eq!(
                    s.material[2], MATERIAL_UV_SCALE,
                    "{name} derives a normal, so it must sample at the floor"
                );
            }
            // Whatever it chose, the sampling rate must stay inside the range where bilinear
            // without mips is honest (see the module doc's table).
            let texels_per_px = 32.5 * s.material[2];
            assert!(
                texels_per_px <= 2.0,
                "{name}: {texels_per_px} texels/px on a 2000-px pane is past the no-mip budget"
            );
        }
        assert_eq!(GRAPHITE.uv_scale, 0.055, "graphite is the one departure");
        for m in [&SLATE, &PAPER, &METAL] {
            assert_eq!(m.uv_scale, MATERIAL_UV_SCALE, "`{}` must stay at the floor", m.name);
        }
    }

    #[test]
    fn slate_sets_every_declared_field() {
        let s = with_material("slate");
        assert_eq!(
            s.material_layer,
            [3.0, 1.0, 6.0, 0.0, 0.0, 0.0, 4.0, 2.0, 0.5, 0.25, 0.20, 1.30, 0.0, 1.0, 0.0, 0.0, 1.0, 512.0]
        );
        assert_eq!(s.material_layer2[16], 0.0, "slate has no overlay");
        assert_eq!(s.material_derive, [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 0.0, 0.0]);
        assert_eq!(s.lighting[7], 0.0, "Standard");
        assert_eq!(s.pbr[0], 0.0, "metallic");
        assert_eq!(s.pbr[1], 0.42, "roughness = the map's mean");
        assert_eq!(s.aniso, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!([s.matcol[0], s.matcol[1], s.matcol[2], s.matcol[3]], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn graphite_sets_every_declared_field() {
        let s = with_material("graphite");
        assert_eq!(s.material[2], 0.055, "the one material off the uv_scale floor");
        assert_eq!(
            s.material_layer,
            [3.0, 0.0, 8.0, 0.0, 0.0, 0.0, 3.0, 2.0, 0.55, 0.30, 1.10, 1.0, 0.15, 0.90, 0.0, 3.0, 1.0, 512.0]
        );
        assert_eq!(s.material_grad, [0.012, 0.012, 0.015, 0.0, 0.055, 0.055, 0.062, 0.0]);
        assert_eq!(
            s.material_layer2,
            [12.0, 1.0, 64.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 0.10, 0.30, 2.40, 0.0, 1.0, 0.0, 5.0, 1.0, 0.0]
        );
        // No derived maps: an albedo-sourced normal is inert on a material this dark (the
        // Sobel scales with the albedo's luminance range — see the doc comment).
        assert_eq!(s.material_derive, DERIVE_NONE);
        assert_eq!(s.lighting[7], 0.0, "Standard — the anisotropy is an OVERLAY");
        assert_eq!(s.pbr[0], 0.0, "metallic 0: a near-black metal has no specular colour");
        assert_eq!(s.pbr[1], 0.19, "= the roughness map's centre; see the band test below");
        assert_eq!(s.aniso, [0.45, 0.0, 1.0, 0.65]);
        assert_eq!(s.matcol[2], 0.35, "saturation");
    }

    /// The beat-check regression, and it guards the **opposite** of what its predecessor did.
    ///
    /// That test floored graphite's roughness — `assert!(lo > 0.40, "still a mirror")` — on the
    /// reasoning that a dielectric's specular is albedo-independent, so a *glossy* near-black
    /// plane is a dark mirror of a bright sky and therefore bright. The first half is true and
    /// the conclusion is not, and believing it drove `gamma` 1.55 → 0.83 → 0.20, each step
    /// making the sheet **paler**. Measured on the pane (2000 px, scrim 96, studio):
    ///
    /// ```text
    ///     band centre   0.189   0.250   0.563   0.871
    ///     graphite       99.7   114.9   159.2   176.2     slate 96.3, paper 155.2
    /// ```
    ///
    /// Rougher is brighter here. The substrate camera is top-down, so the mirror direction is
    /// the **zenith** — the dark part of the baked Nishita sky — and roughness raises the
    /// prefiltered LOD (`cube.wgsl:1071-1074`) until the sample is the bright hemispherical
    /// mean instead. [`METAL`] is the standing proof: glossiest material in the file, and it
    /// measures 29.6 where slate measures 96.3.
    ///
    /// So the guard is a **ceiling** on the band, never a floor. Let graphite drift matte and
    /// it goes pale again — which is the bug this file has now had twice.
    #[test]
    fn graphite_band_stays_glossy_enough_to_read_dark() {
        // The bake's own arithmetic (material_bake.wgsl:315-322), restated.
        let band = |contrast: f32, gamma: f32, raw: f32| {
            ((raw - 0.5) * contrast + 0.5).clamp(0.0, 1.0).powf(gamma.max(1e-3))
        };
        let l = GRAPHITE.overlay.expect("graphite bakes a roughness overlay");
        assert_eq!(l.channel, 1.0, "…to the Roughness channel");
        let (lo, hi) = (band(l.contrast, l.gamma, 0.0), band(l.contrast, l.gamma, 1.0));

        // The band the doc comment quotes, recomputed from the constants rather than copied
        // beside them — so the prose cannot drift away from the numbers it describes.
        assert!((lo - 0.081).abs() < 1e-3, "glossiest stripe {lo}, doc quotes 0.081");
        assert!((hi - 0.356).abs() < 1e-3, "mattest stripe {hi}, doc quotes 0.356");

        // The regression itself: at a centre of 0.563 graphite measured 159.2, *brighter than
        // paper*. Nothing may drift back toward that.
        assert!(hi < 0.40, "mattest stripe {hi} is into the pale regime (0.563 read 159.2)");
        // The lamination is the band's own width, so it may not be bought back by flattening.
        // Iteration 1's gamma 0.20 collapsed this to 0.107 and the stripes vanished.
        assert!(hi - lo > 0.20, "band span {} is too narrow to read as lamination", hi - lo);

        // The map-absent scalar tracks the map's centre — `0.5^gamma`, the same definition
        // slate (1.30 → 0.406, declares 0.42) and metal (2.20 → 0.218, declares 0.22) use — or
        // a dropped roughness bit silently restores the failure above.
        let centre = band(l.contrast, l.gamma, 0.5);
        assert!(
            (GRAPHITE.roughness - centre).abs() < 0.02,
            "scalar roughness {} disagrees with the map's centre {centre}",
            GRAPHITE.roughness
        );
    }

    #[test]
    fn paper_sets_every_declared_field() {
        let s = with_material("paper");
        assert_eq!(
            s.material_layer,
            [3.0, 0.0, 10.0, 0.0, 0.0, 0.0, 4.0, 2.2, 0.45, 0.60, 0.45, 1.0, 0.20, 0.85, 0.0, 7.0, 1.0, 512.0]
        );
        assert_eq!(s.material_grad, [0.300, 0.290, 0.265, 0.0, 0.440, 0.430, 0.395, 0.0]);
        assert_eq!(
            s.material_layer2,
            [3.0, 3.0, 40.0, 0.0, 0.0, 0.0, 2.0, 2.2, 0.5, 0.15, 0.55, 1.0, 0.0, 1.0, 0.0, 11.0, 1.0, 0.0]
        );
        assert_eq!(s.material_derive, [1.0, 0.0, 0.0, 0.22, 1.0, 2.0, 0.0, 0.0]);
        assert_eq!(s.pbr[1], 0.78, "matte, and unopposed: no roughness channel is baked");
        assert_eq!(s.aniso, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.matcol[3], 1.0, "value stays identity — the gradient is the one level dial");
    }

    /// The beat-check regression: **nothing may modulate the ambient term.** Derived AO is
    /// applied as `ambient_mul *= m_ao` at full strength (`cube.wgsl:1419`) after
    /// `material_bake.wgsl:437` has already multiplied the occlusion by **4**, so on a rig
    /// whose environment is most of the light it stamps dark holes into the backdrop — the
    /// "clots that fight the glyphs". No material here derives AO any more.
    ///
    /// The second half is the legibility floor the scrim implies, checked at the **dark** end,
    /// because that is where glyph contrast is actually lost.
    #[test]
    fn paper_cannot_clot_behind_the_glyphs() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            assert_eq!(s.material_derive[1], 0.0, "{name}: derived AO modulates the ambient");
        }
        // The albedo the bake can actually produce, restated from material_bake.wgsl:315-322
        // + :338-340 — remap expands, contrast compresses, then the gradient maps it.
        let l = PAPER.base;
        let shaped = |raw: f32| {
            let v = ((raw - l.remap[0]) / (l.remap[1] - l.remap[0])).clamp(0.0, 1.0);
            ((v - 0.5) * l.contrast + 0.5).clamp(0.0, 1.0)
        };
        let albedo = |raw: f32| PAPER.base_grad[0] + (PAPER.base_grad[4] - PAPER.base_grad[0]) * shaped(raw);
        let (lo, hi) = (albedo(0.0), albedo(1.0));
        assert!(lo > 0.30, "paper's darkest albedo is {lo} — the clot end");
        assert!(hi < 0.46, "paper's brightest albedo is {hi} — too hot behind a 96 scrim");
        assert!(hi - lo < 0.10, "a {} albedo span reads as pattern, not formation", hi - lo);
    }

    #[test]
    fn metal_sets_every_declared_field() {
        let s = with_material("metal");
        assert_eq!(
            s.material_layer,
            [12.0, 0.0, 40.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 0.12, 0.50, 1.0, 0.0, 1.0, 0.0, 2.0, 1.0, 512.0]
        );
        assert_eq!(s.material_grad, [0.520, 0.530, 0.550, 0.0, 0.680, 0.690, 0.720, 0.0]);
        assert_eq!(
            s.material_layer2,
            [12.0, 1.0, 64.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 0.15, 0.25, 2.20, 0.0, 1.0, 0.0, 9.0, 1.0, 0.0]
        );
        // No derived normal: the anisotropic lobe IS the brushed cue, and a Sobel normal off
        // the same stripes would double it at texel scale.
        assert_eq!(s.material_derive, DERIVE_NONE);
        assert_eq!(s.lighting[7], 4.0, "MaterialType::Anisotropic — params.rs:3710-3711");
        assert_eq!(s.pbr[0], 1.0, "a real metal: albedo IS the specular colour");
        assert_eq!(s.pbr[1], 0.22);
        assert_eq!(s.aniso[0], 0.75, "raw amount — overlay enable/blend are unread at type 4");
        assert_eq!(s.matcol[2], 0.50);
    }

    #[test]
    fn rigs_set_every_declared_field() {
        let studio = with_rig("studio");
        assert_eq!(studio.lighting[0], 1.0, "ambient pinned at unity");
        assert_eq!(studio.lighting[1], 2.6, "key");
        assert_eq!(studio.lighting[2], 0.35, "fill");
        assert_eq!(studio.pbr[2], 0.0, "exposure EV");
        assert_eq!(studio.pbr[3], 0.65, "env intensity");

        let daylight = with_rig("daylight");
        assert_eq!(daylight.lighting[0], 1.0, "ambient pinned at unity in BOTH rigs");
        assert_eq!(daylight.lighting[1], 1.1);
        assert_eq!(daylight.lighting[2], 0.9);
        assert_eq!(daylight.pbr[2], -0.8);
        assert_eq!(daylight.pbr[3], 1.9);
    }

    /// The property the `studio` doc comment claims: it is Tier 1's rig verbatim, so applying
    /// it over a fresh substrate snapshot changes **nothing**. If Tier 1's constants ever
    /// move, this is where the two stop agreeing.
    #[test]
    fn studio_is_exactly_tier_ones_shipped_rig() {
        let before = substrate();
        let after = with_rig("studio");
        assert_eq!(
            bytemuck::bytes_of(&after),
            bytemuck::bytes_of(&before),
            "`studio` must reproduce substrate_scene's rig byte for byte"
        );
        // And say it in Tier 1's own vocabulary, so the failure names the drifted constant.
        assert_eq!(STUDIO.ambient, substrate_scene::SUBSTRATE_AMBIENT);
        assert_eq!(STUDIO.key, substrate_scene::SUBSTRATE_KEY_INTENSITY);
        assert_eq!(STUDIO.fill, substrate_scene::SUBSTRATE_FILL_INTENSITY);
        assert_eq!(STUDIO.env, substrate_scene::SUBSTRATE_ENV_INTENSITY);
        assert_eq!(STUDIO.exposure_ev, substrate_scene::SUBSTRATE_EXPOSURE_EV);
    }

    /// `slate` likewise formalizes Tier 1's *surface*: the two scalars Tier 1 set are the
    /// ones it keeps, and it adds no albedo map, so Tier 1's per-vertex ramp survives.
    #[test]
    fn slate_keeps_tier_ones_surface_scalars_and_its_per_vertex_ramp() {
        let s = with_material("slate");
        assert_eq!(s.pbr[0], substrate_scene::SUBSTRATE_METALLIC);
        assert_eq!(s.pbr[1], substrate_scene::SUBSTRATE_ROUGHNESS);
        assert_eq!(s.lighting[7], substrate_scene::SUBSTRATE_MATERIAL_TYPE);
        assert_eq!(s.matcol[2], substrate_scene::SUBSTRATE_MATCOL[2]);
        // The claim that matters: no layer routes Albedo (channel 0), so `has_alb` stays 0
        // and `cube.wgsl:1477`'s `mix(in.color, m_albedo.rgb, has_alb)` returns the mesh's
        // per-vertex colour exactly.
        assert_ne!(s.material_layer[1], 0.0, "slate's base layer must not route Albedo");
        assert_eq!(s.material_layer2[16], 0.0, "…and it has no overlay to route it either");
    }

    // -----------------------------------------------------------------------------
    // (b) the manifests
    // -----------------------------------------------------------------------------

    #[test]
    fn apply_material_touches_only_the_manifest_lanes() {
        for name in MATERIAL_NAMES {
            let before = substrate();
            let mut restored = with_material(name);
            for (_, restore) in material_field_manifest() {
                restore(&mut restored, &before);
            }
            assert_eq!(
                bytemuck::bytes_of(&restored),
                bytemuck::bytes_of(&before),
                "`{name}` wrote a lane material_field_manifest does not declare"
            );
        }
    }

    #[test]
    fn apply_rig_touches_only_the_manifest_lanes() {
        for name in RIG_NAMES {
            let before = substrate();
            let mut restored = with_rig(name);
            for (_, restore) in rig_field_manifest() {
                restore(&mut restored, &before);
            }
            assert_eq!(
                bytemuck::bytes_of(&restored),
                bytemuck::bytes_of(&before),
                "`{name}` wrote a lane rig_field_manifest does not declare"
            );
        }
    }

    #[test]
    fn manifest_names_are_unique_and_the_two_manifests_are_disjoint() {
        let all: Vec<&str> = material_field_manifest()
            .iter()
            .chain(rig_field_manifest())
            .map(|(n, _)| *n)
            .collect();
        assert!(!all.is_empty());
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate/overlapping manifest entry `{a}`");
            }
        }
    }

    /// Disjointness proved by behaviour rather than by name: applying a material and a rig in
    /// either order must give the same bytes. This is what lets the console change one without
    /// re-deriving the other.
    #[test]
    fn material_and_rig_commute() {
        for m in MATERIAL_NAMES {
            for r in RIG_NAMES {
                let mut a = substrate();
                apply_material(&mut a, m);
                apply_rig(&mut a, r);

                let mut b = substrate();
                apply_rig(&mut b, r);
                apply_material(&mut b, m);

                assert_eq!(
                    bytemuck::bytes_of(&a),
                    bytemuck::bytes_of(&b),
                    "`{m}` + `{r}` must not depend on the order they were applied"
                );
            }
        }
    }

    // -----------------------------------------------------------------------------
    // (c) idempotence, totality, and the unknown-name contract
    // -----------------------------------------------------------------------------

    #[test]
    fn apply_is_idempotent() {
        for name in MATERIAL_NAMES {
            let once = with_material(name);
            let mut twice = with_material(name);
            apply_material(&mut twice, name);
            assert_eq!(bytemuck::bytes_of(&twice), bytemuck::bytes_of(&once), "material `{name}`");
        }
        for name in RIG_NAMES {
            let once = with_rig(name);
            let mut twice = with_rig(name);
            apply_rig(&mut twice, name);
            assert_eq!(bytemuck::bytes_of(&twice), bytemuck::bytes_of(&once), "rig `{name}`");
        }
    }

    /// The property the "total over the block" paragraph claims: **every ordered pair** of
    /// materials converges. Switching from a material with an overlay to one without must
    /// clear the overlay, not inherit it — `graphite → slate` is the concrete case, and this
    /// covers all 16.
    #[test]
    fn switching_between_any_two_materials_converges() {
        for from in MATERIAL_NAMES {
            for to in MATERIAL_NAMES {
                let mut s = with_material(from);
                apply_material(&mut s, to);
                assert_eq!(
                    bytemuck::bytes_of(&s),
                    bytemuck::bytes_of(&with_material(to)),
                    "`{from}` → `{to}` did not converge: a lane was inherited"
                );
            }
        }
        for from in RIG_NAMES {
            for to in RIG_NAMES {
                let mut s = with_rig(from);
                apply_rig(&mut s, to);
                assert_eq!(
                    bytemuck::bytes_of(&s),
                    bytemuck::bytes_of(&with_rig(to)),
                    "rig `{from}` → `{to}` did not converge"
                );
            }
        }
    }

    #[test]
    fn an_unknown_name_touches_nothing() {
        for bad in ["", " ", "slat", "graphit3", "chrome", "none", "off", "studio"] {
            let before = substrate();
            let mut s = substrate();
            assert!(!apply_material(&mut s, bad), "`{bad}` must not be a material");
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&before), "material `{bad}`");
        }
        for bad in ["", "studi0", "sunset", "graphite"] {
            let before = substrate();
            let mut s = substrate();
            assert!(!apply_rig(&mut s, bad), "`{bad}` must not be a rig");
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&before), "rig `{bad}`");
        }
    }

    /// Case-insensitive, per `console_main.rs:96`'s precedent for the backdrop selector.
    #[test]
    fn names_match_case_insensitively() {
        for name in MATERIAL_NAMES {
            let mut s = substrate();
            assert!(apply_material(&mut s, &name.to_uppercase()));
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&with_material(name)));
        }
        for name in RIG_NAMES {
            let mut s = substrate();
            assert!(apply_rig(&mut s, &name.to_uppercase()));
            assert_eq!(bytemuck::bytes_of(&s), bytemuck::bytes_of(&with_rig(name)));
        }
    }

    // -----------------------------------------------------------------------------
    // (d) the name tables, and that no two names are the same look
    // -----------------------------------------------------------------------------

    /// Every advertised name applies, **and no two names are the same picture.** The second
    /// half is the one that earns its place: a copy-paste that left two materials identical
    /// would ship four names and three looks, and nothing else here would notice.
    #[test]
    fn every_material_name_applies_and_is_a_distinct_look() {
        assert_eq!(MATERIAL_NAMES.len(), MATERIALS.len());
        for (name, m) in MATERIAL_NAMES.iter().zip(MATERIALS.iter()) {
            assert_eq!(name, &m.name, "MATERIAL_NAMES must be in MATERIALS order");
        }
        for (i, a) in MATERIAL_NAMES.iter().enumerate() {
            let sa = with_material(a);
            for b in MATERIAL_NAMES.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate material name `{a}`");
                assert_ne!(
                    bytemuck::bytes_of(&sa),
                    bytemuck::bytes_of(&with_material(b)),
                    "`{a}` and `{b}` are byte-identical — two names, one look"
                );
            }
        }
    }

    #[test]
    fn every_rig_name_applies_and_is_a_distinct_look() {
        assert_eq!(RIG_NAMES.len(), RIGS.len());
        for (name, r) in RIG_NAMES.iter().zip(RIGS.iter()) {
            assert_eq!(name, &r.name, "RIG_NAMES must be in RIGS order");
        }
        for (i, a) in RIG_NAMES.iter().enumerate() {
            let sa = with_rig(a);
            for b in RIG_NAMES.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate rig name `{a}`");
                assert_ne!(
                    bytemuck::bytes_of(&sa),
                    bytemuck::bytes_of(&with_rig(b)),
                    "`{a}` and `{b}` are byte-identical — two names, one rig"
                );
            }
        }
    }

    // -----------------------------------------------------------------------------
    // (e) the ranges — R6's "fourth way to lie", closed
    // -----------------------------------------------------------------------------

    /// Every value every material writes, against its `params.rs` range. R6 found three
    /// hand-maintained range tables already disagreeing with `params.rs` on 9 of 45 ids; this
    /// is the assertion that stops a scene builder becoming the fourth.
    #[test]
    fn every_material_value_is_inside_its_declared_range() {
        // (name, lo, hi) per material_layer slot — param_table.rs:1220-1241 order,
        // params.rs:8430-8446 ranges. `[16]`/`[17]` are checked separately.
        let layer_ranges: [(&str, f32, f32); 16] = [
            ("noise", 0.0, 15.0),        // MatNoise, 16 variants (params.rs:1494-1511)
            ("channel", 0.0, 4.0),       // MatChannel 0..5; 5 (Emissive) has no slot
            ("scale", 0.25, 64.0),       // params.rs:8432
            ("rotation", 0.0, std::f32::consts::TAU), // :8433
            ("offset_x", -8.0, 8.0),     // :8434
            ("offset_y", -8.0, 8.0),     // :8435
            ("octaves", 1.0, 8.0),       // :8436
            ("lacunarity", 1.2, 4.0),    // :8437
            ("gain", 0.1, 0.9),          // :8438
            ("warp", 0.0, 2.0),          // :8439
            ("contrast", 0.1, 4.0),      // :8440
            ("gamma", 0.2, 4.0),         // :8441
            ("remap_lo", 0.0, 1.0),      // :8442
            ("remap_hi", 0.0, 1.0),      // :8443
            ("invert", 0.0, 1.0),        // :8444
            ("seed", 0.0, 64.0),         // :8445
        ];
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let check = |what: &str, v: f32, lo: f32, hi: f32| {
                assert!(v >= lo && v <= hi, "{name}: {what} = {v} outside {lo}..{hi}");
            };
            check("material[1] projection", s.material[1], 0.0, 2.0); // params.rs:1450-1457
            check("material[2] scale", s.material[2], 0.02, 16.0); // :8425
            for (i, (what, lo, hi)) in layer_ranges.iter().enumerate() {
                check(&format!("material_layer[{i}] {what}"), s.material_layer[i], *lo, *hi);
                // The overlay's slots share the base's ranges (params.rs:8458-8473); its
                // values are inert while disabled but must still be legal.
                check(&format!("material_layer2[{i}] {what}"), s.material_layer2[i], *lo, *hi);
            }
            check("material_layer[17] bake px", s.material_layer[17], 64.0, 2048.0); // render.rs clamp
            check("material_layer2[17] blend", s.material_layer2[17], 0.0, 7.0); // params.rs:1626-1635
            for i in 0..8 {
                check(&format!("material_grad[{i}]"), s.material_grad[i], 0.0, 1.0); // :8447-8452
                check(&format!("material_grad2[{i}]"), s.material_grad2[i], 0.0, 1.0); // :8474-8479
            }
            check("derive[3] normal strength", s.material_derive[3], 0.0, 4.0); // :8485
            check("derive[4] ao strength", s.material_derive[4], 0.0, 1.0); // :8486
            check("derive[5] ao radius", s.material_derive[5], 1.0, 8.0); // :8487
            check("material_live[1] anim speed", s.material_live[1], 0.0, 4.0); // :8492
            check("material_live[3] flow x", s.material_live[3], -1.0, 1.0); // :8494
            check("material_live[4] flow y", s.material_live[4], -1.0, 1.0); // :8495
            check("material_live[5] displace", s.material_live[5], 0.0, 2.0); // :8496
            check("lighting[7] material type", s.lighting[7], 0.0, 7.0); // params.rs:3701-3718
            check("pbr[0] metallic", s.pbr[0], 0.0, 1.0); // :8722
            check("pbr[1] roughness", s.pbr[1], 0.0, 1.0); // :8723
            check("aniso[0] amount", s.aniso[0], -1.0, 1.0); // :8689
            check("aniso[1] rotation", s.aniso[1], 0.0, 360.0); // :8690
            check("aniso[3] blend", s.aniso[3], 0.0, 1.0); // :8692
            check("matcol[0] hue", s.matcol[0], 0.0, 1.0); // :8892
            check("matcol[1] hue cycle", s.matcol[1], -2.0, 2.0); // :8893
            check("matcol[2] saturation", s.matcol[2], 0.0, 1.0); // :8894
            check("matcol[3] value", s.matcol[3], 0.0, 1.0); // :8895
        }
    }

    #[test]
    fn every_rig_value_is_inside_its_declared_range() {
        for name in RIG_NAMES {
            let s = with_rig(name);
            let check = |what: &str, v: f32, lo: f32, hi: f32| {
                assert!(v >= lo && v <= hi, "{name}: {what} = {v} outside {lo}..{hi}");
            };
            check("lighting[0] ambient", s.lighting[0], 0.0, 3.0); // params.rs:8550
            check("lighting[1] key", s.lighting[1], 0.0, 6.0); // :8551
            check("lighting[2] fill", s.lighting[2], 0.0, 3.0); // :8552
            check("pbr[2] exposure EV", s.pbr[2], -8.0, 4.0); // :8724
            check("pbr[3] env intensity", s.pbr[3], 0.0, 4.0); // :8725
        }
    }

    // -----------------------------------------------------------------------------
    // (f) the claims the doc comments make about the engine, restated as the engine
    //     evaluates them
    // -----------------------------------------------------------------------------

    /// `world.rs:10850-10852`: the material uniform turns on from `material_layer[16]` alone,
    /// so no material needs to touch `material[0]` — and none does. If that OR ever becomes
    /// an AND, four materials go inert at once and this is the only place that says so.
    #[test]
    fn the_material_uniform_turns_on_from_the_procedural_flag_alone() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let mtl_x = s.material[0] > 0.5 || s.material_layer[16] > 0.5; // world.rs:10852
            assert!(mtl_x, "{name}: Uniforms.mtl.x must be on");
            assert_eq!(s.material[0], 0.0, "{name}: the PNG master must stay untouched");
        }
    }

    /// `render.rs`'s `present_mask` + `material_bake.wgsl:382`, restated: which channel bits
    /// each material actually lands, and — the part worth pinning — that **derived AO is only
    /// ever asked for over a baked Height layer**.
    #[test]
    fn the_present_mask_each_material_lands_is_what_it_claims() {
        // MatChannel → present bit (render.rs's `channel_slot`).
        let bit = |ch: f32| -> u32 {
            match ch as u32 {
                0 => 1,  // albedo
                1 => 4,  // roughness
                2 => 8,  // metallic
                3 => 32, // height
                4 => 16, // AO
                _ => 0,  // emissive: no bound slot
            }
        };
        let mask_of = |s: &Shared| -> u32 {
            let mut m = bit(s.material_layer[1]);
            if s.material_layer2[16] > 0.5 {
                m |= bit(s.material_layer2[1]);
            }
            if s.material_derive[0] > 0.5 {
                m |= 2; // derived normal
            }
            if s.material_derive[1] > 0.5 {
                m |= 16; // derived AO
            }
            m
        };
        assert_eq!(mask_of(&with_material("slate")), 4, "slate: roughness only");
        assert_eq!(mask_of(&with_material("graphite")), 1 | 4, "graphite: albedo + roughness");
        assert_eq!(mask_of(&with_material("paper")), 1 | 32 | 2, "paper: albedo + height + derived normal");
        assert_eq!(mask_of(&with_material("metal")), 1 | 4, "metal: albedo + roughness");

        // The pairing nothing in `Shared` enforces (see `derive`'s warning): the AO derive
        // pass reads the HEIGHT slot unconditionally, so asking for AO without baking height
        // derives cavities from a texture this material never wrote.
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            if s.material_derive[1] > 0.5 {
                let bakes_height = s.material_layer[1] == 3.0
                    || (s.material_layer2[16] > 0.5 && s.material_layer2[1] == 3.0);
                assert!(bakes_height, "{name}: derived AO without a baked Height layer");
            }
        }
    }

    /// `cube.wgsl:1491-1497`: the two routes into the anisotropic lobe are exclusive, and each
    /// material takes exactly one. Restated because "set both and hope" is the silent failure.
    #[test]
    fn each_material_takes_exactly_one_route_into_the_anisotropic_lobe() {
        for name in MATERIAL_NAMES {
            let s = with_material(name);
            let is_type = s.lighting[7] == 4.0;
            let overlay_on = s.aniso[2] > 0.5;
            assert!(!(is_type && overlay_on), "{name}: type 4 ignores the overlay — do not set both");
            // The effective amount, exactly as the shader resolves it.
            let amt = if is_type {
                s.aniso[0].clamp(-1.0, 1.0)
            } else if overlay_on {
                s.aniso[0].clamp(-1.0, 1.0) * s.aniso[3].clamp(0.0, 1.0)
            } else {
                0.0
            };
            match name {
                "metal" => assert_eq!(amt, 0.75, "metal: the raw amount, via MaterialType"),
                "graphite" => assert!((amt - 0.2925).abs() < 1e-6, "graphite: amount × blend, via the overlay"),
                _ => assert_eq!(amt, 0.0, "{name} must be isotropic"),
            }
        }
    }
}

//! **The substrate look** — one flat, still, lit plane, expressed as nothing but writes
//! into an existing `Shared` snapshot.
//!
//! Console Spike Tier 1 Leaf B; `doc/console_spike_as_built_brief.md` **R5**. R5's finding
//! is the whole shape of this file: the plane is reachable *today*, through
//! `RenderPath::Membrane` + `cube.wgsl` + the existing lighting/composite rig, with **no
//! new shader and no new `Shared` field**. Organon Shell already owns and republishes a
//! full `Shared` every redraw (`shell_main.rs:425-427`), so the substrate "scene" is a
//! **params builder, not a renderer feature** — and a params builder is a pure function
//! over bytes, testable headless, which is why it is a leaf at all.
//!
//! ```text
//! OrganicMathParams::default().to_shared()   →   apply_substrate_look(&mut s)
//!         (params.rs:9281)                              (this file)
//!                                            =   one flat, still, lit plane on world y = 0
//! ```
//!
//! # What this file does NOT own
//!
//! **The camera.** Not `cam_frame`, not distance, yaw, pitch or FOV — that rig is Tier 1
//! Leaf A's arithmetic and the integrator's arm in `world.rs`. The look and the camera
//! compose at integration and are deliberately separable. The one camera-adjacent field
//! written here is `camera` (the **auto-orbit path**, `ipc.rs:55-59`), which is a motion
//! clock rather than a rig, and which R5/R2 named as a stillness item.
//!
//! That separation is not cosmetic. R5's load-bearing warning: a perfectly flat plane +
//! a uniform material + a near-zero FOV yields **one constant colour across the whole
//! backdrop** — N, V and L are identical at every fragment, so Fresnel, the specular lobe
//! and the env variation all collapse. Tier 1 answers that from two sides, and this file
//! is only one of them:
//!
//! 1. **a real (narrow) FOV** — Leaf A's view-vector deviation *is* the shading gradient;
//! 2. **non-uniform albedo** — this file, via the per-vertex `mem_col` route (below);
//! 3. *normal variation* — blocked behind the `#472` material-map gate
//!    (`render.rs:3640-3654` forces `u.mtl[0] = 0` on every non-`Instanced` path, the
//!    Membrane path included). **That gate lift is Tier 2's**, not ours.
//!
//! # Stillness
//!
//! R5 recorded "which `Shared` toggles quiesce the auto-orbit and the Breath scene-scale
//! clocks" as **could not determine**. It is determined here, and every off-switch below
//! carries the `world.rs` line that consumes it. Three of the findings were not obvious:
//!
//! * **One free-running clock is already ticking at the stock defaults.** `bio[2]`
//!   (Ripple Speed) defaults to **0.3** (`params.rs:8781`) and `world.rs:2330` advances
//!   the ripple phase by it *every frame*, gated by neither `animate` nor `pulse`. It is
//!   inert today only because `bio[1]` (intensity) happens to be 0 — a coincidence of two
//!   defaults, not a guarantee. Both are written here.
//! * **`trans_amp` is a clock, not a shape dial.** `draw_membrane` builds its translation
//!   from `tf = apply_func(trans_func, angle)` (`math.rs:11820-11824`) — `angle` being the
//!   animation clock — and lays each node at `idx + tf·trans_amp·idx`
//!   (`math.rs:11899-11901`). Any non-zero `trans_amp` makes the lattice **pitch breathe**
//!   every frame. Zero collapses it to `idx`, which has no time in it at all.
//! * **`lighting[5]` (Glow) is the wash.** `emissive = albedo · (glow + mat_emissive)`
//!   (`cube.wgsl:1544`) — a flat self-emission proportional to albedo, i.e. exactly the
//!   uniform lift that made Tier 0's default frame read as "a near-uniform dark wash at
//!   rest". It defaults to 0.2. It is zeroed here.
//!
//! Anything that could **not** be stilled from `Shared` is reported to the integrator
//! rather than fudged; see `KNOWN_UNSTILLABLE` at the foot of this file.
//!
//! # Invariants honoured
//!
//! * **No `Shared` layout change** (the repo's second invariant). This function only ever
//!   *writes existing fields*; it adds, removes and reorders nothing, and needs no
//!   `LAYOUT_VERSION` bump.
//! * **Every value written lies inside its host parameter's declared range** in
//!   `params.rs`. The range is quoted beside each constant. This matters beyond tidiness:
//!   R6 found three hand-maintained range tables already disagreeing with `params.rs`, and
//!   a scene builder that writes out-of-range bytes would be a fourth way to lie.
//! * `seq` and `layout_version` are never touched — they belong to the publisher.

use crate::ipc::Shared;

// ===========================================================================
// Geometry — the membrane path
// ===========================================================================

/// `Shared.generator` = `GeneratorMode::Original` (`params.rs:7573` declares it the
/// default; the enum's first variant, so wire value 0).
///
/// Half of the membrane-mesh gate: `world.rs:2656-2657` builds the lofted sheet only when
/// `surface_mode == 4 && generator == Original && !membrane_arms`. Every other generator
/// either lofts from its own strands or has no nodes at all.
pub const SUBSTRATE_GENERATOR: u32 = 0;

/// `Shared.surface_mode` = `SurfaceMode::Membrane`. The enum is
/// `Original, FlowAligned, SweptTubes, Metaball, Membrane, …` (`params.rs:1302-1320`), so
/// Membrane is wire value **4** — the numeric encoding verified against the enum, not
/// assumed. Consumed at `world.rs:2334` (`membrane_mode`).
pub const SUBSTRATE_SURFACE_MODE: u32 = 4;

/// `Shared.origin_mode` = `OriginMode::Centered` (`params.rs:1424-1429`; Corner = 0,
/// Centered = 1). Read at `world.rs:2754` and threaded into `draw_membrane`'s `centered`.
///
/// This is a **stillness** choice as much as a layout one. `origin_offset`
/// (`math.rs:335-340`) recentres the lattice on the world origin, so the field's AABB
/// centre is exactly the origin — and `world.rs:5270` lerps `cam_center` 5 %/frame toward
/// that AABB centre with no way to switch it off from `Shared`. Centre the plane and the
/// auto-follow has nowhere to travel: it is a fixed point, not a drift. (R2 called this
/// "a coincidence to name, not lean on"; naming it is the point — see `KNOWN_UNSTILLABLE`.)
pub const SUBSTRATE_ORIGIN_MODE: u32 = 1;

/// Strand count across **x** — one strand per column, so this is the sheet's x tessellation
/// *and* its x extent at once.
///
/// **Why 128, and why the two are one number.** With `loop_count_q = 0`, `draw_membrane`
/// takes the column-strand branch: one strand per `(ix, iy)` running along z, laid on a
/// lattice whose pitch is a **hard-coded 1 world unit** (`math.rs:11884-11908` — the node
/// position starts from the raw loop index). Nothing in `Shared` scales that pitch
/// statically, so vertex density and world extent are a **single knob**: `nx` vertices
/// spanning `nx − 1` units. 128 is `loop_count_x`'s declared maximum
/// (`params.rs:7578`, `ilin(…, 20, 1, 128)`), which makes it simultaneously the densest
/// tessellation and the largest plane the membrane path can express in range — so the
/// choice is the ceiling rather than a taste call. Result: **16 384 vertices, 32 258
/// triangles, 127 × 127 world units.**
///
/// Enough vertices for smooth per-vertex shading with room to spare (the current shading
/// is *linear* in position, so interpolation is exact at any density — the density is
/// really banked for Tier 2, whose height map displaces **per vertex** in
/// `cube.wgsl::mat_displace_world`), and not absurd: this mesh is rebuilt CPU-side every
/// frame by `world.rs:2815`.
pub const SUBSTRATE_GRID_X: f32 = 128.0;

/// Strand count across **y** = 1. This is what makes it *one* sheet.
///
/// The strand grid is `(gx, gy, gz, along_z) = (nx, ny, 1, true)` (`math.rs:11886`), and
/// `loft_membrane` lofts one sheet per line of the woven axis (`math.rs:12176-12182`).
/// With `ny = 1` there is exactly one `(iy, iz)` line, hence exactly one quad sheet. Any
/// `ny > 1` would stack `ny` parallel sheets. 1 is in range (`params.rs:7579`, 1..128).
pub const SUBSTRATE_GRID_Y: f32 = 1.0;

/// Strand length along **z** — nodes per strand, so the sheet's z tessellation and extent.
/// Square with [`SUBSTRATE_GRID_X`] so the plane can be framed from any azimuth the
/// integrator's rig chooses. Range 1..128 (`params.rs:7580`).
pub const SUBSTRATE_GRID_Z: f32 = 128.0;

/// `loop_count[3]` (the q-strand count) = 0 — this **selects the flat branch**.
/// `cq > 0` takes the accumulating turtle-strand path (`math.rs:11855-11883`), which
/// compounds transforms with no reset and is the tentacle/helix machinery. `cq == 0` takes
/// the plain z-column branch (`math.rs:11884-11909`). Range 0..256 (`params.rs:7581`).
pub const SUBSTRATE_LOOP_Q: f32 = 0.0;

/// `Shared.membrane` = `[weave, show_strands, arms, close]` (`ipc.rs:157-166`).
///
/// * `[0] = 1` — `MembraneWeave::AlongX` (`params.rs:3612-3620`; Auto = 0, AlongX = 1).
///   Auto would also pick X here (it takes the valid axis with the most strands, and x is
///   the only valid one at `ny = gz = 1`), but naming the axis means the sheet's
///   orientation does not silently depend on which grid axis happens to be longest
///   (`math.rs:12144-12170`).
/// * `[1] = 0` — Show Strands off. On, `world.rs:2337` also draws the boundary strands as
///   swept tubes and forces `flow_aligned`/`tube` (`world.rs:2646-2651`): pipes on a plane.
/// * `[2] = 0` — Skin Arms off. On, `world.rs:2657` sets `draw_membrane_mesh = false` and
///   there is no sheet at all.
/// * `[3] = 0` — Close Seam off. There is no 360° wrap to bridge (`math.rs:12123-12142`).
pub const SUBSTRATE_MEMBRANE: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

/// `Shared.rot_amp` = `[x, y, z, continuous]`, all zero.
///
/// Rotation amplitude 0 makes `rot_deg` return 0 in **both** clock modes
/// (`math.rs:11825-11831`), so `compose_step` degenerates from `R·T` to a pure translation
/// (`math.rs:315-323`) and the lattice is rectilinear. `[3]` is the continuous-rotation
/// flag read at `world.rs:2323`; zero pins pendulum mode so the choice of phase clock is
/// not left to a field that no longer matters. Range 0..2160 (`params.rs:7584-7586`);
/// already the default, pinned so the plane does not depend on that staying true.
pub const SUBSTRATE_ROT_AMP: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

/// `Shared.trans_amp` = 0 on every axis — **the stillness item hiding in the geometry.**
///
/// `draw_membrane` computes `tf = apply_func(trans_func, angle)` from the *animation
/// clock* (`math.rs:11820-11824`) and places each node at `idx + tf·trans_amp·idx`
/// (`math.rs:11899-11901`). Non-zero `trans_amp` therefore makes the lattice pitch
/// oscillate with `angle` — the plane breathes in and out of the frame. Zero removes
/// `angle` from the position expression entirely, which is the difference between "the
/// clock is stopped" and "there is no clock". Range 0..20 (`params.rs:7597-7599`).
pub const SUBSTRATE_TRANS_AMP: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

/// `Shared.trans_mod` = 0 on every axis — the additive lattice offset
/// (`math.rs:11899-11901`). Zero keeps the sheet on the world **y = 0** plane (with
/// `ny = 1` the y term is `ly + trans_mod.y`, and `ly` is 0 under Centered) and keeps the
/// AABB centred on the origin for the auto-follow. Range −200..200 (`params.rs:7600-7602`).
pub const SUBSTRATE_TRANS_MOD: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

// ===========================================================================
// Stillness — every time- and beat-driven modifier that could move the sheet
// ===========================================================================

/// `Shared.animate` = 0. Gates the animation clock itself: `world.rs:2289-2319` advances
/// `self.angle` and `self.wind_phase` only when set, and `world.rs:2576-2578` advances
/// `self.gen_phase` the same way.
///
/// Belt **and** braces with `SUBSTRATE_ROT_AMP` / `SUBSTRATE_TRANS_AMP`: either alone
/// freezes the sheet (one stops the clock, the other unhooks it), and the redundancy is
/// deliberate because they can be changed independently from the CLI override lane.
/// Defaults to **true** (`params.rs:8544`) — this is a real change, not a pin.
pub const SUBSTRATE_ANIMATE: u32 = 0;

/// `Shared.pulse` = 0. The highest-value single switch in this file, because the pulse
/// does not merely move things — `world.rs:2255-2260` runs `apply_mod(&mut s, …)`, which
/// **rewrites arbitrary fields of `s` itself** each frame. With the pulse on, a routing
/// slot could silently undo any look value set below. It also gates `drive`
/// (`world.rs:2266`), the sole input to Breath and Speed Pulse.
/// Already false (`params.rs:8559`); pinned for the reason above.
pub const SUBSTRATE_PULSE: u32 = 0;

/// `Shared.routing[1]` and `[3]` — the two pulse→param modulation **depths**
/// (`ipc.rs:62-67`), zeroed as the second lock on the door `SUBSTRATE_PULSE` closes.
/// `world.rs:2256-2259` multiplies each depth by the envelope, so depth 0 adds nothing
/// even if the pulse is turned back on from outside. The target ids (`[0]`, `[2]`) are
/// left alone: they select *what* would be modulated, and at depth 0 that is moot.
pub const SUBSTRATE_ROUTING_DEPTH: f32 = 0.0;

/// `Shared.breath[0]` (Breath **amount**) = 0 — the "Breath" scene scale R5 flagged.
///
/// Breath is a pulse-driven uniform scale of the whole scene applied at the *view* level,
/// so it swells every generator and surface mode alike: `world.rs:2279-2286` advances the
/// envelope and `world.rs:10607-10610` folds the resulting scale into `scene_view_proj`.
/// Amount 0 makes `breath_scale_vec` return `Vec3::ONE` unconditionally
/// (`world.rs:11333-11334`). `breath[1..3]` are attack/decay time constants and are left
/// alone — zeroing a time constant would be a lie about what "off" means here.
/// Range 0..3 (`params.rs:8676`).
pub const SUBSTRATE_BREATH_AMOUNT: f32 = 0.0;

/// `Shared.camera[0]` (auto-orbit path id) = 0 = **Off** (`ipc.rs:55-59`).
///
/// R2's claim **verified**: `cam_path` already defaults to `CamPath::Off`
/// (`params.rs:8603`), so this is a pin rather than a change. Path 0 leaves `cam_phase` /
/// `cam_vel` unintegrated (`world.rs:11446-11458`), i.e. manual drag only. Pinned because
/// an orbiting substrate is the single most obvious way for this plane to stop being still,
/// and "it happens to default off" is not a guarantee.
pub const SUBSTRATE_CAMERA_PATH: f32 = 0.0;

/// `Shared.bio[0..3]` = 0 — the three free-running bioluminescence clocks
/// (`ipc.rs:145-156`), which R5 could not determine and which are named individually here:
///
/// * `[0]` Colour Cycle — advances `self.color_phase` every frame off wall time
///   (`world.rs:2329`), flowing the palette sweep along the sheet.
/// * `[1]` Ripple Intensity — the master for the travelling HDR emissive band; it reaches
///   the shader as `Uniforms.ripple.x` (`world.rs:10682`) and is the one that makes the
///   ripple visible at all.
/// * `[2]` Ripple Speed — advances `self.ripple_phase` (`world.rs:2330`).
///
/// 🚨 **`[2]` is a live clock at the stock defaults**: `ripple_speed` defaults to **0.3**
/// (`params.rs:8781`), and `world.rs:2330` advances `self.ripple_phase` by it *every
/// frame, unconditionally* — not gated by `animate`, not gated by `pulse`. It is inert
/// today only because `[1]` (intensity) happens to be 0, so the phase feeds nothing. That
/// is a coincidence of two defaults, not a guarantee, and it is exactly the class of
/// free-running clock R5 recorded as "could not determine". Both are written here.
/// `[0]` and `[1]` do default to 0 (`params.rs:8779-8780`) and are pinned.
///
/// `bio[3..]` (freq / sharpness / geometry) are *shape* dials, inert at intensity 0, and
/// are left alone — `world.rs:10682` even clamps `bio[4]` with `.max(1.0)`, so writing 0
/// there would express nothing.
pub const SUBSTRATE_BIO_CLOCK: f32 = 0.0;

/// `Shared.rd[3]` (intensity) and `rd[4]` (albedo mix) = 0 — the Gray–Scott
/// reaction–diffusion sim (`ipc.rs:167-172`), whose texture **crawls** and which the lit
/// shaders sample triplanar as emissive dapple and as pigment carved into the albedo
/// (`cube.wgsl:1465` and the `rd_d` mix inside `cube.wgsl:1479`). Both default to 0
/// (`params.rs:8794`, `:8798`); pinned. `rd[0..3]` (feed/kill/scale) are pattern shape and
/// are left alone.
pub const SUBSTRATE_RD_OFF: f32 = 0.0;

// ===========================================================================
// Albedo — neutral slate, via the per-vertex `mem_col` route
// ===========================================================================

/// `Shared.surface_fx[6]` = `Palette::Native` (0) — the palette id, which lives in that
/// slot despite `ipc.rs:72-79` documenting `[6]` as reserved (`param_table.rs:295-306`
/// is the authority; `world.rs:2345` is the consumer).
///
/// **Native is what produces the albedo variation R5 requires.** `push_sheet` colours each
/// vertex by its normalized position inside the field bounds
/// (`math.rs:12257-12262`: `c = (p − bmin) / span`), so on a y-flat sheet the per-vertex
/// colour is `(u, 0, v)` with `u, v ∈ [0, 1]` — a linear ramp across the plane rather than
/// one constant value. An explicit LUT would instead sweep a *palette* along the strand,
/// which desaturates to a much wider luma swing.
///
/// It also keeps `Uniforms.amb.w` at 0 (`world.rs:10671`), though the membrane does not
/// actually depend on that: its identity instance carries `tint.w = 0`, which opts it out
/// of the palette-replaces-colour path so the per-vertex colour is used either way
/// (`cube.wgsl:675-680`, `render.rs:1912-1918`). Default 0 (`params.rs:8737`); pinned.
pub const SUBSTRATE_PALETTE: f32 = 0.0;

/// `Shared.matcol[0..4]` = the **generator** material's `[hue, hue_cycle, saturation,
/// value]` (`ipc.rs:1263-1270`), reaching the shader as `Uniforms.matcol` with the hue
/// resolved to `matcol[0] + matcol[1]·beat` (`world.rs:10770`) and applied by
/// `apply_hsv` (`cube.wgsl:167-186`, called at `:1479`).
///
/// * `[0] = 0` hue — no rotation of a colour we are about to discard anyway.
/// * `[1] = 0` **hue cycle** — a stillness item, and the only beat-driven term left in the
///   plane's own albedo: non-zero and the slate crawls around the colour wheel at
///   `tempo` BPM forever. The beat clock itself cannot be stopped from `Shared` (see
///   `KNOWN_UNSTILLABLE`), so this is the switch that matters.
/// * `[2] = 0` **saturation** — the neutral-slate move. `apply_hsv` lerps toward luma at
///   saturation 0 (`cube.wgsl:182-183`), turning the position ramp above into an
///   achromatic one: `luma = 0.2126·u + 0.0722·v ∈ [0, 0.285]`, mean ≈ 0.142 — a slate's
///   reflectance, arrived at physically rather than dialled in. (The green term drops out
///   because the sheet is flat in y, so `c.y` is identically 0.) Defaults to **1.0**
///   (`params.rs:8894`) — a real change.
/// * `[3] = 1` value — identity; it can only lower (range 0..1, `params.rs:8895`) and
///   lowering it would only darken the ramp computed above.
///
/// `matcol[4..8]` is the **scenery** material and is not ours; scenery is off
/// (`params.rs:8205`, `SceneryMode::None`).
pub const SUBSTRATE_MATCOL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

// ===========================================================================
// The lighting rig
// ===========================================================================
//
// One rig: key + fill + ambient + env intensity/rotation + exposure. Decoded from
// `Shared.lighting[8]` and `Shared.pbr[8]` at `world.rs:10612-10641`.
//
// Two structural facts about this rig shape the numbers below, and both are engine
// behaviour rather than taste:
//
// * **The fill's direction is derived, not settable.** `world.rs:10637` builds it as
//   `dir_from_angles(10.0, azimuth − 120.0)` — fixed 10° elevation, fixed 120° off the
//   key. On a horizontal plane that means `N·L_fill = sin(10°) = 0.174`: the fill grazes,
//   and only its *intensity* is ours to choose. "Aim the fill" is not a thing here.
// * **Ambient and env intensity are only ever used as a product.** `ambient_mul` and
//   `env_intensity` appear multiplied together at every env site in the shader
//   (`cube.wgsl:1553`, `:1587`, `:1687`, `:1890`). Setting both would double-dim, so
//   `lighting[0]` is pinned at unity and `pbr[3]` is the single env-level knob.
//
// And one that shapes what the rig can *do*: the plane's normal is `+y` (well, `−y` as
// built — see `push_sheet`'s winding — flipped toward the viewer by the double-sided
// lighting line at `cube.wgsl:1382-1385`, which exists for exactly this geometry). A
// single constant normal means `N·L` is **constant across the entire sheet** for both
// analytic lights. The lights therefore set the plane's overall level and the placement of
// the specular gradient; they cannot draw a shape on it. What varies across the frame is
// the albedo ramp above and the view vector — which is Leaf A's FOV.

/// `lighting[0]` — Ambient (the IBL multiplier), pinned at unity so the environment level
/// is set in exactly one place. See the product note above. Range 0..3
/// (`params.rs:8550`); this is the default.
pub const SUBSTRATE_AMBIENT: f32 = 1.0;

/// `lighting[1]` — Key intensity. Moderate-to-strong, so the direction of the light reads
/// in the specular even though `N·L` is flat. Range 0..6 (`params.rs:8551`, default 2.2).
pub const SUBSTRATE_KEY_INTENSITY: f32 = 2.6;

/// `lighting[2]` — Fill intensity. Dim, and grazing by construction (see the rig note):
/// it lifts the side of the sheet the key's specular has left dark without flattening it.
/// Range 0..3 (`params.rs:8552`, default 0.6).
pub const SUBSTRATE_FILL_INTENSITY: f32 = 0.35;

/// `lighting[3]` — Key elevation, degrees above the horizon. `N·L = sin(42°) = 0.669` on
/// a horizontal plane: clearly lit from above without being top-down (which would kill the
/// specular gradient the narrow FOV is there to produce). Range −90..90
/// (`params.rs:8553`, default 35).
pub const SUBSTRATE_KEY_ELEVATION_DEG: f32 = 42.0;

/// `lighting[4]` — Key azimuth, degrees. `dir_from_angles`
/// (`world.rs:10288-10292`) reads it as `(cos e · sin a, sin e, cos e · cos a)`, the
/// direction **to** the light, so a negative azimuth puts the key on the −x side.
///
/// −10° sits **50° counter-clockwise of the stock camera yaw** (`yaw: 0.7` rad ≈ 40.1° at
/// `world.rs:1745`, which `world.rs:10579` turns into the eye direction), which is what
/// makes it read as *above-left* from that eye.
///
/// 🚨 **This constant is coupled to the camera the integrator installs.** The rig arm in
/// `world.rs` is Leaf A's and the integrator's; if their yaw differs from the stock 0.7
/// rad, rotate this by the same delta and the whole rig follows for free — the fill is
/// derived at `azimuth − 120°` and moves with it. Range −180..180 (`params.rs:8554`,
/// default 40).
pub const SUBSTRATE_KEY_AZIMUTH_DEG: f32 = -10.0;

/// `lighting[5]` — **Glow = 0.** `cube.wgsl:1544` computes
/// `emissive = albedo · (glow + mat_emissive) + …`: glow is a flat self-emission
/// proportional to albedo, added on top of every lit term. It defaults to 0.2
/// (`params.rs:8555`), and it is precisely the uniform lift that produces the
/// "near-uniform dark wash at rest" the Tier 0 record describes. A substrate that is meant
/// to be *lit* must not also be emitting. Range 0..2.
pub const SUBSTRATE_GLOW: f32 = 0.0;

/// `lighting[6]` — Opacity, full. The sheet is a thin two-sided surface
/// (`cube.wgsl:1382-1385`); anything below 1 makes the console backdrop translucent onto
/// whatever is behind it, which under `SUBSTRATE_BG_VISIBLE` is black. Range 0..1
/// (`params.rs:8557`); this is the default.
pub const SUBSTRATE_OPACITY: f32 = 1.0;

/// `lighting[7]` — Material type = `MaterialType::Standard` (`params.rs:3701-3703`, the
/// first variant, wire value 0), decoded at `world.rs:10640`.
///
/// **Tier 1 ships Standard on purpose.** The map-driven materials R5 names — graphite,
/// paper, slate — are `#472`'s procedural channel maps, and those cannot be sampled on
/// this path until `render.rs:3640-3654`'s `u.mtl[0] = 0` gate is lifted for the membrane.
/// That lift is Tier 2's gate work, and pretending otherwise here would produce a
/// parameter that exists and does nothing.
pub const SUBSTRATE_MATERIAL_TYPE: f32 = 0.0;

/// `pbr[0]` — Metallic = 0. A dielectric slab; a metallic slate is not a thing, and a
/// metal's albedo-tinted specular would fight the neutral ramp. Range 0..1
/// (`params.rs:8722`); this is the default. Decoded at `world.rs:10612`.
pub const SUBSTRATE_METALLIC: f32 = 0.0;

/// `pbr[1]` — Roughness. Slightly rougher than stock: on a plane the specular lobe is one
/// broad gradient across the whole frame (N constant, V varying only with the FOV), so
/// widening the lobe turns it into a sheen over the sheet rather than a hot spot in one
/// corner. Range 0..1 (`params.rs:8723`, default 0.35). Decoded at `world.rs:10613`.
pub const SUBSTRATE_ROUGHNESS: f32 = 0.42;

/// `pbr[2]` — Exposure in **EV stops**; `world.rs:10617` takes `2^pbr[2]`, so 0 is unity
/// gain and the composite's tonemap sees the scene as lit. Range −8..4
/// (`params.rs:8724`); this is the default.
pub const SUBSTRATE_EXPOSURE_EV: f32 = 0.0;

/// `pbr[3]` — Environment intensity: the single env-level knob (see the product note
/// above). Modest, because the default environment here is a real Nishita sky baked into
/// the IBL — `atmos_enabled` defaults **true** (`params.rs:9040`) and `world.rs:6694`
/// selects `EnvReq::Atmosphere` — and an upward-facing plane under a full sky collects a
/// great deal of irradiance. Range 0..4 (`params.rs:8725`, default 1.0). Decoded at
/// `world.rs:10618`.
pub const SUBSTRATE_ENV_INTENSITY: f32 = 0.65;

/// `pbr[4]` — Environment rotation, degrees (`world.rs:10619` converts to radians). Left
/// at 0: the baked atmosphere already places its sun at the terrain sun azimuth, 145°
/// (`params.rs:8913`), and rotating the env would slide the sky's bright quarter out from
/// under the key for no gain. Range 0..360 (`params.rs:8726`); this is the default.
pub const SUBSTRATE_ENV_ROTATION_DEG: f32 = 0.0;

/// `pbr[5]` — Bloom intensity = 0. Nothing in this scene is an emitter (see
/// [`SUBSTRATE_GLOW`]), so bloom has nothing legitimate to bleed; what it *would* do is
/// put a halo behind terminal glyphs, which is a legibility cost with no upside on a
/// backdrop. Range 0..1 (`params.rs:8727`, default 0.08). Decoded at `world.rs:10620`.
/// `pbr[6]` (threshold) is left alone — moot at intensity 0.
pub const SUBSTRATE_BLOOM: f32 = 0.0;

/// `Shared.bg_visible` = 0. The skybox stops painting; **the IBL is unaffected**
/// (`ipc.rs:105-106`, and `world.rs:10647` gates only `bg_brightness` on it). So the plane
/// is still lit by the sky — it simply is not backed by one.
///
/// The plane is meant to fill the frame; wherever it does not, black is the right
/// neighbour for a terminal, and the scrim floor (`SCRIM_FLOOR = 96`, `term_view.rs:210-219`)
/// only darkens by so much. This also takes the sky's own tonemap (`bg_tonemap`) and the
/// starfield/day-cycle clocks out of the frame entirely. Defaults **true**
/// (`params.rs:8890`) — a real change.
pub const SUBSTRATE_BG_VISIBLE: u32 = 0;

/// Everything the substrate look cannot express or cannot still from `Shared` alone —
/// **integrator items**, recorded rather than fudged. Each is inert or harmless today; the
/// point is that none of them is guaranteed by anything this file writes.
///
/// 1. **`self.cam_center` auto-follow** (`world.rs:5270`) — lerps 5 %/frame toward the
///    field AABB centre with no `Shared` switch. Neutralised *geometrically* here by
///    [`SUBSTRATE_ORIGIN_MODE`]: a plane centred on the origin makes the origin a fixed
///    point. If the integrator ever offsets the plane, this starts drifting again; R2's
///    "latch it off" is the durable fix and lives in the `world.rs` rig arm.
/// 2. **The beat clock cannot be stopped.** `advance_beat_clock` (`world.rs:11407-11437`)
///    advances `beat_pos` every frame at `active_bpm`, ungated by `animate` or `pulse`,
///    and `tempo`'s range floor is 40 BPM (`params.rs:8561`) so it cannot even be dialled
///    to zero. Every *consumer* that would touch this plane is switched off individually
///    above (`matcol[1]`, and the pulse-gated set); the clock itself keeps running.
/// 3. **The SDR output dither is per-frame and unconditional.** `composite.wgsl:426-433`
///    adds ±½ LSB of triangular noise keyed on a frame counter whenever the output is SDR.
///    No `Shared` field disables it. At ±1/510 it is sub-visible, and it is *wanted* here:
///    this plane is a slow gradient, exactly the banding case that dither exists for.
/// 4. **The lattice pitch is fixed at 1 world unit**, so extent and vertex count are one
///    knob (see [`SUBSTRATE_GRID_X`]). The plane is **127 × 127 world units**; the
///    integrator's camera distance has to frame that, and the plane cannot be made larger
///    without writing `loop_count` outside its declared 1..128 range. (`trans_amp` would
///    scale the pitch, but only as a function of the animation clock's phase — see
///    [`SUBSTRATE_TRANS_AMP`] — which is a clock, not a scale.)
/// 5. **TAA jitter is a per-frame view-proj change, and a deliberate non-item.**
///    `world.rs:7934-7943` rides a Halton-(2,3) sub-pixel offset on `view_proj` while
///    `temporal[0]` is on. It is off at the default (`taa_enabled` false,
///    `params.rs:8826`) so nothing is written here — but if the integrator turns TAA on
///    for image quality, note that on a *static* scene the jitter is not motion: the
///    history integrates true supersamples and converges to a cleanly anti-aliased still,
///    which is the best case for it. Left to the integrator on purpose; it is a
///    per-display quality setting, not a look (`ipc.rs:510-518`).
/// 6. **`ipc.rs:378`'s "Default off → image unchanged" for `atmosphere` is stale.**
///    `atmos_enabled` defaults **true** (`params.rs:9040`), and `world.rs:6623` says so in
///    prose: the physical atmosphere is the default environment. Harmless — it is exactly
///    the environment this rig wants, and its day cycle is off (`terrain_day_speed` = 0,
///    `params.rs:8920`, so `sun_elev_eff` is the static `terrain[4]`, `world.rs:6610-6618`)
///    — but the doc line will mislead the next reader.
pub const KNOWN_UNSTILLABLE: () = ();

/// Write the substrate look into `s`.
///
/// Pure: a total function of the fields it writes, with no dependence on the rest of `s`,
/// no I/O, no clock and no allocation. `OrganicMathParams::default().to_shared()` followed
/// by this call is a `Shared` snapshot under which the World renders **one flat, still,
/// lit plane filling the world x–z plane**.
///
/// Writes existing fields only — no `Shared` layout change, and `seq` / `layout_version`
/// are left to the publisher. Idempotent by construction (every write is a constant), and
/// tested as such. The exact field list is [`substrate_field_manifest`], which the tests
/// use to prove nothing outside it is touched.
pub fn apply_substrate_look(s: &mut Shared) {
    // --- geometry: the membrane path ------------------------------------------------
    s.generator = SUBSTRATE_GENERATOR;
    s.surface_mode = SUBSTRATE_SURFACE_MODE;
    s.origin_mode = SUBSTRATE_ORIGIN_MODE;
    s.loop_count = [
        SUBSTRATE_GRID_X,
        SUBSTRATE_GRID_Y,
        SUBSTRATE_GRID_Z,
        SUBSTRATE_LOOP_Q,
    ];
    s.membrane = SUBSTRATE_MEMBRANE;
    s.rot_amp = SUBSTRATE_ROT_AMP;
    s.trans_amp = SUBSTRATE_TRANS_AMP;
    s.trans_mod = SUBSTRATE_TRANS_MOD;

    // --- stillness ------------------------------------------------------------------
    s.animate = SUBSTRATE_ANIMATE;
    s.pulse = SUBSTRATE_PULSE;
    s.routing[1] = SUBSTRATE_ROUTING_DEPTH;
    s.routing[3] = SUBSTRATE_ROUTING_DEPTH;
    s.breath[0] = SUBSTRATE_BREATH_AMOUNT;
    s.camera[0] = SUBSTRATE_CAMERA_PATH;
    s.bio[0] = SUBSTRATE_BIO_CLOCK; // colour cycle
    s.bio[1] = SUBSTRATE_BIO_CLOCK; // ripple intensity
    s.bio[2] = SUBSTRATE_BIO_CLOCK; // ripple speed
    s.rd[3] = SUBSTRATE_RD_OFF; // RD emissive intensity
    s.rd[4] = SUBSTRATE_RD_OFF; // RD pigment into albedo

    // --- albedo ---------------------------------------------------------------------
    s.surface_fx[6] = SUBSTRATE_PALETTE;
    s.matcol[0] = SUBSTRATE_MATCOL[0];
    s.matcol[1] = SUBSTRATE_MATCOL[1];
    s.matcol[2] = SUBSTRATE_MATCOL[2];
    s.matcol[3] = SUBSTRATE_MATCOL[3];

    // --- the lighting rig -----------------------------------------------------------
    s.lighting = [
        SUBSTRATE_AMBIENT,
        SUBSTRATE_KEY_INTENSITY,
        SUBSTRATE_FILL_INTENSITY,
        SUBSTRATE_KEY_ELEVATION_DEG,
        SUBSTRATE_KEY_AZIMUTH_DEG,
        SUBSTRATE_GLOW,
        SUBSTRATE_OPACITY,
        SUBSTRATE_MATERIAL_TYPE,
    ];
    s.pbr[0] = SUBSTRATE_METALLIC;
    s.pbr[1] = SUBSTRATE_ROUGHNESS;
    s.pbr[2] = SUBSTRATE_EXPOSURE_EV;
    s.pbr[3] = SUBSTRATE_ENV_INTENSITY;
    s.pbr[4] = SUBSTRATE_ENV_ROTATION_DEG;
    s.pbr[5] = SUBSTRATE_BLOOM;
    s.bg_visible = SUBSTRATE_BG_VISIBLE;
}

/// Copy one named `Shared` field from a reference snapshot into a working one. The
/// manifest is a list of these, so it can name a field without restating its value.
pub type FieldRestore = fn(&mut Shared, &Shared);

/// The manifest: every `Shared` field [`apply_substrate_look`] writes, by **name**, paired
/// with a restore that copies that field back from a reference snapshot.
///
/// The restore direction is what makes this testable without restating a single value:
/// apply the look, restore every manifest field from the default, and the result must be
/// byte-identical to the default. Anything left over is a write this list does not
/// declare. Whole-array fields restore the whole array even when `apply` writes only some
/// lanes — deliberately, so the check stays conservative in the direction that matters
/// (it can never *hide* a stray write, only decline to pinpoint one within a declared
/// block).
static SUBSTRATE_FIELD_MANIFEST: &[(&str, FieldRestore)] = &[
    // geometry
    ("generator", |d, r| d.generator = r.generator),
    ("surface_mode", |d, r| d.surface_mode = r.surface_mode),
    ("origin_mode", |d, r| d.origin_mode = r.origin_mode),
    ("loop_count", |d, r| d.loop_count = r.loop_count),
    ("membrane", |d, r| d.membrane = r.membrane),
    ("rot_amp", |d, r| d.rot_amp = r.rot_amp),
    ("trans_amp", |d, r| d.trans_amp = r.trans_amp),
    ("trans_mod", |d, r| d.trans_mod = r.trans_mod),
    // stillness
    ("animate", |d, r| d.animate = r.animate),
    ("pulse", |d, r| d.pulse = r.pulse),
    ("routing", |d, r| d.routing = r.routing),
    ("breath", |d, r| d.breath = r.breath),
    ("camera", |d, r| d.camera = r.camera),
    ("bio", |d, r| d.bio = r.bio),
    ("rd", |d, r| d.rd = r.rd),
    // albedo
    ("surface_fx", |d, r| d.surface_fx = r.surface_fx),
    ("matcol", |d, r| d.matcol = r.matcol),
    // the lighting rig
    ("lighting", |d, r| d.lighting = r.lighting),
    ("pbr", |d, r| d.pbr = r.pbr),
    ("bg_visible", |d, r| d.bg_visible = r.bg_visible),
];

/// See [`SUBSTRATE_FIELD_MANIFEST`].
pub fn substrate_field_manifest() -> &'static [(&'static str, FieldRestore)] {
    SUBSTRATE_FIELD_MANIFEST
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math;
    use crate::params::{FuncName, OrganicMathParams, ParamValues};
    use glam::{DVec3, Vec3};

    /// The baseline every test starts from: the real host parameter set, with no host, no
    /// audio thread and no GPU (`params.rs:3982-3983`, `:9281`).
    fn default_shared() -> Shared {
        OrganicMathParams::default().to_shared()
    }

    fn applied() -> Shared {
        let mut s = default_shared();
        apply_substrate_look(&mut s);
        s
    }

    // -----------------------------------------------------------------------------
    // (a) every field this file claims to set, at its exact value
    // -----------------------------------------------------------------------------
    #[test]
    fn apply_sets_every_declared_field() {
        let s = applied();

        // geometry — the membrane path
        assert_eq!(s.generator, 0, "generator must be Original");
        assert_eq!(s.surface_mode, 4, "surface_mode must be Membrane");
        assert_eq!(s.origin_mode, 1, "origin_mode must be Centered");
        assert_eq!(s.loop_count, [128.0, 1.0, 128.0, 0.0]);
        assert_eq!(s.membrane, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.rot_amp, [0.0; 4]);
        assert_eq!(s.trans_amp, [0.0; 4]);
        assert_eq!(s.trans_mod, [0.0; 4]);

        // stillness
        assert_eq!(s.animate, 0, "the animation clock must not advance");
        assert_eq!(s.pulse, 0, "the pulse rewrites `s` itself — it must be off");
        assert_eq!(s.routing[1], 0.0);
        assert_eq!(s.routing[3], 0.0);
        assert_eq!(s.breath[0], 0.0, "Breath scene scale");
        assert_eq!(s.camera[0], 0.0, "auto-orbit path must be Off");
        assert_eq!(s.bio[0], 0.0, "colour-cycle clock");
        assert_eq!(s.bio[1], 0.0, "emissive ripple intensity");
        assert_eq!(s.bio[2], 0.0, "ripple-phase clock");
        assert_eq!(s.rd[3], 0.0);
        assert_eq!(s.rd[4], 0.0);

        // albedo
        assert_eq!(s.surface_fx[6], 0.0, "palette must be Native");
        assert_eq!(s.matcol[0], 0.0, "hue");
        assert_eq!(s.matcol[1], 0.0, "hue cycle — the beat-driven one");
        assert_eq!(s.matcol[2], 0.0, "saturation 0 = neutral slate");
        assert_eq!(s.matcol[3], 1.0, "value");

        // the lighting rig
        assert_eq!(s.lighting, [1.0, 2.6, 0.35, 42.0, -10.0, 0.0, 1.0, 0.0]);
        assert_eq!(s.pbr[0], 0.0, "metallic");
        assert_eq!(s.pbr[1], 0.42, "roughness");
        assert_eq!(s.pbr[2], 0.0, "exposure EV");
        assert_eq!(s.pbr[3], 0.65, "env intensity");
        assert_eq!(s.pbr[4], 0.0, "env rotation");
        assert_eq!(s.pbr[5], 0.0, "bloom");
        assert_eq!(s.bg_visible, 0);

        // never ours to touch
        let d = default_shared();
        assert_eq!(s.seq, d.seq, "`seq` belongs to the publisher");
        assert_eq!(s.layout_version, d.layout_version, "layout is append-only");
    }

    // -----------------------------------------------------------------------------
    // (b) the manifest: nothing outside the declared field list changes
    // -----------------------------------------------------------------------------
    #[test]
    fn apply_touches_only_the_manifest_fields() {
        let d = default_shared();
        let mut restored = applied();
        for (_, restore) in substrate_field_manifest() {
            restore(&mut restored, &d);
        }
        assert_eq!(
            bytemuck::bytes_of(&restored),
            bytemuck::bytes_of(&d),
            "apply_substrate_look wrote a field that substrate_field_manifest does not declare"
        );
    }

    /// The other direction: every declared field must actually be reachable, i.e. the
    /// manifest may not accumulate names for fields nobody writes any more. A manifest
    /// entry earns its place by being either a change or a deliberate pin, and both are
    /// covered by the value assertions above — so all this asserts is that the list is
    /// non-empty and names are unique (a duplicate would silently mask a stray write).
    #[test]
    fn manifest_names_are_unique() {
        let m = substrate_field_manifest();
        assert!(!m.is_empty());
        for (i, (a, _)) in m.iter().enumerate() {
            for (b, _) in m.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate manifest entry `{a}`");
            }
        }
    }

    /// Which manifest fields are real *changes* against today's defaults, and which are
    /// deliberate pins. Not a correctness claim — a tripwire: if a default moves so that a
    /// pin becomes a change (or vice versa), the reasoning in this file's doc comments
    /// needs re-reading, and this is where that shows up.
    #[test]
    fn changes_versus_pins_are_as_documented() {
        let d = default_shared();
        let s = applied();
        // Real changes against `OrganicMathParams::default()`.
        assert_ne!(s.surface_mode, d.surface_mode);
        assert_ne!(s.origin_mode, d.origin_mode);
        assert_ne!(s.loop_count, d.loop_count);
        assert_ne!(s.membrane, d.membrane);
        assert_ne!(s.animate, d.animate);
        assert_ne!(s.lighting, d.lighting);
        assert_ne!(s.pbr, d.pbr);
        assert_ne!(s.matcol, d.matcol);
        assert_ne!(s.bg_visible, d.bg_visible);
        // `bio` is a CHANGE, not a pin, and the reason is worth keeping in a test:
        // `ripple_speed` (`bio[2]`) defaults to 0.3 (`params.rs:8781`) and `world.rs:2330`
        // advances the ripple phase by it every frame, ungated. See SUBSTRATE_BIO_CLOCK.
        assert_ne!(s.bio, d.bio);
        // Deliberately an inequality, not `== 0.3`: the default round-trips through
        // nih-plug's normalization, so the exact bits are not ours to predict.
        assert!(d.bio[2] > 0.0, "the stock ripple-speed clock is running");
        // Deliberate pins: already correct at the default, written anyway.
        assert_eq!(s.generator, d.generator);
        assert_eq!(s.rot_amp, d.rot_amp);
        assert_eq!(s.trans_amp, d.trans_amp);
        assert_eq!(s.trans_mod, d.trans_mod);
        assert_eq!(s.pulse, d.pulse);
        assert_eq!(s.routing, d.routing);
        assert_eq!(s.breath, d.breath);
        assert_eq!(s.camera, d.camera, "R2's claim: auto-orbit already Off");
        assert_eq!(s.rd, d.rd);
        assert_eq!(s.surface_fx, d.surface_fx);
    }

    // -----------------------------------------------------------------------------
    // (c) idempotence
    // -----------------------------------------------------------------------------
    #[test]
    fn apply_is_idempotent() {
        let once = applied();
        let mut twice = applied();
        apply_substrate_look(&mut twice);
        assert_eq!(bytemuck::bytes_of(&twice), bytemuck::bytes_of(&once));
    }

    /// Stronger than idempotence: applying over an arbitrary *dirtied* snapshot lands on
    /// the same look, so the console can switch to the substrate from whatever the World
    /// happened to be showing. Only fields `apply` writes **whole** are compared — the
    /// partially-written blocks (`routing`, `breath`, `camera`, `bio`, `rd`,
    /// `surface_fx`, `matcol`, `pbr`) keep their other lanes on purpose, which is the
    /// point of writing lanes rather than blocks there.
    #[test]
    fn apply_overrides_a_dirty_snapshot() {
        let mut dirty = default_shared();
        dirty.generator = 2; // DNA
        dirty.surface_mode = 3; // Metaball
        dirty.origin_mode = 0; // Corner
        dirty.animate = 1;
        dirty.pulse = 1;
        dirty.bg_visible = 1;
        dirty.loop_count = [20.0, 20.0, 20.0, 6.0];
        dirty.membrane = [4.0, 1.0, 1.0, 1.0]; // Web weave, strands + arms + seam on
        dirty.rot_amp = [90.0, 45.0, 10.0, 1.0];
        dirty.trans_amp = [7.0, 7.0, 7.0, 0.0];
        dirty.trans_mod = [5.0, -5.0, 5.0, 0.0];
        dirty.lighting = [3.0; 8];
        apply_substrate_look(&mut dirty);

        let clean = applied();
        assert_eq!(dirty.generator, clean.generator);
        assert_eq!(dirty.surface_mode, clean.surface_mode);
        assert_eq!(dirty.origin_mode, clean.origin_mode);
        assert_eq!(dirty.animate, clean.animate);
        assert_eq!(dirty.pulse, clean.pulse);
        assert_eq!(dirty.bg_visible, clean.bg_visible);
        assert_eq!(dirty.loop_count, clean.loop_count);
        assert_eq!(dirty.membrane, clean.membrane);
        assert_eq!(dirty.rot_amp, clean.rot_amp);
        assert_eq!(dirty.trans_amp, clean.trans_amp);
        assert_eq!(dirty.trans_mod, clean.trans_mod);
        assert_eq!(dirty.lighting, clean.lighting);

        // And the geometry that comes out the far end is the same mesh.
        assert_eq!(
            build_sheet(&dirty, DVec3::ZERO).0,
            build_sheet(&clean, DVec3::ZERO).0,
            "a dirty snapshot must converge to the same sheet"
        );
    }

    // -----------------------------------------------------------------------------
    // (d) the membrane gate pair still holds after apply
    // -----------------------------------------------------------------------------
    /// `world.rs:2334` + `:2656-2657`: the lofted sheet is built only when
    /// `surface_mode == 4` **and** `generator == Original` **and** Skin-Arms is off. All
    /// three, restated exactly as the World evaluates them.
    #[test]
    fn membrane_gate_holds() {
        let s = applied();
        let membrane_mode = s.surface_mode == 4; // world.rs:2334
        let membrane_arms = membrane_mode && s.membrane[2] > 0.5; // world.rs:2341
        let draw_membrane_mesh = membrane_mode && s.generator == 0 && !membrane_arms;
        assert!(draw_membrane_mesh, "the membrane mesh gate must be open");

        // The competing render paths ahead of Membrane in `world.rs:7993-8026` are all
        // generator- or surface-mode-driven; at Original + Membrane none can win.
        assert_ne!(s.surface_mode, 3, "Metaball would take the path");
        assert_ne!(s.surface_mode, 5, "Voxel would take the path");
        assert_ne!(s.surface_mode, 6, "Volume would take the path");
        // Show Strands would also draw the boundary strands as swept tubes (world.rs:2337).
        assert!(s.membrane[1] <= 0.5, "Show Strands must be off");
    }

    // -----------------------------------------------------------------------------
    // The geometry itself — built headless through the same call the World makes
    // -----------------------------------------------------------------------------

    /// Build the membrane mesh exactly as `world.rs:2815-2831` does, with the caller
    /// supplying the animation clock so a test can vary it. `ParamValues` is assembled
    /// field-for-field as at `world.rs:2225-2232`.
    fn build_sheet(s: &Shared, angle: DVec3) -> (Vec<Vec3>, Vec<Vec3>, Vec<glam::Vec4>, Vec<u32>) {
        let pv = ParamValues {
            loop_count: Vec3::new(s.loop_count[0], s.loop_count[1], s.loop_count[2]),
            loop_count_q: s.loop_count[3],
            rot_amp: Vec3::new(s.rot_amp[0], s.rot_amp[1], s.rot_amp[2]),
            trans_amp: Vec3::new(s.trans_amp[0], s.trans_amp[1], s.trans_amp[2]),
            trans_mod: Vec3::new(s.trans_mod[0], s.trans_mod[1], s.trans_mod[2]),
            scale_amp: s.scale_amp, // unread by `draw_membrane`; carried for fidelity
        };
        let (mut pos, mut norm, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
        math::draw_membrane(
            &pv,
            FuncName::from_u32(s.rot_func),
            FuncName::from_u32(s.trans_func),
            angle,
            angle,                  // rot_phase: pendulum mode reads `angle` (world.rs:2324)
            s.rot_amp[3] != 0.0,    // continuous (world.rs:2323)
            s.origin_mode != 0,     // centered (world.rs:2754)
            s.surface_fx[6] as u32, // palette (world.rs:2345)
            0.0,                    // color_phase — frozen by `bio[0] = 0`
            s.membrane[0] as u32,   // weave id
            s.membrane[3] > 0.5,    // close the 360° seam
            &mut pos,
            &mut norm,
            &mut col,
            &mut idx,
        );
        (pos, norm, col, idx)
    }

    /// ONE sheet, flat on world y = 0, centred on the origin, with the vertex and triangle
    /// counts the constants promise.
    #[test]
    fn the_sheet_is_one_flat_plane_on_the_world_xz_plane() {
        let s = applied();
        let (pos, norm, col, idx) = build_sheet(&s, DVec3::ZERO);

        let (nx, nz) = (SUBSTRATE_GRID_X as usize, SUBSTRATE_GRID_Z as usize);
        assert_eq!(pos.len(), nx * nz, "exactly one sheet of nx × nz vertices");
        assert_eq!(idx.len(), 6 * (nx - 1) * (nz - 1), "two triangles per quad");
        assert_eq!(norm.len(), pos.len());
        assert_eq!(col.len(), pos.len());

        // Flat: every vertex on y = 0.
        for p in &pos {
            assert_eq!(p.y, 0.0, "the sheet must lie exactly on world y = 0");
        }

        // Extent + centring: a unit-pitch lattice symmetric about the origin.
        let half_x = (nx as f32 - 1.0) * 0.5;
        let half_z = (nz as f32 - 1.0) * 0.5;
        let (mut mnx, mut mxx, mut mnz, mut mxz) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &pos {
            mnx = mnx.min(p.x);
            mxx = mxx.max(p.x);
            mnz = mnz.min(p.z);
            mxz = mxz.max(p.z);
        }
        assert_eq!((mnx, mxx), (-half_x, half_x), "x extent, centred");
        assert_eq!((mnz, mxz), (-half_z, half_z), "z extent, centred");
        assert_eq!(mxx - mnx, 127.0, "127 world units across — see KNOWN_UNSTILLABLE #4");

        // One constant normal. `push_sheet` winds x × z, so the geometric normal is −Y;
        // `cube.wgsl:1382-1385` flips it toward the viewer, which is why a sheet lights
        // correctly from either side. What matters here is that it is CONSTANT.
        for n in &norm {
            assert_eq!(*n, Vec3::NEG_Y, "a flat sheet has one normal everywhere");
        }

        // Native palette → per-vertex colour is the normalized position in the bounds
        // (math.rs:12257-12262). Flat in y ⇒ the green channel is identically 0, and the
        // red/blue ramps span the full 0..1 — the albedo variation R5 asks for.
        let (mut mnr, mut mxr) = (f32::MAX, f32::MIN);
        for c in &col {
            assert_eq!(c.y, 0.0, "flat in y ⇒ no green component");
            mnr = mnr.min(c.x);
            mxr = mxr.max(c.x);
        }
        assert_eq!((mnr, mxr), (0.0, 1.0), "the albedo ramp must span the sheet");
    }

    /// Stillness, mechanically: the mesh is **byte-identical** at wildly different
    /// animation-clock phases. This is the property `SUBSTRATE_TRANS_AMP` and
    /// `SUBSTRATE_ROT_AMP` exist for — with either non-zero, `angle` reaches the vertex
    /// positions through `math.rs:11820-11824` and this fails.
    #[test]
    fn the_sheet_does_not_move_with_the_animation_clock() {
        let s = applied();
        let a = build_sheet(&s, DVec3::ZERO);
        let b = build_sheet(&s, DVec3::new(123.456, -78.9, 4242.0));
        assert_eq!(a.0, b.0, "positions moved with the animation clock");
        assert_eq!(a.1, b.1, "normals moved with the animation clock");
        assert_eq!(a.3, b.3, "topology changed with the animation clock");

        // And the guard is real, not vacuous: put the clock back into the expression and
        // the same comparison must fail.
        let mut breathing = s;
        breathing.trans_amp[0] = 1.0;
        let c = build_sheet(&breathing, DVec3::ZERO);
        let d = build_sheet(&breathing, DVec3::new(1.0, 0.0, 0.0));
        assert_ne!(c.0, d.0, "control: non-zero trans_amp must make the lattice breathe");
    }
}

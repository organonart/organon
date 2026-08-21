//! The Organon Console binary (Console #10 T1 + #14 T1): a terminal, with the
//! engine underneath it.
//!
//! PRD v3.1's form: a real PTY + the adopted VT core, drawn as a GPU glyph grid —
//! and, when summoned, the Organon renderer *behind* the glyphs (tree E Tier 1, the
//! first of the three lit-surface layers: engine-behind-the-terminal). The window
//! plumbing is the v2 lineage's (visual.rs's init, ui_layer.rs's paint order); the
//! device negotiation is `wgpu_editor::bring_up`'s in full — the #6-era debt repaid:
//! a default-limits device opens the window and then fails to create the engine's
//! pipelines, so the shell window always negotiates like the renderer host it is.
//!
//! Backdrop contract (PRD §4.6): summoned, never imposed — `ORGANON_SHELL_BACKDROP`
//! is tonight's dev summons (the typed `surface` command is tree E's real one), and
//! the legibility scrim over the render is not optional at any setting. The Console
//! Spike's Tier 1 gave that summons a second value: `1` is the live world, `substrate`
//! is one flat lit plane. See [`BackdropSource`].
//!
//! **Console Spike Tier 2 turns that summons into a sentence you can type.**
//! `organon console background <name>` / `console rig <name>` append one line each to
//! `cli::console_cmd_path()` — the console's own sidecar, not the World's `cli.txt`,
//! because a backdrop is `Console` state and nothing in the World can reach it (brief R3).
//! [`Console::drain_console`] drains that file every frame on the same file-length
//! watermark the World uses for `cli.txt`, routes each op through the product's first
//! live [`organon_console::command::CommandService`], and applies the survivors by
//! recomputing the published snapshot from one pure function of
//! `(source, material, rig)` — [`look_shared`]. No texture is recreated, no pane is
//! re-measured, so a live switch costs one frame and moves no glyph.
//!
//! **Console Spike Tier 5 puts a third verb on that lane, and it changes the transcript
//! rather than the dressing.** `organon console block <rows>` reserves a contiguous run of
//! blank rows in the active tab, by feeding `\r\n` bytes the console generated itself through
//! the terminal's own parser ([`Console::open_block`]). They are ordinary scrollback rows, so
//! text written afterwards flows **below** them. Nothing is painted into them yet — the hole
//! is the increment.
//!
//! **`organon console patch --up N --rows M --kind <scene|panel>` is the corrected verb and
//! the one that carries something.** The writer prints the gap itself, through the ordinary
//! PTY, and then says where it is; the console records it ([`Console::claim_patch`]) and paints
//! it. The kind selects the paint and **nothing before it** — the claim, the anchor
//! arithmetic and the per-pane ledger are shared — so `scene` samples the rendered substrate
//! through the rows and `panel` puts a live egui control panel in the same rect, whose
//! buttons re-enter this file at [`Console::apply_console`], the same call a typed
//! `organon console background <name>` reaches.
//!
//! **Console Spike §5.9 forks the console into two front-ends over one renderer, and this
//! file hosts both.** The terminal host above is unchanged and is the universal fallback:
//! it runs any program and knows nothing about it. Beside it now sits the **conversation
//! view** — a tab that spawns no PTY at all, drives an agent over pipes, and renders its
//! structured event stream natively ([`Pane`], [`organon_console::conversation_view`]). The
//! window, the tab strip, the command lane and the backdrop are shared; only what a tab
//! *is* differs. `SHELL_ARCHITECTURE.md` owns the shape.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use organic_math_native::agent;
use organic_math_native::cli;
use organic_math_native::console_icon;
use organic_math_native::panel_surface::OrganonPanels;
use organic_math_native::params::OrganicMathParams;
use organic_math_native::scene_input;
use organic_math_native::substrate_camera::SubstrateRig;
use organic_math_native::substrate_epochs::{EpochId, EpochLedger, Look, SlotAction};
use organic_math_native::substrate_materials;
use organic_math_native::substrate_scene;
use organic_math_native::world::World;
use organon_core::edition::EDITION;
use organon_core::ipc;
use organon_core::kind;
use organon_console::block_anchor::Block;
use organon_console::block_panel::{BlockAction, BlockPanel, Patch, PatchContent};
use organon_console::camera;
use organon_console::command::{
    ArgKind, ArgSpec, CommandError, CommandService, CommandSpec, CommandTarget, Reversal,
    TargetKind,
};
use organon_console::conversation::ElementId;
use organon_console::conversation_view::{
    self, ConversationPane, ExhibitContent, ExhibitContents, ExhibitRequest, SurfaceImages,
    SurfaceRequest,
};
use organon_console::harness::{self, HarnessSpec};
use organon_console::platform::Platform;
use organon_console::portal::{self, PortalState};
use organon_console::posture::Posture;
use organon_console::prefs;
use organon_console::session::{Issuer, SessionLog};
use organon_console::tabs::{self, Tab, TabAction, TabStrip};
use organon_console::term::{self, TermSession};
use organon_console::term_view::{self, BandedBackdrop, PaneAnchor};
use organon_console::theme::{self, Theme};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Swapchain + config. The `Device`/`Queue` are owned by the [`World`] after
/// `attach_gpu` and borrowed back for the egui pass — the route-C arrangement.
struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// The gamma arrangement measured in `wgpu_editor::SCENE_PANE_FORMAT`: render the
/// world through the sRGB format, hand egui a non-sRGB view of the same bytes —
/// egui's shader linearizes its samples itself, and a decoded-on-sample view would
/// linearize twice and come out dark.
const BACKDROP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BACKDROP_SAMPLE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Console Spike Tier 1 — what fills the backdrop texture.
///
/// `ORGANON_SHELL_BACKDROP`'s value space is **widened, not replaced**: unset and `0` are
/// off, anything else is the World exactly as before, and one new spelling selects the lit
/// substrate plane. Keeping the World selectable is not politeness — the CLI's override lane
/// (`organon set`/`generator`/`recipe`) drains inside `World::frame_body`, so a substrate
/// that *replaced* the World would silently kill the live response the console demos
/// (brief R1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackdropSource {
    Off,
    World,
    Substrate,
}

/// The one new `ORGANON_SHELL_BACKDROP` value, quoted by the parser *and* by `--help` so the
/// two cannot drift — the discipline `SCRIM_DEFAULT` already earned here.
const BACKDROP_SUBSTRATE: &str = "substrate";

/// **How this binary introduces itself**: the window title, the `--help` header, `--version`,
/// and the startup banner. Deliberately *not* `EDITION.product_name()`, which still answers
/// "Organon Console".
///
/// The artifact is `organon-console` and the public name is **Organon Console**, so a console
/// whose title bar and `--help` said "Organon Console" would introduce itself as a product that
/// is not what you launched. Everything the rename does *not* touch is what something else
/// reads: `EDITION` is `organon-core`'s shared spine (the compositor lib's heading and the
/// edition tests quote it too), the crate is `organon-console`, the feature is `console-edition`,
/// the variables below are a shipped flag surface, and `organon-shell` is a *wire* identifier
/// — the IPC namespace the `organon` CLI joins on. Issue #3 owns collapsing the two names;
/// this constant is the honest seam until then, not a half-done rename.
const PRODUCT_NAME: &str = "Organon Console";

/// What a user types. Kept beside [`PRODUCT_NAME`] because `--help`'s usage line is the one
/// place the *command* is quoted rather than the product, and the two went out of sync the
/// moment the bin target was renamed.
const INVOCATION_NAME: &str = "organon-console";

/// `ORGANON_SHELL_BACKDROP` → a source. `None` is "unset". Pure, so the value space is a
/// test rather than a claim.
fn parse_backdrop_source(v: Option<&str>) -> BackdropSource {
    match v {
        None | Some("0") => BackdropSource::Off,
        Some(s) if s.eq_ignore_ascii_case(BACKDROP_SUBSTRATE) => BackdropSource::Substrate,
        Some(_) => BackdropSource::World,
    }
}

/// The three words `organon console background <name>` accepts that are **sources** rather
/// than materials — [`BackdropSource`]'s value space, spelled.
///
/// 🚨 **This list is one half of a two-literal pin, not a binding.** The other half is
/// `CONSOLE_SOURCES` in `src/bin/ctl.rs`, and the two cannot be bound the way the material
/// and rig tables are: those live in a **library** module (`substrate_materials`) both
/// binaries can import, while `BackdropSource` is this binary's own type and no `bin` can
/// see another `bin`. Each side asserts the triple against its own resolver, so a change to
/// one fails that side's test naming the other — which is a smoke alarm, not a wire. The
/// real fix is a `pub const` in `cli.rs` beside `parse_console_op` (already the declared home
/// of "both ends speak one vocabulary from one place"); it is a four-line change and it is
/// recorded in SHELL_ARCHITECTURE.md's honesty ledger rather than done here, because
/// `cli.rs` is another leaf's file this tier only reads.
const BACKDROP_SOURCE_WORDS: [&str; 3] = ["world", "off", BACKDROP_SUBSTRATE];

/// A `background` argument that names a **source**. `None` means "not a source" — which is
/// how the caller knows to look the name up as a material instead.
///
/// Deliberately NOT [`parse_backdrop_source`]: that function's job is an environment
/// variable whose historical contract is "anything not `0`/unset is the World", so it maps
/// `frobnicate` to `World` on purpose. A typed command must not. Two readers, two rules, and
/// the case where they differ is exactly the one a shared function would get wrong.
fn console_source(name: &str) -> Option<BackdropSource> {
    match name {
        n if n.eq_ignore_ascii_case("world") => Some(BackdropSource::World),
        n if n.eq_ignore_ascii_case("off") => Some(BackdropSource::Off),
        n if n.eq_ignore_ascii_case(BACKDROP_SUBSTRATE) => Some(BackdropSource::Substrate),
        _ => None,
    }
}

/// The canonical spelling of `name` in `names`, matched case-insensitively — or `None`.
///
/// The console stores the canonical form, never what was typed, so [`ConsoleLook`] is a
/// value Tier 4's epoch ledger can compare rather than a transcript of keystrokes.
fn canonical<'a>(names: &[&'a str], name: &str) -> Option<&'a str> {
    names.iter().copied().find(|n| n.eq_ignore_ascii_case(name))
}

/// The substrate lens, in **vertical** degrees — vertical is what the engine takes
/// (`world.rs:10564-10567`), and an axis mix-up is silent.
///
/// 10°, and the width is the deliverable rather than a framing detail: a flat plane under a
/// uniform material shades to one constant colour when the view vector does not vary (brief
/// R5), so the frustum's diagonal half-angle **is** the shading gradient. At 10° / 16:9 that
/// is `substrate_camera::max_view_deviation_deg` ≈ 10.1°. Narrower is now reachable — this
/// tier moved the engine's FOV clamp floor to 4° at both sites — and 4° frames the same plane
/// from ≈1023 world units with ≈4.1° of gradient. That headroom is deliberate and unspent: it
/// is the dial to turn if the backdrop reads as too much perspective.
const SUBSTRATE_FOV_DEG: f32 = 10.0;

/// The substrate plane's side in world units, **derived from the sheet the look actually
/// builds** rather than restated: `substrate_scene`'s lattice is `SUBSTRATE_GRID_X` nodes at
/// the membrane path's hard-coded 1-unit pitch, so it spans one less than that (127). Change
/// the grid and the framing follows.
const SUBSTRATE_EXTENT: f32 = substrate_scene::SUBSTRATE_GRID_X - 1.0;

/// The substrate key light's azimuth in degrees — **re-derived for the camera this file
/// installs**, and the one value of `substrate_scene`'s look that is overridden here.
///
/// Leaf B chose −10° against the *stock* camera (yaw 0.7 rad ≈ 40°, pitch 0.45), where it
/// reads as above-left, and says in as many words that the constant is coupled to whatever rig
/// the integrator installs. This rig is top-down (yaw 0, pitch ≈ π/2). Under
/// `look_at_rh(eye ≈ +Y·d, origin, Vec3::Y)` the screen basis comes out
/// **right = world +X, up = world −Z**: with the ε tilt aside the camera's up-vector has no
/// world Y left in it, so the key's *elevation* contributes nothing to where the light appears
/// to be and its azimuth alone decides the compass point. `dir_from_angles` builds the
/// direction **to** the light as `(cos e·sin a, sin e, cos e·cos a)`, which lands on screen at
/// `(sin a, −cos a)·cos e` — azimuth 0 reads bottom, 90 right, ±180 top, −90 left. Upper-left
/// at 45° is therefore **−135°**, and Leaf B's −10° would have read as lower-*left*: the same
/// light, a different camera. The derived fill follows for free at `a − 120° ≡ +105°`, from the
/// right and slightly above. In range (−180..180, `params.rs:8554`).
///
/// 📌 Checked, because it would have made this constant inert: `build_uniforms` **replaces**
/// `key_dir` with the terrain sun when the terrain backdrop is on with "sun lights scene". It
/// is gated on `terrain[0]`, and `terrain_enabled` defaults to **false** (`params.rs:8908`) —
/// `substrate_scene` writes neither, so the key stays ours. (That gate is the *terrain*
/// backdrop, not the atmosphere: `atmos_enabled` does default true and is exactly the sky this
/// rig wants for its IBL.)
///
/// It lives here and not in `substrate_scene.rs` because the coupling runs this way round: the
/// look is camera-agnostic, and the camera is this file's.
const SUBSTRATE_KEY_AZIMUTH_DEG: f32 = -135.0;

/// The console's substrate dressing: **the (material, rig) pair** and nothing else.
///
/// Named for what consumes it. Tier 4's epoch ledger records exactly this pair beside each
/// backdrop change, so it is one value with one name rather than two loose fields on
/// [`Console`] that a ledger would have to re-pair.
///
/// Both are `Option` because **absent is not the same as default**, and the difference is
/// Tier 1's shipped bytes:
///
/// * `material: None` — the substrate as Tier 1 built it, with no `#472` map stack at all.
///   There is no "none" material in Leaf A's table (its `KNOWN_LIMITS` #3 says so and says
///   why), so `None` here is the only way to express "before any material was named", and it
///   is what startup publishes.
/// * `rig: None` — indistinguishable from `studio` **by construction**: Leaf A's
///   `studio_is_exactly_tier_ones_shipped_rig` proves `apply_rig(studio)` over a fresh
///   substrate snapshot is a no-op. A test below re-states it from this side, so the day
///   `studio` stops being Tier 1's rig, this stops silently meaning two things.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ConsoleLook {
    material: Option<String>,
    rig: Option<String>,
}

/// The rig an **unnamed** rig is. `an_unnamed_rig_is_studio` proves the published bytes are
/// identical, so spelling it here is what stops `rig studio` from opening an epoch that
/// changes no pixel — [`EpochLedger::open`]'s no-op rule compares names, and `None` would
/// compare unequal to the same picture.
const UNNAMED_RIG: &str = "studio";

/// The `(source, look)` pair as the epoch ledger's [`Look`] — **two names, compared for
/// equality and printed**, never resolved back to bytes.
///
/// The ledger's material slot holds either a material name or one of
/// [`BACKDROP_SOURCE_WORDS`], and that widening is load-bearing rather than tidy:
///
/// * **`world` and `off` must not compare equal to a material.** `background world` then
///   `background graphite` publishes the same `(material, rig)` [`ConsoleLook`] it had before
///   the detour — so a ledger keyed on that pair alone would see no change, open no epoch, and
///   claim the rows written under the live World were substrate.
/// * **An undressed substrate is not `slate`.** `ConsoleLook::material: None` is Tier 1's
///   shipped plane with no `#472` map stack at all, and
///   `startup_is_tier_ones_substrate_untouched_by_tier_two` pins that it differs from every
///   named material. There is no "none" material to name it with, so it takes the source
///   word — which is exactly what a user typed to get it.
///
/// The three source words cannot collide with a material name; a test below pins that they
/// are disjoint, because the day they are not, two different looks quietly become one epoch.
fn ledger_look(source: BackdropSource, look: &ConsoleLook) -> Look {
    let rig = look.rig.as_deref().unwrap_or(UNNAMED_RIG);
    match source {
        BackdropSource::Off => Look::new("off", rig),
        BackdropSource::World => Look::new("world", rig),
        BackdropSource::Substrate => {
            Look::new(look.material.as_deref().unwrap_or(BACKDROP_SUBSTRATE), rig)
        }
    }
}

/// What `organon console background substrate` selects when no material has been named yet.
///
/// `slate` because it is the one material defined as Tier 1's surface formalized — it keeps
/// `SUBSTRATE_METALLIC`/`SUBSTRATE_ROUGHNESS`/`SUBSTRATE_MATERIAL_TYPE` and adds no albedo
/// map, so the per-vertex ramp survives (Leaf A's
/// `slate_keeps_tier_ones_surface_scalars_and_its_per_vertex_ramp`). Asking for "the
/// substrate" and getting a look that is not Tier 1's would make the bare source word mean
/// something the tier before it did not.
const CONSOLE_DEFAULT_MATERIAL: &str = "slate";

/// The `Shared` snapshot the console publishes every redraw, for a given backdrop source and
/// substrate dressing.
///
/// **Total over the source**, and that totality is the whole reason live switching needed no
/// new state machine: every command recomputes the snapshot from scratch rather than
/// patching the one before it, so `graphite → world → substrate` cannot leave a lane behind
/// and there is no "have we installed the look yet?" flag to get wrong. The cost is one
/// `Shared`-sized memcpy per typed command, on the frame it is typed.
///
/// The order is `look → material → rig`, and only the first edge of it is load-bearing:
///
/// 1. **`apply_substrate_look` must go first** — it owns the geometry, the stillness
///    switches, the palette and the background, and it *also* writes whole `lighting`,
///    `pbr` and `matcol` blocks (its manifest declares them at block granularity). A
///    material or a rig applied before it would be overwritten wholesale.
/// 2. **material before rig is a free choice**, taken defensively. Leaf A's two manifests
///    are disjoint at lane granularity — material owns `lighting[7]`, `pbr[0..2]`,
///    `matcol[0..4]`, `aniso`; the rig owns `lighting[0..3]`, `pbr[2]`, `pbr[3]` — and
///    `material_and_rig_commute` proves both orders give identical bytes for all eight
///    combinations. This order is the one that stays correct if a future material ever grows
///    a lighting-adjacent lane, and costs nothing today.
/// 3. **The key azimuth is last, always.** `SUBSTRATE_KEY_AZIMUTH_DEG` is the *camera's*
///    correction, not the look's, and Leaf A's rigs write no direction at all (their section
///    comment says so, and names this override as the reason). Applying it after everything
///    makes that survive any future rig that forgets.
///
/// 🚨 **Residue verdict, checked rather than assumed.** `apply_material` is *total over its
/// block*: it writes every lane a material can use on every call, including a disabled
/// overlay and unused gradient stops, and Leaf A's
/// `switching_between_any_two_materials_converges` proves all **16** ordered pairs land
/// byte-identical. So material→material needs no reset. The reset sequence here is not for
/// that: it is for `world → substrate`, where the previous snapshot is the plain default and
/// the substrate look was never applied at all.
///
/// 📌 It also dissolves Leaf A's `KNOWN_LIMITS` #3 — "`apply_substrate_look` does not turn
/// the maps off, so re-running it over a material leaves the material on" — without needing
/// the "none" material it declined to invent. Starting from
/// `OrganicMathParams::default().to_shared()` every time means there is never a previous
/// material to leave on: `background world` publishes a snapshot whose `material_*` blocks
/// are the engine's own defaults, not graphite's with the plane taken away.
fn look_shared(source: BackdropSource, look: &ConsoleLook) -> Box<ipc::Shared> {
    let mut s = Box::new(OrganicMathParams::default().to_shared());
    if source == BackdropSource::Substrate {
        substrate_scene::apply_substrate_look(&mut s);
        if let Some(name) = look.material.as_deref() {
            // A `false` return is impossible by construction (`console_step` canonicalizes
            // against `MATERIAL_NAMES` before a name ever reaches a `ConsoleLook`) and inert
            // if it ever happened — Leaf A's unknown-name contract is "touched nothing".
            let _ = substrate_materials::apply_material(&mut s, name);
        }
        if let Some(name) = look.rig.as_deref() {
            let _ = substrate_materials::apply_rig(&mut s, name);
        }
        // Last, and deliberately after the look: see [`SUBSTRATE_KEY_AZIMUTH_DEG`] for why
        // this one value is the camera's business and not the look's.
        s.lighting[4] = SUBSTRATE_KEY_AZIMUTH_DEG;
    }
    s
}

/// The snapshot at startup: [`look_shared`] with **nothing dressed** — Tier 1's bytes
/// exactly, so a console launched with `ORGANON_SHELL_BACKDROP=substrate` looks in Tier 2
/// precisely as it looked in Tier 1 until someone types a command.
fn initial_shared(source: BackdropSource) -> Box<ipc::Shared> {
    look_shared(source, &ConsoleLook::default())
}
// ---------------------------------------------------------------------------
// The console command lane (#4 Tier 2)
// ---------------------------------------------------------------------------

/// The catalog names the two console verbs dispatch under. Dotted, `<surface>.<verb>`, which
/// is `command.rs`'s convention (`session.note`, `shell.echo`) and what makes
/// `CommandService::suggest("console.")` a usable palette feed on the day there is a palette.
const CMD_BACKGROUND: &str = "console.background";
/// See [`CMD_BACKGROUND`].
const CMD_RIG: &str = "console.rig";
/// See [`CMD_BACKGROUND`]. #38: the console's own palette, not the substrate's dressing.
///
/// ⚠️ **Aliased to the registry's spelling rather than written out again**, because this is the
/// one console verb the conversation view has to *recognise* as well as forward: two of its
/// argument values open the live colour editor in that crate's own band (§1.10) instead of
/// dispatching. Two string literals would compare equal today and stop comparing equal the day
/// somebody renames the verb — and the symptom would be `/theme edit` quietly dispatching as an
/// unknown palette name rather than opening anything.
const CMD_THEME: &str = organon_console::registry::VERB_THEME;
/// See [`CMD_BACKGROUND`]. #38: how the console holds itself, on the terminal↔desktop axis.
const CMD_POSTURE: &str = "console.posture";
/// See [`CMD_BACKGROUND`]. Whether the window covers the display — **a third axis, orthogonal
/// to [`CMD_POSTURE`]**, not a value of it. `organon_console::screen`'s header owns the
/// argument; the one-line form is that a posture is a set of form tokens and full screen moves
/// none of them.
///
/// ⚠️ **Named for the WINDOW's state, and deliberately not "fullscreen".**
/// `CONSOLE_ARCHITECTURE.md` §2's seams table reserves the phrase *full screen* for a portal state
/// that is still unbuilt — the portal taking the whole window, after `immersive` and before the
/// animated grow. Two different rectangles can each be described as going full screen, so this
/// verb says which one it moves.
const CMD_SCREEN: &str = "console.screen";
/// See [`CMD_BACKGROUND`]. **How the pane inside the window is divided, and what each part
/// holds** — a fourth axis, orthogonal to the posture and the screen both. `organon_console::
/// region`'s header owns the argument; the short of it is that a split changes none of `Form`'s
/// tokens and does not move the window, so it is neither of the two verbs beside it.
const CMD_VIEWPORT: &str = "console.viewport";
/// See [`CMD_BACKGROUND`]. **What is IN a region that holds `panel`** — the scrolling column of
/// Organon's editor panels. `organon_console::panel_stack`'s header owns the argument.
///
/// 🚨 **A second verb rather than a third word on [`CMD_VIEWPORT`], and that split IS the
/// tier.** `CONSOLE_ARCHITECTURE.md` §2 recorded the blocker on giving `panel` a body as *"a
/// third word naming which panel, since two rings cannot say it"*. Naming the region and naming
/// the panel in **different commands** dissolves it: `viewport left panel` declares the region,
/// `stack add surface` fills it, and neither sentence needs more than two words.
///
/// ⚠️ **Not spelled `panel`.** `/panel` is a *retired* word the console refuses by name, with a
/// test pinning the refusal, and re-minting it for a different meaning is how somebody comes to
/// type it expecting the old thing. `stack` also names what this verb edits: `panel` is what a
/// *region* holds.
const CMD_STACK: &str = "console.stack";
/// See [`CMD_BACKGROUND`]. **An arrangement of the pane, under a name** — save what the console
/// is holding, bring one back, take one out. `organon_console::layout`'s header owns the
/// argument, and `doc/organon_is_the_product.md` §4 owns why it matters: a layout is the unit of
/// product identity rather than a convenience, so "Claude Code Desktop" and "Organon standalone"
/// can be named arrangements of one program instead of two programs.
///
/// 🚨 **A third verb rather than a fifth word on [`CMD_VIEWPORT`], and the split is the same one
/// [`CMD_STACK`] made.** `viewport` says what *one region* holds; this says "all of that, under a
/// name", so it needs no region word at all and neither sentence grows a ring.
///
/// ⚠️ **An alias of `registry::VERB_LAYOUT`, not a second spelling**, on [`CMD_THEME`]'s rule
/// and for a sharper version of its reason: the conversation view keys this verb's dependent
/// ring on that constant, and a rename that moved only one of two literals would leave the verb
/// working with its ring silently gone.
const CMD_LAYOUT: &str = organon_console::registry::VERB_LAYOUT;
/// The **read**: what is in the layout library. Not in [`console_specs`] — see [`mcp_specs`] for
/// why a read has no sidecar spelling, and [`CMD_NAME`] for why it is a verb of its own rather
/// than a fourth action word on [`CMD_LAYOUT`].
const CMD_LAYOUT_LIST: &str = "console.layout.list";
/// See [`CMD_BACKGROUND`]. Console Spike Tier 5: reserve rows in the transcript.
const CMD_BLOCK: &str = "console.block";
/// See [`CMD_BACKGROUND`]. Console Spike Tier 5, the **corrected** verb: claim a rectangle the
/// writer already left in its own output. The console records; it never writes.
const CMD_PATCH: &str = "console.patch";
/// See [`CMD_BACKGROUND`]. The portal: a screen-anchored, live window onto the world.
const CMD_PORTAL: &str = "console.portal";
/// See [`CMD_BACKGROUND`]. Where the viewer stands: the yaw/pitch/distance a drag and a wheel
/// over the portal already write.
const CMD_CAMERA: &str = "console.camera";
/// The single argument the two **dressing** verbs take. One name, because the sidecar's wire
/// form is `<verb> <word>` and inventing two spellings for one slot is how a schema drifts
/// from its transport.
///
/// ⚠️ `registry::THEME_ARG` is the conversation view's copy of this one slot name — it reads the
/// value out of a dispatch payload to see whether `/theme` was asked to open the editor.
/// [`tests::the_theme_verbs_slot_name_is_the_one_the_view_reads`] pins the two together; they
/// cannot be aliased in the same direction as [`CMD_THEME`] because this name is shared by
/// three verbs and only one of them is read over there.
const CMD_ARG: &str = "name";
/// [`CMD_BLOCK`]'s argument. A **second** slot name rather than a reuse of [`CMD_ARG`],
/// because it is a different kind: `name` is a `Choice` over a table and `rows` is an `Int`,
/// and a palette showing "name: 12" would be describing the wrong thing.
const CMD_ROWS: &str = "rows";
/// [`CMD_PATCH`]'s other argument: how many lines above the current line the rectangle starts.
const CMD_UP: &str = "up";
/// [`CMD_PATCH`]'s third argument: what the console should draw in the rectangle. A `Choice`
/// rather than an `Int` or a free word, over `organon_core::kind::KIND_WORDS` — the same
/// table-not-a-restatement arrangement `background`'s materials use, so the palette, the
/// CLI's `--help` and this schema cannot come to know different kinds.
const CMD_KIND: &str = "kind";
/// [`CMD_PORTAL`]'s only argument: `open` / `close` / `toggle`. A **fourth** slot name rather
/// than a reuse of [`CMD_ARG`], on [`CMD_ROWS`]' rule — `name` is a choice over the materials
/// and rigs, and a palette offering `open` under the heading "name" would be describing the
/// wrong thing. The `Choice` is built from `cli::PORTAL_WORDS`, so the schema, the CLI's
/// `--help` and the parser are three renderings of one table.
const CMD_STATE: &str = "state";
/// [`CMD_VIEWPORT`]'s two slots. **Two named slots rather than one `Choice` of nine × three
/// pairs**, because they are two independent value spaces and a palette that offered
/// `left-agent` as a word would be describing a thing neither table contains — and the second
/// ring is what makes the verb completable: `/viewport left ` then narrows to what a region can
/// hold. Neither is [`CMD_ARG`] or [`CMD_STATE`], on [`CMD_ROWS`]' rule: a region is not a
/// `name`, and `agent` is not a *state* the way `open` and `full` are.
const CMD_REGION: &str = "region";
/// See [`CMD_REGION`].
const CMD_CONTENT: &str = "content";
/// [`CMD_STACK`]'s two slots. Neither reuses [`CMD_ARG`] or [`CMD_REGION`], on [`CMD_ROWS`]'
/// rule: `add` is not a `name`, and a panel is not a region. Both are `Choice`s over
/// `panel_stack`'s own tables, so the MCP schema, the slash palette's two rings and the CLI's
/// `--help` are three renderings of one vocabulary.
///
/// ⚠️ **Both are required, and the emptying word rides [`CMD_PANEL`]** rather than being a third
/// action — `panel_stack::StackCmd`'s doc owns the argument, and the short of it is that the
/// slash grammar fills required arguments positionally and optional ones by keyword, so an
/// optional panel would make the typed line `/stack add panel surface` while the CLI stayed
/// `stack add surface`. One verb, two spellings, is the drift this tree spends its refusals
/// preventing.
const CMD_ACTION: &str = "action";
/// See [`CMD_ACTION`].
const CMD_PANEL: &str = "panel";
/// [`CMD_LAYOUT`]'s second slot: which saved arrangement. **Not [`CMD_ARG`]**, on [`CMD_ROWS`]'
/// rule — `name` there is a `Choice` over the materials and rigs, and this is free text a person
/// invented. [`CMD_ACTION`] *is* shared with [`CMD_STACK`], deliberately: both are the same kind
/// of slot (a closed table of verbs-within-a-verb) and a palette heading "action" describes both
/// correctly, which is the test [`CMD_ROWS`] states.
///
/// 🚨 **Both slots are required, and that is what forces the listing to be a separate verb.**
/// `registry::parse_args` fills required arguments positionally and optional ones **by keyword**,
/// so an optional name would make the typed line `/layout save name mine` while the CLI stayed
/// `console layout save mine` — one verb, two spellings, the drift this tree spends its refusals
/// preventing. With both required there is no honest word to put in this slot for a `list`
/// (`panel_stack`'s `all` works there because `all` genuinely names a value in the panel ring,
/// and no layout name means "every layout"), so the listing is [`CMD_LAYOUT_LIST`] — a **read**,
/// on the precedent [`CMD_CAMERA_READ`] set.
///
/// ⚠️ **An alias of `registry::LAYOUT_NAME_ARG`**, for [`CMD_LAYOUT`]'s reason: the ring hook
/// is offered one argument at a time *by name*, so a slot renamed here and not there would stop
/// narrowing without stopping working.
const CMD_NAME: &str = organon_console::registry::LAYOUT_NAME_ARG;
/// [`CMD_CAMERA`]'s four slots. Named per axis rather than as one `axis` + `value` pair,
/// because framing a shot is **one intent**: a caller that wants to be closer *and* a little
/// above says so once and the viewpoint moves once, instead of travelling through an
/// intermediate framing nobody asked to see — which on a live portal is a frame somebody
/// watches. [`CMD_RESET`] is a `Bool` and the other three are `Float`s with their own bands,
/// which is the second reason not to collapse them: one shared value slot could only declare
/// the union of three different ranges, and a schema that states a range it does not mean is
/// worse than one that states none.
const CMD_YAW: &str = "yaw";
/// See [`CMD_YAW`].
const CMD_PITCH: &str = "pitch";
/// See [`CMD_YAW`].
const CMD_DISTANCE: &str = "distance";
/// See [`CMD_YAW`].
const CMD_RESET: &str = "reset";
/// The **read**: where the viewer stands right now. Not in [`console_specs`] — see
/// [`mcp_specs`] for why this one verb has no sidecar spelling.
const CMD_CAMERA_READ: &str = "console.camera.read";

/// The console's vocabulary, as [`CommandService`] catalog data.
///
/// The `Choice` options are built **from Leaf A's tables** rather than restated, so the
/// service validates against the same list the renderer draws from. That is the shell-side
/// half of the drift guard; the CLI-side half lives in `ctl.rs`'s tests, which bind its clap
/// `PossibleValuesParser` lists to the same two constants.
///
/// [`TargetKind::Viewport`] is the honest one of the five: a backdrop is what the console
/// *shows*, not a project, a runtime, or an artifact. It reaches the log as the literal
/// `"viewport"` (`TargetKind::as_str`), so a reader of `events.jsonl` sees where the command
/// landed without a decoder ring.
fn console_specs() -> Vec<CommandSpec> {
    let backgrounds: Vec<String> = substrate_materials::MATERIAL_NAMES
        .iter()
        .chain(BACKDROP_SOURCE_WORDS.iter())
        .map(|s| (*s).to_string())
        .collect();
    let rigs: Vec<String> =
        substrate_materials::RIG_NAMES.iter().map(|s| (*s).to_string()).collect();
    vec![
        CommandSpec {
            name: CMD_BACKGROUND.into(),
            doc: "What sits behind the glyphs: a substrate material, or a backdrop source"
                .into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: CMD_ARG.into(),
                kind: ArgKind::Choice(backgrounds),
                required: true,
            }],
            // Every backdrop is one command away from every other, including the one it
            // replaced. See [`Reversal`] — the rule and its reason live there.
            reversal: Reversal::Recoverable,
        },
        CommandSpec {
            name: CMD_RIG.into(),
            doc: "The substrate's lighting rig".into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: CMD_ARG.into(),
                kind: ArgKind::Choice(rigs),
                required: true,
            }],
            reversal: Reversal::Recoverable,
        },
        // The palette's vocabulary is `Theme::NAMES` itself, on `console.patch`'s rule and for
        // its reason: it is the same table `Theme::resolve` refuses against and the same one
        // `bin/ctl.rs` builds its `--help` list from, so a fifth palette reaches all three
        // surfaces in the commit that adds it.
        // 🚨 **`edit` and `adjust` are values of this argument, not a verb of their own**, and
        // that is what makes the live editor completable for free: `/theme ` already lists this
        // `Choice`, so the two words narrow beside the palette names with no second table and
        // no change to §1.9's candidate machinery. A verb (`/themeedit`) would have needed its
        // own entry, its own doc and its own ring, to say a thing about the palette that
        // `/theme` is already the word for.
        CommandSpec {
            name: CMD_THEME.into(),
            doc: "Every colour the console paints. Live, and stored as a preference".into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: CMD_ARG.into(),
                kind: ArgKind::Choice(
                    Theme::NAMES
                        .iter()
                        .chain(organon_console::theme_edit::EDIT_WORDS.iter())
                        .map(|s| (*s).to_string())
                        .collect(),
                ),
                required: true,
            }],
            // ⚠️ Including `edit`/`adjust`: the colour editor is a band this pane opens and
            // Escape closes, not an element it appends. A palette is a preference, and every
            // preference here has an inverse.
            reversal: Reversal::Recoverable,
        },
        // ⚠️ **`ArgKind::Choice` and NOT the scalar, which is the one place this schema is
        // deliberately narrower than the CLI.** `Posture::resolve` also accepts a bare
        // `0.0`–`1.0`, and there is no `ArgKind` that says "one of these words, or a float in
        // this band" — `Choice` and `Float` are separate kinds. Offering `Float` instead would
        // lose the two words an agent actually wants; offering both slots would invent a
        // second spelling for one argument. The words are what a caller reaching for a posture
        // means, the scalar is for a hand exploring the axis, and the hand has a terminal.
        CommandSpec {
            name: CMD_POSTURE.into(),
            doc: "How the console holds itself: terminal-tight or desktop-open. Snaps".into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: CMD_ARG.into(),
                kind: ArgKind::Choice(
                    organon_console::posture::POSTURE_WORDS.iter().map(|s| (*s).to_string()).collect(),
                ),
                required: true,
            }],
            reversal: Reversal::Recoverable,
        },
        // 📌 **A `Choice` with no scalar beside it, which is the posture verb's caveat NOT
        // applying** — and the contrast is worth reading, because the two verbs sit next to
        // each other and look alike. A posture's value space is two words *or* a number, so
        // its schema is deliberately narrower than its CLI. A screen state has three words and
        // nothing between them: a window either covers the display or it does not. So here the
        // schema states the whole value space, and an agent gets the complete vocabulary.
        CommandSpec {
            name: CMD_SCREEN.into(),
            doc: "Whether the window covers the display. F11 flips it from inside".into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: CMD_STATE.into(),
                kind: ArgKind::Choice(
                    organon_console::screen::SCREEN_WORDS.iter().map(|s| (*s).to_string()).collect(),
                ),
                required: true,
            }],
            // ⚠️ The one that looks alarming and is not. A window that covered the display
            // uncovers it again with the opposite word — and F11 does it from inside without
            // any command at all, which is the definition of recoverable.
            reversal: Reversal::Recoverable,
        },
        // 📌 **Two `Choice`s, so the whole value space is stated and both rings complete** —
        // `screen`'s case rather than `posture`'s, and for the same reason: each table is a
        // closed list with nothing between its words. What the schema cannot say is the part
        // that depends on *state* — whether this region may hold this content given what the
        // console is holding right now — and that is not a shortcoming to be worked around: it
        // is [`Console::set_viewport`]'s job, because the layout is the console's and the
        // refusal it produces names the region that stood in the way.
        CommandSpec {
            name: CMD_VIEWPORT.into(),
            doc: "Divide the pane into regions and say what each one holds".into(),
            target: TargetKind::Viewport,
            args: vec![
                // 🚨 **A `ChoiceAliased`, and both halves are quoted from `region`'s own tables
                // rather than restated**: the ring is `REGION_WORDS` exactly as before, and the
                // short forms are `REGION_ALIASES`. So `/viewport tl panel` works in the
                // composer, `console.viewport` accepts `tl` over MCP, and neither surface
                // *lists* a thirteenth region word.
                //
                // ⚠️ **This said "the one `ChoiceAliased` in the catalog" until #98 Tier C**,
                // which gave `stack` a region slot of its own. The count is exactly the sort of
                // fact that goes quietly wrong, so it is not restated here — `region_slots_all
                // _accept_the_short_forms` enumerates the catalog and asserts the property
                // instead of the number.
                ArgSpec {
                    name: CMD_REGION.into(),
                    kind: ArgKind::ChoiceAliased {
                        words: organon_console::region::REGION_WORDS
                            .iter()
                            .map(|s| (*s).to_string())
                            .collect(),
                        aliases: organon_console::region::REGION_ALIASES
                            .iter()
                            .map(|(w, a)| ((*w).to_string(), (*a).to_string()))
                            .collect(),
                    },
                    required: true,
                },
                ArgSpec {
                    name: CMD_CONTENT.into(),
                    kind: ArgKind::Choice(
                        organon_console::region::CONTENT_WORDS.iter().map(|s| (*s).to_string()).collect(),
                    ),
                    required: true,
                },
            ],
            // ⚠️ **Recoverable, and the argument is `screen`'s exactly**: `viewport full agent`
            // restores the undivided console from any layout, in one command, and it is in the
            // same ring as everything that got you away from it. Nothing lands in the transcript
            // and nothing is remembered across a launch.
            //
            // 📌 That makes it *eligible* for autorun rather than a candidate for it: the rule
            // fires on a lone completion, and two rings of nine and three words never leave one.
            reversal: Reversal::Recoverable,
        },
        // 📌 **Two `Choice`s again, `viewport`'s arrangement exactly**, and for its reason: both
        // tables are closed lists with nothing between their words, so the whole value space is
        // stated and both rings complete. What the schema cannot say is again the part that
        // depends on **state** — whether a region holds a stack at all, and whether the column
        // is holding the panel a `remove` names — and that is [`Console::set_stack`]'s job,
        // because the layout and the stack are the console's and the refusal it produces names
        // what stood in the way.
        CommandSpec {
            name: CMD_STACK.into(),
            doc: "Add or remove one of Organon's editor panels in a region's scrolling stack"
                .into(),
            target: TargetKind::Viewport,
            args: vec![
                ArgSpec {
                    name: CMD_ACTION.into(),
                    kind: ArgKind::Choice(
                        organon_console::panel_stack::STACK_ACTIONS
                            .iter()
                            .map(|s| (*s).to_string())
                            .collect(),
                    ),
                    required: true,
                },
                ArgSpec {
                    name: CMD_PANEL.into(),
                    // `panels::slugs()` plus the clearing word, built by `panel_stack` rather
                    // than assembled here — a second concatenation is how the schema comes to
                    // offer a word the resolver does not know.
                    kind: ArgKind::Choice(
                        organon_console::panel_stack::panel_words()
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                    ),
                    required: true,
                },
                // 🚨 **The third slot, and it is OPTIONAL — #98 Tier C.** There is a column per
                // region now, so a command has to be able to say which; but the CLI door and
                // the MCP door have no region to be typed into, so requiring it would break two
                // of the four doors §1.8 says must stay one vocabulary. Absent, the console's
                // own destination rule answers exactly as it did before this word existed.
                //
                // ⚠️ **[`CMD_REGION`] is shared with `viewport`, deliberately**, and that is
                // [`CMD_ACTION`]'s rule read the same way: both slots are a region drawn from
                // one table, and a palette heading "region" describes both correctly. A second
                // name for one value space is how a schema comes to offer two rings over one
                // list.
                //
                // 🚨 **And `ChoiceAliased`, for exactly the reason the slot is shared** — #109
                // gave every region word its initials at all four front doors, and a slot that
                // named the same table while refusing `tl` would be a thirteenth region
                // vocabulary arriving as an oversight. One table, one set of short forms, both
                // quoted rather than restated.
                ArgSpec {
                    name: CMD_REGION.into(),
                    kind: ArgKind::ChoiceAliased {
                        words: organon_console::region::REGION_WORDS
                            .iter()
                            .map(|s| (*s).to_string())
                            .collect(),
                        aliases: organon_console::region::REGION_ALIASES
                            .iter()
                            .map(|(w, a)| ((*w).to_string(), (*a).to_string()))
                            .collect(),
                    },
                    required: false,
                },
            ],
            // 🚨 **Permanent, and the classification is argued rather than assumed.** Nothing
            // lands in the transcript, which is `viewport`'s case for `Recoverable` — but
            // `remove all` discards a column somebody assembled and **no single command
            // rebuilds it**, which is `block`'s case for the other column. The conservative
            // reading wins, and the practical effect is that autorun can never fire this verb.
            reversal: Reversal::Permanent,
        },
        // 📌 **One `Choice` and one declared `Text` whose ring depends on the word before it.**
        // The action ring is a table of three words. The name slot is `Text` because that is
        // what it is for `save` — a name a person is inventing, with no value space to state —
        // and `registry::layout_options` narrows it to the library for `load` and `delete`,
        // where the name must already exist. ⚠️ **The declared kind stays `Text` and must**:
        // it is what the MCP schema and `/help` say, and neither has the action word in hand.
        //
        // 🚨 **`save` is not narrowed, and that is the asymmetry the hook exists to express.**
        // Offering the existing names there would make a new name look invalid at the moment a
        // person is choosing one. See `registry::layout_options`, which owns the rule, and
        // §1.15 for what the read costs per keystroke — it was measured before it was wired.
        //
        // 🚨 **What the schema cannot say is again the part that depends on state** — whether a
        // layout of that name exists, whether it still resolves against this build's vocabulary,
        // and whether today's window can draw it. That is [`Console::set_layout`]'s job, through
        // `layout::resolve`, and it refuses by name with nothing changed.
        CommandSpec {
            name: CMD_LAYOUT.into(),
            doc: "Save the console's arrangement under a name, bring one back, or take one out"
                .into(),
            target: TargetKind::Viewport,
            args: vec![
                ArgSpec {
                    name: CMD_ACTION.into(),
                    kind: ArgKind::Choice(
                        organon_console::layout::LAYOUT_ACTIONS
                            .iter()
                            .map(|s| (*s).to_string())
                            .collect(),
                    ),
                    required: true,
                },
                ArgSpec { name: CMD_NAME.into(), kind: ArgKind::Text, required: true },
            ],
            // 🚨 **Permanent, and every one of the three actions earns it separately.** `delete`
            // takes a layout out of a file and nothing puts it back. `save` replaces whatever
            // was stored under that name, and nothing rebuilds the arrangement it replaced.
            // `load` is the one worth arguing: it puts nothing in the transcript, which is
            // `viewport`'s whole case for the other column — but what it *displaces* is the
            // arrangement that was on screen, and no second command restores that unless it too
            // was saved. `/viewport full agent` returns to the **default**, not to what you had.
            // So the answer to "can a second command put back what this displaced" is *only if
            // you had already saved it*, which is not the same as yes. The practical effect is
            // that autorun can never fire this verb — the right outcome for a verb that writes
            // to a file.
            reversal: Reversal::Permanent,
        },
        // ⚠️ `ArgKind::Int` is unbounded — `check_kind` only asks `as_i64`, so the schema
        // cannot express `1..=MAX_BLOCK_ROWS` the way a `Choice` expresses a table. The bound
        // therefore lives in TWO places that are both real gates rather than one that is
        // decorative: clap's `value_parser` range (which is where a human gets a good error,
        // before a byte is written) and [`op_from`] (which is where a hand-written sidecar
        // line meets it, and fails the dispatch with a record).
        CommandSpec {
            name: CMD_BLOCK.into(),
            doc: "Reserve a run of blank rows in the transcript".into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec { name: CMD_ROWS.into(), kind: ArgKind::Int, required: true }],
            // 🚨 The rows land in the transcript and no verb takes them out again. It
            // completes, and then it waits for an Enter.
            reversal: Reversal::Permanent,
        },
        CommandSpec {
            name: CMD_PATCH.into(),
            doc: "Claim a rectangle already left in the writer's own output".into(),
            target: TargetKind::Viewport,
            args: vec![
                ArgSpec { name: CMD_UP.into(), kind: ArgKind::Int, required: true },
                ArgSpec { name: CMD_ROWS.into(), kind: ArgKind::Int, required: true },
                ArgSpec {
                    name: CMD_KIND.into(),
                    kind: ArgKind::Choice(
                        kind::KIND_WORDS.iter().map(|s| (*s).to_string()).collect(),
                    ),
                    required: true,
                },
            ],
            // 🚨 It claims a rectangle of somebody else's output. Same reason as `block`, and
            // a worse mistake: the rectangle is measured from where the writer already is.
            reversal: Reversal::Permanent,
        },
        CommandSpec {
            name: CMD_PORTAL.into(),
            doc: "Open or close the portal — a live window onto the world, floating over the \
                  transcript"
                .into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: CMD_STATE.into(),
                kind: ArgKind::Choice(cli::PORTAL_WORDS.iter().map(|s| (*s).to_string()).collect()),
                required: true,
            }],
            // It floats *over* the transcript rather than landing in it, and `close` is right
            // there in the same ring.
            reversal: Reversal::Recoverable,
        },
        // 🚨 **The ranges are `scene_input`'s constants, not literals, and that is the whole
        // point of the arrangement.** `World::apply_camera_input` clamps a *hand* to the same
        // three numbers. A second copy here is how an agent comes to be refused a viewpoint the
        // drag can reach — or granted one it cannot — and either reads as the camera being
        // broken rather than as two constants disagreeing.
        //
        // ⚠️ Unlike `block`'s `Int`, `ArgKind::Float` **can** state its band, so `validate_args`
        // is the real gate here and `op_from` is a belt. Out of range fails the dispatch with a
        // record rather than clamping: a typed value that far out is a unit mistake far more
        // often than an overshoot, and a silent clamp lets the mistake look like it worked.
        CommandSpec {
            name: CMD_CAMERA.into(),
            doc: "Where the viewer stands: the portal's own yaw, pitch and distance".into(),
            target: TargetKind::Viewport,
            args: vec![
                ArgSpec {
                    name: CMD_RESET.into(),
                    kind: ArgKind::Bool,
                    required: false,
                },
                ArgSpec {
                    name: CMD_YAW.into(),
                    kind: ArgKind::Float {
                        min: -f64::from(scene_input::YAW_LIMIT),
                        max: f64::from(scene_input::YAW_LIMIT),
                    },
                    required: false,
                },
                ArgSpec {
                    name: CMD_PITCH.into(),
                    kind: ArgKind::Float {
                        min: -f64::from(scene_input::PITCH_LIMIT),
                        max: f64::from(scene_input::PITCH_LIMIT),
                    },
                    required: false,
                },
                ArgSpec {
                    name: CMD_DISTANCE.into(),
                    kind: ArgKind::Float {
                        min: f64::from(scene_input::DISTANCE_MIN),
                        max: f64::from(scene_input::DISTANCE_MAX),
                    },
                    required: false,
                },
            ],
            // A viewpoint, and `reset` is in its own ring. ⚠️ It is also the verb autorun can
            // reach fastest — `/camera reset` is a flag, so the line is whole the moment the
            // flag is taken — which is exactly the case the rule is happy to fire on: the
            // worst outcome is a framing, and the next framing replaces it.
            reversal: Reversal::Recoverable,
        },
    ]
}

/// What a **conversation tab's agent** is served as MCP tools: every console verb, plus the one
/// that only the in-process lane can answer.
///
/// 🚨 **Why the read is here and not in [`console_specs`].** `console_specs` is the *sidecar*
/// vocabulary: every entry has a `cli::ConsoleOp`, a line `cli::parse_console_op` reads back, and
/// a clap subcommand — that totality is what `op_from` and `every_op_round_trips_through_its_
/// catalog_name` depend on. A read has none of those and cannot, because
/// `organon console …` is fire-and-forget with **no return path** (`cli::console_cmd_path`'s
/// doc): a line written there produces no answer for anyone to collect. The MCP server, by
/// contrast, runs *inside this process* — [`ConsoleDispatch`] can simply hand back the console's
/// own state.
///
/// So the two sets differ by exactly the **reads**, and the difference is a fact about transports
/// rather than an oversight. Giving the CLI a read means building the request/reply sidecar
/// SHELL_ARCHITECTURE.md §2 names; it is not in scope here and is not quietly half-done.
///
/// ✏️ There are **two** reads now — the camera's and the layout library's. The second arrived for
/// the same reason and with one difference stated at its push site: it reads a file rather than
/// the frame path, so it is the read a future CLI could answer without any new transport at all.
///
/// ⚠️ **A separate verb, not a zero-argument spelling of `console.camera`.** Every axis on that
/// spec is already optional, so `{}` is a shape it can be called with — and it currently earns
/// the message *"needs at least one of […] — a framing that names no axis would move nothing"*,
/// which is the right answer to a model that forgot its arguments. Overloading it would turn
/// that mistake into a silent success returning something the caller did not ask for. It would
/// also give one tool two descriptions to be chosen by and one name for the approval layer to
/// judge, when a read and a write plainly deserve different answers to "may I?".
fn mcp_specs() -> Vec<CommandSpec> {
    let mut specs = console_specs();
    specs.push(CommandSpec {
        name: CMD_CAMERA_READ.into(),
        doc: "Where the viewer stands right now: the portal's yaw, pitch and distance as \
              measured this frame, who moved them last, and whether anything on screen is \
              showing them. Read this before framing a shot — `console.camera` is absolute, so \
              a relative move has to be computed from here"
            .into(),
        target: TargetKind::Viewport,
        // No arguments at all. The generated schema is `{"type":"object","properties":{},
        // "additionalProperties":true}` — see `mcp::input_schema`, which omits `required`
        // entirely rather than emitting an empty array.
        args: Vec::new(),
        // A read changes nothing, which is the cleanest case the rule has.
        reversal: Reversal::Recoverable,
    });
    // The second read, and it is here for the *same* reason rather than by analogy: a listing
    // has no answer on a channel with no return path. ⚠️ It differs from the camera's in one
    // way worth naming — the library is a **file**, so this read needs nothing from the running
    // console and a CLI could in principle answer it out of the same file. It does not today,
    // because the CLI has no spelling for a dotted verb and inventing a second one
    // (`console layouts`) would be one verb with two names. Meanwhile `layouts.json` is legible
    // by design, which is what makes that gap a gap rather than a wall.
    specs.push(CommandSpec {
        name: CMD_LAYOUT_LIST.into(),
        doc: "What is in the layout library: every saved arrangement, what each one holds, and \
              the file they live in. Read this before `console.layout load` — the names are \
              exact, and this is the only place they are listed"
            .into(),
        target: TargetKind::Viewport,
        // No arguments — `CMD_CAMERA_READ`'s shape, and its schema note applies verbatim.
        args: Vec::new(),
        reversal: Reversal::Recoverable,
    });
    specs
}

/// Catalog name ↔ sidecar op, both directions, in one place.
fn spec_name(op: &cli::ConsoleOp) -> &'static str {
    match op {
        cli::ConsoleOp::Background(_) => CMD_BACKGROUND,
        cli::ConsoleOp::Rig(_) => CMD_RIG,
        cli::ConsoleOp::Theme(_) => CMD_THEME,
        cli::ConsoleOp::Posture(_) => CMD_POSTURE,
        cli::ConsoleOp::Screen(_) => CMD_SCREEN,
        cli::ConsoleOp::Viewport { .. } => CMD_VIEWPORT,
        cli::ConsoleOp::Stack { .. } => CMD_STACK,
        cli::ConsoleOp::Layout { .. } => CMD_LAYOUT,
        cli::ConsoleOp::Block(_) => CMD_BLOCK,
        cli::ConsoleOp::Patch { .. } => CMD_PATCH,
        cli::ConsoleOp::Portal(_) => CMD_PORTAL,
        cli::ConsoleOp::Camera(_) => CMD_CAMERA,
    }
}

/// See [`spec_name`]. `Err` carries the message the target reports as an execution failure.
///
/// Two shapes of failure land here, and only one of them is a wiring bug. An unrecognised
/// catalog name cannot happen — the service's own `UnknownCommand` path excludes it — so that
/// arm is a belt on a brace. **A block row count out of range genuinely can**: `ArgKind::Int`
/// carries no bounds (see [`console_specs`]), and a line written straight onto the sidecar
/// never met clap's range. That one is a real gate, and it reports a real message.
fn op_from(name: &str, args: &Value) -> Result<cli::ConsoleOp, String> {
    let word = |slot: &str| -> Result<String, String> {
        args.get(slot)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("{name}: `{slot}` is missing or is not a string"))
    };
    match name {
        CMD_BACKGROUND => Ok(cli::ConsoleOp::Background(word(CMD_ARG)?)),
        CMD_RIG => Ok(cli::ConsoleOp::Rig(word(CMD_ARG)?)),
        // `resolve` rather than a bare pass-through, and the refusal is thrown away: what is
        // wanted here is the *check*, so a dispatch that names a palette this build cannot
        // paint fails with a record instead of succeeding and repainting nothing. Membership
        // was already settled by `validate_args` against the `Choice` above, so this is
        // `patch`'s belt — but it is the belt that catches a line reaching the service by any
        // route that skipped the schema.
        CMD_THEME => {
            let w = word(CMD_ARG)?;
            // 🚨 **The two editor words are legal in the schema and illegal on this lane**, and
            // refusing them here rather than leaving them out of the `Choice` is deliberate.
            // They have to be in the `Choice` or `Registry::resolve` would refuse `/theme edit`
            // during validation, before the conversation view ever sees it — and the view is
            // where the editor is drawn. They must be refused *here* because this is the
            // sidecar: the CLI and an agent's tool call both arrive on it, and neither has a
            // band above a composer to draw a dialog in. Silently doing nothing, or painting a
            // palette nobody asked for, are both worse than saying where the surface lives.
            if organon_console::theme_edit::is_edit_word(&w) {
                return Err(format!(
                    "{name}: `{w}` opens the live colour editor, which is a panel inside a \
                     conversation tab — type `/{}` there. This lane paints a named palette: {}",
                    format_args!("theme {w}"),
                    Theme::NAMES.join(", ")
                ));
            }
            Theme::resolve(&w).map_err(|e| format!("{name}: {e}"))?;
            Ok(cli::ConsoleOp::Theme(w))
        }
        CMD_POSTURE => {
            let w = word(CMD_ARG)?;
            organon_console::posture::Posture::resolve(&w).map_err(|e| format!("{name}: {e}"))?;
            Ok(cli::ConsoleOp::Posture(w))
        }
        // `resolve` and the answer thrown away, on `CMD_THEME`'s rule: membership was settled
        // by `validate_args` against the `Choice` above, so this is the belt that catches a
        // line reaching the service by a route that skipped the schema.
        CMD_SCREEN => {
            let w = word(CMD_STATE)?;
            organon_console::screen::ScreenCmd::resolve(&w).map_err(|e| format!("{name}: {e}"))?;
            Ok(cli::ConsoleOp::Screen(w))
        }
        // Both words resolved and both answers thrown away, on `CMD_SCREEN`'s rule: membership
        // was settled by `validate_args` against the region's `ChoiceAliased` and the content's
        // `Choice`, so this is the belt that catches a call reaching the service by a route that
        // skipped the schema.
        //
        // ⚠️ **The region word may be a short form here, and it stays one.** `Region::resolve`
        // accepts `tl`, and the op is built from `r` as typed rather than from what it resolved
        // to — the sidecar line is the same line the CLI would have written for the same
        // command, and expanding it here would make the two doors disagree in the session log.
        //
        // 🚨 **What is deliberately NOT checked here is whether the assignment is legal.** That
        // depends on the layout the console is holding *at the moment the op is drained*, and
        // this function runs at dispatch — a gate here would be answering with a layout that may
        // have moved by the time the op lands. The one gate is [`Console::set_viewport`], which
        // has the layout in hand and refuses by name.
        CMD_VIEWPORT => {
            let r = word(CMD_REGION)?;
            let c = word(CMD_CONTENT)?;
            organon_console::region::Region::resolve(&r).map_err(|e| format!("{name}: {e}"))?;
            organon_console::region::ContentCmd::resolve(&c).map_err(|e| format!("{name}: {e}"))?;
            Ok(cli::ConsoleOp::Viewport { region: r, content: c })
        }
        // Both words resolved and the answer thrown away, on `CMD_VIEWPORT`'s rule: membership
        // was settled by `validate_args` against the two `Choice`s, so this is the belt that
        // catches a call reaching the service by a route that skipped the schema.
        //
        // 🚨 **What is deliberately NOT checked here is whether the column can honour it** —
        // whether any region holds a stack, and whether it is holding the panel a `remove`
        // names. Both are facts about state at the moment the op is *drained*, and this runs at
        // dispatch. [`Console::set_stack`] is the one gate, and it refuses by name.
        CMD_STACK => {
            let a = word(CMD_ACTION)?;
            let p = word(CMD_PANEL)?;
            organon_console::panel_stack::StackCmd::resolve(&a, &p)
                .map_err(|e| format!("{name}: {e}"))?;
            // The optional third word. **Resolved and the answer thrown away**, exactly as the
            // two required ones are: membership was settled by `validate_args` against the
            // `Choice`, so this is the belt for a call that skipped the schema. ⚠️ What is
            // deliberately NOT checked is whether that region *holds a panel stack* — a fact
            // about the layout at drain time, which `Console::set_stack` is the one gate on.
            let r = args.get(CMD_REGION).and_then(|v| v.as_str()).map(str::to_string);
            if let Some(word) = &r {
                organon_console::region::Region::resolve(word)
                    .map_err(|e| format!("{name}: {e}"))?;
            }
            Ok(cli::ConsoleOp::Stack { action: a, panel: p, region: r })
        }
        // 🚨 **The name check is a REAL gate here, not a belt** — the one difference from the two
        // arms above, and the reason is the transport. `ArgKind::Text` states no value space, so
        // `validate_args` has nothing to check; a name with a space in it would then be written
        // onto a whitespace-delimited sidecar line and arrive at the console truncated, having
        // saved or deleted something nobody named. `check_name` is where a caller learns that,
        // with a record, instead of watching a command appear to work.
        //
        // 🚨 **What is deliberately NOT checked here is whether the layout exists, still
        // resolves, or fits today's window.** All three are facts about state at the moment the
        // op is *drained* — the library may be written between now and then, and the window may
        // be resized. [`Console::set_layout`] is the one gate, and it refuses by name.
        CMD_LAYOUT => {
            let a = word(CMD_ACTION)?;
            let n = word(CMD_NAME)?;
            organon_console::layout::LayoutCmd::resolve(&a).map_err(|e| format!("{name}: {e}"))?;
            organon_console::layout::check_name(&n).map_err(|e| format!("{name}: {e}"))?;
            Ok(cli::ConsoleOp::Layout { action: a, name: n })
        }
        CMD_BLOCK => {
            let n = args
                .get(CMD_ROWS)
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("{name}: `{CMD_ROWS}` is missing or is not an integer"))?;
            match u16::try_from(n) {
                Ok(rows) if (1..=cli::MAX_BLOCK_ROWS).contains(&rows) => {
                    Ok(cli::ConsoleOp::Block(rows))
                }
                _ => Err(format!(
                    "{name}: `{CMD_ROWS}` must be 1..={}, got {n}",
                    cli::MAX_BLOCK_ROWS
                )),
            }
        }
        CMD_PATCH => {
            let int = |slot: &str| -> Result<i64, String> {
                args.get(slot)
                    .and_then(Value::as_i64)
                    .ok_or_else(|| format!("{name}: `{slot}` is missing or is not an integer"))
            };
            let up = int(CMD_UP)?;
            let rows = int(CMD_ROWS)?;
            // The kind is checked here as well as by the schema, and unlike the row range it
            // is a belt rather than a brace: `ArgKind::Choice` *can* state this vocabulary, so
            // membership was already settled by `validate_args`. What this adds is the
            // conversion — a word the schema accepts must still resolve to something this
            // build can paint, and the two lists are the same list.
            //
            // `Kind::resolve` rather than `from_word`, because there is a human (or an agent
            // reading an error) on this end: the refusal it returns carries the known list,
            // which is the difference between "no" and "no, and here is what would work". The
            // sidecar drain uses `from_word` for the opposite reason — nobody is listening
            // there, so an unknown kind skips the line in silence.
            let kind = args
                .get(CMD_KIND)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{name}: `{CMD_KIND}` is missing or is not a string"))
                .and_then(|word| {
                    kind::Kind::resolve(word).map_err(|e| format!("{name}: `{CMD_KIND}` — {e}"))
                })?;
            match (u16::try_from(up), u16::try_from(rows)) {
                (Ok(up), Ok(rows))
                    if (1..=cli::MAX_BLOCK_ROWS).contains(&rows) && up <= cli::MAX_BLOCK_ROWS =>
                {
                    Ok(cli::ConsoleOp::Patch { up, rows, kind })
                }
                _ => Err(format!(
                    "{name}: `{CMD_UP}` must be 0..={} and `{CMD_ROWS}` 1..={}, got {up} and {rows}",
                    cli::MAX_BLOCK_ROWS,
                    cli::MAX_BLOCK_ROWS
                )),
            }
        }
        // Membership was settled by `validate_args` against the `Choice` — this is the
        // conversion, and it is a belt exactly as `patch`'s kind is: a word the schema accepts
        // must still resolve to something this build can act on, from the same one table.
        CMD_PORTAL => args
            .get(CMD_STATE)
            .and_then(Value::as_str)
            .and_then(cli::PortalCmd::from_word)
            .map(cli::ConsoleOp::Portal)
            .ok_or_else(|| {
                format!("{name}: `{CMD_STATE}` must be one of {:?}", cli::PORTAL_WORDS)
            }),
        // Every slot's *range* was already settled by `validate_args` against the `Float`
        // bands above — this is the one thing the schema cannot say: **at least one axis**.
        // `ArgSpec::required` is per-argument, so "all optional, but not all absent" has no
        // spelling in it, and a framing that names nothing would otherwise dispatch, succeed,
        // and move nothing.
        CMD_CAMERA => {
            let float = |slot: &str| -> Result<Option<f32>, String> {
                match args.get(slot) {
                    None | Some(Value::Null) => Ok(None),
                    Some(v) => v
                        .as_f64()
                        .map(|f| Some(f as f32))
                        .ok_or_else(|| format!("{name}: `{slot}` is not a number")),
                }
            };
            let framing = cli::CameraFraming {
                reset: args.get(CMD_RESET).and_then(Value::as_bool).unwrap_or(false),
                yaw: float(CMD_YAW)?,
                pitch: float(CMD_PITCH)?,
                distance: float(CMD_DISTANCE)?,
            };
            if framing.is_empty() {
                return Err(format!(
                    "{name}: needs at least one of {:?} — a framing that names no axis \
                     would move nothing",
                    cli::CAMERA_WORDS
                ));
            }
            // The belt to `validate_args`' brace: `f64 as f32` can round a value that passed
            // the band into one just outside it, and a NaN would pass `as_f64` and then poison
            // the view matrix. Cheap, and it is the last gate before `World`.
            if !framing.in_range() {
                return Err(format!("{name}: an axis is out of range or not finite"));
            }
            Ok(cli::ConsoleOp::Camera(framing))
        }
        _ => Err(format!("no console op for {name:?}")),
    }
}

/// The op's payload, as the dispatch arguments its spec declares.
///
/// A `Value` rather than a `&str` since Tier 5: `block`'s argument is a **number**, and
/// spelling it as a string would pass `ArgKind::Int`'s `as_i64` check to nobody's benefit —
/// it would simply fail validation, one lane's type error reported as another's.
fn op_args(op: &cli::ConsoleOp) -> Value {
    match op {
        cli::ConsoleOp::Background(n)
        | cli::ConsoleOp::Rig(n)
        | cli::ConsoleOp::Theme(n)
        | cli::ConsoleOp::Posture(n) => json!({ CMD_ARG: n }),
        cli::ConsoleOp::Block(rows) => json!({ CMD_ROWS: rows }),
        cli::ConsoleOp::Patch { up, rows, kind } => {
            json!({ CMD_UP: up, CMD_ROWS: rows, CMD_KIND: kind.as_word() })
        }
        cli::ConsoleOp::Portal(cmd) => json!({ CMD_STATE: cmd.as_word() }),
        // `CMD_STATE`, with the portal rather than with the four `CMD_ARG` verbs above, and
        // the slot name is the schema's — `console_specs` declares it, so the two must agree
        // or `validate_args` refuses every dispatch this produces. Both verbs name a *state*
        // rather than a thing; a screen state is a `String` here because that is what crossed
        // the lane, and it has already been resolved once by `op_from`.
        cli::ConsoleOp::Screen(word) => json!({ CMD_STATE: word }),
        // Two slots of its own rather than either name above — `console_specs` declares them,
        // so the two must agree or `validate_args` refuses every dispatch this produces.
        cli::ConsoleOp::Viewport { region, content } => {
            json!({ CMD_REGION: region, CMD_CONTENT: content })
        }
        // Two more slots of its own, for the reason directly above — plus the optional region.
        // ⚠️ **Spelled as `null` when absent** rather than omitted, on the camera's rule and for
        // its reason: `validate_args` reads a present `null` and a missing key the same way for
        // an optional argument, and `op_from` maps both to `None`, so what this buys is that a
        // reader of `events.jsonl` sees the whole slot list rather than whichever subset was set.
        cli::ConsoleOp::Stack { action, panel, region } => {
            json!({ CMD_ACTION: action, CMD_PANEL: panel, CMD_REGION: region })
        }
        // ⚠️ **`CMD_ACTION` is shared with `stack` and `CMD_NAME` is its own**, which is the
        // slot-naming rule applied in both directions at once: the two verbs' action rings are
        // the same *kind* of slot and a palette heading "action" describes both, while a layout
        // name is free text and is not the `name` that means a material.
        cli::ConsoleOp::Layout { action, name } => {
            json!({ CMD_ACTION: action, CMD_NAME: name })
        }
        // `null` for an axis nobody named, which `validate_args` reads as absent for an
        // optional argument and `op_from` maps straight back to `None`. Omitting the key
        // entirely would do the same thing; spelling it keeps the dispatch record — which is
        // what a reader of `events.jsonl` sees — showing the whole slot list rather than
        // whichever subset happened to be set.
        //
        // ⚠️ **That first clause was aspirational when it was written, and this verb is why
        // it is now true.** `validate_args` was written against required-only schemas, so a
        // present `null` hit its `Some(value)` arm and `ArgKind::Float`'s `as_f64` refused it
        // — every partial framing, `--distance 40` included, rejected before the target. The
        // fix is in `command.rs`, where the contract belongs, rather than here: omitting the
        // keys would have unbroken this one caller and left the next optional argument to
        // find the same trap. Never assume a comment describing a collaborator's behaviour
        // has a test behind it; the one that would have caught this had to be written first.
        cli::ConsoleOp::Camera(f) => json!({
            CMD_RESET: f.reset,
            CMD_YAW: f.yaw,
            CMD_PITCH: f.pitch,
            CMD_DISTANCE: f.distance,
        }),
    }
}

/// Where a **validated** console command lands: a bank the drain reads after the service is
/// gone.
///
/// It banks rather than applies because of a real constraint, not a preference.
/// `CommandService::register_target` takes `Box<dyn CommandTarget>` — implicitly `'static` —
/// so a target cannot hold `&mut Console`, and the apply needs one. `MockTarget`'s
/// `Rc<RefCell<…>>` shape is the answer already in the file: the target keeps one handle, the
/// caller keeps another, and the caller reads the bank once the borrow of the session log has
/// ended.
///
/// What this arrangement buys is that **the op that gets applied is the op the service
/// returned**, not a parallel copy of it — dispatch is in the path, not beside it.
#[derive(Clone, Default)]
struct ConsoleTarget {
    accepted: Rc<RefCell<Vec<cli::ConsoleOp>>>,
}

impl CommandTarget for ConsoleTarget {
    fn execute(&mut self, name: &str, args: &Value) -> Result<Value, CommandError> {
        // Presence, type and — for the two `Choice` slots — membership were all checked by
        // `validate_args` against the schema above, so a miss on those is a wiring bug. The
        // one thing the schema cannot state is `block`'s row range (`ArgKind::Int` has no
        // bounds), so [`op_from`] is a real gate as well as a belt. Either way the failure is
        // reported as an execution error — which still leaves a `Failed` record — rather than
        // panicking inside a redraw.
        match op_from(name, args) {
            Ok(op) => {
                self.accepted.borrow_mut().push(op);
                Ok(args.clone())
            }
            Err(message) => {
                Err(CommandError::Execution { command: name.to_string(), message })
            }
        }
    }
}

/// **The console's verbs, reachable from inside the process the agent is already living
/// in** — the [`ToolDispatch`] behind every capability tool a conversation tab serves.
///
/// 🚨 **The bug this closes.** Until this existed the console's MCP server was built with an
/// empty spec table, so an agent that wanted to open the portal had to run
/// `./organon.exe console portal open` through `Bash` — spawning a whole second process to
/// send a message to the one it was inside — and the approval card asked *"may I run this
/// shell command"* instead of naming a capability. Everything else was already here.
///
/// ⚠️ **It writes onto the console's own sidecar rather than applying anything**, and that
/// is the point rather than a shortcut. `Console::drain_console` already reads that file every
/// frame, routes each line through the real [`CommandService`] — which validates against the
/// same [`CommandSpec`] this tool's schema was generated from, and leaves a `CommandRun`
/// record either way — and then applies it. Anything else would be a second apply path
/// beside the audited one. The CLI and the tool now converge on one transport; what the tool
/// removes is the process, not the discipline.
///
/// ⚠️ **So the tool returns "accepted", not "applied".** The op lands on the next frame
/// (~16 ms), and a failure *after* validation here — a name this build cannot paint — is
/// reported on stderr by the drain rather than to the model. Returning a promise the caller
/// can read as a result is the honest cost of reusing the audited path; the alternative was
/// blocking an MCP call on the UI thread's next frame.
///
/// ⚠️ It is `Send` and holds only the read cell: it runs on the MCP transport's serve thread,
/// and the sidecar's path is derived per call from the IPC namespace ([`cli::console_cmd_path`])
/// so two consoles in two namespaces cannot write to each other's.
///
/// # The two lanes, and why one of them is not the sidecar
///
/// **A write goes out** onto the sidecar as above. **A read is answered here**, from
/// [`camera::ViewpointCell`] — the snapshot [`Console::redraw`] publishes each frame. It cannot use
/// the sidecar for the reason the whole read path exists: that transport has no return channel,
/// so there is nowhere for an answer to come back to. Being *inside* the console process is the
/// entire advantage this lane has over the CLI, and the read is what spends it.
struct ConsoleDispatch {
    /// The console's live viewpoint, published once per frame. Shared with [`Console`], never
    /// copied — a second copy is how a read comes to report something the camera never held.
    viewpoint: camera::ViewpointCell,
}

impl organon_console::mcp::ToolDispatch for ConsoleDispatch {
    fn call(&mut self, command: &str, args: Value) -> Result<Value, String> {
        // The read first, because it is the one verb with no `ConsoleOp` — `op_from` would
        // refuse it, correctly, as a name the sidecar has no line for.
        if command == CMD_CAMERA_READ {
            // 🚨 `None` is answered as a *failure*, not as an empty object or a zeroed framing.
            // The console has genuinely not measured anything yet (no frame has been drawn), and
            // a caller that receives `{"yaw":0,…}` has no way to tell that apart from a camera
            // at the origin. An omitted answer beats an invented one.
            return match self.viewpoint.read() {
                Some(v) => Ok(v.report(Instant::now())),
                None => Err(format!(
                    "{command}: the console has not drawn a frame yet, so no viewpoint has been \
                     measured. Ask again once the window is up."
                )),
            };
        }
        // The second read, and the one that needs nothing from the console at all: the library
        // is a file. It is answered here rather than on the sidecar for the camera read's
        // reason — that channel has no return path — and it is re-read per call rather than
        // cached, on [`Console::set_layout`]'s rule: the file is the truth, and a cached copy
        // would fight a hand-edited one and win silently.
        if command == CMD_LAYOUT_LIST {
            use organon_console::layout::{Library, LAYOUTS_FILE, NOTHING_SAVED};
            // 🚨 A missing data directory is a *failure*, not an empty library — an empty
            // answer would say "you have saved nothing" to somebody whose layouts are simply
            // unreachable. `camera.read`'s rule: an omitted answer beats an invented one.
            let Some(root) = Library::store_root() else {
                return Err(format!(
                    "{command}: this platform has no data directory, so there is nowhere for \
                     layouts to be stored. Nothing has been lost — nothing was ever written."
                ));
            };
            let library = Library::load(&root);
            let mut out = json!({
                "file": root.join(LAYOUTS_FILE).display().to_string(),
                "count": library.layouts.len(),
                "layouts": library
                    .layouts
                    .iter()
                    .map(|l| json!({ "name": l.name, "regions": l.regions }))
                    .collect::<Vec<_>>(),
            });
            if library.layouts.is_empty() {
                // An empty list is a true answer and a useless one on its own — it does not say
                // whether the library is empty or the file is somewhere else. The note names
                // the verb that fills it, and `file` above names the file.
                //
                // 🚨 **The sentence lives in `layout` because a second surface now says it**:
                // `/layout load ` with nothing saved answers `Ring::Empty` carrying this exact
                // string. Two copies would be two answers to one question. See
                // [`organon_console::layout::NOTHING_SAVED`].
                out["note"] = json!(NOTHING_SAVED);
            }
            return Ok(out);
        }
        // The same conversion the sidecar drain performs, from the same one place — so a
        // tool call and a `organon console …` line cannot come to mean different things.
        // This is also where `block`'s row range is caught, since `ArgKind::Int` carries no
        // bounds and the generated schema therefore cannot state it.
        let op = op_from(command, &args)?;
        let line = cli::console_op_to_line(&op);
        cli::append_console_ops(std::slice::from_ref(&op))
            .map_err(|e| format!("{command}: could not reach the console's command channel: {e}"))?;
        Ok(json!({ "accepted": line }))
    }
}

/// One console op applied to the `(source, look)` pair — **pure**, so the entire command
/// vocabulary is a test rather than a claim about a `Console` that needs a window server.
///
/// `None` = the op named nothing this console knows, and **nothing changes**. That is the
/// forward-compatibility contract of `cli::parse_console_op` ("an unknown verb is skipped,
/// not fatal") carried one level down to the argument: a newer CLI naming a material this
/// build does not have must leave the backdrop exactly as it found it.
///
/// The two asymmetries are deliberate:
///
/// * A **material** name implies its source. `background graphite` means "put graphite
///   behind the glyphs", and requiring `background substrate` first would be a mode to
///   remember. So a material sets `Substrate` as well as itself.
/// * A **rig** never touches the source, and is remembered even at `world`/`off` where it
///   draws nothing. `rig daylight` then `background substrate` does what it reads like.
fn console_step(
    source: BackdropSource,
    look: &ConsoleLook,
    op: &cli::ConsoleOp,
) -> Option<(BackdropSource, ConsoleLook)> {
    let mut source = source;
    let mut look = look.clone();
    match op {
        cli::ConsoleOp::Background(name) => match console_source(name) {
            Some(src) => {
                source = src;
                // The bare source word needs *a* material to name; every other spelling
                // carries its own. See [`CONSOLE_DEFAULT_MATERIAL`].
                if src == BackdropSource::Substrate && look.material.is_none() {
                    look.material = Some(CONSOLE_DEFAULT_MATERIAL.to_string());
                }
            }
            None => {
                let m = canonical(&substrate_materials::MATERIAL_NAMES, name)?;
                source = BackdropSource::Substrate;
                look.material = Some(m.to_string());
            }
        },
        cli::ConsoleOp::Rig(name) => {
            look.rig = Some(canonical(&substrate_materials::RIG_NAMES, name)?.to_string());
        }
        // **A block is not a look.** It reserves rows in one pane's transcript and touches
        // neither the backdrop source nor the dressing, so it has no `(source, look)` to fold
        // into and never reaches here: [`Console::apply_console`] routes it first. `None` is
        // the honest answer for a resolver whose whole domain is looks — and it is also the
        // safe one, since `None` here means "changed nothing".
        //
        // **A portal is not a look either, and its case is stronger.** It does change what the
        // engine draws for the *backdrop* — an open portal takes the frame, see [`engine_plan`]
        // — but it does that by being consulted at render time, not by writing
        // `backdrop_source`. Folding it in here would make closing a portal restore whatever
        // source it had overwritten, which is one remembered value more than the feature needs.
        //
        // **A camera move is not a look at all.** It writes host state on the `World` — the
        // same three fields a drag writes — which travels in no snapshot, is saved in no
        // preset, and is not the console's dressing in any sense. It never reaches here either.
        //
        // **A palette and a posture are the console's own dressing, not the substrate's**, and
        // this is the distinction the two verbs exist to make: `background` and `rig` say what
        // is *behind* the glyphs, these two say what the glyphs and their chrome are made of
        // and how they are arranged. Neither writes `backdrop_source`, neither changes a pixel
        // of the substrate, and neither belongs in the Tier-4 epoch ledger this function's
        // caller feeds — a band of scrollback records what was behind it, and nothing behind
        // it moved. `Console::apply_console` routes both first, for that reason in full.
        //
        // **A screen state is not a look either, and it is the furthest of all of them from
        // being one**: it changes nothing the console *draws*. It resizes the window and lets
        // the next frame lay out into whatever rectangle it was given — the substrate behind
        // the glyphs is re-rendered at a new size, wearing the identical dressing. Banding the
        // transcript here would mark a look change at a moment the look demonstrably did not
        // change.
        //
        // **A division of the pane is not a look, and it is the case most likely to be argued
        // the other way** — a split does change what is on the screen, dramatically. But what it
        // changes is *how many rectangles the glyphs are drawn into*, not what is behind them:
        // the backdrop is still rendered once, at the whole pane's size, wearing the identical
        // dressing, and every region is drawn over the same picture. So banding the transcript
        // here would mark a look change at a moment the look did not change — `screen`'s
        // argument exactly, one level in.
        cli::ConsoleOp::Block(_)
        | cli::ConsoleOp::Patch { .. }
        | cli::ConsoleOp::Portal(_)
        | cli::ConsoleOp::Camera(_)
        | cli::ConsoleOp::Theme(_)
        | cli::ConsoleOp::Posture(_)
        | cli::ConsoleOp::Screen(_)
        // **And what is inside a `panel` region is not a look either** — one step further in
        // than the split above. It changes which cards are drawn in a rectangle the glyphs
        // already owned; the backdrop behind that rectangle is the same picture, wearing the
        // same dressing, rendered once.
        | cli::ConsoleOp::Viewport { .. }
        | cli::ConsoleOp::Stack { .. }
        // **And a saved arrangement is a recording of the split above, so it is not a look
        // either** — for the split's reason exactly. What `load` changes is how many rectangles
        // the glyphs are drawn into; the backdrop behind them is still rendered once, at the
        // whole pane's size, wearing the identical dressing. `save` and `delete` do not change
        // even that: they write a file.
        | cli::ConsoleOp::Layout { .. } => return None,
    }
    Some((source, look))
}

/// The engine's frame behind the glyphs: sized to the **pane it is painted into** (not the
/// window — see [`Console::render_backdrop`]), recreated when that size changes, rebound to the
/// same egui id (`register_scene_texture`'s no-leak discipline).
struct Backdrop {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    id: Option<egui::TextureId>,
}

/// One **closed** look-epoch's picture: the live backdrop as it stood the instant that look
/// stopped being live (Console Spike Tier 4).
///
/// The same `(texture, view, id)` triple as [`Backdrop`], with two differences that follow
/// from it being history rather than a render target: it is `COPY_DST` instead of
/// `RENDER_ATTACHMENT` (nothing ever draws into it again), and it carries **no size**,
/// because nothing needs one — the pixels are frozen at whatever the pane was when the copy
/// was taken, and [`term_view::band_quads`] samples in UV fractions, so a later resize
/// stretches rather than mismatches. See [`Console::snapshot_live_backdrop`] for why that is
/// honest rather than lazy.
///
/// Shared by `Rc` across tabs: a look change closes an epoch in *every* pane's ledger, and
/// the picture they are closing is the same one picture — one backdrop, one window. Each pane
/// keys it by its own [`EpochId`], so the id spaces stay independent while the GPU pays once.
struct CachedEpoch {
    #[allow(dead_code)] // the texture must outlive the view and the egui registration
    texture: wgpu::Texture,
    #[allow(dead_code)] // …and the view must outlive the registration
    view: wgpu::TextureView,
    id: egui::TextureId,
}

// ---------------------------------------------------------------------------
// Rendered surfaces in a conversation
// ---------------------------------------------------------------------------

/// The most surface textures that may exist at once, across every tab.
///
/// 🚨 **A cap from the first line of this feature, not a later hardening**, for
/// `substrate_epochs`' reason: a transcript grows without bound, every `/surface` adds an
/// element that keeps its id forever, and a texture per surface with nothing bounding it is a
/// leak whose only symptom is the machine getting slower. Four, because that is what the
/// screen can hold: a surface is [`conversation_view`]'s 260 pt tall plus its panel, so at the
/// default 1100×720 window two are visible at a stretch and four covers "two on screen and
/// two just scrolled past" — the case where evicting would be felt as a flicker.
///
/// The ceiling it buys is [`surface_budget_bytes`]. At the pane sizes this console actually
/// runs (2475×1553 physical on ORGANON-ONE, a surface being roughly the full width by 260 pt
/// ≈ 585 px) that is about **23 MB**, against the backdrop's own ~15 MB and the epoch cache's
/// [`substrate_epochs::MAX_EPOCHS`] × pane. Every eviction prints one `[surface]` line naming
/// what went and why — a silently dropped texture reads as "the picture is still there",
/// which is exactly the failure this repo keeps paying for.
const MAX_SURFACE_TEXTURES: usize = 4;

/// How many surfaces the engine may be asked to draw in one frame.
///
/// ⚠️ **This is the double-step bound, and it is the reason the number is one.** Each surface
/// render is a whole `World` frame — `render_to_texture` runs the same generators, passes and
/// post chain as the backdrop — so N surfaces mean N extra frames of engine work *and* N
/// extra advances of everything in the world that is per-frame rather than per-second.
///
/// What is genuinely at risk is narrower than "the world runs at N×", and worth stating
/// exactly because the vague version invites the wrong fix: the beat clock, the camera and
/// every sim in `frame_body` advance by a **wall-clock `dt`** (`world.rs`'s
/// `now - self.last_frame`), so a second render microseconds after the first advances them by
/// microseconds — invisible. What double-steps is what counts *frames*: `frame_index`, which
/// drives the TAA jitter phase, and the temporal history that goes with it. Those are shared
/// between the two targets, so a surface and the backdrop rendered in one frame trade jitter
/// phases. On the still lit plane this feature draws, that is not visible. On a moving World
/// it would be, intermittently — the worst kind — which is why the surface look is the
/// substrate and not the World, and why this is a documented cut rather than a hidden one.
///
/// One render per frame also means a **dirty** surface repaints at full rate while a hand is
/// on its slider, and a settled one costs nothing at all: the budget is only ever spent on a
/// look that has actually changed.
const SURFACE_RENDERS_PER_FRAME: usize = 1;

/// The GPU ceiling [`MAX_SURFACE_TEXTURES`] buys, in bytes, for a given surface size —
/// `substrate_epochs::worst_case_bytes`' arrangement, so the cap's cost is a number a test
/// can quote rather than a claim in prose.
const fn surface_budget_bytes(w: u32, h: u32) -> u64 {
    (MAX_SURFACE_TEXTURES as u64) * (w as u64) * (h as u64) * 4
}

/// What a surface is a picture *of*: the console look, plus the knob positions on top of it.
///
/// Compared for equality to decide whether a surface needs redrawing, which is the whole
/// reason it is a value rather than a pile of fields — a surface whose look has not changed
/// is not re-rendered, and that is what makes an idle conversation cost zero engine frames.
#[derive(Clone, Debug, Default, PartialEq)]
struct SurfaceLook {
    look: ConsoleLook,
    /// `(label, value)` exactly as the driving panel reported them. Carried rather than
    /// resolved so the comparison happens on what the hand did, not on the floats it
    /// happened to produce after a mapping that may change.
    sliders: Vec<(String, f32)>,
}

/// One surface's render target and its egui registration.
///
/// The [`Backdrop`] triple with two additions: the look it currently *holds* (so a frame can
/// tell a stale picture from a current one) and a stamp (so the cap can evict the
/// least-recently-asked-for).
struct SurfaceTexture {
    #[allow(dead_code)] // the texture must outlive the view and the egui registration
    texture: wgpu::Texture,
    #[allow(dead_code)] // …and the view must outlive the registration
    view: wgpu::TextureView,
    id: egui::TextureId,
    size: (u32, u32),
    /// What is actually drawn in it. `None` on the frame it was created or resized, which is
    /// what makes "needs a render" a single comparison rather than a flag plus a comparison.
    holds: Option<SurfaceLook>,
    /// The frame this texture was last asked for. The cap's eviction order and nothing else.
    touched: u64,
}

/// A surface, identified across the whole console.
///
/// **Keyed by pane as well as element**, because an [`ElementId`] is only unique within one
/// transcript: two conversation tabs both start at id 0, and a bare-id map would have them
/// painting into each other's textures the moment both had a surface open.
type SurfaceKey = (usize, ElementId);

/// One exhibit item, by element and index into that exhibit's items.
type ExhibitKey = (ElementId, usize);

/// How many exhibit pictures may hold a texture at once.
///
/// The same figure as [`MAX_SURFACE_TEXTURES`] and deliberately a *separate* ceiling rather
/// than a share of it. The two ledgers are keyed differently and fill differently — surfaces
/// are summoned one at a time, an exhibit can arrive with several items in one command — and a
/// single pooled cap would let a three-item gallery evict the surface a panel is driving. What
/// they share is the *policy*, which is why `surfaces_to_evict` is generic over the key.
const MAX_EXHIBIT_TEXTURES: usize = 4;

/// How much document text may be held across all exhibits, in bytes.
///
/// 🚨 **Documents are budgeted too, and by bytes rather than by count — the review of #86 found
/// this missing.** The first cut capped only pictures, on the reasoning that a `String` costs no
/// GPU. That is true and beside the point: a document that is never evicted is held for the rest
/// of the session, so a long conversation that opened a dozen READMEs keeps every one of them
/// alive behind cards nobody can see any more. A *count* would have been the wrong instrument
/// here (one 8 MB document and eight 2 KB ones are not alike), which is why this is the one
/// ledger measured in bytes.
///
/// 4 MB is many books' worth of Markdown and still an order of magnitude under one picture.
const MAX_DOCUMENT_BYTES_HELD: usize = 4 * 1024 * 1024;

/// The largest file the loader will open, in bytes.
///
/// 🚨 **Checked before the decode, not after**, which is the whole point: a decoder asked for
/// a 500 MB PNG allocates its full pixel buffer before anything can object, and the console
/// dies of an allocation failure rather than saying no. A stat is cheap and a refusal is
/// legible.
const MAX_EXHIBIT_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// The longest edge a picture is scaled to before it becomes a texture.
///
/// Two reasons, both real. A phone photograph is routinely 4000 px on its long edge and would
/// be a 64 MB texture at full size, four of which is a quarter of a gigabyte against a budget
/// that prices its conversation surfaces at ~23 MB. And the card it is drawn in is a few
/// hundred points tall (`conversation_view::EXHIBIT_HEIGHT`), so everything past this is
/// resolution nobody can see.
const MAX_EXHIBIT_EDGE: u32 = 2048;

/// What a loader thread hands back.
enum ExhibitLoad {
    /// Decoded, already scaled to fit [`MAX_EXHIBIT_EDGE`], as tightly-packed RGBA8.
    Picture { size: (u32, u32), rgba: Vec<u8> },
    /// A document's source text, already an `Arc` so the frame path never deep-copies it.
    Document(std::sync::Arc<str>),
    /// A sentence for the person who typed the path.
    Failed(String),
}

/// Read one exhibit item **off the frame thread**, and never panic doing it.
///
/// 🚨 **Every failure arm names the file.** A decoder's own error text describes bytes
/// ("invalid PNG signature", "unexpected EOF") and lands in a console where several things
/// could have produced it; the person who typed a path needs to know *which* path went wrong.
/// This is `organon_core::exhibit::ExhibitError`'s rule applied one stage later — that type
/// refuses a name, this one refuses the bytes behind it.
///
/// ⚠️ **The kind comes from the shared table, not from a flag passed in.** `kind_for_path` is
/// the same function the composer's refusal used, so a file cannot be classified one way when
/// it is accepted and another way when it is read.
fn load_exhibit_item(path: &std::path::Path) -> ExhibitLoad {
    let name = path.display();
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        // The common case by far, and the one worth a plain sentence: a typo, or a relative
        // path resolved against a working directory the person did not have in mind.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return ExhibitLoad::Failed(format!("{name}: no such file"));
        }
        Err(err) => return ExhibitLoad::Failed(format!("{name}: {err}")),
    };
    if meta.is_dir() {
        return ExhibitLoad::Failed(format!("{name}: that is a directory"));
    }
    if meta.len() > MAX_EXHIBIT_FILE_BYTES {
        return ExhibitLoad::Failed(format!(
            "{name}: {} MB is past the {} MB this build will open",
            meta.len() / (1024 * 1024),
            MAX_EXHIBIT_FILE_BYTES / (1024 * 1024)
        ));
    }
    match organon_core::exhibit::Exhibit::kind_for_path(path) {
        Some(kind::Kind::Markdown) => match std::fs::read_to_string(path) {
            Ok(text) => ExhibitLoad::Document(text.into()),
            // The one failure a person will hit here and not understand from an io error
            // alone: a file that is not UTF-8 is a real document in some other encoding, not a
            // broken one.
            Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                ExhibitLoad::Failed(format!("{name}: not UTF-8 text"))
            }
            Err(err) => ExhibitLoad::Failed(format!("{name}: {err}")),
        },
        Some(kind::Kind::Image) => match image::open(path) {
            Ok(decoded) => {
                // `thumbnail` preserves the aspect ratio and is the fast path; a picture
                // already inside the bound is returned untouched by the `>` test rather than
                // resampled for nothing.
                let scaled = if decoded.width() > MAX_EXHIBIT_EDGE
                    || decoded.height() > MAX_EXHIBIT_EDGE
                {
                    decoded.thumbnail(MAX_EXHIBIT_EDGE, MAX_EXHIBIT_EDGE)
                } else {
                    decoded
                };
                let rgba = scaled.to_rgba8();
                ExhibitLoad::Picture { size: (rgba.width(), rgba.height()), rgba: rgba.into_raw() }
            }
            Err(err) => ExhibitLoad::Failed(format!("{name}: {err}")),
        },
        // Unreachable through `/media`, which resolves the kind before an artifact is ever
        // built — but this function takes a path and a path can say anything, so it answers
        // rather than assuming.
        _ => ExhibitLoad::Failed(format!("{name}: not a file this build shows")),
    }
}

/// Which surfaces keep their textures when more are held than [`MAX_SURFACE_TEXTURES`]
/// allows, and which go — **pure**, so the policy is a test rather than a claim.
///
/// Least-recently-**requested** goes first. Note "requested", not "rendered": a surface that
/// is on screen and unchanged is being asked for every frame and must never be evicted in
/// favour of one that merely re-rendered once. `wanted` is this frame's request list, in
/// transcript order, and is what a tie falls back to — so a frame that asks for five surfaces
/// at once evicts the ones furthest up the page rather than an arbitrary one.
///
/// Returns the keys to release, in eviction order.
///
/// 📌 **Generic over the key, because there are now two texture ledgers and there must not be
/// two policies.** A conversation surface is keyed by `(pane, element)` and an exhibit item by
/// `(element, item)`; the *policy* — least-recently-requested first, ties broken down the
/// request list — is a fact about how a person reads a scrollback and is identical for both. A
/// second copy specialised to the other key is how the two would drift into disagreeing about
/// which picture a long session keeps.
fn surfaces_to_evict<K: Copy + PartialEq>(
    held: &[(K, u64)],
    wanted: &[K],
    cap: usize,
) -> Vec<K> {
    if held.len() <= cap {
        return Vec::new();
    }
    let mut order: Vec<(K, u64, usize)> = held
        .iter()
        .map(|(key, touched)| {
            let rank = wanted.iter().position(|w| w == key).unwrap_or(usize::MAX);
            (*key, *touched, rank)
        })
        .collect();
    // Oldest touch first; among equal touches (everything wanted this frame shares one
    // stamp) the one furthest down the request list goes first, so the top of the page —
    // what the reader scrolled *to* — survives.
    order.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)));
    order.truncate(held.len() - cap);
    order.into_iter().map(|(key, _, _)| key).collect()
}

/// Which documents go when more text is held than `budget` allows — **pure**, so the policy is
/// a test rather than a claim, exactly like [`surfaces_to_evict`].
///
/// 📌 **The same rule, weighed instead of counted.** Least-recently-**requested** first, ties
/// broken furthest-down-the-request-list, and eviction stops the moment what remains fits.
/// A separate function rather than a `cap` computed for `surfaces_to_evict` because "how many
/// entries fit" is not answerable in advance when the entries are different sizes: dropping the
/// two oldest might free 4 KB or 8 MB, and only the running total knows when to stop.
///
/// `held` is `(key, last requested, bytes)`. Returns the keys to release, in eviction order —
/// empty when everything already fits, which is the ordinary case.
fn documents_to_evict<K: Copy + PartialEq>(
    held: &[(K, u64, usize)],
    wanted: &[K],
    budget: usize,
) -> Vec<K> {
    let mut total: usize = held.iter().map(|(_, _, n)| *n).sum();
    if total <= budget {
        return Vec::new();
    }
    let mut order = held.to_vec();
    let rank = |k: &K| wanted.iter().position(|w| w == k).unwrap_or(usize::MAX);
    order.sort_by(|a, b| a.1.cmp(&b.1).then(rank(&b.0).cmp(&rank(&a.0))));
    let mut going = Vec::new();
    for (key, _, bytes) in order {
        if total <= budget {
            break;
        }
        going.push(key);
        total -= bytes;
    }
    going
}

/// One slider, applied to the snapshot a surface will be rendered from — **pure**, so the
/// whole knob vocabulary is a test rather than something only a GPU can answer.
///
/// The three labels are chosen to be **orthogonal to the material buttons beside them**, and
/// that is the property that makes the panel read as one instrument: `apply_material` writes
/// `pbr[0..2]` and `lighting[7]`, so a knob on any of those would appear to do nothing the
/// moment a button was pressed. These three are lanes no material touches (`substrate_scene`
/// and `substrate_materials` own the manifests), so a button and a knob compose instead of
/// fighting.
///
/// An unknown label writes nothing, on `console_step`'s forward-compatibility contract: a
/// view naming a knob this build does not have must leave the picture exactly as it found it.
fn apply_surface_slider(s: &mut ipc::Shared, label: &str, v: f32) {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { return };
    match label {
        // The key light swept all the way round, centred on the console's own azimuth — so
        // mid-slider is the shipped look and either direction is a real departure from it.
        // Wrapped rather than clamped, because a light at ±180° is a light, not an error, and
        // `params.rs:8554` states the range as (−180..180].
        "light" => s.lighting[4] = wrap_degrees(SUBSTRATE_KEY_AZIMUTH_DEG + (v - 0.5) * 360.0),
        // Key elevation, horizon to overhead. At 0 the plane is grazed and the gradient is
        // enormous; at 90 it is flat-lit. `substrate_scene`'s shipped 42° sits at v ≈ 0.467.
        "elevation" => s.lighting[3] = v * 90.0,
        // Exposure in EV stops, ±3 around the substrate's own 0. The most unmistakable knob
        // on the panel, deliberately: one of the three has to be legible at a glance from
        // across a room, and this is it.
        "exposure" => s.pbr[2] = (v - 0.5) * 6.0,
        _ => {}
    }
}

/// Fold `d` into (−180, 180]. Written out rather than `rem_euclid`'d in place because the
/// half-open end matters — `params.rs` states the range that way, and 180 and −180 are the
/// same direction.
fn wrap_degrees(d: f32) -> f32 {
    let mut d = d % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d <= -180.0 {
        d += 360.0;
    }
    d
}

/// The knobs a conversation's panels offer, and where they start.
///
/// Handed down to [`ConversationPane`] for `conversation_view`'s reason — that crate cannot
/// see `Shared` and a label it invented would be a knob that moves nothing. The starting
/// values are the ones that reproduce `substrate_scene`'s shipped look exactly, so a summoned
/// surface opens as the substrate and every drag is a departure from it rather than from an
/// arbitrary midpoint.
fn surface_slider_table() -> Vec<(String, f32)> {
    vec![
        ("light".to_string(), 0.5),
        ("elevation".to_string(), substrate_scene::SUBSTRATE_KEY_ELEVATION_DEG / 90.0),
        ("exposure".to_string(), 0.5),
    ]
}

/// The `Shared` snapshot one surface is rendered from: [`look_shared`] at the substrate,
/// with the panel's knobs applied last.
///
/// **Last, and total over the look**, for the reason `look_shared` gives about the key
/// azimuth: the knobs are the *reader's* correction, not the material's, so they survive any
/// material or rig that would otherwise write the same lane.
fn surface_shared(look: &SurfaceLook) -> Box<ipc::Shared> {
    let mut s = look_shared(BackdropSource::Substrate, &look.look);
    for (label, value) in &look.sliders {
        apply_surface_slider(&mut s, label, *value);
    }
    s
}

/// One tab's look history: where it sits in absolute lines, which looks its rows were
/// written under, and the pictures of the closed ones.
///
/// **Per session, because scrollback is per session.** Every tab pumps every frame and every
/// tab has its own buffer, so `dropped` and the boundary lines are its own; the *looks* are
/// the window's, and every pane's ledger receives every look change. Two consequences worth
/// knowing: a tab opened after some look changes starts with one epoch (it has no rows from
/// before it existed), and [`EpochId`]s are only unique **within** a pane — which is why the
/// cache lives here rather than on [`Console`].
struct PaneLooks {
    anchor: PaneAnchor,
    ledger: EpochLedger,
    cache: HashMap<EpochId, Rc<CachedEpoch>>,
    /// Tier 5: the patches this transcript is carrying, in the order they were claimed.
    ///
    /// **Per pane for the same reason the anchor is** — a patch names a line in *this*
    /// tab's absolute-line coordinate, and that coordinate means nothing in another tab.
    ///
    /// **One list across every kind, not a list per kind**, and that is the shape the tier
    /// turns on: a patch's index here is what `block_anchor`'s bands, `block_quads`' quads and
    /// `block_panel`'s placements all mean by "which one", so the two paints share a z-order
    /// and a scene claimed after a panel sits over it exactly as drawing them in sequence
    /// implies. Two lists would be two index spaces and the ordering between them would be
    /// nobody's.
    ///
    /// No cap and no eviction yet: a scene patch paints from the live backdrop texture, which
    /// the console allocates anyway, and a panel is a title and three floats — so a patch owns
    /// no GPU resource of its own to leak. The moment one does, this wants `substrate_epochs`'
    /// bounded-and-logged discipline rather than a second invention. What is genuinely missing
    /// is reaping (`Block::retained_rows(dropped) == 0` is the signal, and it exists) and the
    /// width-change invalidation `PaneAnchor::feed_local`'s doc states as policy.
    blocks: Vec<Patch>,
}

/// What one tab actually is (Console Spike §5.9): the console has **two front-ends over
/// one renderer**, and this is the fork.
///
/// [`Pane::Term`] is everything that came before and is unchanged — a real PTY, the VT
/// core, the glyph grid. It runs `htop`, it runs `vim`, it runs an unmodified Claude Code
/// tab, and none of them will ever know the console exists (§6 Rule 5′, which still
/// governs it in full).
///
/// [`Pane::Conversation`] has **no PTY at all**. It drives an agent over pipes and
/// renders its structured event stream natively, so a tool call is a card rather than the
/// text a terminal would have printed. The two share the window, the tab strip, the
/// command lane and the backdrop; they share nothing below that, which is why this is an
/// enum rather than a flag on one type.
enum Pane {
    Term(TermSession),
    /// Boxed: a conversation carries a transcript and a child process, and the terminal
    /// variant is already large. Without it every `Vec<Pane>` slot pays the bigger size.
    Conversation(Box<ConversationPane>),
}

impl Pane {
    /// The PTY behind this tab, if it has one. **Every terminal-only path goes through
    /// here** — the Tier 5 block/patch verbs, the anchor pump, the epoch boundary — so a
    /// conversation tab is skipped by construction rather than by remembering to check.
    fn term_mut(&mut self) -> Option<&mut TermSession> {
        match self {
            Pane::Term(session) => Some(session),
            Pane::Conversation(_) => None,
        }
    }

    fn term(&self) -> Option<&TermSession> {
        match self {
            Pane::Term(session) => Some(session),
            Pane::Conversation(_) => None,
        }
    }
}

struct Console {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    renderer: Option<egui_wgpu::Renderer>,
    /// One pane per tab, index-aligned with `strip.tabs`. ALL panes pump every frame
    /// (a background agent keeps streaming); only the active one draws. The 2026-08-08
    /// reframe (PRD v3.2): Console is a TUI HOST — the default tab runs the default
    /// HARNESS (Pi first among equals), and the bare terminal is one menu entry, not the
    /// opening position.
    ///
    /// Console Spike §5.9 made the element type an enum: a pane is a terminal **or** a
    /// conversation. See [`Pane`].
    sessions: Vec<Pane>,
    /// The look history of each tab, **index-aligned with `sessions`** (Console Spike Tier
    /// 4). Created and dropped in the same two places a session is.
    pane_looks: Vec<PaneLooks>,
    strip: TabStrip,
    registry: Vec<HarnessSpec>,
    installed: HashSet<String>,
    default_harness: String,
    plus_open: bool,
    quit: bool,
    /// The engine (tree E). Owns the `Device`/`Queue` after `attach_gpu`; renders
    /// only into [`Console::backdrop`], never the swapchain.
    world: World,
    backdrop: Option<Backdrop>,
    backdrop_source: BackdropSource,
    /// The substrate dressing the backdrop is currently wearing — **the pair Tier 4's epoch
    /// ledger records**. Changed only by [`Console::apply_console`], read only by
    /// [`look_shared`].
    console_look: ConsoleLook,
    /// The console command sidecar's drain state: lines already consumed, and the file length
    /// they were consumed at. Exactly the `CmdChannel::cli_cursor` / `cli_len` pair the World
    /// keeps for `cli.txt` (`world.rs:809-810`), and seeded the same way — at CONSTRUCTION,
    /// from ONE read, so a command typed a moment after launch still drains while a backlog
    /// from before this process existed never replays.
    console_cursor: usize,
    console_len: u64,
    /// The session log every console dispatch is recorded in. `None` when the store could not
    /// be opened — see [`Console::dispatch_console`], which says exactly what is lost.
    session_log: Option<SessionLog>,
    /// The terminal pane's size in **points**, and the whole window's beside it, recorded at
    /// the end of each frame and consumed by the next frame's [`Console::render_backdrop`].
    /// `None` until the first frame has laid the panel out.
    ///
    /// ⚠️ **A pair of point sizes, deliberately — not a size and a scale.** The scale that
    /// turns points into pixels is an egui frame *output*, so remembering one means starting
    /// from a stand-in, and the stand-in that reads naturally (`1.0`) is a real 100 % display
    /// as far as the multiply is concerned: the backdrop comes out sized in **points**, 2.25×
    /// too small on this display, and any epoch snapshot copied from it in that window keeps
    /// the small picture for the session. Two point sizes give
    /// [`scene_input::pane_pixels_in`] a *ratio* to apply to the swapchain, which is physical
    /// by definition — so the scale cancels instead of being guessed. That function's doc
    /// owns the argument and the measurement.
    pane_points: Option<(f32, f32)>,
    window_points: Option<(f32, f32)>,
    /// Whether the last [`Console::render_backdrop`] recreated the backdrop texture — the
    /// pane changed size. Carried on `Console` rather than returned because it is
    /// [`EpochLedger::plan`]'s `pane_resized` argument, consumed one call later; it is the
    /// existing `backdrop.size != (w, h)` condition and nothing new.
    pane_resized: bool,
    /// The `Shared` snapshot writer (organon-shell namespace). In the two-process
    /// design the PLUGIN writes this; Console has no plugin, so the terminal writes
    /// the default look itself — which is what makes `organon status`/`get`/`watch`
    /// see a live system from inside the terminal, and gives the in-process world
    /// real params to read instead of zeroes. The CLI's override lane
    /// (`set`/`generator`/…) then applies on top in the world's working copy,
    /// exactly as it does against the standalone visual.
    shared_writer: Option<ipc::Writer>,
    shared: Box<ipc::Shared>,
    /// **What an Organon editor panel drawn in a conversation has asked for** (Console #7).
    /// One per console, not one per element: two `/organon look surface` cards in a transcript
    /// are two views of one instrument, and reading different values off each would make the
    /// claim `/organon` exists to make — *this is the same panel* — false on sight.
    ///
    /// 🚨 **Boxed, and that is a crash fix rather than a style choice.** `OrganonPanels` holds
    /// an `OrganicMathParams` and a `PresetValues` **inline**, and the params struct carries
    /// one nih-plug param object per automatable lane — so the type is enormous. Held by value
    /// here it became part of `Console`, which `Console::new` builds **on the main thread's
    /// stack** before moving it; the console then died at startup with *"thread 'main' has
    /// overflowed its stack"*, before drawing a frame. The box keeps `Console` the size it was
    /// and puts the panels on the heap. ⚠️ Measure before un-boxing: nothing warns, the build
    /// is clean, and the failure is total.
    organon_panels: Box<OrganonPanels>,
    /// True while the surface acquires as `Occluded` — gates the redraw re-arm
    /// (the measured ~98%-CPU-drawing-nothing spin, fixed on the v2 branch).
    occluded: bool,
    /// The render targets behind the conversation view's rendered surfaces, bounded by
    /// [`MAX_SURFACE_TEXTURES`] and evicted with a log line. Keyed by [`SurfaceKey`].
    surfaces: HashMap<SurfaceKey, SurfaceTexture>,
    /// What the conversation view asked for on the **previous** frame, and which pane asked.
    ///
    /// ⚠️ **One frame behind by construction, exactly as `pane_points` is.** A surface's rect
    /// is an output of egui layout, so its size cannot be known until the frame that laid it
    /// out has finished — and the texture has to exist *before* the frame that paints it. The
    /// visible consequence is one "rendering…" frame when a surface is summoned, and nothing
    /// else: a look changed by a drag is rendered on the frame after the drag, which at 60 Hz
    /// is not a thing a hand can perceive.
    surface_requests: Vec<SurfaceRequest>,
    /// What the console has read or decoded for each exhibit item, and what the view draws
    /// from. A missing entry is "still reading" — see `conversation_view::ExhibitContent`.
    exhibits: ExhibitContents,
    /// What the conversation view asked for on the **previous** frame — `surface_requests`'
    /// rule and its reason, one seam over.
    exhibit_requests: Vec<ExhibitRequest>,
    /// The wgpu textures behind `exhibits`' `Picture` entries, held so they outlive the egui
    /// registration that points at them. Keyed by that registration, because freeing one means
    /// telling egui first and dropping the texture second.
    exhibit_textures: HashMap<egui::TextureId, wgpu::Texture>,
    /// Items with a loader thread running. Prevents a second thread being spawned for a file
    /// every frame while the first is still opening it — which, on a slow disk, is how one
    /// picture becomes a hundred threads.
    exhibit_inflight: HashSet<ExhibitKey>,
    /// Last frame each item was asked for, for the eviction policy.
    exhibit_touched: HashMap<ExhibitKey, u64>,
    /// The frame counter the stamps above come from — `surface_clock`'s twin.
    exhibit_clock: u64,
    /// Where loader threads send their results. Drained at the top of `service_exhibits`.
    exhibit_rx: std::sync::mpsc::Receiver<(ExhibitKey, ExhibitLoad)>,
    /// Cloned into each loader thread. Held here so the channel stays open for the life of the
    /// console rather than closing the moment the last job finishes.
    exhibit_tx: std::sync::mpsc::Sender<(ExhibitKey, ExhibitLoad)>,
    surface_pane: usize,
    /// Monotonic frame counter, used only as the cap's recency stamp.
    surface_clock: u64,
    /// Whether the portal is on screen. Moved only by [`Console::apply_console`], through
    /// [`portal::step`].
    portal_state: PortalState,
    /// **How the pane is divided, and what each part holds.** Moved only by
    /// [`Console::set_viewport`], through `region::Layout::assign`.
    ///
    /// ⚠️ **Held rather than derived, unlike [`organon_console::screen::Screen`]** — and the
    /// contrast is worth reading, because that module argues hard *against* keeping a copy. Its
    /// reason is that the window itself knows whether it is full screen, so a remembered bool
    /// would be a second source of truth for a fact the platform already owns. Nothing owns this
    /// one: egui has no notion of a region, the rectangles are recomputed from the pane every
    /// frame and remembered nowhere, and there is no second authority for a stored layout to
    /// disagree with.
    ///
    /// 📌 **Not written to `preferences.json`, on the posture's rule**: a console opens
    /// undivided however it was left. A stored layout would be the first thing that could make
    /// a launch look broken with no command having been typed.
    layout: organon_console::region::Layout,
    /// **A scrolling column of Organon panels PER REGION.** Moved only by
    /// [`Console::set_stack`] and by a `/organon` line's answer.
    ///
    /// ✏️ **Was one stack, console-wide, until #98 Tier C** —
    /// `organon_console::panel_stack`'s header owns the whole argument and was rewritten with
    /// this. In short: the mechanical reason (the add verb having no ring to spare for a region
    /// word) is dissolved by a region's own command line supplying it, and the architectural
    /// one conflated two objects — the parameter **mirror** stays one per console, which is what
    /// [`OrganonPanels`] is; a column's **composition** belongs to the column. James built the
    /// four-region layout on a running console on 2026-08-20 and watched one `stack add surface`
    /// fill both side columns identically, which is not what a person assembling two control
    /// columns means.
    ///
    /// 📌 Not written to `preferences.json`, on the layout's rule directly above — and note
    /// `organon_console::layout` does not record stack contents either, so a saved arrangement
    /// comes back with empty columns. That is §1.15's stated gap, made more visible by this
    /// change rather than caused by it.
    panel_stacks: organon_console::panel_stack::Stacks,
    /// **Every panel column's add/remove control, and the console's one answer to who owns the
    /// keyboard.** `organon_console::region_line`'s header owns the argument.
    ///
    /// ✏️ **Narrowed from #98 Tier C's "a command line in every region".** Only a region holding
    /// `panel` gets one now, and its whole vocabulary is `add <panel>` / `remove <panel>` —
    /// James's own scope, which Tier C overshot. The array is still one line per region because
    /// the *state* is per-region and cheap; what changed is which of them are ever drawn.
    ///
    /// 🚨 **The arbitration is the tier.** `conversation_view::composer_keys` consumes Tab,
    /// Escape and the arrows out of the raw event list — two of them unconditionally on an empty
    /// box — which was safe while the console had exactly one command input. Every frame this
    /// hands `ConversationPane::set_keys` the answer to "did a region line have focus last
    /// frame", measured off that widget's own `has_focus` rather than invented.
    ///
    /// 📌 Not persisted, on the layout's rule: a console opens with empty lines.
    region_lines: organon_console::region_line::Lines,
    /// **The vocabulary a region line resolves against** — built once from [`console_specs`],
    /// which is the same `Vec<CommandSpec>` the MCP schemas and every conversation pane's own
    /// registry are built from.
    ///
    /// 🚨 **A second `Registry` VALUE, never a second table.** `ConversationPane` builds its own
    /// from the same specs, and a region line cannot borrow that one: a console may hold no
    /// conversation pane at all, and even when it does, the pane is behind a `&mut` the region
    /// walk is already using. What matters is that both are `Registry::new(&console_specs())` —
    /// `a_region_line_expands_onto_the_real_console_specs` pins that the two agree.
    ///
    /// ⚠️ **It is still the whole registry even though the control takes two words.** `act`
    /// expands onto `console.stack` and lets the registry validate the panel name and produce the
    /// refusal, which is what keeps the control and the CLI one vocabulary; narrowing this to a
    /// stack-only table would be the second vocabulary §1.8 exists to prevent.
    ///
    /// ⚠️ Built at construction rather than per frame: `Registry::new` clones every spec and
    /// walks them for collisions, which is a cost per keystroke on the candidate path.
    line_registry: organon_console::registry::Registry,
    /// **The viewport's render target — ONE texture serving both presentations.**
    ///
    /// 🚨 **One, not one each, and that is [`engine_plan`]'s guarantee spent rather than a
    /// saving.** At most one presentation gets the World in any frame, so a second texture could
    /// only ever hold a picture nobody is allowed to refresh — i.e. the stale texture §1.14's
    /// vacancy rule forbids showing. The presentation that lost the frame paints a notice, and
    /// the presentation that won owns this. Switching between them is a size change, which this
    /// field already handles the only way it can: free, reallocate, and say so in the log.
    ///
    /// ⚠️ **A field beside [`Console::backdrop`], NOT a [`SurfaceKey`] variant**, and the reason
    /// is about meaning rather than effort. [`surfaces_to_evict`] is a policy for *many things
    /// competing for few slots*; a viewport is *one thing that is live or not*. It is
    /// requested every frame it exists, so its stamp is always `now` and the cap could never
    /// choose it — a key variant would exist solely to be excluded from the one function the
    /// type serves, and would then have to be remembered out of [`Console::free_all_surfaces`]
    /// and taught to the eviction log so it did not print a fabricated element id.
    ///
    /// The deciding argument is smaller and harder: **the viewport must work in a terminal tab**,
    /// where there are no elements and [`ElementId`] means nothing at all. So `SurfaceKey`, its
    /// tests, `SurfaceImages` and the whole `conversation_view` seam are untouched by this
    /// feature — only [`SurfaceTexture`] and [`Console::make_surface_texture`] are reused, which
    /// is the part that was worth reusing.
    viewport: Option<SurfaceTexture>,
    /// The camera gesture the live viewport accumulated this frame, drained into the World after
    /// the UI and before the next render — `wgpu_editor`'s arrangement exactly.
    ///
    /// 🚨 **One accumulator, because there is one camera.** `World` holds a single
    /// yaw/pitch/distance and the console shows it through whichever rectangle is live, so a
    /// second accumulator would be a second name for the same three fields. It is also what
    /// keeps `scene_input::scene_viewport`'s fixed egui id honest: that function interns one id,
    /// so it may be called **at most once per frame** — which is exactly what the precedence in
    /// [`engine_plan`] already guarantees, since the losing presentation paints a notice and
    /// registers no interaction region at all.
    viewport_input: scene_input::SceneInput,
    /// Where the live viewport was drawn **last** frame, in points.
    ///
    /// ⚠️ One frame behind, exactly as [`Console::pane_points`] is and for the same reason: the
    /// rect is derived from the pane, the pane is an egui layout output, and the texture has to
    /// exist before the frame that paints it. The visible consequence is one "the viewport is
    /// there but empty" frame when it opens, and nothing else — the rect *inside* a frame is
    /// recomputed from that frame's own pane, so the rectangle a person sees is never stale
    /// even though the pixels in it are one frame old.
    viewport_points: Option<(f32, f32)>,
    /// When a **hand** last moved the camera, or `None` if none ever has.
    ///
    /// 🚨 This is the whole of what makes "the hand always wins" enforceable, and it has to be
    /// recorded on the console side rather than inside `World`: both a drag and
    /// `organon console camera` arrive at `World::apply_camera_input`, which cannot tell them
    /// apart, so by the time either reaches the world the distinction is gone. Stamped where
    /// the *gesture* is drained (see `redraw`), read by `camera::arbitrate`.
    hand_camera_at: Option<Instant>,
    /// When an **agent** framing was last *applied*, or `None` if none ever has.
    ///
    /// The twin of [`Console::hand_camera_at`], and stamped for the opposite reason: that one
    /// exists so an agent can be held off, this one so a reader can be told who moved the
    /// camera. Stamped in [`Console::frame_camera`] **after** the arbitration, never before — a
    /// framing the hand held off moved nothing and must not claim to have.
    agent_camera_at: Option<Instant>,
    /// Where the viewer stands, published once per frame for the MCP read
    /// ([`ConsoleDispatch`]).
    ///
    /// 🚨 **Published, not remembered.** It is filled from `World::camera_framing()` — the live
    /// three fields, after every writer in the frame — rather than from the last command this
    /// console applied. A hand outranks an agent here (`camera::arbitrate`), so the last thing
    /// an agent set is routinely *not* where the camera is, and answering with it would be a
    /// confident lie of exactly the kind this tree's honesty discipline exists to prevent.
    viewpoint: camera::ViewpointCell,
    /// Every colour the console paints — see [`organon_console::theme`].
    ///
    /// 🚨 **The one owner, and this is the struct that owns it because it is the one thing in
    /// the process that outlives a frame and contains every front-end.** A tab is a terminal
    /// or a conversation and both draw inside one `egui_ctx.run`; the palette is neither
    /// tab's, so putting it on a `Pane` would make "the same console in two colours" a state
    /// nothing forbids. It is borrowed into the closure alongside `sessions` and `strip` and
    /// reaches every draw site as `&Theme` — never cloned per frame, never a `static`, so a
    /// later per-tab or preview palette is a second value rather than a rewrite.
    theme: Theme,
    /// The name [`Console::theme`] answers to, always one of `Theme::NAMES`.
    ///
    /// **Carried rather than searched for**, because the two questions a name answers are both
    /// asked at moments where a reverse lookup would be wrong. `preferences.json` stores a
    /// *name*, so a save needs one; and a `Theme` is a fifty-field struct whose fields
    /// deliberately share values across palettes (§1.4), so "which palette is this?" is not a
    /// question the value can be trusted to answer if two of them ever coincide.
    ///
    /// `&'static str` rather than `String`: it is only ever `Theme::NAMES`' own entry, which is
    /// what stops the store recording a spelling nothing can resolve.
    theme_name: &'static str,
    /// **How the console holds itself** — see [`organon_console::posture`]. The second axis,
    /// and orthogonal to the palette above it: `theme` is what the console is made of,
    /// `posture` is whether it stands terminal-tight or desktop-open, and every combination
    /// of the two is a real console.
    ///
    /// One owner for the same reason `theme` has one, and it is a `Posture` rather than a
    /// resolved `Form` because the scalar is what a later tier *animates*: a `Form` on the
    /// struct would make the tween a question of which of fourteen fields somebody
    /// remembered to move. `redraw` resolves it once per frame and passes `&Form` down.
    ///
    /// ⚠️ **Set once and held.** There is no animation here and no timer: this tier wires the
    /// tokens and ships at the terminal end, so the console draws exactly what it drew
    /// before. Tier C owns the tween, its `request_repaint` discipline and what a moving
    /// layout does to the scroll anchor.
    posture: Posture,
}

/// Register a viewport's interaction region and paint it — **the one implementation, serving
/// both presentations.**
///
/// 🚨 **A viewport is a producer plus a camera plus a texture; the portal and a `3d` region are
/// two ways of presenting one.** That is why this takes a rect and a mode rather than being
/// duplicated per presentation: the gesture, the camera, the image and the edge are identical,
/// and only *where* it sits differs. `SceneMode` is the enum that already modelled exactly this
/// distinction, from before either existed — `Workstation` is "a pane inside the workstation, a
/// widget among widgets", `Immersive` is "the scene is the window and the interface floats over
/// it" — so it is the seam rather than a parallel notion invented here.
///
/// ⚠️ **Both call sites pass `Workstation` today, and that is the honest answer rather than an
/// unused parameter.** A floating rectangle and a region are *both* bounded panes inside an
/// interface; nothing in the console is immersive yet (§2's portal row owns that). It is passed
/// explicitly so each site says which presentation it is, and so the day one of them becomes
/// immersive it is a value changed at a call site rather than a hardcode discovered inside here.
///
/// ⚠️ **Call it at most once per frame.** `scene_input::scene_viewport` interns a single fixed
/// egui id, so two live viewports in one frame would be two widgets fighting over one id. That
/// is not a rule anybody has to remember: [`engine_plan`] gives the World to exactly one
/// presentation, and the other paints a notice instead of calling this.
///
/// # Order inside, and why it is not the obvious one
///
/// The region is registered **before** the image is painted, and that is not stylistic:
/// [`scene_input::scene_viewport`] *consumes* the wheel from inside — it zeroes both
/// `smooth_scroll_delta` and `raw_scroll_delta` on the frames it owns — and a `ScrollArea`
/// reads the smoothed value in its `end()`. Registering first is what makes a zoom over the
/// portal a zoom and not also a scroll.
///
/// ⚠️ **That consumption does NOT cover the terminal front-end**, which reads
/// `raw_scroll_delta` in its own body, *before* this function runs. `term_view::draw`'s
/// explicit rect test is what covers that, and the two are not alternatives — the terminal
/// needs the rect test because it reads raw input, and everything else needs the consumption
/// because it reads egui's.
///
/// # `SceneMode::Workstation`, and `Sense::drag()`
///
/// Workstation is the honest mode: the portal is *a bounded pane inside an interface*, which is
/// precisely what that variant means, and it answers "the press is the scene's" without walking
/// egui's hit test — the immersive walk exists for a scene that is the whole window with the
/// interface floating over it, which is a state this tier does not build.
///
/// `scene_viewport` hardcodes `Sense::drag()`. That is right here and would stop being right in
/// an immersive portal: a drag-only widget is what egui treats as "a big background thing", so
/// a click landing on it is handed to whatever control wanted it. Neither presentation has a
/// click gesture yet, so nothing is lost. When the click-to-grow transition is built, widen
/// `scene_viewport` with a `Sense` parameter — the editor's two call sites passing
/// `Sense::drag()` verbatim so their behaviour is provably unchanged — rather than adding a
/// second `ui.interact` on the same rect: two widgets on one rectangle fight in the hit test,
/// and which one loses is decided by registration order.
fn paint_viewport(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    image: Option<egui::TextureId>,
    input: &mut scene_input::SceneInput,
    mode: scene_input::SceneMode,
    theme: &Theme,
) {
    let _resp = scene_input::scene_viewport(ui, rect, mode, input);
    let painter = ui.painter();
    match image {
        // UV 0..1 with no fit policy to get wrong: the console renders the target at exactly
        // this rect's pixel size, so there is nothing to letterbox — `conversation_view`'s
        // surface quad carries the same comment for the same reason.
        Some(id) => {
            painter.image(
                id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                // The identity multiplier, not a colour — nothing the theme has any business
                // tinting the engine's own render with.
                egui::Color32::WHITE,
            );
        }
        // The one frame between "this viewport is live" and "its rect has been measured, its
        // texture made and a world drawn into it". Filled rather than left transparent so the
        // object exists on screen from the frame it was asked for — an empty outline over live
        // scrollback reads as a rendering failure, which is the very confusion the surface
        // path's own "rendering…" placeholder was added to prevent.
        None => {
            painter.rect_filled(rect, 0.0, theme.panel_fill);
        }
    }
    // The console's own edge, not a second visual language arrived at by copy-paste — the same
    // phosphor hairline a patch panel wears, which is what says "this is a thing, deliberately
    // placed" rather than "the render leaked".
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0_f32, theme.panel_edge),
        egui::StrokeKind::Inside,
    );
}

/// Draw a **divided** pane: one region per rectangle, and a notice in every region that holds
/// nothing yet.
///
/// Only reached when the layout is not the default — `redraw`'s fast path draws an undivided
/// console through exactly the code it always did (invariant #4). Everything here is the
/// consequence of `organon_console::region::plan`'s answer; no rectangle is computed at this
/// site, which is what keeps the geometry headlessly testable.
///
/// # 🚨 The live tab is drawn at most once, and that is structural rather than a rule
///
/// `draw_active_pane` is `&mut dyn FnMut`, so calling it twice would need two simultaneous
/// `&mut` borrows of the same pane — `conversation_view::draw` takes one. A second `agent`
/// region therefore **cannot** show a second copy of the tab, and rather than leave that as a
/// silent blank it says so and names what would fix it. Tier 2's per-region tab is the fix.
///
/// # ⚠️ A pane too small for the layout says so rather than drawing slivers
///
/// `plan` answers `None` when any region falls under `region::MIN_SIDE`. The console is then
/// showing nothing at all, which is the one state that absolutely must not be quiet — so the
/// whole pane carries the sentence and the command that undoes it.
///
/// # The `3d` region, and what it answers with
///
/// Returns **the rectangle the `3d` region was drawn at this frame**, which is what the next
/// frame's [`Console::render_viewport`] sizes its texture to — the same one-frame-behind
/// arrangement the portal already has, for the same reason (a rect is an output of the layout
/// that produced it). `None` when no region holds `3d`, when the pane cannot hold the layout, or
/// when the portal took the frame — in the last case the region is drawn as a **notice**, so
/// there is no viewport rect to remember and nothing must size a texture to it.
fn draw_regions(
    ui: &mut egui::Ui,
    pane: Option<egui::Rect>,
    layout: &organon_console::region::Layout,
    theme: &Theme,
    viewport: &mut RegionViewport<'_>,
    panels: &mut RegionPanels<'_>,
    draw_active_pane: &mut dyn FnMut(&mut egui::Ui),
) -> Option<egui::Rect> {
    use organon_console::region::{plan, Content};
    let Some(placed) = pane.and_then(|p| plan(p, layout)) else {
        ui.centered_and_justified(|ui| {
            ui.monospace(
                "the window is too small for this layout — `organon console viewport full agent`",
            );
        });
        return None;
    };
    let mut viewport_rect = None;
    let mut live_tab_taken = false;
    // 🚨 **The panel column's control band, reserved from the region's rectangle before anything
    // else is laid out — and ONLY for a region holding a `panel`.**
    //
    // ✏️ **Narrowed from "every region that is not an agent".** #98 Tier C put a command line in
    // every region; James rejected that scope on a running console (*"I only particularly wanted
    // to be able to add and remove panels from a panel section"*), so a region holding `3d`, and
    // a region holding nothing, now get no band at all — the same answer an `agent` region
    // already got, reached for a different reason. `region_line`'s header owns the argument, and
    // note the consequence below: a vacant region's notice has to name the CLI again, because it
    // no longer has a line of its own to point at.
    let row = ui.text_style_height(&egui::TextStyle::Monospace);
    let band = organon_console::region_line::band_height(row);
    for slot in &placed {
        let takes_a_line = slot.content == Some(Content::Panel);
        // ⚠️ **Split before either half is drawn, never read back from one of them.** The
        // content's rectangle is an input to what it draws — a `3d` region's is what next
        // frame's texture is sized to — so deriving it from the line's own allocation would make
        // the picture's size depend on the order two widgets happened to run in.
        let (content_rect, line_rect) = if takes_a_line && slot.rect.height() > band * 2.0 {
            let cut = slot.rect.max.y - band;
            (
                egui::Rect::from_min_max(slot.rect.min, egui::pos2(slot.rect.max.x, cut)),
                Some(egui::Rect::from_min_max(
                    egui::pos2(slot.rect.min.x, cut),
                    slot.rect.max,
                )),
            )
        } else {
            // ⚠️ **A region too short for both keeps its content**, and the line is the thing
            // that goes. `plan` already refuses a layout whose regions fall under `MIN_SIDE`, so
            // this is the band between "drawable" and "drawable with a command line in it" — and
            // a rectangle that showed only its own command line would have hidden the thing
            // somebody assigned it for. The vacancy notice below still names the verb.
            (slot.rect, None)
        };
        // A child `Ui` per region, salted by the region's own word so two regions cannot share
        // an egui id — and clipped, which is both meanings at once (`block_panel`'s comment):
        // what is painted, and what the pointer reaches.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("organon-viewport", slot.region.as_word()))
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(content_rect.intersect(ui.clip_rect()));
        match slot.content {
            Some(Content::Agent) if !live_tab_taken => {
                live_tab_taken = true;
                draw_active_pane(&mut child);
            }
            Some(Content::Agent) => paint_region_notice(
                &mut child,
                content_rect,
                slot.region.as_word(),
                "agent — waiting for a tab of its own. The live tab is drawn in the first agent \
                 region; a second one needs Tier 2's per-region tab",
                theme,
            ),
            // 🚨 **The scrolling column of Organon's own editor panels.** `panel_stack::draw`
            // is the whole presentation — it takes the `Ui` it is handed and never reaches for
            // the window's layer, which is what keeps a stack mappable onto a lit surface if
            // the console's own chrome ever becomes one (#17). The panel *bodies* come back
            // through `OrganonDraw`, exactly as they did when a panel was an element in a
            // transcript; only the address changed.
            Some(Content::Panel) if !panels.stacks.get(slot.region).is_empty() => {
                organon_console::panel_stack::draw(
                    &mut child,
                    slot.region,
                    panels.stacks.get(slot.region),
                    theme,
                    panels.form,
                    panels.draw,
                );
            }
            // 🚨 **An empty column is a sentence, and it names the verb that fills it.** Same
            // rule as the vacant region below and with one thing more to say: a region that has
            // been *assigned* and holds nothing looks exactly like one that is broken.
            //
            // ✏️ **It no longer has to be the only place the vocabulary is discoverable from.**
            // The control under this rectangle lists `add` and `remove` on an empty box, so the
            // sentence names the shortest spelling rather than the CLI's.
            // 🚨 **The sentence depends on whether a line was actually drawn**, which is the
            // rule this file keeps re-learning: a notice that names a control the region is
            // too short to show is a status line that cannot be right.
            Some(Content::Panel) if line_rect.is_some() => paint_region_notice(
                &mut child,
                content_rect,
                slot.region.as_word(),
                "panel — an empty column. Type `add surface` in the line below; \
                 `remove all` empties it again",
                theme,
            ),
            Some(Content::Panel) => paint_region_notice(
                &mut child,
                content_rect,
                slot.region.as_word(),
                "panel — an empty column, and this region is too short for a command line. \
                 `organon console stack add <panel> --region <this one>` fills it from a \
                 terminal, or make the region taller",
                theme,
            ),
            // 🚨 **The live 3D viewport — the same mechanism the portal is, in a different
            // rectangle.** `paint_viewport` is one implementation and this is its second call
            // site; nothing about the render, the texture, the gesture or the camera is
            // duplicated here.
            Some(Content::ThreeD) if !viewport.yielded_to_portal => {
                viewport_rect = Some(content_rect);
                paint_viewport(
                    &mut child,
                    content_rect,
                    viewport.image,
                    viewport.input,
                    // A region *is* "a pane inside the workstation, a widget among widgets" —
                    // `SceneMode`'s own words for this variant, written before either
                    // presentation existed. See [`paint_viewport`] on why the mode is passed
                    // rather than assumed.
                    scene_input::SceneMode::Workstation,
                    theme,
                );
            }
            // 🚨 **The loser of [`engine_plan`]'s arbitration says who has the frame and what
            // gives it back — it does not go blank, and it does not show the stale texture it
            // held a moment ago.** §1.14's vacancy rule applies with more force to a picture
            // than to an empty cell: a rectangle that was rendering a world and now is not
            // is exactly what a broken viewport looks like.
            Some(Content::ThreeD) => paint_region_notice(
                &mut child,
                content_rect,
                slot.region.as_word(),
                "3d — the portal has the world. Organon renders at most one frame per console \
                 frame, so the floating portal takes it while it is open; `organon console \
                 portal close` gives it back to this region",
                theme,
            ),
            // 🚨 **Vacant is a sentence, never a blank.** §1.9's `Ring::Empty` argument at the
            // scale of a sixth of a window: a region that draws nothing is indistinguishable
            // from one that is broken, and the console's running tally of "it knew and said
            // nothing" defects is long enough.
            //
            // ✏️ **One arm again, not two.** Tier C gave a vacant region its own command line and
            // this notice named it (*"type `/panel` in the line below"*); the narrowing took that
            // line away, so the sentence names the two doors that actually exist — the console
            // line at an agent, and the CLI. A notice naming a control the region no longer
            // draws is exactly the status line that cannot be right.
            None => paint_region_notice(
                &mut child,
                content_rect,
                slot.region.as_word(),
                "empty — `/viewport <region> panel` at an agent, or `organon console viewport \
                 <region> agent`, `… 3d` or `… panel` from a terminal, fills it",
                theme,
            ),
        }
        // 📌 **The panel column's own control, last, so it sits under whatever the column
        // holds.** Drawn into a child `Ui` of its own at the rectangle reserved above, never
        // into the content's — a line that shared the content's `Ui` would inherit its layout
        // cursor and land wherever the content happened to stop.
        if let Some(rect) = line_rect {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("organon-region-line-host", slot.region.as_word()))
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            let act = organon_console::region_line::draw(
                &mut child,
                organon_console::region_line::Context { region: slot.region },
                panels.registry,
                panels.lines,
                theme,
                panels.form,
            );
            if let organon_console::region_line::Act::Run { name, args } = act {
                // Collected rather than applied: dispatching needs `&mut self`, and `self` is
                // split into disjoint field borrows for the whole of the frame closure. The
                // same arrangement `theme_change` and `panel_wanted` already use, for its
                // reason.
                panels.ran.push((slot.region, name, args));
            }
        }
    }
    paint_region_edges(ui, pane, &placed, theme);
    viewport_rect
}

/// What a `panel` region draws this frame, bundled for [`RegionViewport`]'s reason.
struct RegionPanels<'a> {
    /// **A column per region** — see [`Console::panel_stacks`] on why not one console-wide.
    stacks: &'a organon_console::panel_stack::Stacks,
    /// Every region's command line, and the keyboard owner they share. `&mut` because a line
    /// holds the text somebody is typing into it.
    lines: &'a mut organon_console::region_line::Lines,
    /// The vocabulary a region line dispatches onto — **the console's own registry**, not a
    /// second table. See `region_line`'s header.
    registry: &'a organon_console::registry::Registry,
    /// 🚨 **What a region line's Enter asked for, collected out of the frame closure.** Running
    /// it needs `&mut self` and `self` is split into disjoint field borrows for the whole
    /// closure — `theme_change` and `panel_wanted`'s arrangement, for its reason. The region is
    /// carried so the receipt can be written back above the box that produced it.
    ran: &'a mut Vec<(organon_console::region::Region, String, serde_json::Value)>,
    /// This frame's posture tokens, so a card in a stack and a card in the transcript are the
    /// same object at the same posture rather than two things that resemble each other.
    form: &'a organon_console::posture::Form,
    /// 🚨 **Where a panel's body comes from** — the seam `organon-console` cannot fill, because
    /// it cannot see `OrganicMathParams`, a `ParamSetter` or a `World`. Unchanged from when a
    /// panel was an element in a transcript; only its address moved.
    draw: organon_console::panel_stack::OrganonDraw<'a>,
}

/// What the `3d` region draws this frame — the live viewport's three inputs, bundled so the
/// walk's signature says what it needs rather than growing three more positional arguments.
struct RegionViewport<'a> {
    /// This frame's World render, when the region has it. `None` is the one frame between the
    /// region being asked for and its rect having been measured — [`paint_viewport`] fills it.
    image: Option<egui::TextureId>,
    /// **The console's one camera accumulator**, shared with the portal — see
    /// [`Console::viewport_input`] on why one and not one each.
    input: &'a mut scene_input::SceneInput,
    /// Whether the portal took this frame's World render ([`engine_plan`]'s precedence).
    ///
    /// ⚠️ When true the region paints a **notice**, never the texture: the texture belongs to
    /// the portal this frame, and showing the region's last one would be a picture that has
    /// quietly stopped being live. It also registers no interaction region, which is what keeps
    /// `scene_viewport`'s single egui id to one claimant per frame.
    yielded_to_portal: bool,
}

/// What a region says when it is not the live tab: its own word, then what belongs there.
///
/// Filled rather than left transparent, on [`paint_viewport`]'s rule and for its reason — an
/// outline over whatever the backdrop is painting reads as a rendering failure, which is the
/// confusion the surface path's `rendering…` placeholder already exists to prevent.
fn paint_region_notice(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    heading: &str,
    body: &str,
    theme: &Theme,
) {
    ui.painter().rect_filled(rect, 0.0, theme.panel_fill);
    // The inset is the notice's own, not the region's: the rectangle a region owns is exact
    // (`region_rect` reserves no gutter), so the breathing room is drawn inside it.
    let inner = rect.shrink(REGION_NOTICE_PAD);
    let mut text = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("organon-viewport-notice", heading))
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    text.set_clip_rect(inner.intersect(ui.clip_rect()));
    text.label(egui::RichText::new(heading).monospace().strong().color(theme.panel_title));
    text.label(egui::RichText::new(body).monospace().color(theme.panel_text));
}

/// The hairlines **between** regions — and only between them.
///
/// ⚠️ **Every edge that lies on the pane's own boundary is skipped**, which is what stops a
/// divided console from growing a border it never had: the outer frame of the window is not a
/// separator, and drawing one there would be a visible change to a console that only asked for
/// a split. Duplicate segments (two regions sharing an edge) are drawn twice at the same
/// coordinates, which costs one more line and cannot be seen — cheaper than deduplicating.
fn paint_region_edges(
    ui: &mut egui::Ui,
    pane: Option<egui::Rect>,
    placed: &[organon_console::region::Placed],
    theme: &Theme,
) {
    let Some(pane) = pane else { return };
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.0_f32, theme.panel_edge);
    let inside = |a: f32, b: f32| (a - b).abs() > 0.5;
    for slot in placed {
        let r = slot.rect;
        if inside(r.left(), pane.left()) {
            painter.line_segment([r.left_top(), r.left_bottom()], stroke);
        }
        if inside(r.right(), pane.right()) {
            painter.line_segment([r.right_top(), r.right_bottom()], stroke);
        }
        if inside(r.top(), pane.top()) {
            painter.line_segment([r.left_top(), r.right_top()], stroke);
        }
        if inside(r.bottom(), pane.bottom()) {
            painter.line_segment([r.left_bottom(), r.right_bottom()], stroke);
        }
    }
}

/// The inset a region's notice text sits at, in points. The patch panel's `PAD`, so a notice
/// and a panel in adjacent regions line up rather than each having their own idea of a margin.
const REGION_NOTICE_PAD: f32 = 8.0;

/// Which **presentation** of the viewport the one World frame is being drawn for.
///
/// 🚨 **A viewport is a producer plus a camera plus a texture; this says which rectangle it is
/// shown in.** The portal floats over the transcript and a region sits beside it, and that is
/// the *whole* difference between them — the render, the texture, the sizing, the gesture and
/// the camera are one mechanism underneath, which is what [`Console::render_viewport`] and
/// [`paint_viewport`] are. `organon_world::scene_input::SceneMode` is the other half of the same
/// distinction and is passed to the paint site rather than restated here.
///
/// ⚠️ **Both arms are reachable today**, which is the bar `region.rs` set for adding one at all.
/// There is no `Immersive` arm because there is no immersive presentation — §2's portal row owns
/// it, and an arm nothing can select is an untested branch pretending to be a design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewportTarget {
    /// The floating, screen-anchored rectangle — [`organon_console::portal`].
    Portal,
    /// The region holding `3d` — [`organon_console::region::Content::ThreeD`].
    Region,
}

/// What the engine is asked to draw this frame: the backdrop's source, and **which viewport
/// presentation, if any, gets the World** — pure, so the invariant below is a test rather than a
/// promise.
///
/// 📌 **At most ONE `World` render per frame, in every state, by construction.** That is what
/// this function exists to guarantee, and `the_engine_is_asked_for_at_most_one_frame` proves it
/// over the whole input space. The reason it matters is in [`SURFACE_RENDERS_PER_FRAME`]'s doc:
/// what double-steps when one console frame takes two engine frames is not the wall-clock sims
/// — those advance by microseconds and nobody sees it — but `frame_index`, the TAA jitter phase
/// riding on it, and the temporal history beside it. Those are shared between the two targets.
/// That doc rules the case out on the grounds that a surface shows a *still lit plane*; a live
/// World portal beside a live World backdrop would promote it from a documented non-issue to
/// the default.
///
/// # 🚨 There are now TWO claimants, and the precedence is stated rather than emergent
///
/// **The portal wins.** An open portal takes the frame from a `3d` region exactly as it already
/// takes it from the backdrop, and the argument is the one §1.2 already made rather than a new
/// one: the portal is **temporary and dismissable**, so the state where it holds the frame ends
/// with one word (`organon console portal close`) that is in the same ring as the word that got
/// you there. A region is the persistent thing a person arranged, and it is *still arranged*
/// while the portal is up — nothing is written, nothing is remembered, and closing the portal
/// gives the region its frame back with no stored value to get wrong. That is the same
/// restoration property the backdrop already had, extended to one more claimant rather than
/// re-argued for it.
///
/// The rejected alternative worth naming: **letting the region win** would make `/portal open`
/// a command that appears to do nothing whenever a `3d` region exists, and this lane is
/// fire-and-forget — there is no return path to say why (§1.3, "the refusal reaches nobody").
/// A verb that silently no-ops is the defect this console keeps a tally of. Refusing the second
/// one by name has the same problem for the same reason.
///
/// ⚠️ **The loser does not go blank and does not show a stale texture.** §1.14's vacancy rule at
/// the scale of a picture: a rectangle that draws nothing is indistinguishable from one that is
/// broken. `draw_regions` paints a notice naming what has the frame and the word that releases
/// it — see [`RegionViewport::yielded_to_portal`].
///
/// ⚠️ **The cost, stated rather than discovered: a scene patch shows nothing while either
/// viewport is live.** A patch samples the backdrop's texture, and the promotion that renders a
/// substrate for it (`Off` + `patches_want_image`) is exactly what a viewport displaces. It
/// comes back the moment the last one goes. Two live rectangles showing two different scenes
/// would need the second `World` that `render_surfaces`' doc prices at ~50 shaders and ~62
/// pipelines, and would still trade jitter phases; one at a time is the honest version.
fn engine_plan(
    portal_open: bool,
    region_holds_world: bool,
    backdrop: BackdropSource,
    patches_want_image: bool,
) -> (BackdropSource, Option<ViewportTarget>) {
    if portal_open {
        return (BackdropSource::Off, Some(ViewportTarget::Portal));
    }
    if region_holds_world {
        return (BackdropSource::Off, Some(ViewportTarget::Region));
    }
    let source = if backdrop == BackdropSource::Off && patches_want_image {
        BackdropSource::Substrate
    } else {
        backdrop
    };
    (source, None)
}

impl Console {
    fn new() -> Self {
        let egui_ctx = egui::Context::default();
        // The palette this process opens on. Chosen here and nowhere else, by `theme::select`
        // — `ORGANON_SHELL_THEME`, then `preferences.json`, then `organon`. The precedence
        // lives in that function rather than inline here so it can be tested without a process
        // environment or a store on disk; this end reads the two sources and reports.
        let stored = prefs::Preferences::load_default();
        let selection = theme::select(
            std::env::var(theme::THEME_ENV).ok().as_deref(),
            stored.theme.as_deref(),
        );
        // 🚨 **The notes are printed, never swallowed.** An unknown palette name — in a launch
        // shim or in the stored file — falls through to the next source, which means the
        // console still opens and looks fine. That is the right behaviour and it is also
        // exactly how a typo becomes permanent, so the one thing this must not do is be quiet
        // about it. Empty is the normal case: a stored palette that resolves says nothing.
        for note in &selection.notes {
            eprintln!("organon-console: {note}");
        }
        let mut theme = selection.theme;
        // 🚨 **The tuned colours, laid over the palette that won — the last step of the
        // precedence and deliberately *after* it rather than part of it.** An override is a
        // correction to one named palette, filed under that name, so which palette is being
        // painted has to be settled before there is a question of what corrects it. This also
        // means an override cannot resurrect a palette: `ORGANON_SHELL_THEME=chocolate` picks
        // chocolate, and only chocolate's own tuned colours follow it.
        //
        // ⚠️ **Applied for an environment-selected palette too, which the "loan, never a
        // takeover" rule above might suggest it should not be.** It should: the variable is a
        // loan of *which palette*, and the tuned colours are part of what that palette now
        // looks like on this machine. Skipping them would make a launch shim silently show a
        // palette its owner has never seen.
        if let Some(overrides) = stored.theme_overrides.get(selection.name) {
            for note in theme::apply_overrides(&mut theme, overrides) {
                eprintln!("organon-console: {note}");
            }
        }
        let theme = theme;
        // egui's own chrome — sliders, popup frames, the `TextEdit` selection wash, scrollbars
        // — comes from the palette rather than from a hardcoded `Visuals::dark()`. This call
        // *was* that hardcoded line, and it is why a light palette could not have worked from
        // `Theme`'s fields alone: roughly half the window would have stayed dark. For
        // `Theme::organon` the derivation returns `Visuals::dark()` byte-for-byte, so this
        // console is unchanged — see `Theme::visuals` and the test that pins it.
        egui_ctx.set_visuals(theme.visuals());
        let source =
            parse_backdrop_source(std::env::var("ORGANON_SHELL_BACKDROP").ok().as_deref());
        let shared = initial_shared(source);
        // #4 Tier 2: seed the console sidecar from ONE read, for the reason `World::new`
        // gives at `world.rs:1585-1594` — a split `read_to_string`/`metadata` pair can leave
        // the cached length already matching while the cursor missed the line written
        // between the two calls, and that line is then never drained.
        let (console_cursor, console_len) = match std::fs::read_to_string(cli::console_cmd_path())
        {
            Ok(body) => (agent::cli_seed(&body), body.len() as u64),
            Err(_) => (0, 0),
        };
        // One session per console process. The pid is what keeps two consoles (the co-existence
        // the IPC namespace fork exists for) from interleaving into one file with two
        // independently-advancing `next_seq` counters.
        let session_id = format!(
            "console-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            std::process::id()
        );
        let session_log = match SessionLog::open_default(&session_id) {
            Ok(l) => Some(l),
            Err(e) => {
                eprintln!(
                    "organon-console: session log unavailable ({e}) — console commands will \
                     still apply, but nothing will record that they ran"
                );
                None
            }
        };
        // The loader channel, made before the struct so both ends can be moved into it. It
        // stays open for the console's whole life because the sender is held on the struct —
        // a channel whose last sender dropped would make every later `try_recv` an error.
        let (exhibit_tx, exhibit_rx) = std::sync::mpsc::channel();
        Self {
            window: None,
            gpu: None,
            egui_ctx,
            egui_state: None,
            renderer: None,
            sessions: Vec::new(),
            pane_looks: Vec::new(),
            strip: TabStrip::default(),
            registry: Vec::new(),
            installed: HashSet::new(),
            default_harness: String::new(),
            plus_open: false,
            quit: false,
            // organon#49 T5b — **the Console does not run the AI Performer**, so it has no
            // catalog to give and says so by giving none. Measured, not assumed: this file
            // contains zero references to `agent::dispatch`, `AgentLane`, `ChatMessage`,
            // `HttpChatClient` or `system_prompt`, and nothing here ever bumps `Shared.agent[1]`
            // — the counter whose movement is the only thing that reaches `ensure_agent_worker`.
            //
            // ⚠️ This is NOT the silent-empty-catalog bug `organon-visual`'s manifest warns
            // about. That bug is a host which *does* run the Performer handing it nothing;
            // `World::ensure_agent_worker` now refuses an empty catalog outright and logs why
            // (T5b), so the two cases cannot be confused at runtime.
            //
            // 📌 It is also the last thing tying this file to `param_table`: `core_catalog()`
            // reads the plugin's automation surface and cannot descend, so passing it here is
            // what would have forced `organon-console` to depend upward on the plugin crate.
            world: World::new(Vec::new()),
            backdrop: None,
            backdrop_source: source,
            console_look: ConsoleLook::default(),
            console_cursor,
            console_len,
            session_log,
            pane_points: None,
            window_points: None,
            pane_resized: false,
            shared_writer: None,
            shared,
            organon_panels: Box::new(OrganonPanels::new()),
            occluded: false,
            surfaces: HashMap::new(),
            surface_requests: Vec::new(),
            exhibits: ExhibitContents::new(),
            exhibit_requests: Vec::new(),
            exhibit_textures: HashMap::new(),
            exhibit_inflight: HashSet::new(),
            exhibit_touched: HashMap::new(),
            exhibit_clock: 0,
            exhibit_rx,
            exhibit_tx,
            surface_pane: 0,
            surface_clock: 0,
            // Closed, and not seeded from the environment. `ORGANON_SHELL_BACKDROP` exists and
            // is James's to change (2026-08-11); the portal deliberately gains no twin of it,
            // because the whole claim of this object is that it is **summoned** — a console
            // that opened with a window already floating in it would be back in the state that
            // ruling forbids, by a new route.
            portal_state: PortalState::Closed,
            // One region, `Full`, holding the agent — invariant #4, and `redraw` compares
            // against this exact value to take the pre-region path unchanged.
            layout: organon_console::region::Layout::default(),
            panel_stacks: organon_console::panel_stack::Stacks::default(),
            region_lines: organon_console::region_line::Lines::default(),
            line_registry: organon_console::registry::Registry::new(&console_specs()),
            viewport: None,
            viewport_input: scene_input::SceneInput::default(),
            viewport_points: None,
            hand_camera_at: None,
            agent_camera_at: None,
            viewpoint: camera::ViewpointCell::new(),
            // Built above, because `set_visuals` needs it too — one palette per process, and
            // the chrome and the fields must come from the same one.
            theme,
            theme_name: selection.name,
            // 🚨 **The terminal end, unconditionally — posture is NOT read from the store and
            // has no variable, deliberately.** `organon console posture` can move it while the
            // window is open and every console opens back here. The reasoning is in
            // [`Console::set_posture`]; the short form is that a palette is what the console is
            // made of and a posture is a view you take to look at something, and this axis has
            // never been drawn on a real screen, so "closing it puts it back" is the undo an
            // unaudited layout should still have.
            posture: Posture::TERMINAL,
        }
    }

    fn init_gpu(&mut self, window: Arc<Window>) {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .expect("request adapter");

        // The feature/limit negotiation is `wgpu_editor::bring_up`'s, in full: the
        // engine's cube pipeline needs `max_bind_groups` past wgpu's default of 4,
        // and RT/timestamps are what it probes for. A default-limits device opens
        // the window and then fails to create pipelines.
        let wanted = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::EXPERIMENTAL_RAY_QUERY
            | wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let coopmat_available = adapter
            .features()
            .contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX);
        let f16_available = adapter.features().contains(wgpu::Features::SHADER_F16);

        let mut required_limits = wgpu::Limits::default();
        required_limits.max_bind_groups = adapter.limits().max_bind_groups;
        let required_features = adapter.features() & wanted;
        // wgpu gates EXPERIMENTAL_* behind an acknowledgement token on top of the
        // feature bit. SAFETY: wgpu's "there may be UB bugs in experimental APIs"
        // waiver; all ray-query use is contained in rt.rs (#195's churn rule).
        let experimental_features =
            if required_features.intersects(wgpu::Features::all_experimental_mask()) {
                unsafe { wgpu::ExperimentalFeatures::enabled() }
            } else {
                wgpu::ExperimentalFeatures::disabled()
            };
        if required_features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
            let al = adapter.limits();
            required_limits.max_blas_primitive_count = al.max_blas_primitive_count;
            required_limits.max_blas_geometry_count = al.max_blas_geometry_count;
            required_limits.max_tlas_instance_count = al.max_tlas_instance_count;
            required_limits.max_acceleration_structures_per_shader_stage =
                al.max_acceleration_structures_per_shader_stage;
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("organon-console"),
            required_features,
            required_limits,
            experimental_features,
            ..Default::default()
        }))
        .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format =
            caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: Default::default(),
        };
        surface.configure(&device, &config);

        self.renderer = Some(egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                ..Default::default()
            },
        ));
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        ));
        // The world adopts the device (route C). Its only target here is the
        // backdrop texture, so its composite pipelines build for that format once.
        self.world.attach_gpu(
            device,
            queue,
            BACKDROP_FORMAT,
            None,
            coopmat_available,
            f16_available,
        );
        self.gpu = Some(Gpu { surface, config });
        self.window = Some(window);

        // Publish the default-look snapshot under this edition's namespace so the
        // CLI (and the world) find a live system. Republished each frame in
        // `redraw` — that heartbeat is what `organon watch` follows.
        match ipc::Writer::create() {
            Ok(w) => self.shared_writer = Some(w),
            Err(e) => eprintln!("organon-console: Shared writer unavailable: {e}"),
        }

        // The registry: built-ins + the user's harnesses.json, detection by real
        // PATH probe. Default harness: $ORGANON_SHELL_DEFAULT if valid+installed,
        // else Pi if installed, else the plain shell — the TUI-host opening
        // position (PRD v3.2): your agent greets you, not a cursor.
        self.registry = SessionLog::store_root()
            .map(|r| harness::load(&r))
            .unwrap_or_else(harness::builtin);
        self.installed = harness::detect_installed(&self.registry, harness::on_path);
        self.default_harness = std::env::var("ORGANON_SHELL_DEFAULT")
            .ok()
            .filter(|id| self.installed.contains(id))
            .unwrap_or_else(|| {
                // Pi first (PRD §4.3), and on Windows the WSL entry is a real Pi —
                // usually the only one, since the toolchain lives in the distro.
                ["pi", "pi-wsl"]
                    .into_iter()
                    .find(|id| self.installed.contains(*id))
                    .unwrap_or("shell")
                    .to_string()
            });

        // Initial tabs: `ORGANON_SHELL_CMD` (dev hook, one plain-command tab),
        // else `ORGANON_SHELL_TABS` (comma-separated harness ids), else one tab
        // of the default harness.
        if let Ok(c) = std::env::var("ORGANON_SHELL_CMD") {
            self.open_tab_command(
                "sh".into(),
                Some(organon_console::platform::shell_dash_c(Platform::current(), &c, |k| {
                    std::env::var(k).ok()
                })),
                None,
                "shell".into(),
            );
        } else {
            let ids = std::env::var("ORGANON_SHELL_TABS")
                .unwrap_or_else(|_| self.default_harness.clone());
            for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                self.open_harness_tab(id);
            }
            if self.sessions.is_empty() {
                self.open_harness_tab("shell");
            }
        }
        self.strip.switch(0);
        self.sync_title();
    }

    /// Spawn a tab running `id`'s harness.
    ///
    /// The launch decision — which shell, how to wrap, whether it crosses into WSL,
    /// where its working directory applies — belongs to
    /// [`harness::launch_argv`], not here. This used to build
    /// `/bin/zsh -lc "exec …"` inline, which is why no harness could start on
    /// Windows.
    fn open_harness_tab(&mut self, id: &str) {
        let Some(spec) = self.registry.iter().find(|h| h.id == id).cloned() else {
            eprintln!("organon-console: unknown harness {id:?}");
            return;
        };
        // §5.9: which front-end this tab gets. A conversation spec never reaches
        // `launch_argv` — the flags that make the CLI a persistent session are the
        // agent session's own, not a user-editable argv (see `HarnessSpec::conversation`).
        if spec.conversation {
            // Where the agent works, decided once and out loud — never inherited. See
            // `harness::conversation_cwd` for the four rules and why the product may not
            // name a project of its own.
            let launch = std::env::current_dir().unwrap_or_else(|_| ".".into());
            let resolved = harness::conversation_cwd(
                &spec,
                Platform::current(),
                &launch,
                |k| std::env::var(k).ok(),
                harness::is_project_dir,
            );
            // The labels an inline panel offers, handed down for `claim_patch`'s reason:
            // `organon-console` cannot see `substrate_materials` and must not learn to. It
            // draws them and says which was pressed; this file is the only place that knows
            // a `metal` button and `organon console background metal` are the same act.
            let mut pane = ConversationPane::new(
                Some(resolved.dir.as_str()),
                substrate_materials::MATERIAL_NAMES.iter().map(|s| (*s).to_string()).collect(),
                // …and the knobs, for the same reason and from the same place: a slider label
                // means a lane in `Shared`, which is this file's knowledge and not the
                // compositor crate's. See `surface_slider_table`.
                surface_slider_table(),
                // The console's own vocabulary, served to this tab's agent as MCP tools —
                // built from the **same** `console_specs()` the sidecar drain validates against
                // and the CLI's `--help` is built from, so the three cannot come to know
                // different verbs. `ConsoleDispatch` carries what its schema accepts back onto
                // the one transport that applies it.
                //
                // `mcp_specs()` rather than `console_specs()`: this lane serves one extra verb,
                // the camera read, which exists here and nowhere else because only a caller
                // inside this process has somewhere for an answer to arrive. See `mcp_specs`.
                // The cell is *cloned*, so every tab reads the one the frame path publishes.
                //
                // 🚨 **And the same table is now the composer's slash commands** — the pane
                // builds an `organon_console::registry::Registry` from these very specs, so
                // `/background slate` typed by a human, `mcp__organon__console_background`
                // called by the agent, and `organon console background slate` typed into a
                // terminal are three spellings of one verb. `every_surface_of_a_verb_produces_
                // the_same_console_op` below is what holds that to more than a claim.
                //
                // ⚠️ **`local` is a second `ConsoleDispatch`, not the same one**, because
                // `dispatch` is moved onto the MCP server's serve thread. It is deliberately
                // *not* behind the approval gate: the gate answers "may this agent act on my
                // behalf", and a person's own keystroke was never that question.
                conversation_view::Capabilities {
                    specs: mcp_specs(),
                    dispatch: Box::new(ConsoleDispatch { viewpoint: self.viewpoint.clone() }),
                    local: Box::new(ConsoleDispatch { viewpoint: self.viewpoint.clone() }),
                },
            );
            // Said twice on purpose, to two different readers: into the pane, where it
            // appears at the head of the scrollback for whoever is looking at the console,
            // and onto stderr for whoever started it from a terminal.
            //
            // ⚠️ **stderr is unconditional; the pane is not.** `CwdNote::always` decides which
            // lines a quiet console shows — the bare-project warning always, the resolution
            // itself only under `/trace on` — and the terminal keeps both either way, which is
            // what stops the quiet default from costing a diagnostic rather than a distraction.
            for note in harness::cwd_notes(&resolved) {
                eprintln!("organon-console: {} — {}", spec.name, note.text);
                if note.always {
                    pane.note(note.text);
                } else {
                    pane.trace(note.text);
                }
            }
            // The pane keeps its own failure and shows it; the log line is for whoever
            // started the console from a terminal and is watching stderr.
            if let Some(failure) = pane.failure.as_deref() {
                eprintln!("organon-console: {} — {failure}", spec.name);
            }
            self.push_pane(Pane::Conversation(Box::new(pane)), spec.name.clone(), spec.id.clone());
            return;
        }
        let (argv, cwd) = harness::launch_argv(
            &spec,
            Platform::current(),
            |k| std::env::var(k).ok(),
            harness::on_path,
        );
        self.open_tab_command(spec.name.clone(), Some(argv), cwd, spec.id.clone());
    }

    fn open_tab_command(
        &mut self,
        title: String,
        command: Option<Vec<String>>,
        cwd: Option<String>,
        hid: String,
    ) {
        match TermSession::spawn(80, 24, command, cwd.as_deref()) {
            Ok(s) => self.push_pane(Pane::Term(s), title, hid),
            // The failure a user actually hits is "this harness will not start", so
            // say what was tried, not just the OS error.
            Err(e) => eprintln!(
                "organon-console: failed to spawn {title:?}: {e}\n  \
                 (harness {hid:?}; if this is a WSL entry, check `wsl.exe -- bash -lic 'command -v …'`)"
            ),
        }
    }

    /// Add a pane and the tab that shows it, keeping the three index-aligned vectors
    /// (`sessions`, `pane_looks`, `strip.tabs`) in step. One place, because they are only
    /// ever correct together.
    fn push_pane(&mut self, pane: Pane, title: String, hid: String) {
        self.sessions.push(pane);
        // A new tab's look history starts collapsed at line 0 with whatever the
        // console is wearing right now: it has no rows from before it existed, so
        // there is no older epoch for it to describe. This is `EpochLedger::new`
        // used as the collapse it is — the same shape `background world` produces.
        //
        // A conversation pane gets one too, and it stays inert: the ledger is scrollback
        // arithmetic, and there is no scrollback here. Keeping the vectors the same
        // length is what makes every `zip` and every `get(active)` in this file safe.
        self.pane_looks.push(PaneLooks {
            anchor: PaneAnchor::new(),
            ledger: EpochLedger::new(ledger_look(self.backdrop_source, &self.console_look), 0),
            cache: HashMap::new(),
            blocks: Vec::new(),
        });
        self.strip.add(Tab { title, harness_id: hid });
    }

    fn sync_title(&self) {
        if let (Some(w), Some(tab)) = (self.window.as_ref(), self.strip.active_tab()) {
            w.set_title(&format!("{} — {}", tab.title, PRODUCT_NAME));
        }
    }

    /// Apply one tab action after the egui frame — session lifetimes stay out of
    /// the closure, and closing the last tab quits (a terminal's convention).
    fn apply(&mut self, action: TabAction) {
        match action {
            TabAction::Switch(i) => self.strip.switch(i),
            TabAction::New(id) => self.open_harness_tab(&id),
            TabAction::Close(i) => {
                if i < self.sessions.len() {
                    self.sessions.remove(i);
                }
                // The tab's scrollback is gone, so its look history describes nothing. Free
                // every picture it held that no other tab is still using.
                if i < self.pane_looks.len() {
                    let gone = self.pane_looks.remove(i);
                    for (_, cached) in gone.cache {
                        self.free_cached(cached);
                    }
                }
                // ⚠️ **And every surface texture, because a `SurfaceKey`'s pane is an INDEX
                // into `sessions`** — removing element `i` renumbers everything after it, so
                // a surviving key would silently name a different tab's element. Freeing the
                // lot is one wasted re-render per open surface on the next frame, against a
                // class of bug that would show as one conversation painting into another's
                // rectangle. Same reasoning as the look history above, one level louder.
                self.free_all_surfaces("a tab closed and renumbered the panes");
                if !self.strip.close(i) {
                    self.quit = true;
                }
            }
        }
        self.sync_title();
    }

    /// Drain the console command sidecar and apply whatever survives (#4 Tier 2).
    ///
    /// Called once per frame from [`Console::redraw`], immediately before the snapshot is
    /// published — so a command applied here reaches the World *in the same frame* it drains,
    /// not the next one.
    ///
    /// The transport discipline is the World's for `cli.txt` (`world.rs:9735-9754`),
    /// deliberately and line for line, because it is the same problem: the `organon` CLI is
    /// never an IPC writer, so there is no `Shared` generation counter to watch and growth is
    /// self-detected by file length — one `stat` per frame when idle. `agent::cli_drain_step`
    /// is *reused*, not re-implemented: it already carries the two findings that make this
    /// safe (a failed read commits nothing, so the next frame retries; a shrunk file replays
    /// from zero rather than dropping its content).
    ///
    /// Unknown verbs vanish here without a word — `cli::parse_console_op` returns `None` for
    /// them, which is that format's whole versioning story ("adding a verb is how this format
    /// changes"). A newer `organon` talking to this console degrades to "that op did nothing".
    fn drain_console(&mut self) {
        let path = cli::console_cmd_path();
        let len_now = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let body = if len_now != self.console_len {
            if len_now == 0 {
                Some(String::new())
            } else {
                std::fs::read_to_string(&path).ok()
            }
        } else {
            None
        };
        let Some((lines, new_len, new_cursor)) = agent::cli_drain_step(
            self.console_len,
            len_now,
            body.as_deref(),
            self.console_cursor,
        ) else {
            return;
        };
        self.console_len = new_len;
        self.console_cursor = new_cursor;

        let ops: Vec<cli::ConsoleOp> =
            lines.iter().filter_map(|l| cli::parse_console_op(l)).collect();
        if ops.is_empty() {
            return;
        }
        for op in self.dispatch_console(&ops) {
            self.apply_console(&op);
        }
    }

    /// Route each drained op through the product's first live [`CommandService`], and return
    /// the ones that came back validated.
    ///
    /// **The service is built per batch, not held on `Console`, and that is the shape
    /// `command.rs` asks for.** `CommandService<'log>` borrows `&'log mut SessionLog`; a
    /// `Console` holding both would be self-referential and is not expressible in safe Rust.
    /// Its own doc says the arrangement out loud — "borrows the log rather than owning it —
    /// the log outlives any one service (the app owns it)". Construction costs four
    /// binary-search inserts (two built-ins plus the two console specs) and only happens on a
    /// frame where a command actually arrived.
    ///
    /// `Issuer::Worker("organon-cli")` because that is **what is actually known**. A line on
    /// the sidecar could have been typed by James in a tab or written by an agent in another
    /// process; the console cannot tell, and `Issuer::User` would be a guess recorded as a
    /// fact. The one certainty is the transport, so the record names the transport.
    ///
    /// 🚨 **The no-log path applies without recording, and says so.** If the store could not
    /// be opened there is no service to dispatch through, and the choice is between dropping
    /// the command and applying it unaudited. It applies — the beat outranks the plumbing —
    /// and the shortfall is in SHELL_ARCHITECTURE.md's honesty ledger rather than hidden here.
    /// It is not a silent equivalent: `Console::new` has already printed why on stderr, and the
    /// apply path is total over its own vocabulary regardless (an unknown name changes
    /// nothing), so validation is defence in depth rather than the only gate.
    fn dispatch_console(&mut self, ops: &[cli::ConsoleOp]) -> Vec<cli::ConsoleOp> {
        let target = ConsoleTarget::default();
        let bank = target.accepted.clone();
        let Some(log) = self.session_log.as_mut() else {
            return ops.to_vec();
        };
        let mut service = CommandService::new(log);
        for spec in console_specs() {
            service.register_spec(spec);
        }
        service.register_target(TargetKind::Viewport, Box::new(target));
        for op in ops {
            let args = op_args(op);
            let issuer = Issuer::Worker("organon-cli".into());
            if let Err(e) = service.dispatch(issuer, spec_name(op), args) {
                // The record is already written (every dispatch leaves one, however it
                // ended); this is the human-facing half.
                eprintln!("organon-console: {e}");
            }
        }
        // The binding is required, not stylistic: as a tail expression `bank.borrow()`'s
        // `Ref` would outlive `bank` itself (E0597). Naming it drops the borrow at the end
        // of this statement, before the local goes.
        let accepted = bank.borrow().clone();
        accepted
    }

    /// Apply one validated console op: fold it into `(backdrop_source, console_look)` and
    /// recompute the published snapshot.
    ///
    /// Nothing else moves. The backdrop texture is keyed on the **pane size**, which a
    /// command does not change, so no texture is recreated and no egui id is rebound; the
    /// terminal grid is sized from the same rect it was last frame, so no glyph moves. What
    /// changes is `*self.shared`, which `redraw` publishes a few lines later and the World
    /// reads on the same frame — including the `#472` bake, which re-dispatches because the
    /// layer bytes changed and is a no-op on every frame after (Leaf A's `KNOWN_LIMITS` #4).
    fn apply_console(&mut self, op: &cli::ConsoleOp) {
        // Tier 5: a block changes the transcript, not the dressing. It is routed before
        // `console_step` because that function's domain is `(source, look)` and a block has
        // neither — see its `Block` arm.
        if let cli::ConsoleOp::Block(rows) = op {
            self.open_block(*rows);
            return;
        }
        if let cli::ConsoleOp::Patch { up, rows, kind } = op {
            self.claim_patch(*up, *rows, *kind);
            return;
        }
        // The portal: routed here beside `Block` and `Patch`, for their reason — it changes
        // what the window *holds*, not what the backdrop is dressed in.
        if let cli::ConsoleOp::Portal(cmd) = op {
            let event = match cmd {
                cli::PortalCmd::Open => portal::PortalEvent::Open,
                cli::PortalCmd::Close => portal::PortalEvent::Close,
                cli::PortalCmd::Toggle => portal::PortalEvent::Toggle,
            };
            let next = portal::step(self.portal_state, event);
            if next == self.portal_state {
                return;
            }
            self.portal_state = next;
            // ⚠️ **The texture is NOT freed here, and that is a consolidation rather than an
            // omission.** Closing a portal used to release it on this line; a `3d` region can
            // now stop being live by three more routes (cleared, displaced, or the layout
            // reset), and a release per route is how the one nobody remembered comes to leak.
            // [`Console::render_viewport`]'s gate is the single site, reached every frame, and
            // it is total over every route by construction because it asks [`engine_plan`]
            // rather than asking what just changed.
            return;
        }
        if let cli::ConsoleOp::Camera(framing) = op {
            self.frame_camera(*framing);
            return;
        }
        // 🚨 **The palette and the posture are routed here, BEFORE `console_step`, and that is
        // a correctness rule rather than an arrangement.** Everything below this line ends in
        // [`Console::record_look_change`], which snapshots the backdrop into the Tier-4 epoch
        // ledger — the record of what the *substrate* looked like when a band of scrollback
        // was written. A palette is not a substrate look and a posture is not one either:
        // neither touches `backdrop_source`, neither changes a pixel of what is behind the
        // glyphs, and folding one into the ledger would band the transcript at a moment
        // nothing behind it moved. `Block`, `Patch`, `Portal` and `Camera` are all above for
        // this same reason.
        if let cli::ConsoleOp::Theme(name) = op {
            self.set_theme(name);
            return;
        }
        if let cli::ConsoleOp::Posture(word) = op {
            self.set_posture(word);
            return;
        }
        // Above the ledger for the same reason, and one more of its own: this changes no pixel
        // *behind* the glyphs, and it does not even change the console's drawing — it resizes
        // the window and lets the next frame lay out into whatever it got.
        if let cli::ConsoleOp::Screen(word) = op {
            self.set_screen(word);
            return;
        }
        // Above the ledger for the same reason as the screen, and for one more of its own: the
        // backdrop is still rendered once at the whole pane's size and every region is drawn
        // over the same picture, so nothing *behind* the glyphs moved. See `console_step`'s
        // `Viewport` arm, which is where that argument is written out.
        if let cli::ConsoleOp::Viewport { region, content } = op {
            self.set_viewport(region, content);
            return;
        }
        // Above the ledger for the reason directly above, one level in: this changes which
        // panels a region's column holds, and the backdrop behind that column is the same
        // picture wearing the same dressing.
        if let cli::ConsoleOp::Stack { action, panel, region } = op {
            self.set_stack(action, panel, region.as_deref());
            return;
        }
        // Above the ledger for the reason the two directly above are: a saved arrangement is a
        // recording of the split, and `save`/`delete` do not touch the console's drawing at all.
        if let cli::ConsoleOp::Layout { action, name } = op {
            self.set_layout(action, name);
            return;
        }
        let Some((source, look)) = console_step(self.backdrop_source, &self.console_look, op)
        else {
            eprintln!(
                "organon-console: `{}` names nothing this console knows — ignored",
                cli::console_op_to_line(op)
            );
            return;
        };
        if source == self.backdrop_source && look == self.console_look {
            return;
        }
        // Tier 4: the look that is about to stop being live is the one on screen *right
        // now*, and `self.backdrop` still holds its rendering — `drain_console` runs before
        // `render_backdrop`, so this is the last frame in which that picture exists.
        self.record_look_change(source, ledger_look(source, &look));
        self.backdrop_source = source;
        self.console_look = look;
        *self.shared = *look_shared(self.backdrop_source, &self.console_look);
    }

    /// Repaint in a named palette, **and remember it**.
    ///
    /// Two effects, and they are deliberately not gated on each other:
    ///
    /// 1. **The window**, on the next frame. `redraw` borrows `&self.theme` afresh every
    ///    frame, so the fields need nothing but the assignment; `egui`'s own chrome does not
    ///    work that way — `Visuals` is set on the context and held — so `set_visuals` has to
    ///    be re-issued or roughly half the window (sliders, popups, the selection wash,
    ///    scrollbars) would keep the outgoing palette's colours. That asymmetry is the entire
    ///    reason `Theme::visuals` exists; §1.4 has the measurement.
    /// 2. **`preferences.json`**, so the next console opens here. This is the console's first
    ///    write of a user's choice, and it is what makes the verb a *pick* rather than a
    ///    gesture.
    ///
    /// ⚠️ **No early return when the palette asked for is the one already painted, and the
    /// hole that would open is not hypothetical.** Launch under `ORGANON_SHELL_THEME=chocolate`
    /// over a stored `light`, then type `organon console theme chocolate` — meaning "yes, keep
    /// this one". An early-out on the painted palette would repaint nothing (correct) and
    /// store nothing (wrong), and the choice would evaporate at exit exactly as it did before
    /// any of this existed. The store, not the screen, is what decides whether there is work
    /// to do, and the redundant `set_visuals` costs one frame's worth of nothing.
    ///
    /// ⚠️ **Load, modify, save — never `Preferences { theme }`.** That struct will grow fields,
    /// and constructing a fresh one here would silently discard every preference this call did
    /// not know about. The failure would arrive later, in someone else's tier, looking like
    /// their bug.
    fn set_theme(&mut self, name: &str) {
        // Derived before the move: `Theme` is not `Copy`, and the chrome must come from the
        // same value the fields do — deriving it from `self.theme` afterwards would work today
        // and would be one refactor away from two palettes in one window.
        let (theme, canonical) = match Theme::resolve(name) {
            Ok(v) => v,
            // Refused out loud, never approximated — `Theme::resolve`'s rule. This is the
            // second gate: `bin/ctl.rs` restricts the word at the clap boundary, but a line
            // written straight onto the sidecar reaches here unfiltered, and a console that
            // silently painted its default would make a typo look like a palette nobody likes.
            Err(e) => {
                eprintln!("organon-console: {e} — ignored");
                return;
            }
        };
        let visuals = theme.visuals();
        self.theme = theme;
        self.theme_name = canonical;
        self.egui_ctx.set_visuals(visuals);

        let mut prefs = prefs::Preferences::load_default();
        if prefs.theme.as_deref() == Some(canonical) {
            return;
        }
        prefs.theme = Some(canonical.to_string());
        match prefs.save_default() {
            // The window changing colour is the feedback for the *repaint*; nothing on screen
            // reports the write, so the write is what gets the line. `Preferences::save`
            // returns a result precisely so this can be said rather than swallowed — "I set
            // this and it did not stick" has to be diagnosable.
            Ok(()) => eprintln!(
                "organon-console: theme `{canonical}` — stored, and it will be here next launch"
            ),
            Err(e) => eprintln!(
                "organon-console: painted `{canonical}`, but could not store it ({e}) — it will \
                 be gone at exit"
            ),
        }
    }

    /// Take a palette the live editor changed, and do what it asked with the store.
    ///
    /// The sibling of [`Console::set_theme`] and deliberately not folded into it: that one takes
    /// a *name* and paints a compiled palette, this one takes a *palette* that answers to a name
    /// but is no longer equal to it. Sharing an entry point would mean one function whose
    /// argument decides which of two quite different things it does.
    ///
    /// 🚨 **Painting and storing are separate here for the same reason they are separate in
    /// `set_theme`, but the split lands differently: the common case writes nothing.** A drag
    /// arrives as [`StoreAction::Leave`] many times a second, so touching `preferences.json` on
    /// each one would be a file write per frame, and the editor's `unsaved` marker exists
    /// precisely so it does not have to be.
    ///
    /// ⚠️ **`set_visuals` is re-issued on every change**, not only on a save. Half the window —
    /// sliders, popups, the selection wash, the scrollbars — comes from `Visuals`, which egui
    /// holds on the context rather than reading per frame, so an edit that skipped it would
    /// repaint the console's own painting and leave egui's chrome on the outgoing colours. That
    /// asymmetry is the whole reason `Theme::visuals` exists; `set_theme`'s doc has it in full.
    fn apply_theme_change(&mut self, change: organon_console::theme_edit::ThemeChange) {
        use organon_console::theme_edit::StoreAction;

        let visuals = change.theme.visuals();
        self.theme = change.theme;
        self.egui_ctx.set_visuals(visuals);

        match change.store {
            StoreAction::Leave => return,
            StoreAction::Save | StoreAction::Clear => {}
        }

        // ⚠️ **Load, modify, save — never `Preferences { .. }`.** `set_theme`'s warning, for its
        // reason: the struct grows fields, and a fresh one here would silently discard every
        // preference this call did not know about.
        let mut prefs = prefs::Preferences::load_default();
        let outcome = match change.store {
            StoreAction::Save => {
                let Some(base) = Theme::by_name(&change.name) else {
                    eprintln!(
                        "organon-console: cannot store colours for `{}` — this build has no \
                         palette by that name",
                        change.name
                    );
                    return;
                };
                let overrides = theme::collect_overrides(&self.theme, &base);
                let count = overrides.len();
                if overrides.is_empty() {
                    // Tuned back to exactly the compiled palette. Storing an empty map would
                    // leave a key in the file asserting an override that is not one.
                    prefs.theme_overrides.remove(&change.name);
                } else {
                    prefs.theme_overrides.insert(change.name.clone(), overrides);
                }
                format!("{count} tuned colour(s) for `{}`", change.name)
            }
            StoreAction::Clear => {
                prefs.theme_overrides.remove(&change.name);
                format!("the tuned colours for `{}`, cleared", change.name)
            }
            StoreAction::Leave => unreachable!("returned above"),
        };

        match prefs.save_default() {
            // Nothing on screen reports a *write*, so the write is what gets the line —
            // `set_theme`'s rule. "I saved this and it did not stick" has to be diagnosable.
            Ok(()) => {
                eprintln!("organon-console: stored {outcome} — they will be here next launch")
            }
            Err(e) => eprintln!(
                "organon-console: painted the change, but could not store {outcome} ({e}) — it \
                 will be gone at exit"
            ),
        }
    }

    /// Stand the console differently — **snapping, on the next frame**.
    ///
    /// 🚨 **This does not animate, and the absence is a decision rather than a stub.** The
    /// posture axis exists so a later tier *can* tween it (§1.6: every token is a scalar and
    /// `Form::at` lerps componentwise), but a tween is a much larger question than a verb —
    /// it moves the transcript's wrap width continuously, and `doc/console_rewrap_measurement.md`
    /// prices that at ~7.6 ms per width change at 400 elements with five options and no
    /// decision taken. A snap pays that cost **once**, in a frame nobody perceives as a jump;
    /// an unconsidered tween would pay it every frame of the motion and reflow a wall of text
    /// under someone's eyes to do it.
    ///
    /// ⚠️ **Nothing is stored, and that is the other half of the decision — a posture is not a
    /// preference.** §1.4's palette answers *what the console is made of*, and a person who
    /// picks one means it: it should be there tomorrow. A posture answers *how it stands right
    /// now*, and at this tier that is a view you take to look at something — the desktop end
    /// has never been drawn on a real screen (§3), so a stored `desktop` would mean every
    /// console from then on opens into an unaudited layout, recoverable only by typing the
    /// verb back or editing JSON. Closing the window is a free undo, and it is worth keeping
    /// while the axis is still being auditioned. **Revisit when the tween lands**: an animated
    /// posture that somebody has actually lived in is a preference, and it is one field on
    /// `Preferences` away.
    ///
    /// No `request_repaint` here or in [`Console::set_theme`]: `redraw` ends in
    /// `window.request_redraw()`, so this console is already drawing continuously — which is
    /// also the only reason `drain_console` sees a sidecar line at all.
    fn set_posture(&mut self, word: &str) {
        match Posture::resolve(word) {
            // Refused, not clamped, for `CameraFraming::in_range`'s reason, restated in
            // `Posture::from_scalar`: a typed `90` is degrees where the axis wanted a
            // fraction, and answering `desktop` would let the mistake look like it worked.
            Err(e) => eprintln!("organon-console: {e} — ignored"),
            Ok(p) => self.posture = p,
        }
    }

    /// Fill the display, or give the window its edges back.
    ///
    /// 🚨 **The window is asked where it is, rather than a remembered copy being consulted** —
    /// and that is the whole reason `Console` has no `screen` field. `Window::fullscreen()`
    /// *is* the state; anything beside it would be a second answer to a one-bit question, and
    /// the two can genuinely disagree, because this verb is not the only way a window gets
    /// resized (macOS's green button, a tiling window manager, the platform restoring a
    /// session). After such a divergence a remembered `Windowed` would make `toggle` send a
    /// full-screen window *into* full screen, so the one word whose entire meaning is "the
    /// other one" would do nothing visible and report nothing. `organon_console::screen`'s
    /// header records this as the failure the arrangement forecloses.
    ///
    /// ⚠️ **Borderless, and on the window's current monitor** — `Fullscreen::Borderless(None)`,
    /// which is `organon-visual`'s call. Never `Exclusive`: that takes a video mode from the
    /// display and is a projector's business, not a workstation window's, and it is the variety
    /// that makes alt-tab expensive. Only the *discipline* is shared with that file's
    /// `sync_fullscreen` — touch the window only on a real change — and the **two differ
    /// exactly where it matters**: it holds a `fullscreen_applied` bool and compares against
    /// it, because its intent arrives from `World::wants_fullscreen` every frame and it needs
    /// an edge; this has no periodic intent to debounce, so it asks the window and keeps no
    /// bool at all. **No code is shared, deliberately**: `World::wants_fullscreen` travels in
    /// `Shared` and is written by the visual's own `F` key and its projector launch logic, and
    /// the console's `World` renders only into a backdrop texture and never owns a swapchain —
    /// so reaching for it would mean the console writing into the visual's IPC state to set a
    /// flag on its own window. Two lines of winit is a far smaller price than that coupling.
    ///
    /// ⚠️ **Nothing is stored across launches**, on [`Console::set_posture`]'s rule: the
    /// console opens windowed however you left it. A window that reopens covering the display,
    /// with no title bar, is the state that most needs an undo and has the fewest ways to get
    /// one.
    fn set_screen(&mut self, word: &str) {
        use organon_console::screen::{Screen, ScreenCmd};
        let cmd = match ScreenCmd::resolve(word) {
            // Refused rather than approximated — `Posture::resolve`'s rule: an unrecognised
            // word must not resize a window somebody is looking at.
            Err(e) => {
                eprintln!("organon-console: {e} — ignored");
                return;
            }
            Ok(cmd) => cmd,
        };
        let Some(window) = self.window.as_ref() else {
            // Only reachable if the lane were ever drained before `resumed`, which it is not —
            // `drain_console` runs inside the frame path. Said out loud rather than silently
            // skipped, because a command that vanishes is the failure this whole lane's
            // forward-compatibility story tries to make legible.
            eprintln!(
                "organon-console: `screen {word}` arrived before there was a window — ignored"
            );
            return;
        };
        let now = Screen::from_is_full(window.fullscreen().is_some());
        let want = cmd.apply_to(now);
        if want == now {
            return;
        }
        window.set_fullscreen(want.is_full().then(|| winit::window::Fullscreen::Borderless(None)));
    }

    /// Put a content kind in a region of the pane — **or say, out loud, why not**.
    ///
    /// # 🚨 This is the ONLY gate on an assignment, and it has to be
    ///
    /// `bin/ctl.rs` restricts both words at the clap boundary and [`op_from`] resolves them
    /// again, but neither can answer the question that actually decides a `/viewport` command:
    /// *may this region hold this, given what the console is holding right now?* Overlap, the
    /// last-agent rule and "there is nothing there to clear" are all facts about the **current
    /// layout**, which lives here and nowhere else — and the lane is fire-and-forget, so a
    /// caller cannot read it before writing. Every refusal therefore has to be spoken at this
    /// end, by name, with the region that stood in the way.
    ///
    /// # ⚠️ A displacement is reported, because nobody asked for it in so many words
    ///
    /// `region::Layout::assign` displaces a region that *contains* the one being asked for, or
    /// is contained by it — which is what makes `viewport left agent` work from a console
    /// holding `full`. That is a change to the screen the command did not name, so it is said
    /// rather than swallowed. The alternative — refusing every overlap — makes the first word
    /// of every split a refusal, since the console opens holding `full` and `full off` is
    /// refused by the last-agent rule; `region`'s header has the measurement.
    fn set_viewport(&mut self, region_word: &str, content_word: &str) {
        use organon_console::region::{ContentCmd, Region};
        // Refused rather than approximated — `Posture::resolve`'s rule, and here the cost of an
        // approximation is a window rearranged into a shape nobody named.
        let region = match Region::resolve(region_word) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("organon-console: {e} — ignored");
                return;
            }
        };
        let cmd = match ContentCmd::resolve(content_word) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("organon-console: {e} — ignored");
                return;
            }
        };
        match self.layout.assign(region, cmd) {
            Ok(change) => {
                for gone in &change.displaced {
                    eprintln!(
                        "organon-console: `{}` gave up its place to `{}`",
                        gone.as_word(),
                        region.as_word()
                    );
                }
                self.layout = change.layout;
            }
            // The layout is untouched — `assign` is pure, so there is no half-applied state to
            // unwind and the console goes on drawing exactly what it was.
            Err(refusal) => eprintln!("organon-console: {refusal}"),
        }
    }

    /// Put a panel in the console's column, take one out, or empty it — **or say why not**.
    ///
    /// # 🚨 The only gate that can answer, for [`Console::set_viewport`]'s reason
    ///
    /// clap restricts both words and [`op_from`] resolves them again, but neither can answer
    /// the questions that actually decide this command: *is any region showing a stack*, and
    /// *is the column holding the panel this `remove` names?* Both are facts about state that
    /// lives here, and the lane gets no answer back — so every refusal is spoken at this end,
    /// by name.
    ///
    /// ⚠️ **A stack nothing is showing is refused rather than filled.** A column that exists
    /// only in memory would be a command that appears to work and changes no pixel, which is
    /// the defect this console keeps a running tally of. The refusal carries the `viewport`
    /// line that makes a region to show it in.
    ///
    /// # 🚨 Which column — #98 Tier C
    ///
    /// `region` is the **optional** third word, and it is `None` for every door that has no
    /// region to be typed into: a CLI line, an agent's tool call, and a `/organon` typed at a
    /// conversation. `panel_stack::resolve_target` is the one place the two cases meet — a named
    /// region is checked against the layout, an unnamed one falls to the destination rule — so
    /// the answer cannot be spelled twice and drift.
    fn set_stack(&mut self, action_word: &str, panel_word: &str, region_word: Option<&str>) {
        use organon_console::panel_stack::{resolve_target, Refusal, StackCmd};
        let cmd = match StackCmd::resolve(action_word, panel_word) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("organon-console: {e}");
                return;
            }
        };
        // Resolved here rather than at `op_from`, for `set_viewport`'s reason: an unknown word
        // is the schema's to refuse, but *which* regions hold a column is a fact about the
        // layout at this instant and lives only on `self`.
        let named = match region_word.map(organon_console::region::Region::resolve).transpose() {
            Ok(named) => named,
            Err(e) => {
                eprintln!("organon-console: {e}");
                return;
            }
        };
        // Asked first, and of every arm including `remove`: with nothing showing the column,
        // even emptying it is a change nobody can see.
        let region = match resolve_target(&self.layout, named) {
            Ok(region) => region,
            Err(refusal) => {
                eprintln!("organon-console: {refusal}");
                return;
            }
        };
        // Read before the column is borrowed mutably below. It is a short string built from a
        // handful of slugs, so taking it unconditionally costs nothing — and computing it
        // inside the `NotHeld` arm would be a second borrow of `self` while `column` is live.
        let held = self.held_panel_slugs(region);
        let column = self.panel_stacks.get_mut(region);
        match cmd {
            StackCmd::Add(panel) => {
                column.push(panel);
                // The region is named for the reason `/organon`'s answer names it: several
                // columns now hold different things, so "it was added" leaves a person hunting
                // the window for a panel that is on screen.
                eprintln!(
                    "organon-console: `{}` added to the panel stack in `{}` ({} now)",
                    panel.slug,
                    region.as_word(),
                    column.len()
                );
            }
            StackCmd::Remove(panel) => match column.remove_last(panel.slug) {
                Some(_) => eprintln!(
                    "organon-console: `{}` taken out of `{}`'s panel stack ({} left)",
                    panel.slug,
                    region.as_word(),
                    column.len()
                ),
                // Named rather than shrugged off — `region::Refusal::AlreadyEmpty`'s rule: a
                // command that changes nothing and says nothing is indistinguishable from one
                // that never arrived. The held list is what makes it actionable.
                None => eprintln!(
                    "organon-console: {}",
                    Refusal::NotHeld { slug: panel.slug.to_string(), held }
                ),
            },
            StackCmd::Clear => {
                if column.is_empty() {
                    eprintln!("organon-console: {}", Refusal::AlreadyEmpty);
                    return;
                }
                let n = column.len();
                column.clear();
                eprintln!(
                    "organon-console: `{}`'s panel stack is empty ({n} taken out)",
                    region.as_word()
                );
            }
        }
    }

    /// Write the console's arrangement down under a name, bring one back, or take one out —
    /// **or say why not**.
    ///
    /// # 🚨 A load is a TRANSACTION, and this is the line where that is spent
    ///
    /// `doc/organon_is_the_product.md` §4: *"a layout that cannot be drawn must say so and leave
    /// the current one standing, never half-apply."* `layout::resolve` validates the whole
    /// arrangement and answers either one finished `region::Layout` or one sentence, so the
    /// application below is a **single assignment**. There is no loop over placements here, and
    /// there must never be one: a partial apply that had evicted the last agent region is a
    /// console with nothing to type into, and the verb that would fix it is typed at an agent.
    ///
    /// # 🚨 The one gate, for [`Console::set_viewport`]'s reason
    ///
    /// clap restricts the action word, and [`op_from`] resolves it and the name again — but
    /// neither can answer the questions that decide this command: *is a layout stored under that
    /// name*, *does it still resolve against this build's vocabulary*, and *can today's window
    /// draw it?* The first is a fact about a file that may be written between dispatch and drain;
    /// the last is a fact about a window that may be resized in the same gap. So every refusal is
    /// spoken here, by name.
    ///
    /// ⚠️ **The library is re-read per command rather than held in memory.** It is a small file
    /// and it is the truth: a copy cached at startup would fight a hand-edited `layouts.json` and
    /// win silently. The cost is that two consoles saving at once resolve as last-writer-wins —
    /// never a torn file (the write is a rename), but the loser's layout is simply not there.
    fn set_layout(&mut self, action_word: &str, name: &str) {
        use organon_console::layout::{self, LayoutCmd, Library, Refusal, SavedLayout};
        let cmd = match LayoutCmd::resolve(action_word) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("organon-console: {e}");
                return;
            }
        };
        // Checked again at this end because a line written straight onto the sidecar by hand
        // never met `op_from` — and a name that cannot travel is exactly what such a line has.
        if let Err(e) = layout::check_name(name) {
            eprintln!("organon-console: {e}");
            return;
        }
        let Some(root) = Library::store_root() else {
            eprintln!(
                "organon-console: this platform has no data directory, so layouts cannot be \
                 stored or read"
            );
            return;
        };
        let mut library = Library::load(&root);
        // ⚠️ **The pane, as the console last measured it — one frame behind, exactly as
        // `pane_points` is documented to be.** Only the *size* reaches `region_rect` (it splits
        // at midpoints and measures the sides), so the origin is arbitrary. `None` — no frame
        // drawn yet — means the size question is not asked at all, and the draw path's own
        // "the window is too small for this layout" sentence is the backstop.
        let pane = self
            .pane_points
            .map(|(w, h)| egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h)));
        match cmd {
            LayoutCmd::Save => {
                let replaced = library.upsert(SavedLayout::capture(name, &self.layout));
                match library.save(&root) {
                    // The replacement is said out loud: overwriting an arrangement somebody
                    // assembled is a change they did not name in so many words, and no command
                    // rebuilds the one that was there.
                    Ok(()) => eprintln!(
                        "organon-console: `{name}` {} — {} ({} saved)",
                        if replaced { "replaced the layout that was saved under it" } else { "saved" },
                        self.layout
                            .occupied()
                            .iter()
                            .map(|(r, c)| format!("{} {}", r.as_word(), c.as_word()))
                            .collect::<Vec<_>>()
                            .join(", "),
                        library.layouts.len()
                    ),
                    Err(e) => eprintln!(
                        "organon-console: {}",
                        Refusal::NotWritten {
                            path: root.join(layout::LAYOUTS_FILE).display().to_string(),
                            error: e.to_string(),
                        }
                    ),
                }
            }
            LayoutCmd::Load => {
                let Some(saved) = library.get(name) else {
                    eprintln!(
                        "organon-console: {}",
                        Refusal::NoSuchLayout {
                            name: name.to_string(),
                            known: library.names_or_nothing(),
                        }
                    );
                    return;
                };
                match layout::resolve(saved, pane) {
                    // 🚨 One assignment. See this function's header.
                    Ok(next) => {
                        let same = next == self.layout;
                        self.layout = next;
                        eprintln!(
                            "organon-console: `{name}` loaded — {}{}",
                            self.layout
                                .occupied()
                                .iter()
                                .map(|(r, c)| format!("{} {}", r.as_word(), c.as_word()))
                                .collect::<Vec<_>>()
                                .join(", "),
                            // Said rather than swallowed, on `region::Refusal::AlreadyEmpty`'s
                            // rule: a command that changes nothing and says nothing is
                            // indistinguishable from one that never arrived. It is not a refusal
                            // — loading the arrangement you are already in is a perfectly good
                            // way to be sure of it.
                            if same { " (which is what it was already holding)" } else { "" }
                        );
                    }
                    // The layout is untouched — `resolve` is pure, so there is no half-applied
                    // state to unwind and the console goes on drawing exactly what it was.
                    Err(refusal) => eprintln!("organon-console: {refusal}"),
                }
            }
            LayoutCmd::Delete => {
                if !library.remove(name) {
                    eprintln!(
                        "organon-console: {}",
                        Refusal::NoSuchLayout {
                            name: name.to_string(),
                            known: library.names_or_nothing(),
                        }
                    );
                    return;
                }
                match library.save(&root) {
                    Ok(()) => eprintln!(
                        "organon-console: `{name}` deleted ({} left: {})",
                        library.layouts.len(),
                        library.names_or_nothing()
                    ),
                    Err(e) => eprintln!(
                        "organon-console: {}",
                        Refusal::NotWritten {
                            path: root.join(layout::LAYOUTS_FILE).display().to_string(),
                            error: e.to_string(),
                        }
                    ),
                }
            }
        }
    }

    /// What **one region's** column is holding, for a refusal to quote. `"nothing"` rather than
    /// an empty string, so the sentence reads as a sentence.
    fn held_panel_slugs(&self, region: organon_console::region::Region) -> String {
        let column = self.panel_stacks.get(region);
        if column.is_empty() {
            return "nothing".to_string();
        }
        column
            .entries()
            .iter()
            .map(|e| e.panel().slug)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Move the viewer's viewpoint — **unless a hand is on it**.
    ///
    /// # 🚨 The hand always wins, and this is where that is enforced
    ///
    /// `organon console camera` and a drag on the portal write the same three fields, and
    /// `World::apply_camera_input` cannot tell them apart — so without this the last writer in
    /// the frame would win by accident, and a command landing mid-drag would move the picture
    /// under a hand that is holding it. **A control that fights your hand is worse than no
    /// control.** The policy, the hold and the argument for its length live in
    /// [`organon_console::camera`]; this is the site that obeys them.
    ///
    /// The refused command is **dropped**, never queued — see that module on why a deferred
    /// framing is the same failure delayed.
    ///
    /// ⚠️ **The refusal reaches nobody but a reader of this process's stderr.** The console
    /// command lane is fire-and-forget with no return path by design, so the caller cannot be
    /// told. That gap is in `SHELL_ARCHITECTURE.md`'s honesty ledger.
    ///
    /// # The other thing it says out loud
    ///
    /// A framing applied while **nothing is drawing the world** succeeds, moves real state, and
    /// changes not one pixel — an installed substrate rig overrides the whole camera tuple, and
    /// `off` draws nothing at all. That is the silent trap `portal.rs`'s docs argue about, met
    /// from the other side. The camera really did move and will be there the moment something
    /// shows it, so this reports rather than refuses.
    fn frame_camera(&mut self, framing: cli::CameraFraming) {
        if camera::arbitrate(self.hand_camera_at, Instant::now()) == camera::Verdict::HandHolds {
            eprintln!(
                "organon-console: `{}` ignored — a hand is on the camera (it holds for {} s \
                 after the last drag or wheel). Nothing was queued; ask again.",
                cli::console_op_to_line(&cli::ConsoleOp::Camera(framing)),
                camera::HAND_HOLD.as_secs(),
            );
            return;
        }
        // `reset` first, so `--reset --distance 40` reads as "the default view, then pull in".
        // One `Frame` message either way: the reset supplies the defaults for the axes the
        // caller did not name, and the caller's own values override them.
        let framing = if framing.reset {
            scene_input::CameraInput::Frame {
                yaw: Some(framing.yaw.unwrap_or(scene_input::DEFAULT_YAW)),
                pitch: Some(framing.pitch.unwrap_or(scene_input::DEFAULT_PITCH)),
                distance: Some(framing.distance.unwrap_or(scene_input::DEFAULT_DISTANCE)),
            }
        } else {
            scene_input::CameraInput::Frame {
                yaw: framing.yaw,
                pitch: framing.pitch,
                distance: framing.distance,
            }
        };
        self.world.apply_camera_input(framing);
        // After the apply and after the arbitration, so the stamp means "an agent moved this
        // camera" rather than "an agent asked". The read reports it as `moved_by`; a framing the
        // hand held off returned above and never reaches here.
        self.agent_camera_at = Some(Instant::now());
        if !camera::viewpoint_is_visible(
            self.portal_state.is_open(),
            self.region_showing_world().is_some(),
            self.render_source() == BackdropSource::World,
        ) {
            eprintln!(
                "organon-console: the camera moved, but nothing on screen is showing the \
                 world — `organon console portal open`, `organon console viewport <region> 3d`, \
                 or `organon console background world`. (A substrate backdrop frames its own \
                 plane and ignores the viewpoint entirely.)"
            );
        }
    }

    /// Reserve `rows` blank rows in the **active** pane's transcript (Console Spike Tier 5).
    ///
    /// The mechanism is [`organon_console::term_view::PaneAnchor::feed_local`]: bytes the
    /// console generated itself, through the same parser the child's bytes go through, inside
    /// the same bracket that keeps the scroll anchor's `dropped` counter honest. The rows are
    /// ordinary scrollback rows — they scroll, age, evict and reflow like every other row —
    /// which is the whole reason for doing it this way rather than keeping a parallel list of
    /// "reserved" lines beside the buffer. Nothing is painted into them yet; this increment
    /// makes the hole, and a later one fills it.
    ///
    /// **The active pane, not every pane** — the opposite of [`Console::record_look_change`],
    /// and for the same reason it is the opposite. A look change is the *window's*: every tab
    /// accumulates rows under it and must paint them correctly when switched to. A block is a
    /// hole in *one transcript*, asked for by someone looking at one tab; opening it in every
    /// tab would punch holes in transcripts nobody asked about.
    ///
    /// ⚠️ **The sidecar is drained once per frame and is out of band with the PTY byte
    /// stream**, so the line this block opens at is "wherever the cursor was at drain time" —
    /// correct only while the child is idle. A shell that is mid-output when the drain lands
    /// gets its rows opened in the middle of that output. The in-band fix is a private OSC
    /// sequence scanned in `pump`, so that the console learns where the block goes *from the
    /// byte stream itself*, in order with everything else on it; that is a later increment,
    /// not a defect of this one.
    fn open_block(&mut self, rows: u16) {
        let pane = self.strip.active;
        // A conversation tab has no grid to punch a hole in — its inline artifacts are
        // elements in a flow, which is the whole reason §5.9 split the front-ends. The
        // verb is silently inapplicable rather than wrong.
        let (Some(Some(session)), Some(looks)) = (
            self.sessions.get_mut(pane).map(Pane::term_mut),
            self.pane_looks.get_mut(pane),
        ) else {
            return;
        };
        if rows == 0 {
            return;
        }
        let at = looks.anchor.feed_local(session, &term::block_bytes(rows));
        // A scene, always: this verb predates kinds and nothing on it can name one. It is kept
        // only for a shell that is provably idle — `claim_patch` is the mechanism that works.
        looks.blocks.push(Patch::scene(Block { first_abs: at, rows }));
        // Unconditional, and in `[epochs]`' register: the absolute index is the one number a
        // painter will place a rect from, and an arithmetic error in it is invisible on screen
        // until something is painted into the wrong rows. `[block]` is the tag to grep for.
        eprintln!("[block] opened {rows} rows @ line {at} (pane {pane})");
    }

    /// Record a rectangle the writer already left in its own output — **the console writes
    /// nothing** (Console Spike Tier 5, the corrected mechanism).
    ///
    /// 🚨 **Why this exists and [`Console::open_block`] is not enough.** `open_block` feeds
    /// blank rows at the cursor. But the cursor *is* the live input point — the row a shell's
    /// prompt sits on and a keystroke lands in — so feeding there opens the hole **between the
    /// prompt and the typing**. Measured 2026-08-11: prompt stranded above an eight-row hole
    /// with the cursor below it, and against a real Claude Code tab the harness's whole frame
    /// shifted and it repainted over everything. That is not a failure mode to work around; it
    /// is the mechanism being wrong. No terminal puts a hole between a prompt and its input.
    ///
    /// The writer, by contrast, knows what it is about to print. It emits the gap as ordinary
    /// blank lines through the ordinary PTY — rows the shell, ConPTY and the console all agree
    /// exist, because they arrived the normal way — and then says where they are. This
    /// function only records.
    ///
    /// `up` counts back from the line the cursor is on **now**, which is the line the claiming
    /// command is being run from. Zero means the rectangle starts on that line.
    ///
    /// ⚠️ Still true, and unchanged by this: the sidecar is drained once per frame and is out
    /// of band with the PTY byte stream, so "the line the cursor is on now" is resolved at
    /// drain time. A writer that prints its gap and claims it in the same breath is fine; one
    /// that keeps printing in between is not. The in-band fix is the OSC 8 claim in
    /// `doc/console_patch_protocol.md`, which resolves the anchor from the *cells* rather than
    /// from a clock.
    ///
    /// # The kind selects the paint, and nothing before it
    ///
    /// Everything above this line is identical for every kind, and that is a design constraint
    /// rather than a happy accident: the claim, the anchor arithmetic and the ledger entry are
    /// where an error is **invisible on screen** — a rectangle at the wrong line looks like a
    /// rectangle — so they get one implementation and one set of tests. The kind is read for
    /// the first time when the rows are painted, where a mistake is a thing you can see.
    ///
    /// [`kind::Kind::Panel`] is the one arm that carries state, and it is built here
    /// because the button labels are handed **down**: `organon-console` cannot see
    /// `substrate_materials` and must not learn to. It draws the labels and reports which one
    /// was pressed; this file is the only place that knows a `metal` button and
    /// `organon console background metal` are the same act.
    fn claim_patch(&mut self, up: u16, rows: u16, kind: kind::Kind) {
        let pane = self.strip.active;
        // Terminal-only, for [`Console::open_block`]'s reason.
        let (Some(Some(session)), Some(looks)) = (
            self.sessions.get_mut(pane).map(Pane::term_mut),
            self.pane_looks.get_mut(pane),
        ) else {
            return;
        };
        if rows == 0 {
            return;
        }
        // `boundary_now` is the absolute line just *below* the cursor — the same coordinate a
        // look change opens an epoch at. One back from it is the cursor's own line, and `up`
        // counts from there.
        let cursor_abs = looks.anchor.boundary_now(session).saturating_sub(1);
        let first_abs = cursor_abs.saturating_sub(u64::from(up));
        let block = Block { first_abs, rows };
        looks.blocks.push(match kind {
            kind::Kind::Scene => Patch::scene(block),
            kind::Kind::Panel => Patch::panel(
                block,
                BlockPanel::new(
                    format!("◈ organon · patch @ line {first_abs} · {rows} rows"),
                    substrate_materials::MATERIAL_NAMES
                        .iter()
                        .map(|s| (*s).to_string())
                        .collect(),
                ),
            ),
            // 🚨 **The claim is honoured and the rows say what is in them**, which is the
            // whole of `PatchContent::media_notice`'s argument: the CLI offers these words, so
            // refusing the line here would be a dispatch that succeeds and paints nothing —
            // the exact failure `every_shared_kind_has_exactly_one_patch_arm` exists to catch.
            // The picture itself is a conversation-front-end placement in this tier; a
            // terminal pane has no `ElementId` to key a texture on and no per-patch texture
            // ledger, and building both is #56 T5/T6.
            kind::Kind::Image => Patch { block, content: PatchContent::Image },
            kind::Kind::Markdown => Patch { block, content: PatchContent::Markdown },
        });
        eprintln!(
            "[patch] claimed {rows} rows @ line {first_abs} ({}, up {up}, pane {pane})",
            kind.as_word()
        );
    }

    /// Close the live look-epoch in every pane and open the next one — the Tier 4 half of
    /// [`Console::apply_console`].
    ///
    /// **Every pane, not just the visible one.** A look change is the window's, and each tab
    /// records it at its own cursor, in its own absolute-line coordinate; a tab that is
    /// nowhere near the screen still accumulates rows under the new look and will have to
    /// paint them correctly the moment it is switched to.
    ///
    /// The order inside is [`EpochLedger::plan`]'s order, for its reason: **evictions are
    /// released before the new picture is allocated**, so the transient peak never exceeds
    /// `substrate_epochs::MAX_EPOCHS` textures even on the change that fills the ledger.
    fn record_look_change(&mut self, next_source: BackdropSource, next: Look) {
        // `world` / `off` — collapse. A live World is not a still life, and an `off` backdrop
        // has no picture at all, so none of the cached substrate epochs describes what is on
        // screen any more. History is forgotten deliberately and loudly (every eviction logs);
        // what it buys is that the rows written *under* `off` end up in an epoch with no
        // image, so they keep the plain background they were actually written on.
        if next_source != BackdropSource::Substrate {
            let mut evicted: Vec<(usize, EpochId, String)> = Vec::new();
            for (i, pane) in self.pane_looks.iter_mut().enumerate() {
                for ev in pane.ledger.collapse_to(next.clone()) {
                    evicted.push((i, ev.id, ev.log_line()));
                }
            }
            self.retire_epochs(evicted);
            return;
        }

        // A substrate look. Where each pane opens it: just below its own cursor, so the
        // change is visible in the frame it is asked for rather than a screenful later.
        //
        // 📌 `scroll_anchor::push_boundary` is deliberately NOT used, and this is the one
        // place its absence could look like an oversight. It exists to keep a bare
        // `Vec<u64>` ascending; here the ledger owns the list and `EpochLedger::open`
        // already clamps to `previous + 1` — **strictly** forward, where `push_boundary`
        // clamps to `>=` — so the ledger's rule is the stronger of the two and adding the
        // weaker one on top would be a second place that decides where an epoch starts.
        //
        // A conversation pane opens at 0: it has no cursor and no scrollback, so there
        // is no line to name. `EpochLedger::open` clamps to `previous + 1`, so the
        // ledger stays well-formed and inert — which is what an unused ledger should be.
        let opening: Vec<u64> = self
            .sessions
            .iter()
            .zip(&self.pane_looks)
            .map(|(pane, looks)| match pane.term() {
                Some(session) => looks.anchor.boundary_now(session),
                None => 0,
            })
            .collect();

        let mut evicted: Vec<(usize, EpochId, String)> = Vec::new();
        // Which epoch each pane just closed — read before `open` mints the new one, and the
        // key its picture will be filed under below.
        let mut closing: Vec<(usize, EpochId)> = Vec::new();
        for (i, (pane, at)) in self.pane_looks.iter_mut().zip(opening).enumerate() {
            let was = pane.ledger.current_id();
            let out = pane.ledger.open(next.clone(), at);
            if !out.opened {
                continue; // the same look, or the line coordinate is exhausted — no churn.
            }
            closing.push((i, was));
            if let Some(ev) = out.evicted {
                evicted.push((i, ev.id, ev.log_line()));
            }
        }
        self.retire_epochs(evicted);

        // **Snapshot on close.** The live backdrop texture IS the closing look's rendering —
        // there is no path here that re-renders an arbitrary past look, deliberately (the
        // plan: "do not build a restyle-everything path"), so this one copy is the only
        // moment that picture can be kept. One copy for the whole window, shared by `Rc`:
        // every pane closed the same picture.
        if closing.is_empty() {
            return;
        }
        // ⚠️ **Not while the closing look is `off`.** `render_backdrop` returns early at
        // `off` without rendering, so the backdrop texture still holds whatever was there
        // *before* `off` — copying it would hand the `off` epoch a picture of a look those
        // rows were never written under. `self.backdrop_source` is still the closing source
        // here; the caller updates it after this returns.
        let picture = if self.backdrop_source == BackdropSource::Off {
            None
        } else {
            self.snapshot_live_backdrop()
        };
        let Some(picture) = picture else {
            // No live texture to copy — the backdrop was `off`, or this is the first frame
            // and nothing has rendered yet. The closed epoch keeps no image, and its band
            // paints the plain background, which is what those rows were written on.
            return;
        };
        for (i, id) in closing {
            self.pane_looks[i].cache.insert(id, picture.clone());
        }
    }

    /// Log and free a batch of evicted epochs: the two things `substrate_epochs::Evicted`
    /// asks its integrator for, in one place so neither can be forgotten.
    fn retire_epochs(&mut self, evicted: Vec<(usize, EpochId, String)>) {
        for (pane, id, line) in evicted {
            // Unconditional: a cap that silently drops history is indistinguishable from a
            // bug in the anchor arithmetic. `[epochs]` is the tag to grep for.
            eprintln!("{line}");
            self.release_epoch(pane, id);
        }
    }

    /// Drop one pane's claim on a cached picture, and free the GPU objects if it was the
    /// last claim on it. The one release site; [`Console::apply_epoch_plans`] is the belt.
    fn release_epoch(&mut self, pane: usize, id: EpochId) {
        let Some(cached) = self.pane_looks.get_mut(pane).and_then(|p| p.cache.remove(&id))
        else {
            return;
        };
        self.free_cached(cached);
    }

    /// Free a cached picture's egui registration **iff** no other pane still holds it —
    /// `register_native_texture`'s no-leak discipline, refcounted.
    fn free_cached(&mut self, cached: Rc<CachedEpoch>) {
        let Ok(cached) = Rc::try_unwrap(cached) else {
            return; // another tab's ledger still describes this look.
        };
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.free_texture(&cached.id);
        }
    }

    /// Copy the live backdrop into a texture of its own and register it with egui.
    ///
    /// The formats are the [`Backdrop`] pair exactly — `Rgba8UnormSrgb` storage with an
    /// `Rgba8Unorm` sample view (brief R1's measured gamma arrangement), so a band sampling
    /// history linearizes exactly once, like the live band beside it. `COPY_DST` replaces
    /// `RENDER_ATTACHMENT`: nothing renders into a closed epoch, by design.
    ///
    /// ⚠️ **The size is frozen at the moment of the copy.** A later pane resize leaves every
    /// cached picture at the old resolution, and [`term_view::band_quads`] stretches it into
    /// the band. That is a deliberate cut rather than an oversight: re-rendering it correctly
    /// means re-deriving a past look's `Shared` and drawing the world again per epoch, which
    /// is the unbounded cost the cap exists to prevent. A stretched history is honest — it is
    /// the picture that was there — and the live band, the one the eye is on, is always exact.
    ///
    /// 📌 **That cut only stays honest while the live texture is the size it claims to be**,
    /// which is why [`scene_input::pane_pixels_in`] exists. The first version of the sizing
    /// sized the backdrop as `pane_points × remembered_scale`, and the value standing in for a
    /// scale nobody had measured yet multiplied like a real 100 % display — so the live
    /// texture spent its first frames in POINTS, and a look closing in that window filed a
    /// picture 2.25× too small on this machine's display. The live target rebound itself a
    /// frame later; the snapshot could not, and every band painted from it was magnified back
    /// up for the rest of the session (measured: a 1100×690 epoch picture across a 2475×1553
    /// pane). A frozen size is a cut; a frozen *wrong* size was a bug.
    fn snapshot_live_backdrop(&mut self) -> Option<Rc<CachedEpoch>> {
        let device = self.world.device()?.clone();
        let queue = self.world.queue()?.clone();
        let (texture, view) = {
            let pane = self.backdrop.as_ref()?;
            // No egui id means this texture has never been painted; there is nothing on
            // screen for it to be a picture of.
            pane.id?;
            let (w, h) = pane.size;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shell-backdrop-epoch"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: BACKDROP_FORMAT,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[BACKDROP_SAMPLE_FORMAT],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("shell-backdrop-epoch-sample"),
                format: Some(BACKDROP_SAMPLE_FORMAT),
                ..Default::default()
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shell-backdrop-epoch-copy"),
            });
            encoder.copy_texture_to_texture(
                pane.texture.as_image_copy(),
                texture.as_image_copy(),
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            // Submitted here rather than folded into the frame's encoder: this runs from the
            // command drain, before the egui pass exists, and the copy has to land before
            // the next `render_to_texture` overwrites the source with the new look.
            queue.submit(std::iter::once(encoder.finish()));
            (texture, view)
        };
        let renderer = self.renderer.as_mut()?;
        let id = renderer.register_native_texture(&device, &view, wgpu::FilterMode::Linear);
        Some(Rc::new(CachedEpoch { texture, view, id }))
    }

    /// Reconcile every pane's texture set against its ledger, once per frame.
    ///
    /// [`EpochLedger::plan`] is a **total** description of what should exist, so this is the
    /// place where a texture cannot quietly outlive the epoch it belongs to. Four of the five
    /// arms are deliberate no-ops, and each one is a decision rather than an omission:
    ///
    /// * **the live epoch** — whatever the plan says about it, its texture is the live
    ///   backdrop, owned by [`Console::render_backdrop`], which already recreates it on resize;
    /// * **`Create` for a closed epoch** — there is no picture and no way to make one: a
    ///   closed look is only ever captured by [`Console::snapshot_live_backdrop`], at the
    ///   instant it closed. This fires for epochs that closed while the backdrop was off, and
    ///   for a tab whose history predates its ledger. Those bands paint nothing;
    /// * **`Rerender` for a closed epoch** — the pane resized; the picture is stale-sized and
    ///   gets stretched (see [`Console::snapshot_live_backdrop`]);
    /// * **`Retain`** — nothing to do, which is the steady state.
    ///
    /// `Release` is the arm that does work, and it is a *belt*: evictions already release
    /// directly in [`Console::record_look_change`], so anything reaching here is drift.
    fn apply_epoch_plans(&mut self) {
        let live_backdrop = self.backdrop.as_ref().and_then(|b| b.id).is_some();
        let mut releases: Vec<(usize, EpochId, String)> = Vec::new();
        for (i, pane) in self.pane_looks.iter().enumerate() {
            let mut held: Vec<EpochId> = pane.cache.keys().copied().collect();
            if live_backdrop {
                // The live epoch's texture exists — it is the backdrop being rendered this
                // frame — so declaring it held is what makes the plan describe reality.
                held.push(pane.ledger.current_id());
            }
            for action in pane.ledger.plan(&held, self.pane_resized) {
                if let SlotAction::Release { id } = action {
                    releases.push((
                        i,
                        id,
                        format!("[epochs] released orphaned texture for epoch {id} (pane {i})"),
                    ));
                }
            }
        }
        self.retire_epochs(releases);
    }

    /// The active pane's backdrop, cut into look-epochs: the boundary list
    /// [`term_view::band_quads`] consumes, and one texture per epoch, oldest first.
    ///
    /// ⚠️ **The first boundary is dropped, and that is the seam between the two leaves.**
    /// [`EpochLedger::boundaries`] records the line every epoch opened at, *including the
    /// oldest*; `scroll_anchor` counts boundaries at or below a line to get an epoch index,
    /// so it wants only the changes **between** looks. Hand it the ledger's list unfiltered
    /// and every row shifts one epoch younger — a silent, uniform mis-colouring. The
    /// `textures.len() == boundaries.len() + 1` law is what makes the mistake catchable, and
    /// the test below is what catches it.
    fn band_table(
        ledger: &EpochLedger,
        live: Option<egui::TextureId>,
        cached: impl Fn(EpochId) -> Option<egui::TextureId>,
    ) -> (Vec<u64>, Vec<Option<egui::TextureId>>) {
        let live_id = ledger.current_id();
        let textures = ledger
            .epochs()
            .iter()
            .map(|e| if e.id == live_id { live } else { cached(e.id) })
            .collect();
        let mut boundaries = ledger.boundaries();
        if !boundaries.is_empty() {
            boundaries.remove(0);
        }
        (boundaries, textures)
    }

    /// The engine's frame, sized to the pane it is painted into, behind the glyphs (tree E
    /// Tier 1; Console Spike Tier 1 fixed its aspect and gave it a second source).
    /// Does the active pane hold a patch that needs a picture?
    ///
    /// The active pane only: a patch is painted where it is looked at, and rendering for a
    /// tab nobody is watching would be a scene per background tab.
    ///
    /// **Scene patches only, since Tier 5's kinds.** A panel is egui widgets in the same pass
    /// that draws the glyphs — it has nothing to sample — so a pane holding only panels must
    /// not summon the engine. Asking "are there any patches" instead would render a substrate
    /// for a console that never shows one, which is invisible and costs a frame's worth of GPU
    /// forever.
    fn patches_want_image(&self) -> bool {
        self.pane_looks
            .get(self.strip.active)
            .is_some_and(|p| p.blocks.iter().any(Patch::is_scene))
    }

    /// What the engine should draw this frame, which is **not** the same question as what the
    /// backdrop should paint.
    ///
    /// James's rule, 2026-08-11: *the console must open looking exactly like an ordinary
    /// terminal.* Painting the whole window is the one move that says "this is a picture with
    /// text on it" — the opposite of the claim — and terminal backgrounds have been a solved,
    /// unremarkable thing for thirty years, so leading with one invites the wrong comparison
    /// entirely. A patch is the interesting object: a rendered thing living **in** the page.
    ///
    /// So the two decisions are separated. `backdrop_source` still decides whether the
    /// full-window quad is painted; this decides whether a scene is *rendered at all*. With
    /// the backdrop off and a patch open, the substrate is drawn into the pane target and
    /// **only the patch quads sample it** — one render, no second `World`, no `Shared`
    /// override, and the terminal behind it stays flat black.
    ///
    /// Console Spike, the portal: this is now one half of [`engine_plan`]'s answer rather than
    /// the whole decision. The other half is *which viewport presentation* renders, and they are
    /// computed together precisely so that "at most one World render per frame" is a property
    /// of one function instead of an agreement between three.
    fn render_source(&self) -> BackdropSource {
        self.engine_plan().0
    }

    /// [`engine_plan`] asked with this console's own state — the one site that reads the four
    /// inputs, so no caller can assemble a different set of them.
    fn engine_plan(&self) -> (BackdropSource, Option<ViewportTarget>) {
        engine_plan(
            self.portal_state.is_open(),
            self.region_showing_world().is_some(),
            self.backdrop_source,
            self.patches_want_image(),
        )
    }

    /// The region holding `3d`, if any — **at most one**, which is `region.rs`'s uniqueness rule
    /// rather than a fact about this lookup ([`Content::only_one_because`] is where that limit
    /// is decided and attributed).
    ///
    /// ⚠️ It answers about the **layout**, not about the frame: a region can hold `3d` while the
    /// portal has the World, and that is exactly the state whose notice has to name the portal.
    fn region_showing_world(&self) -> Option<organon_console::region::Region> {
        self.layout.region_holding(organon_console::region::Content::ThreeD)
    }

    fn render_backdrop(&mut self) -> Option<egui::TextureId> {
        // Nothing is re-sized on a frame that renders nothing; the flag is read by
        // [`Console::apply_epoch_plans`] whether or not this function reached its own body.
        self.pane_resized = false;
        let source = self.render_source();
        if source == BackdropSource::Off {
            // ⚠️ **`off` clears the rig too, and it has to be said here** because this is the
            // one arm that returns before the source→rig block below. Tier 1 could leave it
            // out: the source was read once at startup, so nothing ever *became* `off`. With
            // `organon console background off` it can, and a rig left set is a camera framing
            // a plane nobody is drawing — invisible until some later path draws with it.
            // Total over the source means no exceptions, including the early ones.
            self.world.set_substrate_rig(None);
            return None;
        }
        let swapchain = {
            let gpu = self.gpu.as_ref()?;
            (gpu.config.width.max(1), gpu.config.height.max(1))
        };
        // ⚠️ **The pane, not the window.** `term_view` paints this texture at UV 0..1 into the
        // CentralPanel's rect, which egui has already shrunk by the 30-point tab strip declared
        // ahead of it — so a swapchain-sized texture is stretched vertically by exactly that
        // strip. Brief R1 and R4 found the same defect from two directions; it is invisible on
        // a generative world and glaring on a flat plane, which is why it is fixed here in the
        // same tier that puts a plane behind the glyphs. This changes `BACKDROP=1`'s rendering
        // too, on purpose.
        //
        // One frame behind by construction — the world is drawn before the interface that
        // reserves its rect runs — and clamped rather than trusted, both exactly as
        // `wgpu_editor::render_scene_pane` does it. `pane_pixels_in` carries the clamps and
        // their reason: egui hands back a zero or negative rect for a frame mid-resize, and a
        // zero-sized texture is a validation error rather than a blank pane. Frame one has no
        // rect yet and falls back to the swapchain, i.e. to today's behaviour for one frame.
        //
        // ⚠️ **The pane's share of the window applied to the swapchain — never points times a
        // remembered scale.** Sizing it `points × scale` was the Tier 4 band blur: the scale is
        // a frame output, the value standing in for "not measured yet" multiplies like a real
        // 100 % display, and the backdrop comes out in points. The live texture rebinds itself
        // one frame later; the epoch snapshot copied from it never does. `pane_pixels_in` owns
        // the argument, the measurement and the regression test.
        let (w, h) = match (self.pane_points, self.window_points) {
            (Some(pane), Some(window)) => scene_input::pane_pixels_in(swapchain, pane, window),
            _ => swapchain,
        };

        // Total over the source rather than only the substrate arm, so a runtime switch
        // (Tier 2's `organon console background`) needs no new wiring: the World arm actively
        // CLEARS the rig instead of leaving the camera framing a plane that is no longer
        // being drawn. Under `=1` this writes `None` over `None` every frame. Tier 2 closed
        // the one gap this could not reach from here — see the `Off` arm at the top, which
        // returns before this block and so has to clear the rig itself.
        if source == BackdropSource::Substrate {
            // Re-framed every frame, which is how resize is handled without a staleness flag
            // to get wrong: the rig is computed for ONE aspect and the engine reads its own
            // from the render target, so a stale rig is a plane that no longer covers the
            // pane — Leaf A's re-frame warning. It costs six floats of trigonometry.
            let aspect = w as f32 / h.max(1) as f32;
            let rig = SubstrateRig::frame_plane(SUBSTRATE_EXTENT, SUBSTRATE_FOV_DEG, aspect);
            self.world.set_substrate_rig(Some(rig.camera_arm()));
        } else {
            self.world.set_substrate_rig(None);
        }

        let rebind = self.backdrop.as_ref().is_none_or(|b| b.size != (w, h));
        // The same condition, under the name `EpochLedger::plan` knows it by. A pane that
        // changed size stales every cached epoch picture — see `snapshot_live_backdrop`.
        self.pane_resized = rebind && self.backdrop.is_some();
        if rebind {
            let device = self.world.device()?;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shell-backdrop"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: BACKDROP_FORMAT,
                // `COPY_SRC` is Tier 4's one addition: when a look closes, this texture is
                // copied into that epoch's own picture (`snapshot_live_backdrop`). It costs
                // nothing when no look ever changes.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[BACKDROP_SAMPLE_FORMAT],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("shell-backdrop-sample"),
                format: Some(BACKDROP_SAMPLE_FORMAT),
                ..Default::default()
            });
            let id = self.backdrop.as_ref().and_then(|b| b.id);
            self.backdrop = Some(Backdrop { texture, view, size: (w, h), id });
        }

        let pane = self.backdrop.as_ref()?;
        self.world.render_to_texture(&pane.texture, pane.size, BACKDROP_FORMAT);

        let device = self.world.device()?;
        let renderer = self.renderer.as_mut()?;
        let pane = self.backdrop.as_mut()?;
        match pane.id {
            Some(id) => {
                if rebind {
                    renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &pane.view,
                        wgpu::FilterMode::Linear,
                        id,
                    );
                }
                Some(id)
            }
            None => {
                let id =
                    renderer.register_native_texture(device, &pane.view, wgpu::FilterMode::Linear);
                pane.id = Some(id);
                Some(id)
            }
        }
    }

    /// Draw the conversation view's rendered surfaces, and hand back what to paint them with.
    ///
    /// # The seam this reuses
    ///
    /// [`Console::render_source`] already separates *what the engine draws* from *what the
    /// backdrop paints*, which is what lets a Tier 5 patch show a substrate while the terminal
    /// behind it stays flat black. A surface is the same idea one step further: the engine
    /// draws into a target **the conversation owns**, and the window behind it is not involved
    /// at all. Nothing here touches `backdrop_source`, so James's rule holds unchanged — the
    /// console still opens looking exactly like an ordinary terminal, and the material arrives
    /// only where something asked for it.
    ///
    /// # One World, and what that costs
    ///
    /// There is no second [`World`]. A second one would recompile ~50 shaders and ~62
    /// pipelines and duplicate every sim buffer, to draw the same plane. So the one World is
    /// rendered into a different target, with the look published through the same `Shared`
    /// channel the backdrop uses — see [`SURFACE_RENDERS_PER_FRAME`] for exactly what that
    /// double render does and does not disturb.
    ///
    /// # Order inside, and why
    ///
    /// Recency stamps, then eviction, then allocation, then at most
    /// [`SURFACE_RENDERS_PER_FRAME`] renders. Evicting *before* allocating is
    /// `substrate_epochs`' rule and it is here for the same reason: it keeps the transient
    /// peak at the cap rather than at cap-plus-this-frame's-new-ones.
    fn render_surfaces(&mut self, published: &ipc::Shared) -> SurfaceImages {
        self.surface_clock = self.surface_clock.wrapping_add(1);
        let now = self.surface_clock;
        let pane = self.surface_pane;
        let mut images = SurfaceImages::new();

        let mut requests = self.surface_requests.clone();
        if requests.len() > MAX_SURFACE_TEXTURES {
            let dropped = requests.split_off(MAX_SURFACE_TEXTURES);
            // Truncated rather than allowed to allocate and be evicted the same frame, which
            // would be a texture created and freed every frame for as long as the window
            // stayed that way. Said out loud, because a surface that draws "rendering…"
            // forever is otherwise indistinguishable from one that is broken.
            eprintln!(
                "[surface] {} visible surfaces exceeds the cap of {MAX_SURFACE_TEXTURES} — \
                 {} left unrendered (scroll one out of view to free a slot)",
                requests.len() + dropped.len(),
                dropped.len()
            );
        }

        let Some(device) = self.world.device().cloned() else { return images };
        let Some(gpu) = self.gpu.as_ref() else { return images };
        let swapchain = (gpu.config.width.max(1), gpu.config.height.max(1));
        // No frame has been laid out yet, so there is no window to take a fraction of. One
        // frame of "rendering…", which is the same one frame the first request costs anyway.
        let Some(window_points) = self.window_points else { return images };

        // 1. Everything asked for this frame is current, whether or not it gets redrawn.
        let wanted: Vec<SurfaceKey> = requests.iter().map(|r| (pane, r.element)).collect();
        for key in &wanted {
            if let Some(held) = self.surfaces.get_mut(key) {
                held.touched = now;
            }
        }

        // 2. Make room *before* allocating, so the peak is the cap and not more.
        let fresh = wanted.iter().filter(|k| !self.surfaces.contains_key(k)).count();
        let room = MAX_SURFACE_TEXTURES.saturating_sub(fresh);
        let held: Vec<(SurfaceKey, u64)> =
            self.surfaces.iter().map(|(k, t)| (*k, t.touched)).collect();
        for key in surfaces_to_evict(&held, &wanted, room) {
            self.free_surface(key, "the cap");
        }

        // 3. Allocate or resize, then draw at most the budget.
        let mut budget = SURFACE_RENDERS_PER_FRAME;
        for request in &requests {
            let key = (pane, request.element);
            let size = scene_input::pane_pixels_in(swapchain, request.size_points, window_points);
            if self.surfaces.get(&key).is_none_or(|t| t.size != size) {
                self.free_surface(key, "the surface changed size");
                let Some(made) = self.make_surface_texture(&device, size, now) else { continue };
                self.surfaces.insert(key, made);
            }
            // The look this surface should be showing. `canonical` rather than the raw
            // string, for `console_step`'s reason: a name this build does not have leaves the
            // material unset — Tier 1's undressed substrate — instead of failing.
            let desired = SurfaceLook {
                look: ConsoleLook {
                    material: canonical(&substrate_materials::MATERIAL_NAMES, &request.look)
                        .map(str::to_string),
                    // Not the console's rig, deliberately: a surface is meant to be
                    // answerable to the controls *beside it*, and inheriting a value typed
                    // into another tab is the very coupling this element exists to remove.
                    rig: None,
                },
                sliders: request.sliders.clone(),
            };
            let (id, size_px, stale) = {
                let Some(held) = self.surfaces.get_mut(&key) else { continue };
                held.touched = now;
                (held.id, held.size, held.holds.as_ref() != Some(&desired))
            };
            images.insert(request.element, id);
            if !stale || budget == 0 {
                continue;
            }
            budget -= 1;

            // The World has exactly one way to learn what to draw: the snapshot. So the
            // surface's look is published, the frame is taken, and the console's own snapshot
            // goes back afterwards — see the restore below.
            if let Some(writer) = self.shared_writer.as_mut() {
                writer.write(*surface_shared(&desired));
            }
            // Re-framed per surface, for `render_backdrop`'s reason: the rig is computed for
            // ONE aspect and the engine reads its own from the target, so a rig left set for
            // the pane would frame a plane that does not cover this rectangle.
            let aspect = size_px.0 as f32 / size_px.1.max(1) as f32;
            let rig = SubstrateRig::frame_plane(SUBSTRATE_EXTENT, SUBSTRATE_FOV_DEG, aspect);
            self.world.set_substrate_rig(Some(rig.camera_arm()));
            self.world.render_to_texture(&self.surfaces[&key].texture, size_px, BACKDROP_FORMAT);
            if let Some(held) = self.surfaces.get_mut(&key) {
                held.holds = Some(desired);
            }
        }

        // The snapshot at rest is the console's own. Without this the `organon` CLI's
        // `status`/`get` would report whichever surface happened to render last — a lane
        // nobody typed into, describing a picture that is not the window.
        if budget < SURFACE_RENDERS_PER_FRAME {
            if let Some(writer) = self.shared_writer.as_mut() {
                writer.write(*published);
            }
        }
        images
    }

    /// One surface's render target, created and registered with egui. `None` when the
    /// renderer is not up yet, which is the same "one more frame" every other path here has.
    fn make_surface_texture(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
        now: u64,
    ) -> Option<SurfaceTexture> {
        // The [`Backdrop`] pair exactly — `Rgba8UnormSrgb` storage with an `Rgba8Unorm`
        // sample view — so a surface linearizes once, like everything else painted from the
        // engine. No `COPY_SRC`: nothing ever snapshots a surface, because unlike a look
        // epoch a surface has no history to keep.
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shell-conversation-surface"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: BACKDROP_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[BACKDROP_SAMPLE_FORMAT],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shell-conversation-surface-sample"),
            format: Some(BACKDROP_SAMPLE_FORMAT),
            ..Default::default()
        });
        let renderer = self.renderer.as_mut()?;
        let id = renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);
        Some(SurfaceTexture { texture, view, id, size, holds: None, touched: now })
    }

    /// Take everything the loader threads have finished, then start work on anything this
    /// frame asked for and nothing is holding — and answer with what the view may draw.
    ///
    /// # Why this is a drain and not a call
    ///
    /// 🚨 **Never block the frame.** Opening a file and decoding a JPEG are both unbounded in
    /// the only sense that matters here — they depend on a disk and on somebody else's bytes —
    /// so doing either between `begin_pass` and `end_pass` freezes the window for as long as it
    /// takes. Everything else in this console is synchronous and in-process, so this is the
    /// first place that rule has needed enforcing, and the shape is the cheapest one that
    /// enforces it: one thread per item, a channel back, and a frame that only ever *collects*.
    ///
    /// The visible consequence is that a picture arrives one or more frames after it is asked
    /// for, which is the same deferral a conversation surface already has and shows the same
    /// way — `reading...` until it is there.
    fn service_exhibits(&mut self, requests: &[ExhibitRequest]) -> ExhibitContents {
        // 1. Collect. Every message is a job that has finished, so the in-flight set loses it
        //    whether it succeeded or failed — a failure that stayed in-flight would never be
        //    retried and never be drawn.
        while let Ok((key, load)) = self.exhibit_rx.try_recv() {
            self.exhibit_inflight.remove(&key);
            let content = match load {
                ExhibitLoad::Document(text) => ExhibitContent::Document(text),
                ExhibitLoad::Failed(why) => ExhibitContent::Failed(why),
                ExhibitLoad::Picture { size, rgba } => match self.upload_exhibit(size, &rgba) {
                    Some(texture) => ExhibitContent::Picture { texture, size },
                    // The decode worked and the upload did not, which is a different sentence
                    // from a bad file and has to read like one or it sends someone to check
                    // their PNG.
                    None => ExhibitContent::Failed("the GPU would not take this picture".into()),
                },
            };
            self.exhibits.insert(key, content);
        }

        // 2. Evict, on the surfaces' own policy and before anything new is started, so the peak
        //    is the cap rather than the cap plus this frame's arrivals.
        self.exhibit_clock = self.exhibit_clock.wrapping_add(1);
        let now = self.exhibit_clock;
        let wanted: Vec<ExhibitKey> = requests.iter().map(|r| (r.element, r.item)).collect();
        for key in &wanted {
            if let Some(stamp) = self.exhibit_touched.get_mut(key) {
                *stamp = now;
            }
        }
        // Pictures are capped by **texture count**; documents get their own **byte** budget
        // below. Two ledgers on one policy, because the thing each is scarce in differs: a
        // texture is GPU memory in fixed-size slabs, so counting slabs is the right instrument,
        // while documents vary by four orders of magnitude and only their total is meaningful.
        // Putting a document in this list would evict a picture to make room for something that
        // never needed the room.
        let held: Vec<(ExhibitKey, u64)> = self
            .exhibits
            .iter()
            .filter(|(_, c)| matches!(c, ExhibitContent::Picture { .. }))
            .map(|(k, _)| (*k, self.exhibit_touched.get(k).copied().unwrap_or(0)))
            .collect();
        for key in surfaces_to_evict(&held, &wanted, MAX_EXHIBIT_TEXTURES) {
            self.free_exhibit(key, "the cap");
        }
        // Documents, on the **same policy** and a different ledger: least-recently-requested
        // first, until what is held is inside [`MAX_DOCUMENT_BYTES_HELD`]. `surfaces_to_evict`
        // counts entries rather than weighing them, so the cap it is given is derived here —
        // drop the oldest until the total fits, which is the byte-wise reading of the same
        // "oldest goes first" rule and needs no second sort.
        let docs: Vec<(ExhibitKey, u64, usize)> = self
            .exhibits
            .iter()
            .filter_map(|(k, c)| match c {
                ExhibitContent::Document(text) => {
                    Some((*k, self.exhibit_touched.get(k).copied().unwrap_or(0), text.len()))
                }
                _ => None,
            })
            .collect();
        for key in documents_to_evict(&docs, &wanted, MAX_DOCUMENT_BYTES_HELD) {
            self.free_exhibit(key, "the document budget");
        }

        // 3. Start what is missing. `contains_key` covers `Failed` too, which is what stops a
        //    broken file being re-read on every frame for the rest of the session.
        for request in requests {
            let key = (request.element, request.item);
            if self.exhibits.contains_key(&key) || self.exhibit_inflight.contains(&key) {
                continue;
            }
            self.exhibit_touched.insert(key, now);
            self.exhibit_inflight.insert(key);
            let tx = self.exhibit_tx.clone();
            let path = request.path.clone();
            // Detached on purpose: the job owns everything it touches, and a console shutting
            // down while a read is in flight should not wait on a disk. The channel's receiver
            // living on `Console` is what makes a send after teardown a no-op rather than a
            // panic.
            std::thread::spawn(move || {
                let _ = tx.send((key, load_exhibit_item(&path)));
            });
        }

        self.exhibits.clone()
    }

    /// Put a decoded picture on the GPU, or `None` if there is no device yet.
    ///
    /// The [`make_surface_texture`](Self::make_surface_texture) pair exactly —
    /// `Rgba8UnormSrgb` storage with an `Rgba8Unorm` sample view — so a photograph and the
    /// engine's own render are linearized the same number of times. Getting that wrong is not a
    /// crash; it is a picture that looks washed out beside a surface, which is the kind of
    /// thing only a hand can see.
    fn upload_exhibit(&mut self, size: (u32, u32), rgba: &[u8]) -> Option<egui::TextureId> {
        let device = self.world.device().cloned()?;
        let queue = self.world.queue().cloned()?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shell-exhibit"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: BACKDROP_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[BACKDROP_SAMPLE_FORMAT],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("shell-exhibit-sample"),
            format: Some(BACKDROP_SAMPLE_FORMAT),
            ..Default::default()
        });
        let renderer = self.renderer.as_mut()?;
        let id = renderer.register_native_texture(&device, &view, wgpu::FilterMode::Linear);
        self.exhibit_textures.insert(id, texture);
        Some(id)
    }

    /// Drop one exhibit item's texture, saying why.
    ///
    /// Unconditional logging, on [`Console::free_surface`]'s rule and for its reason: *"a
    /// silently dropped texture reads as 'the picture is still there'"*. `[exhibit]` is the tag
    /// to grep for.
    ///
    /// ⚠️ **The entry goes, not just the texture.** Leaving a `Picture` behind pointing at a
    /// freed registration is a dangling `TextureId`; removing the entry is what makes the next
    /// frame ask for it again, which is exactly what "a reference, never bytes" buys — the file
    /// is still on disk, so an eviction costs a re-read and never costs the picture.
    fn free_exhibit(&mut self, key: ExhibitKey, why: &str) {
        let Some(gone) = self.exhibits.remove(&key) else { return };
        self.exhibit_touched.remove(&key);
        // A document has no texture and still says it went, on the same rule: the next frame
        // re-reads the file, and a re-read nobody was told about is how a document that quietly
        // reloads on every scroll looks like a console that is merely slow.
        if let ExhibitContent::Document(text) = &gone {
            eprintln!(
                "[exhibit] released the {}-byte document for element {} item {} - {why}",
                text.len(),
                key.0 .0,
                key.1
            );
            return;
        }
        let ExhibitContent::Picture { texture, size } = gone else { return };
        self.exhibit_textures.remove(&texture);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.free_texture(&texture);
        }
        eprintln!(
            "[exhibit] released the {}x{} texture for element {} item {} - {why}; \
             {} of {MAX_EXHIBIT_TEXTURES} live",
            size.0,
            size.1,
            key.0 .0,
            key.1,
            self.exhibits.values().filter(|c| matches!(c, ExhibitContent::Picture { .. })).count()
        );
    }

    /// Drop one surface's texture and its egui registration, saying why.
    ///
    /// Unconditional logging, on [`Console::retire_epochs`]' rule: a cap that silently drops a
    /// picture is indistinguishable from a renderer that failed to draw one. `[surface]` is
    /// the tag to grep for.
    fn free_surface(&mut self, key: SurfaceKey, why: &str) {
        let Some(gone) = self.surfaces.remove(&key) else { return };
        eprintln!(
            "[surface] released the {}×{} texture for element {} (pane {}) — {why}; \
             {} of {MAX_SURFACE_TEXTURES} live, budget {} bytes",
            gone.size.0,
            gone.size.1,
            key.1 .0,
            key.0,
            self.surfaces.len(),
            surface_budget_bytes(gone.size.0, gone.size.1),
        );
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.free_texture(&gone.id);
        }
    }

    /// The viewport's frame: render the **World** into the one viewport target and hand back
    /// what to paint it with. `None` when no presentation is live, or for the one frame before
    /// the live one's rect is known.
    ///
    /// # 🚨 One render, whichever presentation asked for it
    ///
    /// [`engine_plan`] has already decided *which* — the portal if it is open, otherwise a
    /// region holding `3d` — and this function does not need to know which it was: the answer
    /// is entirely carried by [`Console::viewport_points`], which the previous frame's winner
    /// wrote. That is what makes this one mechanism rather than two that happen to agree.
    /// Switching presentation is a size change, handled by the same free-and-reallocate path a
    /// window resize already takes, with the same unconditional log line.
    ///
    /// # 🚨 Why this is the World and not the substrate
    ///
    /// A substrate portal cannot be orbited, and it fails *silently*. `World`'s camera
    /// finalization reads `substrate_rig` first and returns its whole six-tuple before
    /// `yaw`/`pitch`/`distance` are consulted — and those three are exactly what
    /// [`World::apply_camera_input`] writes. So the drag would be read, converted, applied, and
    /// then discarded, with a green build and no log line. [`portal`]'s module docs carry the
    /// argument in full; this is the site that depends on it.
    ///
    /// ⚠️ **`set_substrate_rig(None)` here matters, and is not defensive tidiness.**
    /// [`Console::render_surfaces`] installs a rig per surface and never clears it, and it runs
    /// *before* this. A conversation tab that drew one surface would otherwise leave the
    /// viewport's World framing a plane nobody is drawing — the same stale-rig hazard
    /// [`Console::render_backdrop`]'s `Off` arm was given its own clear for.
    ///
    /// # What it does NOT have to do
    ///
    /// No `Shared` publish-and-restore. A surface has to overwrite the snapshot with its own
    /// look and put the console's back, or `organon status` would report a picture that is not
    /// the window. The portal shows the console's **own** snapshot — already published before
    /// any of this runs — so there is nothing to override and nothing to restore. And because
    /// `render_to_texture` runs `frame_body`, the CLI's parameter lane drains inside this call:
    /// `organon set` / `generator` / `recipe` typed at a prompt in this console reach this
    /// world, with no wiring at all.
    fn render_viewport(&mut self) -> Option<egui::TextureId> {
        // 🚨 **The one gate, and it is [`engine_plan`]'s answer rather than a second reading of
        // the state.** Asking `portal_state.is_open() || region_showing_world().is_some()` here
        // would be a copy of the precedence rule, and a copy is how a console comes to render a
        // frame nothing paints — or to paint one nothing rendered.
        //
        // 🚨 **And it is the single release site.** Nothing live means the texture goes now
        // rather than being held against a re-open: a viewport is one thing that is live or
        // not, not a cache, and 2.5 MB held for a rectangle nobody asked for is the kind of cost
        // that is invisible until somebody profiles. Releasing *here* rather than at each verb
        // is what makes it total — the portal closing, the region being cleared, the region
        // being displaced and `viewport full agent` are four routes to the same state, and this
        // asks about the state. The gesture goes with it, or a latch stranded mid-drag would
        // have the next viewport claiming the wheel with no drag behind it.
        let Some(_live) = self.engine_plan().1 else {
            self.free_viewport(
                "nothing is showing the world — the portal is closed and no region holds `3d`",
            );
            self.viewport_input = scene_input::SceneInput::default();
            return None;
        };
        let device = self.world.device().cloned()?;
        let gpu = self.gpu.as_ref()?;
        let swapchain = (gpu.config.width.max(1), gpu.config.height.max(1));
        let window_points = self.window_points?;
        // The live viewport's rect from the previous frame, as a fraction of the window applied
        // to the swapchain — `pane_pixels_in`'s ratio, never points times a remembered scale.
        // That argument is `render_backdrop`'s and the measurement is `pane_pixels_in`'s.
        let size = scene_input::pane_pixels_in(swapchain, self.viewport_points?, window_points);
        if self.viewport.as_ref().is_none_or(|t| t.size != size) {
            self.free_viewport("the viewport changed size");
            self.viewport = self.make_surface_texture(&device, size, self.surface_clock);
        }
        let held = self.viewport.as_ref()?;
        let (id, texture_size) = (held.id, held.size);
        // The World, not a rig — see this function's doc, and the module docs it points at.
        self.world.set_substrate_rig(None);
        let texture = &self.viewport.as_ref()?.texture;
        self.world.render_to_texture(texture, texture_size, BACKDROP_FORMAT);
        Some(id)
    }

    /// Drop the viewport's texture and its egui registration, saying why —
    /// [`Console::free_surface`]'s body and its unconditional log, with the one clause that
    /// identifies it changed.
    ///
    /// ⚠️ It prints `the viewport` rather than an element and a pane, which is the concrete half
    /// of why this is not a `SurfaceKey` variant: there is no element and, in a terminal tab,
    /// no element *space* — a key here would have had to fabricate both to satisfy the log.
    /// **Which presentation held it rides in `why`**, which is that argument's job: the caller
    /// knows and this function has no reason to.
    fn free_viewport(&mut self, why: &str) {
        let Some(gone) = self.viewport.take() else { return };
        eprintln!(
            "[surface] released the {}×{} texture for the viewport — {why}; \
             {} of {MAX_SURFACE_TEXTURES} conversation surfaces live, viewport {} bytes",
            gone.size.0,
            gone.size.1,
            self.surfaces.len(),
            u64::from(gone.size.0) * u64::from(gone.size.1) * 4,
        );
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.free_texture(&gone.id);
        }
    }

    /// Free every surface texture. Used where a `SurfaceKey`'s pane index stops meaning what
    /// it meant — see the `Close` arm of [`Console::apply`].
    ///
    /// ⚠️ **The portal is deliberately not among them.** This fires when a tab closes and every
    /// `SurfaceKey`'s pane index stops meaning what it meant — a renumbering the portal has no
    /// stake in, since it is keyed by nothing. Blanking it on every tab close would be a
    /// visible flicker in the one thing on screen that is meant to hold still.
    fn free_all_surfaces(&mut self, why: &str) {
        for key in self.surfaces.keys().copied().collect::<Vec<_>>() {
            self.free_surface(key, why);
        }
        self.surface_requests.clear();
    }

    fn redraw(&mut self) {
        let frame = {
            let (Some(window), Some(gpu)) = (self.window.as_ref(), self.gpu.as_mut()) else {
                return;
            };
            let Some(device) = self.world.device() else { return };
            match gpu.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Occluded => {
                    self.occluded = true;
                    return;
                }
                _ => {
                    gpu.surface.configure(device, &gpu.config);
                    window.request_redraw();
                    return;
                }
            }
        };

        // #4 Tier 2: the console command lane, drained immediately BEFORE the publication
        // below rather than after it — a `background` typed this frame reaches the World this
        // frame, not next. See [`Console::drain_console`].
        self.drain_console();

        // The plugin's job, done by the terminal: publish the snapshot the world
        // and the CLI read. The same bytes every frame until a console command
        // rewrites them — the CLI's *param* lane (`organon set`/`generator`/…)
        // mutates the world's working copy, not this base, per #317's rule, while
        // the console lane rewrites the base itself because a backdrop is not a
        // param override.
        // When a patch is being rendered against an `off` backdrop, the snapshot has to
        // describe the substrate the patch will show — the World draws what this says, and it
        // has no other way to learn that anything wants a picture this frame.
        let mut published = if self.render_source() == self.backdrop_source {
            *self.shared
        } else {
            *look_shared(self.render_source(), &self.console_look)
        };
        // …and then whatever an Organon editor panel in a conversation has asked for, on top
        // (Console #7). **Here rather than inside `look_shared`** for two reasons: that
        // function is recomputed-from-scratch by design and is shared with the per-surface
        // path, which is deliberately answerable only to the controls beside it; and a panel's
        // opinion is a *later* statement than the backdrop's dressing, so it composes on top
        // rather than being part of the look. Inert until a control is moved — see
        // [`OrganonPanels::overlay`].
        //
        // ⚠️ **One frame behind, and that is the same arrangement everything else here uses.**
        // The conversation is drawn further down this function, so a drag this frame lands in
        // next frame's snapshot — exactly as `surface_requests` and `pane_points` are recorded
        // now and consumed next time. At redraw rates it is not perceptible, and the
        // alternative is publishing twice per frame.
        self.organon_panels.overlay(&mut published);
        let published = published;
        if let Some(w) = self.shared_writer.as_mut() {
            w.write(published);
        }

        // The engine first, the terminal over it — the backdrop texture this frame
        // paints under the glyphs is the one just rendered.
        let backdrop = self.render_backdrop();
        // …and the epochs behind it: which cached pictures should still exist, now that the
        // pane's size for this frame is known.
        self.apply_epoch_plans();
        // …and then the conversation view's own surfaces, from the rects it laid out last
        // frame. After the backdrop, so that the substrate rig this leaves set is re-framed
        // by the next frame's `render_backdrop` rather than the other way round, and so that
        // a console with the backdrop on still gets the picture it published.
        let surface_images = self.render_surfaces(&published);
        // …and the exhibits, which are not rendered at all: this collects what the loader
        // threads finished, starts anything new, and hands the view what it may draw. Placed
        // beside `render_surfaces` because it answers the same question for the other kind of
        // picture, and one frame behind for the same reason.
        let asked = std::mem::take(&mut self.exhibit_requests);
        let exhibit_contents = self.service_exhibits(&asked);
        // …and the viewport last, after everything that installs a substrate rig, because it is
        // the one target that must have none. [`Console::render_viewport`] clears it explicitly
        // rather than relying on this order — the order is what makes the clear cheap, not what
        // makes it correct.
        //
        // ONE render for whichever presentation [`engine_plan`] gave the frame to; the two call
        // sites below paint it, and at most one of them is reached.
        let viewport_image = self.render_viewport();
        // Which presentation that was, so the paint sites can tell "I have the frame" from "the
        // other one does". Read after the render, from the same function the render asked, so
        // the picture and the notice cannot disagree about who owns the world this frame.
        let portal_has_the_frame = self.engine_plan().1 == Some(ViewportTarget::Portal);

        let (Some(window), Some(gpu), Some(state), Some(renderer)) = (
            self.window.as_ref(),
            self.gpu.as_mut(),
            self.egui_state.as_mut(),
            self.renderer.as_mut(),
        ) else {
            return;
        };
        let Some(device) = self.world.device().cloned() else { return };
        let Some(queue) = self.world.queue().cloned() else { return };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let raw = state.take_egui_input(window);
        // Every tab pumps every frame — a background agent keeps streaming into
        // its grid; only the active one draws.
        //
        // Through the pane's anchor, never bare: `PaneAnchor::pump` is the ONE site that
        // advances the scroll-anchor's `dropped` counter, and every tab needs its own kept
        // current whether or not it is the tab being banded (its rows are accumulating under
        // the live look right now, and it will be switched to later).
        //
        // A conversation pane pumps too, for the same reason and by the same rule: an
        // agent keeps answering while you are looking at another tab. Its drain is the
        // event channel rather than the PTY, and it touches no anchor because it has no
        // scrollback to anchor into.
        for (pane, looks) in self.sessions.iter_mut().zip(self.pane_looks.iter_mut()) {
            match pane {
                Pane::Term(session) => looks.anchor.pump(session),
                Pane::Conversation(chat) => {
                    chat.pump();
                }
            }
        }
        let active = self.strip.active;
        // The active pane's look history, resolved to textures before the closure borrows
        // `pane_looks` mutably for its anchor. `None` when there is no backdrop at all, which
        // is the pre-Tier-4 path exactly.
        //
        // ⚠️ Gated on the **backdrop** source, not the render source: a scene rendered purely
        // so a patch has something to show must not also paint itself across the window. That
        // separation is the whole of James's "it must open like an ordinary terminal" rule —
        // see [`Console::render_source`].
        let bands = (self.backdrop_source != BackdropSource::Off).then_some(()).and_then(|()| {
            backdrop.and_then(|live| {
                self.pane_looks.get(active).map(|pane| {
                    Self::band_table(&pane.ledger, Some(live), |id| {
                        pane.cache.get(&id).map(|c| c.id)
                    })
                })
            })
        });
        // The picture a patch samples, available whether or not the backdrop paints.
        let patch_image = backdrop;
        let strip = &self.strip;
        let registry = &self.registry;
        let installed = &self.installed;
        let plus_open = &mut self.plus_open;
        let default_harness = self.default_harness.as_str();
        let sessions = &mut self.sessions;
        let pane_looks = &mut self.pane_looks;
        let mut action: Option<TabAction> = None;
        // The full-screen chord, collected out of the closure exactly as `action` is and for
        // the same reason: `Console::set_screen` needs `&mut self`, and `self` is split into
        // disjoint borrows for the duration of `egui_ctx.run`.
        let mut screen_cmd: Option<organon_console::screen::ScreenCmd> = None;
        // Buttons pressed inside a patch's panel this frame, collected out of the closure the
        // same way `action` is and for the same reason: applying one needs `&mut self`, and
        // `self` is split into disjoint borrows for the duration of `egui_ctx.run`.
        let mut block_actions: Vec<BlockAction> = Vec::new();
        // ⚠️ **There was a second vector here, for buttons pressed inside an inline artifact
        // in a conversation, and it is gone.** Only a panel wired to the *console* could
        // fill it, which is what `/panel` summoned; a conversation has no scrollback for a
        // backdrop to band across, so the effect landed on a terminal tab and the panel read
        // as a set of dead knobs. A panel now always drives an element in its own transcript
        // and the press never leaves that crate.
        //
        // …what does come back is where its rendered surfaces ended up, which is the NEXT frame's render list.
        // Collected out of the closure for the same reason: acting on it needs `&mut self`.
        let mut surface_requests: Vec<SurfaceRequest> = Vec::new();
        let mut exhibit_requests: Vec<ExhibitRequest> = Vec::new();
        // The rect the terminal actually paints into, captured for the NEXT frame's backdrop
        // (see `render_backdrop`). Taken from the same `ui` and by the same call
        // `term_view::draw` sizes its grid from, so the texture and the quad cannot disagree.
        let mut pane_rect: Option<egui::Rect> = None;
        // …and the whole window beside it, in the SAME points, so the two divide into the
        // ratio `render_backdrop` applies to the swapchain. Read from the same frame as
        // `pane_rect` for exactly that reason: a ratio only cancels the scale if both halves
        // were measured under it.
        let mut window_rect: Option<egui::Rect> = None;
        // The viewport, split out of `self` for the closure exactly as everything else here is.
        // The portal's state is `Copy`, the **one** gesture accumulator is borrowed, and the
        // live rect comes back out to be remembered for the next frame's `render_viewport`.
        let portal_open = self.portal_state.is_open();
        let viewport_input = &mut self.viewport_input;
        // Organon's editor panels, split out of `self` exactly as everything else here is —
        // mutably, because the whole point is that a control inside the conversation writes
        // to it. Its snapshot is read at the *top* of the next frame, so nothing has to come
        // back out of the closure.
        let organon_panels = &mut self.organon_panels;
        // The palette, split out of `self` exactly as everything else here is — one shared
        // borrow that every draw call inside the closure passes down.
        let theme = &self.theme;
        let theme_name = self.theme_name;
        // A palette the live editor changed this frame. Collected out of the closure for the
        // same reason `surface_requests` is: acting on it needs `&mut self`, and `theme` is
        // borrowed from `self` for the whole of the closure.
        let mut theme_change: Option<organon_console::theme_edit::ThemeChange> = None;
        // The form tokens at this frame's posture, resolved **once** and borrowed exactly as
        // the palette is. Resolving per draw call would be cheap and wrong for a reason that
        // outlives this tier: two calls in one frame could then disagree, which is precisely
        // the tearing a tween would make visible.
        let form = &self.posture.form();
        let mut portal_rect: Option<egui::Rect> = None;
        // Where the `3d` region was drawn this frame, if it had the world — `draw_regions`'
        // answer, collected out of the closure exactly as `portal_rect` is and remembered for
        // the next frame's `render_viewport`.
        let mut region_viewport_rect: Option<egui::Rect> = None;
        // How the pane is divided this frame, borrowed exactly as the palette is. Read only —
        // the layout is moved by `set_viewport`, which has already run in `drain_console` at the
        // top of this function, so the division a frame draws is the one every command issued
        // before it asked for.
        let layout = self.layout;
        // Which region holds `3d`, read off the copy above rather than through
        // [`Console::region_showing_world`] — `self` is split into disjoint field borrows for
        // the duration of `egui_ctx.run`, so a method taking `&self` cannot be called here. It
        // is the same lookup on the same value: `Layout` is `Copy` and this is that copy.
        let three_d_region =
            layout.region_holding(organon_console::region::Content::ThreeD);
        // The console's one panel column, split out of `self` for the closure. Shared rather
        // than mutable: a `/organon` line asks for a push by leaving a value on
        // `ConversationOutput`, which is applied after the closure with `&mut self` in hand —
        // the same arrangement `theme_change` and `surface_requests` use, and for its reason.
        let panel_stacks = &self.panel_stacks;
        // 🚨 **The frame's keyboard arbitration, taken before anything draws.** `begin` promotes
        // last frame's observation to this frame's answer and starts a fresh one; every region
        // line then records its own focus as it draws. Called unconditionally — including on a
        // frame with no region lines at all — because that is what hands the keys back to the
        // composer when the last line goes away. See `region_line::Lines::begin`.
        self.region_lines.begin();
        let composer_owns_keys = self.region_lines.composer_owns_keys();
        let region_lines = &mut self.region_lines;
        let registry_for_lines = &self.line_registry;
        // What a region line's Enter asked for, collected out of the closure and applied below.
        let mut region_ran: Vec<(organon_console::region::Region, String, serde_json::Value)> =
            Vec::new();
        // 🚨 **Where a summoned panel would go**, computed from the layout before anything is
        // drawn and handed to the conversation front-end. This is what lets the refusal for
        // "no region holds a stack" be spoken *in the composer*, beside the words that are
        // still in it, rather than on a stderr nobody is reading.
        let panel_home = organon_console::panel_stack::Home::of(&layout);
        // A panel a `/organon` line asked for this frame, collected out of the closure exactly
        // as `theme_change` is and for its reason: pushing it needs `&mut self`.
        let mut panel_wanted: Option<&'static organon_core::panels::Panel> = None;
        let out = self.egui_ctx.run(raw, |ctx| {
            window_rect = Some(ctx.screen_rect());
            // ⌘-keys are the host's chrome (term_view skips them for the PTY).
            // `repeat` is forwarded rather than filtered here: which chords a held
            // key may stream is `command_key_action`'s decision, taken per action
            // and tested there. `action.is_none()` bounds this to one per frame,
            // which is a rate and not a total — autorepeat is slower than the frame
            // rate, so every repeat that arrives gets its own frame to act in.
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Key { key, pressed: true, repeat, modifiers, .. } = ev {
                        if action.is_none() {
                            action = tabs::command_key_action(
                                *key,
                                *modifiers,
                                *repeat,
                                strip,
                                default_harness,
                            );
                        }
                        // 🚨 **Read here — beside the ⌘ chords, from the raw frame events,
                        // before a single panel is laid out — and that placement is what makes
                        // it the way OUT of full screen rather than a key that works when
                        // nothing has focus.** A window with no title bar has no close button,
                        // so the escape hatch cannot be conditional on which pane is active or
                        // on whether the composer has the caret. This site sees every keystroke
                        // regardless of both, which is exactly why ⌘T works while you are
                        // typing.
                        //
                        // ⚠️ Deliberately **not** consumed out of `i.events`, unlike the state-
                        // conditional Escape ownership §2's portal row reserves. Nothing downstream
                        // wants this key: `term::encode_key` returns `None` for every function
                        // key, so the PTY receives nothing whether or not it is removed, and
                        // both conversation-side key tables answer `Ignore`. All three are
                        // pinned by tests in `organon_console::screen`, so a future mapping that
                        // did want F11 would fail there rather than fight silently here.
                        //
                        // ⚠️ **`repeat` is passed and is not decoration.** Holding a key streams
                        // `pressed: true` events, so without it a resting finger would flip the
                        // window once per repeat and the state on release would come down to
                        // parity. `screen_key` refuses them; its own doc says why this chord
                        // needs it more than the `⌘` ones above — which now take the flag too,
                        // and answer it differently: `command_key_action` streams `Switch` on
                        // repeat and refuses it only for `New`/`Close`. Two key tables, one
                        // event, opposite right answers, which is why they resolve separately.
                        if screen_cmd.is_none() {
                            screen_cmd =
                                organon_console::screen::screen_key(*key, *modifiers, *repeat);
                        }
                    }
                }
            });
            // The tab strip: the one permitted chrome (FR-T11), Superconductor's
            // form factor — along the top, + menu with the numbered registry.
            egui::TopBottomPanel::top("tab-strip")
                .exact_height(30.0)
                .frame(egui::Frame::NONE.fill(theme.tab_strip_fill))
                .show(ctx, |ui| {
                    let bar =
                        tabs::tab_bar(ui, strip, registry, installed, plus_open, theme);
                    if let Some(a) = bar {
                        action = Some(a);
                    }
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(theme.term_bg))
                .show(ctx, |ui| {
                    // Before anything is allocated in it — `term_view::draw`'s own first act
                    // is this same call.
                    pane_rect = Some(ui.available_rect_before_wrap());
                    // **This frame's pane, not last frame's** — which is what makes the portal
                    // screen-anchored in the sense that matters: the rectangle is a function of
                    // where the window is *now*. Only the pixels inside it are a frame old.
                    portal_rect = pane_rect
                        .filter(|_| portal_open)
                        .and_then(organon_console::portal::portal_rect);
                    // 🚨 **The `3d` region's rectangle, computed BEFORE anything is drawn**,
                    // because the terminal arbitrates the wheel against it and the terminal may
                    // be drawn first — the walk visits regions in `Region::ALL` order, so
                    // relying on the viewport having painted (and consumed the scroll from
                    // inside `scene_viewport`) would be relying on the layout's alphabet.
                    //
                    // 🚨 **This is §1.14's "what a split does NOT yet change" becoming real.**
                    // Regions are disjoint, so this rectangle never overlaps the one the
                    // transcript is in — but `term_view` reads the wheel from **raw input**,
                    // which is global and knows nothing about any rectangle, so without an
                    // explicit test a wheel over the viewport would zoom its camera *and*
                    // scroll the transcript beside it. A viewport region is the second wheel
                    // consumer that section said would arrive, and the mechanism is
                    // `block_panel::pointer_inside`'s and the portal's — not a third one.
                    //
                    // Computed from the layout whether or not the region has this frame's
                    // world: a rectangle that is showing the "the portal has it" notice still
                    // is not the transcript, and a wheel over it must not scroll the transcript
                    // either.
                    let region_claim = pane_rect
                        .zip(three_d_region)
                        .and_then(|(p, r)| organon_console::region::region_rect(p, r));
                    // 🚨 **And every `panel` region, which is the consumer §1.14 named in
                    // advance**: *"it becomes real the moment a region holds something
                    // scrollable"*. A stack scrolls, so a wheel over one must not also scroll
                    // a transcript nowhere near the pointer.
                    //
                    // ⚠️ **Every panel region, not just the one `/organon` names.** They all
                    // show the one stack and all of them scroll; listing only the first would
                    // leave the wheel over the second stealing the transcript's scroll.
                    //
                    // ⚠️ Walked over `Region::ALL` with `Layout::get` rather than through
                    // `occupied()`, which allocates: this is the frame path, and the sentence
                    // below about a fixed-size array with no allocation has to stay true of
                    // the whole block rather than only of the array it names.
                    let mut stack_claims = [egui::Rect::NOTHING; 4];
                    if let Some(p) = pane_rect {
                        let mut next = 0usize;
                        for region in organon_console::region::Region::ALL.iter().copied() {
                            if layout.get(region)
                                != Some(organon_console::region::Content::Panel)
                            {
                                continue;
                            }
                            // A layout is at most four disjoint regions, so this cannot
                            // overrun — the guard is a belt, and it is what makes that a
                            // statement rather than a hope.
                            let Some(slot) = stack_claims.get_mut(next) else { break };
                            if let Some(r) = organon_console::region::region_rect(p, region) {
                                *slot = r;
                            }
                            next += 1;
                        }
                    }
                    // The rectangles the transcript does not own, as a fixed-size array — see
                    // the `term_view::draw` call below on why `Rect::NOTHING` stands in for an
                    // absent claim rather than a shorter slice. ⚠️ Four slots for the stacks
                    // because a layout is at most four disjoint regions; `zip` above stops at
                    // whichever runs out, so a fifth region kind could never overrun it.
                    let viewport_claims = [
                        portal_rect.unwrap_or(egui::Rect::NOTHING),
                        region_claim.unwrap_or(egui::Rect::NOTHING),
                        stack_claims[0],
                        stack_claims[1],
                        stack_claims[2],
                        stack_claims[3],
                    ];
                    // 🚨 **The live tab, drawn into whatever rectangle it is given.** This was
                    // the body of the `CentralPanel` closure and is now a closure of its own,
                    // called **at most once per frame** — see the region walk below for why at
                    // most, and `region.rs`'s header for why that is a fact about the borrow
                    // checker rather than a policy: `conversation_view::draw` takes the pane
                    // `&mut`, so a second live copy of one tab is not something this seam
                    // *declines* to draw, it is something it cannot express.
                    //
                    // §5.9's fork, at the one place it shows: the same panel, the same
                    // window, two renderings. The terminal branch is what it was — Tier 5's
                    // patch ledger and the actions its panels return included; a conversation
                    // tab has neither because it has no transcript of terminal lines to claim
                    // a rectangle in.
                    let mut draw_active_pane = |ui: &mut egui::Ui| {
                        match (sessions.get_mut(active), pane_looks.get_mut(active)) {
                            (Some(Pane::Term(session)), Some(pane)) => {
                                // `&mut pane.anchor` and `&mut pane.blocks` are disjoint fields of
                                // the same pane, which is exactly why the patch ledger lives on
                                // `PaneLooks` beside the anchor rather than in a parallel `Vec` on
                                // `Console` that would have to be indexed in step with it. The
                                // ledger is `&mut` because a panel's sliders are real: a value
                                // dragged this frame has to still be there on the next one.
                                block_actions = term_view::draw(
                                    ui,
                                    session,
                                    &mut pane.anchor,
                                    bands.as_ref().map(|(boundaries, textures)| BandedBackdrop {
                                        boundaries,
                                        textures,
                                    }),
                                    &mut pane.blocks,
                                    patch_image,
                                    // 🚨 The wheel arbitration, and the only thing this crate
                                    // does with the rects. The terminal reads the wheel from
                                    // **raw input**, so registering a viewport after it — or as
                                    // an `Area`, or as a modal — would not keep a scroll over
                                    // one out of the scrollback. Nothing but an explicit rect
                                    // test can, which is why `block_panel::pointer_inside`
                                    // exists and why this copies it.
                                    //
                                    // **Two rectangles now, and it is a slice rather than two
                                    // parameters** — one mechanism serving both presentations,
                                    // the same consolidation `paint_viewport` is. At most one of
                                    // them is *live* in any frame, but both are rectangles the
                                    // transcript does not own, and listing only the live one
                                    // would make a wheel over the yielded region scroll text
                                    // that is nowhere near the pointer.
                                    //
                                    // An absent claim is `Rect::NOTHING`, whose `contains` is
                                    // false for every point — so this is a fixed-size array with
                                    // no allocation in the frame path, and "there is no portal"
                                    // is answered by the geometry rather than by a length.
                                    &viewport_claims,
                                    theme,
                                );
                            }
                            (Some(Pane::Conversation(chat)), _) => {
                                // 🚨 **The one arbitration point between the console's several
                                // command inputs**, told before the composer reads a key.
                                // `composer_keys` consumes Tab, Escape and the arrows out of
                                // the raw event list — two of them unconditionally on an empty
                                // box — so a region line that had focus last frame would find
                                // its own keys already gone. `true` whenever no region line has
                                // focus, which is every console that has not divided its pane.
                                chat.set_keys(composer_owns_keys);
                                // No PTY, so no patch ledger and no block actions: `block_actions`
                                // stays the empty `Vec` it was initialised to and the loop below
                                // does nothing. An inline artifact needs none of that machinery —
                                // it is an element in a flow that draws itself — so what comes
                                // back is where its surfaces ended up, and nothing else.
                                let out = conversation_view::draw(
                                    ui,
                                    chat,
                                    &surface_images,
                                    &exhibit_contents,
                                    theme,
                                    theme_name,
                                    form,
                                    // 🚨 **Where a panel summoned here would GO — a destination
                                    // travelling in, not a body.** The seam that used to be
                                    // here handed this crate a way to *draw* an Organon panel
                                    // in the flow; a panel is not an element of a transcript
                                    // any more (§1.14: a transcript is a log and a control is
                                    // not a log entry), so what the view needs is the answer to
                                    // "is there a stack, and whose region is showing it" — and
                                    // that answer is what lets `/organon` refuse *in the
                                    // composer* instead of on a stderr nobody reads.
                                    panel_home,
                                );
                                surface_requests = out.surfaces;
                                exhibit_requests = out.exhibits;
                                // Applied after the frame, not here: `theme` is borrowed from
                                // `self` for the whole of this closure, so assigning it now is a
                                // borrow error rather than a style choice.
                                theme_change = out.theme;
                                // …and the panel a `/organon` line asked for, out for the same
                                // reason: pushing it needs `&mut self`, and `self` is split
                                // into disjoint field borrows for the whole of this closure.
                                panel_wanted = out.panel;
                            }
                            _ => {
                                ui.centered_and_justified(|ui| {
                                    ui.monospace("no live tab — ⌘T opens one");
                                });
                            }
                        }
                    };
                    // 🚨 **The single-region fast path, and it is the whole of invariant #4.**
                    // A console that has had no `/viewport` typed does not merely *look* like
                    // the one built before this module existed — it runs the identical code:
                    // no child `Ui`, no id salt, no clip rect, no separator. The comparison is
                    // against the value `Console::new` starts from, so the two cannot drift.
                    if layout == organon_console::region::Layout::default() {
                        draw_active_pane(ui);
                    } else {
                        region_viewport_rect = draw_regions(
                            ui,
                            pane_rect,
                            &layout,
                            theme,
                            &mut RegionViewport {
                                // The image only when the region is the one that got the frame;
                                // when the portal took it the region paints a notice and this is
                                // never read, but handing it a texture it must not draw is a
                                // trap set for whoever edits the arm next.
                                image: (!portal_has_the_frame).then_some(viewport_image).flatten(),
                                input: viewport_input,
                                yielded_to_portal: portal_has_the_frame,
                            },
                            &mut RegionPanels {
                                stacks: panel_stacks,
                                lines: region_lines,
                                registry: registry_for_lines,
                                ran: &mut region_ran,
                                form,
                                // 🚨 **The seam only this crate can fill** — the console lib
                                // cannot see `OrganicMathParams`, and it cannot see
                                // `theme::card` either, which is why the *card* crosses here
                                // and not just the body (`panel_stack::OrganonDraw`). Reached
                                // for **every** panel in the column, `Declared` ones included:
                                // they get the same chrome, with the console's own sentence
                                // inside it.
                                //
                                // ⚠️ `absent_body` is asked rather than `panel.status` matched,
                                // so the rule about which panels have controls is stated in one
                                // place. `true` because this build really does draw a card.
                                //
                                // What a control writes is a `PresetValues` mirror rather than
                                // a parameter, because a parameter cannot be written from
                                // outside `nih_plug` at all: `param_sink` owns that account,
                                // and `OrganonPanels::overlay` is where the mirror reaches the
                                // world.
                                draw: &mut |ui, panel| {
                                    organon_panels.card(
                                        ui,
                                        panel,
                                        organon_console::panel_stack::absent_body(panel),
                                    );
                                    true
                                },
                            },
                            &mut draw_active_pane,
                        );
                    }
                    // The portal, over whichever front-end just drew. **After the content and
                    // inside the same layer**, which buys both halves at once: within one layer
                    // painter order is draw order, so it lands over the glyphs with no z-order
                    // machinery, and registering the interaction region after the content is
                    // what wins the tie for a drag — `scene_input`'s own tested arrangement,
                    // "in workstation mode the pane registers after the scroll area, and egui
                    // breaks a tie by taking the topmost".
                    //
                    // ⚠️ Unchanged from a person's point of view, and structurally it is now the
                    // *second* call site of one function rather than the only one. The portal
                    // always has the frame when it is open ([`engine_plan`]), so the image here
                    // is unconditionally its own — which is why this arm needs no equivalent of
                    // the region's yielded notice.
                    if let Some(rect) = portal_rect {
                        paint_viewport(
                            ui,
                            rect,
                            viewport_image,
                            viewport_input,
                            scene_input::SceneMode::Workstation,
                            theme,
                        );
                    }
                });
        });
        // Taken out of the accumulator here, on the first line after the closure, so the
        // field borrow ends before anything below needs `&mut self` (`Console::apply`,
        // `Console::apply_console`). Applying it is a few lines further down, once those have
        // run — see there for why the camera reaches the world in the frame it was moved in.
        //
        // 🚨 **One accumulator, drained once, whichever rectangle filled it.** There is one
        // camera because there is one `World`, so a viewport region and the portal are two
        // windows onto the same viewpoint rather than two viewpoints — and the hand-outranks-an-
        // agent arbitration below therefore needed no widening at all: a drag is a drag.
        let camera = viewport_input.gesture.take();
        state.handle_platform_output(window, out.platform_output);
        // What the next frame's backdrop is sized to. Two point sizes, never pixels and never
        // a scale: the conversion belongs with the clamps in `pane_pixels_in`, and it is the
        // *ratio* of these two that survives a scale nobody has measured yet.
        self.pane_points = pane_rect.map(|r| (r.width(), r.height()));
        self.window_points = window_rect.map(|r| (r.width(), r.height()));
        // What the next frame's `render_surfaces` draws, and which tab asked. Recorded here,
        // beside the two point sizes, because it is the same one-frame-behind arrangement for
        // the same reason — a rect is an output of the layout that produced it.
        self.surface_requests = surface_requests;
        self.exhibit_requests = exhibit_requests;
        self.surface_pane = active;
        // The live colour editor's work, applied now that the closure's borrow of `self.theme`
        // has ended.
        if let Some(change) = theme_change {
            self.apply_theme_change(change);
        }
        // …and a panel a `/organon` line asked for, applied now that the closure's shared
        // borrow of `self.panel_stack` has ended.
        //
        // ⚠️ **No destination check here, and none is missing.** `panel_home` was computed
        // from the same layout this frame drew, and the view refuses in the composer when it
        // is `Nowhere` — so an answer arriving at all means a region was showing a stack when
        // the line was typed. Re-asking here would be a second gate that could only ever
        // disagree with the sentence a person has already been shown.
        if let Some(panel) = panel_wanted {
            // ⚠️ **`Home` again, not a remembered answer.** With a column per region this is a
            // real choice rather than a name for the only one — and it is asked of the same
            // layout the frame drew, which is what `panel_home` was computed from, so the
            // region the view *named* in its answer is the region written to here.
            if let organon_console::panel_stack::Home::Shown(region) = panel_home {
                self.panel_stacks.get_mut(region).push(panel);
            }
        }
        // 🚨 **What a region's own command line asked for, applied through the SAME dispatch
        // every other door uses.** Nothing is applied locally: the call goes to
        // `Capabilities::local` — the composer's own lane — which writes the console's sidecar,
        // which `drain_console` drains next frame through the real `CommandService`. So a line
        // typed in a region leaves a `CommandRun` record exactly as a line typed in the composer
        // does, and the receipt says **accepted**, never applied (§1.8).
        for (region, name, args) in region_ran {
            use organon_console::mcp::ToolDispatch;
            // ⚠️ **A `ConsoleDispatch` built here rather than held**, exactly as the pane's
            // `local` is built at tab construction: it is a cheap value over the published
            // viewpoint cell, and holding one on `Console` would be a second handle to the same
            // cells with nothing to gain by it.
            let mut dispatch = ConsoleDispatch { viewpoint: self.viewpoint.clone() };
            let result = dispatch.call(&name, args);
            let receipt = organon_console::registry::receipt_of(&name, &result);
            // 🚨 **A refusal is news; an acceptance is not.** ✏️ This wrote the receipt back
            // unconditionally, which is the third row James asked to remove from these bands
            // (*"completely remove the status lines in the add remove inputs on the panels"*) —
            // and an acceptance had the least to say of the three: `console.stack —
            // {"accepted":"stack add surface region left"}` restates the line still on screen,
            // and the panel appearing in the column above is the answer.
            //
            // ⚠️ **`receipt.ok` rather than a test on the text**, which is what that field is
            // for: this is the one place a refusal from *dispatch* — as opposed to one
            // `region_line::act` produced before dispatching — can reach the person who typed it,
            // and a surface that dropped it would leave a command that silently did nothing.
            if !receipt.ok {
                self.region_lines.note(region, receipt.text);
            }
        }
        // What the next frame's `render_viewport` sizes its texture to — points, never pixels,
        // for `pane_points`' reason: it is the *ratio* to the window that survives a scale
        // nobody has measured yet.
        //
        // 🚨 **`or`, and the order IS [`engine_plan`]'s precedence.** `portal_rect` is `Some`
        // only while the portal is open, and an open portal takes the frame — so preferring it
        // here is the same rule spelled in the same order, not a second decision that has to be
        // kept in step. `region_viewport_rect` is `None` whenever the region yielded (it painted
        // a notice, so there is no viewport rectangle to size anything to), which is what stops
        // a yielded region from quietly resizing the portal's own texture underneath it.
        self.viewport_points =
            portal_rect.or(region_viewport_rect).map(|r| (r.width(), r.height()));
        // The camera gesture into the world, once per frame, after the UI and before the next
        // render — `wgpu_editor`'s precedent exactly, so a drag reaches the camera in the frame
        // it was made. This is the line the whole "shows the World, not the substrate" argument
        // exists to make effective: with a substrate rig installed, every one of these writes
        // would be discarded downstream, with no error and no log line.
        //
        // 🚨 **And the stamp that makes "the hand always wins" enforceable.** It is taken here,
        // from the gesture, because this is the last place the two kinds of camera input are
        // still distinguishable: one line below they are both `CameraInput` and `World` has no
        // way to ask which was which. `inputs()` is empty on every frame nobody is touching the
        // portal, so an idle console never stamps and an agent is never held off by a hand that
        // is not there.
        let mut moved_by_hand = false;
        for input in camera.inputs() {
            moved_by_hand = true;
            self.world.apply_camera_input(input);
        }
        if moved_by_hand {
            self.hand_camera_at = Some(Instant::now());
        }
        // 🚨 **The read's publication point, and the position is the whole of its correctness.**
        // Both writers have now run: the agent's framing in `drain_console` at the top of this
        // function, and the hand's gesture on the line above. Publishing anywhere earlier would
        // hand an agent a value from halfway through the frame — most damagingly, one taken
        // before the drag it is about to be told did not happen.
        //
        // It reads `World::camera_framing()` rather than anything this file remembers, so the
        // three axes are the ones the world actually holds *after its own clamps*, and a hand's
        // move is reported exactly as an agent's is. That is what makes this a measurement
        // rather than an echo.
        //
        // Unconditional: a frame in which nothing moved still republishes, because `portal_open`,
        // `region_3d` and `backdrop_shows_world` can all change without the camera doing so, and
        // a cell that only updated on movement would report a stale visibility forever.
        let (yaw, pitch, distance) = self.world.camera_framing();
        self.viewpoint.publish(camera::Viewpoint {
            yaw,
            pitch,
            distance,
            portal_open: self.portal_state.is_open(),
            // ⚠️ **The LAYOUT, not the frame.** A region can hold `3d` while the portal has the
            // world, and in that state the region is showing a notice — but the camera is still
            // emphatically visible (the portal is drawing it), so `visible` is true either way.
            // Reporting the layout is also the more useful fact: it is what a `console.camera`
            // caller would have to know to predict where its framing will show up.
            region_3d: self.region_showing_world().is_some(),
            backdrop_shows_world: self.render_source() == BackdropSource::World,
            hand_last: self.hand_camera_at,
            agent_last: self.agent_camera_at,
        });
        if let Some(action) = action {
            self.apply(action);
        }
        // The chord and `organon console screen toggle` are the **same call** from here on —
        // one word, resolved once, so the key can never drift from the verb. Spelled back into
        // its word rather than passed as a value, which is the arrangement `apply_console` uses
        // for a button in the scrollback: it costs a three-way match and it means there is
        // exactly one path a screen change can take.
        if let Some(cmd) = screen_cmd {
            self.set_screen(cmd.as_word());
        }
        // A button pressed inside the scrollback and the same word typed at a prompt are the
        // **same call** from here on — `apply_console` is exactly where `organon console
        // background <name>` lands after `drain_console` has validated it, Tier 4's look-epoch
        // record included. That is the whole claim of the panel kind in one line: it is not
        // simulating the console, it is driving it. (A label this console does not know falls
        // into `apply_console`'s own "names nothing" arm and says so on stderr; nothing here
        // has to re-check it.)
        for act in block_actions {
            self.apply_console(&cli::ConsoleOp::Background(act.button));
        }
        // ⚠️ **A second loop stood here, for the conversation view's inline panels.** It was
        // reachable only from `/panel`, and what it did was exactly the complaint: a material
        // clicked in a conversation repainted the backdrop of a *terminal* tab, so the panel
        // you were looking at appeared to do nothing. `/surface` supersedes it — its panel
        // drives an element in the same transcript, and the press is consumed by the surface
        // it aims at rather than travelling out here.
        let (Some(window), Some(gpu), Some(state), Some(renderer)) = (
            self.window.as_ref(),
            self.gpu.as_mut(),
            self.egui_state.as_mut(),
            self.renderer.as_mut(),
        ) else {
            return;
        };
        let _ = state;

        let jobs = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
        let sd = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.config.width, gpu.config.height],
            pixels_per_point: out.pixels_per_point,
        };
        for (id, delta) in &out.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, delta);
        }
        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("shell-ui") });
        let mut staged = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &sd);
        {
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shell-ui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.003,
                                g: 0.004,
                                b: 0.003,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            renderer.render(&mut rp, &jobs, &sd);
        }
        // Staging buffers first, then our draws — the order ui_layer.rs documents.
        staged.push(encoder.finish());
        queue.submit(staged);
        // wgpu 30 moved `present` onto the queue (the World::present precedent).
        queue.present(frame);
        for id in &out.textures_delta.free {
            renderer.free_texture(id);
        }
        window.request_redraw();
    }
}

impl ApplicationHandler for Console {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // The icon is hung on by `console_icon::apply` rather than inline, because the
        // title-bar icon and the taskbar icon are two different slots set by two
        // different APIs — only one of which is portable. See that module.
        let attrs = console_icon::apply(
            Window::default_attributes()
                .with_title(PRODUCT_NAME)
                .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0)),
        );
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        self.init_gpu(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::Occluded(hidden) = event {
            self.occluded = hidden;
            if !hidden {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
        if let (Some(window), Some(state)) = (self.window.as_ref(), self.egui_state.as_mut()) {
            let response = state.on_window_event(window, &event);
            if response.repaint && !self.occluded {
                window.request_redraw();
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(device)) = (self.gpu.as_mut(), self.world.device()) {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(device, &gpu.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.quit {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

/// What the command line asked for, as a value.
///
/// Pure so the decision is unit-tested without a window server — the same reason
/// [`organon_console::platform::Platform`] is a value rather than a `#[cfg]`.
#[derive(Debug, PartialEq, Eq)]
enum Invocation {
    Help,
    Version,
    Run,
}

/// ⚠️ **`organon-console --help` used to hang forever** (back when the binary was still named
/// `organon-shell`). There was no argument handling at all: the flag was ignored, the banner
/// printed, and the winit event loop started — so the
/// obvious way to probe a new product ate the terminal until the user found the window or
/// killed it. The first public-repo trial gave up after three minutes. Every other binary in
/// this tree answers `--help`; this one is a GUI app, which is a reason to keep the answer
/// short, not a reason not to answer.
fn invocation(args: &[String]) -> Invocation {
    for a in args.iter().skip(1) {
        match a.as_str() {
            "-h" | "--help" | "help" => return Invocation::Help,
            "-V" | "--version" => return Invocation::Version,
            _ => {}
        }
    }
    Invocation::Run
}

/// The interface is environment variables, not flags, so `--help` documents *those*.
/// Listing flags this binary does not have would be worse than the silence it replaces.
fn help_text() -> String {
    format!(
        "{PRODUCT_NAME}\n\
         \n\
         Usage: {INVOCATION_NAME}          (no flags; the surface is environment variables)\n\
         \n\
         Options:\n    \
             -h, --help       print this and exit\n    \
             -V, --version    print the version and exit\n\
         \n\
         Environment:\n    \
             ORGANON_SHELL_BACKDROP=<src> behind the glyphs: 0/unset off, 1 the world,\n                                 \
             {substrate} the lit substrate plane\n    \
             ORGANON_SHELL_SCRIM=<0..255> legibility scrim alpha (default {scrim_default}; the\n                                 \
             floor is the PALETTE's — {scrim_floor} on a dark page, {scrim_floor_light} on {light_theme})\n    \
             ORGANON_SHELL_THEME=<name>   palette for THIS launch only: {themes}\n                                 \
             (overrides a stored choice, never writes one)\n    \
             ORGANON_SHELL_TABS=a,b,c     open these harness ids at start\n    \
             ORGANON_SHELL_DEFAULT=<id>   harness for the first tab (else Pi if installed)\n    \
             ORGANON_SHELL_CMD=<cmd>      one plain-command tab, for headless checks\n    \
             ORGANON_SHELL_PTY_DEBUG=1    trace the PTY byte path to stderr ([pty]/[grid])\n    \
             ORGANON_CLAUDE_BIN=<path>    the CLI a conversation tab drives (default: claude)\n    \
             ORGANON_IPC_NS=<name>        IPC namespace; fork it to run beside another Organon\n\
         \n\
         Two front-ends, one window. A harness id opens a terminal tab unless the registry\n\
         marks it a conversation — `claude-chat` is the one that does, and it renders the\n\
         agent's event stream natively instead of its character grid:\n    \
             ORGANON_SHELL_TABS=claude-chat        one conversation tab\n    \
             ORGANON_SHELL_TABS=claude-chat,shell  a conversation beside a terminal\n\
         \n\
         In a conversation tab the composer takes one local command, never sent to the\n\
         agent:\n    \
             /surface   a rendered surface, with the controls that drive it beneath it\n\
         \n\
         Inside a tab the `organon` CLI addresses this process — the namespace is inherited:\n    \
             organon console background <{backgrounds}>\n    \
             organon console rig <{rigs}>\n    \
             organon console theme <{themes}>       live, and stored as a preference\n    \
             organon console posture <{postures}|0.0-1.0>  snaps; not remembered\n    \
             organon console screen <{screens}>     the window, not the form; F11 flips it\n    \
             organon console viewport <{regions}>\n                              \
             <{contents}>  divide the pane; `off` empties a region\n\
         \n\
         Docs: SHELL_ARCHITECTURE.md\n",
        substrate = BACKDROP_SUBSTRATE,
        scrim_default = term_view::SCRIM_DEFAULT,
        scrim_floor = term_view::SCRIM_FLOOR,
        // ⚠️ The floor is the *palette's* since #38, so quoting one number would be a lie the
        // moment `light` became selectable — which is exactly what `organon console theme`,
        // listed further down this same text, made possible. Both floors are quoted, and the
        // light one names the palettes it belongs to.
        scrim_floor_light = term_view::SCRIM_FLOOR_LIGHT,
        // Derived, not named: *which* palettes carry the light floor is a fact about
        // `Theme::scrim_floor`, and a hardcoded "light" would go stale the day a fifth
        // palette wants a light page.
        light_theme = Theme::NAMES
            .iter()
            .filter(|n| {
                Theme::by_name(n).is_some_and(|t| t.scrim_floor == term_view::SCRIM_FLOOR_LIGHT)
            })
            .copied()
            .collect::<Vec<_>>()
            .join("|"),
        themes = Theme::NAMES.join("|"),
        postures = organon_console::posture::POSTURE_WORDS.join("|"),
        screens = organon_console::screen::SCREEN_WORDS.join("|"),
        // Both quoted from `region`'s own tables, on the `backgrounds` rule below: `--help`
        // must not be able to offer a region the console cannot divide into, nor a content
        // word nothing resolves.
        regions = organon_console::region::REGION_WORDS.join("|"),
        contents = organon_console::region::CONTENT_WORDS.join("|"),
        // Quoted from the tables the drain resolves against, never restated — the discipline
        // the scrim line already earned here, and the reason `--help` cannot advertise a
        // material this build cannot draw.
        backgrounds = substrate_materials::MATERIAL_NAMES
            .iter()
            .chain(BACKDROP_SOURCE_WORDS.iter())
            .copied()
            .collect::<Vec<_>>()
            .join("|"),
        rigs = substrate_materials::RIG_NAMES.join("|"),
    )
}

fn main() {
    match invocation(&std::env::args().collect::<Vec<_>>()) {
        Invocation::Help => {
            print!("{}", help_text());
            return;
        }
        Invocation::Version => {
            println!("{} {}", PRODUCT_NAME, env!("CARGO_PKG_VERSION"));
            return;
        }
        Invocation::Run => {}
    }

    eprintln!("{PRODUCT_NAME}");
    let event_loop = EventLoop::new().expect("event loop");
    let mut shell = Console::new();
    event_loop.run_app(&mut shell).expect("run app");
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_and_version_are_answered_not_swallowed() {
        for spelling in [v(&[INVOCATION_NAME, "--help"]), v(&[INVOCATION_NAME, "-h"])] {
            assert_eq!(invocation(&spelling), Invocation::Help, "{spelling:?}");
        }
        for spelling in [v(&[INVOCATION_NAME, "--version"]), v(&[INVOCATION_NAME, "-V"])] {
            assert_eq!(invocation(&spelling), Invocation::Version, "{spelling:?}");
        }
    }

    /// argv[0] is a path, and on Windows it can be `...\organon-console.exe` — neither may be
    /// mistaken for a flag, or the app would print help instead of starting.
    #[test]
    fn argv0_is_never_read_as_a_flag() {
        assert_eq!(invocation(&v(&["/usr/local/bin/organon-console"])), Invocation::Run);
        assert_eq!(invocation(&v(&[r"C:\tools\help\organon-console.exe"])), Invocation::Run);
        assert_eq!(invocation(&v(&[INVOCATION_NAME])), Invocation::Run);
    }

    /// The scrim line is quoted from `term_view`'s constants, not restated — the first draft
    /// said `<0..1>` from memory, and `0.5` fails the `u8` parse, gets swallowed by `.ok()`
    /// and silently falls back. This pins both halves: the byte scale in the notation, and
    /// the actual numbers. Change `SCRIM_DEFAULT`/`SCRIM_FLOOR` and the help follows; write a
    /// literal back into the help and this fails.
    ///
    /// ⚠️ **BOTH floors, since #38.** The floor became the palette's when `light` was added,
    /// and quoting only `SCRIM_FLOOR` was true exactly as long as no palette could be
    /// selected — `organon console theme light` ended that, so a single number in `--help`
    /// would now be a confident lie about the console the reader is looking at.
    #[test]
    fn the_scrim_line_matches_the_code_it_documents() {
        let h = help_text();
        assert!(h.contains("ORGANON_SHELL_SCRIM=<0..255>"), "scrim scale is a u8, not 0..1");
        assert!(!h.contains("<0..1>"), "the 0..1 form silently no-ops — never document it");
        assert!(
            h.contains(&format!("default {}", organon_console::term_view::SCRIM_DEFAULT)),
            "help does not quote SCRIM_DEFAULT"
        );
        for floor in
            [organon_console::term_view::SCRIM_FLOOR, organon_console::term_view::SCRIM_FLOOR_LIGHT]
        {
            assert!(h.contains(&floor.to_string()), "help does not quote the floor {floor}");
        }
        // …and it names which palette the light floor belongs to, so the two numbers are
        // usable rather than merely present.
        assert!(h.contains("light"), "help does not say whose floor {} is",
            organon_console::term_view::SCRIM_FLOOR_LIGHT);
    }

    /// CONTRACT: **`--help` offers every palette and posture the console can actually
    /// reach**, quoted from `Theme::NAMES` and `POSTURE_WORDS` rather than restated — the
    /// discipline the background/rig lines already earned here, for its reason: a `--help`
    /// that advertises a palette this build cannot paint is worse than one that lists none.
    #[test]
    fn the_help_offers_the_palettes_and_postures_that_exist() {
        let h = help_text();
        assert!(h.contains("organon console theme"), "the theme verb is unlisted");
        assert!(h.contains("organon console posture"), "the posture verb is unlisted");
        assert!(h.contains(theme::THEME_ENV), "the one-launch override is undocumented");
        for name in Theme::NAMES {
            assert!(h.contains(name), "`{name}` is a palette `--help` never mentions");
        }
        for word in organon_console::posture::POSTURE_WORDS {
            assert!(h.contains(word), "`{word}` is a posture `--help` never mentions");
        }
        assert!(h.contains("organon console screen"), "the screen verb is unlisted");
        for word in organon_console::screen::SCREEN_WORDS {
            assert!(h.contains(word), "`{word}` is a screen state `--help` never mentions");
        }
        // 🚨 **The chord is documented where somebody stuck in full screen would look**, and
        // that is the one part of this verb whose absence is not a documentation gap but a
        // trap: a borderless window has no close button, so a person who does not know the key
        // and does not remember the verb has no way back inside the app. It is asserted rather
        // than trusted for exactly that reason.
        assert!(
            h.contains(&format!("{:?}", organon_console::screen::CHORD)),
            "the way OUT of full screen is not in `--help`"
        );
    }

    /// 🚨 CONTRACT: **the slot `/theme` carries its value in is the slot the conversation view
    /// reads.** The view has to look inside a `console.theme` dispatch to see whether it was
    /// asked to open the live colour editor (§1.10), and it names that slot with its own
    /// constant. Two spellings would agree today and diverge silently the day this one is
    /// renamed — `/theme edit` would then dispatch as an unknown palette instead of opening
    /// anything, with nothing to say so. The verb's own name needs no test: `CMD_THEME` is an
    /// alias of `registry::VERB_THEME` and cannot drift at all.
    #[test]
    fn the_theme_verbs_slot_name_is_the_one_the_view_reads() {
        assert_eq!(CMD_ARG, organon_console::registry::THEME_ARG);
        assert_eq!(CMD_THEME, organon_console::registry::VERB_THEME);
    }

    /// 🚨 CONTRACT: **the two editor words are offered by the schema and refused on this lane**,
    /// and both halves are necessary. They must be in the `Choice` or `Registry::resolve`
    /// refuses `/theme edit` during validation, before the conversation view — where the editor
    /// is actually drawn — ever sees it. They must be refused *here* because this is the
    /// sidecar, which the CLI and an agent's tool call both arrive on, and neither has a band
    /// above a composer to put a dialog in.
    #[test]
    fn the_editor_words_are_offered_but_never_dispatched() {
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_THEME)
            .expect("the theme spec");
        let ArgKind::Choice(options) = &spec.args[0].kind else {
            panic!("`{CMD_THEME}`'s argument stopped being a Choice, so nothing completes it");
        };
        for word in organon_console::theme_edit::EDIT_WORDS {
            assert!(options.contains(&word.to_string()), "`{word}` is not offered: {options:?}");
            let err = op_from(CMD_THEME, &json!({ CMD_ARG: word }))
                .expect_err("the sidecar must refuse an editor word");
            assert!(err.contains(word), "the refusal does not quote what was asked: {err}");
            assert!(
                err.contains("conversation tab"),
                "the refusal must say where the editor actually lives: {err}"
            );
        }
        // …and every palette name still goes through untouched.
        for name in Theme::NAMES {
            op_from(CMD_THEME, &json!({ CMD_ARG: name }))
                .unwrap_or_else(|e| panic!("`{name}` should still dispatch: {e}"));
        }
    }

    /// The backdrop's value space, both halves: the new spelling reaches the substrate, and
    /// **every other value still means what it meant before**. That second half is the point —
    /// Tier 1 widened `ORGANON_SHELL_BACKDROP` rather than redefining it, and a console that
    /// quietly stopped rendering the World would take the CLI's live override lane with it.
    #[test]
    fn the_backdrop_value_space_is_widened_not_redefined() {
        assert_eq!(parse_backdrop_source(None), BackdropSource::Off);
        assert_eq!(parse_backdrop_source(Some("0")), BackdropSource::Off);
        assert_eq!(parse_backdrop_source(Some(BACKDROP_SUBSTRATE)), BackdropSource::Substrate);
        assert_eq!(parse_backdrop_source(Some("SUBSTRATE")), BackdropSource::Substrate);
        // Every spelling that was "on" before is still the World, including the ones nobody
        // types: the old rule was literally `!= "0"`.
        for on in ["1", "2", "", "true", "yes", "substrat", "substratee"] {
            assert_eq!(parse_backdrop_source(Some(on)), BackdropSource::World, "{on:?}");
        }
    }

    /// **`ORGANON_SHELL_BACKDROP=1` publishes today's bytes, unchanged.** The snapshot the
    /// console writes every redraw is what `organon status`/`get`/`watch` read and what the
    /// world renders from, so a substrate write leaking into the World source would change the
    /// product's whole default look — quietly, since it would still render something. The
    /// claim is checked against the raw bytes rather than a field list, which is the only form
    /// that cannot be outgrown by a future look constant.
    #[test]
    fn only_the_substrate_source_touches_the_published_snapshot() {
        let base = OrganicMathParams::default().to_shared();
        for src in [BackdropSource::Off, BackdropSource::World] {
            assert_eq!(
                bytemuck::bytes_of(&*initial_shared(src)),
                bytemuck::bytes_of(&base),
                "{src:?} must publish exactly today's default look"
            );
        }
        let sub = initial_shared(BackdropSource::Substrate);
        assert_ne!(bytemuck::bytes_of(&*sub), bytemuck::bytes_of(&base), "substrate is a look");
        // The azimuth override is applied AFTER the look, and really does replace it — the
        // ordering is the whole content of that constant's doc comment.
        assert_eq!(sub.lighting[4], SUBSTRATE_KEY_AZIMUTH_DEG);
        assert_ne!(
            SUBSTRATE_KEY_AZIMUTH_DEG,
            substrate_scene::SUBSTRATE_KEY_AZIMUTH_DEG,
            "if the leaf ever adopts this camera's azimuth, delete the override rather than \
             keeping two constants that agree"
        );
    }

    /// The new value is documented, and quoted from the constant the parser uses — the same
    /// reason the scrim line is formatted rather than restated. A `substrate` that worked and
    /// was undocumented would be `ORGANON_SHELL_PTY_DEBUG`'s mistake a second time.
    #[test]
    fn the_backdrop_line_documents_the_substrate_value() {
        let h = help_text();
        assert!(h.contains(BACKDROP_SUBSTRATE), "help does not name the substrate source");
        assert!(h.contains("0/unset off"), "help does not say what unset means");
    }

    /// **The binary introduces itself as what you launched, and never as the *shell*.**
    /// `--help`'s header and usage line are the console's front door.
    ///
    /// ⚠️ **This test used to assert the opposite of what it asserts now, and the inversion was
    /// a deliberate product decision rather than a drift.** It was written when
    /// [`PRODUCT_NAME`] was the artifact string and carried
    /// `assert!(!h.contains("Organon Console"))` — on the reasoning that a header naming the
    /// product rather than the binary would name "a product that is not what ran". `474e8cd`
    /// ("Give the console its own name everywhere the name is ours to give") then set
    /// `PRODUCT_NAME` to `"Organon Console"` on purpose, which made that assertion contradict
    /// the `starts_with` on the line above it — the two could not both hold, and the console
    /// edition's test leg went red the moment they met. The surviving intent is the *shell*
    /// half, which is what the name of this test was always about.
    ///
    /// ⚠️ The **variable names** below are the opposite case: `ORGANON_SHELL_*` is a shipped
    /// flag surface and stays. Presentation renames, identifiers do not.
    #[test]
    fn the_console_does_not_introduce_itself_as_the_shell() {
        let h = help_text();
        assert!(h.starts_with(PRODUCT_NAME), "help header does not name the product");
        assert!(
            h.contains(&format!("Usage: {INVOCATION_NAME}")),
            "usage line names the wrong command"
        );
        assert!(!h.contains("Organon Shell"), "the console must not present as Organon Shell");
        assert!(!h.contains("organon-shell "), "the usage line still names the old binary");
        // …and the environment variables are untouched by all of the above.
        assert!(h.contains("ORGANON_SHELL_BACKDROP"), "the flag surface is NOT renamed");
    }

    // -------------------------------------------------------------------------
    // The console command lane (#4 Tier 2)
    // -------------------------------------------------------------------------

    fn look(material: Option<&str>, rig: Option<&str>) -> ConsoleLook {
        ConsoleLook { material: material.map(str::to_string), rig: rig.map(str::to_string) }
    }

    fn bg(name: &str) -> cli::ConsoleOp {
        cli::ConsoleOp::Background(name.to_string())
    }

    fn rig_op(name: &str) -> cli::ConsoleOp {
        cli::ConsoleOp::Rig(name.to_string())
    }

    /// The whole command grammar as one state machine, stated once: a material implies its
    /// source, a bare source word fills in a default material *once*, and a rig never moves
    /// the source. All three are decisions a reader could reasonably expect to go the other
    /// way, so all three are pinned rather than commented.
    #[test]
    fn a_typed_name_folds_into_the_source_and_the_look() {
        let cold = ConsoleLook::default();

        // A material names its own source — `background substrate` is not a prior mode.
        let (src, l) = console_step(BackdropSource::Off, &cold, &bg("graphite")).unwrap();
        assert_eq!(src, BackdropSource::Substrate);
        assert_eq!(l, look(Some("graphite"), None));

        // The bare source word fills in the default material…
        let (s2, l2) = console_step(BackdropSource::Off, &cold, &bg(BACKDROP_SUBSTRATE)).unwrap();
        assert_eq!((s2, l2), (BackdropSource::Substrate, look(Some(CONSOLE_DEFAULT_MATERIAL), None)));
        // …and never overwrites one already chosen.
        let (_, l3) = console_step(src, &l, &bg(BACKDROP_SUBSTRATE)).unwrap();
        assert_eq!(l3, look(Some("graphite"), None));

        // A rig changes the rig and nothing else — including at a source that draws none of
        // it, so `rig daylight` then `background substrate` reads as it looks.
        let (s4, l4) = console_step(BackdropSource::World, &cold, &rig_op("daylight")).unwrap();
        assert_eq!((s4, l4.clone()), (BackdropSource::World, look(None, Some("daylight"))));

        // `world` / `off` keep the dressing so `substrate` restores it rather than resetting.
        let (s5, l5) = console_step(src, &l, &bg("world")).unwrap();
        assert_eq!((s5, l5.clone()), (BackdropSource::World, look(Some("graphite"), None)));
        let (s6, l6) = console_step(s5, &l5, &bg("off")).unwrap();
        assert_eq!((s6, l6.clone()), (BackdropSource::Off, look(Some("graphite"), None)));
        let (s7, l7) = console_step(s6, &l6, &bg(BACKDROP_SUBSTRATE)).unwrap();
        assert_eq!((s7, l7), (BackdropSource::Substrate, look(Some("graphite"), None)));
    }

    /// Whatever was typed, the *canonical* spelling is stored — so [`ConsoleLook`] is a value
    /// Tier 4's epoch ledger can compare, not a transcript of keystrokes.
    #[test]
    fn names_are_stored_canonically_however_they_were_typed() {
        let cold = ConsoleLook::default();
        for (typed, want) in [("GRAPHITE", "graphite"), ("Slate", "slate"), ("mEtAl", "metal")] {
            let (_, l) = console_step(BackdropSource::Off, &cold, &bg(typed)).unwrap();
            assert_eq!(l, look(Some(want), None), "{typed}");
        }
        let (_, l) = console_step(BackdropSource::Off, &cold, &rig_op("DAYLIGHT")).unwrap();
        assert_eq!(l, look(None, Some("daylight")));
        // The source words too, though they leave no name behind.
        assert_eq!(console_source("Off"), Some(BackdropSource::Off));
    }

    /// The sidecar format's forward-compatibility contract, carried one level down from the
    /// verb to the argument: `parse_console_op` skips a verb this build does not know, and a
    /// **name** it does not know must be just as inert — the backdrop stays exactly as it was.
    #[test]
    fn an_unknown_name_changes_nothing_at_all() {
        let dressed = look(Some("metal"), Some("daylight"));
        for op in [
            bg("nonsense"),
            bg("studio"), // a rig is not a background
            bg(""),
            bg("substrat"), // the near-miss `parse_backdrop_source` deliberately accepts
            rig_op("slate"), // …and a material is not a rig
            rig_op("sunset"),
            rig_op(""),
        ] {
            assert_eq!(console_step(BackdropSource::Substrate, &dressed, &op), None, "{op:?}");
        }
    }

    /// **Default-inertness, extended to the live paths.** Tier 1 pinned it for
    /// `initial_shared`; a console that can now *arrive* at `world` from a dressed substrate
    /// has to publish today's default bytes there too, whatever look it is still remembering.
    /// Otherwise `background world` would hand the World a frozen membrane and call it the
    /// default look.
    #[test]
    fn world_and_off_publish_todays_default_bytes_whatever_look_is_remembered() {
        let base = OrganicMathParams::default().to_shared();
        for src in [BackdropSource::Off, BackdropSource::World] {
            for l in [
                ConsoleLook::default(),
                look(Some("metal"), Some("daylight")),
                look(Some("paper"), None),
                look(None, Some("studio")),
            ] {
                assert_eq!(
                    bytemuck::bytes_of(&*look_shared(src, &l)),
                    bytemuck::bytes_of(&base),
                    "{src:?} must publish exactly today's default look, look was {l:?}"
                );
            }
        }
    }

    /// **Startup is Tier 1's substrate, byte for byte.** Tier 2 adds a vocabulary, not a new
    /// opening position: a console launched with `ORGANON_SHELL_BACKDROP=substrate` looks
    /// exactly as it did before this tier until somebody types a command.
    #[test]
    fn startup_is_tier_ones_substrate_untouched_by_tier_two() {
        let mut want = OrganicMathParams::default().to_shared();
        substrate_scene::apply_substrate_look(&mut want);
        want.lighting[4] = SUBSTRATE_KEY_AZIMUTH_DEG;
        assert_eq!(
            bytemuck::bytes_of(&*initial_shared(BackdropSource::Substrate)),
            bytemuck::bytes_of(&want),
            "startup must apply no material at all"
        );
        assert_eq!(ConsoleLook::default().material, None, "…which is what `None` means");
    }

    /// **The residue verdict, from the integrator's side.** Leaf A proves all 16 ordered
    /// material pairs converge *within its own block*; this proves the whole published
    /// snapshot converges through the reset sequence, including a detour via the two sources
    /// that wipe it. If `look_shared` ever became a patch instead of a recompute, this is the
    /// test that would fail.
    #[test]
    fn every_route_to_a_dressing_lands_on_the_same_bytes() {
        for to in substrate_materials::MATERIAL_NAMES {
            for rig in [None, Some("studio"), Some("daylight")] {
                let cold = look_shared(BackdropSource::Substrate, &look(Some(to), rig));
                for from in substrate_materials::MATERIAL_NAMES {
                    let mut src = BackdropSource::Off;
                    let mut l = ConsoleLook::default();
                    let mut walk = vec![bg(from), bg("world"), bg("off"), bg(to)];
                    if let Some(r) = rig {
                        walk.push(rig_op(r));
                    }
                    for op in &walk {
                        let (s, next) = console_step(src, &l, op).expect("every step is known");
                        src = s;
                        l = next;
                    }
                    assert_eq!(src, BackdropSource::Substrate);
                    assert_eq!(
                        bytemuck::bytes_of(&*look_shared(src, &l)),
                        bytemuck::bytes_of(&*cold),
                        "`{from}` → world → off → `{to}` / {rig:?} did not converge"
                    );
                }
            }
        }
    }

    /// `rig: None` and `rig studio` are the same picture, which is what lets startup name no
    /// rig without being a secret third state. The day Leaf A's `studio` stops being Tier 1's
    /// shipped rig, this is where the two stop agreeing.
    #[test]
    fn an_unnamed_rig_is_studio() {
        for m in substrate_materials::MATERIAL_NAMES {
            assert_eq!(
                bytemuck::bytes_of(&*look_shared(BackdropSource::Substrate, &look(Some(m), None))),
                bytemuck::bytes_of(&*look_shared(
                    BackdropSource::Substrate,
                    &look(Some(m), Some("studio"))
                )),
                "{m}"
            );
        }
    }

    /// **The camera's azimuth override survives every dressing.** It is applied after the
    /// look, the material *and* the rig precisely so nothing downstream can take it back;
    /// Leaf A's rigs write no direction today and name this override as the reason, so a rig
    /// that started writing one would fail here instead of silently re-aiming the key light.
    #[test]
    fn the_key_azimuth_override_survives_every_material_and_rig() {
        for m in substrate_materials::MATERIAL_NAMES {
            for r in substrate_materials::RIG_NAMES {
                let s = look_shared(BackdropSource::Substrate, &look(Some(m), Some(r)));
                assert_eq!(s.lighting[4], SUBSTRATE_KEY_AZIMUTH_DEG, "{m} / {r}");
            }
        }
    }

    /// **The shell-side half of the name-list drift guard.** The `CommandService` schema and
    /// the resolver are two independent code paths over the same tables — the schema is built
    /// from `MATERIAL_NAMES`/`RIG_NAMES`/[`BACKDROP_SOURCE_WORDS`], the resolver goes through
    /// `console_source` + `canonical` — so this pins that they admit *exactly* the same names.
    /// A vocabulary the catalog validates but the resolver refuses would log `Ok` and change
    /// nothing. The CLI-side half (clap's lists against the same two tables) is in
    /// `bin/ctl.rs`'s tests.
    #[test]
    fn the_catalog_and_the_resolver_accept_exactly_the_same_names() {
        let specs = console_specs();
        let choice = |name: &str| -> Vec<String> {
            let spec = specs.iter().find(|s| s.name == name).expect("spec registered");
            assert_eq!(spec.target, TargetKind::Viewport, "{name}: a backdrop is the viewport");
            let arg = spec.args.iter().find(|a| a.name == CMD_ARG).expect("one `name` argument");
            assert!(arg.required, "{name}: the name is not optional");
            match &arg.kind {
                ArgKind::Choice(v) => v.clone(),
                k => panic!("{name}: {k:?} is not a Choice — unknown names would reach the apply"),
            }
        };
        let backgrounds = choice(CMD_BACKGROUND);
        let rigs = choice(CMD_RIG);

        // Everything the schema admits, the resolver applies.
        let cold = ConsoleLook::default();
        for name in &backgrounds {
            assert!(
                console_step(BackdropSource::Off, &cold, &bg(name)).is_some(),
                "the catalog offers background `{name}` but the resolver refuses it"
            );
        }
        for name in &rigs {
            assert!(
                console_step(BackdropSource::Off, &cold, &rig_op(name)).is_some(),
                "the catalog offers rig `{name}` but the resolver refuses it"
            );
        }
        // …and everything the tables hold, the schema admits.
        for m in substrate_materials::MATERIAL_NAMES {
            assert!(backgrounds.iter().any(|c| c == m), "material `{m}` is not in the catalog");
        }
        for w in BACKDROP_SOURCE_WORDS {
            assert!(backgrounds.iter().any(|c| c == w), "source `{w}` is not in the catalog");
        }
        for r in substrate_materials::RIG_NAMES {
            assert!(rigs.iter().any(|c| c == r), "rig `{r}` is not in the catalog");
            assert!(
                !backgrounds.iter().any(|c| c == r),
                "`{r}` leaked into the background vocabulary — the two lists are separate"
            );
        }
    }

    /// 🚨 **The MCP half of the same drift guard: every console verb is served as a tool,
    /// with a schema GENERATED from its spec.**
    ///
    /// The tool table and the sidecar's validation are now two renderings of one
    /// `console_specs()`, exactly as the CLI's `--help` is a third. This pins that the set is
    /// complete (a verb added to the table is served without an edit here), that no name is
    /// lost to MCP's tool-name grammar, and that the schema each tool carries is the one its
    /// spec generates — the thing a hand-written second table cannot promise, and which this
    /// tree has already paid for at nine wrong ranges out of forty-five.
    ///
    /// ⚠️ **Runs under `cargo check --profile test` only in this session** — it lives behind
    /// `console-edition` in `console_main.rs`, so CI is what actually executes it.
    #[test]
    fn every_console_verb_is_served_as_a_tool_with_the_schema_its_spec_generates() {
        use organon_console::mcp::{input_schema, tool_name_for, McpServer, PermissionDecision};
        let specs = mcp_specs();
        let server = McpServer::new(
            &specs,
            Box::new(|_: &organon_console::mcp::PermissionRequest| {
                PermissionDecision::deny("not this test's business")
            }),
        )
        .with_server_name(conversation_view::SERVER_NAME);

        assert!(
            server.name_collisions().is_empty(),
            "a verb whose sanitised tool name collides is silently NOT served: {:?}",
            server.name_collisions()
        );
        assert_eq!(server.tools().len(), specs.len(), "every verb, and nothing hand-added");

        for spec in &specs {
            let entry = server
                .tools()
                .iter()
                .find(|t| t.command_name == spec.name)
                .unwrap_or_else(|| panic!("`{}` is in the table but is not served", spec.name));
            assert_eq!(entry.tool_name, tool_name_for(&spec.name));
            assert_eq!(
                entry.input_schema,
                input_schema(spec),
                "`{}` would be served a schema its spec did not generate",
                spec.name
            );
            assert_eq!(entry.description, spec.doc, "the palette's line is the tool's");
        }

        // The names the agent actually types, and the one that must never be among them.
        let served = server.namespaced_tool_names();
        assert!(served.contains(&"mcp__organon__console_portal".to_string()), "{served:?}");
        assert!(!served.contains(&server.permission_tool_flag_value()));
        // ⚠️ Dotted verbs cannot be MCP tool names — the grammar is `[a-zA-Z0-9_-]` — so the
        // dot becomes `_` and the mapping back is the server's, not a second table's. The read
        // verb has two dots, so it is the sharpest case this rule has.
        assert!(served.iter().all(|n| !n.contains('.')), "{served:?}");
        assert!(
            served.contains(&"mcp__organon__console_camera_read".to_string()),
            "the read is what a conversation tab has that the CLI does not: {served:?}"
        );
        // ✏️ The second read, and the second dotted name flattened by the same rule — which is
        // worth pinning separately because `console.layout` and `console.layout.list` differ by
        // one dot and would collide if the flattening ever dropped a segment instead of
        // replacing it.
        assert!(
            served.contains(&"mcp__organon__console_layout_list".to_string()),
            "the layout listing is served under its flattened name: {served:?}"
        );
        assert!(served.contains(&"mcp__organon__console_layout".to_string()), "{served:?}");
    }

    /// 🚨 **The MCP table is the sidecar table plus exactly the READS, and every extra verb has
    /// to be one.** Both halves matter. If `mcp_specs` ever *dropped* a console verb an agent
    /// would silently lose a capability the CLI still has; if it gained an extra verb that is
    /// **not** answered in-process, that verb would be one `op_from` refuses and
    /// [`ConsoleDispatch`] does not special-case, so every call to it would fail with "no console
    /// op for …" — a tool served and unusable.
    ///
    /// ⚠️ The reads are deliberately **absent** from `console_specs()`: they have no `ConsoleOp`,
    /// no sidecar line and no clap subcommand, because that transport has no return path. See
    /// [`mcp_specs`].
    ///
    /// ✏️ **The list was `[camera.read]` and is now two**, which is the edit this test exists to
    /// force. It is spelled as a literal rather than as a count so the diff names the verb that
    /// arrived; ⚠️ **re-derive it from `mcp_specs()` when it moves, never append and assume the
    /// order** — a list of the whole vocabulary is the thing a merge invalidates without a
    /// conflict, which the two tests below record happening twice.
    ///
    /// ⚠️ `cargo check --profile test` only in this session; CI executes it.
    #[test]
    fn the_mcp_table_is_the_sidecar_table_plus_the_verbs_only_this_process_can_answer() {
        let sidecar: Vec<String> = console_specs().into_iter().map(|s| s.name).collect();
        let served: Vec<String> = mcp_specs().into_iter().map(|s| s.name).collect();

        for name in &sidecar {
            assert!(served.contains(name), "`{name}` is reachable from the CLI but not from MCP");
        }
        let extra: Vec<&String> = served.iter().filter(|n| !sidecar.contains(n)).collect();
        assert_eq!(
            extra,
            [&CMD_CAMERA_READ.to_string(), &CMD_LAYOUT_LIST.to_string()],
            "the extra verbs are the reads, in the order `mcp_specs` pushes them"
        );

        // …and neither read has a sidecar spelling, rather than merely being omitted from the
        // list: `op_from` is what a call would fall through to, and it must refuse.
        for read in [CMD_CAMERA_READ, CMD_LAYOUT_LIST] {
            assert!(
                op_from(read, &json!({})).is_err(),
                "a read must never convert into a line written onto a fire-and-forget channel"
            );
        }
        // 🚨 **The write verb beside the listing is a different verb, and is NOT a read.** The
        // two are one dot apart in the catalog, so a dispatch that fell through to the listing's
        // in-process arm would answer a `layout load` with a directory listing.
        assert!(sidecar.contains(&CMD_LAYOUT.to_string()), "the write verb is on the sidecar");
        assert_eq!(
            op_from(CMD_LAYOUT, &json!({ CMD_ACTION: "load", CMD_NAME: "desk" }))
                .map(|op| cli::console_op_to_line(&op)),
            Ok("layout load desk".to_string())
        );

        // A read takes no arguments at all — the point of a separate verb rather than a
        // zero-argument spelling of `console.camera`, whose axes are all optional and whose
        // empty call therefore already means something else.
        for name in [CMD_CAMERA_READ, CMD_LAYOUT_LIST] {
            let read = mcp_specs()
                .into_iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name} is registered"));
            assert!(read.args.is_empty(), "{name}: a read has nothing to say");
            assert_eq!(read.target, TargetKind::Viewport, "{name}: the pane is the viewport");
            assert_eq!(read.reversal, Reversal::Recoverable, "{name}: a read changes nothing");
        }

        // The empty framing still earns its own message on the write verb — proof the two did
        // not get conflated.
        let e = op_from(CMD_CAMERA, &json!({})).expect_err("a framing that names no axis");
        assert!(e.contains("at least one of"), "{e}");
    }

    /// 🚨 **A capability call becomes the line the CLI would have written — the same op,
    /// onto the same audited channel, with no process spawned.**
    ///
    /// The whole point of Part 1. It pins the two halves of [`ConsoleDispatch`]'s write lane: a
    /// valid call converts to the exact sidecar line (so a tool call and a
    /// `organon console …` line cannot come to mean different things), and an out-of-range
    /// `block` is refused *before* anything is written — the one gate `ArgKind::Int` cannot
    /// express in the generated schema, and therefore the one failure this dispatch reports
    /// to the model itself rather than leaving to the drain.
    ///
    /// ⚠️ **It deliberately does NOT call the dispatch, because calling it would write to the
    /// live sidecar** — `console_cmd_path()` is namespace-derived, not test-scoped, so a
    /// console running beside the test suite would drain the line and open a portal. What is
    /// therefore uncovered here is the append itself, which is `cli::append_console_ops` —
    /// one line, already exercised by the CLI path that has always used it.
    ///
    /// ⚠️ `cargo check --profile test` only in this session; CI executes it.
    #[test]
    fn a_capability_call_becomes_the_sidecar_line_the_cli_would_have_written() {
        let line = |name: &str, args: Value| -> Result<String, String> {
            op_from(name, &args).map(|op| cli::console_op_to_line(&op))
        };
        assert_eq!(line(CMD_PORTAL, json!({ CMD_STATE: "open" })), Ok("portal open".into()));
        assert_eq!(
            line(CMD_BACKGROUND, json!({ CMD_ARG: "graphite" })),
            Ok("background graphite".into())
        );
        assert_eq!(line(CMD_BLOCK, json!({ CMD_ROWS: 12 })), Ok("block 12".into()));

        // Every line this produces must be one the drain can read back, or the tool would
        // write something the console silently skips.
        for spec in console_specs() {
            let args = match spec.name.as_str() {
                CMD_BACKGROUND | CMD_RIG => json!({ CMD_ARG: match spec.args[0].kind {
                    ArgKind::Choice(ref v) => v[0].clone(),
                    _ => panic!("{}: expected a Choice", spec.name),
                } }),
                CMD_BLOCK => json!({ CMD_ROWS: 3 }),
                CMD_PATCH => json!({ CMD_UP: 1, CMD_ROWS: 2, CMD_KIND: kind::KIND_WORDS[0] }),
                CMD_PORTAL => json!({ CMD_STATE: cli::PORTAL_WORDS[0] }),
                // The partial form on purpose: the flagship framing is one axis, and the
                // three nulls are what a partial call actually serializes to.
                CMD_CAMERA => {
                    json!({ CMD_RESET: false, CMD_YAW: null, CMD_PITCH: null, CMD_DISTANCE: 40.0 })
                }
                // 🚨 **A palette NAME, taken from `Theme::NAMES` rather than from the spec's
                // own `Choice`.** That `Choice` also carries `edit`/`adjust`, which open the
                // live editor and are refused on this lane by design (§1.10) — so reaching for
                // `v[0]` the way the two dressing verbs above do would be one reordering away
                // from this test asserting that an editor word writes a sidecar line, which is
                // exactly what must never happen.
                CMD_THEME => json!({ CMD_ARG: Theme::NAMES[0] }),
                CMD_POSTURE => {
                    json!({ CMD_ARG: organon_console::posture::POSTURE_WORDS[0] })
                }
                // `SCREEN_WORDS[0]` the way the two dressing verbs reach for `v[0]`, and safe
                // to do so where `CMD_THEME` is not: this `Choice` carries no word that is
                // legal in the schema and illegal on the lane, because every screen state is
                // reachable from here. If one ever is not, this must stop being an index.
                CMD_SCREEN => {
                    json!({ CMD_STATE: organon_console::screen::SCREEN_WORDS[0] })
                }
                // 🚨 **Named words, not `v[0]` from either `Choice`** — and this is the
                // `CMD_THEME` caveat arriving on a second verb rather than the two dressing
                // verbs' shortcut. `CONTENT_WORDS[2]` is `off`, which is legal in the schema
                // and is a *refusal* on a default console (it would evict the last agent), so
                // indexing would be one reordering away from asserting that a clearing word
                // writes a line the console then refuses. `full agent` is the one pair that is
                // always accepted, because it is the layout the console opens in.
                CMD_VIEWPORT => json!({ CMD_REGION: "full", CMD_CONTENT: "agent" }),
                // 🚨 **Named words for `CMD_VIEWPORT`'s reason, arriving on a third verb.**
                // The panel ring's last entry is `all`, which is legal in the schema and is a
                // *refusal* under `add` (it names the whole column, and this verb does not fill
                // one from a word) — so indexing either `Choice` would be one reordering away
                // from asserting that a refused pair writes a sidecar line. `add surface` is
                // the pair that is always accepted at this gate: Surface is the one panel with
                // a body, and adding never depends on what the column is holding.
                CMD_STACK => json!({ CMD_ACTION: "add", CMD_PANEL: "surface" }),
                // 🚨 **Named words on a fourth verb, and here the name ring has no table to
                // index at all** — `ArgKind::Text` states no value space, which is exactly why
                // `op_from` checks the name itself. `load` is chosen over `delete` for
                // `CMD_VIEWPORT`'s reason: this gate is about the *line being written*, and
                // picking the action that also destroys something would be one edit away from
                // a test that deletes from whatever library the machine running it has.
                CMD_LAYOUT => json!({ CMD_ACTION: "load", CMD_NAME: "desk" }),
                other => panic!("{other}: this test has no arguments for a new verb"),
            };
            let written = line(&spec.name, args).unwrap_or_else(|e| panic!("{}: {e}", spec.name));
            assert!(
                cli::parse_console_op(&written).is_some(),
                "`{}` writes `{written}`, which the drain cannot read back",
                spec.name
            );
        }

        // The row range: a real gate, and the model is told why.
        let refused = line(CMD_BLOCK, json!({ CMD_ROWS: 9000 })).expect_err("out of range");
        assert!(refused.contains("must be 1..="), "{refused}");
    }

    /// 🚨 **The fourth front door: every console verb is typeable as a slash command, and the
    /// word to type is derived from the same table the tool name is derived from.**
    ///
    /// This is the property that makes "one registry" a fact rather than an intention. A verb
    /// added to [`console_specs`] becomes an MCP tool, a sidecar line *and* a slash command
    /// with no edit anywhere — and a verb that somehow existed for the agent and not for the
    /// person sitting in front of the console would fail here.
    ///
    /// ⚠️ The typed lines are **generated from each spec's own arguments**, not listed, for the
    /// reason the round-trip loop above is: a hand-written list stops covering the table the
    /// day the table grows, and does so silently.
    ///
    /// ⚠️ `cargo check --profile test` only in this session; CI executes it.
    #[test]
    fn every_console_verb_is_typeable_as_a_slash_command() {
        use organon_console::registry::{Lane, Registry, Resolved};

        let specs = mcp_specs();
        let registry = Registry::new(&specs);
        assert!(
            registry.collisions().is_empty(),
            "a verb whose word is already held is silently untypeable: {:?}",
            registry.collisions()
        );

        for spec in &specs {
            let entry = registry
                .entries()
                .iter()
                .find(|e| e.name() == spec.name)
                .unwrap_or_else(|| panic!("`{}` is in the catalog but is not typeable", spec.name));
            assert_eq!(entry.lane(), Lane::Console);
            assert_eq!(entry.doc(), spec.doc, "the composer's line is the tool's");
            assert_eq!(entry.args(), spec.args, "one schema, not two");
            // The word is the catalog name minus the product's own namespace — so the read
            // verb, whose name carries two dots, is `/camera.read`.
            assert_eq!(registry.entry(entry.verb()).map(|e| e.name()), Some(spec.name.as_str()));
        }

        // The view lane is present beside it, in its own group, so a menu draws one tree.
        assert_eq!(registry.groups(), ["console", "view"]);
        assert!(registry.entry("surface").is_some(), "the composer keeps its own verb");
        assert!(registry.entry("help").is_some(), "and can say what it answers");

        // Now the mechanical half: a minimal typed line per verb, built from that verb's own
        // schema, resolving to the catalog name and to arguments `op_from` accepts.
        for spec in console_specs() {
            let verb = registry
                .entries()
                .iter()
                .find(|e| e.name() == spec.name)
                .expect("registered above")
                .verb()
                .to_string();
            let mut typed = format!("/{verb}");
            for arg in &spec.args {
                if !arg.required {
                    continue;
                }
                typed.push(' ');
                typed.push_str(&match &arg.kind {
                    ArgKind::Choice(options) => options[0].clone(),
                    // ✏️ **The SHORT form, deliberately.** This loop walks every verb from the
                    // composer through `op_from` to the sidecar line and back out of the drain,
                    // which is exactly the chain an abbreviation has to survive — and typing
                    // the long word here would leave the whole chain untested for the one
                    // argument that has a second spelling. The fallback is the long word, so a
                    // future `ChoiceAliased` that aliases only some of its words still types.
                    ArgKind::ChoiceAliased { words, aliases } => aliases
                        .iter()
                        .find(|(full, _)| *full == words[0])
                        .map_or_else(|| words[0].clone(), |(_, short)| short.clone()),
                    ArgKind::Int => "2".to_string(),
                    ArgKind::Float { min, .. } => format!("{min}"),
                    ArgKind::Bool => "true".to_string(),
                    ArgKind::Text => "x".to_string(),
                });
            }
            // `console.camera` has no required argument at all, and a framing that names
            // nothing is refused by design — so it is typed with the axis a human would.
            if spec.name == CMD_CAMERA {
                typed = format!("/{verb} distance 40");
            }
            let Resolved::Run { lane, name, args } = registry.resolve(&typed) else {
                panic!("`{typed}` must resolve for `{}`", spec.name);
            };
            assert_eq!(lane, Lane::Console);
            assert_eq!(name, spec.name);
            let op = op_from(&name, &args).unwrap_or_else(|e| panic!("{typed}: {e}"));
            let written = cli::console_op_to_line(&op);
            assert!(
                cli::parse_console_op(&written).is_some(),
                "`{typed}` writes `{written}`, which the drain cannot read back"
            );
        }
    }

    /// 🚨 **One verb, four spellings, one [`cli::ConsoleOp`] — the invariant the whole
    /// registry exists to make true.**
    ///
    /// Each row is the same act said three ways this test can reach: as the `ConsoleOp` the
    /// CLI's clap layer builds, as the tool arguments an agent's MCP call carries
    /// ([`op_args`]), and as the line a human types in the composer. The pie menu is the
    /// fourth and is not built — but it produces a `(name, args)` pair out of the same
    /// registry, so it lands in this test the day it exists.
    ///
    /// ⚠️ If these ever diverge, the symptom is not a build failure. It is
    /// `/background slate` doing something subtly different from
    /// `organon console background slate` — two resolvers that can disagree, eventually
    /// disagreeing, which is the failure this tree has recorded from the store path to
    /// `doc-rules.sh` to the three kind registries `kind.rs` collapsed.
    ///
    /// ⚠️ `cargo check --profile test` only in this session; CI executes it.
    #[test]
    fn every_surface_of_a_verb_produces_the_same_console_op() {
        use organon_console::registry::{Registry, Resolved};

        let registry = Registry::new(&mcp_specs());
        let cases: Vec<(&str, cli::ConsoleOp)> = vec![
            ("/background slate", bg("slate")),
            ("/background world", bg("world")),
            ("/rig daylight", rig_op("daylight")),
            ("/block 7", cli::ConsoleOp::Block(7)),
            ("/patch 12 12 panel", cli::ConsoleOp::Patch { up: 12, rows: 12, kind: kind::Kind::Panel }),
            ("/patch 0 7 scene", cli::ConsoleOp::Patch { up: 0, rows: 7, kind: kind::Kind::Scene }),
            ("/portal toggle", cli::ConsoleOp::Portal(cli::PortalCmd::Toggle)),
            (
                "/camera reset",
                cli::ConsoleOp::Camera(cli::CameraFraming { reset: true, ..Default::default() }),
            ),
            (
                "/camera distance 40",
                cli::ConsoleOp::Camera(cli::CameraFraming {
                    distance: Some(40.0),
                    ..Default::default()
                }),
            ),
            (
                "/camera reset yaw -1.2 pitch 0.3 distance 12.5",
                cli::ConsoleOp::Camera(cli::CameraFraming {
                    reset: true,
                    yaw: Some(-1.2),
                    pitch: Some(0.3),
                    distance: Some(12.5),
                }),
            ),
        ];

        for (typed, expected) in cases {
            // The agent's spelling → the op. (The CLI's own spelling *is* `expected`: clap
            // builds a `ConsoleOp` directly, which is why it is the value on the right.)
            assert_eq!(
                op_from(spec_name(&expected), &op_args(&expected)),
                Ok(expected.clone()),
                "the MCP spelling of {expected:?}"
            );
            // The human's spelling → the same op, through the same converter.
            let Resolved::Run { name, args, .. } = registry.resolve(typed) else {
                panic!("`{typed}` must be a command")
            };
            assert_eq!(name, spec_name(&expected), "`{typed}` names its own catalog entry");
            assert_eq!(op_from(&name, &args), Ok(expected.clone()), "the slash spelling of {typed}");
            // 📌 …and the typed line, minus its slash, **is** the sidecar line. That is not a
            // coincidence to be enjoyed: the slash grammar is required-positional then
            // keyword-tagged precisely because that is what `cli::console_op_to_line` already
            // writes, so a human reading `queued: camera reset distance 40` on a terminal
            // knows what to type in the composer without a translation table.
            assert_eq!(
                cli::console_op_to_line(&expected),
                typed.trim_start_matches('/'),
                "the typed line is the sidecar line, verb and all"
            );
        }

        // The read has no op and must not acquire one by being typeable.
        let Resolved::Run { name, args, .. } = registry.resolve("/camera.read") else {
            panic!("the read is typeable")
        };
        assert_eq!(name, CMD_CAMERA_READ);
        assert!(
            op_from(&name, &args).is_err(),
            "a read must never convert into a line written onto a fire-and-forget channel"
        );
    }

    /// 🚨 **The row a person actually sees when they type `/`, pinned against the real
    /// table.**
    ///
    /// The compact command panel is generated from [`Registry::candidates`], so nothing in
    /// the console restates the verb list — which is right, and which also means the thing
    /// James looks at exists nowhere as a string that could be read. It does now, here,
    /// because this is the only module that can see the real catalog.
    ///
    /// ⚠️ **This test is a *witness*, not a specification.** It must be updated whenever a
    /// verb is added — that is the point: a diff to this line is what tells a reviewer the
    /// surface changed, and it is far cheaper than noticing on a running console.
    ///
    /// ⚠️ James's own sketch of the row named eight verbs
    /// (`surface|theme|posture|background|rig|patch|portal|camera`). The panel deliberately
    /// shows the **true** list, which is thirteen: `block`, `camera.read` and `help` are
    /// typeable, so hiding them would be the surface disagreeing with the registry — a
    /// second vocabulary, in the one place that exists to prevent one. `screen` and `organon`
    /// are the twelfth and thirteenth, each earning its place the moment it became typeable —
    /// and they arrived on separate branches, which is what made the hidden count below merge
    /// wrong. Update the number here in the same breath as the assertions.
    ///
    /// ⚠️ `cargo check --tests --features console-edition` only in this session; CI executes
    /// it — the same standing caveat the sibling test above carries.
    #[test]
    fn the_compact_panel_shows_the_real_table() {
        use organon_console::conversation_view::compact_line;
        use organon_console::registry::Registry;

        let registry = Registry::new(&mcp_specs());
        let all = registry.candidates("/").expect("a bare slash opens the whole table");
        assert_eq!(
            compact_line(&all, 0, 200),
            // ✏️ `stack` sits between `viewport` and `block` because `console_specs` declares
            // it there — beside the verb it splits a sentence with, not beside the two verbs
            // it shares a `Reversal` with. The order here is the table's, read out.
            // ✏️ `layout` sits after `stack` and before `block` because `console_specs`
            // declares it there — beside the two verbs whose work it records — and
            // `layout.list` sits after `camera.read` because `mcp_specs` pushes the reads at
            // the end, in the order it pushes them. The order here is the table's, read out.
            // ✏️ `trace` sits between `help` and `media` because `view_entries` declares it
            // there — after the verb it shares a lane-tail with and before the exhibit. Read
            // off the table rather than appended: `Registry::new` lays the console specs down
            // first and extends with `view_entries()`, so the row is `mcp_specs()` in its own
            // order followed by the view lane in its own order, and `trace`'s position in the
            // row is exactly its position in that function.
            "[background] | rig | theme | posture | screen | viewport | stack | layout | block | \
             patch | portal | camera | camera.read | layout.list | surface | help | trace | \
             media | organon"
        );
        // 120 columns, so it fits a full-width pane at any sane text size — and narrows to a
        // count rather than an ellipsis when it does not.
        // ✏️ **139 with `viewport`** (§1.14), which is the fifteenth verb and the fourth to move
        // this line. Re-derived from the string above rather than nudged, on the paragraph
        // below's rule.
        // ✏️ **147 with `stack`** (#98 Tier A), the sixteenth verb and the fifth to move this
        // line. **Re-derived, not nudged**, on the paragraph below's rule: the sixteen words
        // are 102 characters (`background` in brackets counts 12) and the fifteen separators
        // 45 — the word is five letters and its separator three, so the arithmetic and the
        // number agree by construction rather than by my having added eight.
        // ✏️ **170 with `layout` and `layout.list`** (§1.15) — the seventeenth and eighteenth
        // verbs, and the sixth change to this line. **Re-derived, not nudged**, on the paragraph
        // below's rule: the eighteen words are 119 characters (`background` in brackets counts
        // 12) and the seventeen separators 51. The two new words are 6 and 11 and they bring two
        // separators with them, so 147 + 17 + 6 = 170 — the arithmetic and the number agree by
        // construction rather than by my having added twenty-three.
        // ✏️ **178 with `trace`** (#117) — the nineteenth verb and the seventh change to this
        // line. **Re-derived, not nudged**, on the paragraph below's rule: the nineteen words are
        // 124 characters (`background` in brackets counts 12) and the eighteen separators 54. The
        // new word is 5 and it brings one separator, so 170 + 3 + 5 = 178 — the arithmetic and
        // the number agree by construction rather than by my having added eight.
        assert_eq!(compact_line(&all, 0, 200).chars().count(), 178);
        // 🚨 **This line is why the test is a witness rather than a specification, and it very
        // nearly merged wrong.** `screen` and `organon` landed on separate branches, and BOTH
        // changed this from `+9` to `+10` — identically, so git auto-merged it with no conflict
        // to look at, while the combined table is thirteen verbs and the true answer is `+11`.
        // A hidden count is the one assertion here that a merge can silently invalidate: the
        // row above conflicts loudly because both sides edited the same words, and this one
        // does not because both sides happened to write the same number for different reasons.
        // ✏️ **Fourteen verbs now, so `+12`** — `media` (the exhibit, §1.13) is the third verb
        // to move this line, and the count was re-derived rather than nudged: two verbs are
        // shown at this width and `mcp_specs()` yields fourteen, so twelve are hidden. The
        // paragraph above is why that sentence is written out instead of the number simply
        // being incremented.
        // ✏️ **Fifteen verbs now, so `+13`** — `viewport` (§1.14) is the fourth verb to move this
        // line, and the count was re-derived rather than incremented: two verbs are shown at
        // this width and `mcp_specs()` plus the view lane yield fifteen, so thirteen are hidden.
        // ✏️ **Sixteen verbs now, so `+14`** — `stack` (#98 Tier A) is the fifth verb to move
        // this line, and the count was re-derived rather than incremented: two verbs are shown
        // at this width, `mcp_specs()` yields twelve and the view lane four, so fourteen are
        // hidden. The paragraph above is why that sentence is written out instead of the
        // number simply being bumped.
        // ✏️ **Eighteen verbs now, so `+16`** — re-derived rather than incremented, which the
        // paragraph above is the reason for: two verbs are shown at this width, `mcp_specs()`
        // yields fourteen (twelve on the sidecar plus two reads) and the view lane four, so
        // sixteen are hidden.
        // ✏️ **Nineteen verbs now, so `+17`** — re-derived rather than incremented, which the
        // paragraph above is the reason for: `mcp_specs()` yields fourteen (twelve on the sidecar
        // plus two reads) and `view_entries()` five — `surface`, `help`, `trace`, `media`,
        // `organon` — and two are shown at this width, so seventeen are hidden. The width
        // arithmetic is why two is still the answer and not three: three words plus the note is
        // 12 + 3 + 5 + two separators + `" | +16"` = 33 characters against 30.
        assert_eq!(compact_line(&all, 0, 30), "[background] | rig | +17");

        // The value ring of the verb James found offering nothing: `/portal` completes to
        // `/portal ` on its own (one candidate), and that is what opens this.
        let portal = registry.candidates("/portal ").expect("the value ring");
        assert_eq!(compact_line(&portal, 0, 200), "[open] | close | toggle");
        // …and an argument with no closed value space says what it wants instead.
        let block = registry.candidates("/block ").expect("the value ring");
        assert_eq!(compact_line(&block, 0, 200), "rows: a whole number");
    }

    /// 🚨 **Which verbs run without an Enter, in the only place the real table can be seen.**
    ///
    /// The compiler already forces every `CommandSpec` to answer — `Reversal` has no default
    /// on purpose — so what this adds is the *answers*, pinned. A verb moving from one column
    /// to the other is then a deliberate edit to a list somebody reads, rather than a word
    /// changed in a literal three hundred lines away from anything that shows the consequence.
    ///
    /// ⚠️ **It reads the registry rather than `mcp_specs()`, so the view lane is covered too.**
    /// `surface`, `help` and `organon` have no `CommandSpec` at all, and two of the three are
    /// on the waiting side — a check over the specs alone would have missed exactly the verbs
    /// whose classification is least obvious.
    #[test]
    fn the_real_table_says_which_verbs_may_run_without_an_enter() {
        use organon_console::registry::Registry;

        let registry = Registry::new(&mcp_specs());
        let table: Vec<(&str, bool)> = registry
            .entries()
            .iter()
            .map(|e| (e.verb(), e.reversal() == Reversal::Recoverable))
            .collect();
        assert_eq!(
            table,
            [
                // Settings with an inverse, and one read. Wrong is one command away from right.
                ("background", true),
                ("rig", true),
                ("theme", true),
                ("posture", true),
                ("screen", true),
                // ✏️ **`viewport` sits with the settings and not with the two below it**, which
                // is the classification worth stating rather than assuming: a split changes the
                // window dramatically and puts nothing in the transcript, and
                // `viewport full agent` restores the undivided console from any layout in one
                // command. Wrong is one command away from right, which is the whole test.
                ("viewport", true),
                // ✏️ **`stack` sits with the two below it and NOT with `viewport` above it**,
                // which is the classification worth stating rather than assuming, because the
                // two verbs are neighbours and look alike. Nothing lands in the transcript,
                // which is `viewport`'s whole case for the other column — but `stack remove
                // all` discards a column somebody assembled and **no single command rebuilds
                // it**, which is `block`'s case for this one. Wrong is *many* commands away
                // from right, so it waits for an Enter.
                ("stack", false),
                // ✏️ **`layout` sits with `stack` and not with `viewport`**, and each of its
                // three actions earns that separately. `delete` takes a layout out of a file and
                // nothing puts it back; `save` replaces what was stored under a name and nothing
                // rebuilds it. `load` is the one worth arguing: it puts nothing in the
                // transcript, which is `viewport`'s whole case for the other column — but what
                // it *displaces* is the arrangement on screen, and no second command restores
                // that unless it too was saved. `/viewport full agent` returns to the DEFAULT,
                // not to what you had. "Only if you had already saved it" is not yes, so it
                // waits for an Enter.
                ("layout", false),
                // Rows in the transcript, and a rectangle claimed in somebody else's output.
                ("block", false),
                ("patch", false),
                ("portal", true),
                ("camera", true),
                ("camera.read", true),
                // ✏️ **The second read, beside the first** — `mcp_specs` pushes the reads at the
                // end, so this is where the table puts it rather than beside the verb it lists
                // for. A read changes nothing, which is the cleanest case the rule has.
                ("layout.list", true),
                // The view lane. `surface`, `media` and `organon` put an element in the
                // transcript; `help` writes a few log lines and reads a table.
                //
                // ✏️ **`media` is here because this list is the SECOND casualty of the same
                // merge**, and the paragraph at `compact_line`'s `+12` above describes the
                // first. `/media` joined the view lane on the exhibit branch, where `Reversal`
                // did not exist; this whole test arrived on the autorun branch, where `/media`
                // did not. Neither side could be red, git had no conflict to show, and the
                // combination did not compile at all — so this assertion had never once run
                // against a table containing `media`. ⚠️ The lesson is the one that line already
                // teaches: **a list of the whole vocabulary is invalidated by a merge that
                // touches neither end of it.** Re-derive it from `view_entries()` when it moves;
                // do not append to it and assume the order.
                ("surface", false),
                ("help", true),
                // ✏️ **`trace` is Recoverable, and it is the cleanest case on this side of the
                // table.** It changes what is *drawn* from here on and appends no element to the
                // transcript, and its inverse is the other word of a two-word ring — `/trace off`
                // undoes `/trace on` exactly, which is what this column asks. Re-derived from
                // `view_entries()` rather than appended: `trace` is declared between `help` and
                // `media` there, so it is between them here.
                ("trace", true),
                ("media", false),
                ("organon", false),
            ],
            "the reversal column of the console's whole vocabulary"
        );
    }

    /// **The block verb's row range is a gate on both sides of the sidecar, and this is the
    /// console-side half.** `ArgKind::Int` carries no bounds, so unlike a material name the
    /// count is *not* fully checked by the schema — `op_from` is what stands between a
    /// hand-written `block 9000` line and a command that opens a block of 9000 rows. The
    /// CLI-side half (clap's `value_parser` range) is in `bin/ctl.rs`'s tests.
    #[test]
    fn the_block_verb_bounds_its_row_count_where_the_schema_cannot() {
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_BLOCK)
            .expect("console.block is registered");
        assert_eq!(spec.target, TargetKind::Viewport, "a hole in the transcript is the viewport");
        let arg = spec.args.iter().find(|a| a.name == CMD_ROWS).expect("one `rows` argument");
        assert!(arg.required, "the count is not optional");
        assert_eq!(arg.kind, ArgKind::Int, "rows is a number, not a word");

        for rows in [1u16, 12, cli::MAX_BLOCK_ROWS] {
            assert_eq!(
                op_from(CMD_BLOCK, &json!({ CMD_ROWS: rows })),
                Ok(cli::ConsoleOp::Block(rows)),
                "{rows} rows is inside the range"
            );
        }
        for bad in [0i64, -1, i64::from(cli::MAX_BLOCK_ROWS) + 1, 70_000, i64::MAX] {
            let e = op_from(CMD_BLOCK, &json!({ CMD_ROWS: bad })).expect_err("out of range");
            assert!(e.contains(CMD_ROWS), "the message must name the slot: {e}");
        }
        assert!(op_from(CMD_BLOCK, &json!({ CMD_ROWS: "twelve" })).is_err());
        assert!(op_from(CMD_BLOCK, &json!({})).is_err(), "a missing count is not zero");
    }

    /// **The viewport verb's two rings are the region module's own two tables**, quoted rather
    /// than restated — so a tenth region or a third content kind reaches the MCP schema, the
    /// slash palette and the CLI's `--help` in the commit that adds it.
    ///
    /// ⚠️ It also pins the one thing this verb does **not** check at dispatch: whether the
    /// assignment is legal. `full off` passes every gate here and is refused by
    /// [`Console::set_viewport`], because "is there another agent region" is a fact about the
    /// layout the console is holding when the op lands, and this function runs before that.
    #[test]
    fn the_viewport_verbs_rings_are_the_region_tables_and_it_checks_words_not_layouts() {
        use organon_console::region::{CONTENT_WORDS, REGION_ALIASES, REGION_WORDS};
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_VIEWPORT)
            .expect("console.viewport is registered");
        assert_eq!(spec.target, TargetKind::Viewport, "dividing the pane is the viewport");
        assert_eq!(spec.args.len(), 2, "a region and a content, never one fused word");
        let slot = |name: &str| -> ArgKind {
            spec.args.iter().find(|a| a.name == name).expect("the slot").kind.clone()
        };
        // ✏️ **The region slot is a `ChoiceAliased` and the content slot is still a plain
        // `Choice`** — the one asymmetry in this verb, and it is asserted rather than allowed
        // to be inferred: the regions have declared short forms and the content words do not.
        let ring = |name: &str| -> Vec<String> {
            slot(name).choices().unwrap_or_else(|| panic!("{name} has no closed value space")).to_vec()
        };
        assert_eq!(ring(CMD_REGION), REGION_WORDS.to_vec());
        assert_eq!(ring(CMD_CONTENT), CONTENT_WORDS.to_vec());
        match slot(CMD_CONTENT) {
            ArgKind::Choice(_) => {}
            other => panic!("the content ring is {other:?} — it has no short forms to carry"),
        }
        // 🚨 **The ring the schema DISPLAYS is twelve words and no short forms.** That is the
        // whole constraint on this feature: an abbreviation is accepted everywhere and listed
        // nowhere, so a vocabulary with twelve shapes never reads as one with twenty-four.
        match slot(CMD_REGION) {
            ArgKind::ChoiceAliased { words, aliases } => {
                assert_eq!(words, REGION_WORDS.to_vec());
                assert_eq!(
                    aliases,
                    REGION_ALIASES
                        .iter()
                        .map(|(w, a)| ((*w).to_string(), (*a).to_string()))
                        .collect::<Vec<_>>(),
                    "the short forms are quoted from `region`'s table, never restated here"
                );
                for (_, short) in REGION_ALIASES {
                    assert!(!words.contains(&(*short).to_string()), "`{short}` leaked into the ring");
                }
            }
            other => panic!("the region ring is {other:?}, not a ChoiceAliased"),
        }
        // …and the MCP tool's JSON Schema says the same: twelve in the `enum`, the short forms
        // named in the `description` where a model can read them without being told to pick one.
        let schema = organon_console::mcp::input_schema(&spec);
        let region_schema = &schema["properties"][CMD_REGION];
        assert_eq!(
            region_schema["enum"].as_array().expect("an enum").len(),
            REGION_WORDS.len(),
            "the schema enum is the twelve canonical words: {region_schema}"
        );
        for word in REGION_WORDS {
            assert!(region_schema["enum"].as_array().unwrap().iter().any(|v| v == word));
        }
        for (_, short) in REGION_ALIASES {
            assert!(
                !region_schema["enum"].as_array().unwrap().iter().any(|v| v == short),
                "`{short}` is in the schema enum and must not be: {region_schema}"
            );
        }
        let described = region_schema["description"].as_str().expect("a description");
        assert!(described.contains("short form"), "the schema keeps them secret: {described}");
        for a in &spec.args {
            assert!(a.required, "`{}` is not optional — half a command is not a command", a.name);
        }

        // Every pair the schema offers converts, and every one of those lines is one the drain
        // reads back — the cross product, because the failure of a pair that survives one
        // direction only is a command the console skips in silence.
        for r in REGION_WORDS {
            for c in CONTENT_WORDS {
                let op = op_from(CMD_VIEWPORT, &json!({ CMD_REGION: r, CMD_CONTENT: c }))
                    .unwrap_or_else(|e| panic!("`{r} {c}`: {e}"));
                assert_eq!(
                    op,
                    cli::ConsoleOp::Viewport { region: (*r).into(), content: (*c).into() }
                );
                let line = cli::console_op_to_line(&op);
                assert_eq!(cli::parse_console_op(&line), Some(op), "line was {line:?}");
            }
        }
        // 🚨 **Every short form is accepted at THIS door too, and it travels as typed.** The
        // MCP tool is the door the schema under-describes on purpose (the `enum` is twelve
        // words), so it is the one where "advertised" and "accepted" could quietly come apart —
        // and it is checked over the whole cross product for the reason the loop above is: a
        // pair that survives one direction only is a command the console skips in silence.
        for (word, short) in REGION_ALIASES {
            for c in CONTENT_WORDS {
                let op = op_from(CMD_VIEWPORT, &json!({ CMD_REGION: short, CMD_CONTENT: c }))
                    .unwrap_or_else(|e| panic!("`{short} {c}`: {e}"));
                assert_eq!(
                    op,
                    cli::ConsoleOp::Viewport { region: (*short).into(), content: (*c).into() },
                    "`{short}` must reach the console as typed — `region::Region::resolve` is \
                     the one place it becomes `{word}`, so both doors agree on the line"
                );
                let line = cli::console_op_to_line(&op);
                assert_eq!(cli::parse_console_op(&line), Some(op), "line was {line:?}");
            }
        }
        // 🚨 The slots are not interchangeable, and a swapped call is refused by name rather
        // than half-understood — `agent` is not a region and `left` is not a content.
        let e = op_from(CMD_VIEWPORT, &json!({ CMD_REGION: "agent", CMD_CONTENT: "left" }))
            .expect_err("the words are in the wrong slots");
        assert!(e.contains("agent"), "the refusal must quote what was typed: {e}");
        assert!(e.contains("region"), "…and which table it was read against: {e}");
        assert!(op_from(CMD_VIEWPORT, &json!({ CMD_REGION: "middle", CMD_CONTENT: "agent" })).is_err());
        // ✏️ **This line used to read `"3d"`, pinning it as a word the vocabulary did not have.**
        // Tier 2b gives it one, so the assertion moved to `media` — the kind that is still
        // absent (§1.13's placement question owns it). Changed rather than deleted: a table
        // whose refusals are never exercised stops being a closed value space the day somebody
        // adds a word to one of the four renderings and not the others.
        assert!(op_from(CMD_VIEWPORT, &json!({ CMD_REGION: "left", CMD_CONTENT: "media" })).is_err());
        assert!(op_from(CMD_VIEWPORT, &json!({ CMD_REGION: "left" })).is_err(), "no default");
        assert!(op_from(CMD_VIEWPORT, &json!({})).is_err());
    }

    /// **The stack verb's two rings are `panel_stack`'s own two tables**, quoted rather than
    /// restated — so a twenty-sixth Organon panel reaches the MCP schema, the slash palette and
    /// the CLI's `--help` in the commit that adds it.
    ///
    /// ⚠️ It also pins the thing this verb does **not** check at dispatch: whether the column
    /// can honour the command. `remove bloom` passes every gate here and is refused by
    /// [`Console::set_stack`], because "is any region showing a stack" and "is the column
    /// holding this panel" are facts about state at the moment the op *lands*, and this
    /// function runs before that.
    #[test]
    fn the_stack_verbs_rings_are_the_panel_stack_tables_and_it_checks_words_not_columns() {
        use organon_console::panel_stack::{panel_words, ALL_WORD, STACK_ACTIONS};
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_STACK)
            .expect("console.stack is registered");
        assert_eq!(spec.target, TargetKind::Viewport, "what a region draws is the viewport");
        assert_eq!(
            spec.args.len(),
            3,
            "an action and a panel, never one fused word — plus the optional region"
        );
        // ⚠️ **Two closures rather than one, because the three slots are no longer one kind.**
        // `closed` reads any slot with a stated value space; `plain` additionally insists the
        // slot has no short forms. The action and panel rings are `Choice` and must stay so —
        // neither table has declared abbreviations — while the region slot carries `region`'s.
        let closed = |slot: &str| -> Vec<String> {
            spec.args
                .iter()
                .find(|a| a.name == slot)
                .expect("the slot")
                .kind
                .choices()
                .unwrap_or_else(|| panic!("{slot} has no closed value space"))
                .to_vec()
        };
        let plain = |slot: &str| -> Vec<String> {
            match &spec.args.iter().find(|a| a.name == slot).expect("the slot").kind {
                ArgKind::Choice(v) => v.clone(),
                other => panic!("{slot} is {other:?}, not a Choice"),
            }
        };
        assert_eq!(plain(CMD_ACTION), STACK_ACTIONS.to_vec());
        assert_eq!(plain(CMD_PANEL), panel_words());
        // 🚨 **The region ring is `region::REGION_WORDS`, the same table `viewport`'s first ring
        // is built from** — one region vocabulary, not a second one that resembles it. And it
        // carries `REGION_ALIASES` for the same reason: #109 gave every region word its
        // initials at all four front doors, so a slot naming that table while refusing `tl`
        // would be a divergence rather than a narrower offer.
        assert_eq!(closed(CMD_REGION), organon_console::region::REGION_WORDS.to_vec());
        match &spec.args.iter().find(|a| a.name == CMD_REGION).expect("the slot").kind {
            ArgKind::ChoiceAliased { words, aliases } => {
                assert_eq!(
                    aliases,
                    &organon_console::region::REGION_ALIASES
                        .iter()
                        .map(|(w, a)| ((*w).to_string(), (*a).to_string()))
                        .collect::<Vec<_>>(),
                    "the short forms are quoted from `region`'s table, never restated here"
                );
                for (_, short) in organon_console::region::REGION_ALIASES {
                    assert!(
                        !words.contains(&(*short).to_string()),
                        "`{short}` leaked into the ring — accepted everywhere, listed nowhere"
                    );
                }
            }
            other => panic!("the region ring is {other:?}, not a ChoiceAliased"),
        }
        for a in &spec.args {
            let optional = a.name == CMD_REGION;
            assert_eq!(
                a.required, !optional,
                "`{}`: the two words a command is made of are required, and the region — which \
                 three of the four front doors have no way to name — is not",
                a.name
            );
        }
        // ⚠️ **One slot name for one value space.** `panel_stack::REGION_ARG` is what
        // `region_line` puts into the resolved arguments; this is what the schema declares.
        // A second spelling is a comparison that silently stops matching.
        assert_eq!(CMD_REGION, organon_console::panel_stack::REGION_ARG);

        // Every pair the schema offers converts, and every one of those lines is one the drain
        // reads back — `viewport`'s cross product, for its reason. ⚠️ `add all` is the one pair
        // that must NOT convert: `all` names the whole column and this verb does not fill one
        // from a single word.
        for a in STACK_ACTIONS {
            for p in panel_words() {
                let asked = op_from(CMD_STACK, &json!({ CMD_ACTION: a, CMD_PANEL: p }));
                if *a == "add" && p == ALL_WORD {
                    let e = asked.expect_err("`add all` must be refused by name");
                    assert!(e.contains(ALL_WORD), "the refusal quotes the word: {e}");
                    continue;
                }
                let op = asked.unwrap_or_else(|e| panic!("`{a} {p}`: {e}"));
                assert_eq!(
                    op,
                    cli::ConsoleOp::Stack {
                        action: (*a).into(),
                        panel: p.into(),
                        region: None
                    }
                );
                let line = cli::console_op_to_line(&op);
                assert_eq!(cli::parse_console_op(&line), Some(op), "line was {line:?}");
                // …and again with the optional region, which is the spelling a region's own
                // command line produces. It has to survive the same trip, or a panel typed into
                // one column would arrive in whichever one the destination rule picked.
                let asked = op_from(
                    CMD_STACK,
                    &json!({ CMD_ACTION: a, CMD_PANEL: p, CMD_REGION: "topright" }),
                );
                let op = asked.unwrap_or_else(|e| panic!("`{a} {p} region topright`: {e}"));
                assert_eq!(
                    op,
                    cli::ConsoleOp::Stack {
                        action: (*a).into(),
                        panel: p.into(),
                        region: Some("topright".into())
                    }
                );
                let line = cli::console_op_to_line(&op);
                assert_eq!(cli::parse_console_op(&line), Some(op), "line was {line:?}");
            }
        }
        // 🚨 The slots are not interchangeable — `surface` is not an action and `add` is not a
        // panel — and a swapped call is refused by name rather than half-understood.
        let e = op_from(CMD_STACK, &json!({ CMD_ACTION: "surface", CMD_PANEL: "add" }))
            .expect_err("the words are in the wrong slots");
        assert!(e.contains("surface"), "the refusal must quote what was typed: {e}");
        // `clear` was never an action word: the emptying word rides the panel ring. Exercised
        // because it is the obvious guess, and a table whose refusals are never asked for stops
        // being a closed value space the day somebody adds a word to one rendering and not the
        // others.
        assert!(op_from(CMD_STACK, &json!({ CMD_ACTION: "clear", CMD_PANEL: "all" })).is_err());
        assert!(op_from(CMD_STACK, &json!({ CMD_ACTION: "add" })).is_err(), "no default");
        assert!(op_from(CMD_STACK, &json!({})).is_err());
    }

    /// 🚨 **Every slot in the catalog that names a region accepts the short forms — asserted
    /// over the catalog rather than over a remembered count.**
    ///
    /// ⚠️ **This exists because a comment said "the one `ChoiceAliased` in the catalog" and
    /// #98 Tier C made it two.** The number was true when written, went quietly false in a
    /// commit that had no reason to look at it, and nothing would have failed: a second region
    /// slot built as a plain `Choice` refuses `tl` while its neighbour accepts it, which reads
    /// as a typo rather than as a divergence. So the property is pinned instead of the count —
    /// a *third* region slot added tomorrow either carries `REGION_ALIASES` or fails here, and
    /// this test needs no edit either way.
    ///
    /// 📌 The converse is pinned too: a slot carrying region short forms while claiming some
    /// other value space would be the same drift from the other side, so the walk keys on the
    /// slot *name* and checks both directions.
    #[test]
    fn region_slots_all_accept_the_short_forms() {
        use organon_console::region::{REGION_ALIASES, REGION_WORDS};
        let expected: Vec<(String, String)> =
            REGION_ALIASES.iter().map(|(w, a)| ((*w).to_string(), (*a).to_string())).collect();
        let mut seen = 0usize;
        for spec in console_specs() {
            for arg in &spec.args {
                let is_region_slot = arg.name == CMD_REGION;
                match &arg.kind {
                    ArgKind::ChoiceAliased { words, aliases } => {
                        assert!(
                            is_region_slot,
                            "`{}`'s `{}` carries short forms but is not a region slot — either \
                             it is a region under another name (one value space, one name) or \
                             it has invented a second alias table",
                            spec.name, arg.name
                        );
                        seen += 1;
                        assert_eq!(
                            words,
                            &REGION_WORDS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
                            "`{}`'s region ring is not `REGION_WORDS`",
                            spec.name
                        );
                        assert_eq!(
                            aliases, &expected,
                            "`{}`'s short forms are not `REGION_ALIASES`",
                            spec.name
                        );
                        for (_, short) in REGION_ALIASES {
                            assert!(
                                !words.contains(&(*short).to_string()),
                                "`{}` lists `{short}` — a short form is accepted everywhere and \
                                 listed nowhere",
                                spec.name
                            );
                        }
                    }
                    other => assert!(
                        !is_region_slot,
                        "`{}`'s `{}` names a region and is {other:?} — it would refuse `tl` \
                         while every other region slot accepts it",
                        spec.name, arg.name
                    ),
                }
            }
        }
        // ⚠️ Not a count of *how many*, which is the fact that rotted — only that the walk
        // found some. A catalog with no region slot at all would pass every assertion above
        // vacuously, and that is the one way this test could go green while saying nothing.
        assert!(seen >= 2, "expected the catalog to hold region slots; the walk found {seen}");
    }

    /// 🚨 **A panel column's control, resolved against the REAL catalog — the one binding
    /// `organon-console` cannot make for itself.** `region_line`'s own tests build a fixture
    /// whose shapes copy `console_specs()`; this is the only module that can see the real
    /// thing, so it is the only place "`add surface` in a panel column does what
    /// `organon console stack add surface` does" can actually be asserted end to end.
    ///
    /// ⚠️ It walks the whole way — typed line → `region_line::act` → `op_from` → the sidecar
    /// line → `parse_console_op` — because every one of those steps is a place the supplied
    /// region can be dropped, and a dropped region does not fail: it edits a *different*
    /// column, silently.
    ///
    /// ✏️ **The `viewport` half of this test is gone with the feature it covered.** Tier C let a
    /// content word typed in a region assign that region; the control takes `add` and `remove`
    /// only, so `/panel` is now refused here and `console.viewport` is reached from the composer
    /// and the CLI, which have their own coverage.
    #[test]
    fn a_region_line_expands_onto_the_real_console_specs() {
        use organon_console::region::Region;
        use organon_console::region_line::{act, Act, Context};
        use organon_console::registry::Registry;

        let registry = Registry::new(&console_specs());
        let column = Context { region: Region::Left };

        let Act::Run { name, args } = act(&registry, column, "add surface") else {
            panic!("`add surface` in a panel column did not resolve")
        };
        assert_eq!(name, CMD_STACK);
        let op = op_from(&name, &args).expect("the real schema accepts what the line produced");
        assert_eq!(
            op,
            cli::ConsoleOp::Stack {
                action: "add".into(),
                panel: "surface".into(),
                region: Some("left".into()),
            },
            "the region the line was typed in did not reach the op"
        );
        let line = cli::console_op_to_line(&op);
        assert_eq!(line, "stack add surface region left");
        assert_eq!(cli::parse_console_op(&line), Some(op));

        // …and `remove all`, the emptying spelling, all the way through the same path.
        let Act::Run { name, args } = act(&registry, column, "remove all") else {
            panic!("`remove all` did not resolve")
        };
        let op = op_from(&name, &args).expect("the real schema accepts `remove all`");
        assert_eq!(
            op,
            cli::ConsoleOp::Stack {
                action: "remove".into(),
                panel: "all".into(),
                region: Some("left".into()),
            }
        );
    }

    /// 🚨 **The narrowing, asserted against the REAL table.** #98 Tier C made this control a front
    /// door onto the whole catalog; James rejected that scope, so **every** console verb other
    /// than `stack`'s two actions is now refused here — by name, and saying where it does work.
    ///
    /// ⚠️ This is the test that replaced `every_console_verb_still_runs_in_a_region_line`, and it
    /// asserts the *opposite* of it on purpose. It walks the catalog rather than a handful of
    /// verbs somebody remembered, so a verb added later is covered — and it would also catch the
    /// one quiet way the narrowing could go wrong: if `add` or `remove` ever became a catalog
    /// verb in its own right, the line would expand onto `stack` while the catalog meant
    /// something else, and the `assert!(refused)` below would fire on it.
    #[test]
    fn no_console_verb_but_the_stack_actions_runs_in_a_panel_column() {
        use organon_console::panel_stack::STACK_ACTIONS;
        use organon_console::region::Region;
        use organon_console::region_line::{act, Act, Context};
        use organon_console::registry::{Lane, Registry};

        let registry = Registry::new(&console_specs());
        let column = Context { region: Region::Left };
        let mut seen = 0;
        for entry in registry.entries().iter().filter(|e| e.lane() == Lane::Console) {
            let verb = entry.verb();
            assert!(
                !STACK_ACTIONS.contains(&verb),
                "`{verb}` is both a catalog verb and one of this control's two words; the \
                 expansion onto `stack` would now shadow it"
            );
            for line in [verb.to_string(), format!("/{verb}")] {
                let act = act(&registry, column, &line);
                let Act::Refused(message) = act else {
                    panic!("`{line}` is a console verb and still runs in a panel column: {act:?}")
                };
                assert!(message.contains(verb), "the refusal does not name `{verb}`: {message}");
                for action in STACK_ACTIONS {
                    assert!(message.contains(action), "{message}");
                }
            }
            seen += 1;
        }
        // Not a count of how many — only that the walk found some. An empty catalog would pass
        // every assertion above vacuously.
        assert!(seen >= 4, "expected the catalog to hold console verbs; the walk found {seen}");
    }

    /// 🚨 **The layout verb's first ring is `layout.rs`'s own table, and its second ring is not a
    /// table at all** — the first console verb whose arguments are not both closed lists.
    ///
    /// That asymmetry is the thing to pin. A layout's name is whatever a person called it, so
    /// `ArgKind::Text` is honest where a `Choice` would be a lie — and it means `validate_args`
    /// checks *nothing* about the name, which is why `op_from` has to. ⚠️ **The name rule is a
    /// fact about the transport**: the sidecar line is whitespace-delimited, so a name with a
    /// space in it would arrive at the console truncated, having saved or deleted something
    /// nobody named. This is the gate that stops it, and the last assertion measures the
    /// truncation it is stopping.
    ///
    /// ⚠️ What it deliberately does **not** check is whether the layout exists, still resolves,
    /// or fits today's window. All three are state at drain time; `Console::set_layout` is the
    /// one gate for them, exactly as `set_viewport` is for an assignment.
    ///
    /// ⚠️ `cargo check --profile test` only in this session; CI executes it.
    #[test]
    fn the_layout_verbs_rings_are_its_own_table_and_a_name_it_checks_rather_than_lists() {
        use organon_console::layout::{check_name, LAYOUT_ACTIONS, MAX_NAME};
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_LAYOUT)
            .expect("console.layout is registered");
        assert_eq!(spec.target, TargetKind::Viewport, "the arrangement of the pane is the viewport");
        assert_eq!(spec.args.len(), 2, "an action and a name, never one fused word");
        let action = spec.args.iter().find(|a| a.name == CMD_ACTION).expect("the action slot");
        match &action.kind {
            ArgKind::Choice(v) => assert_eq!(v, &LAYOUT_ACTIONS.to_vec()),
            other => panic!("the action ring is {other:?}, not a Choice"),
        }
        let name = spec.args.iter().find(|a| a.name == CMD_NAME).expect("the name slot");
        assert_eq!(name.kind, ArgKind::Text, "a name a person invented has no value space");
        for a in &spec.args {
            assert!(a.required, "`{}` is not optional — see CMD_NAME on why", a.name);
        }
        // 🚨 **The two slots do not collide with the neighbouring verb's.** `action` is shared
        // with `stack` on purpose (one kind of slot, one palette heading) and `layout` is its
        // own, so a call built for one verb cannot validate against the other.
        assert_eq!(CMD_ACTION, "action");
        assert_ne!(CMD_NAME, CMD_ARG, "a layout name is not the `name` that means a material");
        assert_ne!(CMD_NAME, CMD_PANEL);

        // 🚨 **The join the completion ring hangs on, pinned as an alias rather than as an
        // agreement.** `registry::layout_options` is keyed on the catalog name and offered the
        // name slot *by name*, so two literals that merely matched would take the ring away the
        // day either was renamed — and take it away **silently**, since the verb would keep
        // working perfectly with no ring above it. Both are `const … = registry::…` here, so
        // this reads back what the alias already guarantees; it is the sentence, not the check,
        // that a reader needs. `CMD_THEME`'s arrangement, for the same reason.
        assert_eq!(CMD_LAYOUT, organon_console::registry::VERB_LAYOUT);
        assert_eq!(CMD_NAME, organon_console::registry::LAYOUT_NAME_ARG);
        // ⚠️ And the declared kind stays `Text` *because* the ring is chosen by the action word:
        // the MCP schema and `/help` have no action in hand, so the honest declaration is the one
        // that states no value space. Narrowing this to the library would be a lie to the two
        // surfaces that cannot ask. See §1.15.
        assert_eq!(name.kind, ArgKind::Text);

        // Every action the schema offers converts, and every line it writes is one the drain
        // reads back — `stack`'s cross product, for its reason.
        for a in LAYOUT_ACTIONS {
            let op = op_from(CMD_LAYOUT, &json!({ CMD_ACTION: a, CMD_NAME: "desk" }))
                .unwrap_or_else(|e| panic!("`{a} desk`: {e}"));
            assert_eq!(op, cli::ConsoleOp::Layout { action: (*a).into(), name: "desk".into() });
            let line = cli::console_op_to_line(&op);
            assert_eq!(cli::parse_console_op(&line), Some(op), "line was {line:?}");
        }
        // `list` was never an action word — the listing is a verb of its own, because a name is
        // not a thing a listing takes. Exercised because it is the obvious guess.
        assert!(op_from(CMD_LAYOUT, &json!({ CMD_ACTION: "list", CMD_NAME: "desk" })).is_err());
        assert!(op_from(CMD_LAYOUT, &json!({ CMD_ACTION: "save" })).is_err(), "no default");
        assert!(op_from(CMD_LAYOUT, &json!({})).is_err());

        // 🚨 The name gate, and the failure it exists to prevent, measured rather than argued.
        for bad in ["", "two words", "a\tb", &"x".repeat(MAX_NAME + 1)] {
            assert!(check_name(bad).is_err(), "`{bad}` must not be storable");
            let e = op_from(CMD_LAYOUT, &json!({ CMD_ACTION: "save", CMD_NAME: bad }))
                .expect_err("a name that cannot travel must not reach the sidecar");
            assert!(e.starts_with(CMD_LAYOUT), "the refusal names the verb: {e}");
        }
        // …and this is what would happen if it did not: the line is whitespace-delimited, so the
        // console would act on `my` and never see `desk`.
        assert_eq!(
            cli::parse_console_op("layout save my desk"),
            Some(cli::ConsoleOp::Layout { action: "save".into(), name: "my".into() })
        );
    }

    /// A block is not a look, and `console_step` must say so rather than quietly folding it
    /// into one. `None` is what the apply path reads as "changed nothing", so if the routing
    /// in `apply_console` were ever removed the failure would be an ignored command with a
    /// stderr line — not a backdrop that silently changed.
    #[test]
    fn a_block_is_not_a_dressing() {
        let cold = ConsoleLook::default();
        for rows in [1u16, 12, cli::MAX_BLOCK_ROWS] {
            assert_eq!(
                console_step(BackdropSource::Substrate, &cold, &cli::ConsoleOp::Block(rows)),
                None,
                "a block must not touch the source or the look"
            );
        }
        // The same for a claim, of **every** kind — a kind selects a paint, and a paint is not
        // a dressing. The panel kind is the one that could plausibly drift here, since its
        // buttons change the look: they do it by re-entering `apply_console` as a `Background`
        // op, never by the claim itself meaning something.
        for kind in [kind::Kind::Scene, kind::Kind::Panel] {
            assert_eq!(
                console_step(
                    BackdropSource::Substrate,
                    &cold,
                    &cli::ConsoleOp::Patch { up: 12, rows: 12, kind }
                ),
                None,
                "a {kind:?} claim must not touch the source or the look"
            );
        }
    }

    /// **The claim's kind is a `Choice` the schema can state**, unlike its row count — so
    /// unlike the row range, `op_from` is a belt here rather than the only gate. What this
    /// pins is that the schema, the CLI's `--help` and the paint all know the same kinds:
    /// the failure it exists to catch is a kind the catalog offers and the console cannot
    /// resolve, which would dispatch, record a success, and paint nothing.
    #[test]
    fn the_patch_verb_offers_exactly_the_kinds_it_can_resolve() {
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_PATCH)
            .expect("console.patch is registered");
        let arg = spec.args.iter().find(|a| a.name == CMD_KIND).expect("one `kind` argument");
        assert!(arg.required, "a claim states what it is for");
        let ArgKind::Choice(offered) = &arg.kind else {
            panic!("the kind is a closed vocabulary, not a free word: {:?}", arg.kind)
        };
        assert_eq!(
            offered.as_slice(),
            kind::KIND_WORDS,
            "the catalog and the CLI are built from one table"
        );
        for word in offered {
            let kind = kind::Kind::from_word(word)
                .unwrap_or_else(|| panic!("the catalog offers `{word}` and nothing resolves it"));
            assert_eq!(
                op_from(CMD_PATCH, &json!({ CMD_UP: 12, CMD_ROWS: 12, CMD_KIND: word })),
                Ok(cli::ConsoleOp::Patch { up: 12, rows: 12, kind })
            );
        }
        for bad in [json!({ CMD_UP: 12, CMD_ROWS: 12 }), json!({ CMD_UP: 12, CMD_ROWS: 12, CMD_KIND: "hologram" })] {
            let e = op_from(CMD_PATCH, &bad).expect_err("no kind this console can paint");
            assert!(e.contains(CMD_KIND), "the message must name the slot: {e}");
        }
        // 🚨 **The refusal carries the known list**, which is the whole difference between a
        // usable error and a dead end: an agent that named a kind this build cannot draw has
        // no other way to ask what it *can* draw — `organon console` is fire-and-forget and
        // returns nothing. Asserted word by word rather than against a fixed sentence, so a
        // kind added later has to appear here without this test being edited.
        let e = op_from(CMD_PATCH, &json!({ CMD_UP: 12, CMD_ROWS: 12, CMD_KIND: "hologram" }))
            .expect_err("`hologram` is not a kind");
        assert!(e.contains("hologram"), "it quotes back what was asked for: {e}");
        for word in kind::KIND_WORDS {
            assert!(e.contains(word), "the refusal must offer `{word}`: {e}");
        }
    }

    /// The three source words, and the reason they do NOT go through
    /// [`parse_backdrop_source`]: that function's contract is an environment variable whose
    /// historical rule is "anything not `0`/unset is the World", and a typed command that
    /// resolved garbage to a working source would be unable to report a typo.
    #[test]
    fn every_source_word_resolves_and_a_typed_name_is_stricter_than_the_env_var() {
        assert_eq!(
            BACKDROP_SOURCE_WORDS,
            ["world", "off", "substrate"],
            "the other half of this literal is `CONSOLE_SOURCES` in src/bin/ctl.rs"
        );
        let got: Vec<BackdropSource> =
            BACKDROP_SOURCE_WORDS.iter().map(|w| console_source(w).expect(w)).collect();
        assert_eq!(
            got,
            [BackdropSource::World, BackdropSource::Off, BackdropSource::Substrate],
            "the three words must cover the whole value space, one each"
        );
        for bad in ["", "1", "frobnicate", "substrat", "graphite", "studio"] {
            assert_eq!(console_source(bad), None, "{bad:?} is not a source");
        }
        assert_eq!(
            parse_backdrop_source(Some("frobnicate")),
            BackdropSource::World,
            "the ENV rule stays deliberately looser than the typed one"
        );
    }

    /// 🚨 **The engine is asked for at most one frame per console frame, in every state.**
    /// The whole input space, exhaustively — and it is exhaustive on purpose, because the
    /// failure this guards is not a crash. [`SURFACE_RENDERS_PER_FRAME`]'s doc names it: two
    /// renders in one frame double-step `frame_index` and the TAA jitter phase riding on it, so
    /// the two targets trade phases. On a still lit plane that is invisible, which is why the
    /// existing surface path can afford to allow it; on a **moving World** it is visible and
    /// intermittent, the worst kind, and a live portal beside a live backdrop is exactly that.
    ///
    /// The property is bought by the state machine rather than by a check somewhere, which is
    /// what makes it survive: an open portal takes the frame, and the future immersive state
    /// *is* the backdrop rather than a second thing beside it.
    ///
    /// ✏️ **Tier 2b doubled the input space and the proof had to grow with it, not merely keep
    /// passing.** There are now *two* claimants for the one frame — the portal and a region
    /// holding `3d` — so the cross product is 2 × 2 × 3 × 2, and every one of the twenty-four
    /// states is checked. Widening the function while leaving this loop at its old arity is the
    /// exact shape of a proof that keeps reporting green about a space it no longer covers.
    #[test]
    fn the_engine_is_asked_for_at_most_one_frame() {
        for portal_open in [false, true] {
            for region_holds_world in [false, true] {
                for backdrop in
                    [BackdropSource::Off, BackdropSource::World, BackdropSource::Substrate]
                {
                    for patches in [false, true] {
                        let (source, viewport) =
                            engine_plan(portal_open, region_holds_world, backdrop, patches);
                        let renders = usize::from(source != BackdropSource::Off)
                            + usize::from(viewport.is_some());
                        assert!(
                            renders <= 1,
                            "portal_open={portal_open} region={region_holds_world} \
                             backdrop={backdrop:?} patches={patches} asks the engine for \
                             {renders} frames"
                        );
                    }
                }
            }
        }
    }

    /// 🚨 **The precedence between the two viewport presentations, stated as a test rather than
    /// left to whichever branch happens to come first.**
    ///
    /// **The portal wins**, and the argument is §1.2's own rather than a new one: it is
    /// *temporary and dismissable*, so the state where it holds the frame ends with one word
    /// that sits in the same ring as the word that got you there. A region is the persistent
    /// thing a person arranged, and it is still arranged — nothing about the layout is written,
    /// so closing the portal hands the frame straight back.
    ///
    /// Three properties, and the third is the one that would rot quietly:
    ///
    /// 1. with both claimants, the portal renders;
    /// 2. with only a region, the region renders and the backdrop does not — a `3d` region costs
    ///    the backdrop exactly what an open portal costs it, which is the documented price of
    ///    one World;
    /// 3. **neither claimant writes `backdrop_source`**, so removing them both restores whatever
    ///    the backdrop was with no remembered value to get wrong. That is asserted by comparing
    ///    against the pre-viewport answer computed here from the inputs alone.
    #[test]
    fn the_portal_outranks_a_region_viewport_and_neither_disturbs_the_backdrop() {
        for backdrop in [BackdropSource::Off, BackdropSource::World, BackdropSource::Substrate] {
            for patches in [false, true] {
                for region in [false, true] {
                    assert_eq!(
                        engine_plan(true, region, backdrop, patches),
                        (BackdropSource::Off, Some(ViewportTarget::Portal)),
                        "an open portal takes the frame from everything, region={region} \
                         backdrop={backdrop:?}"
                    );
                }
                assert_eq!(
                    engine_plan(false, true, backdrop, patches),
                    (BackdropSource::Off, Some(ViewportTarget::Region)),
                    "with the portal shut the region has it, from {backdrop:?}"
                );
                // The pre-viewport answer, derived from the inputs rather than quoted — so this
                // is a statement about the *rule* and not a second copy of the table.
                let untouched = if backdrop == BackdropSource::Off && patches {
                    BackdropSource::Substrate
                } else {
                    backdrop
                };
                assert_eq!(
                    engine_plan(false, false, backdrop, patches),
                    (untouched, None),
                    "with neither claimant, {backdrop:?} is exactly what it was (patches={patches})"
                );
            }
        }
    }

    /// **The portal takes the frame, and gives it back untouched.** Two halves, and the second
    /// is the one that would rot quietly: `backdrop_source` is never written, so closing the
    /// portal restores whatever the backdrop was with no remembered value to get wrong — a
    /// property that would be lost the day someone "simplified" this by setting the field.
    ///
    /// ⚠️ It also pins the documented cost: while the portal is open a scene patch has no
    /// picture to sample, because the `Off` + `patches_want_image` promotion is precisely what
    /// the portal displaces.
    #[test]
    fn an_open_portal_takes_the_frame_and_closing_it_gives_the_backdrop_back() {
        for backdrop in [BackdropSource::Off, BackdropSource::World, BackdropSource::Substrate] {
            for patches in [false, true] {
                assert_eq!(
                    engine_plan(true, false, backdrop, patches),
                    (BackdropSource::Off, Some(ViewportTarget::Portal)),
                    "an open portal renders and the backdrop does not, from {backdrop:?}"
                );
                // The same inputs with the portal closed are the pre-portal answer exactly.
                let want = if backdrop == BackdropSource::Off && patches {
                    BackdropSource::Substrate
                } else {
                    backdrop
                };
                assert_eq!(
                    engine_plan(false, false, backdrop, patches),
                    (want, None),
                    "closing it restores {backdrop:?} (patches={patches})"
                );
            }
        }
    }

    /// **The portal's vocabulary is one table with three renderings, and this is the
    /// console-side drift guard.** The catalog's `Choice`, `PortalCmd`'s resolver and the
    /// sidecar's line form are independent code paths over `cli::PORTAL_WORDS`; a word the
    /// catalog offered but the resolver refused would validate `Ok` and change nothing, which
    /// is the failure §5.9.25 names ("a hand-written copy is how a CLI comes to accept a word
    /// nothing can act on"). The CLI-side half is in `bin/ctl.rs`'s tests.
    #[test]
    fn the_catalog_and_the_resolver_agree_about_the_portal() {
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_PORTAL)
            .expect("console.portal is registered");
        assert_eq!(spec.target, TargetKind::Viewport, "a window onto the world is the viewport");
        let arg = spec.args.iter().find(|a| a.name == CMD_STATE).expect("one `state` argument");
        assert!(arg.required, "there is no state a portal command silently means");
        let ArgKind::Choice(offered) = &arg.kind else {
            panic!("{:?} is not a Choice — an unknown state would reach the apply", arg.kind);
        };
        assert_eq!(offered.len(), cli::PORTAL_WORDS.len(), "the catalog is the whole table");
        for word in cli::PORTAL_WORDS {
            assert!(offered.iter().any(|o| o == word), "`{word}` is not in the catalog");
            let cmd = cli::PortalCmd::from_word(word)
                .unwrap_or_else(|| panic!("the catalog offers `{word}` and nothing resolves it"));
            assert_eq!(cmd.as_word(), *word, "the word must survive the round trip");
            assert_eq!(
                op_from(CMD_PORTAL, &json!({ CMD_STATE: word })),
                Ok(cli::ConsoleOp::Portal(cmd))
            );
        }
        let e = op_from(CMD_PORTAL, &json!({ CMD_STATE: "ajar" })).expect_err("no such state");
        assert!(e.contains(CMD_STATE), "the message names the slot: {e}");
        assert!(op_from(CMD_PORTAL, &json!({})).is_err(), "a missing state is not a default");
    }

    /// **A portal command is not a look**, so it must fall through `console_step` unchanged —
    /// exactly as a block and a patch do, and for a sharper reason: it *does* change what the
    /// engine draws for the backdrop, but by being consulted at render time rather than by
    /// writing `backdrop_source`. If it ever folded in, closing the portal would restore a
    /// source it had overwritten, which is one remembered value more than the feature needs.
    #[test]
    fn a_portal_command_leaves_the_backdrop_and_its_dressing_exactly_as_it_found_them() {
        let dressed = look(Some("graphite"), Some("daylight"));
        for cmd in [cli::PortalCmd::Open, cli::PortalCmd::Close, cli::PortalCmd::Toggle] {
            for src in [BackdropSource::Off, BackdropSource::World, BackdropSource::Substrate] {
                assert_eq!(
                    console_step(src, &dressed, &cli::ConsoleOp::Portal(cmd)),
                    None,
                    "`portal {}` at {src:?} must fold into no look at all",
                    cmd.as_word()
                );
            }
        }
    }

    /// 🚨 **The camera schema's bands are the ones the HAND is clamped to** — read from
    /// `scene_input` rather than restated, so a limit that moves for a drag moves for a typed
    /// command in the same commit. A second copy here is how an agent comes to be refused a
    /// viewpoint the drag can reach, and that reads as the camera being broken rather than as
    /// two constants disagreeing.
    #[test]
    fn the_camera_schema_declares_the_same_bands_the_hand_is_clamped_to() {
        let spec = console_specs()
            .into_iter()
            .find(|s| s.name == CMD_CAMERA)
            .expect("console.camera is registered");
        assert_eq!(spec.target, TargetKind::Viewport, "where the viewer stands is the viewport");
        assert_eq!(spec.args.len(), cli::CAMERA_WORDS.len(), "one slot per word in the table");
        for word in cli::CAMERA_WORDS {
            assert!(
                spec.args.iter().any(|a| a.name == *word),
                "`{word}` is in the CLI's table and has no slot in the schema"
            );
        }
        for arg in &spec.args {
            assert!(
                !arg.required,
                "`{}` must be optional — framing one axis must not oblige the other two",
                arg.name
            );
        }
        let band = |name: &str| match &spec.args.iter().find(|a| a.name == name).unwrap().kind {
            ArgKind::Float { min, max } => (*min, *max),
            other => panic!("`{name}` is {other:?}, so its range is not stated at all"),
        };
        assert_eq!(
            band(CMD_YAW),
            (-f64::from(scene_input::YAW_LIMIT), f64::from(scene_input::YAW_LIMIT))
        );
        assert_eq!(
            band(CMD_PITCH),
            (-f64::from(scene_input::PITCH_LIMIT), f64::from(scene_input::PITCH_LIMIT))
        );
        assert_eq!(
            band(CMD_DISTANCE),
            (f64::from(scene_input::DISTANCE_MIN), f64::from(scene_input::DISTANCE_MAX))
        );
        assert!(
            matches!(
                spec.args.iter().find(|a| a.name == CMD_RESET).unwrap().kind,
                ArgKind::Bool
            ),
            "reset is a flag, not an axis with a value"
        );
        // The default framing `--reset` restores must sit inside the bands the schema states,
        // or reset would be the one command the lane refuses.
        for (v, (lo, hi)) in [
            (scene_input::DEFAULT_YAW, band(CMD_YAW)),
            (scene_input::DEFAULT_PITCH, band(CMD_PITCH)),
            (scene_input::DEFAULT_DISTANCE, band(CMD_DISTANCE)),
        ] {
            assert!((lo..=hi).contains(&f64::from(v)), "the default {v} is outside {lo}..{hi}");
        }
    }

    /// The one thing `ArgSpec::required` cannot say — **at least one of four** — is
    /// [`op_from`]'s to say, and it must say it: a framing that names nothing would otherwise
    /// dispatch, succeed, and move nothing, which is the shape of a bug nobody can see.
    #[test]
    fn a_camera_command_that_names_no_axis_is_refused_by_the_resolver() {
        let e = op_from(CMD_CAMERA, &json!({})).expect_err("no axis at all");
        assert!(e.contains("at least one"), "the message says what is missing: {e}");
        let e = op_from(CMD_CAMERA, &json!({ CMD_RESET: false })).expect_err("a false flag");
        assert!(e.contains("at least one"), "{e}");
        assert_eq!(
            op_from(CMD_CAMERA, &json!({ CMD_RESET: true })),
            Ok(cli::ConsoleOp::Camera(cli::CameraFraming { reset: true, ..Default::default() })),
            "reset alone is a whole command"
        );
        assert_eq!(
            op_from(CMD_CAMERA, &json!({ CMD_DISTANCE: 40.0 })),
            Ok(cli::ConsoleOp::Camera(cli::CameraFraming {
                distance: Some(40.0),
                ..Default::default()
            }))
        );
        // The belt under `validate_args`' brace: a NaN passes `as_f64` and would poison the
        // view matrix, and `f64 as f32` can round a just-legal value out of the band.
        assert!(op_from(CMD_CAMERA, &json!({ CMD_YAW: f64::NAN })).is_err(), "NaN");
        assert!(op_from(CMD_CAMERA, &json!({ CMD_DISTANCE: 9000.0 })).is_err(), "out of band");
        assert!(op_from(CMD_CAMERA, &json!({ CMD_YAW: "sideways" })).is_err(), "not a number");
    }

    /// 🚨 **The whole lane, for the framing the PR was written for: `--distance 40`.**
    ///
    /// Every other camera test here calls [`op_from`] or [`op_args`] directly, and that is
    /// exactly how a blocker reached review — the two halves each behaved correctly and the
    /// thing between them did not. `CommandService::dispatch` validates against
    /// [`console_specs`] *before* `ConsoleTarget::execute` ever calls `op_from`, so a partial
    /// framing was refused by `validate_args` for a slot the caller had deliberately left
    /// empty, and no test that skips the service could see it. This one is wired the way
    /// [`Console::dispatch_console`] is wired: the real specs, the real target, the real log.
    ///
    /// ⚠️ **The bug it pins is a `null`, not a missing key**, so the arguments have to come
    /// from [`op_args`] rather than be spelled here — `op_args` is what puts the whole slot
    /// list in the record, and hand-writing a tidier object would test a call this console
    /// never makes.
    ///
    /// ⚠️ `cargo check --profile test` only in this session; CI executes it. The pure-crate
    /// half — `an_optional_arg_present_as_null_is_absent_and_a_required_one_is_missing` in
    /// `organon-console/src/command.rs` — is the one that can be run on this machine, and it is
    /// where the fix itself is pinned.
    #[test]
    fn a_partial_framing_survives_the_real_dispatch_and_reaches_the_target() {
        let root = std::env::temp_dir()
            .join(format!("organon-console-camera-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut log = SessionLog::open(&root, "s1").unwrap();

        let target = ConsoleTarget::default();
        let bank = target.accepted.clone();
        let mut service = CommandService::new(&mut log);
        for spec in console_specs() {
            service.register_spec(spec);
        }
        service.register_target(TargetKind::Viewport, Box::new(target));

        // Each of these names a strict subset of the four slots, so each serializes with at
        // least one `null` — the shape that was refused. `reset` alone is the sharpest: three
        // nulls and a flag.
        let framings = [
            cli::CameraFraming { distance: Some(40.0), ..Default::default() },
            cli::CameraFraming { reset: true, ..Default::default() },
            cli::CameraFraming { yaw: Some(-1.2), ..Default::default() },
            cli::CameraFraming { pitch: Some(0.3), distance: Some(12.5), ..Default::default() },
            cli::CameraFraming { reset: true, distance: Some(40.0), ..Default::default() },
        ];
        for f in framings {
            let op = cli::ConsoleOp::Camera(f);
            let args = op_args(&op);
            assert!(
                cli::CAMERA_WORDS.iter().any(|w| args[*w].is_null()),
                "{args} names every slot, so it is not the partial case this test is about"
            );
            if let Err(e) = service.dispatch(Issuer::Worker("organon-cli".into()), spec_name(&op), args) {
                panic!("`{}` was refused before the target: {e}", cli::console_op_to_line(&op));
            }
        }
        // The ops the *service* handed on, not a parallel copy — the same read
        // `dispatch_console` makes one line after this.
        assert_eq!(
            bank.borrow().len(),
            framings.len(),
            "every framing reached ConsoleTarget::execute"
        );
        assert_eq!(
            bank.borrow()[0],
            cli::ConsoleOp::Camera(cli::CameraFraming {
                distance: Some(40.0),
                ..Default::default()
            }),
            "the axes nobody named must still be None on the far side"
        );

        // The band is still a gate — a null in a sibling slot is absence, never a bypass.
        let out_of_band = json!({ CMD_RESET: false, CMD_YAW: null, CMD_PITCH: null,
                                  CMD_DISTANCE: f64::from(scene_input::DISTANCE_MAX) + 1.0 });
        let err = service
            .dispatch(Issuer::Worker("organon-cli".into()), CMD_CAMERA, out_of_band)
            .expect_err("out of band");
        assert!(matches!(err, CommandError::InvalidArgs { .. }), "{err:?}");

        // And the rule the schema cannot state still stops an empty framing, at the target.
        let empty = op_args(&cli::ConsoleOp::Camera(cli::CameraFraming::default()));
        let err = service
            .dispatch(Issuer::Worker("organon-cli".into()), CMD_CAMERA, empty)
            .expect_err("a framing that names no axis");
        assert!(matches!(err, CommandError::Execution { .. }), "{err:?}");
        assert_eq!(bank.borrow().len(), framings.len(), "neither refusal banked an op");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// **A camera move is not a look**, so it falls through `console_step` untouched — a
    /// sharper case than the portal's: it writes host state on the `World`, which travels in no
    /// snapshot and is saved in no preset, so there is no `(source, look)` it could fold into
    /// even in principle.
    #[test]
    fn a_camera_command_leaves_the_backdrop_and_its_dressing_exactly_as_it_found_them() {
        let dressed = look(Some("graphite"), Some("daylight"));
        let framings = [
            cli::CameraFraming { reset: true, ..Default::default() },
            cli::CameraFraming { distance: Some(40.0), ..Default::default() },
            cli::CameraFraming { reset: false, yaw: Some(0.2), pitch: Some(0.1), distance: Some(9.0) },
        ];
        for f in framings {
            for src in [BackdropSource::Off, BackdropSource::World, BackdropSource::Substrate] {
                assert_eq!(
                    console_step(src, &dressed, &cli::ConsoleOp::Camera(f)),
                    None,
                    "a camera move at {src:?} must fold into no look at all"
                );
            }
        }
    }

    /// Catalog name ↔ sidecar op, both directions. The service hands back the op it
    /// validated, so a mismatch here applies a command nobody issued.
    #[test]
    fn every_op_round_trips_through_its_catalog_name() {
        for op in [
            bg("slate"),
            bg("world"),
            bg("off"),
            rig_op("daylight"),
            cli::ConsoleOp::Block(7),
            cli::ConsoleOp::Patch { up: 0, rows: 7, kind: kind::Kind::Scene },
            cli::ConsoleOp::Patch { up: 12, rows: 12, kind: kind::Kind::Panel },
            cli::ConsoleOp::Portal(cli::PortalCmd::Open),
            cli::ConsoleOp::Portal(cli::PortalCmd::Close),
            cli::ConsoleOp::Portal(cli::PortalCmd::Toggle),
            // The first op on this lane carrying two words, so the round trip has a way to be
            // wrong the others do not: swapping the slots. Both orders of meaning are covered —
            // a region word in the region slot and a content word in the content slot — and the
            // clearing word rides along, since `off` is the only way back from a split.
            cli::ConsoleOp::Viewport { region: "full".into(), content: "agent".into() },
            cli::ConsoleOp::Viewport { region: "topleft".into(), content: "panel".into() },
            cli::ConsoleOp::Viewport { region: "right".into(), content: "off".into() },
            // The second two-word op, with the same way to be wrong plus one of its own: the
            // emptying word rides the *panel* slot, so `remove all` has to survive the trip or
            // a column becomes unclearable.
            cli::ConsoleOp::Stack {
                action: "add".into(),
                panel: "surface".into(),
                region: None,
            },
            cli::ConsoleOp::Stack {
                action: "remove".into(),
                panel: "bloom".into(),
                region: None,
            },
            cli::ConsoleOp::Stack { action: "remove".into(), panel: "all".into(), region: None },
            // …and with the optional region, which is a third way to be wrong: a slot that is
            // present in one direction and absent in the other reads as "no region named" and
            // edits a column the caller did not mean.
            cli::ConsoleOp::Stack {
                action: "add".into(),
                panel: "surface".into(),
                region: Some("right".into()),
            },
            cli::ConsoleOp::Stack {
                action: "remove".into(),
                panel: "all".into(),
                region: Some("bottomleft".into()),
            },
            // The third two-word op. Its way to be wrong is its own: the second word is a NAME
            // rather than a table entry, so nothing downstream would notice the slots being
            // swapped by their contents — only the round trip would.
            cli::ConsoleOp::Layout { action: "save".into(), name: "desk".into() },
            cli::ConsoleOp::Layout { action: "load".into(), name: "desk".into() },
            cli::ConsoleOp::Layout { action: "delete".into(), name: "james.two-up_1".into() },
            cli::ConsoleOp::Camera(cli::CameraFraming { reset: true, ..Default::default() }),
            cli::ConsoleOp::Camera(cli::CameraFraming {
                distance: Some(40.0),
                ..Default::default()
            }),
            cli::ConsoleOp::Camera(cli::CameraFraming {
                reset: false,
                yaw: Some(-1.2),
                pitch: Some(0.3),
                distance: Some(12.5),
            }),
        ] {
            assert_eq!(op_from(spec_name(&op), &op_args(&op)), Ok(op.clone()), "{op:?}");
        }
        // A catalog name this console does not implement produces no op — the belt under
        // `CommandService`'s own unknown-command brace.
        assert!(op_from("session.note", &json!({ CMD_ARG: "x" })).is_err());
        assert!(op_from("console.scrim", &json!({ CMD_ARG: "0.5" })).is_err());
    }

    /// `--help` advertises exactly what the drain resolves — quoted from the tables, so it
    /// cannot offer a material this build has no way to draw. Same discipline as the scrim
    /// line, for the same reason.
    #[test]
    fn help_names_the_console_verbs_and_every_name_they_take() {
        let h = help_text();
        assert!(h.contains("organon console background"), "help omits the background verb");
        assert!(h.contains("organon console rig"), "help omits the rig verb");
        for name in substrate_materials::MATERIAL_NAMES
            .iter()
            .chain(substrate_materials::RIG_NAMES.iter())
            .chain(BACKDROP_SOURCE_WORDS.iter())
        {
            assert!(h.contains(name), "help does not offer `{name}`");
        }
    }

    // -------------------------------------------------------------------------
    // The look history (#4 Tier 4)
    // -------------------------------------------------------------------------

    /// **What the epoch ledger compares.** Three properties, each of which silently merges
    /// two different pictures into one epoch if it fails.
    #[test]
    fn the_ledger_look_names_the_source_and_never_collides_with_a_material() {
        let cold = ConsoleLook::default();
        assert_eq!(ledger_look(BackdropSource::Off, &cold), Look::new("off", UNNAMED_RIG));
        assert_eq!(ledger_look(BackdropSource::World, &cold), Look::new("world", UNNAMED_RIG));
        assert_eq!(
            ledger_look(BackdropSource::Substrate, &cold),
            Look::new(BACKDROP_SUBSTRATE, UNNAMED_RIG),
            "an undressed plane is not `slate`; the source word is what the user typed"
        );
        assert_eq!(
            ledger_look(BackdropSource::Substrate, &look(Some("graphite"), Some("daylight"))),
            Look::new("graphite", "daylight")
        );

        // 1. A source word can never be read as a material.
        for w in BACKDROP_SOURCE_WORDS {
            assert!(
                !substrate_materials::MATERIAL_NAMES.contains(&w),
                "`{w}` is both a source and a material — two looks would share one epoch"
            );
        }
        // 2. The detour through `world` is visible even though the `ConsoleLook` is not: the
        //    console remembers the material across it, so the pair alone cannot tell.
        let dressed = look(Some("graphite"), None);
        assert_ne!(
            ledger_look(BackdropSource::World, &dressed),
            ledger_look(BackdropSource::Substrate, &dressed),
            "`background world` then `background graphite` must open a new epoch"
        );
        // 3. An unnamed rig is not a secret third state — it is `studio`, byte for byte
        //    (`an_unnamed_rig_is_studio`), so `rig studio` must not churn the ledger.
        assert_eq!(
            ledger_look(BackdropSource::Substrate, &look(Some("metal"), None)),
            ledger_look(BackdropSource::Substrate, &look(Some("metal"), Some(UNNAMED_RIG)))
        );
    }

    /// **The Tier 4 beat, as a state machine.** Three `background` changes in one session and
    /// then `background world`, driven through the real chain — `console_step` →
    /// [`ledger_look`] → [`EpochLedger`] over `scroll_anchor`'s own boundary arithmetic. This
    /// is the only place all three modules run together without a window.
    #[test]
    fn the_beat_opens_one_epoch_per_change_and_collapses_at_world() {
        let mut source = BackdropSource::Substrate;
        let mut dressing = ConsoleLook::default();
        let mut ledger = EpochLedger::new(ledger_look(source, &dressing), 0);
        assert_eq!(ledger.current_look(), &Look::new(BACKDROP_SUBSTRATE, UNNAMED_RIG));

        let mut history = 0usize; // lines that have scrolled into scrollback between changes
        let mut opened_at: Vec<u64> = Vec::new();
        for (i, name) in ["graphite", "paper", "metal"].iter().enumerate() {
            history += 40;
            let state = organon_console::scroll_anchor::ViewState {
                rows: 24,
                display_offset: 0,
                history,
                dropped: 0,
                alt_screen: false,
            };
            let (s, l) = console_step(source, &dressing, &bg(name)).expect("a known material");
            source = s;
            dressing = l;
            let at = organon_console::scroll_anchor::boundary_now(state, 5);
            let out = ledger.open(ledger_look(source, &dressing), at);
            assert!(out.opened, "`background {name}` must close the epoch before it");
            assert_eq!(out.boundary, at, "and open exactly where the cursor is");
            assert_eq!(out.evicted, None, "three changes cannot reach the cap");
            assert_eq!(ledger.epoch_count(), i + 2);
            opened_at.push(at);
        }
        assert!(opened_at.windows(2).all(|w| w[1] > w[0]), "epochs open forward: {opened_at:?}");
        assert_eq!(
            ledger.epochs().iter().map(|e| e.look.material.as_str()).collect::<Vec<_>>(),
            vec![BACKDROP_SUBSTRATE, "graphite", "paper", "metal"],
            "oldest first, and the launch look is still the oldest"
        );

        // Ask again for what is already on screen: no epoch, no picture, no churn.
        let repeat = ledger.open(ledger_look(source, &dressing), 9_999);
        assert!(!repeat.opened);
        assert_eq!(ledger.epoch_count(), 4);

        // `background world` is not a fifth look, it is a collapse — and every epoch it
        // forgets comes back with the line to print.
        let (s, l) = console_step(source, &dressing, &bg("world")).unwrap();
        let evicted = ledger.collapse_to(ledger_look(s, &l));
        assert_eq!(evicted.len(), 4, "every prior epoch goes");
        assert_eq!(ledger.epoch_count(), 1);
        assert_eq!(ledger.current_look(), &Look::new("world", UNNAMED_RIG));
        for ev in &evicted {
            assert!(ev.log_line().starts_with("[epochs] evicted"), "{}", ev.log_line());
            assert!(ev.log_line().ends_with("(collapsed)"), "a collapse must not blame the cap");
        }
    }

    /// **The seam between the two leaves, row by row.** [`Console::band_table`] hands
    /// `scroll_anchor` a boundary list with the ledger's first entry dropped; get that wrong
    /// and every row lands one epoch younger — uniformly, silently, and with the picture
    /// still moving, so it looks plausible. Asserted per visible row against the ledger's own
    /// `band_for_line`, which is the independent answer.
    #[test]
    fn every_row_paints_the_look_it_was_written_under() {
        let mut ledger = EpochLedger::new(Look::new("graphite", UNNAMED_RIG), 0);
        assert!(ledger.open(Look::new("paper", UNNAMED_RIG), 105).opened);
        assert!(ledger.open(Look::new("metal", UNNAMED_RIG), 112).opened);

        let ids: Vec<EpochId> = ledger.epochs().iter().map(|e| e.id).collect();
        let live = egui::TextureId::User(99);
        // The two closed epochs were snapshotted when they closed; the live one is the
        // backdrop being rendered this frame and holds no cache entry at all.
        let cached: HashMap<EpochId, egui::TextureId> = ids[..2]
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, egui::TextureId::User(i as u64)))
            .collect();

        let (boundaries, textures) =
            Console::band_table(&ledger, Some(live), |id| cached.get(&id).copied());
        assert_eq!(
            boundaries,
            vec![105, 112],
            "the ledger records where the OLDEST epoch opened too; that is not a look CHANGE"
        );
        assert_eq!(textures.len(), boundaries.len() + 1, "the length law bands are indexed by");
        assert_eq!(textures.last().copied().flatten(), Some(live), "the live look is last");

        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        let cell_h = 10.0;
        let state = organon_console::scroll_anchor::ViewState {
            rows: 20,
            display_offset: 0,
            history: 100,
            dropped: 0,
            alt_screen: false,
        };
        let quads = term_view::band_quads(&boundaries, &textures, state, rect, cell_h);
        assert_eq!(quads.len(), 3, "two changes inside the viewport, three bands");

        for v in 0..state.rows {
            let line = state.first_line() + u64::from(v);
            let band = ledger.band_for_line(line).expect("inside the ledger's range");
            let want = if band + 1 == ledger.epoch_count() {
                Some(live)
            } else {
                cached.get(&ids[band]).copied()
            };
            let mid = rect.top() + f32::from(v) * cell_h + cell_h * 0.5;
            let quad = quads
                .iter()
                .find(|q| q.rect.top() <= mid && mid < q.rect.bottom())
                .expect("every row is covered by exactly one band");
            assert_eq!(quad.texture, want, "row {v} (absolute line {line})");
        }
    }

    /// **A closed epoch's picture is the pane at its PHYSICAL size, or history is magnified.**
    ///
    /// [`Console::snapshot_live_backdrop`] copies the live backdrop at `Backdrop::size`, so the
    /// epoch's resolution is whatever [`Console::render_backdrop`] decided — and that decision is
    /// the one the first beat check caught. Sized `pane_points × remembered_scale`, the
    /// backdrop is sized in **points** for as long as the scale is still the value that stood
    /// in for "egui has not reported one yet"; the live texture rebinds a frame later, the
    /// snapshot never does, and every band painted from it is magnified for the session. This
    /// pins the console's own geometry through the decision, at the ratio it was measured at.
    ///
    /// [`scene_input::pane_pixels_in`]'s tests own the general property; this one owns the
    /// numbers — the window `resumed` asks for, the 30-point strip `redraw` declares, and the
    /// 2.25 the display reports.
    #[test]
    fn a_closed_epoch_is_the_pane_at_its_physical_size() {
        // The console's own shape: `Window::default_attributes().with_inner_size(1100×720)`
        // logical, a `TopBottomPanel::top(…).exact_height(30.0)`, on a 225 % display.
        let (window_points, ppp) = ((1100.0f32, 720.0f32), 2.25f32);
        let pane_points = (window_points.0, window_points.1 - 30.0);
        let swapchain =
            ((window_points.0 * ppp).round() as u32, (window_points.1 * ppp).round() as u32);
        assert_eq!(swapchain, (2475, 1620));

        // `snapshot_live_backdrop` copies `Backdrop::size` verbatim, so an epoch's texture IS
        // whatever this returns — which is why the epoch invariant is asserted here.
        let live = scene_input::pane_pixels_in(swapchain, pane_points, window_points);
        assert_eq!(live, (2475, 1553), "the backdrop, and so the epoch, is the pane in PIXELS");
        // And what it must never be: the pane measured in points, which is what a scale
        // standing in at 1.0 produces and what a band then magnifies 2.25× back up.
        assert_eq!(scene_input::pane_pixels(pane_points, 1.0), (1100, 690));
        assert_ne!(live, (1100, 690), "an epoch sized in points is the Tier 4 band blur");
    }

    /// A pane that has no picture of a look — the backdrop was `off` while those rows were
    /// written — must report `None` for it rather than borrowing the neighbouring look's
    /// texture. The band then paints nothing, which is what `off` looked like.
    #[test]
    fn an_epoch_with_no_picture_reports_no_texture() {
        let mut ledger = EpochLedger::new(Look::new("off", UNNAMED_RIG), 0);
        assert!(ledger.open(Look::new("slate", UNNAMED_RIG), 60).opened);
        let (boundaries, textures) =
            Console::band_table(&ledger, Some(egui::TextureId::User(1)), |_| None);
        assert_eq!(boundaries, vec![60]);
        assert_eq!(textures, vec![None, Some(egui::TextureId::User(1))]);
    }

    /// The help text has to name the environment variables, because they ARE the interface —
    /// a help screen that omitted them would be true and useless.
    ///
    /// ⚠️ **This array is an allow-list, so it catches a REMOVAL and not an ADDITION.** A
    /// variable deleted from `help_text` fails here; a brand-new flag that is added to
    /// neither stays invisible. `ORGANON_SHELL_PTY_DEBUG` is how that was found — it
    /// shipped on its own branch, green, while the help it is meant to be documented by
    /// said nothing about it. Add a flag to `help_text` and to this array together.
    #[test]
    fn help_names_every_documented_environment_variable() {
        let h = help_text();
        for var in [
            "ORGANON_SHELL_BACKDROP",
            "ORGANON_SHELL_SCRIM",
            "ORGANON_SHELL_TABS",
            "ORGANON_SHELL_DEFAULT",
            "ORGANON_SHELL_CMD",
            "ORGANON_SHELL_PTY_DEBUG",
            "ORGANON_CLAUDE_BIN",
            "ORGANON_IPC_NS",
        ] {
            assert!(h.contains(var), "help does not mention {var}");
        }
    }

    fn key(pane: usize, element: u64) -> SurfaceKey {
        (pane, ElementId(element))
    }

    /// The cap, and the order it bites in. Nothing is evicted while the set fits, and what
    /// goes first is what has been unwanted the longest — never what is on screen right now.
    /// 🚨 **Documents are evicted too, and by weight** — the gap #86's review found. A budget
    /// that only counted pictures let a document live for the rest of the session behind a card
    /// nobody could see any more.
    #[test]
    fn the_document_budget_drops_the_oldest_until_what_is_left_fits() {
        // Three documents, 100 bytes each, in a 250-byte budget: exactly one must go, and it
        // must be the oldest.
        let held = [(key(0, 1), 10, 100), (key(0, 2), 30, 100), (key(0, 3), 20, 100)];
        assert_eq!(documents_to_evict(&held, &[], 250), vec![key(0, 1)]);
        // …and the loop stops as soon as it fits rather than draining to the floor.
        assert_eq!(documents_to_evict(&held, &[], 150), vec![key(0, 1), key(0, 3)]);
    }

    /// Everything fitting is the ordinary case and must cost nothing — an empty answer, not a
    /// sorted list nobody uses.
    #[test]
    fn the_document_budget_evicts_nothing_when_it_already_fits() {
        let held = [(key(0, 1), 10, 100), (key(0, 2), 20, 100)];
        assert!(documents_to_evict(&held, &[], 4096).is_empty());
        let none: [(SurfaceKey, u64, usize); 0] = [];
        assert!(documents_to_evict(&none, &[], 0).is_empty(), "nothing held, nothing to drop");
    }

    /// 🚨 **One oversized document does not evict the whole ledger.** The weighing loop stops on
    /// the running total, so a single 10 MB file over a 1 MB budget goes alone — a count-based
    /// cap would have taken its small, freshly-read neighbours with it.
    #[test]
    fn one_huge_document_goes_alone() {
        let held = [(key(0, 1), 10, 10_000_000), (key(0, 2), 20, 1_000), (key(0, 3), 30, 1_000)];
        assert_eq!(documents_to_evict(&held, &[], 1_000_000), vec![key(0, 1)]);
    }

    /// The tie-break is `surfaces_to_evict`'s: among documents last requested on the same
    /// frame, the one **furthest down** this frame's list goes first, so the top of the page —
    /// what the reader scrolled to — survives.
    #[test]
    fn the_document_tie_break_keeps_the_top_of_the_page() {
        let held = [(key(0, 1), 10, 100), (key(0, 2), 10, 100)];
        let wanted = [key(0, 1), key(0, 2)];
        assert_eq!(documents_to_evict(&held, &wanted, 150), vec![key(0, 2)]);
    }

    #[test]
    fn the_surface_cap_evicts_the_least_recently_wanted() {
        let held = vec![(key(0, 1), 10u64), (key(0, 2), 40), (key(0, 3), 20), (key(0, 4), 30)];
        let wanted = vec![key(0, 2)];
        assert!(
            surfaces_to_evict(&held, &wanted, 4).is_empty(),
            "a set at the cap evicts nothing"
        );
        assert!(surfaces_to_evict(&held, &wanted, 9).is_empty(), "…nor does one under it");

        assert_eq!(surfaces_to_evict(&held, &wanted, 3), vec![key(0, 1)], "the oldest goes");
        assert_eq!(
            surfaces_to_evict(&held, &wanted, 2),
            vec![key(0, 1), key(0, 3)],
            "then the next oldest, in order"
        );
        assert!(
            !surfaces_to_evict(&held, &wanted, 1).contains(&key(0, 2)),
            "the one being looked at survives to the last slot"
        );
    }

    /// The tie that a bare recency stamp cannot break: everything visible this frame was
    /// touched on the same frame, so the request order decides — and it decides in favour of
    /// the top of the page, which is what a reader scrolled *to*.
    #[test]
    fn a_tie_evicts_from_the_bottom_of_the_page_up() {
        let held =
            vec![(key(0, 1), 7u64), (key(0, 2), 7), (key(0, 3), 7), (key(0, 4), 7)];
        let wanted = vec![key(0, 1), key(0, 2), key(0, 3), key(0, 4)];
        assert_eq!(surfaces_to_evict(&held, &wanted, 2), vec![key(0, 4), key(0, 3)]);
    }

    /// Two conversation tabs both start at element 0. Keying on the id alone would have them
    /// painting into each other's textures; the pane half of the key is what stops that, and
    /// this is the test that fails if it is ever dropped.
    #[test]
    fn surfaces_in_two_panes_are_two_surfaces() {
        let held = vec![(key(0, 0), 1u64), (key(1, 0), 2)];
        assert_eq!(surfaces_to_evict(&held, &[key(1, 0)], 1), vec![key(0, 0)]);
        assert_ne!(key(0, 0), key(1, 0));
    }

    /// The cap's cost, as a number rather than a claim. A surface at the size this console
    /// actually draws one, times four, is the ceiling the eviction log quotes.
    #[test]
    fn the_surface_budget_is_four_textures_worth() {
        assert_eq!(surface_budget_bytes(100, 100), 4 * 100 * 100 * 4);
        // ORGANON-ONE's pane at 225 %: a full-width surface 260 pt tall.
        let measured = surface_budget_bytes(2475, 585);
        assert!(
            (22_000_000..24_000_000).contains(&measured),
            "the ~23 MB figure in MAX_SURFACE_TEXTURES' doc is now {measured}"
        );
    }

    /// **The knobs start where the shipped substrate is.** A surface must open looking
    /// exactly like `substrate_scene`'s plane, so that every drag reads as a departure from
    /// the console's own look rather than from an arbitrary midpoint.
    #[test]
    fn the_starting_knobs_reproduce_the_shipped_substrate() {
        let table = surface_slider_table();
        let base = look_shared(BackdropSource::Substrate, &ConsoleLook::default());
        let mut dressed = base.clone();
        for (label, value) in &table {
            apply_surface_slider(&mut dressed, label, *value);
        }
        // Approximately, not bit-for-bit: `elevation`'s starting value is a division the
        // knob then multiplies back out, and demanding an exact round trip through f32 would
        // be pinning the floating-point unit rather than the look.
        let close = |got: f32, want: f32, what: &str| {
            assert!((got - want).abs() < 1e-3, "{what}: {got} is not the shipped {want}");
        };
        close(dressed.lighting[4], base.lighting[4], "key azimuth");
        close(dressed.lighting[3], base.lighting[3], "key elevation");
        close(dressed.pbr[2], base.pbr[2], "exposure");
    }

    /// Each knob writes its own lane and nothing else, and an unknown label writes nothing —
    /// `console_step`'s forward-compatibility contract, one level down.
    #[test]
    fn a_knob_writes_one_lane_and_an_unknown_one_writes_none() {
        let base = look_shared(BackdropSource::Substrate, &ConsoleLook::default());

        let mut s = base.clone();
        apply_surface_slider(&mut s, "exposure", 1.0);
        assert_eq!(s.pbr[2], 3.0);
        assert_eq!(s.lighting[3], base.lighting[3], "exposure did not move the light");

        let mut s = base.clone();
        apply_surface_slider(&mut s, "elevation", 1.0);
        assert_eq!(s.lighting[3], 90.0);

        let mut s = base.clone();
        apply_surface_slider(&mut s, "bloom", 1.0);
        assert_eq!(bytemuck::bytes_of(&*s), bytemuck::bytes_of(&*base), "an unknown knob is inert");

        let mut s = base.clone();
        apply_surface_slider(&mut s, "exposure", f32::NAN);
        assert_eq!(s.pbr[2], base.pbr[2], "a non-finite value is refused, not written");
    }

    /// The light sweeps all the way round without ever leaving `params.rs`'s stated
    /// (−180, 180] — a wrap, not a clamp, because ±180 is a direction and not an error.
    #[test]
    fn the_light_knob_wraps_instead_of_piling_up_at_the_end() {
        for step in 0..=100 {
            let v = step as f32 / 100.0;
            let mut s = look_shared(BackdropSource::Substrate, &ConsoleLook::default());
            apply_surface_slider(&mut s, "light", v);
            let a = s.lighting[4];
            assert!(a > -180.0 && a <= 180.0, "azimuth {a} out of range at v={v}");
        }
        assert_eq!(wrap_degrees(SUBSTRATE_KEY_AZIMUTH_DEG), SUBSTRATE_KEY_AZIMUTH_DEG);
        assert_eq!(wrap_degrees(180.0), 180.0);
        assert_eq!(wrap_degrees(-180.0), 180.0, "the range is half-open at the bottom");
        assert_eq!(wrap_degrees(270.0), -90.0);
    }

    /// A surface's look is built the same way the backdrop's is — through `look_shared` —
    /// so a material button in a conversation and `organon console background <name>` cannot
    /// come to mean different pictures.
    #[test]
    fn a_surfaces_look_is_the_consoles_look_with_the_knobs_on_top() {
        let look = SurfaceLook {
            look: ConsoleLook { material: Some("metal".into()), rig: None },
            sliders: vec![("exposure".to_string(), 1.0)],
        };
        let s = surface_shared(&look);
        let console = look_shared(
            BackdropSource::Substrate,
            &ConsoleLook { material: Some("metal".into()), rig: None },
        );
        assert_eq!(s.pbr[0], console.pbr[0], "the material's metallic survives");
        assert_eq!(s.pbr[1], console.pbr[1], "…and its roughness");
        assert_eq!(s.pbr[2], 3.0, "…and the knob is applied over the top");
    }

    /// §5.9's second front-end is reached by a harness id, so `--help` has to name the
    /// id — a feature nobody can spell is a feature nobody has.
    #[test]
    fn help_names_the_conversation_tab() {
        let h = help_text();
        assert!(h.contains("claude-chat"), "the conversation harness id is the whole interface");
        assert!(
            harness::builtin().iter().any(|s| s.id == "claude-chat" && s.conversation),
            "…and it must be a real registry row, not just prose"
        );
    }
}

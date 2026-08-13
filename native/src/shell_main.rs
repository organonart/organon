//! The Organon Shell binary (Shell #10 T1 + #14 T1): a terminal, with the
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
//! because a backdrop is `Shell` state and nothing in the World can reach it (brief R3).
//! [`Shell::drain_console`] drains that file every frame on the same file-length
//! watermark the World uses for `cli.txt`, routes each op through the product's first
//! live [`organon_shell::command::CommandService`], and applies the survivors by
//! recomputing the published snapshot from one pure function of
//! `(source, material, rig)` — [`look_shared`]. No texture is recreated, no pane is
//! re-measured, so a live switch costs one frame and moves no glyph.
//!
//! **Console Spike Tier 5 puts a third verb on that lane, and it changes the transcript
//! rather than the dressing.** `organon console block <rows>` reserves a contiguous run of
//! blank rows in the active tab, by feeding `\r\n` bytes the console generated itself through
//! the terminal's own parser ([`Shell::open_block`]). They are ordinary scrollback rows, so
//! text written afterwards flows **below** them. Nothing is painted into them yet — the hole
//! is the increment.
//!
//! **`organon console patch --up N --rows M --kind <scene|panel>` is the corrected verb and
//! the one that carries something.** The writer prints the gap itself, through the ordinary
//! PTY, and then says where it is; the console records it ([`Shell::claim_patch`]) and paints
//! it. The kind selects the paint and **nothing before it** — the claim, the anchor
//! arithmetic and the per-pane ledger are shared — so `scene` samples the rendered substrate
//! through the rows and `panel` puts a live egui control panel in the same rect, whose
//! buttons re-enter this file at [`Shell::apply_console`], the same call a typed
//! `organon console background <name>` reaches.
//!
//! **Console Spike §5.9 forks the console into two front-ends over one renderer, and this
//! file hosts both.** The terminal host above is unchanged and is the universal fallback:
//! it runs any program and knows nothing about it. Beside it now sits the **conversation
//! view** — a tab that spawns no PTY at all, drives an agent over pipes, and renders its
//! structured event stream natively ([`Pane`], [`organon_shell::conversation_view`]). The
//! window, the tab strip, the command lane and the backdrop are shared; only what a tab
//! *is* differs. `SHELL_ARCHITECTURE.md` owns the shape.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use organic_math_native::agent;
use organic_math_native::cli;
use organic_math_native::params::OrganicMathParams;
use organic_math_native::scene_input;
use organic_math_native::substrate_camera::SubstrateRig;
use organic_math_native::substrate_epochs::{EpochId, EpochLedger, Look, SlotAction};
use organic_math_native::substrate_materials;
use organic_math_native::substrate_scene;
use organic_math_native::world::World;
use organon_core::edition::EDITION;
use organon_core::ipc;
use organon_shell::block_anchor::Block;
use organon_shell::block_panel::{BlockAction, BlockPanel, Patch};
use organon_shell::command::{
    ArgKind, ArgSpec, CommandError, CommandService, CommandSpec, CommandTarget, TargetKind,
};
use organon_shell::conversation::ElementId;
use organon_shell::conversation_view::{
    self, ArtifactAction, ConversationPane, SurfaceImages, SurfaceRequest,
};
use organon_shell::harness::{self, HarnessSpec};
use organon_shell::platform::Platform;
use organon_shell::session::{Issuer, SessionLog};
use organon_shell::tabs::{self, Tab, TabAction, TabStrip};
use organon_shell::term::{self, TermSession};
use organon_shell::term_view::{self, BandedBackdrop, PaneAnchor};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
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
/// "Organon Shell".
///
/// The artifact is `organon-console` and the public name is **Organon Console**, so a console
/// whose title bar and `--help` said "Organon Shell" would introduce itself as a product that
/// is not what you launched. Everything the rename does *not* touch is what something else
/// reads: `EDITION` is `organon-core`'s shared spine (the compositor lib's heading and the
/// edition tests quote it too), the crate is `organon-shell`, the feature is `shell-edition`,
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
/// [`Shell`] that a ledger would have to re-pair.
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
/// See [`CMD_BACKGROUND`]. Console Spike Tier 5: reserve rows in the transcript.
const CMD_BLOCK: &str = "console.block";
/// See [`CMD_BACKGROUND`]. Console Spike Tier 5, the **corrected** verb: claim a rectangle the
/// writer already left in its own output. The console records; it never writes.
const CMD_PATCH: &str = "console.patch";
/// The single argument the two **dressing** verbs take. One name, because the sidecar's wire
/// form is `<verb> <word>` and inventing two spellings for one slot is how a schema drifts
/// from its transport.
const CMD_ARG: &str = "name";
/// [`CMD_BLOCK`]'s argument. A **second** slot name rather than a reuse of [`CMD_ARG`],
/// because it is a different kind: `name` is a `Choice` over a table and `rows` is an `Int`,
/// and a palette showing "name: 12" would be describing the wrong thing.
const CMD_ROWS: &str = "rows";
/// [`CMD_PATCH`]'s other argument: how many lines above the current line the rectangle starts.
const CMD_UP: &str = "up";
/// [`CMD_PATCH`]'s third argument: what the console should draw in the rectangle. A `Choice`
/// rather than an `Int` or a free word, over `cli::PATCH_KIND_WORDS` — the same
/// table-not-a-restatement arrangement `background`'s materials use, so the palette, the
/// CLI's `--help` and this schema cannot come to know different kinds.
const CMD_KIND: &str = "kind";

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
                        cli::PATCH_KIND_WORDS.iter().map(|s| (*s).to_string()).collect(),
                    ),
                    required: true,
                },
            ],
        },
    ]
}

/// Catalog name ↔ sidecar op, both directions, in one place.
fn spec_name(op: &cli::ConsoleOp) -> &'static str {
    match op {
        cli::ConsoleOp::Background(_) => CMD_BACKGROUND,
        cli::ConsoleOp::Rig(_) => CMD_RIG,
        cli::ConsoleOp::Block(_) => CMD_BLOCK,
        cli::ConsoleOp::Patch { .. } => CMD_PATCH,
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
            let kind = args
                .get(CMD_KIND)
                .and_then(Value::as_str)
                .and_then(cli::PatchKind::from_word)
                .ok_or_else(|| {
                    format!("{name}: `{CMD_KIND}` must be one of {:?}", cli::PATCH_KIND_WORDS)
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
        cli::ConsoleOp::Background(n) | cli::ConsoleOp::Rig(n) => json!({ CMD_ARG: n }),
        cli::ConsoleOp::Block(rows) => json!({ CMD_ROWS: rows }),
        cli::ConsoleOp::Patch { up, rows, kind } => {
            json!({ CMD_UP: up, CMD_ROWS: rows, CMD_KIND: kind.as_word() })
        }
    }
}

/// Where a **validated** console command lands: a bank the drain reads after the service is
/// gone.
///
/// It banks rather than applies because of a real constraint, not a preference.
/// `CommandService::register_target` takes `Box<dyn CommandTarget>` — implicitly `'static` —
/// so a target cannot hold `&mut Shell`, and the apply needs one. `MockTarget`'s
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

/// One console op applied to the `(source, look)` pair — **pure**, so the entire command
/// vocabulary is a test rather than a claim about a `Shell` that needs a window server.
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
        // into and never reaches here: [`Shell::apply_console`] routes it first. `None` is
        // the honest answer for a resolver whose whole domain is looks — and it is also the
        // safe one, since `None` here means "changed nothing".
        cli::ConsoleOp::Block(_) | cli::ConsoleOp::Patch { .. } => return None,
    }
    Some((source, look))
}

/// The engine's frame behind the glyphs: sized to the **pane it is painted into** (not the
/// window — see [`Shell::render_backdrop`]), recreated when that size changes, rebound to the
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
/// stretches rather than mismatches. See [`Shell::snapshot_live_backdrop`] for why that is
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
fn surfaces_to_evict(
    held: &[(SurfaceKey, u64)],
    wanted: &[SurfaceKey],
    cap: usize,
) -> Vec<SurfaceKey> {
    if held.len() <= cap {
        return Vec::new();
    }
    let mut order: Vec<(SurfaceKey, u64, usize)> = held
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
/// cache lives here rather than on [`Shell`].
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

struct Shell {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    renderer: Option<egui_wgpu::Renderer>,
    /// One pane per tab, index-aligned with `strip.tabs`. ALL panes pump every frame
    /// (a background agent keeps streaming); only the active one draws. The 2026-08-08
    /// reframe (PRD v3.2): Shell is a TUI HOST — the default tab runs the default
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
    /// only into [`Shell::backdrop`], never the swapchain.
    world: World,
    backdrop: Option<Backdrop>,
    backdrop_source: BackdropSource,
    /// The substrate dressing the backdrop is currently wearing — **the pair Tier 4's epoch
    /// ledger records**. Changed only by [`Shell::apply_console`], read only by
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
    /// be opened — see [`Shell::dispatch_console`], which says exactly what is lost.
    session_log: Option<SessionLog>,
    /// The terminal pane's size in **points**, and the whole window's beside it, recorded at
    /// the end of each frame and consumed by the next frame's [`Shell::render_backdrop`].
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
    /// Whether the last [`Shell::render_backdrop`] recreated the backdrop texture — the
    /// pane changed size. Carried on `Shell` rather than returned because it is
    /// [`EpochLedger::plan`]'s `pane_resized` argument, consumed one call later; it is the
    /// existing `backdrop.size != (w, h)` condition and nothing new.
    pane_resized: bool,
    /// The `Shared` snapshot writer (organon-shell namespace). In the two-process
    /// design the PLUGIN writes this; Shell has no plugin, so the terminal writes
    /// the default look itself — which is what makes `organon status`/`get`/`watch`
    /// see a live system from inside the terminal, and gives the in-process world
    /// real params to read instead of zeroes. The CLI's override lane
    /// (`set`/`generator`/…) then applies on top in the world's working copy,
    /// exactly as it does against the standalone visual.
    shared_writer: Option<ipc::Writer>,
    shared: Box<ipc::Shared>,
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
    surface_pane: usize,
    /// Monotonic frame counter, used only as the cap's recency stamp.
    surface_clock: u64,
}

impl Shell {
    fn new() -> Self {
        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());
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
            world: World::new(),
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
            occluded: false,
            surfaces: HashMap::new(),
            surface_requests: Vec::new(),
            surface_pane: 0,
            surface_clock: 0,
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
                Some(organon_shell::platform::shell_dash_c(Platform::current(), &c, |k| {
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
            let cwd = spec.cwd.as_deref().map(|c| {
                organon_shell::platform::expand_tilde(
                    c,
                    organon_shell::platform::home_dir(Platform::current(), |k| {
                        std::env::var(k).ok()
                    })
                    .as_deref(),
                )
            });
            // The labels an inline panel offers, handed down for `claim_patch`'s reason:
            // `organon-shell` cannot see `substrate_materials` and must not learn to. It
            // draws them and says which was pressed; this file is the only place that knows
            // a `metal` button and `organon console background metal` are the same act.
            let pane = ConversationPane::new(
                cwd.as_deref(),
                substrate_materials::MATERIAL_NAMES.iter().map(|s| (*s).to_string()).collect(),
                // …and the knobs, for the same reason and from the same place: a slider label
                // means a lane in `Shared`, which is this file's knowledge and not the
                // compositor crate's. See `surface_slider_table`.
                surface_slider_table(),
            );
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
    /// Called once per frame from [`Shell::redraw`], immediately before the snapshot is
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
    /// **The service is built per batch, not held on `Shell`, and that is the shape
    /// `command.rs` asks for.** `CommandService<'log>` borrows `&'log mut SessionLog`; a
    /// `Shell` holding both would be self-referential and is not expressible in safe Rust.
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
    /// It is not a silent equivalent: `Shell::new` has already printed why on stderr, and the
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

    /// Reserve `rows` blank rows in the **active** pane's transcript (Console Spike Tier 5).
    ///
    /// The mechanism is [`organon_shell::term_view::PaneAnchor::feed_local`]: bytes the
    /// console generated itself, through the same parser the child's bytes go through, inside
    /// the same bracket that keeps the scroll anchor's `dropped` counter honest. The rows are
    /// ordinary scrollback rows — they scroll, age, evict and reflow like every other row —
    /// which is the whole reason for doing it this way rather than keeping a parallel list of
    /// "reserved" lines beside the buffer. Nothing is painted into them yet; this increment
    /// makes the hole, and a later one fills it.
    ///
    /// **The active pane, not every pane** — the opposite of [`Shell::record_look_change`],
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
    /// 🚨 **Why this exists and [`Shell::open_block`] is not enough.** `open_block` feeds
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
    /// [`cli::PatchKind::Panel`] is the one arm that carries state, and it is built here
    /// because the button labels are handed **down**: `organon-shell` cannot see
    /// `substrate_materials` and must not learn to. It draws the labels and reports which one
    /// was pressed; this file is the only place that knows a `metal` button and
    /// `organon console background metal` are the same act.
    fn claim_patch(&mut self, up: u16, rows: u16, kind: cli::PatchKind) {
        let pane = self.strip.active;
        // Terminal-only, for [`Shell::open_block`]'s reason.
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
            cli::PatchKind::Scene => Patch::scene(block),
            cli::PatchKind::Panel => Patch::panel(
                block,
                BlockPanel::new(
                    format!("◈ organon · patch @ line {first_abs} · {rows} rows"),
                    substrate_materials::MATERIAL_NAMES
                        .iter()
                        .map(|s| (*s).to_string())
                        .collect(),
                ),
            ),
        });
        eprintln!(
            "[patch] claimed {rows} rows @ line {first_abs} ({}, up {up}, pane {pane})",
            kind.as_word()
        );
    }

    /// Close the live look-epoch in every pane and open the next one — the Tier 4 half of
    /// [`Shell::apply_console`].
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
    /// last claim on it. The one release site; [`Shell::apply_epoch_plans`] is the belt.
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
    ///   backdrop, owned by [`Shell::render_backdrop`], which already recreates it on resize;
    /// * **`Create` for a closed epoch** — there is no picture and no way to make one: a
    ///   closed look is only ever captured by [`Shell::snapshot_live_backdrop`], at the
    ///   instant it closed. This fires for epochs that closed while the backdrop was off, and
    ///   for a tab whose history predates its ledger. Those bands paint nothing;
    /// * **`Rerender` for a closed epoch** — the pane resized; the picture is stale-sized and
    ///   gets stretched (see [`Shell::snapshot_live_backdrop`]);
    /// * **`Retain`** — nothing to do, which is the steady state.
    ///
    /// `Release` is the arm that does work, and it is a *belt*: evictions already release
    /// directly in [`Shell::record_look_change`], so anything reaching here is drift.
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
    fn render_source(&self) -> BackdropSource {
        if self.backdrop_source == BackdropSource::Off && self.patches_want_image() {
            BackdropSource::Substrate
        } else {
            self.backdrop_source
        }
    }

    fn render_backdrop(&mut self) -> Option<egui::TextureId> {
        // Nothing is re-sized on a frame that renders nothing; the flag is read by
        // [`Shell::apply_epoch_plans`] whether or not this function reached its own body.
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
    /// [`Shell::render_source`] already separates *what the engine draws* from *what the
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

    /// Drop one surface's texture and its egui registration, saying why.
    ///
    /// Unconditional logging, on [`Shell::retire_epochs`]' rule: a cap that silently drops a
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

    /// Free every surface texture. Used where a `SurfaceKey`'s pane index stops meaning what
    /// it meant — see the `Close` arm of [`Shell::apply`].
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
        // frame, not next. See [`Shell::drain_console`].
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
        let published = if self.render_source() == self.backdrop_source {
            *self.shared
        } else {
            *look_shared(self.render_source(), &self.console_look)
        };
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
        // see [`Shell::render_source`].
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
        // Buttons pressed inside a patch's panel this frame, collected out of the closure the
        // same way `action` is and for the same reason: applying one needs `&mut self`, and
        // `self` is split into disjoint borrows for the duration of `egui_ctx.run`.
        let mut block_actions: Vec<BlockAction> = Vec::new();
        // The same thing from the other front-end: a button pressed inside an *inline
        // artifact* in a conversation. A second vector rather than a shared one because the
        // two name their panel differently — a patch by its ledger index, an artifact by the
        // transcript's `ElementId` — and collapsing them would need one of the two to lie.
        // They converge one screen down, at the `apply_console` both of them call.
        let mut artifact_actions: Vec<ArtifactAction> = Vec::new();
        // …and where its rendered surfaces ended up, which is the NEXT frame's render list.
        // Collected out of the closure for the same reason: acting on it needs `&mut self`.
        let mut surface_requests: Vec<SurfaceRequest> = Vec::new();
        // The rect the terminal actually paints into, captured for the NEXT frame's backdrop
        // (see `render_backdrop`). Taken from the same `ui` and by the same call
        // `term_view::draw` sizes its grid from, so the texture and the quad cannot disagree.
        let mut pane_rect: Option<egui::Rect> = None;
        // …and the whole window beside it, in the SAME points, so the two divide into the
        // ratio `render_backdrop` applies to the swapchain. Read from the same frame as
        // `pane_rect` for exactly that reason: a ratio only cancels the scale if both halves
        // were measured under it.
        let mut window_rect: Option<egui::Rect> = None;
        let out = self.egui_ctx.run(raw, |ctx| {
            window_rect = Some(ctx.screen_rect());
            // ⌘-keys are the host's chrome (term_view skips them for the PTY).
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                        if action.is_none() {
                            action =
                                tabs::command_key_action(*key, *modifiers, strip, default_harness);
                        }
                    }
                }
            });
            // The tab strip: the one permitted chrome (FR-T11), Superconductor's
            // form factor — along the top, + menu with the numbered registry.
            egui::TopBottomPanel::top("tab-strip")
                .exact_height(30.0)
                .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(0x07, 0x09, 0x07)))
                .show(ctx, |ui| {
                    if let Some(a) = tabs::tab_bar(ui, strip, registry, installed, plus_open) {
                        action = Some(a);
                    }
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(term_view::DEFAULT_BG))
                .show(ctx, |ui| {
                    // Before anything is allocated in it — `term_view::draw`'s own first act
                    // is this same call.
                    pane_rect = Some(ui.available_rect_before_wrap());
                    // §5.9's fork, at the one place it shows: the same panel, the same
                    // window, two renderings. The terminal branch is what it was — Tier 5's
                    // patch ledger and the actions its panels return included; a conversation
                    // tab has neither because it has no transcript of terminal lines to claim
                    // a rectangle in.
                    match (sessions.get_mut(active), pane_looks.get_mut(active)) {
                        (Some(Pane::Term(session)), Some(pane)) => {
                            // `&mut pane.anchor` and `&mut pane.blocks` are disjoint fields of
                            // the same pane, which is exactly why the patch ledger lives on
                            // `PaneLooks` beside the anchor rather than in a parallel `Vec` on
                            // `Shell` that would have to be indexed in step with it. The ledger
                            // is `&mut` because a panel's sliders are real: a value dragged this
                            // frame has to still be there on the next one.
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
                            );
                        }
                        (Some(Pane::Conversation(chat)), _) => {
                            // No PTY, so no patch ledger and no block actions: `block_actions`
                            // stays the empty `Vec` it was initialised to and the loop below
                            // does nothing. An inline artifact needs none of that machinery —
                            // it is an element in a flow that draws itself — so what comes
                            // back is the buttons, and where its surfaces ended up.
                            let out = conversation_view::draw(ui, chat, &surface_images);
                            artifact_actions = out.actions;
                            surface_requests = out.surfaces;
                        }
                        _ => {
                            ui.centered_and_justified(|ui| {
                                ui.monospace("no live tab — ⌘T opens one");
                            });
                        }
                    }
                });
        });
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
        self.surface_pane = active;
        if let Some(action) = action {
            self.apply(action);
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
        // …and the conversation view's inline panel arrives at the same call, which is the
        // point of routing it rather than giving the second front-end its own effects.
        //
        // ⚠️ **`/panel`'s buttons still land on a backdrop you cannot see from here**, and
        // that is now a choice rather than the only option: the backdrop is not drawn behind
        // a conversation (banding is scrollback arithmetic and there is no scrollback), so a
        // material clicked in a `/panel` is seen on a terminal tab. `/surface` is the answer
        // to that — its panel drives an element in this same transcript and its buttons never
        // reach this loop at all, having been consumed by the surface they aim at.
        for act in artifact_actions {
            self.apply_console(&cli::ConsoleOp::Background(act.button));
        }
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

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(PRODUCT_NAME)
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0));
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
/// [`organon_shell::platform::Platform`] is a value rather than a `#[cfg]`.
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
             ORGANON_SHELL_SCRIM=<0..255> legibility scrim alpha (default {scrim_default}, floor {scrim_floor})\n    \
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
         In a conversation tab the composer takes two local commands, never sent to the\n\
         agent:\n    \
             /surface   a rendered surface, with the controls that drive it beneath it\n    \
             /panel     a control panel that drives this console's backdrop instead\n\
         \n\
         Inside a tab the `organon` CLI addresses this process — the namespace is inherited:\n    \
             organon console background <{backgrounds}>\n    \
             organon console rig <{rigs}>\n\
         \n\
         Docs: SHELL_ARCHITECTURE.md\n",
        substrate = BACKDROP_SUBSTRATE,
        scrim_default = term_view::SCRIM_DEFAULT,
        scrim_floor = term_view::SCRIM_FLOOR,
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
    let mut shell = Shell::new();
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
    #[test]
    fn the_scrim_line_matches_the_code_it_documents() {
        let h = help_text();
        assert!(h.contains("ORGANON_SHELL_SCRIM=<0..255>"), "scrim scale is a u8, not 0..1");
        assert!(!h.contains("<0..1>"), "the 0..1 form silently no-ops — never document it");
        assert!(
            h.contains(&format!("default {}", organon_shell::term_view::SCRIM_DEFAULT)),
            "help does not quote SCRIM_DEFAULT"
        );
        assert!(
            h.contains(&format!("floor {}", organon_shell::term_view::SCRIM_FLOOR)),
            "help does not quote SCRIM_FLOOR"
        );
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

    /// **The binary introduces itself as what you launched.** `--help`'s header and usage line
    /// are the console's front door, and they are the one place the rename is *visible*: the
    /// artifact is `organon-console`, so a header reading "Organon Shell" would name a product
    /// that is not what ran. This is a real regression risk rather than a hypothetical —
    /// [`PRODUCT_NAME`] deliberately shadows `EDITION.product_name()`, which still answers
    /// "Organon Shell" and will keep tempting a future tidy-up back onto it.
    ///
    /// ⚠️ The **variable names** below are the opposite case: `ORGANON_SHELL_*` is a shipped
    /// flag surface and stays. Presentation renames, identifiers do not — that split is the
    /// whole content of this change.
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
        for kind in [cli::PatchKind::Scene, cli::PatchKind::Panel] {
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
            cli::PATCH_KIND_WORDS,
            "the catalog and the CLI are built from one table"
        );
        for word in offered {
            let kind = cli::PatchKind::from_word(word)
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
            cli::ConsoleOp::Patch { up: 0, rows: 7, kind: cli::PatchKind::Scene },
            cli::ConsoleOp::Patch { up: 12, rows: 12, kind: cli::PatchKind::Panel },
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
            let state = organon_shell::scroll_anchor::ViewState {
                rows: 24,
                display_offset: 0,
                history,
                dropped: 0,
                alt_screen: false,
            };
            let (s, l) = console_step(source, &dressing, &bg(name)).expect("a known material");
            source = s;
            dressing = l;
            let at = organon_shell::scroll_anchor::boundary_now(state, 5);
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

    /// **The seam between the two leaves, row by row.** [`Shell::band_table`] hands
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
            Shell::band_table(&ledger, Some(live), |id| cached.get(&id).copied());
        assert_eq!(
            boundaries,
            vec![105, 112],
            "the ledger records where the OLDEST epoch opened too; that is not a look CHANGE"
        );
        assert_eq!(textures.len(), boundaries.len() + 1, "the length law bands are indexed by");
        assert_eq!(textures.last().copied().flatten(), Some(live), "the live look is last");

        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        let cell_h = 10.0;
        let state = organon_shell::scroll_anchor::ViewState {
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
    /// [`Shell::snapshot_live_backdrop`] copies the live backdrop at `Backdrop::size`, so the
    /// epoch's resolution is whatever [`Shell::render_backdrop`] decided — and that decision is
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
            Shell::band_table(&ledger, Some(egui::TextureId::User(1)), |_| None);
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
